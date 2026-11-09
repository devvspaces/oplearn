//! LibreTexts: ~4,000 open textbooks. The Commons API gives the catalogue and each book's table
//! of contents; the official "pages" download is a ZIP with one PDF per page (`1.1! Title.pdf`),
//! so every section becomes a reading lesson with its own PDF (searchable after indexing).
//! The page HTML API (`/@api`) is disallowed by robots.txt, so it isn't used.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::build::{LessonBody, Plan, PlanLesson, PlanMaterial, PlanModule};
use super::common::{OnProgress, Polite};
use super::{
    Fact, OutlineModule, Provider, ProviderCard, ProviderCounts, ProviderMeta, ProviderPreview,
};
use crate::error::{AppError, AppResult};

pub const API: &str = "https://commons.libretexts.org/api/v1/commons";
pub const GAP: Duration = Duration::from_millis(500);
pub const MAX_ZIP_BYTES: u64 = 3 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Book {
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub affiliation: Option<String>,
    pub subject: Option<String>,
    pub library: Option<String>,
    pub summary: Option<String>,
    pub license: Option<String>,
    pub license_url: Option<String>,
    pub online: Option<String>,
    pub zip: Option<String>,
    pub pdf: Option<String>,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
}

/// LibreTexts licence code (+ `licenseversion:40` tag) → label and URL.
pub fn license(code: &str, version: Option<&str>) -> (Option<String>, Option<String>) {
    let ver = version
        .filter(|v| v.len() == 2 && v.chars().all(|c| c.is_ascii_digit()))
        .map(|v| format!("{}.{}", &v[..1], &v[1..]));
    let cc = |kind: &str| {
        let label = match &ver {
            Some(v) => format!("CC {} {v}", kind.to_uppercase()),
            None => format!("CC {}", kind.to_uppercase()),
        };
        let url = ver
            .as_ref()
            .map(|v| format!("https://creativecommons.org/licenses/{kind}/{v}/"));
        (Some(label), url)
    };
    match code {
        "ccby" => cc("by"),
        "ccbysa" => cc("by-sa"),
        "ccbync" => cc("by-nc"),
        "ccbyncsa" => cc("by-nc-sa"),
        "ccbynd" => cc("by-nd"),
        "ccbyncnd" => cc("by-nc-nd"),
        "publicdomain" => (Some("Public domain".into()), None),
        "gnufdl" => (
            Some("GNU FDL".into()),
            Some("https://www.gnu.org/licenses/fdl-1.3.html".into()),
        ),
        "gnu" => (Some("GNU GPL".into()), None),
        "ck12" => (
            Some("CK-12 license".into()),
            Some("https://www.ck12info.org/curriculum-materials-license/".into()),
        ),
        "mixed" | "multiple" => (Some("Mixed (see the book's licensing page)".into()), None),
        "arr" => (Some("All rights reserved".into()), None),
        "" => (None, None),
        other => (Some(other.to_string()), None),
    }
}

pub fn parse_catalog(v: &Value) -> AppResult<Vec<Book>> {
    let books = v
        .get("books")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::internal("LibreTexts changed its catalogue format"))?;
    Ok(books
        .iter()
        .filter_map(|b| {
            let version = b
                .get("libraryTags")
                .and_then(Value::as_array)
                .and_then(|t| {
                    t.iter()
                        .filter_map(Value::as_str)
                        .find_map(|t| t.strip_prefix("licenseversion:"))
                });
            let (license, license_url) = license(&s(b, "license").unwrap_or_default(), version);
            let link = |k: &str| b.get("links").and_then(|l| s(l, k));
            Some(Book {
                id: s(b, "bookID")?,
                title: s(b, "title")?,
                author: s(b, "author"),
                affiliation: s(b, "affiliation"),
                subject: s(b, "subject"),
                library: s(b, "library"),
                summary: s(b, "summary").map(|x| x.chars().take(400).collect()),
                license,
                license_url,
                online: link("online"),
                zip: link("zip"),
                pdf: link("pdf"),
            })
        })
        .collect())
}

