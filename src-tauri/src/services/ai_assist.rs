//! AI study assist: explain a moment or page, answer a question, summarise a lesson, and
//! suggest references. Prompts, schemas, focus windows and anchor rendering are pure here;
//! the commands live in `commands/ai_assist.rs`.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::db::models::{AssistFocus, ReferenceDraft, SourceRef};
use crate::services::ai_context::{Anchor, Context, LessonMaterial};

/// Seconds of transcript either side of a focused moment.
pub const SPAN_SEC: f64 = 90.0;
/// Pages either side of a focused page.
pub const SPAN_PAGES: i32 = 1;

const CITE: &str = "Cite the material you rely on with its anchors in square brackets, copied exactly \
(for example [T:L1@754] or [P:M1#3]); never invent anchors. Use Markdown and $…$ for maths.";

pub fn explain_system() -> String {
    format!(
        "You are a patient tutor helping a self-learner who paused a lecture because something was unclear. \
Explain the idea at the focus point in plain language: what is being said, why it matters, and a small example \
if it helps. Build only on the material given and well-established knowledge; say so if the material is \
ambiguous. Keep it to 120-300 words. {CITE}"
    )
}

pub fn answer_system() -> String {
    format!(
        "You are a tutor answering a self-learner's question about a lecture. Answer directly first, then \
justify briefly. Use the lecture material provided; if it doesn't cover the question, say that and answer from \
well-established knowledge, clearly marked as going beyond the lecture. Keep it to 80-300 words. {CITE}"
    )
}

pub fn summary_system() -> String {
    format!(
        "You summarise a lecture for a self-learner's notebook: a one-sentence overview, then 5-10 key points \
as bullets (definitions, results, methods, common pitfalls), then 2-3 questions to check understanding. Each key \
point cites where it is taught. {CITE}"
    )
}

pub const REFERENCES_SYSTEM: &str = "You suggest further reading for a self-learner who just studied a lecture: \
well-known textbooks, chapters, seminal papers, survey articles, lecture notes or videos that a professor would \
recommend for this exact material. Only suggest works you are confident exist, with their real titles and \
authors; if unsure, suggest fewer. Never include URLs. For each, say in one sentence why it helps with this lecture.";

pub fn answer_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["answer"],
        "properties": {"answer": {"type": "string"}}
    })
}

pub fn references_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["references"],
        "properties": {"references": {"type": "array", "items": {
            "type": "object", "additionalProperties": false,
            "required": ["title", "author", "refType", "why"],
            "properties": {
                "title": {"type": "string"},
                "author": {"type": "string"},
                "refType": {"type": "string", "enum": ["paper", "article", "book", "video", "other"]},
                "why": {"type": "string"}
            }
        }}}
    })
}

pub const EXPECTED_ANSWER_OUTPUT: i32 = 900;
pub const EXPECTED_SUMMARY_OUTPUT: i32 = 1400;
pub const EXPECTED_REFERENCES_OUTPUT: i32 = 1000;

/// Keeps only the material around the focus: ±`SPAN_SEC` of transcript and notes for a moment,
/// or ±`SPAN_PAGES` pages of one document for a page. No focus keeps everything.
pub fn focus_material(mut m: LessonMaterial, f: &AssistFocus) -> LessonMaterial {
    if let Some(sec) = f.sec {
        let near = |s: f64| (s - sec).abs() <= SPAN_SEC;
        m.cues.retain(|(s, _)| near(*s));
        m.notes.retain(|(s, _)| near(*s));
        m.page_notes.clear();
        m.pdfs.clear();
    } else if let (Some(mid), Some(page)) = (&f.material_id, f.page) {
        let near = |p: i32| (p - page).abs() <= SPAN_PAGES;
        m.cues.clear();
        m.notes.clear();
        m.page_notes.retain(|(id, p, _)| id == mid && near(*p));
        m.pdfs.retain(|(id, _, _)| id == mid);
        for (_, _, pages) in &mut m.pdfs {
            pages.retain(|(p, _)| near(*p));
        }
    }
    m
}

