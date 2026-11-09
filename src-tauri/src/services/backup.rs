//! Database backups (`<data>/backups/*.db`) and restore, plus a full JSON export/import of the
//! library. Backups are consistent snapshots made with `VACUUM INTO`; media files (videos, PDFs,
//! attachments) live in the data folder and are not part of either.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Local, NaiveDateTime};
use rusqlite::{Connection, OpenFlags, types::ValueRef};
use serde::Serialize;
use serde_json::{Map, Value, json};
use specta::Type;
use tauri::{AppHandle, Manager};

use crate::db::{self, LATEST_VERSION};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Daily backups kept (manual and safety backups are never pruned).
pub const KEEP_DAILY: usize = 7;
const EXPORT_FORMAT: &str = "lectern-export";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BackupKind {
    Daily,
    Manual,
    /// Taken automatically before a restore or import.
    Safety,
}

impl BackupKind {
    fn tag(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Manual => "manual",
            Self::Safety => "safety",
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub path: String,
    pub file_name: String,
    pub kind: BackupKind,
    /// Local time, `YYYY-MM-DD HH:MM`.
    pub created_at: String,
    pub size_bytes: f64,
}

/// `library-20261109-143005-daily.db` → (time, kind).
pub fn parse_name(name: &str) -> Option<(NaiveDateTime, BackupKind)> {
    let rest = name.strip_prefix("library-")?.strip_suffix(".db")?;
    let (ts, kind) = rest.rsplit_once('-')?;
    let kind = match kind {
        "daily" => BackupKind::Daily,
        "manual" => BackupKind::Manual,
        "safety" => BackupKind::Safety,
        _ => return None,
    };
    Some((
        NaiveDateTime::parse_from_str(ts, "%Y%m%d-%H%M%S").ok()?,
        kind,
    ))
}

pub fn file_name(at: NaiveDateTime, kind: BackupKind) -> String {
    format!("library-{}-{}.db", at.format("%Y%m%d-%H%M%S"), kind.tag())
}

/// Backups in `dir`, newest first.
pub fn list(dir: &Path) -> AppResult<Vec<BackupInfo>> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Ok(vec![]);
    };
    let mut out: Vec<(NaiveDateTime, BackupInfo)> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let (at, kind) = parse_name(&name)?;
            let size = e.metadata().ok()?.len() as f64;
            Some((
                at,
                BackupInfo {
                    path: e.path().to_string_lossy().into_owned(),
                    file_name: name,
                    kind,
                    created_at: at.format("%Y-%m-%d %H:%M").to_string(),
                    size_bytes: size,
                },
            ))
        })
        .collect();
    out.sort_by_key(|x| std::cmp::Reverse(x.0));
    Ok(out.into_iter().map(|x| x.1).collect())
}

/// Writes a consistent snapshot of the open database into `dir`.
pub fn backup(
    conn: &Connection,
    dir: &Path,
    kind: BackupKind,
    at: NaiveDateTime,
) -> AppResult<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut path = dir.join(file_name(at, kind));
    let mut n = 1;
    while path.exists() {
        path = dir.join(file_name(at + Duration::seconds(n), kind));
        n += 1;
    }
    conn.execute("VACUUM INTO ?1", [path.to_string_lossy()])?;
    Ok(path)
}

/// Deletes daily backups beyond the newest `keep`.
pub fn prune(dir: &Path, keep: usize) -> AppResult<usize> {
    let daily: Vec<BackupInfo> = list(dir)?
        .into_iter()
        .filter(|b| b.kind == BackupKind::Daily)
        .collect();
    let mut removed = 0;
    for b in daily.iter().skip(keep) {
        if std::fs::remove_file(&b.path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// True when no daily backup was made in the last 24 hours.
pub fn daily_due(dir: &Path, now: NaiveDateTime) -> AppResult<bool> {
    Ok(!list(dir)?
        .iter()
        .filter_map(|b| parse_name(&b.file_name))
        .any(|(at, kind)| kind == BackupKind::Daily && now - at < Duration::hours(24)))
}

/// Checks that a file is a healthy Lectern database this build can open; returns its version.
pub fn validate(path: &Path) -> AppResult<i64> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| AppError::invalid("that file isn't a database"))?;
    let ok: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|_| AppError::invalid("that file isn't a Lectern backup"))?;
    if ok != "ok" {
        return Err(AppError::invalid(format!("the backup is damaged ({ok})")));
    }
    let version = db::schema_version(&conn)?;
    let has_core: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('courses', 'lessons', 'annotations')",
        [],
        |r| r.get(0),
    )?;
    if has_core != 3 || version < 1 {
        return Err(AppError::invalid("that file isn't a Lectern backup"));
    }
    if version > LATEST_VERSION {
        return Err(AppError::invalid(
            "that backup is from a newer version of Lectern; update the app first",
        ));
    }
    Ok(version)
}

