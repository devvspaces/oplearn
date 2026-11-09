use rusqlite::{Connection, OptionalExtension, params};

use crate::db::models::WatchProgress;
use crate::db::now;
use crate::error::{AppError, AppResult};

pub fn get_progress(conn: &Connection, lesson_id: &str) -> AppResult<Option<WatchProgress>> {
    Ok(conn
        .query_row(
            "SELECT lesson_id, last_position_sec, watched_ratio FROM watch_progress WHERE lesson_id = ?1",
            [lesson_id],
            |r| Ok(WatchProgress { lesson_id: r.get(0)?, last_position_sec: r.get(1)?, watched_ratio: r.get(2)? }),
        )
        .optional()?)
}

pub fn save_position(conn: &Connection, lesson_id: &str, position_sec: f64) -> AppResult<()> {
    if !position_sec.is_finite() || position_sec < 0.0 {
        return Err(AppError::invalid("position must be a non-negative number"));
    }
    let ts = now();
    conn.execute(
        "INSERT INTO watch_progress (lesson_id, last_position_sec, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)
         ON CONFLICT(lesson_id) DO UPDATE SET last_position_sec = excluded.last_position_sec,
                                               updated_at = excluded.updated_at",
        params![lesson_id, position_sec, ts],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::*;
    use crate::db::open_in_memory;
    use crate::db::repo::{courses, lessons, modules};

    #[test]
    fn position_upserts() {
        let conn = open_in_memory().unwrap();
        let c = courses::create_course(
            &conn,
            &NewCourse {
                title: "C".into(),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        let m = modules::create_module(&conn, &c.id, "M").unwrap();
        let l = lessons::create_lesson(
            &conn,
            &NewLesson {
                module_id: m.id,
                lesson_type: LessonType::Video,
                title: "L".into(),
                description: None,
                url: None,
            },
        )
        .unwrap();
        assert!(get_progress(&conn, &l.id).unwrap().is_none());
        save_position(&conn, &l.id, 42.0).unwrap();
        save_position(&conn, &l.id, 99.5).unwrap();
        assert_eq!(
            get_progress(&conn, &l.id)
                .unwrap()
                .unwrap()
                .last_position_sec,
            99.5
        );
        assert_eq!(
            lessons::get_lesson(&conn, &l.id).unwrap().last_position_sec,
            Some(99.5)
        );
        assert!(save_position(&conn, &l.id, -1.0).is_err());
        assert!(
            save_position(&conn, "nope", 1.0).is_err(),
            "FK must reject unknown lessons"
        );
    }
}
