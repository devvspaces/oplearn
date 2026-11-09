//! Course → Markdown export (SPEC §7.5). Pure: takes loaded data, returns a string.
//! Timestamps link back to YouTube as `[12:34](https://youtu.be/<id>?t=754)`.

use std::collections::HashMap;
use std::fmt::Write;

use crate::db::models::{AnnotationInContext, AnnotationType, CourseDetail, License};

pub const ATTACHMENT_SCHEME: &str = "lectern-attachment:";

/// Rewrites `lectern-attachment:<data-relative path>` image links to `<assets_dir>/<file name>`
/// and returns the data-relative paths that must be copied there.
pub fn rewrite_attachment_links(md: &str, assets_dir: &str) -> (String, Vec<String>) {
    let re = regex::Regex::new(r"lectern-attachment:([A-Za-z0-9_./-]+)").expect("valid regex");
    let mut found = Vec::new();
    let out = re.replace_all(md, |c: &regex::Captures<'_>| {
        let rel = c[1].to_string();
        let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
        if !rel.contains("..") {
            found.push(rel);
        }
        format!("{assets_dir}/{name}")
    });
    found.dedup();
    (out.into_owned(), found)
}

pub fn fmt_ts(sec: f64) -> String {
    let s = sec.max(0.0).floor() as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn ts_link(video_id: Option<&str>, sec: Option<f64>) -> Option<String> {
    let sec = sec?;
    Some(match video_id {
        Some(id) => format!(
            "[{}](https://youtu.be/{id}?t={})",
            fmt_ts(sec),
            sec.max(0.0).floor() as u64
        ),
        None => fmt_ts(sec),
    })
}

/// Indents continuation lines so multi-line Markdown stays inside its list item.
fn indent(md: &str, pad: &str) -> String {
    let mut lines = md.trim().lines();
    let mut out = lines.next().unwrap_or("").to_string();
    for l in lines {
        out.push('\n');
        if !l.is_empty() {
            out.push_str(pad);
        }
        out.push_str(l);
    }
    out
}

pub fn build_course_markdown(
    detail: &CourseDetail,
    annotations: &[AnnotationInContext],
    lesson_notebooks: &HashMap<String, String>,
    course_notebook: &str,
    exported_on: &str,
) -> String {
    let c = &detail.course;
    let mut md = String::new();
    let _ = writeln!(md, "# {}\n", c.title);
    let mut meta = Vec::new();
    if let Some(i) = &c.instructor {
        meta.push(format!("Instructor: {i}"));
    }
    if let Some(u) = &c.source_url {
        meta.push(format!("Source: <{u}>"));
    }
    meta.push(format!("Exported from Lectern on {exported_on}"));
    let _ = writeln!(md, "> {}\n", meta.join(" · "));
    if let Some(d) = c.description.as_deref().filter(|d| !d.trim().is_empty()) {
        let _ = writeln!(md, "{}\n", d.trim());
    }
    if !c.tags.is_empty() {
        let tags: Vec<String> = c
            .tags
            .iter()
            .map(|t| format!("#{}", t.replace(' ', "-")))
            .collect();
        let _ = writeln!(md, "{}\n", tags.join(" "));
    }

    let mut by_lesson: HashMap<&str, Vec<&AnnotationInContext>> = HashMap::new();
    for a in annotations {
        by_lesson
            .entry(a.annotation.lesson_id.as_str())
            .or_default()
            .push(a);
    }

    for m in &detail.modules {
        let _ = writeln!(md, "## {}\n", m.module.title);
        for l in &m.lessons {
            let done = if l.completed_at.is_some() { " ✓" } else { "" };
            match &l.youtube_video_id {
                Some(id) => {
                    let _ = writeln!(md, "### [{}](https://youtu.be/{id}){done}\n", l.title);
                }
                None => match &l.url {
                    Some(u) => {
                        let _ = writeln!(md, "### [{}]({u}){done}\n", l.title);
                    }
                    None => {
                        let _ = writeln!(md, "### {}{done}\n", l.title);
                    }
                },
            }
            let mut attribution = Vec::new();
            if let Some(ch) = &l.channel_title {
                attribution.push(ch.clone());
            }
            match l.license {
                Some(License::CreativeCommon) => attribution.push("Creative Commons".into()),
                Some(License::Youtube) => attribution.push("Standard YouTube license".into()),
                None => {}
            }
            if !attribution.is_empty() {
                let _ = writeln!(md, "*{}*\n", attribution.join(" · "));
            }

            let items = by_lesson
                .get(l.id.as_str())
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            for ctx in items {
                let a = &ctx.annotation;
                let mut line = String::from("- ");
                if a.kind == AnnotationType::Todo {
                    line.push_str(if a.status.as_deref() == Some("done") {
                        "[x] "
                    } else {
                        "[ ] "
                    });
                }
                if let Some(ts) = ts_link(l.youtube_video_id.as_deref(), a.start_sec) {
                    line.push_str(&ts);
                    line.push(' ');
                } else if let Some(p) = a.page {
                    let _ = write!(line, "p.{p} ");
                }
                let _ = write!(line, "**{}**", a.kind.label());
                if a.kind == AnnotationType::Question
                    && let Some(s) = &a.status
                {
                    let _ = write!(line, " ({s})");
                }
                if a.kind == AnnotationType::Todo {
                    if let Some(p) = &a.priority {
                        let _ = write!(line, " ({p})");
                    }
                    if let Some(d) = &a.due_date {
                        let _ = write!(line, " — due {d}");
                    }
                }
                match (&a.title, &a.url) {
                    (Some(t), Some(u)) => {
                        let _ = write!(line, " [{t}]({u})");
                    }
                    (None, Some(u)) => {
                        let _ = write!(line, " <{u}>");
                    }
                    (Some(t), None) => {
                        let _ = write!(line, " {t}");
                    }
                    (None, None) => {}
                }
                if let Some(rt) = &a.ref_type {
                    let _ = write!(line, " _({rt})_");
                }
                if let Some(b) = a.body_md.as_deref().filter(|b| !b.trim().is_empty()) {
                    let sep = if a.title.is_some() || a.url.is_some() {
                        " — "
                    } else {
                        " "
                    };
                    let _ = write!(line, "{sep}{}", indent(b, "  "));
                }
                if a.ai_generated {
                    line.push_str(" _(AI-generated)_");
                }
                let _ = writeln!(md, "{line}");
                if let Some(ans) = a.answer_md.as_deref().filter(|s| !s.trim().is_empty()) {
                    let _ = writeln!(md, "  - **Answer:** {}", indent(ans, "    "));
                }
            }
            if !items.is_empty() {
                md.push('\n');
            }
            if let Some(nb) = lesson_notebooks.get(&l.id).filter(|s| !s.trim().is_empty()) {
                let _ = writeln!(md, "#### Lesson notebook\n\n{}\n", nb.trim());
            }
        }
    }
    if !course_notebook.trim().is_empty() {
        let _ = writeln!(md, "## Course notebook\n\n{}\n", course_notebook.trim());
    }
    md.trim_end().to_string() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::*;

    fn lesson(id: &str, title: &str, vid: Option<&str>, done: bool) -> Lesson {
        Lesson {
            id: id.into(),
            module_id: "m1".into(),
            position: 0,
            lesson_type: if vid.is_some() {
                LessonType::Video
            } else {
                LessonType::Reading
            },
            title: title.into(),
            description: None,
            url: if vid.is_none() {
                Some("https://example.com/paper.pdf".into())
            } else {
                None
            },
            youtube_video_id: vid.map(Into::into),
            channel_title: vid.map(|_| "MIT".into()),
            license: vid.map(|_| License::CreativeCommon),
            duration_sec: Some(4800),
            local_video_path: None,
            local_quality: None,
            thumbnail_path: None,
            completed_at: done.then(|| "2026-01-01T00:00:00Z".into()),
            due_date: None,
            last_position_sec: None,
            doc_kind: None,
            doc_pages: None,
        }
    }

    fn ann(lesson: &str, kind: AnnotationType, at: Option<f64>, body: &str) -> AnnotationInContext {
        AnnotationInContext {
            annotation: Annotation {
                id: format!("a{at:?}"),
                lesson_id: lesson.into(),
                kind,
                start_sec: at,
                end_sec: None,
                title: None,
                body_md: Some(body.into()),
                status: None,
                priority: None,
                due_date: None,
                url: None,
                ref_type: None,
                answer_md: None,
                tags: vec![],
                ai_generated: false,
                created_at: String::new(),
                updated_at: String::new(),
                material_id: None,
                page: None,
            },
            lesson_title: String::new(),
            youtube_video_id: None,
            module_id: "m1".into(),
            module_title: "Lectures".into(),
        }
    }

    #[test]
    fn rewrites_attachment_links() {
        let md = "a ![Frame 1:05](lectern-attachment:media/c/l1/frames/X.png) b ![f](lectern-attachment:../../etc/passwd)";
        let (out, files) = rewrite_attachment_links(md, "notes.assets");
        assert_eq!(
            out,
            "a ![Frame 1:05](notes.assets/X.png) b ![f](notes.assets/passwd)"
        );
        assert_eq!(
            files,
            vec!["media/c/l1/frames/X.png".to_string()],
            "path traversal is never copied"
        );
    }

    #[test]
    fn timestamps() {
        assert_eq!(fmt_ts(754.9), "12:34");
        assert_eq!(fmt_ts(3723.0), "1:02:03");
        assert_eq!(fmt_ts(5.0), "0:05");
    }

    #[test]
    fn golden_course_export() {
        let detail = CourseDetail {
            course: Course {
                id: "c1".into(),
                title: "MIT 6.824".into(),
                slug: "mit".into(),
                description: Some("Spring 2020".into()),
                instructor: Some("Robert Morris".into()),
                source_url: Some("https://www.youtube.com/playlist?list=PLx".into()),
                youtube_playlist_id: Some("PLx".into()),
                cover_path: None,
                tags: vec!["distributed systems".into()],
                mode: CourseMode::Stream,
                status: CourseStatus::Active,
                playback_rate: 1.0,
                created_at: String::new(),
                updated_at: String::new(),
            },
            modules: vec![ModuleWithLessons {
                module: Module {
                    id: "m1".into(),
                    course_id: "c1".into(),
                    title: "Week 1".into(),
                    position: 0,
                },
                lessons: vec![
                    lesson("l1", "Lecture 1", Some("cQP8WApzIQQ"), true),
                    lesson("l2", "MapReduce paper", None, false),
                ],
            }],
        };
        let mut q = ann("l1", AnnotationType::Question, Some(754.0), "Why a master?");
        q.annotation.status = Some("answered".into());
        q.annotation.answer_md = Some("Coordination.\nSimplicity.".into());
        let mut todo = ann("l1", AnnotationType::Todo, Some(3723.0), "Do lab 1");
        todo.annotation.priority = Some("high".into());
        todo.annotation.due_date = Some("2026-02-01".into());
        let mut r = ann(
            "l2",
            AnnotationType::Reference,
            None,
            "Dean & Ghemawat 2004",
        );
        r.annotation.title = Some("MapReduce".into());
        r.annotation.url = Some("https://research.google/pubs/pub62/".into());
        r.annotation.ref_type = Some("paper".into());
        let notes = vec![
            ann("l1", AnnotationType::Note, Some(65.0), "Line one\nLine two"),
            q,
            todo,
            r,
        ];
        let nbs = HashMap::from([("l1".to_string(), "Lesson thoughts".to_string())]);
        let md = build_course_markdown(&detail, &notes, &nbs, "Overall summary", "2026-11-07");
        let expected = "\
# MIT 6.824

> Instructor: Robert Morris · Source: <https://www.youtube.com/playlist?list=PLx> · Exported from Lectern on 2026-11-07

Spring 2020

#distributed-systems

## Week 1

### [Lecture 1](https://youtu.be/cQP8WApzIQQ) ✓

*MIT · Creative Commons*

- [1:05](https://youtu.be/cQP8WApzIQQ?t=65) **Note** Line one
  Line two
- [12:34](https://youtu.be/cQP8WApzIQQ?t=754) **Question** (answered) Why a master?
  - **Answer:** Coordination.
    Simplicity.
- [ ] [1:02:03](https://youtu.be/cQP8WApzIQQ?t=3723) **To-do** (high) — due 2026-02-01 Do lab 1

#### Lesson notebook

Lesson thoughts

### [MapReduce paper](https://example.com/paper.pdf)

- **Reference** [MapReduce](https://research.google/pubs/pub62/) _(paper)_ — Dean & Ghemawat 2004

## Course notebook

Overall summary
";
        assert_eq!(md, expected, "\n--- got ---\n{md}");
    }
}