/// Replaces the live database with `file`, after a safety backup. Newer migrations run on reopen.
pub fn restore(state: &AppState, file: &Path) -> AppResult<PathBuf> {
    let mut guard = state.db()?;
    restore_into(
        &mut guard,
        &state.paths.db_file(),
        file,
        &state.paths.backups_dir(),
    )
}

/// `live` is the open connection to `target`; it is swapped for the restored database.
pub fn restore_into(
    live: &mut Connection,
    target: &Path,
    file: &Path,
    backups: &Path,
) -> AppResult<PathBuf> {
    validate(file)?;
    let safety = backup(
        live,
        backups,
        BackupKind::Safety,
        Local::now().naive_local(),
    )?;
    // Copy to a temp file first so a failed copy never leaves a half-written library.
    let tmp = target.with_extension("db.restoring");
    std::fs::copy(file, &tmp)?;
    *live = Connection::open_in_memory()?; // closes the live file
    for ext in ["db-wal", "db-shm"] {
        let _ = std::fs::remove_file(target.with_extension(ext));
    }
    std::fs::rename(&tmp, target)?;
    *live = db::open(target)?;
    Ok(safety)
}

/// Hourly check; makes the daily backup when due and prunes old ones.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            if let Some(state) = app.try_state::<AppState>() {
                let dir = state.paths.backups_dir();
                let now = Local::now().naive_local();
                let r = daily_due(&dir, now).and_then(|due| {
                    if due {
                        let conn = state.db()?;
                        backup(&conn, &dir, BackupKind::Daily, now)?;
                        prune(&dir, KEEP_DAILY)?;
                    }
                    Ok(())
                });
                if let Err(e) = r {
                    log::warn!("daily backup failed: {e}");
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    });
}

// ---- JSON export / import ----

/// User tables in an order that satisfies foreign keys on insert.
pub const EXPORT_TABLES: &[&str] = &[
    "courses",
    "modules",
    "lessons",
    "materials",
    "course_meta",
    "chapters",
    "transcript_cues",
    "watch_progress",
    "annotations",
    "attachments",
    "notebooks",
    "resources",
    "study_sessions",
    "plan_items",
    "flashcards",
    "downloads",
    "quiz_questions",
    "quiz_attempts",
    "ai_usage",
    "settings",
];
/// Tables deliberately left out: caches that rebuild themselves.
pub const SKIPPED_TABLES: &[&str] = &["yt_search_cache", "quota_usage"];

fn columns(conn: &Connection, table: &str) -> AppResult<Vec<String>> {
    let mut st = conn.prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
    Ok(st
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<Result<_, _>>()?)
}

fn to_json(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => {
            json!({ "$hex": b.iter().map(|x| format!("{x:02x}")).collect::<String>() })
        }
    }
}

fn to_sql(v: &Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as V;
    match v {
        Value::Null => V::Null,
        Value::Bool(b) => V::Integer(i64::from(*b)),
        Value::Number(n) => n
            .as_i64()
            .map(V::Integer)
            .unwrap_or_else(|| V::Real(n.as_f64().unwrap_or(0.0))),
        Value::String(s) => V::Text(s.clone()),
        Value::Object(o) if o.contains_key("$hex") => {
            let h = o["$hex"].as_str().unwrap_or("");
            V::Blob(
                (0..h.len() / 2)
                    .filter_map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok())
                    .collect(),
            )
        }
        other => V::Text(other.to_string()),
    }
}

/// Every user table as rows of `{column: value}`, plus PDF page text.
pub fn export_json(conn: &Connection, exported_at: DateTime<Local>) -> AppResult<Value> {
    let mut tables = Map::new();
    for t in EXPORT_TABLES {
        let cols = columns(conn, t)?;
        let mut st = conn.prepare(&format!("SELECT * FROM \"{t}\""))?;
        let mut rows = st.query([])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let mut obj = Map::new();
            for (i, c) in cols.iter().enumerate() {
                obj.insert(c.clone(), to_json(r.get_ref(i)?));
            }
            out.push(Value::Object(obj));
        }
        tables.insert((*t).into(), Value::Array(out));
    }
    let mut st =
        conn.prepare("SELECT material_id, CAST(page AS INTEGER), text FROM material_pages_fts")?;
    let pages: Vec<Value> = st
        .query_map([], |r| {
            Ok(json!({"material_id": r.get::<_, String>(0)?, "page": r.get::<_, i64>(1)?, "text": r.get::<_, String>(2)?}))
        })?
        .collect::<Result<_, _>>()?;
    Ok(json!({
        "format": EXPORT_FORMAT,
        "app_version": env!("CARGO_PKG_VERSION"),
        "schema_version": db::schema_version(conn)?,
        "exported_at": exported_at.to_rfc3339(),
        "tables": tables,
        "material_pages": pages,
    }))
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub courses: i32,
    pub rows: i32,
}

