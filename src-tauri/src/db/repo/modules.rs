use rusqlite::{Connection, OptionalExtension, params};

use crate::db::models::Module;
use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};

fn map(r: &rusqlite::Row<'_>) -> rusqlite::Result<Module> {
    Ok(Module {
        id: r.get(0)?,
        course_id: r.get(1)?,
        title: r.get(2)?,
        position: r.get(3)?,
    })
}

pub fn list_modules(conn: &Connection, course_id: &str) -> AppResult<Vec<Module>> {
    let mut stmt = conn.prepare(
        "SELECT id, course_id, title, position FROM modules WHERE course_id = ?1 ORDER BY position",
    )?;
    Ok(stmt
        .query_map([course_id], map)?
        .collect::<Result<_, _>>()?)
}

pub fn get_module(conn: &Connection, id: &str) -> AppResult<Module> {
    conn.query_row(
        "SELECT id, course_id, title, position FROM modules WHERE id = ?1",
        [id],
        map,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("module"))
}

pub fn create_module(conn: &Connection, course_id: &str, title: &str) -> AppResult<Module> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::invalid("module title is required"));
    }
    super::courses::get_course(conn, course_id)?;
    let pos: i32 = conn.query_row(
        "SELECT coalesce(max(position) + 1, 0) FROM modules WHERE course_id = ?1",
        [course_id],
        |r| r.get(0),
    )?;
    let id = new_id();
    let ts = now();
    conn.execute(
        "INSERT INTO modules (id, course_id, title, position, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?5)",
        params![id, course_id, title, pos, ts],
    )?;
    get_module(conn, &id)
}

pub fn rename_module(conn: &Connection, id: &str, title: &str) -> AppResult<Module> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::invalid("module title is required"));
    }
    let n = conn.execute(
        "UPDATE modules SET title=?2, updated_at=?3 WHERE id=?1",
        params![id, title, now()],
    )?;
    if n == 0 {
        return Err(AppError::not_found("module"));
    }
    get_module(conn, id)
}

/// Deletes a module. Its lessons are moved to the end of an adjacent module so no
/// notes or progress are lost. Deleting the only module of a course with lessons is refused.
pub fn delete_module(conn: &Connection, id: &str) -> AppResult<()> {
    let m = get_module(conn, id)?;
    let tx = conn.unchecked_transaction()?;
    let lesson_count: i32 = tx.query_row(
        "SELECT count(*) FROM lessons WHERE module_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    if lesson_count > 0 {
        let target: Option<String> = tx
            .query_row(
                "SELECT id FROM modules WHERE course_id = ?1 AND id != ?2
                 ORDER BY CASE WHEN position < ?3 THEN 0 ELSE 1 END, abs(position - ?3) LIMIT 1",
                params![m.course_id, id, m.position],
                |r| r.get(0),
            )
            .optional()?;
        let Some(target) = target else {
            return Err(AppError::invalid(
                "cannot delete the only module while it still has lessons",
            ));
        };
        let base: i32 = tx.query_row(
            "SELECT coalesce(max(position) + 1, 0) FROM lessons WHERE module_id = ?1",
            [&target],
            |r| r.get(0),
        )?;
        tx.execute(
            "UPDATE lessons SET module_id = ?1, position = ?2 + position, updated_at = ?4 WHERE module_id = ?3",
            params![target, base, id, now()],
        )?;
        super::lessons::renumber(&tx, &target)?;
    }
    tx.execute("DELETE FROM modules WHERE id = ?1", [id])?;
    renumber_modules(&tx, &m.course_id)?;
    tx.commit()?;
    Ok(())
}

pub fn reorder_modules(conn: &Connection, course_id: &str, ids: &[String]) -> AppResult<()> {
    let existing = list_modules(conn, course_id)?;
    let mut a: Vec<&str> = existing.iter().map(|m| m.id.as_str()).collect();
    let mut b: Vec<&str> = ids.iter().map(String::as_str).collect();
    a.sort_unstable();
    b.sort_unstable();
    if a != b {
        return Err(AppError::invalid(
            "module order must list every module of the course exactly once",
        ));
    }
    let tx = conn.unchecked_transaction()?;
    let ts = now();
    for (i, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE modules SET position=?2, updated_at=?3 WHERE id=?1",
            params![id, i as i32, ts],
        )?;
    }
    tx.commit()?;
    Ok(())
}

fn renumber_modules(conn: &Connection, course_id: &str) -> AppResult<()> {
    let ids: Vec<String> = list_modules(conn, course_id)?
        .into_iter()
        .map(|m| m.id)
        .collect();
    for (i, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE modules SET position=?2 WHERE id=?1",
            params![id, i as i32],
        )?;
    }
    Ok(())
}
