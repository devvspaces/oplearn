//! YouTube search result cache (24 h) and Data API quota accounting (SPEC §7.2).

use chrono::{DateTime, Duration, FixedOffset, Utc};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db::models::{QuotaInfo, YtSearchQuery, YtSearchResult};
use crate::db::repo::settings;
use crate::error::AppResult;

pub const CACHE_TTL_HOURS: i64 = 24;
pub const DEFAULT_DAILY_QUOTA: i32 = 10_000;
pub const QUOTA_LIMIT_KEY: &str = "youtube_quota_limit";
/// `search.list` cost.
pub const SEARCH_COST: i32 = 100;

/// The quota day in US Pacific time. Approximated as UTC−8 (ignores daylight saving, so the
/// reset is off by an hour in summer; it's an estimate shown to the user, not enforcement).
pub fn quota_day(at: DateTime<Utc>) -> String {
    at.with_timezone(&FixedOffset::west_opt(8 * 3600).expect("valid offset"))
        .format("%Y-%m-%d")
        .to_string()
}

pub fn cache_key(q: &YtSearchQuery) -> String {
    let norm = YtSearchQuery {
        query: q.query.trim().to_lowercase(),
        ..q.clone()
    };
    serde_json::to_string(&norm).unwrap_or_default()
}

pub fn cached(
    conn: &Connection,
    key: &str,
    at: DateTime<Utc>,
) -> AppResult<Option<Vec<YtSearchResult>>> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT json, fetched_at FROM yt_search_cache WHERE key = ?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(json, fetched)| {
        let t = DateTime::parse_from_rfc3339(&fetched)
            .ok()?
            .with_timezone(&Utc);
        (at - t < Duration::hours(CACHE_TTL_HOURS))
            .then(|| serde_json::from_str(&json).ok())
            .flatten()
    }))
}

pub fn store(
    conn: &Connection,
    key: &str,
    results: &[YtSearchResult],
    at: DateTime<Utc>,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO yt_search_cache (key, json, fetched_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET json = excluded.json, fetched_at = excluded.fetched_at",
        params![key, serde_json::to_string(results)?, at.to_rfc3339()],
    )?;
    // Keep the cache small.
    let cutoff = (at - Duration::hours(CACHE_TTL_HOURS * 7)).to_rfc3339();
    conn.execute(
        "DELETE FROM yt_search_cache WHERE fetched_at < ?1",
        [cutoff],
    )?;
    Ok(())
}

/// Generic JSON cache entry (same 24 h TTL), e.g. playlist previews under `playlist:<id>`.
pub fn cached_json<T: serde::de::DeserializeOwned>(
    conn: &Connection,
    key: &str,
    at: DateTime<Utc>,
) -> AppResult<Option<T>> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT json, fetched_at FROM yt_search_cache WHERE key = ?1",
            [key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(json, fetched)| {
        let t = DateTime::parse_from_rfc3339(&fetched)
            .ok()?
            .with_timezone(&Utc);
        (at - t < Duration::hours(CACHE_TTL_HOURS))
            .then(|| serde_json::from_str(&json).ok())
            .flatten()
    }))
}

pub fn store_json<T: serde::Serialize>(
    conn: &Connection,
    key: &str,
    value: &T,
    at: DateTime<Utc>,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO yt_search_cache (key, json, fetched_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET json = excluded.json, fetched_at = excluded.fetched_at",
        params![key, serde_json::to_string(value)?, at.to_rfc3339()],
    )?;
    Ok(())
}

pub fn add_units(conn: &Connection, units: i32, at: DateTime<Utc>) -> AppResult<()> {
    if units <= 0 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO quota_usage (day, units) VALUES (?1, ?2)
         ON CONFLICT(day) DO UPDATE SET units = units + excluded.units",
        params![quota_day(at), units],
    )?;
    Ok(())
}

pub fn quota(conn: &Connection, at: DateTime<Utc>) -> AppResult<QuotaInfo> {
    let day = quota_day(at);
    let used: i32 = conn
        .query_row(
            "SELECT units FROM quota_usage WHERE day = ?1",
            [&day],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let limit = settings::get::<i32>(conn, QUOTA_LIMIT_KEY)?.unwrap_or(DEFAULT_DAILY_QUOTA);
    Ok(QuotaInfo { day, used, limit })
}

/// Marks today's quota as spent (after a quotaExceeded error) so searches fall back until reset.
pub fn exhaust(conn: &Connection, at: DateTime<Utc>) -> AppResult<()> {
    let q = quota(conn, at)?;
    add_units(conn, (q.limit - q.used).max(0), at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::SearchKind;
    use crate::db::open_in_memory;

    fn q(s: &str) -> YtSearchQuery {
        YtSearchQuery {
            query: s.into(),
            kind: SearchKind::Video,
            duration: None,
            upload: None,
            cc_only: true,
        }
    }

    #[test]
    fn cache_ttl_and_normalised_keys() {
        let conn = open_in_memory().unwrap();
        let t = Utc::now();
        assert_eq!(cache_key(&q(" Raft ")), cache_key(&q("raft")));
        assert_ne!(
            cache_key(&q("raft")),
            cache_key(&YtSearchQuery {
                cc_only: false,
                ..q("raft")
            })
        );
        let r = vec![YtSearchResult {
            kind: SearchKind::Video,
            id: "aaaaaaaaaaa".into(),
            title: "Raft".into(),
            channel_title: None,
            thumbnail_url: None,
            duration_sec: Some(60),
            view_count: Some(5.0),
            license: None,
            published_at: None,
            item_count: None,
        }];
        store(&conn, "k", &r, t).unwrap();
        assert_eq!(
            cached(&conn, "k", t + Duration::hours(23)).unwrap(),
            Some(r)
        );
        assert_eq!(cached(&conn, "k", t + Duration::hours(25)).unwrap(), None);
    }

    #[test]
    fn quota_accumulates_per_pacific_day() {
        let conn = open_in_memory().unwrap();
        // 07:59 UTC is still the previous day in Pacific (UTC−8).
        let a = DateTime::parse_from_rfc3339("2026-11-09T07:59:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let b = DateTime::parse_from_rfc3339("2026-11-09T08:01:00Z")
            .unwrap()
            .with_timezone(&Utc);
        add_units(&conn, 101, a).unwrap();
        add_units(&conn, 100, a).unwrap();
        assert_eq!(quota(&conn, a).unwrap().used, 201);
        assert_eq!(quota(&conn, b).unwrap().used, 0);
        exhaust(&conn, b).unwrap();
        let qb = quota(&conn, b).unwrap();
        assert_eq!(qb.used, qb.limit);
    }
}
