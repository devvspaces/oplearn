//! AI flashcards and quiz banks (prompts, schemas, and pure conversion of model output into
//! validated drafts with resolved anchors).

use serde::Deserialize;
use serde_json::{Value, json};

use crate::db::models::{FlashcardDraft, QuizDraft, QuizHint, QuizKind};
use crate::db::repo::quiz;
use crate::services::ai_context::{Anchor, Context};

pub const CARDS_SYSTEM: &str = "You write spaced-repetition flashcards from lecture material for a self-learner. \
Each card tests one idea: a precise question on the front and a short, complete answer on the back (1-3 sentences, \
plain text or simple Markdown; use $…$ for maths). Prefer understanding (why/how/when) over trivia, avoid yes/no \
questions, and never ask about the lecturer or the course logistics. Every card must cite the single anchor from the \
material it is based on, copied exactly (for example T:L1@754 or P:M1#3).";

pub const QUIZ_SYSTEM: &str = "You write quiz questions from lecture material for a self-learner. \
Multiple-choice questions have 4 options (3-5 allowed) with exactly one correct answer and plausible distractors \
that reflect common misconceptions; give answerIndex as the 0-based index of the correct option. Short-answer \
questions have a concise model answer (answerIndex -1, choices empty). Every question has a one or two sentence \
explanation and cites the single anchor where the answer is taught, copied exactly (for example T:L1@754 or P:M1#3), \
so the learner can jump there for a hint. Mix difficulties. Use $…$ for maths.";

pub fn cards_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["cards"],
        "properties": {"cards": {"type": "array", "items": {
            "type": "object", "additionalProperties": false, "required": ["front", "back", "anchor"],
            "properties": {"front": {"type": "string"}, "back": {"type": "string"}, "anchor": {"type": "string"}}
        }}}
    })
}

pub fn quiz_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["questions"],
        "properties": {"questions": {"type": "array", "items": {
            "type": "object", "additionalProperties": false,
            "required": ["kind", "prompt", "choices", "answerIndex", "answer", "explanation", "anchor", "difficulty"],
            "properties": {
                "kind": {"type": "string", "enum": ["mcq", "short"]},
                "prompt": {"type": "string"},
                "choices": {"type": "array", "items": {"type": "string"}},
                "answerIndex": {"type": "integer"},
                "answer": {"type": "string"},
                "explanation": {"type": "string"},
                "anchor": {"type": "string"},
                "difficulty": {"type": "string", "enum": ["easy", "medium", "hard"]}
            }
        }}}
    })
}

pub fn cards_prompt(ctx: &Context, count: u32) -> String {
    format!(
        "Write {count} flashcards from this material.\n\n{}",
        ctx.prompt
    )
}

pub fn quiz_prompt(ctx: &Context, count: u32, mcq_share: f64) -> String {
    let mcq = ((f64::from(count)) * mcq_share.clamp(0.0, 1.0)).round() as u32;
    format!(
        "Write {count} quiz questions from this material: about {mcq} multiple-choice and {} short-answer.\n\n{}",
        count - mcq.min(count),
        ctx.prompt
    )
}

pub fn expected_cards_output(count: u32) -> i32 {
    800 + 90 * count as i32
}

pub fn expected_quiz_output(count: u32) -> i32 {
    1000 + 220 * count as i32
}

#[derive(Debug, Deserialize)]
struct RawCards {
    cards: Vec<RawCard>,
}
#[derive(Debug, Deserialize)]
struct RawCard {
    front: String,
    back: String,
    anchor: String,
}

