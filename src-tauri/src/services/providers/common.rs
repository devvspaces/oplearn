//! Shared provider plumbing: HTML sanitising, local catalogue search, polite HTTP, file and
//! image downloads, moving an import into the course's media folder, and PDF indexing.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use regex::Regex;
use tauri::ipc::Channel;
use tokio::io::AsyncWriteExt;

use super::ImportProgress;
use crate::db::repo::materials;
use crate::error::{AppError, AppResult};
use crate::services::pdf_text;
use crate::state::AppState;

/// Catalogues are cached this long (`yt_search_cache`, key `catalog:<provider>`).
pub const CATALOG_TTL_HOURS: i64 = 24;
/// Search results shown at most.
pub const MAX_RESULTS: usize = 60;
/// Images larger than this are left out of a page.
pub const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

/// Scheme for images stored with a course; resolved to the loopback URL when a page is served
/// (the loopback token changes every launch). Paths are relative to the media folder.
pub const MEDIA_SCHEME: &str = "lectern-media:";

// ---------------------------------------------------------------- HTML

const MATH_TAGS: &[&str] = &[
    "math",
    "semantics",
    "mrow",
    "mi",
    "mn",
    "mo",
    "ms",
    "mtext",
    "mspace",
    "msup",
    "msub",
    "msubsup",
    "mfrac",
    "msqrt",
    "mroot",
    "mstyle",
    "mpadded",
    "mphantom",
    "menclose",
    "mfenced",
    "mover",
    "munder",
    "munderover",
    "mtable",
    "mtr",
    "mtd",
    "mlabeledtr",
    "mmultiscripts",
    "mprescripts",
    "none",
    "merror",
    "maction",
];
const MATH_ATTRS: &[&str] = &[
    "display",
    "mathvariant",
    "displaystyle",
    "scriptlevel",
    "stretchy",
    "fence",
    "separator",
    "lspace",
    "rspace",
    "linethickness",
    "accent",
    "accentunder",
    "columnalign",
    "rowalign",
    "columnspan",
    "rowspacing",
    "columnspacing",
    "open",
    "close",
    "separators",
    "notation",
    "width",
    "height",
    "depth",
    "largeop",
    "movablelimits",
    "symmetric",
    "minsize",
    "maxsize",
    "form",
    "bevelled",
    "frame",
    "align",
    "equalrows",
    "equalcolumns",
];
const EXTRA_TAGS: &[&str] = &[
    "figure",
    "figcaption",
    "section",
    "aside",
    "details",
    "summary",
    "caption",
    "colgroup",
    "col",
    "mark",
    "small",
    "abbr",
    "cite",
    "dfn",
    "kbd",
    "samp",
    "var",
    "time",
];

fn sanitizer() -> ammonia::Builder<'static> {
    let mut b = ammonia::Builder::default();
    b.add_tags(MATH_TAGS)
        .add_tags(EXTRA_TAGS)
        .add_generic_attributes(["id", "colspan", "rowspan", "scope", "dir"])
        .add_tag_attributes("img", ["width", "height"])
        .add_tag_attributes("a", ["name"])
        .add_tag_attributes("ol", ["start", "type"]);
    for t in MATH_TAGS {
        b.add_tag_attributes(t, MATH_ATTRS);
    }
    // Content MathML duplicates the presentation markup; drop it with its contents.
    b.add_clean_content_tags(["annotation-xml", "annotation", "head", "title", "noscript"]);
    b
}

/// Allow-list sanitiser for provider HTML (text, tables, figures, images, MathML). Scripts,
/// styles, event handlers and unknown schemes are removed. Relative URLs are made absolute
/// against `base`; `#fragment` links are kept as they are (in-app links like `#material:<id>`).
pub fn sanitize_html(html: &str, base: Option<&str>) -> String {
    let base = base.and_then(|b| url::Url::parse(b).ok());
    let mut b = sanitizer();
    b.url_relative(ammonia::UrlRelative::Custom(Box::new(Absolute(base))));
    b.clean(html).to_string()
}

