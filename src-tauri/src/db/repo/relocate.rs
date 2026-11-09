//! Keeps stored media paths valid when the data folder moves (e.g. to an external drive),
//! or when a library from another machine is restored.
//!
//! Media paths are stored as absolute paths under `<data_dir>/media`. When the data dir
//! changes, each path outside the current one is rebased onto `<data_dir>/media/<rel>`,
//! but only if that file actually exists there; a missing file is left as it was.

use std::path::{Component, Path, PathBuf};

use rusqlite::{Connection, params};

use crate::error::AppResult;

/// Every column that stores an absolute path inside the media folder.
/// Add new path columns here.
pub const PATH_COLUMNS: &[(&str, &str)] = &[
    ("lessons", "local_video_path"),
    ("lessons", "thumbnail_path"),
    ("materials", "local_path"),
    ("courses", "cover_path"),
];

/// Where `stored` lives under `data_dir`, if it points into some other data dir's `media` folder
/// and the file exists in this one.
fn rebased(stored: &str, data_dir: &Path) -> Option<PathBuf> {
    let p = Path::new(stored);
    if !p.is_absolute() || p.starts_with(data_dir) {
        return None;
    }
    let comps: Vec<Component> = p.components().collect();
    let i = comps.iter().rposition(|c| c.as_os_str() == "media")?;
    let rel: PathBuf = comps[i + 1..].iter().collect();
    if rel.as_os_str().is_empty() {
        return None;
    }
    let candidate = data_dir.join("media").join(rel);
    candidate.exists().then_some(candidate)
}

/// Rewrites stale media paths to the current data dir. Returns how many values changed.
pub fn rebase_media_paths(conn: &Connection, data_dir: &Path) -> AppResult<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut changed = 0;
    for (table, col) in PATH_COLUMNS {
        let rows: Vec<(i64, String)> = {
            let mut stmt = tx.prepare(&format!(
                "SELECT rowid, {col} FROM {table} WHERE {col} IS NOT NULL"
            ))?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?
        };
        for (rowid, stored) in rows {
            if let Some(new) = rebased(&stored, data_dir) {
                tx.execute(
                    &format!("UPDATE {table} SET {col} = ?1 WHERE rowid = ?2"),
                    params![new.to_string_lossy(), rowid],
                )?;
                changed += 1;
            }
        }
    }
    tx.commit()?;
    Ok(changed)
}

/// [`rebase_media_paths`], logging the outcome instead of failing (used at startup/restore).
pub fn rebase_and_log(conn: &Connection, data_dir: &Path) {
    match rebase_media_paths(conn, data_dir) {
        Ok(0) => {}
        Ok(n) => log::info!("rebased {n} media path(s) onto {}", data_dir.display()),
        Err(e) => log::warn!("rebasing media paths failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn course(conn: &Connection, id: &str, cover: Option<&str>) {
        conn.execute(
            "INSERT INTO courses (id, title, slug, cover_path, created_at, updated_at)
             VALUES (?1, 'C', ?1, ?2, '2026-01-01', '2026-01-01')",
            params![id, cover],
        )
        .unwrap();
    }

    fn cover(conn: &Connection, id: &str) -> Option<String> {
        conn.query_row("SELECT cover_path FROM courses WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap()
    }

    #[test]
    fn rebases_only_existing_files_from_other_data_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("Drive").join("Lectern");
        std::fs::create_dir_all(data.join("media/course-a")).unwrap();
        std::fs::write(data.join("media/course-a/cover.jpg"), b"x").unwrap();
        let conn = open_in_memory().unwrap();

        course(
            &conn,
            "moved",
            Some("/Users/me/Lectern/media/course-a/cover.jpg"),
        );
        course(
            &conn,
            "missing",
            Some("/Users/me/Lectern/media/course-b/cover.jpg"),
        );
        let here = data
            .join("media/course-a/cover.jpg")
            .to_string_lossy()
            .into_owned();
        course(&conn, "current", Some(&here));
        course(&conn, "none", None);

        assert_eq!(rebase_media_paths(&conn, &data).unwrap(), 1);
        assert_eq!(cover(&conn, "moved").as_deref(), Some(here.as_str()));
        assert_eq!(
            cover(&conn, "missing").as_deref(),
            Some("/Users/me/Lectern/media/course-b/cover.jpg")
        );
        assert_eq!(cover(&conn, "current").as_deref(), Some(here.as_str()));
        assert_eq!(cover(&conn, "none"), None);
        // Idempotent.
        assert_eq!(rebase_media_paths(&conn, &data).unwrap(), 0);
    }

    #[test]
    fn every_path_column_exists() {
        let conn = open_in_memory().unwrap();
        for (t, c) in PATH_COLUMNS {
            conn.prepare(&format!("SELECT {c} FROM {t} LIMIT 0"))
                .unwrap();
        }
    }
}