/// Cards with their anchor resolved to a timestamp or a PDF page.
pub fn to_card_drafts(v: Value, ctx: &Context) -> Vec<FlashcardDraft> {
    let Ok(r) = serde_json::from_value::<RawCards>(v) else {
        return vec![];
    };
    r.cards
        .into_iter()
        .filter(|c| !c.front.trim().is_empty())
        .map(|c| {
            let anchor = ctx.resolve(&c.anchor);
            let (source_sec, pdf) = match anchor {
                Some(Anchor::Video { sec, .. }) => (Some(sec), None),
                Some(Anchor::Pdf {
                    material_id, page, ..
                }) => (None, Some((material_id, page))),
                None => (None, None),
            };
            FlashcardDraft {
                front_md: c.front.trim().to_string(),
                back_md: c.back.trim().to_string(),
                source_sec,
                source_material_id: pdf.as_ref().map(|p| p.0.clone()),
                source_page: pdf.map(|p| p.1),
            }
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawQuiz {
    questions: Vec<RawQuestion>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawQuestion {
    kind: String,
    prompt: String,
    #[serde(default)]
    choices: Vec<String>,
    answer_index: i32,
    #[serde(default)]
    answer: String,
    #[serde(default)]
    explanation: String,
    anchor: String,
    difficulty: Option<String>,
}

/// Converts model output to validated drafts; returns (drafts, dropped).
pub fn to_quiz_drafts(v: Value, ctx: &Context) -> (Vec<QuizDraft>, i32) {
    let Ok(r) = serde_json::from_value::<RawQuiz>(v) else {
        return (vec![], 0);
    };
    let mut dropped = 0;
    let drafts = r
        .questions
        .into_iter()
        .filter_map(|q| {
            let (hint, lesson_id) = match ctx.resolve(&q.anchor) {
                Some(Anchor::Video { lesson_id, sec }) => (
                    QuizHint::Video {
                        lesson_id: lesson_id.clone(),
                        sec,
                    },
                    Some(lesson_id),
                ),
                Some(Anchor::Pdf {
                    material_id,
                    page,
                    lesson_id,
                }) => (
                    QuizHint::Pdf {
                        material_id,
                        page,
                        lesson_id: lesson_id.clone(),
                    },
                    lesson_id,
                ),
                None => (QuizHint::None, None),
            };
            let kind = if q.kind == "mcq" {
                QuizKind::Mcq
            } else {
                QuizKind::Short
            };
            let d = QuizDraft {
                lesson_id,
                kind,
                prompt_md: q.prompt,
                choices: q.choices,
                answer_index: (kind == QuizKind::Mcq).then_some(q.answer_index),
                answer_md: q.answer,
                explanation_md: q.explanation,
                hint,
                difficulty: q.difficulty,
            };
            match quiz::validate(d) {
                Ok(d) => Some(d),
                Err(_) => {
                    dropped += 1;
                    None
                }
            }
        })
        .collect();
    (drafts, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai_context::{LessonMaterial, build};

    fn ctx() -> Context {
        build(
            &[LessonMaterial {
                lesson_id: "les".into(),
                title: "Induction".into(),
                duration_sec: Some(600.0),
                cues: vec![(0.0, "intro".into()), (60.0, "base case".into())],
                notes: vec![],
                page_notes: vec![],
                pdfs: vec![(
                    "mat".into(),
                    "Notes".into(),
                    vec![(1, "p1".into()), (2, "p2".into())],
                )],
            }],
            10_000,
        )
    }

    #[test]
    fn cards_resolve_anchors() {
        let v = json!({"cards": [
            {"front": "What is the base case?", "back": "The first step.", "anchor": "T:L1@61"},
            {"front": "From notes", "back": "x", "anchor": "P:M1#2"},
            {"front": " ", "back": "dropped", "anchor": "T:L1@0"}
        ]});
        let d = to_card_drafts(v, &ctx());
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].source_sec, Some(60.0));
        assert_eq!(d[1].source_sec, None);
    }

    #[test]
    fn quiz_drafts_validate_and_link_hints() {
        let v = json!({"questions": [
            {"kind": "mcq", "prompt": "Which is a base case?", "choices": ["n=0", "n=k", "n=k+1", "none"], "answerIndex": 0,
             "answer": "", "explanation": "Start at 0.", "anchor": "T:L1@60", "difficulty": "easy"},
            {"kind": "short", "prompt": "Define induction.", "choices": [], "answerIndex": -1, "answer": "Proof technique…",
             "explanation": "", "anchor": "P:M1#1", "difficulty": "medium"},
            {"kind": "mcq", "prompt": "Broken", "choices": ["a"], "answerIndex": 0, "answer": "", "explanation": "",
             "anchor": "bogus", "difficulty": "hard"}
        ]});
        let (d, dropped) = to_quiz_drafts(v, &ctx());
        assert_eq!((d.len(), dropped), (2, 1));
        assert_eq!(
            d[0].hint,
            QuizHint::Video {
                lesson_id: "les".into(),
                sec: 60.0
            }
        );
        assert_eq!(d[0].lesson_id.as_deref(), Some("les"));
        assert_eq!(d[0].answer_md, "n=0");
        assert_eq!(
            d[1].hint,
            QuizHint::Pdf {
                material_id: "mat".into(),
                page: 1,
                lesson_id: Some("les".into())
            }
        );
        assert!(d[1].answer_index.is_none());
    }

    #[test]
    fn prompts_mention_counts() {
        let c = ctx();
        assert!(quiz_prompt(&c, 10, 0.7).contains("about 7 multiple-choice and 3 short-answer"));
        assert!(cards_prompt(&c, 8).starts_with("Write 8 flashcards"));
    }
}

// ---- AI study plan ----

pub const PLAN_SYSTEM: &str = "You plan a self-learner's study schedule for a course. Order the remaining lessons \
(lecture videos and documents such as readings, problem sets, recitations and exams) into weeks so that prerequisites \
come first, readings sit next to the lectures they support, and problem sets come after the lectures they cover. \
Respect the learner's weekly time budget given in minutes (video length is known; budget ~45 minutes per reading or \
problem set unless stated). Give each week a short focus line. Use only the lesson ids provided, each exactly once. \
In the rationale, explain the main decisions in 2-4 sentences.";

pub fn plan_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["weeks", "rationale"],
        "properties": {
            "weeks": {"type": "array", "items": {
                "type": "object", "additionalProperties": false, "required": ["focus", "lessonIds"],
                "properties": {"focus": {"type": "string"}, "lessonIds": {"type": "array", "items": {"type": "string"}}}
            }},
            "rationale": {"type": "string"}
        }
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawPlan {
    pub weeks: Vec<RawWeek>,
    pub rationale: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawWeek {
    pub focus: String,
    pub lesson_ids: Vec<String>,
}

/// Keeps only valid, open lesson ids (each once) and appends any the model skipped as extra
/// weeks of similar size. Returns (weeks as (focus, ids), dropped).
pub fn normalize_plan(raw: RawPlan, open_ids: &[String]) -> (Vec<(String, Vec<String>)>, i32) {
    let valid: std::collections::HashSet<&str> = open_ids.iter().map(String::as_str).collect();
    let mut seen = std::collections::HashSet::new();
    let mut dropped = 0;
    let mut weeks: Vec<(String, Vec<String>)> = raw
        .weeks
        .into_iter()
        .map(|w| {
            let ids: Vec<String> = w
                .lesson_ids
                .into_iter()
                .filter(|id| {
                    let ok = valid.contains(id.as_str()) && seen.insert(id.clone());
                    if !ok {
                        dropped += 1;
                    }
                    ok
                })
                .collect();
            (w.focus.trim().to_string(), ids)
        })
        .filter(|w| !w.1.is_empty())
        .collect();
    let missing: Vec<String> = open_ids
        .iter()
        .filter(|id| !seen.contains(*id))
        .cloned()
        .collect();
    if !missing.is_empty() {
        let per = weeks
            .iter()
            .map(|w| w.1.len())
            .sum::<usize>()
            .checked_div(weeks.len())
            .unwrap_or(5)
            .max(1);
        for chunk in missing.chunks(per) {
            weeks.push(("Remaining lessons".into(), chunk.to_vec()));
        }
    }
    (weeks, dropped)
}

#[cfg(test)]
mod plan_tests {
    use super::*;

    #[test]
    fn plan_normalisation() {
        let raw = RawPlan {
            weeks: vec![
                RawWeek {
                    focus: "Proofs".into(),
                    lesson_ids: vec!["a".into(), "b".into(), "zzz".into()],
                },
                RawWeek {
                    focus: "Empty".into(),
                    lesson_ids: vec!["a".into()],
                },
            ],
            rationale: "r".into(),
        };
        let (w, dropped) = normalize_plan(raw, &["a".into(), "b".into(), "c".into(), "d".into()]);
        assert_eq!(dropped, 2);
        assert_eq!(w[0], ("Proofs".into(), vec!["a".into(), "b".into()]));
        assert_eq!(
            w[1],
            ("Remaining lessons".into(), vec!["c".into(), "d".into()])
        );
    }
}
