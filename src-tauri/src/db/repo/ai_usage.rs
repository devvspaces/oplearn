//! AI usage ledger: every call's tokens and cost, for the Settings summary and the monthly cap.

use chrono::{DateTime, Datelike, Local, TimeZone, Utc};
use rusqlite::{Connection, params};

use crate::db::models::{AiFeatureUsage, AiUsageRow, AiUsageSummary};
use crate::db::new_id;
use crate::error::AppResult;
use crate::services::ai::Used;

fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Start of the local calendar month / day containing `at`, in UTC.
fn month_start(at: DateTime<Utc>) -> DateTime<Utc> {
    let l = at.with_timezone(&Local);
    Local
        .with_ymd_and_hms(l.year(), l.month(), 1, 0, 0, 0)
        .earliest()
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or(at)
}

fn day_start(at: DateTime<Utc>) -> DateTime<Utc> {
    let l = at.with_timezone(&Local).date_naive();
    l.and_hms_opt(0, 0, 0)
        .and_then(|d| Local.from_local_datetime(&d).earliest())
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or(at)
}

pub fn record_at(
    conn: &Connection,
    feature: &str,
    used: &Used,
    course_id: Option<&str>,
    lesson_id: Option<&str>,
    ok: bool,
    at: DateTime<Utc>,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO ai_usage (id, at, feature, model, course_id, lesson_id, input_tokens, output_tokens, cost_usd, ok)
         VALUES (?1, ?2, ?3, ?4, (SELECT id FROM courses WHERE id = ?5), (SELECT id FROM lessons WHERE id = ?6), ?7, ?8, ?9, ?10)",
        params![new_id(), iso(at), feature, used.model, course_id, lesson_id, used.input_tokens, used.output_tokens, used.cost_usd, ok],
    )?;
    Ok(())
}

pub fn record(
    conn: &Connection,
    feature: &str,
    used: &Used,
    course_id: Option<&str>,
    lesson_id: Option<&str>,
    ok: bool,
) -> AppResult<()> {
    record_at(conn, feature, used, course_id, lesson_id, ok, Utc::now())
}

fn sum_since(conn: &Connection, since: Option<DateTime<Utc>>) -> AppResult<(f64, i32)> {
    let since = since.map(iso).unwrap_or_default();
    Ok(conn.query_row(
        "SELECT coalesce(sum(cost_usd), 0), count(*) FROM ai_usage WHERE at >= ?1",
        [since],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

pub fn month_to_date(conn: &Connection, at: DateTime<Utc>) -> AppResult<f64> {
    Ok(sum_since(conn, Some(month_start(at)))?.0)
}

pub fn summary(
    conn: &Connection,
    at: DateTime<Utc>,
    limit_usd: Option<f64>,
) -> AppResult<AiUsageSummary> {
    let (month_usd, month_calls) = sum_since(conn, Some(month_start(at)))?;
    let mut st = conn.prepare(
        "SELECT feature, count(*), sum(cost_usd) FROM ai_usage WHERE at >= ?1 GROUP BY feature ORDER BY sum(cost_usd) DESC",
    )?;
    let by_feature = st
        .query_map([iso(month_start(at))], |r| {
            Ok(AiFeatureUsage {
                feature: r.get(0)?,
                calls: r.get(1)?,
                cost_usd: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    let mut st = conn.prepare(
        "SELECT at, feature, model, input_tokens, output_tokens, cost_usd, ok FROM ai_usage ORDER BY at DESC LIMIT 50",
    )?;
    let recent = st
        .query_map([], |r| {
            Ok(AiUsageRow {
                at: r.get(0)?,
                feature: r.get(1)?,
                model: r.get(2)?,
                input_tokens: r.get(3)?,
                output_tokens: r.get(4)?,
                cost_usd: r.get(5)?,
                ok: r.get(6)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(AiUsageSummary {
        today_usd: sum_since(conn, Some(day_start(at)))?.0,
        month_usd,
        all_time_usd: sum_since(conn, None)?.0,
        month_calls,
        limit_usd,
        by_feature,
        recent,
    })
}

pub fn clear(conn: &Connection) -> AppResult<()> {
    conn.execute("DELETE FROM ai_usage", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn used(cost: f64) -> Used {
        Used {
            model: "claude-opus-5-5".into(),
            input_tokens: 100,
            output_tokens: 10,
            cost_usd: cost,
        }
    }

    #[test]
    fn aggregates_by_month_and_feature() {
        let conn = open_in_memory().unwrap();
        let now = Utc::now();
        let last_month = month_start(now) - chrono::Duration::days(3);
        record_at(
            &conn,
            "restructure",
            &used(0.5),
            Some("nope"),
            None,
            true,
            last_month,
        )
        .unwrap();
        record_at(&conn, "flashcards", &used(0.10), None, None, true, now).unwrap();
        record_at(&conn, "flashcards", &used(0.05), None, None, false, now).unwrap();
        record_at(&conn, "quiz", &used(0.20), None, None, true, now).unwrap();
        let s = summary(&conn, now, Some(1.0)).unwrap();
        assert!((s.month_usd - 0.35).abs() < 1e-9);
        assert!((s.all_time_usd - 0.85).abs() < 1e-9);
        assert_eq!(s.month_calls, 3);
        assert_eq!(s.by_feature[0].feature, "quiz");
        assert_eq!(s.by_feature[1].calls, 2);
        assert_eq!(s.recent.len(), 4);
        assert!((month_to_date(&conn, now).unwrap() - 0.35).abs() < 1e-9);
        clear(&conn).unwrap();
        assert_eq!(summary(&conn, now, None).unwrap().all_time_usd, 0.0);
    }
}
