//! Flashcards with SM-2 scheduling (SPEC §7.8).

use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db::models::*;
use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};
use crate::services::sm2::{self, CardState, Grade};

const SELECT: &str =
    "SELECT f.id, f.lesson_id, l.title, c.title, f.source_sec, f.front_md, f.back_md, f.ease,
        f.interval_days, f.repetitions, f.due_at, f.last_reviewed_at, f.ai_generated, f.created_at,
        f.source_material_id, f.source_page
   FROM flashcards f
   LEFT JOIN lessons l ON l.id = f.lesson_id
   LEFT JOIN modules m ON m.id = l.module_id
   LEFT JOIN courses c ON c.id = m.course_id";

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<Flashcard> {
    Ok(Flashcard {
        id: r.get(0)?,
        lesson_id: r.get(1)?,
        lesson_title: r.get(2)?,
        course_title: r.get(3)?,
        source_sec: r.get(4)?,
        source_material_id: r.get(14)?,
        source_page: r.get(15)?,
        front_md: r.get(5)?,
        back_md: r.get(6)?,
        ease: r.get(7)?,
        interval_days: r.get(8)?,
        repetitions: r.get(9)?,
        due_at: r.get(10)?,
        last_reviewed_at: r.get(11)?,
        ai_generated: r.get(12)?,
        created_at: r.get(13)?,
    })
}

fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Flashcard> {
    conn.query_row(&format!("{SELECT} WHERE f.id = ?1"), [id], map)
        .optional()?
        .ok_or_else(|| AppError::not_found("flashcard"))
}

pub fn create(conn: &Connection, n: &NewFlashcard) -> AppResult<Flashcard> {
    create_with(conn, n, false)
}

/// Creates a card, marking it AI-generated when `ai` is set (cleared on the first manual edit).
pub fn create_with(conn: &Connection, n: &NewFlashcard, ai: bool) -> AppResult<Flashcard> {
    let (front, back) = (n.front_md.trim(), n.back_md.trim());
    if front.is_empty() {
        return Err(AppError::invalid("the front of a card can't be empty"));
    }
    if let Some(l) = &n.lesson_id {
        super::lessons::get_lesson(conn, l)?;
    }
    if n.source_page.is_some_and(|p| p < 1)
        || (n.source_page.is_some() && n.source_material_id.is_none())
    {
        return Err(AppError::invalid(
            "a source page needs a document and must be 1 or more",
        ));
    }
    if let Some(m) = &n.source_material_id {
        super::materials::get(conn, m)?;
    }
    let id = new_id();
    let ts = now();
    conn.execute(
        "INSERT INTO flashcards (id, lesson_id, source_sec, front_md, back_md, due_at, ai_generated, created_at, updated_at,
             source_material_id, source_page)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, ?6, ?8, ?9)",
        params![id, n.lesson_id, n.source_sec, front, back, ts, ai, n.source_material_id, n.source_page],
    )?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, front_md: &str, back_md: &str) -> AppResult<Flashcard> {
    if front_md.trim().is_empty() {
        return Err(AppError::invalid("the front of a card can't be empty"));
    }
    let n = conn.execute(
        "UPDATE flashcards SET front_md = ?2, back_md = ?3, ai_generated = 0, updated_at = ?4 WHERE id = ?1",
        params![id, front_md.trim(), back_md.trim(), now()],
    )?;
    if n == 0 {
        return Err(AppError::not_found("flashcard"));
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    if conn.execute("DELETE FROM flashcards WHERE id = ?1", [id])? == 0 {
        return Err(AppError::not_found("flashcard"));
    }
    Ok(())
}

/// All cards, or one lesson's, newest first.
pub fn list(conn: &Connection, lesson_id: Option<&str>) -> AppResult<Vec<Flashcard>> {
    let mut st = conn.prepare(&format!(
        "{SELECT} WHERE (?1 IS NULL OR f.lesson_id = ?1) ORDER BY f.created_at DESC"
    ))?;
    Ok(st.query_map([lesson_id], map)?.collect::<Result<_, _>>()?)
}

/// Cards due at `at`, most overdue first.
pub fn due(conn: &Connection, at: DateTime<Utc>, limit: i32) -> AppResult<Vec<Flashcard>> {
    let mut st = conn.prepare(&format!(
        "{SELECT} WHERE f.due_at <= ?1 ORDER BY f.due_at LIMIT ?2"
    ))?;
    Ok(st
        .query_map(params![iso(at), limit.clamp(1, 500)], map)?
        .collect::<Result<_, _>>()?)
}

/// Applies an SM-2 review. "Again" makes the card due again in 10 minutes (same session).
pub fn review(
    conn: &Connection,
    id: &str,
    grade: Grade,
    at: DateTime<Utc>,
) -> AppResult<Flashcard> {
    let c = get(conn, id)?;
    let next = sm2::review(
        CardState {
            ease: c.ease,
            interval_days: c.interval_days,
            repetitions: c.repetitions,
        },
        grade,
    );
    let due = if grade == Grade::Again {
        at + Duration::minutes(10)
    } else {
        at + Duration::days(next.interval_days as i64)
    };
    conn.execute(
        "UPDATE flashcards SET ease = ?2, interval_days = ?3, repetitions = ?4, due_at = ?5,
                last_reviewed_at = ?6, updated_at = ?6 WHERE id = ?1",
        params![
            id,
            next.ease,
            next.interval_days,
            next.repetitions,
            iso(due),
            iso(at)
        ],
    )?;
    get(conn, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn create_review_and_due() {
        let conn = open_in_memory().unwrap();
        let c = create(
            &conn,
            &NewFlashcard {
                lesson_id: None,
                source_sec: None,
                source_material_id: None,
                source_page: None,
                front_md: "What is Raft?".into(),
                back_md: "Consensus".into(),
            },
        )
        .unwrap();
        let now = Utc::now() + Duration::seconds(1);
        assert_eq!(
            due(&conn, now, 10).unwrap().len(),
            1,
            "new cards are due immediately"
        );
        let r = review(&conn, &c.id, Grade::Good, now).unwrap();
        assert_eq!(r.interval_days, 1);
        assert!(due(&conn, now, 10).unwrap().is_empty());
        assert_eq!(due(&conn, now + Duration::days(1), 10).unwrap().len(), 1);
        let again = review(&conn, &c.id, Grade::Again, now).unwrap();
        assert_eq!(again.repetitions, 0);
        assert_eq!(
            due(&conn, now + Duration::minutes(11), 10).unwrap().len(),
            1
        );
        assert!(
            create(
                &conn,
                &NewFlashcard {
                    lesson_id: None,
                    source_sec: None,
                    source_material_id: None,
                    source_page: None,
                    front_md: " ".into(),
                    back_md: "".into()
                }
            )
            .is_err()
        );
        update(&conn, &c.id, "Q", "A").unwrap();
        delete(&conn, &c.id).unwrap();
        assert!(list(&conn, None).unwrap().is_empty());
    }
}