struct Absolute(Option<url::Url>);

impl<'a> ammonia::UrlRelativeEvaluate<'a> for Absolute {
    fn evaluate<'url>(&self, u: &'url str) -> Option<Cow<'url, str>> {
        if u.starts_with('#') {
            return Some(Cow::Borrowed(u));
        }
        self.0
            .as_ref()?
            .join(u)
            .ok()
            .map(|x| Cow::Owned(x.to_string()))
    }
}

/// Readable text of sanitised HTML (for search and AI context).
pub fn text_of(html: &str) -> String {
    crate::services::ai_context::html_to_text(html)
}

static IMG_SRC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<img\b[^>]*?\ssrc="([^"]+)"[^>]*>"#).unwrap());

/// Remote image URLs in sanitised HTML, in order, without duplicates.
pub fn image_urls(html: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    IMG_SRC
        .captures_iter(html)
        .map(|c| decode_attr(&c[1]))
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        .filter(|u| seen.insert(u.clone()))
        .collect()
}

fn decode_attr(s: &str) -> String {
    s.replace("&amp;", "&")
}

/// Points images at stored copies (`map`: remote URL → path relative to the import folder);
/// images that weren't stored are replaced by their alt text, so no remote request is made.
pub fn rewrite_images(html: &str, map: &HashMap<String, String>) -> String {
    static ALT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\salt="([^"]*)""#).unwrap());
    IMG_SRC
        .replace_all(html, |c: &regex::Captures<'_>| {
            let tag = &c[0];
            match map.get(&decode_attr(&c[1])) {
                Some(rel) => tag.replacen(&c[1], &format!("{MEDIA_SCHEME}{rel}"), 1),
                None => ALT
                    .captures(tag)
                    .map(|a| format!("<span>[{}]</span>", &a[1]))
                    .filter(|s| s != "<span>[]</span>")
                    .unwrap_or_default(),
            }
        })
        .into_owned()
}

/// Serve-time: `lectern-media:<rel>` → the current loopback URL for `<media>/<rel>`.
pub fn resolve_media(
    html: &str,
    media_dir: &Path,
    url_of: impl Fn(&Path) -> Option<String>,
) -> String {
    static REF: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"lectern-media:([A-Za-z0-9._/\-]+)"#).unwrap());
    if !html.contains(MEDIA_SCHEME) {
        return html.to_string();
    }
    REF.replace_all(html, |c: &regex::Captures<'_>| {
        let rel = &c[1];
        if rel.split('/').any(|p| p == ".." || p.is_empty()) {
            return String::new();
        }
        url_of(&media_dir.join(rel)).unwrap_or_default()
    })
    .into_owned()
}

// ---------------------------------------------------------------- search

fn norm(s: &str) -> String {
    s.to_lowercase()
}

