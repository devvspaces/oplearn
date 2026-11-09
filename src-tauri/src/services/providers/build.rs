//! Writes a provider's `Plan` as a Lectern course: modules, video lessons (with transcripts and
//! chapters), reading lessons, materials (local files, HTML, searchable text) and About pages.
//! Providers only produce the plan; this is the one place that touches the database.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

use super::{Provider, ProviderMeta};
use crate::db::models::*;
use crate::db::repo::{courses, import as import_repo, lessons, materials, modules, transcripts};
use crate::error::AppResult;
use crate::services::import::ImportedVideo;
use crate::services::vtt::Cue;
use crate::services::ytdlp::ChapterInfo;

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub provider: Option<Provider>,
    pub provider_ref: String,
    pub title: String,
    pub description: Option<String>,
    pub instructor: Option<String>,
    pub source_url: String,
    /// Cover image, relative to the import folder.
    pub cover: Option<String>,
    pub tags: Vec<String>,
    pub meta: ProviderMeta,
    pub modules: Vec<PlanModule>,
    /// About pages (syllabus, overview…).
    pub pages: Vec<PlanPage>,
    /// Course-wide materials not tied to a lesson.
    pub course_materials: Vec<PlanMaterial>,
    /// Lessons the provider listed that couldn't be used.
    pub skipped: i32,
}

#[derive(Debug, Clone, Default)]
pub struct PlanModule {
    pub title: String,
    pub lessons: Vec<PlanLesson>,
}

#[derive(Debug, Clone)]
pub enum LessonBody {
    Video {
        youtube_id: String,
        channel: String,
        cues: Vec<Cue>,
        chapters: Vec<ChapterInfo>,
    },
    Reading,
}

#[derive(Debug, Clone)]
pub struct PlanLesson {
    pub title: String,
    pub description: Option<String>,
    /// Online page for the lesson.
    pub url: Option<String>,
    pub body: LessonBody,
    /// The first non-solution material of a reading lesson is what the lesson shows.
    pub materials: Vec<PlanMaterial>,
}

