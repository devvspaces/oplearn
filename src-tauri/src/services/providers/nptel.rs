//! NPTEL (nptel.ac.in): ~3,500 IIT/IISc courses. The site is SvelteKit, so its pages have
//! `__data.json` twins in devalue format (`devalue.rs`). Video courses list weeks → lectures
//! with a YouTube id and subtitle files; "Web" courses link to HTML lecture notes.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use super::build::{LessonBody, Plan, PlanLesson, PlanModule, PlanPage};
use super::common::{self, OnProgress, Polite};
use super::devalue::{self, changed};
use super::{
    Fact, OutlineModule, Provider, ProviderCard, ProviderCounts, ProviderMeta, ProviderPreview,
};
use crate::error::{AppError, AppResult};
use crate::services::vtt;

pub const BASE: &str = "https://nptel.ac.in";
/// As stated in the nptel.ac.in footer ("Distributed under Creative Commons … CC BY-NC-SA").
pub const LICENSE: &str = "CC BY-NC-SA (as stated by NPTEL)";
pub const LICENSE_URL: &str = "https://creativecommons.org/licenses/by-nc-sa/4.0/";
pub const GAP: Duration = Duration::from_millis(250);

pub fn course_url(id: &str) -> String {
    format!("{BASE}/courses/{id}")
}

/// Course id from an id or a course link (`nptel.ac.in/courses/106106145`).
pub fn parse_id(input: &str) -> Option<String> {
    let s = input.trim().trim_end_matches('/');
    let id = s.rsplit('/').next()?;
    (id.len() >= 6 && id.len() <= 12 && id.chars().all(|c| c.is_ascii_digit())).then(|| id.into())
}

fn str_of(v: &Value, k: &str) -> Option<String> {
    match v.get(k)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn display_title(t: &str) -> String {
    t.trim_start_matches("NOC:").trim().to_string()
}

/// Catalogue (`/courses/__data.json`) → cards.
pub fn parse_catalog(data: &Value) -> AppResult<Vec<ProviderCard>> {
    let courses = data
        .get("courses")
        .and_then(Value::as_array)
        .ok_or_else(|| changed("no course list"))?;
    let names = |k: &str| -> HashMap<String, String> {
        data.get(k)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|d| Some((str_of(d, "id")?, str_of(d, "name")?)))
                    .collect()
            })
            .unwrap_or_default()
    };
    let disciplines = names("disciplines");
    Ok(courses
        .iter()
        .filter_map(|c| {
            let id = str_of(c, "id")?;
            let title = str_of(c, "title")?;
            let mut tags = vec![];
            if let Some(d) = str_of(c, "disciplineId").and_then(|d| disciplines.get(&d).cloned()) {
                tags.push(d);
            }
            if str_of(c, "contentType").as_deref() == Some("Web") {
                tags.push("Web notes".into());
            }
            let subtitle = [str_of(c, "instituteName"), str_of(c, "professor")]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            Some(ProviderCard {
                url: course_url(&id),
                id,
                title: display_title(&title),
                subtitle: (!subtitle.is_empty()).then_some(subtitle),
                license: Some(LICENSE.into()),
                license_url: Some(LICENSE_URL.into()),
                tags,
                ..Default::default()
            })
        })
        .collect())
}

pub async fn catalog(polite: &Polite) -> AppResult<Vec<ProviderCard>> {
    let reply: Value = polite.json(&format!("{BASE}/courses/__data.json")).await?;
    parse_catalog(&devalue::page_data(&reply)?)
}

