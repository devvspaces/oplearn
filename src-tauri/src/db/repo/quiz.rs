//! Quiz banks: AI-generated (or manual) MCQ and short-answer questions with hints that link to a
//! video moment or a PDF page.

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::models::*;
use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};

/// Validates and tidies a draft: MCQ needs 3–5 distinct choices and one correct index; short
/// answer needs a model answer. Returns a human-readable reason on failure.
pub fn validate(mut d: QuizDraft) -> Result<QuizDraft, String> {
    d.prompt_md = d.prompt_md.trim().to_string();
    d.answer_md = d.answer_md.trim().to_string();
    d.explanation_md = d.explanation_md.trim().to_string();
    if d.prompt_md.is_empty() {
        return Err("empty question".into());
    }
    match d.kind {
        QuizKind::Mcq => {
            d.choices = d
                .choices
                .into_iter()
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect();
            if !(3..=5).contains(&d.choices.len()) {
                return Err(format!("{} choices (need 3–5)", d.choices.len()));
            }
            let mut seen = HashSet::new();
            if !d.choices.iter().all(|c| seen.insert(c.to_lowercase())) {
                return Err("duplicate choices".into());
            }
            match d.answer_index {
                Some(i) if i >= 0 && (i as usize) < d.choices.len() => {}
                _ => return Err("answer index out of range".into()),
            }
            if d.answer_md.is_empty() {
                d.answer_md = d.choices[d.answer_index.unwrap_or(0) as usize].clone();
            }
        }
        QuizKind::Short => {
            d.choices.clear();
            d.answer_index = None;
            if d.answer_md.is_empty() {
                return Err("short answer without a model answer".into());
            }
        }
    }
    Ok(d)
}

const SELECT: &str = "SELECT q.id, q.course_id, q.lesson_id, q.kind, q.prompt_md, q.choices_json, q.answer_index, q.answer_md,
        q.explanation_md, q.hint_kind, q.hint_sec, q.hint_material_id, q.hint_page, q.difficulty, q.ai_generated, q.created_at,
        l.title,
        (SELECT count(*) FROM quiz_attempts a WHERE a.question_id = q.id),
        (SELECT count(*) FROM quiz_attempts a WHERE a.question_id = q.id AND a.correct),
        (SELECT a.correct FROM quiz_attempts a WHERE a.question_id = q.id ORDER BY a.answered_at DESC, a.rowid DESC LIMIT 1),
        (SELECT mt.lesson_id FROM materials mt WHERE mt.id = q.hint_material_id)
   FROM quiz_questions q LEFT JOIN lessons l ON l.id = q.lesson_id";

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<QuizQuestion> {
    let kind: String = r.get(3)?;
    let hint_kind: String = r.get(9)?;
    let lesson_id: Option<String> = r.get(2)?;
    let hint = match hint_kind.as_str() {
        "video" => match (lesson_id.clone(), r.get::<_, Option<f64>>(10)?) {
            (Some(l), Some(sec)) => QuizHint::Video { lesson_id: l, sec },
            _ => QuizHint::None,
        },
        "pdf" => match (
            r.get::<_, Option<String>>(11)?,
            r.get::<_, Option<i32>>(12)?,
        ) {
            (Some(material_id), Some(page)) => QuizHint::Pdf {
                material_id,
                page,
                lesson_id: r.get(20)?,
            },
            _ => QuizHint::None,
        },
        _ => QuizHint::None,
    };
    let choices: Vec<String> = r
        .get::<_, Option<String>>(5)?
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    Ok(QuizQuestion {
        id: r.get(0)?,
        course_id: r.get(1)?,
        draft: QuizDraft {
            lesson_id,
            kind: if kind == "mcq" {
                QuizKind::Mcq
            } else {
                QuizKind::Short
            },
            prompt_md: r.get(4)?,
            choices,
            answer_index: r.get(6)?,
            answer_md: r.get(7)?,
            explanation_md: r.get(8)?,
            hint,
            difficulty: r.get(13)?,
        },
        ai_generated: r.get(14)?,
        created_at: r.get(15)?,
        lesson_title: r.get(16)?,
        attempts: r.get(17)?,
        correct: r.get(18)?,
        last_correct: r.get(19)?,
    })
}

pub fn get(conn: &Connection, id: &str) -> AppResult<QuizQuestion> {
    conn.query_row(&format!("{SELECT} WHERE q.id = ?1"), [id], map)
        .optional()?
        .ok_or_else(|| AppError::not_found("question"))
}

