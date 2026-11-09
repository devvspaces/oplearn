//! Builds study material into AI prompts with **anchors** the model cites back, so generated
//! flashcards and quiz hints link to the exact place they came from:
//!
//! - `[T:L1@754]` transcript window starting at 754 s in lesson L1
//! - `[N:L1@754]` the user's note at 754 s
//! - `[P:M2#3]`   page 3 of material M2 (PDF lecture notes / readings)
//! - `[N:M2#3]`   the user's note on page 3 of material M2
//!
//! `L1`/`M2` are short keys mapped back to real ids by [`Context`]. Everything here is pure
//! except the `load_*` functions, which only read the database.

use std::collections::HashMap;

use rusqlite::Connection;
use serde_json::Value;

use crate::db::repo::{annotations, lessons, transcripts};
use crate::error::AppResult;

/// Seconds of transcript per window.
pub const WINDOW_SEC: f64 = 30.0;

/// Strips HTML to readable text: block tags become newlines, table cells become " | ",
/// common entities are decoded and whitespace collapsed.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            for t in chars.by_ref() {
                if t == '>' {
                    break;
                }
                tag.push(t);
            }
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            match name.as_str() {
                "td" | "th" if !tag.starts_with('/') => out.push_str(" | "),
                "br" | "p" | "div" | "tr" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                | "table" | "ul" | "ol" => out.push('\n'),
                _ => {}
            }
        } else {
            out.push(c);
        }
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&rsquo;", "’")
        .replace("&ldquo;", "“")
        .replace("&rdquo;", "”")
        .replace("&ndash;", "–")
        .replace("&mdash;", "—");
    decoded
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .map(|l| {
            l.trim_start_matches("| ")
                .trim_end_matches(" |")
                .to_string()
        })
        .filter(|l| !l.is_empty() && l != "|")
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let t: String = s.chars().take(n).collect();
    format!("{t}…")
}

/// One anchored snippet.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub anchor: String,
    pub text: String,
}

/// A PDF: (material id, title, pages as (page number, text)).
pub type PdfPages = (String, String, Vec<(i32, String)>);

/// A lesson's material for the prompt.
#[derive(Debug, Clone, Default)]
pub struct LessonMaterial {
    pub lesson_id: String,
    pub title: String,
    pub duration_sec: Option<f64>,
    /// (start_sec, text) transcript cues.
    pub cues: Vec<(f64, String)>,
    /// (start_sec, text) notes.
    pub notes: Vec<(f64, String)>,
    /// (material_id, page, text) notes anchored to a PDF page.
    pub page_notes: Vec<(String, i32, String)>,
    /// (material_id, title, pages[(page, text)]).
    pub pdfs: Vec<PdfPages>,
}

/// Groups cues into windows of `WINDOW_SEC`, returning (window_start, joined text).
pub fn windows(cues: &[(f64, String)]) -> Vec<(f64, String)> {
    let mut out: Vec<(f64, String)> = Vec::new();
    for (start, text) in cues {
        match out.last_mut() {
            Some((w, t)) if *start - *w < WINDOW_SEC => {
                t.push(' ');
                t.push_str(text);
            }
            _ => out.push(((start.max(0.0)).floor(), text.clone())),
        }
    }
    out
}

/// Anchors resolved back to real ids.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub prompt: String,
    /// "L1" → lesson id.
    pub lessons: HashMap<String, String>,
    /// lesson id → (duration, window starts)
    pub lesson_secs: HashMap<String, (Option<f64>, Vec<f64>)>,
    /// "M1" → (material id, page count, lesson id)
    pub materials: HashMap<String, (String, i32, Option<String>)>,
    pub items: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Anchor {
    Video {
        lesson_id: String,
        sec: f64,
    },
    Pdf {
        material_id: String,
        page: i32,
        lesson_id: Option<String>,
    },
}