pub fn card(b: &Book) -> ProviderCard {
    ProviderCard {
        id: b.id.clone(),
        title: b.title.clone(),
        subtitle: Some(
            [b.author.clone(), b.affiliation.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · "),
        )
        .filter(|x| !x.is_empty()),
        description: b.summary.clone(),
        image_url: None,
        license: b.license.clone(),
        license_url: b.license_url.clone(),
        url: b
            .online
            .clone()
            .unwrap_or_else(|| format!("https://commons.libretexts.org/book/{}", b.id)),
        tags: b.subject.iter().cloned().collect(),
    }
}

pub async fn catalog(polite: &Polite) -> AppResult<Vec<Book>> {
    parse_catalog(
        &polite
            .json::<Value>(&format!("{API}/catalog?limit=10000"))
            .await?,
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct TocPage {
    pub title: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TocChapter {
    pub title: String,
    pub url: Option<String>,
    pub pages: Vec<TocPage>,
}

/// Front/back-matter pages that are boilerplate in every book.
const SKIP: &[&str] = &[
    "titlepage",
    "infopage",
    "tableofcontents",
    "licensing",
    "detailedlicensing",
    "index",
    "cover",
];

fn key(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// TOC → chapters of pages (deeper levels flattened into their chapter; boilerplate dropped).
pub fn parse_toc(v: &Value) -> AppResult<Vec<TocChapter>> {
    let root = v
        .get("toc")
        .ok_or_else(|| AppError::internal("LibreTexts changed its contents format"))?;
    fn flat(n: &Value, out: &mut Vec<TocPage>) {
        for c in n
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let title = s(c, "title").unwrap_or_default();
            if !SKIP.contains(&key(&title).as_str()) {
                out.push(TocPage {
                    title,
                    url: s(c, "url"),
                });
            }
            flat(c, out);
        }
    }
    let chapters: Vec<TocChapter> = root
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|c| {
            let mut pages = vec![];
            flat(c, &mut pages);
            let title = s(c, "title").unwrap_or_default();
            TocChapter {
                url: s(c, "url"),
                // A chapter with no sections is itself the reading.
                pages: if pages.is_empty() {
                    vec![TocPage {
                        title: title.clone(),
                        url: s(c, "url"),
                    }]
                } else {
                    pages
                },
                title,
            }
        })
        .collect();
    Ok(chapters)
}

/// Number prefix of a TOC title ("1.1: Intro" → "1.1") or a ZIP name ("1.1! Intro.pdf" → "1.1").
fn number_of(s: &str, sep: char) -> Option<&str> {
    let (n, _) = s.split_once(sep)?;
    (!n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '.')).then_some(n)
}

/// Matches TOC pages to ZIP entries: by section number, else by title (front/back matter).
/// Returns TOC (chapter, page) → ZIP entry name.
pub fn match_files(toc: &[TocChapter], names: &[String]) -> HashMap<(usize, usize), String> {
    let mut by_num: HashMap<String, &String> = HashMap::new();
    let mut by_title: HashMap<String, &String> = HashMap::new();
    for n in names.iter().filter(|n| n.to_lowercase().ends_with(".pdf")) {
        let file = n.rsplit('/').next().unwrap_or(n);
        let Some((num, rest)) = file.split_once('!') else {
            continue;
        };
        let title = rest
            .trim()
            .trim_end_matches(".pdf")
            .trim_end_matches(".PDF");
        if !num.starts_with("00000") && !num.starts_with("99999") {
            by_num.entry(num.to_string()).or_insert(n);
        }
        // Front/back matter names look like "00000!E Introduction…"; drop the letter.
        let t = title
            .split_once(' ')
            .filter(|(l, _)| l.len() == 1)
            .map_or(title, |(_, t)| t);
        by_title.entry(key(t)).or_insert(n);
    }
    let mut out = HashMap::new();
    for (ci, c) in toc.iter().enumerate() {
        for (pi, p) in c.pages.iter().enumerate() {
            let found = number_of(&p.title, ':')
                .and_then(|num| by_num.get(num))
                .or_else(|| {
                    by_title.get(&key(p.title.split_once(": ").map_or(&p.title, |(_, t)| t)))
                })
                .or_else(|| by_title.get(&key(&p.title)));
            if let Some(n) = found {
                out.insert((ci, pi), (*n).clone());
            }
        }
    }
    out
}

pub struct Loaded {
    pub book: Book,
    pub toc: Vec<TocChapter>,
}

pub async fn load(polite: &Polite, book: Book) -> AppResult<Loaded> {
    let v: Value = polite.json(&format!("{API}/book/{}/toc", book.id)).await?;
    Ok(Loaded {
        toc: parse_toc(&v)?,
        book,
    })
}

pub fn preview_of(l: &Loaded, zip_bytes: Option<f64>) -> ProviderPreview {
    let n: usize = l.toc.iter().map(|c| c.pages.len()).sum();
    ProviderPreview {
        card: card(&l.book),
        outline: l
            .toc
            .iter()
            .map(|c| OutlineModule {
                title: c.title.clone(),
                items: c.pages.iter().map(|p| p.title.clone()).collect(),
            })
            .collect(),
        counts: ProviderCounts {
            lessons: n as i32,
            readings: n as i32,
            pdfs: n as i32,
            ..Default::default()
        },
        notes: vec![
            format!(
                "Each of the {n} pages becomes a reading lesson with its own PDF (LibreTexts' official per-page download)."
            ),
            "PDF text is indexed for search (⌘K) and the AI features.".into(),
        ],
        est_bytes: zip_bytes,
        pdf_bytes: None,
    }
}

/// Downloads the pages ZIP into `root`, extracts the PDFs as `pages/NNN.pdf` and builds the plan.
pub async fn plan(polite: &Polite, l: &Loaded, root: &Path, on: OnProgress<'_>) -> AppResult<Plan> {
    let url =
        l.book.zip.clone().ok_or_else(|| {
            AppError::invalid("LibreTexts doesn't offer a download for this book")
        })?;
    let zip_path = root.join("pages.zip");
    let report = |d: f64, t: f64| on("download", d, t, "Downloading the book's pages…");
    polite
        .download(&url, &zip_path, MAX_ZIP_BYTES, &report)
        .await?;
    on("extract", 0.0, 0.0, "Unpacking…");
    let (zp, dir) = (zip_path.clone(), root.join("pages"));
    let files = tokio::task::spawn_blocking(move || extract_pdfs(&zp, &dir))
        .await
        .map_err(|e| AppError::internal(e.to_string()))??;
    let _ = tokio::fs::remove_file(&zip_path).await;
    let names: Vec<String> = files.keys().cloned().collect();
    let matched = match_files(&l.toc, &names);
    Ok(build_plan(l, &matched, &files))
}

/// Extracts top-level PDFs to `dest/NNN.pdf` (our own names, so no path from the ZIP is used).
/// Returns entry name → (stored file name, size).
pub fn extract_pdfs(zip_path: &Path, dest: &Path) -> AppResult<HashMap<String, (String, i64)>> {
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::internal(format!("the book ZIP is unreadable: {e}")))?;
    std::fs::create_dir_all(dest)?;
    let mut out = HashMap::new();
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::internal(format!("ZIP entry {i}: {e}")))?;
        let name = entry.name().to_string();
        if entry.is_dir()
            || !name.to_lowercase().ends_with(".pdf")
            || entry.enclosed_name().is_none()
        {
            continue;
        }
        let stored = format!("{:03}.pdf", out.len() + 1);
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        std::fs::write(dest.join(&stored), &buf)?;
        out.insert(name, (stored, buf.len() as i64));
    }
    if out.is_empty() {
        return Err(AppError::internal("the book download had no pages"));
    }
    Ok(out)
}

