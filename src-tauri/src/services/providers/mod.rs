//! Course providers beyond YouTube playlists. MIT OCW has its own commands (`commands/ocw.rs`);
//! the others share one search → preview → import flow (`commands/providers.rs`): each module
//! turns provider data into a `build::Plan`, and `build::write` creates the course.

pub mod build;
pub mod common;
pub mod devalue;
pub mod libretexts;
pub mod nptel;
pub mod ocw;
pub mod ocw_import;
pub mod openstax;
pub mod yale;

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Youtube,
    MitOcw,
    Nptel,
    Openstax,
    Libretexts,
    Yale,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Youtube => "youtube",
            Self::MitOcw => "mit_ocw",
            Self::Nptel => "nptel",
            Self::Openstax => "openstax",
            Self::Libretexts => "libretexts",
            Self::Yale => "yale",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Youtube => "YouTube",
            Self::MitOcw => "MIT OpenCourseWare",
            Self::Nptel => "NPTEL",
            Self::Openstax => "OpenStax",
            Self::Libretexts => "LibreTexts",
            Self::Yale => "Open Yale Courses",
        }
    }
}

/// A search result.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCard {
    pub id: String,
    pub title: String,
    /// e.g. "IIT Madras · Prof. X" or the author list.
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
    /// As the provider states it; `None` when it doesn't say (never assumed).
    pub license: Option<String>,
    pub license_url: Option<String>,
    pub url: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OutlineModule {
    pub title: String,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCounts {
    pub lessons: i32,
    pub videos: i32,
    pub readings: i32,
    pub pdfs: i32,
    pub transcripts: i32,
}

#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreview {
    pub card: ProviderCard,
    pub outline: Vec<OutlineModule>,
    pub counts: ProviderCounts,
    /// Plain sentences about what the import brings (and leaves out).
    pub notes: Vec<String>,
    /// Approximate download size, when known.
    pub est_bytes: Option<f64>,
    /// Size of the optional full-book PDF (OpenStax), when offered.
    pub pdf_bytes: Option<f64>,
}

/// Import progress, sent over a channel (MIT OCW uses the same shape).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    /// download | extract | build | index | done
    pub stage: String,
    pub done: f64,
    pub total: f64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImportReport {
    pub course_id: String,
    pub lessons: i32,
    pub materials: i32,
    pub transcripts: i32,
    pub pdfs_indexed: i32,
    /// Items the provider listed but we couldn't use (e.g. lessons without a video).
    pub skipped: i32,
}

/// Course metadata for the About tab (every provider but MIT OCW, which has `OcwMeta`).
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMeta {
    pub provider: String,
    pub provider_label: String,
    pub title: String,
    pub url: String,
    pub license: Option<String>,
    pub license_url: Option<String>,
    pub attribution: Option<String>,
    pub instructors: Vec<String>,
    pub institution: Option<String>,
    pub description_html: Option<String>,
    /// Extra facts shown as a list (label, value).
    pub facts: Vec<Fact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    pub label: String,
    pub value: String,
}

/// Live checks against the real sites (network): `cargo test --lib -- --ignored live_provider --nocapture`.
#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::repo::{lessons, materials, search};

    fn http() -> reqwest::Client {
        reqwest::Client::builder()
            .user_agent(concat!("Lectern/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap()
    }

    fn report(p: &build::Plan, dir: &std::path::Path) -> String {
        let conn = open_in_memory().unwrap();
        let w = build::write(&conn, p, dir).unwrap();
        let ls = lessons::list_course_lessons(&conn, &w.course_id).unwrap();
        let mats = materials::list_for_course(&conn, &w.course_id).unwrap();
        let info = materials::course_info(&conn, &w.course_id).unwrap();
        assert!(!ls.is_empty());
        assert!(info.provider_meta.as_ref().unwrap().license.is_some());
        let _ = search::global_search(&conn, "the", 5).unwrap();
        format!(
            "{} lessons, {} materials, {} transcripts, {} pdfs, {} pages; first: {:?}",
            w.lessons,
            mats.len(),
            w.transcripts,
            w.pdfs.len(),
            info.pages.len(),
            ls.first().map(|l| &l.title)
        )
    }

    fn quiet() -> impl Fn(&str, f64, f64, &str) + Send + Sync {
        common::no_progress()
    }

    #[tokio::test]
    #[ignore]
    async fn live_provider_nptel() {
        let pol = common::Polite::new(&http(), nptel::GAP);
        let cards = nptel::catalog(&pol).await.unwrap();
        assert!(cards.len() > 3000, "{}", cards.len());
        let mut c = nptel::course(&pol, "106106145").await.unwrap();
        c.weeks.truncate(1);
        let p = nptel::plan(&pol, &c, &quiet()).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        println!("NPTEL: {}", report(&p, dir.path()));
    }

    #[tokio::test]
    #[ignore]
    async fn live_provider_openstax() {
        let pol = common::Polite::new(&http(), openstax::GAP);
        let books = openstax::catalog(&pol).await.unwrap();
        let book = books.into_iter().find(|b| b.slug == "calculus-volume-1").unwrap();
        let mut l = openstax::load(&pol, book).await.unwrap();
        l.tree.chapters.truncate(1);
        l.tree.chapters[0].sections.truncate(2);
        let dir = tempfile::tempdir().unwrap();
        let p = openstax::plan(&pol, &l, dir.path(), false, &quiet()).await.unwrap();
        let html = p.modules[0].lessons[1].materials[0].html.clone().unwrap();
        assert!(html.contains("<math") && html.contains("lectern-media:img/"), "math and stored images");
        println!("OpenStax: {}", report(&p, dir.path()));
    }

    #[tokio::test]
    #[ignore]
    async fn live_provider_libretexts() {
        let pol = common::Polite::new(&http(), libretexts::GAP);
        let books = libretexts::catalog(&pol).await.unwrap();
        assert!(books.len() > 3000);
        let book = books.into_iter().find(|b| b.id == "human-354234").unwrap();
        let l = libretexts::load(&pol, book).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = libretexts::plan(&pol, &l, dir.path(), &quiet()).await.unwrap();
        let with_pdf = p.modules.iter().flat_map(|m| &m.lessons).filter(|l| !l.materials.is_empty()).count();
        assert_eq!(with_pdf, p.lesson_count(), "every lesson has its PDF");
        println!("LibreTexts: {}", report(&p, dir.path()));
    }

    #[tokio::test]
    #[ignore]
    async fn live_provider_yale() {
        let pol = common::Polite::new(&http(), yale::GAP);
        let list = yale::catalog(&pol).await.unwrap();
        assert!(list.len() > 30, "{}", list.len());
        let mut c = yale::course(&pol, "astronomy/astr-160").await.unwrap();
        let exam = c.sessions.iter().position(|s| s.label.starts_with("Exam")).unwrap();
        c.sessions = vec![c.sessions[0].clone(), c.sessions[exam].clone()];
        let dir = tempfile::tempdir().unwrap();
        let p = yale::plan(&pol, &c, dir.path(), &quiet()).await.unwrap();
        println!("Yale: {}", report(&p, dir.path()));
    }
}
