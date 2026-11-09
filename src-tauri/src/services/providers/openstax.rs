//! OpenStax: free, peer-reviewed textbooks. The catalogue comes from the CMS API; each book's
//! chapter tree and section pages come from the archive JSON that openstax.org's own reader
//! uses (`rex/release.json` names the current archive and book version).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::build::{LessonBody, Plan, PlanLesson, PlanMaterial, PlanModule, PlanPage};
use super::common::{self, OnProgress, Polite};
use super::{
    Fact, OutlineModule, Provider, ProviderCard, ProviderCounts, ProviderMeta, ProviderPreview,
};
use crate::error::{AppError, AppResult};

pub const BASE: &str = "https://openstax.org";
pub const CATALOG_URL: &str = "https://openstax.org/apps/cms/api/v2/pages/?type=books.Book&fields=title,slug,cnx_id,book_state,license_name,license_url,high_resolution_pdf_url,cover_url,description,book_subjects,authors&limit=250";
/// Static files behind a CDN; images make up most requests.
pub const GAP: Duration = Duration::from_millis(50);
pub const MAX_PDF_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Book {
    pub slug: String,
    pub uuid: String,
    pub title: String,
    pub state: String,
    pub license: Option<String>,
    pub license_url: Option<String>,
    pub pdf_url: Option<String>,
    pub cover_url: Option<String>,
    pub description: Option<String>,
    pub subjects: Vec<String>,
    pub authors: Vec<String>,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
}

/// CMS catalogue → books that are still offered (retired, deprecated and unlisted are hidden).
pub fn parse_catalog(v: &Value) -> AppResult<Vec<Book>> {
    let items = v
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::internal("OpenStax changed its catalogue format"))?;
    let mut books: Vec<Book> = items
        .iter()
        .filter_map(|b| {
            let state = s(b, "book_state")?;
            if !matches!(state.as_str(), "live" | "new_edition_available") {
                return None;
            }
            Some(Book {
                slug: b.get("meta").and_then(|m| s(m, "slug"))?,
                uuid: s(b, "cnx_id")?,
                title: s(b, "title")?,
                state,
                license: s(b, "license_name"),
                license_url: s(b, "license_url"),
                pdf_url: s(b, "high_resolution_pdf_url"),
                cover_url: s(b, "cover_url"),
                description: s(b, "description").map(|d| common::text_of(&d)),
                subjects: b
                    .get("book_subjects")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|x| s(x, "subject_name")).collect())
                    .unwrap_or_default(),
                authors: b
                    .get("authors")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter(|x| {
                                x.pointer("/value/senior_author")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(true)
                            })
                            .filter_map(|x| x.get("value").and_then(|v| s(v, "name")))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect();
    books.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(books)
}

/// "Creative Commons Attribution-NonCommercial-ShareAlike License" → "CC BY-NC-SA 4.0".
pub fn short_license(name: &str, url: Option<&str>) -> String {
    static VER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"/licenses/([a-z-]+)/(\d\.\d)").unwrap());
    if let Some(c) = url.and_then(|u| VER.captures(u)) {
        return format!("CC {} {}", c[1].to_uppercase(), &c[2]);
    }
    name.to_string()
}

pub fn card(b: &Book) -> ProviderCard {
    ProviderCard {
        id: b.slug.clone(),
        title: b.title.clone(),
        subtitle: Some(b.authors.join(", ")).filter(|x| !x.is_empty()),
        description: b.description.clone(),
        image_url: b.cover_url.clone(),
        license: b
            .license
            .as_deref()
            .map(|l| short_license(l, b.license_url.as_deref())),
        license_url: b.license_url.clone(),
        url: format!("{BASE}/details/books/{}", b.slug),
        tags: b.subjects.clone(),
    }
}

pub async fn catalog(polite: &Polite) -> AppResult<Vec<Book>> {
    parse_catalog(&polite.json::<Value>(CATALOG_URL).await?)
}