/// Pure: TOC + matched files → plan. Pages without a file still become lessons that open online.
pub fn build_plan(
    l: &Loaded,
    matched: &HashMap<(usize, usize), String>,
    files: &HashMap<String, (String, i64)>,
) -> Plan {
    let b = &l.book;
    let modules = l
        .toc
        .iter()
        .enumerate()
        .map(|(ci, c)| PlanModule {
            title: c.title.clone(),
            lessons: c
                .pages
                .iter()
                .enumerate()
                .map(|(pi, p)| {
                    let file = matched.get(&(ci, pi)).and_then(|n| files.get(n));
                    PlanLesson {
                        title: p.title.clone(),
                        description: None,
                        url: p.url.clone(),
                        body: LessonBody::Reading,
                        materials: file
                            .map(|(stored, size)| PlanMaterial {
                                uid: format!("page:{ci}.{pi}"),
                                kind: "reading".into(),
                                title: p.title.clone(),
                                section: Some(c.title.clone()),
                                source_url: p.url.clone(),
                                local: Some(format!("pages/{stored}")),
                                file_type: Some("application/pdf".into()),
                                file_size: Some(*size),
                                ..Default::default()
                            })
                            .into_iter()
                            .collect(),
                    }
                })
                .collect(),
        })
        .filter(|m| !m.lessons.is_empty())
        .collect();
    let url = b
        .online
        .clone()
        .unwrap_or_else(|| format!("https://commons.libretexts.org/book/{}", b.id));
    Plan {
        provider: Some(Provider::Libretexts),
        provider_ref: b.id.clone(),
        title: b.title.clone(),
        description: b.summary.clone(),
        instructor: b.author.clone(),
        source_url: url.clone(),
        cover: None,
        tags: ["LibreTexts".to_string()]
            .into_iter()
            .chain(b.subject.clone())
            .collect(),
        meta: ProviderMeta {
            provider: Provider::Libretexts.as_str().into(),
            provider_label: Provider::Libretexts.label().into(),
            title: b.title.clone(),
            url,
            license: b.license.clone(),
            license_url: b.license_url.clone(),
            attribution: Some(format!(
                "“{}”{} via LibreTexts. Individual pages may carry their own licenses; see the book's Licensing page.",
                b.title,
                b.author
                    .as_deref()
                    .map(|a| format!(" by {a}"))
                    .unwrap_or_default()
            )),
            instructors: b.author.iter().cloned().collect(),
            institution: b.affiliation.clone(),
            description_html: b
                .summary
                .as_deref()
                .map(|d| format!("<p>{}</p>", super::nptel::esc(d))),
            facts: [
                ("Subject", b.subject.clone()),
                ("Library", b.library.clone()),
            ]
            .into_iter()
            .filter_map(|(k, v)| {
                Some(Fact {
                    label: k.into(),
                    value: v?,
                })
            })
            .collect(),
        },
        modules,
        pages: vec![],
        course_materials: vec![],
        skipped: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/src/services/fixtures/providers/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    fn loaded() -> Loaded {
        let books =
            parse_catalog(&serde_json::from_str(&fixture("libretexts_catalog.json")).unwrap())
                .unwrap();
        let book = books.into_iter().find(|b| b.id == "human-354234").unwrap();
        Loaded {
            toc: parse_toc(&serde_json::from_str(&fixture("libretexts_toc.json")).unwrap())
                .unwrap(),
            book,
        }
    }

    #[test]
    fn catalog_and_licenses() {
        let books =
            parse_catalog(&serde_json::from_str(&fixture("libretexts_catalog.json")).unwrap())
                .unwrap();
        assert_eq!(books.len(), 3);
        let b = &books[0];
        assert_eq!(b.license.as_deref(), Some("Public domain"));
        assert!(b.zip.as_deref().unwrap().ends_with("/human-354234/pages"));
        assert_eq!(
            card(b).subtitle.as_deref(),
            Some("Jessica Boehman · LaGuardia Community College")
        );
        assert!(books.iter().any(|b| b.license.is_none()));
        assert_eq!(
            license("ccbyncsa", Some("40")),
            (
                Some("CC BY-NC-SA 4.0".into()),
                Some("https://creativecommons.org/licenses/by-nc-sa/4.0/".into())
            )
        );
        assert_eq!(license("ccby", None).0.as_deref(), Some("CC BY"));
    }

    #[test]
    fn toc_matches_zip_pages() {
        let l = loaded();
        assert_eq!(l.toc[0].title, "Front Matter");
        assert!(
            l.toc[0]
                .pages
                .iter()
                .all(|p| p.title != "TitlePage" && p.title != "Licensing")
        );
        let ch3 = l
            .toc
            .iter()
            .find(|c| c.title == "3: Graphic Narrative")
            .unwrap();
        assert_eq!(
            ch3.pages.len(),
            1,
            "a chapter without sections is its own lesson"
        );
        assert!(
            l.toc
                .last()
                .unwrap()
                .pages
                .iter()
                .all(|p| p.title != "Index")
        );

        let names: Vec<String> = fixture("libretexts_zip_names.txt")
            .lines()
            .map(str::to_string)
            .collect();
        let m = match_files(&l.toc, &names);
        let n: usize = l.toc.iter().map(|c| c.pages.len()).sum();
        assert_eq!(m.len(), n, "every page has its PDF: {m:?}");
        let ch1 = l
            .toc
            .iter()
            .position(|c| c.title.starts_with("1:"))
            .unwrap();
        assert_eq!(
            m[&(ch1, 0)],
            "1.1! The Specifications- Getting Started with Any Illustration.pdf"
        );
        // "Where do your Illustrations Belong?" — the file name drops the "?".
        let ch2 = l
            .toc
            .iter()
            .position(|c| c.title.starts_with("2:"))
            .unwrap();
        assert_eq!(m[&(ch2, 0)], "2.1! Where do your Illustrations Belong.pdf");
        let intro = l.toc[0]
            .pages
            .iter()
            .position(|p| p.title.starts_with("Introduction"))
            .unwrap();
        assert!(m[&(0, intro)].starts_with("00000!E Introduction"));

        let files: HashMap<String, (String, i64)> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), (format!("{:03}.pdf", i + 1), 1000)))
            .collect();
        let p = build_plan(&l, &m, &files);
        assert_eq!(p.lesson_count(), n);
        let first = &p.modules[1].lessons[0];
        assert_eq!(
            first.materials[0].file_type.as_deref(),
            Some("application/pdf")
        );
        assert!(
            first.materials[0]
                .local
                .as_deref()
                .unwrap()
                .starts_with("pages/")
        );
        assert!(
            first
                .url
                .as_deref()
                .unwrap()
                .starts_with("https://human.libretexts.org/")
        );
        assert_eq!(p.meta.license.as_deref(), Some("Public domain"));
    }

    #[test]
    fn extracts_only_pdfs_under_our_own_names() {
        let dir = tempfile::tempdir().unwrap();
        let zp = dir.path().join("b.zip");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&zp).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            for n in ["1.1! A.pdf", "../../evil.pdf", "notes.txt"] {
                z.start_file(n, o).unwrap();
                std::io::Write::write_all(&mut z, b"%PDF-1.4").unwrap();
            }
            z.finish().unwrap();
        }
        let out = extract_pdfs(&zp, &dir.path().join("pages")).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out["1.1! A.pdf"].0, "001.pdf");
        assert!(dir.path().join("pages/001.pdf").is_file());
        assert!(!dir.path().join("evil.pdf").exists());
    }
}