#[derive(Debug, Clone, Default)]
pub struct PlanMaterial {
    /// Unique within the course (`provider_uid`).
    pub uid: String,
    pub kind: String,
    pub title: String,
    pub section: Option<String>,
    pub description: Option<String>,
    pub source_url: Option<String>,
    pub file_url: Option<String>,
    /// Local file, relative to the import folder.
    pub local: Option<String>,
    pub file_type: Option<String>,
    pub file_size: Option<i64>,
    /// Sanitised HTML (shown in the app).
    pub html: Option<String>,
    /// Searchable text, stored as one page (HTML materials).
    pub text: Option<String>,
    /// `uid` of the material this one solves.
    pub solution_of: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PlanPage {
    /// syllabus | page
    pub kind: String,
    pub title: String,
    pub html: String,
    pub source_url: Option<String>,
}

impl PlanPage {
    pub fn new(kind: &str, title: &str, html: String, source_url: Option<String>) -> Self {
        Self {
            kind: kind.into(),
            title: title.into(),
            html,
            source_url,
        }
    }
}

impl Plan {
    pub fn lesson_count(&self) -> usize {
        self.modules.iter().map(|m| m.lessons.len()).sum()
    }
}

#[derive(Debug)]
pub struct Written {
    pub course_id: String,
    /// (material id, absolute PDF path, still under the import folder) to index.
    pub pdfs: Vec<(String, PathBuf)>,
    pub lessons: i32,
    pub materials: i32,
    pub transcripts: i32,
}

/// Duration from the last caption cue.
pub fn duration_of(cues: &[Cue]) -> Option<i32> {
    cues.iter()
        .map(|c| c.end)
        .fold(None, |m: Option<f64>, e| Some(m.map_or(e, |m| m.max(e))))
        .map(|d| d.ceil() as i32)
}

/// Creates the course. `root` is the import folder that the plan's relative paths point into.
/// Repos open their own transactions and SQLite can't nest them, so a failure part-way deletes
/// the half-built course instead of rolling back.
pub fn write(conn: &Connection, plan: &Plan, root: &Path) -> AppResult<Written> {
    let course = courses::create_course(
        conn,
        &NewCourse {
            title: plan.title.clone(),
            description: plan.description.clone(),
            instructor: plan.instructor.clone(),
            source_url: Some(plan.source_url.clone()),
            cover_path: plan
                .cover
                .as_ref()
                .map(|c| root.join(c))
                .filter(|p| p.is_file())
                .map(|p| p.to_string_lossy().into_owned()),
            tags: plan.tags.clone(),
        },
        None,
    )?;
    match fill(conn, plan, root, &course.id) {
        Ok(w) => Ok(w),
        Err(e) => {
            let _ = courses::delete_course(conn, &course.id);
            Err(e)
        }
    }
}

fn fill(conn: &Connection, plan: &Plan, root: &Path, course_id: &str) -> AppResult<Written> {
    let provider = plan.provider.map(|p| p.as_str()).unwrap_or("youtube");
    conn.execute(
        "UPDATE courses SET provider = ?2, provider_ref = ?3 WHERE id = ?1",
        params![course_id, provider, plan.provider_ref],
    )?;
    materials::set_provider_meta(conn, course_id, &plan.meta)?;

    let mut w = Written {
        course_id: course_id.into(),
        pdfs: vec![],
        lessons: 0,
        materials: 0,
        transcripts: 0,
    };
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut solutions: Vec<(String, String, Option<String>)> = vec![];
    let mut position = 0;
    let mut add = |conn: &Connection,
                   m: &PlanMaterial,
                   lesson_id: Option<&str>,
                   w: &mut Written,
                   position: &mut i32|
     -> AppResult<()> {
        let local = m
            .local
            .as_ref()
            .map(|l| root.join(l))
            .filter(|p| p.is_file());
        let id = materials::insert(
            conn,
            &NewMaterial {
                course_id: course_id.into(),
                lesson_id: lesson_id.map(str::to_string),
                provider_uid: Some(m.uid.clone()),
                kind: m.kind.clone(),
                title: m.title.clone(),
                section: m.section.clone(),
                description: m.description.clone(),
                source_url: m.source_url.clone(),
                file_url: m.file_url.clone(),
                local_path: local.as_ref().map(|p| p.to_string_lossy().into_owned()),
                file_type: m.file_type.clone(),
                file_size: m.file_size,
                html: m.html.clone(),
                position: *position,
                ..Default::default()
            },
        )?;
        *position += 1;
        if let Some(text) = m.text.as_ref().filter(|t| !t.trim().is_empty()) {
            materials::set_pages(conn, &id, std::slice::from_ref(text))?;
        }
        if m.file_type.as_deref() == Some("application/pdf")
            && let Some(p) = local
        {
            w.pdfs.push((id.clone(), p));
        }
        if let Some(base) = &m.solution_of {
            solutions.push((id.clone(), base.clone(), lesson_id.map(str::to_string)));
        }
        ids.insert(m.uid.clone(), id);
        w.materials += 1;
        Ok(())
    };

    for m in &plan.modules {
        let module = modules::create_module(conn, course_id, &m.title)?;
        for l in &m.lessons {
            let lesson = match &l.body {
                LessonBody::Video {
                    youtube_id,
                    channel,
                    cues,
                    chapters,
                } => {
                    let v = ImportedVideo {
                        video_id: youtube_id.clone(),
                        title: l.title.clone(),
                        description: l.description.clone(),
                        channel_title: Some(channel.clone()),
                        duration_sec: duration_of(cues)
                            .or_else(|| chapters.last().map(|c| c.start.ceil() as i32)),
                        license: Some(License::CreativeCommon),
                    };
                    // A video listed twice (e.g. a repeated lecture) is only imported once.
                    let Ok(lesson) = import_repo::add_video(conn, &module.id, &v) else {
                        continue;
                    };
                    if !cues.is_empty() {
                        transcripts::replace(
                            conn,
                            &lesson.id,
                            "manual",
                            Some("en"),
                            cues,
                            chapters,
                        )?;
                        w.transcripts += 1;
                    } else if !chapters.is_empty() {
                        transcripts::set_chapters(conn, &lesson.id, chapters)?;
                    }
                    lesson
                }
                LessonBody::Reading => lessons::create_lesson(
                    conn,
                    &NewLesson {
                        module_id: module.id.clone(),
                        lesson_type: LessonType::Reading,
                        title: l.title.clone(),
                        description: l.description.clone(),
                        url: l.url.clone(),
                    },
                )?,
            };
            w.lessons += 1;
            for m in &l.materials {
                add(conn, m, Some(&lesson.id), &mut w, &mut position)?;
            }
        }
    }
    for m in &plan.course_materials {
        add(conn, m, None, &mut w, &mut position)?;
    }
    for (sol, base, lesson) in &solutions {
        if let Some(b) = ids.get(base) {
            materials::set_solution_of(conn, sol, b, lesson.as_deref())?;
        }
    }
    for (i, p) in plan.pages.iter().enumerate() {
        materials::insert(
            conn,
            &NewMaterial {
                course_id: course_id.into(),
                provider_uid: Some(format!("page:{i}")),
                kind: p.kind.clone(),
                title: p.title.clone(),
                section: Some(format!("about-{i}")),
                source_url: p.source_url.clone(),
                html: Some(p.html.clone()),
                position: -1000 + i as i32,
                ..Default::default()
            },
        )?;
    }
    Ok(w)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::repo::search;

    pub fn sample_plan() -> Plan {
        let cue = |s: f64, t: &str| Cue {
            start: s,
            end: s + 5.0,
            text: t.into(),
        };
        Plan {
            provider: Some(Provider::Nptel),
            provider_ref: "106106145".into(),
            title: "Programming in Python".into(),
            description: Some("Intro".into()),
            instructor: Some("Prof. M".into()),
            source_url: "https://nptel.ac.in/courses/106106145".into(),
            tags: vec!["NPTEL".into()],
            meta: ProviderMeta {
                provider: "nptel".into(),
                title: "Programming in Python".into(),
                license: Some("CC BY-SA 4.0".into()),
                ..Default::default()
            },
            modules: vec![
                PlanModule {
                    title: "Week 1".into(),
                    lessons: vec![
                        PlanLesson {
                            title: "Lecture 1: gcd".into(),
                            description: None,
                            url: None,
                            body: LessonBody::Video {
                                youtube_id: "9MmC_uGjBsM".into(),
                                channel: "NPTEL".into(),
                                cues: vec![cue(0.0, "welcome to euclid"), cue(600.0, "the end")],
                                chapters: vec![],
                            },
                            materials: vec![],
                        },
                        PlanLesson {
                            title: "1.1 Reading".into(),
                            description: None,
                            url: Some("https://x/1.1".into()),
                            body: LessonBody::Reading,
                            materials: vec![
                                PlanMaterial {
                                    uid: "sec:1.1".into(),
                                    kind: "reading".into(),
                                    title: "1.1 Reading".into(),
                                    html: Some("<p>Pythagorean triples</p>".into()),
                                    text: Some("Pythagorean triples".into()),
                                    ..Default::default()
                                },
                                PlanMaterial {
                                    uid: "sol:1.1".into(),
                                    kind: "reading_solution".into(),
                                    title: "Answers".into(),
                                    html: Some("<p>a</p>".into()),
                                    solution_of: Some("sec:1.1".into()),
                                    ..Default::default()
                                },
                            ],
                        },
                    ],
                },
                PlanModule {
                    title: "Week 2".into(),
                    lessons: vec![PlanLesson {
                        title: "Lecture 2".into(),
                        description: None,
                        url: None,
                        body: LessonBody::Video {
                            youtube_id: "Bc__4fV94lE".into(),
                            channel: "NPTEL".into(),
                            cues: vec![],
                            chapters: vec![ChapterInfo {
                                start: 0.0,
                                end: None,
                                title: "Intro".into(),
                            }],
                        },
                        materials: vec![],
                    }],
                },
            ],
            pages: vec![PlanPage::new(
                "syllabus",
                "Syllabus",
                "<p>Weeks</p>".into(),
                None,
            )],
            ..Default::default()
        }
    }

    #[test]
    fn writes_a_plan_as_a_course() {
        let conn = open_in_memory().unwrap();
        let w = write(&conn, &sample_plan(), Path::new("/nonexistent")).unwrap();
        assert_eq!((w.lessons, w.materials, w.transcripts), (3, 2, 1));
        let (provider, pref): (String, String) = conn
            .query_row(
                "SELECT provider, provider_ref FROM courses WHERE id = ?1",
                [&w.course_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((provider.as_str(), pref.as_str()), ("nptel", "106106145"));
        let all = lessons::list_course_lessons(&conn, &w.course_id).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].duration_sec, Some(605));
        // Section text is searchable; the solution is linked to its section.
        let hits = search::global_search(&conn, "pythagorean", 10).unwrap();
        assert_eq!(hits.materials.len(), 1);
        let mats = materials::list_for_course(&conn, &w.course_id).unwrap();
        let sec = mats.iter().find(|m| m.kind == "reading").unwrap();
        let sol = mats.iter().find(|m| m.kind == "reading_solution").unwrap();
        assert_eq!(sol.solution_of.as_deref(), Some(sec.id.as_str()));
        let info = materials::course_info(&conn, &w.course_id).unwrap();
        assert_eq!(
            info.provider_meta.unwrap().license.as_deref(),
            Some("CC BY-SA 4.0")
        );
        assert!(info.meta.is_none());
        assert!(info.pages.iter().any(|p| p.kind == "syllabus"));
        let chapters: i64 = conn
            .query_row("SELECT count(*) FROM chapters", [], |r| r.get(0))
            .unwrap();
        assert_eq!(chapters, 1);
    }

    #[test]
    fn a_failure_part_way_removes_the_course() {
        let conn = open_in_memory().unwrap();
        let mut plan = sample_plan();
        // Two materials with the same uid violate the unique index.
        let dup = plan.modules[0].lessons[1].materials[0].clone();
        plan.modules[0].lessons[1].materials.push(dup);
        assert!(write(&conn, &plan, Path::new("/x")).is_err());
        let n: i64 = conn
            .query_row("SELECT count(*) FROM courses", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
}