/// Local catalogue search: every query token must appear somewhere; title matches rank first,
/// then earlier matches. An empty query returns the catalogue in its own order.
pub fn rank<T: Clone>(query: &str, items: &[T], fields: impl Fn(&T) -> (String, String)) -> Vec<T> {
    let tokens: Vec<String> = query.split_whitespace().map(norm).collect();
    if tokens.is_empty() {
        return items.iter().take(MAX_RESULTS).cloned().collect();
    }
    let mut scored: Vec<(i64, usize, &T)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| {
            let (title, rest) = fields(it);
            let (t, r) = (norm(&title), norm(&rest));
            let mut score = 0i64;
            for tok in &tokens {
                if let Some(p) = t.find(tok.as_str()) {
                    score += 100 - (p as i64).min(50);
                    if t.split(|c: char| !c.is_alphanumeric()).any(|w| w == tok) {
                        score += 20;
                    }
                } else if r.contains(tok.as_str()) {
                    score += 10;
                } else {
                    return None;
                }
            }
            if t == tokens.join(" ") {
                score += 500;
            }
            Some((score, i, it))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .take(MAX_RESULTS)
        .map(|(_, _, it)| it.clone())
        .collect()
}

/// Cached catalogue: returns the stored copy if fresh, otherwise fetches and stores it.
pub async fn catalogue<T, F, Fut>(state: &AppState, key: &str, fetch: F) -> AppResult<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<T>>,
{
    use crate::db::repo::yt_search;
    let now = chrono::Utc::now();
    if let Some(v) = yt_search::cached_json::<T>(&*state.db()?, key, now)? {
        return Ok(v);
    }
    let v = fetch().await?;
    yt_search::store_json(&*state.db()?, key, &v, now)?;
    Ok(v)
}

// ---------------------------------------------------------------- HTTP

/// A shared HTTP client that keeps a minimum gap between requests to one provider.
pub struct Polite {
    pub http: reqwest::Client,
    gap: Duration,
    next: tokio::sync::Mutex<Instant>,
}

impl Polite {
    pub fn new(http: &reqwest::Client, gap: Duration) -> Arc<Self> {
        Arc::new(Self {
            http: http.clone(),
            gap,
            next: tokio::sync::Mutex::new(Instant::now()),
        })
    }

    async fn turn(&self) {
        let mut next = self.next.lock().await;
        let now = Instant::now();
        if *next > now {
            tokio::time::sleep(*next - now).await;
        }
        *next = Instant::now() + self.gap;
    }

    pub async fn get(&self, url: &str) -> AppResult<reqwest::Response> {
        self.turn().await;
        let r = self.http.get(url).send().await?;
        if r.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::internal(
                "the provider is rate-limiting requests; try again in a few minutes",
            ));
        }
        r.error_for_status()
            .map_err(|e| AppError::internal(format!("request failed: {e}")))
    }

    pub async fn text(&self, url: &str) -> AppResult<String> {
        Ok(self.get(url).await?.text().await?)
    }

    pub async fn json<T: serde::de::DeserializeOwned>(&self, url: &str) -> AppResult<T> {
        let body = self.get(url).await?.text().await?;
        serde_json::from_str(&body)
            .map_err(|e| AppError::internal(format!("unexpected reply from {url}: {e}")))
    }

    /// Streams a file to `dest` (max `limit` bytes), reporting (done, total) as it goes.
    pub async fn download(
        &self,
        url: &str,
        dest: &Path,
        limit: u64,
        on: &(dyn Fn(f64, f64) + Send + Sync),
    ) -> AppResult<u64> {
        self.turn().await;
        let mut resp = self
            .http
            .get(url)
            .timeout(Duration::from_secs(60 * 60))
            .send()
            .await?
            .error_for_status()
            .map_err(|e| AppError::internal(format!("download failed: {e}")))?;
        let total = resp.content_length().unwrap_or(0);
        if total > limit {
            return Err(AppError::invalid("file is too large"));
        }
        if let Some(p) = dest.parent() {
            tokio::fs::create_dir_all(p).await?;
        }
        let part = dest.with_extension("part");
        let mut f = tokio::fs::File::create(&part).await?;
        let mut done: u64 = 0;
        let mut last = Instant::now();
        while let Some(chunk) = resp.chunk().await? {
            done += chunk.len() as u64;
            if done > limit {
                drop(f);
                let _ = tokio::fs::remove_file(&part).await;
                return Err(AppError::invalid("file is too large"));
            }
            f.write_all(&chunk).await?;
            if last.elapsed() > Duration::from_millis(250) {
                on(done as f64, total as f64);
                last = Instant::now();
            }
        }
        f.flush().await?;
        drop(f);
        if total > 0 && done != total {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(AppError::internal("the download was incomplete; try again"));
        }
        tokio::fs::rename(&part, dest).await?;
        Ok(done)
    }

    /// Content-Length of a URL (following redirects), if the server says.
    pub async fn size(&self, url: &str) -> Option<f64> {
        self.turn().await;
        let r = self.http.head(url).send().await.ok()?;
        r.status()
            .is_success()
            .then(|| r.content_length())?
            .map(|n| n as f64)
    }
}

