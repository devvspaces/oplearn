//! MIT OpenCourseWare provider.
//!
//! Source of truth is the course's offline ZIP (`/courses/<slug>/<short_id>.zip`, linked from
//! `/courses/<slug>/download/`). Its layout:
//! - `data.json`: course metadata (title, number, term/year, level, instructors, topics, image)
//! - `pages/<section>/data.json`: syllabus, calendar, readings, assignments… as HTML `content`
//! - `resources/<dir>/data.json`: one per file/video (`resourcetype`, `parent_title` section,
//!   `file` → `static_resources/<name>`, videos with `video_metadata.youtube_id` and captions)
//! - `video_galleries/<name>/data.json`: gallery with `videos` in order
//! - `static_resources/`: every PDF, caption (VTT/SRT) and image; videos live on YouTube.
//!
//! Everything here is pure (strings in, structs out) except `search`, `preview`, `download_zip`
//! and `extract_zip`, so the structure logic is unit-tested without the network.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::services::ai_context::html_to_text;

pub const BASE: &str = "https://ocw.mit.edu";
const SEARCH_URL: &str = "https://api.learn.mit.edu/api/v1/learning_resources_search/";
pub const LICENSE_URL: &str = "https://creativecommons.org/licenses/by-nc-sa/4.0/";

// ---- course identification ----

static SLUG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:ocw\.mit\.edu)?/?courses/([a-z0-9][a-z0-9-]+)").unwrap());

/// Accepts a course URL (any page of it) or a bare slug.
pub fn parse_slug(input: &str) -> Option<String> {
    let s = input.trim();
    if let Some(c) = SLUG_RE.captures(s) {
        return Some(c[1].to_string());
    }
    let ok = !s.is_empty()
        && s.len() < 200
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    ok.then(|| s.to_string())
}

pub fn course_url(slug: &str) -> String {
    format!("{BASE}/courses/{slug}/")
}

// ---- search (MIT Learn API) ----

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OcwCourseCard {
    pub slug: String,
    pub url: String,
    pub title: String,
    pub number: Option<String>,
    pub term: Option<String>,
    pub instructors: Vec<String>,
    pub level: Option<String>,
    pub image_url: Option<String>,
    pub description: Option<String>,
    pub features: Vec<String>,
}

pub fn parse_search(v: &Value) -> Vec<OcwCourseCard> {
    let results = v
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    results
        .into_iter()
        .filter_map(|r| {
            let url = r.get("url")?.as_str()?.to_string();
            let slug = parse_slug(&url).filter(|_| url.contains("ocw.mit.edu/courses/"))?;
            let run = r
                .get("runs")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .cloned()
                .unwrap_or(Value::Null);
            let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
            Some(OcwCourseCard {
                slug,
                title: s(&r, "title").unwrap_or_default(),
                number: s(&r, "readable_id")
                    .and_then(|id| id.split('+').next().map(str::to_string)),
                term: match (s(&run, "semester"), run.get("year").and_then(Value::as_i64)) {
                    (Some(sem), Some(y)) => Some(format!("{sem} {y}")),
                    (None, Some(y)) => Some(y.to_string()),
                    _ => None,
                },
                instructors: run
                    .get("instructors")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|i| s(i, "full_name")).collect())
                    .unwrap_or_default(),
                level: run
                    .get("level")
                    .and_then(Value::as_array)
                    .and_then(|a| a.first())
                    .and_then(|l| s(l, "name")),
                image_url: r.get("image").and_then(|i| s(i, "url")),
                description: s(&r, "description")
                    .map(|d| crate::services::ai_context::truncate(&d, 400)),
                features: r
                    .get("course_feature")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|f| f.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                url,
            })
        })
        .collect()
}

pub async fn search(http: &reqwest::Client, query: &str) -> AppResult<Vec<OcwCourseCard>> {
    let resp = http
        .get(SEARCH_URL)
        .query(&[
            ("q", query),
            ("platform", "ocw"),
            ("resource_type", "course"),
            ("limit", "24"),
        ])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(AppError::new(
            ErrorCode::Http,
            format!("MIT Learn search returned HTTP {}", resp.status()),
        ));
    }
    Ok(parse_search(&resp.json::<Value>().await?))
}

// ---- course metadata ----

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct OcwMeta {
    pub slug: String,
    pub title: String,
    pub course_number: Option<String>,
    pub extra_numbers: Option<String>,
    pub term: Option<String>,
    pub level: Vec<String>,
    pub instructors: Vec<String>,
    pub topics: Vec<String>,
    pub departments: Vec<String>,
    pub resource_types: Vec<String>,
    pub description: Option<String>,
    pub description_html: Option<String>,
    /// Path inside the ZIP (e.g. `static_resources/…jpg`).
    pub image_path: Option<String>,
    pub url: String,
    pub license: String,
}

fn str_vec(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_meta(json: &str, slug: &str) -> AppResult<OcwMeta> {
    let v: Value = serde_json::from_str(json)
        .map_err(|e| AppError::internal(format!("course data.json: {e}")))?;
    let s = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let html = s("course_description_html");
    Ok(OcwMeta {
        slug: s("site_url_path")
            .and_then(|p| parse_slug(&p))
            .unwrap_or_else(|| slug.to_string()),
        title: s("course_title")
            .ok_or_else(|| AppError::internal("course data.json has no title"))?,
        course_number: s("primary_course_number"),
        extra_numbers: s("extra_course_numbers"),
        term: match (s("term"), s("year")) {
            (Some(t), Some(y)) => Some(format!("{t} {y}")),
            (t, y) => t.or(y),
        },
        level: str_vec(v.get("level")),
        instructors: v
            .get("instructors")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|i| {
                        i.get("title")
                            .and_then(Value::as_str)
                            .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        topics: v
            .get("topics")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|p| str_vec(Some(p)).join(" > "))
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        departments: str_vec(v.get("department_numbers")),
        resource_types: str_vec(v.get("learning_resource_types")),
        description: html
            .as_deref()
            .map(html_to_text)
            .or_else(|| s("course_description")),
        description_html: html,
        image_path: s("image_src").map(|p| p.trim_start_matches("./").to_string()),
        url: course_url(slug),
        license: LICENSE_URL.into(),
    })
}

// ---- resources ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MaterialKind {
    Video,
    LectureNotes,
    Reading,
    Assignment,
    Exam,
    Recitation,
    Project,
    Transcript,
    Caption,
    Image,
    Other,
}

