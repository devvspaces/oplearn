//! Key/value settings stored as JSON.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};

use crate::db::now;
use crate::error::AppResult;

pub fn get_raw(conn: &Connection, key: &str) -> AppResult<Option<serde_json::Value>> {
    let s: Option<String> = conn
        .query_row(
            "SELECT value_json FROM settings WHERE key = ?1",
            [key],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match s {
        Some(s) => Some(serde_json::from_str(&s)?),
        None => None,
    })
}

pub fn get<T: DeserializeOwned>(conn: &Connection, key: &str) -> AppResult<Option<T>> {
    Ok(match get_raw(conn, key)? {
        Some(v) => serde_json::from_value(v).ok(),
        None => None,
    })
}

pub fn set<T: Serialize + ?Sized>(conn: &Connection, key: &str, value: &T) -> AppResult<()> {
    let ts = now();
    conn.execute(
        "INSERT INTO settings (key, value_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at",
        params![key, serde_json::to_string(value)?, ts],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let conn = crate::db::open_in_memory().unwrap();
        assert_eq!(get::<String>(&conn, "theme").unwrap(), None);
        set(&conn, "theme", "dark").unwrap();
        set(&conn, "theme", "light").unwrap();
        assert_eq!(
            get::<String>(&conn, "theme").unwrap().as_deref(),
            Some("light")
        );
    }
}

const ONBOARDED: &str = "onboarded";

/// Whether first-run onboarding is done. Libraries that already have courses count as done,
/// so existing users never see it.
pub fn onboarded(conn: &Connection) -> AppResult<bool> {
    if get::<bool>(conn, ONBOARDED)?.unwrap_or(false) {
        return Ok(true);
    }
    let courses: i64 = conn.query_row("SELECT count(*) FROM courses", [], |r| r.get(0))?;
    if courses > 0 {
        set(conn, ONBOARDED, &true)?;
        return Ok(true);
    }
    Ok(false)
}

pub fn set_onboarded(conn: &Connection, done: bool) -> AppResult<()> {
    set(conn, ONBOARDED, &done)
}

#[cfg(test)]
mod onboarding_tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn fresh_library_needs_onboarding_existing_one_does_not() {
        let conn = open_in_memory().unwrap();
        assert!(!onboarded(&conn).unwrap());
        set_onboarded(&conn, true).unwrap();
        assert!(onboarded(&conn).unwrap());

        let conn = open_in_memory().unwrap();
        crate::db::repo::annotations::tests::lesson(&conn);
        assert!(
            onboarded(&conn).unwrap(),
            "a library with courses is auto-marked"
        );
        assert_eq!(get::<bool>(&conn, ONBOARDED).unwrap(), Some(true));
    }
}