impl Context {
    /// Resolves an anchor like `T:L1@754`, `N:L1@754` or `P:M2#3` (brackets optional). Video
    /// times are snapped to the nearest transcript window and must fall inside the video.
    pub fn resolve(&self, raw: &str) -> Option<Anchor> {
        let a = raw.trim().trim_start_matches('[').trim_end_matches(']');
        if let Some(rest) = a.strip_prefix("N:").filter(|r| r.contains('#')) {
            return self.resolve(&format!("P:{rest}"));
        }
        if let Some(rest) = a.strip_prefix("T:").or_else(|| a.strip_prefix("N:")) {
            let (key, sec) = rest.split_once('@')?;
            let lesson_id = self.lessons.get(key)?.clone();
            let sec: f64 = sec.trim().parse().ok()?;
            let (dur, starts) = self.lesson_secs.get(&lesson_id)?;
            if sec < 0.0 || dur.is_some_and(|d| sec > d + 1.0) {
                return None;
            }
            let snapped = starts
                .iter()
                .copied()
                .min_by(|a, b| (a - sec).abs().total_cmp(&(b - sec).abs()))
                .filter(|s| (s - sec).abs() <= WINDOW_SEC)
                .unwrap_or(sec);
            return Some(Anchor::Video {
                lesson_id,
                sec: snapped,
            });
        }
        if let Some(rest) = a.strip_prefix("P:") {
            let (key, page) = rest.split_once('#')?;
            let (material_id, pages, lesson_id) = self.materials.get(key)?.clone();
            let page: i32 = page.trim().parse().ok()?;
            return (page >= 1 && page <= pages.max(1)).then_some(Anchor::Pdf {
                material_id,
                page,
                lesson_id,
            });
        }
        None
    }
}

/// Renders lessons into an anchored prompt within `max_chars`, trimming every item evenly.
pub fn build(lessons: &[LessonMaterial], max_chars: usize) -> Context {
    let mut ctx = Context::default();
    // Collect items per lesson first to size the per-item budget.
    let mut blocks: Vec<(String, Vec<Item>)> = Vec::new();
    let mut m_index = 0;
    for (li, l) in lessons.iter().enumerate() {
        let key = format!("L{}", li + 1);
        ctx.lessons.insert(key.clone(), l.lesson_id.clone());
        let wins = windows(&l.cues);
        ctx.lesson_secs.insert(
            l.lesson_id.clone(),
            (l.duration_sec, wins.iter().map(|w| w.0).collect()),
        );
        let mut items: Vec<Item> = wins
            .into_iter()
            .map(|(s, t)| Item {
                anchor: format!("T:{key}@{}", s as i64),
                text: t,
            })
            .collect();
        items.extend(l.notes.iter().map(|(s, t)| Item {
            anchor: format!("N:{key}@{}", *s as i64),
            text: t.clone(),
        }));
        let mut keys: HashMap<&str, String> = HashMap::new();
        for (mid, _title, pages) in &l.pdfs {
            m_index += 1;
            let mk = format!("M{m_index}");
            ctx.materials.insert(
                mk.clone(),
                (mid.clone(), pages.len() as i32, Some(l.lesson_id.clone())),
            );
            keys.insert(mid, mk.clone());
            items.extend(pages.iter().map(|(p, t)| Item {
                anchor: format!("P:{mk}#{p}"),
                text: t.clone(),
            }));
        }
        for (mid, page, text) in &l.page_notes {
            let mk = match keys.get(mid.as_str()) {
                Some(k) => k.clone(),
                None => {
                    // A document whose text isn't indexed: register it so the note's page resolves.
                    m_index += 1;
                    let k = format!("M{m_index}");
                    keys.insert(mid, k.clone());
                    ctx.materials
                        .insert(k.clone(), (mid.clone(), 0, Some(l.lesson_id.clone())));
                    k
                }
            };
            if let Some(e) = ctx.materials.get_mut(&mk) {
                e.1 = e.1.max(*page);
            }
            items.push(Item {
                anchor: format!("N:{mk}#{page}"),
                text: text.clone(),
            });
        }
        let header = format!("## {key}: {}", l.title);
        blocks.push((header, items));
    }
    let n: usize = blocks.iter().map(|b| b.1.len()).sum::<usize>().max(1);
    let per_item = (max_chars / n).max(80);
    let mut out = String::new();
    for (header, items) in blocks {
        out.push_str(&header);
        out.push('\n');
        for it in items {
            let text = it.text.split_whitespace().collect::<Vec<_>>().join(" ");
            out.push_str(&format!("[{}] {}\n", it.anchor, truncate(&text, per_item)));
            ctx.items += 1;
        }
        out.push('\n');
    }
    ctx.prompt = out;
    ctx
}