impl MaterialKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::LectureNotes => "lecture_notes",
            Self::Reading => "reading",
            Self::Assignment => "assignment",
            Self::Exam => "exam",
            Self::Recitation => "recitation",
            Self::Project => "project",
            Self::Transcript => "transcript",
            Self::Caption => "caption",
            Self::Image => "image",
            Self::Other => "other",
        }
    }
    /// Section module title used when a document isn't placed by the calendar.
    pub fn section_title(self) -> &'static str {
        match self {
            Self::LectureNotes => "Lecture Notes",
            Self::Reading => "Readings",
            Self::Assignment => "Assignments",
            Self::Exam => "Exams",
            Self::Recitation => "Recitations",
            Self::Project => "Projects",
            _ => "Other Materials",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct OcwResource {
    /// Directory name under `resources/` (= the resource's URL slug).
    pub dir: String,
    pub uid: String,
    pub title: String,
    pub description: Option<String>,
    pub resource_type: String,
    pub section: Option<String>,
    pub resource_types: Vec<String>,
    /// Path inside the ZIP, e.g. `static_resources/<hash>_<name>.pdf`.
    pub zip_path: Option<String>,
    /// Absolute online URL of the file.
    pub file_url: Option<String>,
    pub file_type: Option<String>,
    pub file_size: Option<i64>,
    pub youtube_id: Option<String>,
    pub archive_url: Option<String>,
    pub caption_zip_path: Option<String>,
}

impl OcwResource {
    pub fn source_url(&self, slug: &str) -> String {
        format!("{BASE}/courses/{slug}/resources/{}/", self.dir)
    }
    pub fn is_pdf(&self) -> bool {
        self.file_type.as_deref() == Some("application/pdf")
            || self
                .zip_path
                .as_deref()
                .is_some_and(|p| p.to_ascii_lowercase().ends_with(".pdf"))
    }
    pub fn is_solution(&self) -> bool {
        let d = self.dir.to_ascii_lowercase();
        self.resource_types
            .iter()
            .any(|t| t.to_ascii_lowercase().contains("solution"))
            || self.title.to_ascii_lowercase().contains("solution")
            || d.ends_with("_sol")
            || d.contains("_sol_")
            || d.ends_with("sol")
    }
    pub fn kind(&self) -> MaterialKind {
        let sec = self.section.as_deref().unwrap_or("").to_ascii_lowercase();
        let types = self.resource_types.join(" ").to_ascii_lowercase();
        let title = self.title.to_ascii_lowercase();
        if self.resource_type == "Video" || self.youtube_id.is_some() {
            return MaterialKind::Video;
        }
        if self.resource_type == "Image" {
            return MaterialKind::Image;
        }
        if title.contains("caption file")
            || self
                .zip_path
                .as_deref()
                .is_some_and(|p| p.ends_with(".vtt") || p.ends_with(".srt"))
        {
            return MaterialKind::Caption;
        }
        if title.contains("3play pdf") || title.contains("transcript") {
            return MaterialKind::Transcript;
        }
        let has = |s: &str| sec.contains(s) || types.contains(s);
        if has("lecture note") || has("lecture slide") || has("lecture summar") {
            MaterialKind::LectureNotes
        } else if has("exam") || has("quiz") {
            MaterialKind::Exam
        } else if has("recitation") || has("tutorial") {
            MaterialKind::Recitation
        } else if has("project") {
            MaterialKind::Project
        } else if has("assignment") || has("problem set") || has("homework") || has("lab") {
            MaterialKind::Assignment
        } else if has("reading") || has("textbook") {
            MaterialKind::Reading
        } else {
            MaterialKind::Other
        }
    }
}

/// `/courses/<slug>/<name>` → `static_resources/<name>`.
fn zip_path_of(file: &str) -> Option<String> {
    let name = file.rsplit('/').next().filter(|n| !n.is_empty())?;
    Some(format!("static_resources/{name}"))
}

pub fn parse_resource(dir: &str, json: &str) -> AppResult<OcwResource> {
    let v: Value = serde_json::from_str(json)
        .map_err(|e| AppError::internal(format!("resource {dir}: {e}")))?;
    let s = |v: &Value, k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let file = s(&v, "file");
    let vf = v.get("video_files").cloned().unwrap_or(Value::Null);
    let vm = v.get("video_metadata").cloned().unwrap_or(Value::Null);
    let caption = vf
        .get("video_captions_resources")
        .and_then(Value::as_array)
        .and_then(|a| {
            a.iter()
                .find(|c| {
                    c.get("language")
                        .and_then(Value::as_str)
                        .is_none_or(|l| l.starts_with("en"))
                })
                .or_else(|| a.first())
        })
        .and_then(|c| s(c, "file"))
        .or_else(|| s(&vf, "video_captions_file"));
    Ok(OcwResource {
        dir: dir.to_string(),
        uid: s(&v, "uid").unwrap_or_else(|| dir.to_string()),
        title: s(&v, "title").unwrap_or_else(|| dir.to_string()),
        description: s(&v, "description")
            .or_else(|| s(&v, "content").map(|h| html_to_text(&h)))
            .filter(|d| !d.is_empty()),
        resource_type: s(&v, "resourcetype").unwrap_or_default(),
        section: s(&v, "parent_title"),
        resource_types: str_vec(v.get("learning_resource_types")),
        zip_path: file.as_deref().and_then(zip_path_of),
        file_url: file.as_deref().map(|f| {
            if f.starts_with("http") {
                f.to_string()
            } else {
                format!("{BASE}{f}")
            }
        }),
        file_type: s(&v, "file_type"),
        file_size: v.get("file_size").and_then(|x| {
            x.as_i64()
                .or_else(|| x.as_str().and_then(|s| s.parse().ok()))
        }),
        youtube_id: s(&vm, "youtube_id"),
        archive_url: s(&vf, "archive_url"),
        caption_zip_path: caption.as_deref().and_then(zip_path_of),
    })
}

/// Video order from a gallery's `videos` list (resource uids or titles).
pub fn gallery_order(json: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return vec![];
    };
    v.get("videos")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    x.get("uid")
                        .and_then(Value::as_str)
                        .or_else(|| x.as_str())
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---- HTML tables in section pages ----

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cell {
    pub text: String,
    /// `resources/<dir>` links in this cell.
    pub links: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
}

static LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"href="[^"]*?/resources/([a-z0-9_.\-]+)/?(?:index\.html)?""#).unwrap()
});