/// "Where the learner is", in anchor terms, so the model knows what to explain.
pub fn focus_line(ctx: &Context, f: &AssistFocus) -> String {
    if let Some(sec) = f.sec {
        let key = ctx
            .lessons
            .iter()
            .find(|(_, id)| **id == f.lesson_id)
            .map(|(k, _)| k.as_str())
            .unwrap_or("L1");
        return format!(
            "Focus: the learner is at {} in {key} (around anchor T:{key}@{}).",
            fmt_ts(sec),
            sec as i64
        );
    }
    if let (Some(mid), Some(page)) = (&f.material_id, f.page) {
        let key = ctx
            .materials
            .iter()
            .find(|(_, v)| v.0 == *mid)
            .map(|(k, _)| k.as_str())
            .unwrap_or("M1");
        return format!("Focus: the learner is on page {page} of {key} (anchor P:{key}#{page}).");
    }
    "Focus: the whole lesson.".into()
}

pub fn explain_prompt(ctx: &Context, f: &AssistFocus, question: Option<&str>) -> String {
    let ask = question
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(|q| format!("They ask: {q}"))
        .unwrap_or_else(|| "Explain what is going on here.".into());
    format!("{}\n{ask}\n\nMaterial:\n{}", focus_line(ctx, f), ctx.prompt)
}

pub fn answer_prompt(ctx: &Context, f: &AssistFocus, question: &str) -> String {
    format!(
        "{}\nQuestion: {}\n\nMaterial:\n{}",
        focus_line(ctx, f),
        question.trim(),
        ctx.prompt
    )
}

pub fn summary_prompt(ctx: &Context, title: &str) -> String {
    format!(
        "Summarise the lesson \"{title}\".\n\nMaterial:\n{}",
        ctx.prompt
    )
}

pub fn references_prompt(ctx: &Context, title: &str, course: &str) -> String {
    format!(
        "Suggest up to 6 references for the lesson \"{title}\" in the course \"{course}\".\n\nMaterial:\n{}",
        ctx.prompt
    )
}

pub fn fmt_ts(sec: f64) -> String {
    let s = sec.max(0.0).floor() as i64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

static BRACKETS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^\[\]\n]{3,120})\](\()?").unwrap());
static ANCHOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[TN]:L\d+@\d+|[PN]:M\d+#\d+)$").unwrap());

fn source_of(a: &Anchor) -> SourceRef {
    match a {
        Anchor::Video { lesson_id, sec } => SourceRef {
            label: fmt_ts(*sec),
            lesson_id: Some(lesson_id.clone()),
            sec: Some(*sec),
            material_id: None,
            page: None,
        },
        Anchor::Pdf {
            material_id,
            page,
            lesson_id,
        } => SourceRef {
            label: format!("p.{page}"),
            lesson_id: lesson_id.clone(),
            sec: None,
            material_id: Some(material_id.clone()),
            page: Some(*page),
        },
    }
}

/// Replaces cited anchors with readable labels ("(12:34, p.3)") and collects them as sources.
/// Invalid anchors are dropped; other brackets (e.g. Markdown links) are left alone.
pub fn render_anchors(text: &str, ctx: &Context) -> (String, Vec<SourceRef>) {
    let mut sources: Vec<SourceRef> = Vec::new();
    let out = BRACKETS.replace_all(text, |c: &regex::Captures<'_>| {
        if c.get(2).is_some() {
            return c[0].to_string(); // a Markdown link: [text](url)
        }
        let tokens: Vec<&str> = c[1].split([',', ';']).map(str::trim).collect();
        if !tokens.iter().all(|t| ANCHOR.is_match(t)) {
            return c[0].to_string();
        }
        let labels: Vec<String> = tokens
            .iter()
            .filter_map(|t| ctx.resolve(t))
            .map(|a| {
                let s = source_of(&a);
                let label = s.label.clone();
                if !sources
                    .iter()
                    .any(|x| x.label == s.label && x.material_id == s.material_id)
                {
                    sources.push(s);
                }
                label
            })
            .collect();
        if labels.is_empty() {
            String::new()
        } else {
            format!("({})", labels.join(", "))
        }
    });
    // Tidy spaces left by removed anchors.
    let tidy = out.replace(" .", ".").replace(" ,", ",").replace("  ", " ");
    (tidy.trim().to_string(), sources)
}

#[derive(Deserialize)]
struct RawAnswer {
    answer: String,
}