/// Replaces the whole library with an export (in one transaction; rolled back on any error).
/// Older exports are accepted: columns they lack take their defaults.
pub fn import_json(conn: &Connection, v: &Value) -> AppResult<ImportSummary> {
    if v["format"] != EXPORT_FORMAT {
        return Err(AppError::invalid("that file isn't a Lectern export"));
    }
    let version = v["schema_version"].as_i64().unwrap_or(0);
    if version > LATEST_VERSION {
        return Err(AppError::invalid(
            "that export is from a newer version of Lectern; update the app first",
        ));
    }
    let tables = v["tables"]
        .as_object()
        .ok_or_else(|| AppError::invalid("the export has no tables"))?;
    let tx = conn.unchecked_transaction()?;
    tx.pragma_update(None, "defer_foreign_keys", "ON")?;
    for t in EXPORT_TABLES.iter().rev() {
        tx.execute(&format!("DELETE FROM \"{t}\""), [])?;
    }
    tx.execute("DELETE FROM material_pages_fts", [])?;
    let mut rows = 0;
    for t in EXPORT_TABLES {
        let Some(list) = tables.get(*t).and_then(Value::as_array) else {
            continue;
        };
        let cols = columns(&tx, t)?;
        for row in list {
            let Some(obj) = row.as_object() else { continue };
            let present: Vec<&String> = cols.iter().filter(|c| obj.contains_key(*c)).collect();
            if present.is_empty() {
                continue;
            }
            let sql = format!(
                "INSERT INTO \"{t}\" ({}) VALUES ({})",
                present
                    .iter()
                    .map(|c| format!("\"{c}\""))
                    .collect::<Vec<_>>()
                    .join(", "),
                (1..=present.len())
                    .map(|i| format!("?{i}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let vals: Vec<rusqlite::types::Value> =
                present.iter().map(|c| to_sql(&obj[*c])).collect();
            tx.execute(&sql, rusqlite::params_from_iter(vals))?;
            rows += 1;
        }
    }
    if let Some(pages) = v["material_pages"].as_array() {
        for p in pages {
            tx.execute(
                "INSERT INTO material_pages_fts (material_id, page, text) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    p["material_id"].as_str().unwrap_or(""),
                    p["page"].as_i64().unwrap_or(1),
                    p["text"].as_str().unwrap_or("")
                ],
            )?;
        }
    }
    let courses: i32 = tx.query_row("SELECT count(*) FROM courses", [], |r| r.get(0))?;
    tx.commit()?;
    Ok(ImportSummary { courses, rows })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::*;
    use crate::db::open_in_memory;
    use crate::db::repo::{annotations, search};

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn library() -> (Connection, String) {
        let conn = open_in_memory().unwrap();
        let (_, l) = annotations::tests::lesson(&conn);
        annotations::create(
            &conn,
            &annotations::tests::new(&l, AnnotationType::Note, 3.0, "pigeonhole note"),
        )
        .unwrap();
        (conn, l)
    }

    #[test]
    fn names_round_trip() {
        let t = at("2026-11-09 14:30");
        let n = file_name(t, BackupKind::Daily);
        assert_eq!(n, "library-20261109-143000-daily.db");
        assert_eq!(parse_name(&n), Some((t, BackupKind::Daily)));
        assert_eq!(parse_name("library.db"), None);
        assert_eq!(parse_name("library-20261109-143000-weird.db"), None);
    }

    #[test]
    fn backup_restore_validate_and_prune() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _) = library();
        let p = backup(
            &conn,
            dir.path(),
            BackupKind::Manual,
            at("2026-11-09 10:00"),
        )
        .unwrap();
        assert_eq!(validate(&p).unwrap(), LATEST_VERSION);
        // The snapshot has the note.
        let snap = Connection::open(&p).unwrap();
        let n: i64 = snap
            .query_row("SELECT count(*) FROM annotations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);

        let junk = dir.path().join("junk.db");
        std::fs::write(&junk, b"not a database at all").unwrap();
        assert!(validate(&junk).is_err());
        let newer = dir.path().join("newer.db");
        {
            let c = Connection::open(&newer).unwrap();
            c.execute_batch(
                "CREATE TABLE courses(x); CREATE TABLE lessons(x); CREATE TABLE annotations(x);",
            )
            .unwrap();
            c.pragma_update(None, "user_version", LATEST_VERSION + 1)
                .unwrap();
        }
        assert!(
            validate(&newer)
                .unwrap_err()
                .to_string()
                .contains("newer version")
        );

        for d in 1..=9 {
            backup(
                &conn,
                dir.path(),
                BackupKind::Daily,
                at(&format!("2026-11-0{d} 09:00")),
            )
            .unwrap();
        }
        assert_eq!(prune(dir.path(), KEEP_DAILY).unwrap(), 2);
        let left = list(dir.path()).unwrap();
        assert_eq!(
            left.iter().filter(|b| b.kind == BackupKind::Daily).count(),
            7
        );
        assert_eq!(
            left.iter().filter(|b| b.kind == BackupKind::Manual).count(),
            1
        );
        assert_eq!(left[0].created_at, "2026-11-09 10:00", "newest first");
        assert!(!daily_due(dir.path(), at("2026-11-09 20:00")).unwrap());
        assert!(daily_due(dir.path(), at("2026-11-10 10:00")).unwrap());
    }

    #[test]
    fn restore_brings_back_the_backup() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("library.db");
        let backups = dir.path().join("backups");
        let mut live = db::open(&target).unwrap();
        let (_, l) = annotations::tests::lesson(&live);
        let snap = backup(&live, &backups, BackupKind::Manual, at("2026-11-09 10:00")).unwrap();
        annotations::create(
            &live,
            &annotations::tests::new(&l, AnnotationType::Note, 1.0, "after"),
        )
        .unwrap();
        let count = |c: &Connection| -> i64 {
            c.query_row("SELECT count(*) FROM annotations", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(count(&live), 1);
        let safety = restore_into(&mut live, &target, &snap, &backups).unwrap();
        assert_eq!(count(&live), 0, "the note made after the backup is gone");
        assert_eq!(
            count(&Connection::open(&safety).unwrap()),
            1,
            "safety backup keeps it"
        );
        // Writes still work on the reopened database.
        annotations::create(
            &live,
            &annotations::tests::new(&l, AnnotationType::Note, 2.0, "again"),
        )
        .unwrap();
        assert!(
            restore_into(&mut live, &target, &dir.path().join("missing.db"), &backups).is_err()
        );
        assert_eq!(count(&live), 1, "a failed restore leaves the library alone");
    }

    #[test]
    fn every_table_is_exported_or_skipped() {
        let conn = open_in_memory().unwrap();
        let mut st = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
                   AND name NOT LIKE '%_fts' AND name NOT LIKE '%_fts_%'",
            )
            .unwrap();
        let names: Vec<String> = st
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for n in names {
            assert!(
                EXPORT_TABLES.contains(&n.as_str()) || SKIPPED_TABLES.contains(&n.as_str()),
                "table {n} is neither exported nor skipped"
            );
        }
    }

    #[test]
    fn json_round_trip_restores_rows_and_search() {
        let (src, l) = library();
        conn_page_text(&src, &l);
        let export = export_json(&src, Local::now()).unwrap();
        let text = serde_json::to_string(&export).unwrap();

        let dst = open_in_memory().unwrap();
        annotations::tests::lesson(&dst); // something to be replaced
        let s = import_json(&dst, &serde_json::from_str(&text).unwrap()).unwrap();
        assert_eq!(s.courses, 1);
        for t in EXPORT_TABLES {
            let count = |c: &Connection| -> i64 {
                c.query_row(&format!("SELECT count(*) FROM \"{t}\""), [], |r| r.get(0))
                    .unwrap()
            };
            assert_eq!(count(&src), count(&dst), "table {t}");
        }
        let hits = search::global_search(&dst, "pigeonhole", 5).unwrap();
        assert_eq!(
            hits.annotations.len(),
            1,
            "annotation FTS rebuilt by triggers"
        );
        assert_eq!(hits.materials.len(), 1, "PDF page text restored");
        assert!(import_json(&dst, &json!({"format": "other"})).is_err());
        assert!(
            import_json(
                &dst,
                &json!({"format": EXPORT_FORMAT, "schema_version": 999, "tables": {}})
            )
            .is_err()
        );
    }

    fn conn_page_text(conn: &Connection, lesson_id: &str) {
        let course = crate::db::repo::lessons::course_id_of_lesson(conn, lesson_id).unwrap();
        let m = crate::db::repo::materials::insert(
            conn,
            &NewMaterial {
                course_id: course,
                lesson_id: Some(lesson_id.into()),
                kind: "lecture_notes".into(),
                title: "Notes".into(),
                file_type: Some("application/pdf".into()),
                ..Default::default()
            },
        )
        .unwrap();
        crate::db::repo::materials::set_pages(conn, &m, &["the pigeonhole principle".into()])
            .unwrap();
    }
}
