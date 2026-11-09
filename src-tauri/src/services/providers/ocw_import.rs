//! Turns an extracted OCW course folder into a Lectern course: modules from the calendar,
//! video lessons (with caption transcripts), reading lessons for documents, materials with
//! local PDF paths and online links, syllabus/calendar pages, and provider metadata.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::{Connection, params};

use crate::db::models::*;
use crate::db::repo::{courses, import as import_repo, lessons, materials, modules, transcripts};
use crate::error::{AppError, AppResult};
use crate::services::import::ImportedVideo;
use crate::services::providers::ocw::{
    self, CalendarRow, MaterialKind, OcwMeta, OcwResource, PlanItem, Table,
};

/// Everything read from an extracted course folder.
pub struct Loaded {
    pub root: PathBuf,
    pub meta: OcwMeta,
    pub resources: Vec<OcwResource>,
    pub calendar: Vec<CalendarRow>,
    pub gallery: Vec<String>,
    pub tables: Vec<Table>,
    /// (section dir, title, html)
    pub pages: Vec<(String, String, String)>,
}

fn read(p: &Path) -> AppResult<String> {
    std::fs::read_to_string(p)
        .map_err(|e| AppError::internal(format!("reading {}: {e}", p.display())))
}

pub fn load(root: &Path, slug: &str) -> AppResult<Loaded> {
    let meta = ocw::parse_meta(&read(&root.join("data.json"))?, slug)?;
    let mut resources = vec![];
    if let Ok(dirs) = std::fs::read_dir(root.join("resources")) {
        let mut dirs: Vec<_> = dirs.filter_map(Result::ok).map(|e| e.path()).collect();
        dirs.sort();
        for d in dirs {
            let f = d.join("data.json");
            let name = d
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            if let Ok(json) = std::fs::read_to_string(&f)
                && let Ok(r) = ocw::parse_resource(&name, &json)
            {
                resources.push(r);
            }
        }
    }
    let mut pages = vec![];
    let mut tables = vec![];
    let mut calendar = vec![];
    if let Ok(dirs) = std::fs::read_dir(root.join("pages")) {
        let mut dirs: Vec<_> = dirs
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for d in dirs {
            let name = d
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            let Ok(json) = std::fs::read_to_string(d.join("data.json")) else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&json) else {
                continue;
            };
            let html = v
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_string();
            let title = v
                .get("title")
                .and_then(|c| c.as_str())
                .unwrap_or(&name)
                .to_string();
            if name == "calendar" {
                calendar = ocw::parse_calendar(&html);
            }
            tables.extend(ocw::parse_tables(&html));
            if !html.trim().is_empty() {
                pages.push((name, title, html));
            }
        }
    }
    let mut gallery = vec![];
    if let Ok(dirs) = std::fs::read_dir(root.join("video_galleries")) {
        for d in dirs
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
        {
            if let Ok(json) = std::fs::read_to_string(d.join("data.json")) {
                gallery.extend(ocw::gallery_order(&json));
            }
        }
    }
    Ok(Loaded {
        root: root.to_path_buf(),
        meta,
        resources,
        calendar,
        gallery,
        tables,
        pages,
    })
}