/// A forgiving parser for OCW's (slightly malformed) section tables.
pub fn parse_tables(html: &str) -> Vec<Table> {
    let lower = html.to_ascii_lowercase();
    let mut tables = vec![];
    let mut pos = 0;
    while let Some(start) = lower[pos..].find("<table").map(|i| i + pos) {
        let end = lower[start..]
            .find("</table>")
            .map(|i| i + start)
            .unwrap_or(lower.len());
        let body = &html[start..end];
        let lb = &lower[start..end];
        let mut t = Table::default();
        for (ri, row) in split_tag(body, lb, "tr").into_iter().enumerate() {
            let rl = row.to_ascii_lowercase();
            let is_head = rl.contains("<th");
            let cells: Vec<Cell> = split_tag(row, &rl, if is_head { "th" } else { "td" })
                .into_iter()
                .map(|c| Cell {
                    text: html_to_text(c).replace('\n', " ").trim().to_string(),
                    links: LINK_RE.captures_iter(c).map(|m| m[1].to_string()).collect(),
                })
                .collect();
            if is_head && ri == 0 && t.headers.is_empty() {
                t.headers = cells.into_iter().map(|c| c.text.to_uppercase()).collect();
            } else if !cells.is_empty() {
                t.rows.push(cells);
            }
        }
        tables.push(t);
        pos = end.max(start + 6);
        if pos >= lower.len() {
            break;
        }
    }
    tables
}

/// Contents of each `<tag …>…</tag>` (or up to the next opening tag when unclosed).
fn split_tag<'a>(html: &'a str, lower: &str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = vec![];
    let mut i = 0;
    while let Some(s) = lower[i..].find(&open).map(|x| x + i) {
        // Skip e.g. "<tr" matching "<track".
        let after = lower
            .as_bytes()
            .get(s + open.len())
            .copied()
            .unwrap_or(b'>');
        if !(after == b'>' || after.is_ascii_whitespace()) {
            i = s + open.len();
            continue;
        }
        let content_start = lower[s..]
            .find('>')
            .map(|x| x + s + 1)
            .unwrap_or(lower.len());
        let next_open = lower[content_start..]
            .find(&open)
            .map(|x| x + content_start);
        let next_close = lower[content_start..]
            .find(&close)
            .map(|x| x + content_start);
        let end = match (next_close, next_open) {
            (Some(c), Some(o)) => c.min(o),
            (Some(c), None) => c,
            (None, Some(o)) => o,
            (None, None) => lower.len(),
        };
        out.push(&html[content_start..end]);
        i = end;
    }
    out
}

// ---- calendar ----

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CalendarRow {
    pub lec: Option<u32>,
    pub label: String,
    pub topic: String,
    pub key_dates: String,
}

static NUM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());

fn col(headers: &[String], keys: &[&str]) -> Option<usize> {
    headers
        .iter()
        .position(|h| keys.iter().any(|k| h.contains(k)))
}

/// Reads the calendar's first table with a lecture/session column.
pub fn parse_calendar(html: &str) -> Vec<CalendarRow> {
    for t in parse_tables(html) {
        let Some(lc) = col(&t.headers, &["LEC", "SES", "CLASS", "WEEK", "#"]) else {
            continue;
        };
        let tc =
            col(&t.headers, &["TOPIC", "LECTURE", "DESCRIPTION", "CONTENT"]).filter(|c| *c != lc);
        let kc = col(&t.headers, &["KEY DATE", "DUE", "ASSIGN", "DELIVERABLE"])
            .filter(|c| Some(*c) != tc && *c != lc);
        let rows: Vec<CalendarRow> = t
            .rows
            .iter()
            .filter(|r| r.len() > 1)
            .map(|r| {
                let get = |i: Option<usize>| {
                    i.and_then(|i| r.get(i))
                        .map(|c| c.text.clone())
                        .unwrap_or_default()
                };
                let label = get(Some(lc));
                CalendarRow {
                    lec: NUM_RE.find(&label).and_then(|m| m.as_str().parse().ok()),
                    topic: get(tc.or(Some(1))),
                    key_dates: get(kc),
                    label,
                }
            })
            .collect();
        if !rows.is_empty() {
            return rows;
        }
    }
    vec![]
}

