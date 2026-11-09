//! Open Yale Courses (oyc.yale.edu): ~40 recorded Yale courses. There's no API, so the Drupal
//! pages are parsed: the course list, a course page (About / Syllabus / Sessions tabs) and each
//! session page (YouTube embed, chapter times, an untimed transcript, PDF handouts).
//! Requests are spaced a second apart.

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};

use super::build::{LessonBody, Plan, PlanLesson, PlanMaterial, PlanModule, PlanPage};
use super::common::{self, OnProgress, Polite};
use super::{
    Fact, OutlineModule, Provider, ProviderCard, ProviderCounts, ProviderMeta, ProviderPreview,
};
use crate::error::{AppError, AppResult};
use crate::services::ytdlp::ChapterInfo;

pub const BASE: &str = "https://oyc.yale.edu";
pub const LICENSE: &str = "CC BY-NC-SA 3.0 US";
pub const LICENSE_URL: &str = "https://creativecommons.org/licenses/by-nc-sa/3.0/us/";
pub const GAP: Duration = Duration::from_secs(1);
pub const MAX_PDF_BYTES: u64 = 200 * 1024 * 1024;

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("valid selector")
}

fn text(e: &ElementRef<'_>) -> String {
    e.text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn changed(what: &str) -> AppError {
    AppError::internal(format!(
        "Open Yale Courses changed its pages ({what}); please report this"
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Listed {
    /// Path such as `astronomy/astr-160`.
    pub id: String,
    pub number: String,
    pub title: String,
    pub department: String,
    pub professor: String,
    pub semester: String,
}

/// Valid course path: `<department>/<code>`.
pub fn parse_id(input: &str) -> Option<String> {
    let s = input.trim().trim_end_matches('/');
    let s = s
        .split("oyc.yale.edu/")
        .last()
        .unwrap_or(s)
        .trim_start_matches('/');
    let parts: Vec<&str> = s.split('/').collect();
    let ok = |p: &str| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    (parts.len() >= 2 && ok(parts[0]) && ok(parts[1])).then(|| format!("{}/{}", parts[0], parts[1]))
}

pub fn parse_courses(html: &str) -> AppResult<Vec<Listed>> {
    let doc = Html::parse_document(html);
    let row = sel("tr");
    let cell = |c: &str| sel(&format!("td.views-field-{c}"));
    let (dept, num, title, prof, sem) = (
        cell("title"),
        cell("field-course-number"),
        cell("title-1"),
        cell("field-course-professor-last-name"),
        cell("field-semester"),
    );
    let a = sel("a[href]");
    let out: Vec<Listed> = doc
        .select(&row)
        .filter_map(|r| {
            let t = r.select(&title).next()?;
            let href = t.select(&a).next()?.value().attr("href")?;
            let get = |s: &Selector| r.select(s).next().map(|e| text(&e)).unwrap_or_default();
            Some(Listed {
                id: parse_id(href)?,
                number: get(&num),
                title: text(&t),
                department: get(&dept),
                professor: get(&prof),
                semester: get(&sem),
            })
        })
        .collect();
    if out.is_empty() {
        return Err(changed("no courses listed"));
    }
    Ok(out)
}

pub fn card(l: &Listed) -> ProviderCard {
    ProviderCard {
        id: l.id.clone(),
        title: l.title.clone(),
        subtitle: Some(format!("{} · {} · {}", l.number, l.professor, l.semester)),
        description: None,
        image_url: None,
        license: Some(LICENSE.into()),
        license_url: Some(LICENSE_URL.into()),
        url: format!("{BASE}/{}", l.id),
        tags: vec![l.department.clone()],
    }
}

pub async fn catalog(polite: &Polite) -> AppResult<Vec<Listed>> {
    parse_courses(&polite.text(&format!("{BASE}/courses")).await?)
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRef {
    /// e.g. "Lecture 1", "Exam 1", "Update 2".
    pub label: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Course {
    pub id: String,
    pub title: String,
    pub number: Option<String>,
    pub professor: Option<String>,
    pub about_html: Option<String>,
    pub syllabus_html: Option<String>,
    pub structure: Option<String>,
    pub sessions: Vec<SessionRef>,
    /// Course-wide PDFs linked from the syllabus: (title, url).
    pub pdfs: Vec<(String, String)>,
}

fn abs(href: &str) -> String {
    let h = href.replace("http://oyc.yale.edu", BASE);
    if h.starts_with('/') {
        format!("{BASE}{h}")
    } else {
        h
    }
}

fn field(scope: &ElementRef<'_>, name: &str) -> Option<String> {
    let s = sel(&format!(".views-field-{name} .field-content"));
    scope
        .select(&s)
        .next()
        .map(|e| e.inner_html())
        .filter(|h| !h.trim().is_empty())
}

static PDF_HREF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:https?://oyc\.yale\.edu)?/sites/default/files/[^?#]+\.pdf$").unwrap()
});

/// PDF links under `scope` (only Yale's own files), with their link text.
fn pdf_links(scope: &ElementRef<'_>) -> Vec<(String, String)> {
    let a = sel("a[href]");
    let mut out: Vec<(String, String)> = vec![];
    for e in scope.select(&a) {
        let href = e.value().attr("href").unwrap_or_default();
        if PDF_HREF.is_match(href) {
            let url = abs(href);
            if !out.iter().any(|(_, u)| *u == url) {
                let t = text(&e).replace("[PDF]", "").trim().to_string();
                out.push((if t.is_empty() { "Handout".into() } else { t }, url));
            }
        }
    }
    out
}

pub fn parse_course(id: &str, html: &str) -> AppResult<Course> {
    let doc = Html::parse_document(html);
    let tab = |n: u8| {
        doc.select(&sel(&format!("#quicktabs-tabpage-course_quicktabs-{n}")))
            .next()
    };
    let sessions_tab = tab(2).ok_or_else(|| changed("no sessions tab"))?;
    let row = sel("tr");
    let (num, ttl, a) = (
        sel(".views-field-field-session-display-number"),
        sel(".views-field-field-session-display-title"),
        sel("a[href]"),
    );
    let sessions: Vec<SessionRef> = sessions_tab
        .select(&row)
        .filter_map(|r| {
            let t = r.select(&ttl).next()?;
            let href = t.select(&a).next()?.value().attr("href")?;
            Some(SessionRef {
                label: r.select(&num).next().map(|e| text(&e)).unwrap_or_default(),
                title: text(&t),
                url: abs(href),
            })
        })
        .collect();
    if sessions.is_empty() {
        return Err(changed("no sessions"));
    }
    let about = tab(0);
    let syllabus = tab(1);
    let title = doc
        .select(&sel("h1.page-title"))
        .next()
        .map(|e| text(&e))
        .ok_or_else(|| changed("no title"))?;
    let prof = syllabus
        .as_ref()
        .and_then(|s| {
            s.select(&sel(
                ".views-field-field-course-professor-last-name .field-content",
            ))
            .next()
        })
        .map(|e| text(&e));
    let url = format!("{BASE}/{id}");
    let clean = |h: String| common::sanitize_html(&h, Some(&url));
    Ok(Course {
        id: id.into(),
        title,
        number: about
            .as_ref()
            .and_then(|a| field(a, "field-course-number"))
            .map(|h| common::text_of(&h).trim().to_string()),
        professor: prof,
        about_html: about.as_ref().and_then(|a| field(a, "body")).map(clean),
        structure: about
            .as_ref()
            .and_then(|a| field(a, "field-course-structure"))
            .map(|h| common::text_of(&h).trim().to_string()),
        syllabus_html: syllabus
            .as_ref()
            .and_then(|s| s.select(&sel(".view-content")).next())
            .map(|e| clean(e.inner_html())),
        pdfs: syllabus.as_ref().map(pdf_links).unwrap_or_default(),
        sessions,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub title: String,
    pub overview: Option<String>,
    pub youtube_id: Option<String>,
    pub chapters: Vec<ChapterInfo>,
    /// Sanitised transcript HTML (untimed; chapter headings carry times).
    pub transcript_html: Option<String>,
    pub pdfs: Vec<(String, String)>,
}

static EMBED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"youtube(?:-nocookie)?\.com/embed/([A-Za-z0-9_-]{11})").unwrap());

pub fn parse_session(html: &str, url: &str) -> AppResult<Session> {
    let doc = Html::parse_document(html);
    let node = doc
        .select(&sel("article.node--type-session"))
        .next()
        .ok_or_else(|| changed("no session"))?;
    let title = node
        .select(&sel(".session-full__title"))
        .next()
        .map(|e| text(&e))
        .unwrap_or_default();
    let overview = node
        .select(&sel(".session-full__body p"))
        .map(|e| text(&e))
        .collect::<Vec<_>>()
        .join("\n\n");
    let youtube_id = node.select(&sel("iframe[src]")).find_map(|f| {
        EMBED
            .captures(f.value().attr("src")?)
            .map(|c| c[1].to_string())
    });
    let names: Vec<String> = doc
        .select(&sel(
            ".views-field-field-session-chapters-name .field-content",
        ))
        .map(|e| text(&e))
        .collect();
    let starts: Vec<f64> = doc
        .select(&sel(
            ".views-field-field-session-chapter-start-time .field-content",
        ))
        .filter_map(|e| text(&e).parse().ok())
        .collect();
    let chapters: Vec<ChapterInfo> = names
        .iter()
        .zip(&starts)
        .enumerate()
        .map(|(i, (n, &s))| ChapterInfo {
            start: s,
            end: starts.get(i + 1).copied(),
            title: n.clone(),
        })
        .collect();
    let transcript_html = doc.select(&sel("#inline_content")).next().map(|e| {
        let h = e.inner_html();
        // Drop the repeated course/lecture headings at the top.
        let h = match h.find("</h2>") {
            Some(i) if i < 600 => h[i + 5..].to_string(),
            _ => h,
        };
        common::sanitize_html(&h, Some(url))
    });
    let resources = doc
        .select(&sel(
            "#block-yale-oyc-views-block-session-resource-files-block-1",
        ))
        .next();
    Ok(Session {
        title,
        overview: Some(overview).filter(|o| !o.is_empty()),
        youtube_id,
        chapters,
        transcript_html: transcript_html.filter(|t| t.len() > 200),
        pdfs: resources.as_ref().map(pdf_links).unwrap_or_default(),
    })
}

pub async fn course(polite: &Polite, id: &str) -> AppResult<Course> {
    let html = polite.text(&format!("{BASE}/{id}")).await.map_err(|e| {
        if e.message.contains("404") {
            AppError::not_found("that Open Yale course")
        } else {
            e
        }
    })?;
    parse_course(id, &html)
}

pub fn preview_of(c: &Course) -> ProviderPreview {
    let lectures = c
        .sessions
        .iter()
        .filter(|s| s.label.starts_with("Lecture"))
        .count() as i32;
    ProviderPreview {
        card: ProviderCard {
            id: c.id.clone(),
            title: c.title.clone(),
            subtitle: Some([c.number.clone(), c.professor.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ")),
            description: c.about_html.as_deref().map(common::text_of),
            license: Some(LICENSE.into()),
            license_url: Some(LICENSE_URL.into()),
            url: format!("{BASE}/{}", c.id),
            ..Default::default()
        },
        outline: modules_of(&c.sessions)
            .into_iter()
            .map(|(title, ss)| OutlineModule {
                title,
                items: ss.iter().map(|s| format!("{}: {}", s.label, s.title)).collect(),
            })
            .collect(),
        counts: ProviderCounts {
            lessons: c.sessions.len() as i32,
            videos: lectures,
            transcripts: lectures,
            pdfs: c.pdfs.len() as i32,
            ..Default::default()
        },
        notes: vec![
            "Lectures stream from YouTube, with chapter markers and Yale's full transcript (searchable).".into(),
            "Handouts, problem sets and exams (PDF) are downloaded.".into(),
            format!("Each of the {} session pages is fetched a second apart, so the import takes a minute or two.", c.sessions.len()),
        ],
        est_bytes: None,
        pdf_bytes: None,
    }
}

/// Groups sessions into modules that end at each exam ("Part 1: Lecture 1–7, Exam 1").
pub fn modules_of(sessions: &[SessionRef]) -> Vec<(String, Vec<&SessionRef>)> {
    let mut out: Vec<Vec<&SessionRef>> = vec![vec![]];
    for s in sessions {
        out.last_mut().unwrap().push(s);
        if s.label.starts_with("Exam") {
            out.push(vec![]);
        }
    }
    out.retain(|m| !m.is_empty());
    let n = out.len();
    out.into_iter()
        .enumerate()
        .map(|(i, m)| {
            let lec: Vec<&str> = m
                .iter()
                .filter_map(|s| s.label.strip_prefix("Lecture "))
                .collect();
            let range = match (lec.first(), lec.last()) {
                (Some(a), Some(b)) if a != b => format!("Lectures {a}–{b}"),
                (Some(a), _) => format!("Lecture {a}"),
                _ => "Sessions".into(),
            };
            let title = if n == 1 {
                range
            } else {
                format!("Part {}: {range}", i + 1)
            };
            (title, m)
        })
        .collect()
}

fn pdf_kind(title: &str) -> &'static str {
    let t = title.to_lowercase();
    if t.contains("problem set") || t.contains("assignment") || t.contains("homework") {
        "assignment"
    } else if t.contains("exam")
        || t.contains("midterm")
        || t.contains("final")
        || t.contains("quiz")
    {
        "exam"
    } else if t.contains("reading") || t.contains("paper") {
        "reading"
    } else {
        "lecture_notes"
    }
}

/// Fetches every session page (1 s apart) and their PDFs into `root/pdf`, then builds the plan.
pub async fn plan(
    polite: &Arc<Polite>,
    c: &Course,
    root: &std::path::Path,
    on: OnProgress<'_>,
) -> AppResult<Plan> {
    let total = c.sessions.len() as f64;
    let mut sessions = vec![];
    for (i, s) in c.sessions.iter().enumerate() {
        on(
            "download",
            i as f64,
            total,
            &format!("Reading session pages ({} of {})…", i + 1, c.sessions.len()),
        );
        let parsed = match polite.text(&s.url).await {
            Ok(h) => parse_session(&h, &s.url).ok(),
            Err(e) => {
                log::warn!("Yale session {} skipped: {e}", s.url);
                None
            }
        };
        sessions.push(parsed);
    }
    let mut pdfs: Vec<(String, String)> = c.pdfs.clone();
    for s in sessions.iter().flatten() {
        pdfs.extend(s.pdfs.iter().cloned());
    }
    let mut stored = std::collections::HashMap::new();
    let n = pdfs.len() as f64;
    for (i, (_, u)) in pdfs.iter().enumerate() {
        if stored.contains_key(u) {
            continue;
        }
        on("download", i as f64, n, "Downloading handouts and exams…");
        let name = u.rsplit('/').next().unwrap_or("file.pdf");
        let rel = format!(
            "pdf/{}",
            common::file_name(name.trim_end_matches(".pdf"), "pdf")
        );
        let quiet = |_: f64, _: f64| {};
        match polite
            .download(u, &root.join(&rel), MAX_PDF_BYTES, &quiet)
            .await
        {
            Ok(size) => {
                stored.insert(u.clone(), (rel, size as i64));
            }
            Err(e) => log::warn!("Yale PDF skipped {u}: {e}"),
        }
    }
    Ok(build_plan(c, &sessions, &stored))
}

/// Pure: course + parsed sessions (None = couldn't fetch) + stored PDFs (url → (rel, size)).
pub fn build_plan(
    c: &Course,
    sessions: &[Option<Session>],
    stored: &std::collections::HashMap<String, (String, i64)>,
) -> Plan {
    let pdf_material = |title: &str, url: &str, uid: String, prev: Option<&str>| -> PlanMaterial {
        let (local, size) = stored
            .get(url)
            .cloned()
            .map_or((None, None), |(r, s)| (Some(r), Some(s)));
        let is_solution = title.to_lowercase().contains("solution");
        let kind = pdf_kind(title);
        PlanMaterial {
            uid,
            kind: if is_solution {
                format!("{kind}_solution")
            } else {
                kind.into()
            },
            title: title.into(),
            source_url: Some(url.into()),
            file_url: Some(url.into()),
            local,
            file_type: Some("application/pdf".into()),
            file_size: size,
            solution_of: if is_solution {
                prev.map(str::to_string)
            } else {
                None
            },
            ..Default::default()
        }
    };
    let mut skipped = 0;
    let by_url: std::collections::HashMap<&str, &Session> = c
        .sessions
        .iter()
        .zip(sessions)
        .filter_map(|(r, s)| Some((r.url.as_str(), s.as_ref()?)))
        .collect();
    let modules = modules_of(&c.sessions)
        .into_iter()
        .map(|(title, refs)| PlanModule {
            title,
            lessons: refs
                .into_iter()
                .filter_map(|r| {
                    let Some(s) = by_url.get(r.url.as_str()) else {
                        skipped += 1;
                        return None;
                    };
                    let lesson_title = format!("{}: {}", r.label, r.title);
                    let mut materials = vec![];
                    let mut last_base: Option<String> = None;
                    for (t, u) in &s.pdfs {
                        let uid = format!("pdf:{u}");
                        let m = pdf_material(t, u, uid.clone(), last_base.as_deref());
                        if m.solution_of.is_none() {
                            last_base = Some(uid);
                        }
                        materials.push(m);
                    }
                    let body = match &s.youtube_id {
                        Some(yt) => {
                            if let Some(t) = &s.transcript_html {
                                materials.insert(
                                    0,
                                    PlanMaterial {
                                        uid: format!("transcript:{}", r.url),
                                        kind: "transcript".into(),
                                        title: format!("Transcript: {}", r.title),
                                        source_url: Some(r.url.clone()),
                                        text: Some(common::text_of(t)),
                                        html: Some(t.clone()),
                                        ..Default::default()
                                    },
                                );
                            }
                            LessonBody::Video {
                                youtube_id: yt.clone(),
                                channel: "YaleCourses".into(),
                                cues: vec![],
                                chapters: s.chapters.clone(),
                            }
                        }
                        None => LessonBody::Reading,
                    };
                    if matches!(body, LessonBody::Reading)
                        && materials.is_empty()
                        && s.overview.is_none()
                    {
                        skipped += 1;
                        return None;
                    }
                    Some(PlanLesson {
                        title: lesson_title,
                        description: s.overview.clone(),
                        url: Some(r.url.clone()),
                        body,
                        materials,
                    })
                })
                .collect(),
        })
        .filter(|m| !m.lessons.is_empty())
        .collect();
    let url = format!("{BASE}/{}", c.id);
    let mut pages = vec![];
    if let Some(h) = &c.about_html {
        pages.push(PlanPage::new(
            "page",
            "About the course",
            h.clone(),
            Some(url.clone()),
        ));
    }
    if let Some(h) = &c.syllabus_html {
        pages.push(PlanPage::new(
            "syllabus",
            "Syllabus",
            h.clone(),
            Some(url.clone()),
        ));
    }
    let course_materials = c
        .pdfs
        .iter()
        .map(|(t, u)| pdf_material(t, u, format!("pdf:{u}"), None))
        // A course PDF that's also a session handout is already attached to that session.
        .filter(|m| {
            !sessions
                .iter()
                .flatten()
                .any(|s| s.pdfs.iter().any(|(_, u)| Some(u) == m.source_url.as_ref()))
        })
        .collect();
    let title = match &c.number {
        Some(n) => format!("{n}: {}", c.title),
        None => c.title.clone(),
    };
    Plan {
        provider: Some(Provider::Yale),
        provider_ref: c.id.clone(),
        title,
        description: c.about_html.as_deref().map(common::text_of),
        instructor: c.professor.clone(),
        source_url: url.clone(),
        cover: None,
        tags: ["Open Yale Courses".to_string()].into_iter().chain(c.number.clone()).collect(),
        meta: ProviderMeta {
            provider: Provider::Yale.as_str().into(),
            provider_label: Provider::Yale.label().into(),
            title: c.title.clone(),
            url,
            license: Some(LICENSE.into()),
            license_url: Some(LICENSE_URL.into()),
            attribution: Some(
                "Open Yale Courses, Yale University. Third-party content noted in a lecture's credits isn't covered by the Creative Commons license.".into(),
            ),
            instructors: c.professor.iter().cloned().collect(),
            institution: Some("Yale University".into()),
            description_html: c.about_html.clone(),
            facts: c
                .structure
                .iter()
                .map(|s| Fact { label: "Course structure".into(), value: s.clone() })
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
    use std::collections::HashMap;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/src/services/fixtures/providers/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn course_list() {
        let cs = parse_courses(&fixture("yale_courses.html")).unwrap();
        assert!(cs.len() >= 5);
        let a = cs.iter().find(|c| c.id == "astronomy/astr-160").unwrap();
        assert_eq!(a.number, "ASTR 160");
        assert_eq!(a.title, "Frontiers and Controversies in Astrophysics");
        assert_eq!(a.professor, "Charles Bailyn");
        assert_eq!(a.semester, "Spring 2007");
        assert_eq!(card(a).license.as_deref(), Some(LICENSE));
        assert_eq!(
            parse_id("https://oyc.yale.edu/astronomy/astr-160/lecture-1").as_deref(),
            Some("astronomy/astr-160")
        );
        assert_eq!(parse_id("../../etc"), None);
    }

    #[test]
    fn course_page_sessions_and_modules() {
        let c = parse_course("astronomy/astr-160", &fixture("yale_course.html")).unwrap();
        assert_eq!(c.title, "Frontiers and Controversies in Astrophysics");
        assert_eq!(c.number.as_deref(), Some("ASTR 160"));
        assert!(
            c.professor
                .as_deref()
                .unwrap()
                .starts_with("Charles Bailyn")
        );
        assert_eq!(c.sessions.len(), 31);
        assert_eq!(c.sessions[0].label, "Lecture 1");
        assert_eq!(
            c.sessions[0].url,
            "https://oyc.yale.edu/astronomy/astr-160/lecture-1"
        );
        assert!(
            c.pdfs.iter().any(|(_, u)| u
                == "https://oyc.yale.edu/sites/default/files/problem_sets_instructions.pdf")
        );
        assert!(
            c.about_html
                .as_deref()
                .unwrap()
                .contains("Extra-Solar Planets")
        );
        let mods = modules_of(&c.sessions);
        assert_eq!(mods.len(), 3);
        assert_eq!(mods[0].0, "Part 1: Lectures 1–7");
        assert!(mods[0].1.last().unwrap().label.starts_with("Exam 1"));
    }

    #[test]
    fn session_page_video_chapters_transcript_pdfs() {
        let url = "https://oyc.yale.edu/astronomy/astr-160/lecture-1";
        let s = parse_session(&fixture("yale_lecture.html"), url).unwrap();
        assert_eq!(s.youtube_id.as_deref(), Some("nzmFc2gjUo4"));
        assert_eq!(s.chapters.len(), 6);
        assert_eq!(
            (s.chapters[1].start, s.chapters[1].end),
            (338.0, Some(777.0))
        );
        assert_eq!(s.chapters[1].title, "Topics of the Course");
        let t = s.transcript_html.as_deref().unwrap();
        assert!(
            t.contains("Professor Charles Bailyn")
                && !t.contains("<script")
                && !t.contains("transcript-top")
        );
        assert_eq!(
            s.pdfs,
            vec![(
                "Class Notes - Lecture 1".to_string(),
                "https://oyc.yale.edu/sites/default/files/astr160_lec1_0.pdf".to_string()
            )]
        );
        assert!(
            s.overview
                .as_deref()
                .unwrap()
                .starts_with("Professor Bailyn introduces")
        );

        let exam = parse_session(
            &fixture("yale_exam.html"),
            "https://oyc.yale.edu/astronomy/astr-160/exam-1",
        )
        .unwrap();
        assert!(exam.youtube_id.is_none());
        assert_eq!(exam.pdfs.len(), 2);

        let c = parse_course("astronomy/astr-160", &fixture("yale_course.html")).unwrap();
        let mut sessions = vec![None; c.sessions.len()];
        sessions[0] = Some(s);
        let ei = c
            .sessions
            .iter()
            .position(|r| r.label.starts_with("Exam 1"))
            .unwrap();
        sessions[ei] = Some(exam);
        let stored = HashMap::from([(
            "https://oyc.yale.edu/sites/default/files/astr160_lec1_0.pdf".to_string(),
            ("pdf/astr160_lec1_0.pdf".to_string(), 1234),
        )]);
        let p = build_plan(&c, &sessions, &stored);
        assert_eq!(p.lesson_count(), 2);
        assert_eq!(p.skipped as usize, c.sessions.len() - 2);
        let l1 = &p.modules[0].lessons[0];
        assert_eq!(l1.title, "Lecture 1: Introduction");
        assert!(matches!(&l1.body, LessonBody::Video { chapters, .. } if chapters.len() == 6));
        assert_eq!(l1.materials[0].kind, "transcript");
        assert_eq!(
            l1.materials[1].local.as_deref(),
            Some("pdf/astr160_lec1_0.pdf")
        );
        let ex = &p.modules[0].lessons[1];
        assert!(matches!(ex.body, LessonBody::Reading));
        assert_eq!(ex.materials[0].kind, "exam");
        assert_eq!(ex.materials[1].kind, "exam_solution");
        assert_eq!(
            ex.materials[1].solution_of.as_deref(),
            Some(ex.materials[0].uid.as_str())
        );
        assert!(p.title.starts_with("ASTR 160: "));
        assert!(p.pages.iter().any(|pg| pg.kind == "syllabus"));
    }
}