/// Archive location and current version of a book.
pub fn parse_release(v: &Value, uuid: &str) -> AppResult<(String, String)> {
    let archive = s(v, "archiveUrl")
        .ok_or_else(|| AppError::internal("OpenStax changed its release format"))?;
    let ver = v
        .pointer(&format!("/books/{uuid}/defaultVersion"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::invalid("this OpenStax book isn't available in the online reader")
        })?;
    let archive = if archive.starts_with("http") {
        archive
    } else {
        format!("{BASE}{archive}")
    };
    Ok((archive, ver.to_string()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub page_id: String,
    pub slug: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub title: String,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tree {
    pub title: String,
    /// Front matter shown in About (Preface).
    pub front: Vec<Section>,
    pub chapters: Vec<Chapter>,
}

fn clean_title(t: &str) -> String {
    common::text_of(t)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn section_of(n: &Value) -> Option<Section> {
    let id = s(n, "id")?;
    Some(Section {
        page_id: id.split('@').next()?.to_string(),
        slug: s(n, "slug").unwrap_or_default(),
        title: clean_title(&s(n, "title")?),
    })
}

fn is_leaf(n: &Value) -> bool {
    n.get("contents")
        .and_then(Value::as_array)
        .is_none_or(|c| c.is_empty())
}

/// Leaves under `n`, with group titles prefixed ("Chapter Review: Key Terms").
fn leaves(n: &Value, prefix: &str, out: &mut Vec<Section>) {
    for c in n
        .get("contents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if is_leaf(c) {
            if let Some(mut sec) = section_of(c) {
                if !prefix.is_empty() {
                    sec.title = format!("{prefix}: {}", sec.title);
                }
                out.push(sec);
            }
        } else {
            let t = clean_title(&s(c, "title").unwrap_or_default());
            let p = if prefix.is_empty() {
                t
            } else {
                format!("{prefix}: {t}")
            };
            leaves(c, &p, out);
        }
    }
}

/// Book tree → chapters (modules) of sections. Units are flattened ("Unit 1 · Chapter 2 …");
/// the Preface goes to About, other top-level pages (appendices) to "Back matter", and the
/// index is left out.
pub fn parse_tree(v: &Value) -> AppResult<Tree> {
    let tree = v
        .get("tree")
        .ok_or_else(|| AppError::internal("OpenStax changed its book format"))?;
    let mut front = vec![];
    let mut back = vec![];
    let mut chapters = vec![];
    fn walk(n: &Value, unit: &str, chapters: &mut Vec<Chapter>) {
        let title = clean_title(&s(n, "title").unwrap_or_default());
        let kids = n
            .get("contents")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if kids.iter().any(is_leaf) {
            let mut sections = vec![];
            leaves(n, "", &mut sections);
            let title = if unit.is_empty() {
                title
            } else {
                format!("{unit} · {title}")
            };
            chapters.push(Chapter { title, sections });
        } else {
            for k in &kids {
                walk(k, &title, chapters);
            }
        }
    }
    for n in tree
        .get("contents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if is_leaf(n) {
            let Some(sec) = section_of(n) else { continue };
            match sec.slug.as_str() {
                "preface" => front.push(sec),
                "index" => {}
                _ => back.push(sec),
            }
        } else {
            walk(n, "", &mut chapters);
        }
    }
    if !back.is_empty() {
        chapters.push(Chapter {
            title: "Back matter".into(),
            sections: back,
        });
    }
    if chapters.is_empty() {
        return Err(AppError::internal("this OpenStax book has no chapters"));
    }
    Ok(Tree {
        title: s(v, "title").unwrap_or_default(),
        front,
        chapters,
    })
}

pub fn page_url(archive: &str, uuid: &str, ver: &str, page_id: &str) -> String {
    format!("{archive}/contents/{uuid}@{ver}:{page_id}.json")
}

pub fn web_url(book: &str, page_slug: &str) -> String {
    format!("{BASE}/books/{book}/pages/{page_slug}")
}

/// Section page JSON → sanitised body HTML (relative image links made absolute).
pub fn section_html(page: &Value, url: &str) -> AppResult<String> {
    let content = page
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::internal("OpenStax changed its page format"))?;
    let body = match (content.find("<body"), content.rfind("</body>")) {
        (Some(a), Some(b)) if b > a => &content[a..b],
        _ => content,
    };
    Ok(common::sanitize_html(body, Some(url)))
}

pub struct Loaded {
    pub book: Book,
    pub archive: String,
    pub version: String,
    pub tree: Tree,
}

pub async fn load(polite: &Polite, book: Book) -> AppResult<Loaded> {
    let release: Value = polite.json(&format!("{BASE}/rex/release.json")).await?;
    let (archive, version) = parse_release(&release, &book.uuid)?;
    let tree: Value = polite
        .json(&format!("{archive}/contents/{}@{version}.json", book.uuid))
        .await?;
    Ok(Loaded {
        tree: parse_tree(&tree)?,
        book,
        archive,
        version,
    })
}

pub fn preview_of(l: &Loaded, pdf_bytes: Option<f64>) -> ProviderPreview {
    let n: usize = l.tree.chapters.iter().map(|c| c.sections.len()).sum();
    ProviderPreview {
        card: card(&l.book),
        outline: l
            .tree
            .chapters
            .iter()
            .map(|c| OutlineModule {
                title: c.title.clone(),
                items: c.sections.iter().map(|s| s.title.clone()).collect(),
            })
            .collect(),
        counts: ProviderCounts {
            lessons: n as i32,
            readings: n as i32,
            ..Default::default()
        },
        notes: vec![
            format!(
                "Each of the {n} sections becomes a reading lesson, with its figures and math, stored on your computer."
            ),
            "Section text is searchable (⌘K) and available to the AI features.".into(),
        ],
        est_bytes: None,
        pdf_bytes,
    }
}

/// Fetches every section (4 at a time) and its images into `root/img`, then builds the plan.
/// With `with_pdf`, also downloads the full-book PDF into `root`.
pub async fn plan(
    polite: &Arc<Polite>,
    l: &Loaded,
    root: &Path,
    with_pdf: bool,
    on: OnProgress<'_>,
) -> AppResult<Plan> {
    let all: Vec<&Section> = l
        .tree
        .front
        .iter()
        .chain(l.tree.chapters.iter().flat_map(|c| &c.sections))
        .collect();
    let total = all.len() as f64;
    let gate = Arc::new(tokio::sync::Semaphore::new(4));
    let mut jobs = tokio::task::JoinSet::new();
    for s in &all {
        let url = page_url(&l.archive, &l.book.uuid, &l.version, &s.page_id);
        let (gate, polite, id) = (gate.clone(), polite.clone(), s.page_id.clone());
        jobs.spawn(async move {
            let _slot = gate.acquire_owned().await;
            let r = async {
                let v: Value = polite.json(&url).await?;
                section_html(&v, &url)
            }
            .await;
            (id, r)
        });
    }
    let mut html: HashMap<String, String> = HashMap::new();
    let mut n = 0.0;
    while let Some(j) = jobs.join_next().await {
        n += 1.0;
        let (id, r) = j.map_err(|e| AppError::internal(e.to_string()))?;
        match r {
            Ok(h) => {
                html.insert(id, h);
            }
            Err(e) => log::warn!("OpenStax section {id} skipped: {e}"),
        }
        on(
            "download",
            n,
            total,
            &format!("Fetching sections ({n} of {total})…"),
        );
    }
    if html.is_empty() {
        return Err(AppError::internal(
            "couldn't fetch any sections of this book; try again",
        ));
    }
    let mut urls = vec![];
    for h in html.values() {
        urls.extend(common::image_urls(h));
    }
    urls.sort();
    urls.dedup();
    let images = common::fetch_images(polite, urls, root, on).await;
    for h in html.values_mut() {
        *h = common::rewrite_images(h, &images);
    }
    let mut pdf = None;
    if with_pdf && let Some(u) = &l.book.pdf_url {
        let dest = root.join("book.pdf");
        let report = |d: f64, t: f64| on("download", d, t, "Downloading the book PDF…");
        match polite.download(u, &dest, MAX_PDF_BYTES, &report).await {
            Ok(size) => pdf = Some(size as i64),
            Err(e) => log::warn!("OpenStax PDF skipped: {e}"),
        }
    }
    Ok(build_plan(l, &html, pdf))
}

/// Pure: tree + fetched section HTML (by page id) → plan.
pub fn build_plan(l: &Loaded, html: &HashMap<String, String>, pdf_size: Option<i64>) -> Plan {
    let b = &l.book;
    let mut skipped = 0;
    let modules = l
        .tree
        .chapters
        .iter()
        .map(|c| PlanModule {
            title: c.title.clone(),
            lessons: c
                .sections
                .iter()
                .filter_map(|s| {
                    let Some(h) = html.get(&s.page_id) else {
                        skipped += 1;
                        return None;
                    };
                    let url = web_url(&b.slug, &s.slug);
                    Some(PlanLesson {
                        title: s.title.clone(),
                        description: None,
                        url: Some(url.clone()),
                        body: LessonBody::Reading,
                        materials: vec![PlanMaterial {
                            uid: format!("page:{}", s.page_id),
                            kind: "reading".into(),
                            title: s.title.clone(),
                            section: Some(c.title.clone()),
                            source_url: Some(url),
                            text: Some(common::text_of(h)),
                            html: Some(h.clone()),
                            ..Default::default()
                        }],
                    })
                })
                .collect(),
        })
        .filter(|m| !m.lessons.is_empty())
        .collect();
    let pages = l
        .tree
        .front
        .iter()
        .filter_map(|s| {
            Some(PlanPage::new(
                "syllabus",
                &s.title,
                html.get(&s.page_id)?.clone(),
                Some(web_url(&b.slug, &s.slug)),
            ))
        })
        .collect();
    let course_materials = pdf_size
        .map(|size| PlanMaterial {
            uid: "book-pdf".into(),
            kind: "reading".into(),
            title: format!("{} (full PDF)", b.title),
            source_url: b.pdf_url.clone(),
            file_url: b.pdf_url.clone(),
            local: Some("book.pdf".into()),
            file_type: Some("application/pdf".into()),
            file_size: Some(size),
            ..Default::default()
        })
        .into_iter()
        .collect();
    let url = format!("{BASE}/details/books/{}", b.slug);
    let license = b
        .license
        .as_deref()
        .map(|n| short_license(n, b.license_url.as_deref()));
    Plan {
        provider: Some(Provider::Openstax),
        provider_ref: b.slug.clone(),
        title: b.title.clone(),
        description: b.description.clone(),
        instructor: Some(b.authors.join(", ")).filter(|a| !a.is_empty()),
        source_url: url.clone(),
        cover: None,
        tags: ["OpenStax".to_string()]
            .into_iter()
            .chain(b.subjects.iter().cloned())
            .collect(),
        meta: ProviderMeta {
            provider: Provider::Openstax.as_str().into(),
            provider_label: Provider::Openstax.label().into(),
            title: b.title.clone(),
            url,
            license,
            license_url: b.license_url.clone(),
            attribution: Some(format!(
                "“{}” by OpenStax (Rice University), {}. Access for free at openstax.org.",
                b.title,
                b.license
                    .as_deref()
                    .unwrap_or("see the book for its license")
            )),
            instructors: b.authors.clone(),
            institution: Some("OpenStax, Rice University".into()),
            description_html: b
                .description
                .as_deref()
                .map(|d| format!("<p>{}</p>", super::nptel::esc(d))),
            facts: [
                ("Subjects", b.subjects.join(", ")),
                ("Version", l.version.clone()),
            ]
            .into_iter()
            .filter(|(_, v)| !v.is_empty())
            .map(|(k, v)| Fact {
                label: k.into(),
                value: v,
            })
            .collect(),
        },
        modules,
        pages,
        course_materials,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let p = format!(
            "{}/src/services/fixtures/providers/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    fn loaded() -> Loaded {
        let books = parse_catalog(&fixture("openstax_catalog.json")).unwrap();
        let book = books
            .into_iter()
            .find(|b| b.slug == "calculus-volume-1")
            .unwrap();
        let (archive, version) =
            parse_release(&fixture("openstax_release.json"), &book.uuid).unwrap();
        Loaded {
            tree: parse_tree(&fixture("openstax_tree.json")).unwrap(),
            book,
            archive,
            version,
        }
    }

    #[test]
    fn catalog_hides_retired_books_and_keeps_licenses() {
        let books = parse_catalog(&fixture("openstax_catalog.json")).unwrap();
        assert!(books.iter().all(|b| b.state != "retired"));
        let calc = books
            .iter()
            .find(|b| b.slug == "calculus-volume-1")
            .unwrap();
        assert_eq!(calc.authors[0], "Gilbert Strang");
        assert_eq!(calc.subjects, vec!["Math"]);
        let c = card(calc);
        assert_eq!(c.license.as_deref(), Some("CC BY-NC-SA 4.0"));
        assert!(
            c.image_url
                .unwrap()
                .starts_with("https://assets.openstax.org/")
        );
        // A book without a stated license shows none rather than a guess.
        if let Some(b) = books.iter().find(|b| b.license.is_none()) {
            assert_eq!(card(b).license, None);
        }
    }

    #[test]
    fn tree_becomes_chapters_with_review_pages_and_back_matter() {
        let l = loaded();
        assert_eq!(
            l.archive,
            "https://openstax.org/apps/archive/20260604.144757"
        );
        let t = &l.tree;
        assert_eq!(t.front.len(), 1);
        assert_eq!(t.front[0].title, "Preface");
        assert_eq!(t.chapters[0].title, "Chapter 1 Functions and Graphs");
        let titles: Vec<&str> = t.chapters[0]
            .sections
            .iter()
            .map(|s| s.title.as_str())
            .collect();
        assert_eq!(titles[0], "Introduction");
        assert_eq!(titles[1], "1.1 Review of Functions");
        assert!(titles[2].starts_with("Chapter Review: "), "{titles:?}");
        assert_eq!(
            t.chapters[0].sections[1].page_id,
            "7cfe0552-bc12-452e-9c56-418ec508a280"
        );
        assert!(t.chapters.iter().any(|c| c.title == "Answer Key"));
        let back = t.chapters.last().unwrap();
        assert_eq!(back.title, "Back matter");
        assert!(back.sections.iter().all(|s| s.slug != "index"));
        assert_eq!(
            page_url(&l.archive, &l.book.uuid, &l.version, "x"),
            "https://openstax.org/apps/archive/20260604.144757/contents/8b89d172-2927-466f-8661-01abc7ccdba4@8dbc2ce:x.json"
        );
    }

    #[test]
    fn section_html_is_clean_with_math_and_absolute_images() {
        let l = loaded();
        let url = page_url(
            &l.archive,
            &l.book.uuid,
            &l.version,
            "7cfe0552-bc12-452e-9c56-418ec508a280",
        );
        let h = section_html(&fixture("openstax_page.json"), &url).unwrap();
        assert!(!h.contains("<script") && !h.contains("alert") && !h.contains("<style"));
        assert!(h.contains("Review of Functions") && h.contains("<math"));
        let imgs = common::image_urls(&h);
        assert_eq!(imgs.len(), 1);
        assert!(
            imgs[0].starts_with("https://openstax.org/apps/archive/20260604.144757/resources/"),
            "{imgs:?}"
        );

        let html = HashMap::from([("7cfe0552-bc12-452e-9c56-418ec508a280".to_string(), h)]);
        let p = build_plan(&l, &html, None);
        assert_eq!(p.lesson_count(), 1);
        assert!(p.skipped > 0);
        let m = &p.modules[0].lessons[0].materials[0];
        assert!(m.text.as_deref().unwrap().contains("Review of Functions"));
        assert_eq!(p.meta.license.as_deref(), Some("CC BY-NC-SA 4.0"));
        assert!(p.course_materials.is_empty());
        assert_eq!(build_plan(&l, &html, Some(10)).course_materials.len(), 1);
    }
}