// ---- numbering helpers ----

static LEC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:lecture|lec|session|ses|class|l)\s*#?\s*0*(\d+)").unwrap()
});
static CODE_NUM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:lec|ses|assn|ps|pset|hw|rec|chap|ch|quiz|lab)0*(\d+)").unwrap()
});
static PSET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:problem\s*set|pset|ps|assignment|homework|hw)\s*#?\s*0*(\d+)").unwrap()
});

pub fn lecture_number(title: &str) -> Option<u32> {
    LEC_RE.captures(title).and_then(|c| c[1].parse().ok())
}

/// Number carried by an assignment title or file code ("Problem Set 4", `assn04`).
pub fn assignment_number(r: &OcwResource) -> Option<u32> {
    PSET_RE
        .captures(&r.title)
        .or_else(|| CODE_NUM_RE.captures(&r.dir))
        .and_then(|c| c[1].parse().ok())
}

/// Problem-set numbers mentioned in a calendar KEY DATES cell ("Problem set 1 due").
pub fn due_assignments(text: &str) -> Vec<u32> {
    PSET_RE
        .captures_iter(text)
        .filter_map(|c| c[1].parse().ok())
        .collect()
}

// ---- solutions ----

fn base_code(dir: &str) -> String {
    let d = dir.to_ascii_lowercase();
    for suf in ["_solutions", "_solution", "_soln", "_sols", "_sol", "sol"] {
        if let Some(b) = d.strip_suffix(suf) {
            return b.trim_end_matches('_').to_string();
        }
    }
    d
}

/// Maps solution dir → the dir it solves, using section-table rows first (same row = same
/// item, as in Exams/Recitations pages) and then file codes (`assn03_sol` ↔ `assn03`).
pub fn pair_solutions(resources: &[OcwResource], tables: &[Table]) -> HashMap<String, String> {
    let by_dir: HashMap<&str, &OcwResource> =
        resources.iter().map(|r| (r.dir.as_str(), r)).collect();
    let mut out = HashMap::new();
    for t in tables {
        for row in &t.rows {
            let links: Vec<&str> = row
                .iter()
                .flat_map(|c| c.links.iter().map(String::as_str))
                .collect();
            let (sols, bases): (Vec<&str>, Vec<&str>) = links
                .iter()
                .partition(|d| by_dir.get(*d).is_some_and(|r| r.is_solution()));
            if let (Some(base), false) = (bases.first(), sols.is_empty()) {
                for s in sols {
                    out.insert(s.to_string(), base.to_string());
                }
            }
        }
    }
    let bases: HashMap<String, &str> = resources
        .iter()
        .filter(|r| !r.is_solution())
        .map(|r| (r.dir.to_ascii_lowercase(), r.dir.as_str()))
        .collect();
    for r in resources.iter().filter(|r| r.is_solution()) {
        if !out.contains_key(&r.dir)
            && let Some(b) = bases.get(&base_code(&r.dir))
        {
            out.insert(r.dir.clone(), b.to_string());
        }
    }
    out
}

/// Row descriptions from section tables (e.g. a recitation's topic), keyed by resource dir.
pub fn row_topics(tables: &[Table]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for t in tables {
        let tc = col(&t.headers, &["TOPIC", "DESCRIPTION", "TITLE", "CHAPTER"]);
        for row in t.rows.iter() {
            let topic = tc
                .and_then(|i| row.get(i))
                .map(|c| c.text.clone())
                .filter(|s| !s.is_empty() && s.len() < 300);
            if let Some(topic) = topic {
                for l in row.iter().flat_map(|c| c.links.iter()) {
                    out.entry(l.clone()).or_insert_with(|| topic.clone());
                }
            }
        }
    }
    out
}

// ---- structure ----