// ---- database loaders ----

/// Loads one lesson's transcript, (optionally) notes, and linked PDF pages.
pub fn load_lesson(
    conn: &Connection,
    lesson_id: &str,
    include_notes: bool,
) -> AppResult<LessonMaterial> {
    let l = lessons::get_lesson(conn, lesson_id)?;
    let t = transcripts::get_transcript(conn, lesson_id)?;
    let cues = t.cues.into_iter().map(|c| (c.start, c.text)).collect();
    let mut page_notes = vec![];
    let notes = if include_notes {
        annotations::list_for_lesson(conn, lesson_id)?
            .into_iter()
            .filter_map(|a| {
                let page = a.material_id.clone().zip(a.page);
                let body = html_to_text(&a.body_md.unwrap_or_default());
                let text = [
                    a.title.unwrap_or_default(),
                    body,
                    a.answer_md.unwrap_or_default(),
                ]
                .into_iter()
                .filter(|s| !s.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" — ");
                if text.is_empty() {
                    return None;
                }
                if let Some((m, p)) = page {
                    page_notes.push((m, p, text));
                    return None;
                }
                Some((a.start_sec.unwrap_or(0.0), text))
            })
            .collect()
    } else {
        vec![]
    };
    let mut st = conn.prepare(
        "SELECT id, title FROM materials WHERE lesson_id = ?1 AND solution_of IS NULL
           AND ((local_path IS NOT NULL AND file_type = 'application/pdf') OR COALESCE(page_count, 0) > 0)
          ORDER BY position LIMIT 4",
    )?;
    let mats: Vec<(String, String)> = st
        .query_map([lesson_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut pdfs = vec![];
    for (id, title) in mats {
        pdfs.push((id.clone(), title, material_pages(conn, &id)?));
    }
    Ok(LessonMaterial {
        lesson_id: l.id,
        title: l.title,
        duration_sec: l.duration_sec.map(f64::from),
        cues,
        notes,
        page_notes,
        pdfs,
    })
}

pub fn material_pages(conn: &Connection, material_id: &str) -> AppResult<Vec<(i32, String)>> {
    let mut st = conn.prepare(
        "SELECT CAST(page AS INTEGER), text FROM material_pages_fts WHERE material_id = ?1 ORDER BY CAST(page AS INTEGER)",
    )?;
    Ok(st
        .query_map([material_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?)
}

/// Extra course context for planning/restructuring: provider metadata, syllabus and calendar.
/// None for courses without imported materials.
pub fn course_outline_context(conn: &Connection, course_id: &str) -> AppResult<Option<String>> {
    let meta: Option<String> = conn
        .query_row(
            "SELECT json FROM course_meta WHERE course_id = ?1",
            [course_id],
            |r| r.get(0),
        )
        .ok();
    let mut st = conn.prepare(
        "SELECT kind, title, html FROM materials WHERE course_id = ?1 AND kind IN ('syllabus','calendar') AND html IS NOT NULL",
    )?;
    let pages: Vec<(String, String, String)> = st
        .query_map([course_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let mut st = conn.prepare(
        "SELECT kind, title, page_count, lesson_id FROM materials WHERE course_id = ?1 AND kind NOT IN ('syllabus','calendar','page')
          ORDER BY position",
    )?;
    let mats: Vec<(String, String, Option<i32>, Option<String>)> = st
        .query_map([course_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<Result<_, _>>()?;
    if meta.is_none() && pages.is_empty() && mats.is_empty() {
        return Ok(None);
    }
    let mut s = String::from("Additional course information from the provider:\n");
    if let Some(m) = meta.and_then(|j| serde_json::from_str::<Value>(&j).ok()) {
        for k in ["courseNumber", "term", "level", "instructors", "topics"] {
            if let Some(v) = m.get(k).filter(|v| !v.is_null()) {
                s.push_str(&format!(
                    "{k}: {}\n",
                    if let Some(x) = v.as_str() {
                        x.to_string()
                    } else {
                        v.to_string()
                    }
                ));
            }
        }
    }
    for (kind, title, html) in pages {
        s.push_str(&format!(
            "\n### {title} ({kind})\n{}\n",
            truncate(&html_to_text(&html), 6000)
        ));
    }
    if !mats.is_empty() {
        s.push_str("\n### Materials\n");
        for (kind, title, pages, lesson) in mats.iter().take(200) {
            s.push_str(&format!(
                "- [{kind}] {title}{}{}\n",
                pages.map(|p| format!(" ({p} pages)")).unwrap_or_default(),
                lesson
                    .as_ref()
                    .map(|l| format!(" (lesson id={l})"))
                    .unwrap_or_default()
            ));
        }
    }
    Ok(Some(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_tables_become_rows() {
        let t = html_to_text(
            "<p><table><tr><th>LEC #</th><th>TOPICS</th></tr><tr><td>1</td><td>Proofs &amp; induction</td></tr></table></p>",
        );
        assert_eq!(t, "LEC # | TOPICS\n1 | Proofs & induction");
    }

    #[test]
    fn windows_group_cues() {
        let w = windows(&[
            (0.0, "a".into()),
            (10.0, "b".into()),
            (31.0, "c".into()),
            (65.5, "d".into()),
        ]);
        assert_eq!(
            w,
            vec![(0.0, "a b".into()), (31.0, "c".into()), (65.0, "d".into())]
        );
    }

    fn sample() -> Context {
        build(
            &[LessonMaterial {
                lesson_id: "lesson-a".into(),
                title: "Induction".into(),
                duration_sec: Some(600.0),
                cues: vec![(0.0, "hello".into()), (40.0, "strong induction".into())],
                notes: vec![(42.0, "remember base case".into())],
                page_notes: vec![
                    ("mat-1".into(), 2, "why strong?".into()),
                    ("mat-9".into(), 5, "unindexed doc".into()),
                ],
                pdfs: vec![(
                    "mat-1".into(),
                    "Notes".into(),
                    vec![(1, "page one".into()), (2, "page two".into())],
                )],
            }],
            10_000,
        )
    }

    #[test]
    fn prompt_has_anchors_and_they_resolve() {
        let c = sample();
        assert!(c.prompt.contains("## L1: Induction"));
        assert!(c.prompt.contains("[T:L1@40] strong induction"));
        assert!(c.prompt.contains("[N:L1@42] remember base case"));
        assert!(c.prompt.contains("[P:M1#2] page two"));
        assert!(c.prompt.contains("[N:M1#2] why strong?"));
        assert!(c.prompt.contains("[N:M2#5] unindexed doc"));
        assert_eq!(
            c.resolve("[N:M1#2]"),
            Some(Anchor::Pdf {
                material_id: "mat-1".into(),
                page: 2,
                lesson_id: Some("lesson-a".into())
            })
        );
        assert!(matches!(
            c.resolve("N:M2#5"),
            Some(Anchor::Pdf { page: 5, .. })
        ));
        assert_eq!(
            c.resolve("N:M2#6"),
            None,
            "beyond the noted page of an unindexed doc"
        );
        assert_eq!(c.items, 7);
        assert_eq!(
            c.resolve("[T:L1@44]"),
            Some(Anchor::Video {
                lesson_id: "lesson-a".into(),
                sec: 40.0
            })
        );
        assert_eq!(
            c.resolve("N:L1@42"),
            Some(Anchor::Video {
                lesson_id: "lesson-a".into(),
                sec: 40.0
            })
        );
        assert_eq!(
            c.resolve("P:M1#2"),
            Some(Anchor::Pdf {
                material_id: "mat-1".into(),
                page: 2,
                lesson_id: Some("lesson-a".into())
            })
        );
        assert_eq!(c.resolve("P:M1#9"), None, "page out of range");
        assert_eq!(c.resolve("T:L1@9000"), None, "beyond the video");
        assert_eq!(c.resolve("T:L7@10"), None, "unknown lesson");
        assert_eq!(c.resolve("nonsense"), None);
    }

    #[test]
    fn truncates_evenly() {
        let long = "word ".repeat(1000);
        let c = build(
            &[LessonMaterial {
                lesson_id: "x".into(),
                title: "T".into(),
                duration_sec: Some(100.0),
                cues: vec![(0.0, long.clone()), (40.0, long)],
                ..Default::default()
            }],
            400,
        );
        assert!(c.prompt.chars().count() < 700, "{}", c.prompt.len());
        assert_eq!(c.prompt.matches('…').count(), 2);
    }
}