/// Saves drafts (validated again) and returns how many were stored.
pub fn save(
    conn: &Connection,
    course_id: &str,
    drafts: &[QuizDraft],
    ai_generated: bool,
) -> AppResult<i32> {
    let tx = conn.unchecked_transaction()?;
    let ts = now();
    let mut n = 0;
    for d in drafts {
        let Ok(d) = validate(d.clone()) else { continue };
        let (hk, hs, hm, hp) = match &d.hint {
            QuizHint::None => ("none", None, None, None),
            QuizHint::Video { sec, .. } => ("video", Some(*sec), None, None),
            QuizHint::Pdf {
                material_id, page, ..
            } => ("pdf", None, Some(material_id.clone()), Some(*page)),
        };
        // A video hint implies its lesson.
        let lesson = match &d.hint {
            QuizHint::Video { lesson_id, .. } => Some(lesson_id.clone()),
            _ => d.lesson_id.clone(),
        };
        tx.execute(
            "INSERT INTO quiz_questions (id, course_id, lesson_id, kind, prompt_md, choices_json, answer_index, answer_md,
                    explanation_md, hint_kind, hint_sec, hint_material_id, hint_page, difficulty, ai_generated, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                new_id(),
                course_id,
                lesson,
                if d.kind == QuizKind::Mcq { "mcq" } else { "short" },
                d.prompt_md,
                (d.kind == QuizKind::Mcq).then(|| serde_json::to_string(&d.choices).unwrap_or_default()),
                d.answer_index,
                d.answer_md,
                d.explanation_md,
                hk,
                hs,
                hm,
                hp,
                d.difficulty,
                ai_generated,
                ts
            ],
        )?;
        n += 1;
    }
    tx.commit()?;
    Ok(n)
}

/// Questions for a lesson, a module, or a whole course.
pub fn list(conn: &Connection, scope: QuizScope, scope_id: &str) -> AppResult<Vec<QuizQuestion>> {
    let filter = match scope {
        QuizScope::Lesson => "q.lesson_id = ?1",
        QuizScope::Module => "q.lesson_id IN (SELECT id FROM lessons WHERE module_id = ?1)",
        QuizScope::Course => "q.course_id = ?1",
    };
    let mut st = conn.prepare(&format!("{SELECT} WHERE {filter} ORDER BY q.created_at"))?;
    Ok(st.query_map([scope_id], map)?.collect::<Result<_, _>>()?)
}

pub fn record_answer(conn: &Connection, id: &str, correct: bool) -> AppResult<()> {
    get(conn, id)?;
    conn.execute(
        "INSERT INTO quiz_attempts (id, question_id, correct, answered_at) VALUES (?1, ?2, ?3, ?4)",
        params![new_id(), id, correct, now()],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    if conn.execute("DELETE FROM quiz_questions WHERE id = ?1", [id])? == 0 {
        return Err(AppError::not_found("question"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::repo::annotations::tests::lesson;

    fn mcq(choices: &[&str], idx: i32) -> QuizDraft {
        QuizDraft {
            lesson_id: None,
            kind: QuizKind::Mcq,
            prompt_md: " What is 2+2? ".into(),
            choices: choices.iter().map(|s| s.to_string()).collect(),
            answer_index: Some(idx),
            answer_md: "".into(),
            explanation_md: "arithmetic".into(),
            hint: QuizHint::None,
            difficulty: Some("easy".into()),
        }
    }

    #[test]
    fn validation_rules() {
        let ok = validate(mcq(&["3", "4", "5"], 1)).unwrap();
        assert_eq!(ok.prompt_md, "What is 2+2?");
        assert_eq!(ok.answer_md, "4");
        assert!(validate(mcq(&["3", "4"], 1)).is_err(), "too few");
        assert!(validate(mcq(&["3", "4", "4 "], 1)).is_err(), "duplicate");
        assert!(validate(mcq(&["3", "4", "5"], 3)).is_err(), "index");
        let short = QuizDraft {
            kind: QuizKind::Short,
            answer_md: "".into(),
            ..mcq(&[], 0)
        };
        assert!(validate(short.clone()).is_err());
        let s = validate(QuizDraft {
            answer_md: "four".into(),
            ..short
        })
        .unwrap();
        assert!(s.choices.is_empty() && s.answer_index.is_none());
    }

    #[test]
    fn save_list_answer_and_hints() {
        let conn = open_in_memory().unwrap();
        let (course, l) = lesson(&conn);
        let video = QuizDraft {
            hint: QuizHint::Video {
                lesson_id: l.clone(),
                sec: 754.0,
            },
            ..mcq(&["3", "4", "5"], 1)
        };
        let bad = mcq(&["a"], 0);
        assert_eq!(
            save(&conn, &course, &[video, bad], true).unwrap(),
            1,
            "invalid drafts are skipped"
        );
        let qs = list(&conn, QuizScope::Course, &course).unwrap();
        assert_eq!(qs.len(), 1);
        assert_eq!(
            qs[0].draft.hint,
            QuizHint::Video {
                lesson_id: l.clone(),
                sec: 754.0
            }
        );
        assert_eq!(qs[0].draft.lesson_id.as_deref(), Some(l.as_str()));
        assert_eq!(list(&conn, QuizScope::Lesson, &l).unwrap().len(), 1);
        record_answer(&conn, &qs[0].id, false).unwrap();
        record_answer(&conn, &qs[0].id, true).unwrap();
        let q = get(&conn, &qs[0].id).unwrap();
        assert_eq!((q.attempts, q.correct, q.last_correct), (2, 1, Some(true)));
        delete(&conn, &q.id).unwrap();
        assert!(list(&conn, QuizScope::Course, &course).unwrap().is_empty());
    }
}