#[derive(Debug, Clone, PartialEq)]
pub enum PlanItem {
    /// Index into resources (a video).
    Video(usize),
    /// Index into resources (a document that becomes a reading lesson).
    Doc(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanModule {
    pub title: String,
    pub items: Vec<PlanItem>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Structure {
    pub modules: Vec<PlanModule>,
    /// Lecture-notes resource index → video resource index (attached, not separate lessons).
    pub notes_for_video: HashMap<usize, usize>,
    /// Solution index → solved resource index.
    pub solutions: HashMap<usize, usize>,
}

fn short(s: &str, n: usize) -> String {
    crate::services::ai_context::truncate(s, n)
}

/// Builds modules from the calendar when it numbers lectures: a module ends at each row whose
/// key dates mention something due or an exam, and problem sets due at that row join it.
/// Remaining documents go to section modules. Without a usable calendar: one "Lectures"
/// module in gallery order plus section modules.
pub fn plan_structure(
    resources: &[OcwResource],
    calendar: &[CalendarRow],
    gallery: &[String],
    tables: &[Table],
) -> Structure {
    let by_dir: HashMap<&str, usize> = resources
        .iter()
        .enumerate()
        .map(|(i, r)| (r.dir.as_str(), i))
        .collect();
    let mut st = Structure::default();
    for (s, b) in pair_solutions(resources, tables) {
        if let (Some(&si), Some(&bi)) = (by_dir.get(s.as_str()), by_dir.get(b.as_str())) {
            st.solutions.insert(si, bi);
        }
    }

    // Videos in order: gallery uids, then by lecture number, then title.
    let mut videos: Vec<usize> = (0..resources.len())
        .filter(|&i| resources[i].kind() == MaterialKind::Video)
        .collect();
    let rank: HashMap<&str, usize> = gallery
        .iter()
        .enumerate()
        .map(|(i, u)| (u.as_str(), i))
        .collect();
    videos.sort_by_key(|&i| {
        let r = &resources[i];
        (
            rank.get(r.uid.as_str()).copied().unwrap_or(usize::MAX),
            lecture_number(&r.title).unwrap_or(u32::MAX),
            r.title.clone(),
        )
    });

    // Lecture notes attach to the video with the same lecture number.
    let video_by_lec: HashMap<u32, usize> = videos
        .iter()
        .filter_map(|&i| lecture_number(&resources[i].title).map(|n| (n, i)))
        .collect();
    let docs: Vec<usize> = (0..resources.len())
        .filter(|&i| {
            let k = resources[i].kind();
            !matches!(
                k,
                MaterialKind::Video
                    | MaterialKind::Caption
                    | MaterialKind::Transcript
                    | MaterialKind::Image
            ) && resources[i].is_pdf()
                && !st.solutions.contains_key(&i)
        })
        .collect();
    let mut placed: HashSet<usize> = HashSet::new();
    for &d in &docs {
        let r = &resources[d];
        if r.kind() == MaterialKind::LectureNotes
            && let Some(&v) = lecture_number(&r.title)
                .or_else(|| CODE_NUM_RE.captures(&r.dir).and_then(|c| c[1].parse().ok()))
                .and_then(|n| video_by_lec.get(&n))
        {
            st.notes_for_video.insert(d, v);
            placed.insert(d);
        }
    }

    let assignments: HashMap<u32, usize> = docs
        .iter()
        .filter(|&&d| resources[d].kind() == MaterialKind::Assignment)
        .filter_map(|&d| assignment_number(&resources[d]).map(|n| (n, d)))
        .collect();

    let usable_calendar = calendar
        .iter()
        .filter(|r| r.lec.is_some_and(|n| video_by_lec.contains_key(&n)))
        .count()
        >= 3;
    let mut used_videos: HashSet<usize> = HashSet::new();
    if usable_calendar {
        let mut cur: Vec<PlanItem> = vec![];
        let mut topics: Vec<String> = vec![];
        let mut lecs: Vec<u32> = vec![];
        let flush = |cur: &mut Vec<PlanItem>,
                     topics: &mut Vec<String>,
                     lecs: &mut Vec<u32>,
                     out: &mut Vec<PlanModule>| {
            if cur.is_empty() {
                return;
            }
            let range = match (lecs.first(), lecs.last()) {
                (Some(a), Some(b)) if a != b => format!("Lectures {a}–{b}"),
                (Some(a), _) => format!("Lecture {a}"),
                _ => format!("Part {}", out.len() + 1),
            };
            let t: Vec<String> = topics.iter().take(3).cloned().collect();
            let title = if t.is_empty() {
                range
            } else {
                format!("{range}: {}", short(&t.join(", "), 70))
            };
            out.push(PlanModule {
                title,
                items: std::mem::take(cur),
            });
            topics.clear();
            lecs.clear();
        };
        for row in calendar {
            if let Some(&v) = row.lec.and_then(|n| video_by_lec.get(&n))
                && used_videos.insert(v)
            {
                cur.push(PlanItem::Video(v));
                lecs.extend(row.lec);
                if !row.topic.is_empty() {
                    topics.push(row.topic.clone());
                }
            }
            for n in due_assignments(&row.key_dates) {
                if let Some(&a) = assignments.get(&n)
                    && placed.insert(a)
                {
                    cur.push(PlanItem::Doc(a));
                }
            }
            let kd = row.key_dates.to_ascii_lowercase();
            if kd.contains("due")
                || kd.contains("exam")
                || kd.contains("quiz")
                || kd.contains("midterm")
            {
                flush(&mut cur, &mut topics, &mut lecs, &mut st.modules);
            }
        }
        // Lectures the calendar didn't mention, in order.
        for &v in &videos {
            if used_videos.insert(v) {
                cur.push(PlanItem::Video(v));
                lecs.extend(lecture_number(&resources[v].title));
            }
        }
        flush(&mut cur, &mut topics, &mut lecs, &mut st.modules);
    } else if !videos.is_empty() {
        st.modules.push(PlanModule {
            title: "Lecture Videos".into(),
            items: videos.iter().map(|&v| PlanItem::Video(v)).collect(),
        });
    }

    // Section modules for everything else, in a sensible order.
    let order = [
        MaterialKind::LectureNotes,
        MaterialKind::Reading,
        MaterialKind::Recitation,
        MaterialKind::Assignment,
        MaterialKind::Project,
        MaterialKind::Exam,
        MaterialKind::Other,
    ];
    for k in order {
        let mut items: Vec<usize> = docs
            .iter()
            .copied()
            .filter(|d| !placed.contains(d) && resources[*d].kind() == k)
            .collect();
        items.sort_by_key(|&d| {
            let r = &resources[d];
            (
                assignment_number(r)
                    .or_else(|| CODE_NUM_RE.captures(&r.dir).and_then(|c| c[1].parse().ok()))
                    .unwrap_or(u32::MAX),
                r.title.clone(),
            )
        });
        if !items.is_empty() {
            st.modules.push(PlanModule {
                title: k.section_title().into(),
                items: items.into_iter().map(PlanItem::Doc).collect(),
            });
        }
    }
    st
}

// ---- ZIP ----

/// Directory prefixes we need from the course ZIP (skips the bundled site assets).
const WANTED: &[&str] = &[
    "data.json",
    "content_map.json",
    "pages/",
    "resources/",
    "video_galleries/",
    "static_resources/",
];

/// Extracts the parts of an OCW ZIP we use into `dest`, refusing any entry that would land
/// outside it (zip-slip). Returns the number of files written.
pub fn extract_zip(zip_path: &Path, dest: &Path) -> AppResult<usize> {
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::internal(format!("course ZIP is unreadable: {e}")))?;
    std::fs::create_dir_all(dest)?;
    let root = dest.canonicalize()?;
    // Some ZIPs wrap everything in one top-level folder; detect and strip it.
    let prefix = common_prefix(&mut zip);
    let mut n = 0;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::internal(format!("course ZIP entry {i}: {e}")))?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(AppError::invalid(format!(
                "unsafe path in course ZIP: {}",
                entry.name()
            )));
        };
        let rel = rel.strip_prefix(&prefix).unwrap_or(&rel).to_path_buf();
        let rel_s = rel.to_string_lossy().replace('\\', "/");
        if entry.is_dir() || !WANTED.iter().any(|w| rel_s == *w || rel_s.starts_with(w)) {
            continue;
        }
        let out: PathBuf = root.join(&rel);
        if !out.starts_with(&root) {
            return Err(AppError::invalid(format!(
                "unsafe path in course ZIP: {rel_s}"
            )));
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        std::fs::write(&out, buf)?;
        n += 1;
    }
    Ok(n)
}

