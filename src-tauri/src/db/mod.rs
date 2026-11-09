//! SQLite access: connection setup, migrations and repositories.

pub mod models;
pub mod repo;

use std::path::Path;

use rusqlite::Connection;

use crate::error::AppResult;

/// Ordered list of migrations. Index + 1 is the schema version stored in `PRAGMA user_version`.
/// Never edit a released migration; append a new one instead.
const MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/0001_init.sql"),
    include_str!("../../migrations/0002_search.sql"),
    include_str!("../../migrations/0003_downloads.sql"),
    include_str!("../../migrations/0004_progress.sql"),
    include_str!("../../migrations/0005_search_accounts_cards.sql"),
    include_str!("../../migrations/0006_ai_quiz_providers.sql"),
    include_str!("../../migrations/0007_pdf_anchors_plan_order.sql"),
];

pub fn open(path: &Path) -> AppResult<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> AppResult<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> AppResult<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

/// The schema version this build creates (number of migrations).
pub const LATEST_VERSION: i64 = MIGRATIONS.len() as i64;

pub fn schema_version(conn: &Connection) -> AppResult<i64> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

fn migrate(conn: &Connection) -> AppResult<()> {
    let current = schema_version(conn)? as usize;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let version = i + 1;
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version as i64)?;
        tx.commit()?;
        log::info!("applied migration {version}");
    }
    Ok(())
}

/// Current time as ISO 8601 UTC with millisecond precision.
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn new_id() -> String {
    ulid::Ulid::new().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_create_all_tables() {
        let conn = open_in_memory().unwrap();
        assert_eq!(schema_version(&conn).unwrap(), MIGRATIONS.len() as i64);
        let expected = [
            "courses",
            "modules",
            "lessons",
            "chapters",
            "transcript_cues",
            "transcript_fts",
            "watch_progress",
            "annotations",
            "annotations_fts",
            "attachments",
            "notebooks",
            "resources",
            "study_sessions",
            "plan_items",
            "flashcards",
            "downloads",
            "settings",
            "lessons_fts",
            "notebooks_fts",
        ];
        for t in expected {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name = ?1",
                    [t],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing table {t}");
        }
    }

    #[test]
    fn migrations_are_idempotent_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.db");
        drop(open(&p).unwrap());
        let conn = open(&p).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), MIGRATIONS.len() as i64);
    }

    #[test]
    fn fts5_triggers_index_transcripts() {
        let conn = open_in_memory().unwrap();
        let ts = now();
        conn.execute_batch(&format!(
            "INSERT INTO courses(id,title,slug,created_at,updated_at) VALUES ('c','C','c','{ts}','{ts}');
             INSERT INTO modules(id,course_id,title,position,created_at,updated_at) VALUES ('m','c','M',0,'{ts}','{ts}');
             INSERT INTO lessons(id,module_id,position,type,title,created_at,updated_at) VALUES ('l','m',0,'video','L','{ts}','{ts}');
             INSERT INTO transcript_cues(lesson_id,lang,start_sec,end_sec,text,created_at,updated_at)
               VALUES ('l','en',12.5,15.0,'raft leader election is tricky','{ts}','{ts}');"
        ))
        .unwrap();
        let start: f64 = conn
            .query_row(
                "SELECT c.start_sec FROM transcript_fts f JOIN transcript_cues c ON c.id = f.rowid
                 WHERE transcript_fts MATCH 'elections'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(start, 12.5);
    }
}