pub fn parse_answer(v: Value) -> Option<String> {
    serde_json::from_value::<RawAnswer>(v)
        .ok()
        .map(|a| a.answer)
        .filter(|a| !a.trim().is_empty())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRef {
    title: String,
    author: String,
    ref_type: String,
    why: String,
}
#[derive(Deserialize)]
struct RawRefs {
    references: Vec<RawRef>,
}

pub fn parse_references(v: Value) -> Vec<ReferenceDraft> {
    let Ok(r) = serde_json::from_value::<RawRefs>(v) else {
        return vec![];
    };
    r.references
        .into_iter()
        .filter(|x| !x.title.trim().is_empty())
        .map(|x| {
            // Never keep a URL the model slipped in despite the instructions.
            let clean = |s: &str| {
                s.split_whitespace()
                    .filter(|w| !w.contains("://") && !w.starts_with("www."))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            ReferenceDraft {
                title: clean(&x.title),
                author: Some(clean(&x.author)).filter(|a| !a.is_empty()),
                ref_type: if crate::db::repo::annotations::REF_TYPES.contains(&x.ref_type.as_str())
                {
                    x.ref_type
                } else {
                    "other".into()
                },
                why: clean(&x.why),
            }
        })
        .take(10)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai_context::build;

    fn material() -> LessonMaterial {
        LessonMaterial {
            lesson_id: "les".into(),
            title: "Induction".into(),
            duration_sec: Some(3000.0),
            cues: vec![
                (0.0, "intro".into()),
                (600.0, "base case".into()),
                (660.0, "inductive step".into()),
                (900.0, "far away".into()),
            ],
            notes: vec![(650.0, "my note".into()), (2000.0, "later note".into())],
            page_notes: vec![("pdf".into(), 3, "page note".into())],
            pdfs: vec![(
                "pdf".into(),
                "Notes".into(),
                vec![
                    (1, "p1".into()),
                    (2, "p2".into()),
                    (3, "p3".into()),
                    (5, "p5".into()),
                ],
            )],
        }
    }

    fn focus(sec: Option<f64>, page: Option<i32>) -> AssistFocus {
        AssistFocus {
            lesson_id: "les".into(),
            sec,
            material_id: page.map(|_| "pdf".into()),
            page,
        }
    }

    #[test]
    fn moment_keeps_nearby_transcript_only() {
        let m = focus_material(material(), &focus(Some(640.0), None));
        let starts: Vec<f64> = m.cues.iter().map(|c| c.0).collect();
        assert_eq!(starts, vec![600.0, 660.0]);
        assert_eq!(m.notes.len(), 1);
        assert!(m.pdfs.is_empty() && m.page_notes.is_empty());
    }

    #[test]
    fn page_keeps_neighbouring_pages_only() {
        let m = focus_material(material(), &focus(None, Some(2)));
        assert!(m.cues.is_empty() && m.notes.is_empty());
        let pages: Vec<i32> = m.pdfs[0].2.iter().map(|p| p.0).collect();
        assert_eq!(pages, vec![1, 2, 3]);
        assert_eq!(m.page_notes.len(), 1);
        let whole = focus_material(material(), &focus(None, None));
        assert_eq!(whole.cues.len(), 4);
    }

    #[test]
    fn anchors_become_labels_and_sources() {
        let ctx = build(&[material()], 10_000);
        assert!(focus_line(&ctx, &focus(Some(754.0), None)).contains("12:34"));
        let (md, src) = render_anchors(
            "The base case comes first [T:L1@600]. Then the step [T:L1@660, P:M1#3]. \
             Bogus [T:L9@1]. See [the book](https://x.org) and [note].",
            &ctx,
        );
        assert!(md.contains("first (10:00)."), "{md}");
        assert!(md.contains("step (11:00, p.3)."), "{md}");
        assert!(md.contains("Bogus."), "{md}");
        assert!(md.contains("[the book](https://x.org)") && md.contains("[note]"));
        assert_eq!(src.len(), 3);
        assert_eq!(src[2].page, Some(3));
    }

    #[test]
    fn references_drop_urls_and_unknown_types() {
        let v = json!({"references": [
            {"title": "Concrete Mathematics", "author": "Graham, Knuth, Patashnik", "refType": "book",
             "why": "Chapter 1 https://bad.example covers recurrences."},
            {"title": "", "author": "", "refType": "book", "why": ""},
            {"title": "Odd", "author": "", "refType": "podcast", "why": "x"}
        ]});
        let r = parse_references(v);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].why, "Chapter 1 covers recurrences.");
        assert_eq!(r[1].ref_type, "other");
        assert_eq!(r[1].author, None);
        assert_eq!(parse_answer(json!({"answer": "  "})), None);
        assert_eq!(fmt_ts(3725.0), "1:02:05");
    }
}
