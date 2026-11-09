//! Free-form Markdown notebooks, one per lesson and one per course.

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::models::{Notebook, NotebookScope};
use crate::db::{new_id, now};
use crate::error::AppResult;

pub fn get(conn: &Connection, scope: NotebookScope, scope_id: &str) -> AppResult<Notebook> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT body_md, updated_at FROM notebooks WHERE scope = ?1 AND scope_id = ?2",
            params![scope, scope_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(match row {
        Some((body_md, updated_at)) => Notebook {
            scope,
            scope_id: scope_id.into(),
            body_md,
            updated_at: Some(updated_at),
        },
        None => Notebook {
            scope,
            scope_id: scope_id.into(),
            body_md: String::new(),
            updated_at: None,
        },
    })
}

pub fn save(
    conn: &Connection,
    scope: NotebookScope,
    scope_id: &str,
    body_md: &str,
) -> AppResult<Notebook> {
    let ts = now();
    conn.execute(
        "INSERT INTO notebooks (id, scope, scope_id, body_md, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(scope, scope_id) DO UPDATE SET body_md = excluded.body_md, updated_at = excluded.updated_at",
        params![new_id(), scope, scope_id, body_md, ts],
    )?;
    get(conn, scope, scope_id)
}

/// Removes notebooks whose lesson/course no longer exists (notebooks have no FK).
pub fn delete_for(conn: &Connection, scope: NotebookScope, scope_id: &str) -> AppResult<()> {
    conn.execute(
        "DELETE FROM notebooks WHERE scope = ?1 AND scope_id = ?2",
        params![scope, scope_id],
    )?;
    Ok(())
}

/// Appends a Markdown section to a notebook (e.g. an AI summary), separated by a blank line.
pub fn append(
    conn: &Connection,
    scope: NotebookScope,
    scope_id: &str,
    section_md: &str,
) -> AppResult<Notebook> {
    let cur = get(conn, scope, scope_id)?.body_md;
    let body = if cur.trim().is_empty() {
        section_md.trim().to_string()
    } else {
        format!("{}\n\n{}", cur.trim_end(), section_md.trim())
    };
    save(conn, scope, scope_id, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_and_fts() {
        let conn = crate::db::open_in_memory().unwrap();
        assert_eq!(get(&conn, NotebookScope::Lesson, "l1").unwrap().body_md, "");
        save(&conn, NotebookScope::Lesson, "l1", "# Raft\nterm numbers").unwrap();
        let nb = save(&conn, NotebookScope::Lesson, "l1", "# Raft\nlog matching").unwrap();
        assert_eq!(nb.body_md, "# Raft\nlog matching");
        assert!(nb.updated_at.is_some());
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM notebooks_fts WHERE notebooks_fts MATCH 'matching'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM notebooks_fts WHERE notebooks_fts MATCH 'term'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
        assert_eq!(
            get(&conn, NotebookScope::Course, "l1").unwrap().body_md,
            "",
            "scopes are separate"
        );
    }
}