/// A lecture from a course outline.
#[derive(Debug, Clone, PartialEq)]
pub struct Lecture {
    pub title: String,
    pub youtube_id: Option<String>,
    /// English subtitles, as a fetchable URL.
    pub vtt: Option<String>,
    /// Web courses: the HTML/PDF lecture page.
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Week {
    pub title: String,
    pub lectures: Vec<Lecture>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Course {
    pub id: String,
    pub title: String,
    pub professor: Option<String>,
    pub institute: Option<String>,
    pub web: bool,
    pub weeks: Vec<Week>,
    pub about_html: Option<String>,
    pub instructor_html: Option<String>,
    pub books_html: Option<String>,
    /// (week, topic) from the syllabus.
    pub syllabus_weeks: Vec<(String, String)>,
    pub facts: Vec<Fact>,
}

/// Subtitle reference → URL: `gs://bucket/path` (public Cloud Storage), a URL, or a Google
/// Drive file id (newer English tracks).
pub fn vtt_url(r: &str) -> Option<String> {
    let r = r.trim();
    if let Some(rest) = r.strip_prefix("gs://") {
        return Some(format!("https://storage.googleapis.com/{rest}"));
    }
    if r.starts_with("https://") {
        return Some(r.into());
    }
    (r.len() >= 20
        && r.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    .then(|| format!("https://drive.google.com/uc?export=download&id={r}"))
}

fn youtube_id(v: &Value) -> Option<String> {
    let id = str_of(v, "youtube_id")?;
    (id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
    .then_some(id)
}

/// Course page (`/courses/<id>/__data.json`) → outline.
pub fn parse_course(id: &str, data: &Value) -> AppResult<Course> {
    let co = data
        .get("courseOutline")
        .filter(|v| v.is_object())
        .ok_or_else(|| changed("no course outline"))?;
    let units = co
        .get("units")
        .and_then(Value::as_array)
        .ok_or_else(|| changed("no units"))?;
    let weeks = units
        .iter()
        .map(|u| Week {
            title: str_of(u, "name").unwrap_or_else(|| "Lectures".into()),
            lectures: u
                .get("lessons")
                .and_then(Value::as_array)
                .map(|ls| {
                    ls.iter()
                        .map(|l| Lecture {
                            title: str_of(l, "name").unwrap_or_else(|| "Lecture".into()),
                            youtube_id: youtube_id(l),
                            vtt: l
                                .get("vtt_url")
                                .and_then(|v| v.get("English"))
                                .and_then(Value::as_str)
                                .and_then(vtt_url),
                            link: str_of(l, "lecturelink").filter(|u| u.starts_with("http")),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
        .filter(|w| !w.lectures.is_empty())
        .collect();
    let syl = co.get("syllabus").filter(|s| s.is_object());
    let html = |k: &str| syl.and_then(|s| str_of(s, k));
    let facts = syl
        .into_iter()
        .flat_map(|s| ["meta", "facts"].map(|k| s.get(k).and_then(Value::as_array).cloned()))
        .flatten()
        .flatten()
        .filter_map(|f| {
            let label = str_of(&f, "label")?;
            let value = str_of(&f, "value")?;
            (!label.starts_with("Category")).then_some(Fact { label, value })
        })
        .collect();
    Ok(Course {
        id: id.into(),
        title: display_title(&str_of(co, "title").unwrap_or_else(|| "NPTEL course".into())),
        professor: str_of(co, "professor"),
        institute: str_of(co, "instituteName"),
        web: str_of(co, "contentType").as_deref() == Some("Web"),
        weeks,
        about_html: html("aboutHtml"),
        instructor_html: html("instructorHtml"),
        books_html: html("booksHtml"),
        syllabus_weeks: syl
            .and_then(|s| s.get("weeks"))
            .and_then(Value::as_array)
            .map(|ws| {
                ws.iter()
                    .filter_map(|w| Some((str_of(w, "week")?, str_of(w, "topic")?)))
                    .collect()
            })
            .unwrap_or_default(),
        facts,
    })
}

pub async fn course(polite: &Polite, id: &str) -> AppResult<Course> {
    let reply: Value = polite
        .json(&format!("{BASE}/courses/{id}/__data.json"))
        .await
        .map_err(|e| {
            if e.message.contains("404") {
                AppError::not_found("that NPTEL course")
            } else {
                e
            }
        })?;
    parse_course(id, &devalue::page_data(&reply)?)
}

fn card_of(c: &Course) -> ProviderCard {
    ProviderCard {
        id: c.id.clone(),
        title: c.title.clone(),
        subtitle: Some(
            [c.institute.clone(), c.professor.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · "),
        )
        .filter(|s| !s.is_empty()),
        description: c.about_html.as_deref().map(common::text_of),
        license: Some(LICENSE.into()),
        license_url: Some(LICENSE_URL.into()),
        url: course_url(&c.id),
        ..Default::default()
    }
}

pub fn preview_of(c: &Course) -> ProviderPreview {
    let mut counts = ProviderCounts::default();
    for w in &c.weeks {
        for l in &w.lectures {
            counts.lessons += 1;
            if l.youtube_id.is_some() {
                counts.videos += 1;
                if l.vtt.is_some() {
                    counts.transcripts += 1;
                }
            } else if l.link.is_some() {
                counts.readings += 1;
            }
        }
    }
    let mut notes = vec![];
    if counts.videos > 0 {
        notes.push(format!(
            "{} lecture videos stream from YouTube; {} have English subtitles that become transcripts.",
            counts.videos, counts.transcripts
        ));
    }
    if counts.readings > 0 {
        notes.push(format!(
            "{} lecture notes open on nptel.ac.in (they aren't downloaded).",
            counts.readings
        ));
    }
    let unusable = counts.lessons - counts.videos - counts.readings;
    if unusable > 0 {
        notes.push(format!(
            "{unusable} items have no video or link and are left out."
        ));
    }
    ProviderPreview {
        card: card_of(c),
        outline: c
            .weeks
            .iter()
            .map(|w| OutlineModule {
                title: w.title.clone(),
                items: w.lectures.iter().map(|l| l.title.clone()).collect(),
            })
            .collect(),
        counts,
        notes,
        est_bytes: None,
        pdf_bytes: None,
    }
}

/// Fetches subtitles (4 at a time) and builds the plan.
pub async fn plan(polite: &Arc<Polite>, c: &Course, on: OnProgress<'_>) -> AppResult<Plan> {
    let urls: Vec<String> = c
        .weeks
        .iter()
        .flat_map(|w| &w.lectures)
        .filter(|l| l.youtube_id.is_some())
        .filter_map(|l| l.vtt.clone())
        .collect();
    let total = urls.len() as f64;
    let gate = Arc::new(tokio::sync::Semaphore::new(4));
    let mut jobs = tokio::task::JoinSet::new();
    for u in urls {
        let (gate, polite) = (gate.clone(), polite.clone());
        jobs.spawn(async move {
            let _slot = gate.acquire_owned().await;
            let text = polite
                .text(&u)
                .await
                .ok()
                .filter(|t| t.trim_start().starts_with("WEBVTT"));
            (u, text)
        });
    }
    let mut subs: HashMap<String, String> = HashMap::new();
    let mut n = 0.0;
    while let Some(j) = jobs.join_next().await {
        n += 1.0;
        if let Ok((u, Some(t))) = j {
            subs.insert(u, t);
        }
        on("download", n, total, "Fetching subtitles…");
    }
    Ok(build_plan(c, &subs))
}

/// Pure: course outline + fetched subtitles (by URL) → plan.
pub fn build_plan(c: &Course, subs: &HashMap<String, String>) -> Plan {
    let mut skipped = 0;
    let modules = c
        .weeks
        .iter()
        .map(|w| PlanModule {
            title: w.title.clone(),
            lessons: w
                .lectures
                .iter()
                .filter_map(|l| {
                    let body = if let Some(yt) = &l.youtube_id {
                        LessonBody::Video {
                            youtube_id: yt.clone(),
                            channel: "NPTEL".into(),
                            cues: l
                                .vtt
                                .as_ref()
                                .and_then(|u| subs.get(u))
                                .map(|t| vtt::parse(t, false))
                                .unwrap_or_default(),
                            chapters: vec![],
                        }
                    } else if l.link.is_some() {
                        LessonBody::Reading
                    } else {
                        skipped += 1;
                        return None;
                    };
                    Some(PlanLesson {
                        title: l.title.clone(),
                        description: None,
                        url: l.link.clone(),
                        body,
                        materials: vec![],
                    })
                })
                .collect(),
        })
        .filter(|m| !m.lessons.is_empty())
        .collect();
    let url = course_url(&c.id);
    let mut pages = vec![];
    if let Some(h) = &c.about_html {
        pages.push(PlanPage::new(
            "syllabus",
            "About the course",
            common::sanitize_html(h, Some(&url)),
            Some(url.clone()),
        ));
    }
    if !c.syllabus_weeks.is_empty() {
        let rows: String = c
            .syllabus_weeks
            .iter()
            .map(|(w, t)| format!("<tr><td>{}</td><td>{}</td></tr>", esc(w), esc(t)))
            .collect();
        pages.push(PlanPage::new(
            "page",
            "Weekly topics",
            format!("<table><thead><tr><th>Week</th><th>Topics</th></tr></thead><tbody>{rows}</tbody></table>"),
            Some(url.clone()),
        ));
    }
    if let Some(h) = &c.books_html {
        pages.push(PlanPage::new(
            "page",
            "Books and references",
            common::sanitize_html(h, Some(&url)),
            Some(url.clone()),
        ));
    }
    if let Some(h) = &c.instructor_html {
        pages.push(PlanPage::new(
            "page",
            "Instructor",
            common::sanitize_html(h, Some(&url)),
            Some(url.clone()),
        ));
    }
    Plan {
        provider: Some(Provider::Nptel),
        provider_ref: c.id.clone(),
        title: c.title.clone(),
        description: c
            .about_html
            .as_deref()
            .map(common::text_of)
            .map(|t| t.chars().take(2000).collect()),
        instructor: c.professor.clone(),
        source_url: url.clone(),
        cover: None,
        tags: ["NPTEL".to_string()]
            .into_iter()
            .chain(c.institute.clone())
            .collect(),
        meta: ProviderMeta {
            provider: Provider::Nptel.as_str().into(),
            provider_label: Provider::Nptel.label().into(),
            title: c.title.clone(),
            url,
            license: Some(LICENSE.into()),
            license_url: Some(LICENSE_URL.into()),
            attribution: Some(
                "NPTEL (National Programme on Technology Enhanced Learning), IITs and IISc.".into(),
            ),
            instructors: c.professor.iter().cloned().collect(),
            institution: c.institute.clone(),
            description_html: c
                .about_html
                .as_deref()
                .map(|h| common::sanitize_html(h, Some(BASE))),
            facts: c.facts.clone(),
        },
        modules,
        pages,
        course_materials: vec![],
        skipped,
    }
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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

    #[test]
    fn catalog_cards() {
        let cards =
            parse_catalog(&devalue::page_data(&fixture("nptel_catalog.json")).unwrap()).unwrap();
        assert!(cards.len() >= 4);
        let py = cards.iter().find(|c| c.id == "106106145").unwrap();
        assert_eq!(
            py.title,
            "Programming, Data Structures and Algorithms using Python"
        );
        assert!(py.subtitle.as_deref().unwrap().contains("Madhavan Mukund"));
        assert_eq!(py.url, "https://nptel.ac.in/courses/106106145");
        assert!(py.license.as_deref().unwrap().contains("BY-NC-SA"));
        let web = cards.iter().find(|c| c.id == "106103069").unwrap();
        assert!(web.tags.iter().any(|t| t == "Web notes"));
        let found = common::rank("python data structures", &cards, |c| {
            (c.title.clone(), c.subtitle.clone().unwrap_or_default())
        });
        assert_eq!(found[0].id, "106106145");
    }

    #[test]
    fn course_outline_and_plan() {
        let c = parse_course(
            "106106145",
            &devalue::page_data(&fixture("nptel_course.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(c.weeks.len(), 2);
        assert!(c.weeks[0].title.starts_with("Week 1"));
        let l = &c.weeks[0].lectures[0];
        assert_eq!(l.youtube_id.as_deref(), Some("9MmC_uGjBsM"));
        assert!(
            l.vtt
                .as_deref()
                .unwrap()
                .starts_with("https://drive.google.com/uc?export=download&id=1Pxj")
        );
        assert_eq!(
            c.institute.as_deref(),
            Some("Chennai Mathematical Institute")
        );
        assert!(c.facts.iter().any(|f| f.label == "Duration"));
        assert!(!c.facts.iter().any(|f| f.label == "Category"));

        let vtt = "WEBVTT\n\n00:01.270 --> 00:07.520\nWelcome to the first lecture\n\n61:00.000 --> 61:04.000\nend\n";
        let subs = HashMap::from([(l.vtt.clone().unwrap(), vtt.to_string())]);
        let p = build_plan(&c, &subs);
        assert_eq!(p.modules.len(), 2);
        let LessonBody::Video { cues, .. } = &p.modules[0].lessons[0].body else {
            panic!()
        };
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[1].start, 3660.0);
        assert!(
            p.pages
                .iter()
                .any(|pg| pg.title == "About the course" && !pg.html.contains("script"))
        );
        assert!(p.pages.iter().any(|pg| pg.title == "Weekly topics"));
        assert_eq!(p.meta.license.as_deref(), Some(LICENSE));
        let pv = preview_of(&c);
        assert_eq!(pv.counts.videos, 4);
    }

    #[test]
    fn web_courses_become_readings_that_open_online() {
        let c = parse_course(
            "106103069",
            &devalue::page_data(&fixture("nptel_web_course.json")).unwrap(),
        )
        .unwrap();
        assert!(c.web);
        let p = build_plan(&c, &HashMap::new());
        let l = &p.modules[0].lessons[0];
        assert!(matches!(l.body, LessonBody::Reading));
        assert!(
            l.url
                .as_deref()
                .unwrap()
                .starts_with("https://archive.nptel.ac.in/")
        );
        assert!(
            preview_of(&c)
                .notes
                .iter()
                .any(|n| n.contains("open on nptel.ac.in"))
        );
    }

    #[test]
    fn subtitle_references_and_ids() {
        assert_eq!(
            vtt_url("gs://swayam-node1-production.appspot.com/assets/vtt/x/Hindi/Lec-01.vtt")
                .as_deref(),
            Some(
                "https://storage.googleapis.com/swayam-node1-production.appspot.com/assets/vtt/x/Hindi/Lec-01.vtt"
            )
        );
        assert_eq!(vtt_url("short"), None);
        assert_eq!(vtt_url("javascript:alert(1)"), None);
        assert_eq!(
            parse_id("https://nptel.ac.in/courses/106106145/").as_deref(),
            Some("106106145")
        );
        assert_eq!(parse_id("106106145").as_deref(), Some("106106145"));
        assert_eq!(parse_id("python"), None);
        let bad = serde_json::json!({"courseOutline": {"title": "x"}});
        assert!(
            parse_course("1", &bad)
                .unwrap_err()
                .to_string()
                .contains("NPTEL changed")
        );
    }
}