fn common_prefix<R: std::io::Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>) -> PathBuf {
    let has_root_data = zip.file_names().any(|n| n == "data.json");
    if has_root_data {
        return PathBuf::new();
    }
    zip.file_names()
        .find(|n| n.ends_with("/data.json") && n.matches('/').count() == 1)
        .and_then(|n| n.split('/').next().map(PathBuf::from))
        .unwrap_or_default()
}

static ZIP_HREF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"href="([^"]+\.zip)""#).unwrap());

/// Finds the course ZIP link on its `/download/` page.
pub fn find_zip_href(html: &str, slug: &str) -> Option<String> {
    let href = ZIP_HREF_RE.captures(html)?.get(1)?.as_str().to_string();
    Some(if href.starts_with("http") {
        href
    } else if href.starts_with('/') {
        format!("{BASE}{href}")
    } else {
        format!("{BASE}/courses/{slug}/{}", href.trim_start_matches("./"))
    })
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OcwPreview {
    pub meta: OcwMeta,
    pub image_url: Option<String>,
    pub zip_url: Option<String>,
    pub zip_bytes: Option<f64>,
}

pub async fn preview(http: &reqwest::Client, slug: &str) -> AppResult<OcwPreview> {
    let json = http
        .get(format!("{BASE}/courses/{slug}/data.json"))
        .send()
        .await?
        .error_for_status()
        .map_err(|_| AppError::not_found("that OpenCourseWare course"))?
        .text()
        .await?;
    let meta = parse_meta(&json, slug)?;
    let html = http
        .get(format!("{BASE}/courses/{slug}/download/"))
        .send()
        .await?
        .text()
        .await
        .unwrap_or_default();
    let zip_url = find_zip_href(&html, slug);
    let zip_bytes = match &zip_url {
        Some(u) => http.head(u).send().await.ok().and_then(|r| {
            r.headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok()?.parse::<f64>().ok())
        }),
        None => None,
    };
    Ok(OcwPreview {
        image_url: meta
            .image_path
            .as_ref()
            .map(|p| format!("{BASE}/courses/{slug}/{}", p.trim_start_matches("./"))),
        meta,
        zip_url,
        zip_bytes,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub const SLUG: &str = "6-042j-mathematics-for-computer-science-fall-2010";

    pub fn meta_json() -> String {
        json!({
            "course_title": "Mathematics for Computer Science",
            "course_description_html": "<p>This course covers elementary discrete mathematics.</p>",
            "primary_course_number": "6.042J", "extra_course_numbers": "18.062J",
            "term": "Fall", "year": "2010", "level": ["Undergraduate"],
            "instructors": [{"title": "Prof. Tom Leighton"}, {"title": "Dr. Marten  van Dijk"}],
            "topics": [["Mathematics", "Discrete Mathematics"], ["Engineering", "Computer Science"]],
            "department_numbers": ["6", "18"],
            "learning_resource_types": ["Problem Sets", "Lecture Videos"],
            "image_src": "./static_resources/42ba_6-042jf10.jpg",
            "site_url_path": format!("courses/{SLUG}")
        })
        .to_string()
    }

    pub fn video(n: u32, yt: &str, topic: &str) -> (String, String) {
        (
            format!("lecture-{n}-x"),
            json!({
                "title": format!("Lecture {n}: {topic}"), "resourcetype": "Video", "parent_title": "Video Lectures",
                "learning_resource_types": ["Lecture Videos"], "uid": format!("uid-lec-{n}"),
                "video_files": {"archive_url": format!("http://archive.org/{n}.mp4"),
                    "video_captions_resources": [{"file": format!("/courses/{SLUG}/hash{n}_{yt}.vtt"), "language": "en"}]},
                "video_metadata": {"youtube_id": yt}
            })
            .to_string(),
        )
    }

    pub fn doc(dir: &str, title: &str, section: &str, types: &[&str]) -> (String, String) {
        (
            dir.to_string(),
            json!({
                "title": title, "resourcetype": "Document", "parent_title": section,
                "learning_resource_types": types, "file": format!("/courses/{SLUG}/h_{}.pdf", dir.to_uppercase()),
                "file_type": "application/pdf", "file_size": 1000, "uid": format!("uid-{dir}")
            })
            .to_string(),
        )
    }

    pub const CALENDAR: &str = "<p><table><thead><tr><th>LEC #</th><th>TOPICS</th><th>KEY DATES</th></p></tr><p></thead>\
        <tr><td>1</td><td>Introduction and proofs</td><td> </td></p><p></tr>\
        <tr><td>2</td><td>Induction</td><td>Problem set 1 due</td></p><p></tr>\
        <tr><td>3</td><td>Strong induction</td><td> </td></tr>\
        <tr><td>4</td><td>Number theory I</td><td>Problem set 2 due</td></tr></table>";

    pub const EXAMS: &str = "<table><thead><tr><th>DESCRIPTIONS</th><th>EXAMS</th><th>SOLUTIONS</th></tr></thead>\
        <tr><td>Midterm practice problems</td><td>(<a href=\"../../resources/mit6_042jf10_mid_practice/index.html\">PDF</a>)</td>\
        <td>(<a href=\"../../resources/mit6_042jf10_mid_pctce_sol/index.html\">PDF</a>)</td></tr></table>";

    pub fn resources() -> Vec<OcwResource> {
        let mut v = vec![
            video(1, "L3LMbpZIKhQ", "Introduction and Proofs"),
            video(2, "z8HKWUWS-lA", "Induction"),
            video(3, "NuGDkmwEObM", "Strong Induction"),
            video(4, "NuY7szYSXSw", "Number Theory I"),
            doc(
                "mit6_042jf10_assn01",
                "Problem Set 1",
                "Assignments",
                &["Problem Sets"],
            ),
            doc(
                "mit6_042jf10_assn02",
                "Problem Set 2",
                "Assignments",
                &["Problem Sets"],
            ),
            doc(
                "mit6_042jf10_assn02_sol",
                "Problem Set 2 Solutions",
                "Assignments",
                &["Problem Set Solutions"],
            ),
            doc(
                "mit6_042jf10_chap01",
                "Chapter 1: Propositions",
                "Readings",
                &["Readings"],
            ),
            doc(
                "mit6_042jf10_mid_practice",
                "Midterm Practice Problems",
                "Exams",
                &["Exams"],
            ),
            doc(
                "mit6_042jf10_mid_pctce_sol",
                "Midterm Practice Problems Solutions",
                "Exams",
                &["Exams", "Exam Solutions"],
            ),
            doc(
                "mit6_042jf10_rec01",
                "Problems for Recitation 1",
                "Recitations",
                &["Recitations"],
            ),
        ];
        v.push((
            "l3lmbpzikhq".into(),
            json!({"title": "3play caption file", "resourcetype": "Other", "file": format!("/courses/{SLUG}/hash1_L3LMbpZIKhQ.vtt"),
                   "file_type": "application/x-subrip"})
            .to_string(),
        ));
        v.into_iter()
            .map(|(d, j)| parse_resource(&d, &j).unwrap())
            .collect()
    }

    #[test]
    fn slugs() {
        assert_eq!(
            parse_slug(&format!(
                "https://ocw.mit.edu/courses/{SLUG}/pages/syllabus/"
            ))
            .as_deref(),
            Some(SLUG)
        );
        assert_eq!(parse_slug(SLUG).as_deref(), Some(SLUG));
        assert_eq!(parse_slug("not a slug!"), None);
    }

    #[test]
    fn meta_parsing() {
        let m = parse_meta(&meta_json(), SLUG).unwrap();
        assert_eq!(m.title, "Mathematics for Computer Science");
        assert_eq!(m.course_number.as_deref(), Some("6.042J"));
        assert_eq!(m.term.as_deref(), Some("Fall 2010"));
        assert_eq!(
            m.instructors,
            vec!["Prof. Tom Leighton", "Dr. Marten van Dijk"]
        );
        assert_eq!(m.topics[0], "Mathematics > Discrete Mathematics");
        assert_eq!(
            m.description.as_deref(),
            Some("This course covers elementary discrete mathematics.")
        );
        assert_eq!(
            m.image_path.as_deref(),
            Some("static_resources/42ba_6-042jf10.jpg")
        );
    }

    #[test]
    fn resource_parsing_and_kinds() {
        let r = resources();
        assert_eq!(r[0].kind(), MaterialKind::Video);
        assert_eq!(r[0].youtube_id.as_deref(), Some("L3LMbpZIKhQ"));
        assert_eq!(
            r[0].caption_zip_path.as_deref(),
            Some("static_resources/hash1_L3LMbpZIKhQ.vtt")
        );
        assert_eq!(r[4].kind(), MaterialKind::Assignment);
        assert_eq!(
            r[4].zip_path.as_deref(),
            Some("static_resources/h_MIT6_042JF10_ASSN01.pdf")
        );
        assert!(r[6].is_solution() && !r[5].is_solution());
        assert_eq!(r[7].kind(), MaterialKind::Reading);
        assert_eq!(r[8].kind(), MaterialKind::Exam);
        assert_eq!(r[10].kind(), MaterialKind::Recitation);
        assert_eq!(r[11].kind(), MaterialKind::Caption);
        assert_eq!(
            r[4].source_url(SLUG),
            format!("{BASE}/courses/{SLUG}/resources/mit6_042jf10_assn01/")
        );
    }

    #[test]
    fn tables_and_calendar() {
        let cal = parse_calendar(CALENDAR);
        assert_eq!(cal.len(), 4);
        assert_eq!(
            cal[1],
            CalendarRow {
                lec: Some(2),
                label: "2".into(),
                topic: "Induction".into(),
                key_dates: "Problem set 1 due".into()
            }
        );
        let t = parse_tables(EXAMS);
        assert_eq!(t[0].headers, vec!["DESCRIPTIONS", "EXAMS", "SOLUTIONS"]);
        assert_eq!(t[0].rows[0][1].links, vec!["mit6_042jf10_mid_practice"]);
        assert_eq!(
            row_topics(&t)
                .get("mit6_042jf10_mid_practice")
                .map(String::as_str),
            Some("Midterm practice problems")
        );
    }

    #[test]
    fn numbering() {
        assert_eq!(lecture_number("Lecture 12: Sums"), Some(12));
        assert_eq!(lecture_number("Ses 3 notes"), Some(3));
        assert_eq!(due_assignments("Problem set 3 due; PS 4 out"), vec![3, 4]);
        let r = resources();
        assert_eq!(assignment_number(&r[5]), Some(2));
    }

    #[test]
    fn solutions_pair_by_row_and_by_code() {
        let r = resources();
        let p = pair_solutions(&r, &parse_tables(EXAMS));
        assert_eq!(
            p.get("mit6_042jf10_mid_pctce_sol").map(String::as_str),
            Some("mit6_042jf10_mid_practice"),
            "row pairing"
        );
        assert_eq!(
            p.get("mit6_042jf10_assn02_sol").map(String::as_str),
            Some("mit6_042jf10_assn02"),
            "code pairing"
        );
    }

    #[test]
    fn structure_from_calendar() {
        let r = resources();
        let s = plan_structure(&r, &parse_calendar(CALENDAR), &[], &parse_tables(EXAMS));
        let titles: Vec<&str> = s.modules.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "Lectures 1–2: Introduction and proofs, Induction",
                "Lectures 3–4: Strong induction, Number theory I",
                "Readings",
                "Recitations",
                "Exams"
            ]
        );
        // Problem set 1 is in the first module (due at lecture 2); set 2 in the second.
        assert_eq!(
            s.modules[0].items,
            vec![PlanItem::Video(0), PlanItem::Video(1), PlanItem::Doc(4)]
        );
        assert_eq!(
            s.modules[1].items,
            vec![PlanItem::Video(2), PlanItem::Video(3), PlanItem::Doc(5)]
        );
        // Solutions are linked, not modules of their own.
        assert_eq!(s.solutions.get(&6), Some(&5));
        assert_eq!(s.solutions.get(&9), Some(&8));
        assert!(
            !s.modules
                .iter()
                .flat_map(|m| &m.items)
                .any(|i| *i == PlanItem::Doc(6) || *i == PlanItem::Doc(9))
        );
    }

    #[test]
    fn structure_without_calendar() {
        let r = resources();
        let s = plan_structure(&r, &[], &["uid-lec-2".into()], &[]);
        assert_eq!(s.modules[0].title, "Lecture Videos");
        assert_eq!(
            s.modules[0].items[0],
            PlanItem::Video(1),
            "gallery order first"
        );
        assert!(s.modules.iter().any(|m| m.title == "Assignments"));
    }

    #[test]
    fn zip_extraction_is_scoped_and_safe() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let zp = dir.path().join("c.zip");
        {
            let f = std::fs::File::create(&zp).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let o = zip::write::SimpleFileOptions::default();
            for (name, body) in [
                ("data.json", "{}"),
                ("resources/a/data.json", "{}"),
                ("static_resources/x.pdf", "%PDF"),
                ("static_shared/js/big.js", "skip me"),
            ] {
                w.start_file(name, o).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        let out = dir.path().join("out");
        assert_eq!(extract_zip(&zp, &out).unwrap(), 3);
        assert!(out.join("static_resources/x.pdf").is_file());
        assert!(!out.join("static_shared").exists());

        let evil = dir.path().join("evil.zip");
        {
            let f = std::fs::File::create(&evil).unwrap();
            let mut w = zip::ZipWriter::new(f);
            w.start_file("../../escape.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        assert!(extract_zip(&evil, &dir.path().join("out2")).is_err());
        assert!(!dir.path().join("escape.txt").exists());
    }

    #[test]
    fn zip_href_and_search_mapping() {
        let h = format!("<a href=\"/courses/{SLUG}/6.042j-fall-2010.zip\">Download</a>");
        assert_eq!(
            find_zip_href(&h, SLUG).unwrap(),
            format!("{BASE}/courses/{SLUG}/6.042j-fall-2010.zip")
        );
        let v = json!({"results": [
            {"title": "Mathematics for Computer Science", "url": format!("https://ocw.mit.edu/courses/{SLUG}/"), "readable_id": "6.042J+fall_2010",
             "runs": [{"semester": "Fall", "year": 2010, "instructors": [{"full_name": "Prof. Tom Leighton"}], "level": [{"name": "Undergraduate"}]}],
             "image": {"url": "https://ocw.mit.edu/img.jpg"}, "course_feature": ["Lecture Videos"]},
            {"title": "Elsewhere", "url": "https://openlearninglibrary.mit.edu/courses/x/about"}
        ]});
        let c = parse_search(&v);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].slug, SLUG);
        assert_eq!(c[0].number.as_deref(), Some("6.042J"));
        assert_eq!(c[0].term.as_deref(), Some("Fall 2010"));
        assert_eq!(c[0].level.as_deref(), Some("Undergraduate"));
    }

    /// `cargo test --lib -- --ignored live_ocw --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_ocw_search_and_preview() {
        let http = reqwest::Client::builder()
            .user_agent("Lectern-test")
            .build()
            .unwrap();
        let r = search(&http, "mathematics for computer science")
            .await
            .unwrap();
        println!(
            "{} results; first: {} {:?} {:?}",
            r.len(),
            r[0].title,
            r[0].number,
            r[0].term
        );
        assert!(r.iter().any(|c| c.slug == SLUG));
        let p = preview(&http, SLUG).await.unwrap();
        println!(
            "{:?} {:?} instructors={:?}",
            p.zip_url, p.zip_bytes, p.meta.instructors
        );
        assert!(p.zip_url.as_deref().is_some_and(|u| u.ends_with(".zip")));
        assert!(p.zip_bytes.unwrap_or(0.0) > 1e6);
    }
}