pub fn progress(
    ch: &Channel<ImportProgress>,
    stage: &str,
    done: f64,
    total: f64,
    msg: impl Into<String>,
) {
    let _ = ch.send(ImportProgress {
        stage: stage.into(),
        done,
        total,
        message: msg.into(),
    });
}

/// A progress callback usable from provider code (tests pass a no-op).
pub type OnProgress<'a> = &'a (dyn Fn(&str, f64, f64, &str) + Send + Sync);

pub fn no_progress() -> impl Fn(&str, f64, f64, &str) + Send + Sync {
    |_, _, _, _| {}
}

/// File stem for a stored image: a hash of its URL.
fn image_stem(url: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(url.as_bytes())
        .iter()
        .take(10)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Image extension from a Content-Type (only types the media server serves as images).
fn image_ext(content_type: &str) -> Option<&'static str> {
    match content_type.split(';').next()?.trim() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/svg+xml" => Some("svg"),
        _ => None,
    }
}

async fn fetch_image(polite: &Polite, url: &str, root: &Path) -> AppResult<String> {
    let mut resp = polite.get(url).await?;
    let ext = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(image_ext)
        .ok_or_else(|| AppError::invalid("not an image"))?;
    if resp.content_length().unwrap_or(0) > MAX_IMAGE_BYTES {
        return Err(AppError::invalid("image too large"));
    }
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        buf.extend_from_slice(&chunk);
        if buf.len() as u64 > MAX_IMAGE_BYTES {
            return Err(AppError::invalid("image too large"));
        }
    }
    let rel = format!("img/{}.{ext}", image_stem(url));
    let dest = root.join(&rel);
    if let Some(p) = dest.parent() {
        tokio::fs::create_dir_all(p).await?;
    }
    tokio::fs::write(&dest, &buf).await?;
    Ok(rel)
}

/// Downloads every image in `urls` into `<root>/img/`, 4 at a time. Returns the map of remote
/// URL → path relative to `root` for `rewrite_images`. Failed, oversized or non-image
/// responses are skipped (they become alt text).
pub async fn fetch_images(
    polite: &Arc<Polite>,
    urls: Vec<String>,
    root: &Path,
    on: OnProgress<'_>,
) -> HashMap<String, String> {
    let total = urls.len() as f64;
    let gate = Arc::new(tokio::sync::Semaphore::new(4));
    let mut jobs = tokio::task::JoinSet::new();
    for u in urls {
        let (gate, polite, root) = (gate.clone(), polite.clone(), root.to_path_buf());
        jobs.spawn(async move {
            let _slot = gate.acquire_owned().await;
            match fetch_image(&polite, &u, &root).await {
                Ok(rel) => Some((u, rel)),
                Err(e) => {
                    log::warn!("image skipped {u}: {e}");
                    None
                }
            }
        });
    }
    let mut map = HashMap::new();
    let mut n = 0.0;
    while let Some(j) = jobs.join_next().await {
        n += 1.0;
        if let Ok(Some((u, rel))) = j {
            map.insert(u, rel);
        }
        if (n as usize).is_multiple_of(5) || n == total {
            on("download", n, total, "Downloading images…");
        }
    }
    map
}

// ---------------------------------------------------------------- after the build