static RES_HREF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"href="(?:\.\./)*resources/([a-z0-9_.\-]+)/?(?:index\.html)?""#).unwrap()
});
static PAGE_HREF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"href="(?:\.\./)*pages/([a-z0-9_.\-]+)/?(?:index\.html)?""#).unwrap()
});
static REL_HREF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"href="((?:\.\./|\./)[^"]*)""#).unwrap());
/// Sanitises page HTML and rewrites links: same-course resources → `#material:<id>`, section
/// pages → `#page:<section>`, other relative links → absolute OCW URLs (opened in the browser).
pub fn rewrite_html(html: &str, slug: &str, material_ids: &HashMap<String, String>) -> String {
    let s = RES_HREF.replace_all(html, |c: &regex::Captures<'_>| {
        match material_ids.get(&c[1]) {
            Some(id) => format!("href=\"#material:{id}\""),
            None => format!("href=\"{}/courses/{slug}/resources/{}/\"", ocw::BASE, &c[1]),
        }
    });
    let s = PAGE_HREF.replace_all(&s, |c: &regex::Captures<'_>| {
        format!("href=\"#page:{}\"", &c[1])
    });
    let s = REL_HREF
        .replace_all(&s, |c: &regex::Captures<'_>| {
            let rel = c[1]
                .trim_start_matches("./")
                .trim_start_matches("../")
                .trim_start_matches("../");
            format!(
                "href=\"{}/courses/{slug}/{}\"",
                ocw::BASE,
                rel.trim_start_matches("../")
            )
        })
        .into_owned();
    super::common::sanitize_html(&s, Some(&format!("{}/courses/{slug}/", ocw::BASE)))
}

/// Duration from the last caption cue (OCW metadata has no video lengths).
fn duration_of(cues: &[crate::services::vtt::Cue]) -> Option<i32> {
    cues.iter()
        .map(|c| c.end)
        .fold(None, |m: Option<f64>, e| Some(m.map_or(e, |m| m.max(e))))
        .map(|d| d.ceil() as i32)
}

pub struct Written {
    pub course_id: String,
    /// (material id, absolute PDF path) to index after the transaction.
    pub pdfs: Vec<(String, PathBuf)>,
    pub lessons: i32,
    pub materials: i32,
    pub transcripts: i32,
}

/// Creates the course. `root` must already be the course's final media folder.
pub fn write(conn: &Connection, l: &Loaded) -> AppResult<Written> {
    let structure = ocw::plan_structure(&l.resources, &l.calendar, &l.gallery, &l.tables);
    let topics = ocw::row_topics(&l.tables);
    let path_of =
        |zip: &Option<String>| zip.as_ref().map(|z| l.root.join(z)).filter(|p| p.is_file());

    let mut tags: Vec<String> = vec!["MIT OCW".into()];
    tags.extend(l.meta.course_number.clone());
    tags.extend(l.meta.level.iter().cloned());
    tags.extend(
        l.meta
            .topics
            .iter()
            .filter_map(|t| t.rsplit(" > ").next().map(str::to_string))
            .take(3),
    );
    let cover = l
        .meta
        .image_path
        .as_ref()
        .map(|p| l.root.join(p))
        .filter(|p| p.is_file());
    let title = match &l.meta.term {
        Some(t) => format!("{} ({t})", l.meta.title),
        None => l.meta.title.clone(),
    };

    let course = courses::create_course(
        conn,
        &NewCourse {
            title,
            description: l.meta.description.clone(),
            instructor: (!l.meta.instructors.is_empty()).then(|| l.meta.instructors.join(", ")),
            source_url: Some(l.meta.url.clone()),
            cover_path: cover.map(|p| p.to_string_lossy().into_owned()),
            tags,
        },
        None,
    )?;
    // Repos open their own transactions and SQLite can't nest them, so instead of one big
    // transaction a failure part-way deletes the half-built course.
    match fill(conn, l, &course, &structure, &topics, &path_of) {
        Ok(w) => Ok(w),
        Err(e) => {
            let _ = courses::delete_course(conn, &course.id);
            Err(e)
        }
    }
}