/// Moves an import folder to `media/<course slug>/<sub>` and fixes stored paths: material
/// files, the cover, and `lectern-media:` image references (relative to the media folder).
pub async fn relocate(
    state: &AppState,
    course_id: &str,
    staging: &Path,
    sub: &str,
) -> AppResult<PathBuf> {
    let slug: String =
        state
            .db()?
            .query_row("SELECT slug FROM courses WHERE id = ?1", [course_id], |r| {
                r.get(0)
            })?;
    let final_dir = state.paths.media_dir().join(&slug).join(sub);
    if let Some(p) = final_dir.parent() {
        tokio::fs::create_dir_all(p).await?;
    }
    let _ = tokio::fs::remove_dir_all(&final_dir).await;
    if staging.is_dir() {
        tokio::fs::rename(staging, &final_dir).await?;
    } else {
        tokio::fs::create_dir_all(&final_dir).await?;
    }
    let (from, to) = (
        format!("{}{}", staging.to_string_lossy(), std::path::MAIN_SEPARATOR),
        format!(
            "{}{}",
            final_dir.to_string_lossy(),
            std::path::MAIN_SEPARATOR
        ),
    );
    let c = state.db()?;
    c.execute(
        "UPDATE materials SET local_path = replace(local_path, ?2, ?3) WHERE course_id = ?1 AND local_path IS NOT NULL",
        rusqlite::params![course_id, from, to],
    )?;
    c.execute(
        "UPDATE courses SET cover_path = replace(cover_path, ?2, ?3) WHERE id = ?1 AND cover_path IS NOT NULL",
        rusqlite::params![course_id, from, to],
    )?;
    c.execute(
        "UPDATE materials SET html = replace(html, ?2, ?3) WHERE course_id = ?1 AND html LIKE '%lectern-media:%'",
        rusqlite::params![course_id, MEDIA_SCHEME, format!("{MEDIA_SCHEME}{slug}/{sub}/")],
    )?;
    Ok(final_dir)
}

/// Extracts PDF text (search + AI) in helper processes, 3 at a time; returns how many worked.
pub async fn index_pdfs(
    state: &AppState,
    pdfs: Vec<(String, PathBuf)>,
    ch: &Channel<ImportProgress>,
) -> AppResult<i32> {
    let total = pdfs.len() as f64;
    if pdfs.is_empty() {
        return Ok(0);
    }
    progress(ch, "index", 0.0, total, "Indexing PDFs for search and AI…");
    // Each PDF is read by a helper process with a time limit, so a bad file is only skipped.
    let exe = std::env::current_exe()
        .map_err(|e| AppError::internal(format!("couldn't locate the app: {e}")))?;
    let gate = Arc::new(tokio::sync::Semaphore::new(pdf_text::CONCURRENCY));
    let mut jobs = tokio::task::JoinSet::new();
    for (id, path) in pdfs {
        let (gate, exe) = (gate.clone(), exe.clone());
        jobs.spawn(async move {
            let _slot = gate.acquire_owned().await;
            let pages = pdf_text::extract(&exe, &path, pdf_text::TIMEOUT).await;
            (id, pages)
        });
    }
    let (mut finished, mut indexed) = (0.0, 0);
    while let Some(job) = jobs.join_next().await {
        finished += 1.0;
        if let Ok((id, Some(pages))) = job
            && materials::set_pages(&*state.db()?, &id, &pages).is_ok()
        {
            indexed += 1;
        }
        progress(
            ch,
            "index",
            finished,
            total,
            "Indexing PDFs for search and AI…",
        );
    }
    Ok(indexed)
}

/// `<media>/.incoming/<name>`, emptied first.
pub async fn staging_dir(state: &AppState, name: &str) -> AppResult<PathBuf> {
    let dir = state.paths.media_dir().join(".incoming").join(name);
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir).await?;
    Ok(dir)
}