fn fill(
    tx: &Connection,
    l: &Loaded,
    course: &Course,
    structure: &ocw::Structure,
    topics: &HashMap<String, String>,
    path_of: &dyn Fn(&Option<String>) -> Option<PathBuf>,
) -> AppResult<Written> {
    let slug = &l.meta.slug;
    tx.execute(
        "UPDATE courses SET provider = 'mit_ocw', provider_ref = ?2 WHERE id = ?1",
        params![course.id, slug],
    )?;
    materials::set_meta(tx, &course.id, &l.meta)?;

    let mut lesson_of: HashMap<usize, String> = HashMap::new();
    let mut n_lessons = 0;
    let mut n_transcripts = 0;
    for m in &structure.modules {
        let module = modules::create_module(tx, &course.id, &m.title)?;
        for item in &m.items {
            match *item {
                PlanItem::Video(i) => {
                    let r = &l.resources[i];
                    let Some(yt) = r.youtube_id.clone() else {
                        continue;
                    };
                    let cues = path_of(&r.caption_zip_path)
                        .and_then(|p| std::fs::read_to_string(p).ok())
                        .map(|t| crate::services::vtt::parse(&t, false))
                        .unwrap_or_default();
                    let v = ImportedVideo {
                        video_id: yt,
                        title: r.title.clone(),
                        description: r.description.clone(),
                        channel_title: Some("MIT OpenCourseWare".into()),
                        duration_sec: duration_of(&cues),
                        license: Some(License::CreativeCommon),
                    };
                    let Ok(lesson) = import_repo::add_video(tx, &module.id, &v) else {
                        continue;
                    };
                    if !cues.is_empty() {
                        transcripts::replace(tx, &lesson.id, "manual", Some("en"), &cues, &[])?;
                        n_transcripts += 1;
                    }
                    lesson_of.insert(i, lesson.id);
                    n_lessons += 1;
                }
                PlanItem::Doc(i) => {
                    let r = &l.resources[i];
                    let lesson = lessons::create_lesson(
                        tx,
                        &NewLesson {
                            module_id: module.id.clone(),
                            lesson_type: LessonType::Reading,
                            title: r.title.clone(),
                            description: topics
                                .get(&r.dir)
                                .cloned()
                                .or_else(|| r.description.clone()),
                            url: Some(r.source_url(slug)),
                        },
                    )?;
                    lesson_of.insert(i, lesson.id);
                    n_lessons += 1;
                }
            }
        }
    }
    for (&notes, &video) in &structure.notes_for_video {
        if let Some(lid) = lesson_of.get(&video).cloned() {
            lesson_of.insert(notes, lid);
        }
    }

    // Materials for every file/video (captions, 3Play transcripts and images are internal).
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut by_index: HashMap<usize, String> = HashMap::new();
    let mut pdfs = vec![];
    let mut n_mat = 0;
    for (i, r) in l.resources.iter().enumerate() {
        let kind = r.kind();
        if matches!(
            kind,
            MaterialKind::Caption | MaterialKind::Transcript | MaterialKind::Image
        ) {
            continue;
        }
        let local = path_of(&r.zip_path);
        let id = materials::insert(
            tx,
            &NewMaterial {
                course_id: course.id.clone(),
                lesson_id: lesson_of.get(&i).cloned(),
                provider_uid: Some(r.uid.clone()),
                kind: if r.is_solution() {
                    format!("{}_solution", kind.as_str())
                } else {
                    kind.as_str().into()
                },
                title: r.title.clone(),
                section: r.section.clone(),
                description: topics
                    .get(&r.dir)
                    .cloned()
                    .or_else(|| r.description.clone()),
                source_url: Some(r.source_url(slug)),
                file_url: r.file_url.clone().or_else(|| r.archive_url.clone()),
                local_path: local.as_ref().map(|p| p.to_string_lossy().into_owned()),
                file_type: r.file_type.clone(),
                file_size: r.file_size,
                position: i as i32,
                ..Default::default()
            },
        )?;
        if r.is_pdf()
            && let Some(p) = local
        {
            pdfs.push((id.clone(), p));
        }
        ids.insert(r.dir.clone(), id.clone());
        by_index.insert(i, id);
        n_mat += 1;
    }
    for (&sol, &base) in &structure.solutions {
        if let (Some(s), Some(b)) = (by_index.get(&sol), by_index.get(&base)) {
            materials::set_solution_of(tx, s, b, lesson_of.get(&base).map(String::as_str))?;
        }
    }

    // Section pages (syllabus, calendar, readings…), with links rewritten.
    for (pos, (name, title, html)) in l.pages.iter().enumerate() {
        let kind = match name.as_str() {
            "syllabus" => "syllabus",
            "calendar" => "calendar",
            _ => "page",
        };
        materials::insert(
            tx,
            &NewMaterial {
                course_id: course.id.clone(),
                provider_uid: Some(format!("page:{name}")),
                kind: kind.into(),
                title: title.clone(),
                section: Some(name.clone()),
                source_url: Some(format!("{}/courses/{slug}/pages/{name}/", ocw::BASE)),
                html: Some(rewrite_html(html, slug, &ids)),
                position: -1000 + pos as i32,
                ..Default::default()
            },
        )?;
    }
    Ok(Written {
        course_id: course.id.clone(),
        pdfs,
        lessons: n_lessons,
        materials: n_mat,
        transcripts: n_transcripts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::services::providers::ocw::tests::{CALENDAR, EXAMS, SLUG, doc, meta_json, video};

    fn fixture(dir: &Path) {
        let w = |p: &str, s: &str| {
            let f = dir.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, s).unwrap();
        };
        w("data.json", &meta_json());
        w("static_resources/42ba_6-042jf10.jpg", "jpg");
        for (d, j) in [
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
                "mit6_042jf10_mid_practice",
                "Midterm Practice Problems",
                "Exams",
                &["Exams"],
            ),
            doc(
                "mit6_042jf10_mid_pctce_sol",
                "Midterm Practice Problems Solutions",
                "Exams",
                &["Exam Solutions"],
            ),
        ] {
            w(&format!("resources/{d}/data.json"), &j);
        }
        w(
            "static_resources/hash1_L3LMbpZIKhQ.vtt",
            "WEBVTT\n\n00:00:01.000 --> 00:00:04.000\nWelcome to 6.042\n\n01:19:00.000 --> 01:19:34.500\nSee you next time\n",
        );
        w("static_resources/h_MIT6_042JF10_ASSN01.pdf", "%PDF-fake");
        w(
            "pages/calendar/data.json",
            &serde_json::json!({"title": "Calendar", "content": CALENDAR}).to_string(),
        );
        w(
            "pages/exams/data.json",
            &serde_json::json!({"title": "Exams", "content": EXAMS}).to_string(),
        );
        w(
            "pages/syllabus/data.json",
            &serde_json::json!({"title": "Syllabus", "content": "<p>Prereq <a href=\"https://ocw.mit.edu/courses/18-01\">18.01</a>. See <a href=\"../../resources/mit6_042jf10_assn01/index.html\">PS1</a> and <a href=\"../../pages/calendar/index.html\">calendar</a>.</p><script>alert(1)</script>"}).to_string(),
        );
    }

    #[test]
    fn imports_fixture_course() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let loaded = load(dir.path(), SLUG).unwrap();
        assert_eq!(loaded.calendar.len(), 4);
        let conn = open_in_memory().unwrap();
        let w = write(&conn, &loaded).unwrap();
        assert_eq!(w.transcripts, 1);
        assert_eq!(w.pdfs.len(), 1, "only PDFs present on disk are indexed");

        let detail = courses::get_course_detail(&conn, &w.course_id).unwrap();
        assert_eq!(
            detail.course.title,
            "Mathematics for Computer Science (Fall 2010)"
        );
        assert!(detail.course.tags.contains(&"6.042J".to_string()));
        assert!(
            detail
                .course
                .cover_path
                .as_deref()
                .is_some_and(|p| p.ends_with(".jpg"))
        );
        let mods: Vec<&str> = detail
            .modules
            .iter()
            .map(|m| m.module.title.as_str())
            .collect();
        assert_eq!(mods[0], "Lectures 1–2: Introduction and proofs, Induction");
        assert!(mods.contains(&"Exams"));
        let first: Vec<&str> = detail.modules[0]
            .lessons
            .iter()
            .map(|l| l.title.as_str())
            .collect();
        assert_eq!(
            first,
            vec![
                "Lecture 1: Introduction and Proofs",
                "Lecture 2: Induction",
                "Problem Set 1"
            ]
        );
        let l1 = &detail.modules[0].lessons[0];
        assert_eq!(l1.youtube_video_id.as_deref(), Some("L3LMbpZIKhQ"));
        assert_eq!(l1.duration_sec, Some(4775), "duration from last caption");
        assert_eq!(l1.license, Some(License::CreativeCommon));
        assert_eq!(
            transcripts::get_transcript(&conn, &l1.id)
                .unwrap()
                .cues
                .len(),
            2
        );

        // Solutions hang off their problem set's lesson.
        let ps2 = detail.modules[1]
            .lessons
            .iter()
            .find(|l| l.title == "Problem Set 2")
            .unwrap();
        let mats = materials::list_for_lesson(&conn, &ps2.id).unwrap();
        assert_eq!(mats.len(), 2);
        assert!(mats[1].solution_of.as_deref() == Some(mats[0].id.as_str()));
        assert_eq!(mats[1].kind, "assignment_solution");

        let info = materials::course_info(&conn, &w.course_id).unwrap();
        assert_eq!(info.provider, "mit_ocw");
        assert_eq!(info.meta.unwrap().instructors.len(), 2);
        let syl = materials::html(&conn, info.syllabus_id.as_deref().unwrap())
            .unwrap()
            .unwrap();
        assert!(syl.contains("href=\"#material:") && syl.contains("href=\"#page:calendar\""));
        assert!(syl.contains("https://ocw.mit.edu/courses/18-01") && !syl.contains("<script"));
    }

    #[test]
    fn link_rewriting() {
        let ids = HashMap::from([("a".to_string(), "M1".to_string())]);
        let h = rewrite_html(
            r#"<a href="../../resources/a/index.html">x</a><a href="../../resources/b/">y</a><a onclick="evil()" href="./download/">z</a>"#,
            SLUG,
            &ids,
        );
        assert!(h.contains("href=\"#material:M1\""));
        assert!(h.contains(&format!(
            "href=\"https://ocw.mit.edu/courses/{SLUG}/resources/b/\""
        )));
        assert!(h.contains(&format!(
            "href=\"https://ocw.mit.edu/courses/{SLUG}/download/\""
        )));
        assert!(!h.contains("onclick"));
    }

    /// Real course: `OCW_DIR=/path/to/extracted cargo test --lib -- --ignored ocw_real_folder --nocapture`
    #[test]
    #[ignore]
    fn ocw_real_folder() {
        let dir = std::env::var("OCW_DIR").expect("OCW_DIR");
        let loaded = load(Path::new(&dir), SLUG).unwrap();
        let conn = open_in_memory().unwrap();
        let w = write(&conn, &loaded).unwrap();
        let d = courses::get_course_detail(&conn, &w.course_id).unwrap();
        for m in &d.modules {
            println!("{} ({} lessons)", m.module.title, m.lessons.len());
        }
        println!(
            "lessons={} materials={} transcripts={} pdfs={}",
            w.lessons,
            w.materials,
            w.transcripts,
            w.pdfs.len()
        );
        for m in d
            .modules
            .iter()
            .filter(|m| m.module.title == "Other Materials")
        {
            for l in &m.lessons {
                println!("  other: {}", l.title);
            }
        }
        assert!(w.transcripts >= 20 && w.pdfs.len() >= 80);
        let (id, p) = &w.pdfs[0];
        let pages = crate::services::pdf_text::extract_in_process(p).unwrap();
        println!(
            "{id}: {} pages, first: {}",
            pages.len(),
            crate::services::ai_context::truncate(pages[0].trim(), 120)
        );
    }
}