/// Safe file name from a title (for downloaded PDFs).
pub fn file_name(title: &str, ext: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim_matches('_');
    let s: String = s.chars().take(80).collect();
    format!("{}.{ext}", if s.is_empty() { "file" } else { &s })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizer_strips_active_content_and_keeps_math() {
        let html = r##"<html><head><title>x</title><style>p{}</style></head><body>
            <script>alert(1)</script><p onclick="x()">Hi <a href="javascript:alert(1)">j</a>
            <a href="../other/page">rel</a> <a href="#material:abc">m</a></p>
            <img src="../resources/abc" alt="Graph"/>
            <math display="block"><semantics><mrow><mi>x</mi><mo>=</mo><mn>2</mn></mrow>
            <annotation-xml encoding="MathML-Content"><ci>x</ci></annotation-xml></semantics></math>
            <iframe src="https://evil"></iframe><form><input></form></body></html>"##;
        let out = sanitize_html(
            html,
            Some("https://openstax.org/apps/archive/1/contents/book.json"),
        );
        assert!(!out.contains("script") && !out.contains("alert") && !out.contains("onclick"));
        assert!(!out.contains("<style") && !out.contains("p{}") && !out.contains("<title"));
        assert!(!out.contains("iframe") && !out.contains("<input"));
        assert!(
            out.contains(r#"href="https://openstax.org/apps/archive/1/other/page""#),
            "{out}"
        );
        assert!(out.contains(r##"href="#material:abc""##), "{out}");
        assert!(
            out.contains(r#"src="https://openstax.org/apps/archive/1/resources/abc""#),
            "{out}"
        );
        assert!(
            out.contains("<math") && out.contains("<mi>x</mi>") && out.contains("<mn>2</mn>"),
            "{out}"
        );
        assert!(out.contains(r#"display="block""#), "{out}");
        assert!(
            !out.contains("annotation") && !out.contains("<ci>"),
            "{out}"
        );
    }

    #[test]
    fn images_are_listed_rewritten_and_resolved() {
        let html = r#"<p><img src="https://x.org/a.png" alt="A"><img src="https://x.org/b.jpg?x=1&amp;y=2" alt="B"><img src="https://x.org/a.png" alt="A again"><img src="https://x.org/c.png" alt=""></p>"#;
        let urls = image_urls(html);
        assert_eq!(
            urls,
            vec![
                "https://x.org/a.png",
                "https://x.org/b.jpg?x=1&y=2",
                "https://x.org/c.png"
            ]
        );
        let map = HashMap::from([("https://x.org/a.png".to_string(), "img/a.png".to_string())]);
        let out = rewrite_images(html, &map);
        assert_eq!(out.matches("lectern-media:img/a.png").count(), 2);
        assert!(
            out.contains("<span>[B]</span>")
                && !out.contains("x.org/b")
                && !out.contains("x.org/c")
        );
        let served = resolve_media(
            "<img src=\"lectern-media:c/openstax/img/a.png\"><img src=\"lectern-media:../x\">",
            Path::new("/m"),
            |p| {
                Some(format!(
                    "http://127.0.0.1:1/media/t/{}",
                    p.strip_prefix("/m").unwrap().display()
                ))
            },
        );
        assert_eq!(
            served,
            "<img src=\"http://127.0.0.1:1/media/t/c/openstax/img/a.png\"><img src=\"\">"
        );
    }

    #[test]
    fn rank_requires_all_tokens_and_prefers_titles() {
        let items = vec![
            ("Algorithms in Python".to_string(), "".to_string()),
            ("Intro to Python".to_string(), "data structures".to_string()),
            ("Data Structures and Algorithms".to_string(), "".to_string()),
            ("Chemistry".to_string(), "".to_string()),
        ];
        let r = rank("data structures", &items, |x| x.clone());
        assert_eq!(r[0].0, "Data Structures and Algorithms");
        assert_eq!(r[1].0, "Intro to Python");
        assert_eq!(r.len(), 2);
        assert_eq!(rank("", &items, |x| x.clone()).len(), 4);
        assert!(rank("physics", &items, |x| x.clone()).is_empty());
    }

    #[test]
    fn image_names_are_stable_and_typed() {
        assert_eq!(
            image_stem("https://a/b/c.png"),
            image_stem("https://a/b/c.png")
        );
        assert_ne!(
            image_stem("https://a/b/c.png"),
            image_stem("https://a/b/d.png")
        );
        assert_eq!(image_ext("image/jpeg; charset=x"), Some("jpg"));
        assert_eq!(image_ext("text/html"), None);
        assert_eq!(
            file_name("1.1: Intro / Basics?", "pdf"),
            "1_1__Intro___Basics.pdf"
        );
    }
}
