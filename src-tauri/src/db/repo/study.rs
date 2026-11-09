//! Phase 4 persistence: watched coverage, study sessions, plans and dashboard stats (SPEC §7.6).
//!
//! Timestamps are stored in UTC; "days" are local calendar days (`chrono::Local`). Functions take
//! `now`/`today` explicitly so tests are deterministic.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Duration, Local, NaiveDate, Utc, Weekday};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db::models::*;
use crate::db::repo::{lessons, settings};
use crate::db::{new_id, now};
use crate::error::{AppError, AppResult};
use crate::services::progress::schedule::{self, PlanLesson, PlanStatus, Target};
use crate::services::progress::{intervals, streak};

pub const SETTINGS_KEY: &str = "study";
/// A new session row starts after this much inactivity.
const SESSION_GAP_MIN: i64 = 10;

pub fn get_settings(conn: &Connection) -> AppResult<StudySettings> {
    Ok(settings::get::<StudySettings>(conn, SETTINGS_KEY)?.unwrap_or_default())
}

pub fn set_settings(conn: &Connection, s: &StudySettings) -> AppResult<StudySettings> {
    let mut s = s.clone();
    s.daily_goal_min = s.daily_goal_min.clamp(5, 600);
    s.completion_threshold = s.completion_threshold.clamp(0.5, 1.0);
    s.study_days.retain(|d| parse_weekday(d).is_some());
    s.reminders.retain(|r| parse_hm(&r.time).is_some());
    settings::set(conn, SETTINGS_KEY, &s)?;
    Ok(s)
}

pub fn parse_weekday(s: &str) -> Option<Weekday> {
    s.parse::<Weekday>().ok()
}

pub fn parse_hm(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.split_once(':')?;
    let (h, m) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

fn weekdays(s: &StudySettings) -> Vec<Weekday> {
    s.study_days
        .iter()
        .filter_map(|d| parse_weekday(d))
        .collect()
}

fn local_date(ts: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|t| t.with_timezone(&Local).date_naive())
}

fn ymd(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn parse_ymd(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

// ---- sessions ----

/// Adds active seconds to the current session for (lesson, source), or opens a new one after a
/// gap or when the local day changed.
pub fn add_session_seconds(
    conn: &Connection,
    lesson_id: Option<&str>,
    source: &str,
    secs: f64,
    at: DateTime<Utc>,
) -> AppResult<()> {
    if !(secs.is_finite() && secs > 0.0) {
        return Ok(());
    }
    let secs = secs.min(600.0).round() as i64;
    if secs == 0 {
        return Ok(());
    }
    let ts = at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let cutoff = (at - Duration::minutes(SESSION_GAP_MIN))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let open: Option<(String, String)> = conn
        .query_row(
            "SELECT id, started_at FROM study_sessions
              WHERE source = ?1 AND lesson_id IS ?2 AND ended_at >= ?3
              ORDER BY ended_at DESC LIMIT 1",
            params![source, lesson_id, cutoff],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let today = at.with_timezone(&Local).date_naive();
    match open.filter(|(_, started)| local_date(started) == Some(today)) {
        Some((id, _)) => {
            conn.execute(
                "UPDATE study_sessions SET ended_at = ?2, active_seconds = active_seconds + ?3, updated_at = ?2
                  WHERE id = ?1",
                params![id, ts, secs],
            )?;
        }
        None => {
            let started =
                (at - Duration::seconds(secs)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            conn.execute(
                "INSERT INTO study_sessions (id, lesson_id, started_at, ended_at, active_seconds, source, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?4, ?4)",
                params![new_id(), lesson_id, started, ts, secs, source],
            )?;
        }
    }
    Ok(())
}

/// Records watched segments: merges coverage, auto-completes at the threshold (never
/// un-completes), and counts `wall_secs` toward today's study minutes.
pub fn record_watch(
    conn: &Connection,
    lesson_id: &str,
    segments: &[WatchSegment],
    wall_secs: f64,
    at: DateTime<Utc>,
) -> AppResult<WatchResult> {
    let lesson = lessons::get_lesson(conn, lesson_id)?;
    let duration = lesson.duration_sec.filter(|d| *d > 0).map(f64::from);
    let existing: Vec<intervals::Interval> = conn
        .query_row(
            "SELECT watched_intervals_json FROM watch_progress WHERE lesson_id = ?1",
            [lesson_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    let segs: Vec<intervals::Interval> = segments.iter().map(|s| [s.from, s.to]).collect();
    let merged = intervals::add(existing, &segs, duration);
    let ratio = intervals::coverage(&merged, duration);
    let ts = now();
    conn.execute(
        "INSERT INTO watch_progress (lesson_id, watched_intervals_json, watched_ratio, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT(lesson_id) DO UPDATE SET watched_intervals_json = excluded.watched_intervals_json,
                watched_ratio = excluded.watched_ratio, updated_at = excluded.updated_at",
        params![lesson_id, serde_json::to_string(&merged)?, ratio, ts],
    )?;
    let threshold = get_settings(conn)?.completion_threshold;
    let completed_now = lesson.completed_at.is_none() && duration.is_some() && ratio >= threshold;
    if completed_now {
        lessons::set_completed(conn, lesson_id, true)?;
    }
    add_session_seconds(conn, Some(lesson_id), "playback", wall_secs, at)?;
    Ok(WatchResult {
        watched_ratio: ratio,
        completed_now,
    })
}

pub fn record_activity(
    conn: &Connection,
    lesson_id: Option<&str>,
    source: ActivitySource,
    secs: f64,
    at: DateTime<Utc>,
) -> AppResult<()> {
    let s = match source {
        ActivitySource::Notes => "notes",
        ActivitySource::Review => "review",
        ActivitySource::Timer => "timer",
    };
    add_session_seconds(conn, lesson_id, s, secs, at)
}

/// Active minutes per local day since `from` (inclusive).
pub fn day_minutes(conn: &Connection, from: NaiveDate) -> AppResult<BTreeMap<NaiveDate, u32>> {
    // Pad a day for time zones; bucketing below is exact.
    let since = (from - Duration::days(1))
        .format("%Y-%m-%dT00:00:00Z")
        .to_string();
    let mut st = conn
        .prepare("SELECT started_at, active_seconds FROM study_sessions WHERE started_at >= ?1")?;
    let rows = st.query_map([since], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    let mut secs: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    for row in rows {
        let (ts, s) = row?;
        if let Some(d) = local_date(&ts).filter(|d| *d >= from) {
            *secs.entry(d).or_default() += s;
        }
    }
    Ok(secs
        .into_iter()
        .map(|(d, s)| (d, (s / 60) as u32))
        .collect())
}

pub fn activity(conn: &Connection, days: u32, today: NaiveDate) -> AppResult<Vec<DayActivity>> {
    let from = today - Duration::days(days.max(1) as i64 - 1);
    let mins = day_minutes_exact(conn, from)?;
    Ok((0..days.max(1))
        .map(|i| {
            let d = from + Duration::days(i as i64);
            DayActivity {
                date: ymd(d),
                minutes: mins.get(&d).copied().unwrap_or(0.0),
            }
        })
        .collect())
}

fn day_minutes_exact(conn: &Connection, from: NaiveDate) -> AppResult<BTreeMap<NaiveDate, f64>> {
    let since = (from - Duration::days(1))
        .format("%Y-%m-%dT00:00:00Z")
        .to_string();
    let mut st = conn
        .prepare("SELECT started_at, active_seconds FROM study_sessions WHERE started_at >= ?1")?;
    let rows = st.query_map([since], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    let mut out: BTreeMap<NaiveDate, f64> = BTreeMap::new();
    for row in rows {
        let (ts, s) = row?;
        if let Some(d) = local_date(&ts).filter(|d| *d >= from) {
            *out.entry(d).or_default() += s as f64 / 60.0;
        }
    }
    Ok(out)
}

// ---- plans ----

fn target_of(conn: &Connection, course_id: &str) -> AppResult<Option<Target>> {
    let (pace_type, pace_value, date): (Option<String>, Option<f64>, Option<String>) = conn
        .query_row(
            "SELECT pace_type, pace_value, target_finish_date FROM courses WHERE id = ?1",
            [course_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("course"))?;
    let v = pace_value.unwrap_or(0.0).max(1.0) as u32;
    Ok(match pace_type.as_deref() {
        Some("date") => date.map(|date| Target::Date { date }),
        Some("lessons_per_week") => Some(Target::LessonsPerWeek { value: v }),
        Some("minutes_per_week") => Some(Target::MinutesPerWeek { value: v }),
        _ => None,
    })
}

/// Sets (or clears) a course's target and regenerates its plan from `today`.
pub fn set_course_plan(
    conn: &Connection,
    course_id: &str,
    target: Option<&Target>,
    today: NaiveDate,
) -> AppResult<CoursePlan> {
    let (t, v, date): (Option<&str>, Option<f64>, Option<String>) = match target {
        Some(Target::Date { date }) => {
            parse_ymd(date).ok_or_else(|| AppError::invalid("target date must be YYYY-MM-DD"))?;
            (Some("date"), None, Some(date.clone()))
        }
        Some(Target::LessonsPerWeek { value }) => {
            (Some("lessons_per_week"), Some(f64::from(*value)), None)
        }
        Some(Target::MinutesPerWeek { value }) => {
            (Some("minutes_per_week"), Some(f64::from(*value)), None)
        }
        None => (None, None, None),
    };
    let n = conn.execute(
        "UPDATE courses SET pace_type = ?2, pace_value = ?3, target_finish_date = ?4, updated_at = ?5 WHERE id = ?1",
        params![course_id, t, v, date, now()],
    )?;
    if n == 0 {
        return Err(AppError::not_found("course"));
    }
    rebalance(conn, course_id, today)?;
    get_course_plan(conn, course_id, today)
}

/// `ts` plus `i` milliseconds, so rows written together keep their order in `created_at`.
fn seq_ts(base: &chrono::DateTime<chrono::Utc>, i: usize) -> String {
    (*base + Duration::milliseconds(i as i64)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// The course's plan order: "outline" (default) or "custom" (an applied AI plan).
pub fn plan_order(conn: &Connection, course_id: &str) -> AppResult<String> {
    conn.query_row(
        "SELECT plan_order FROM courses WHERE id = ?1",
        [course_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("course"))
}

pub fn set_plan_order(conn: &Connection, course_id: &str, order: &str) -> AppResult<()> {
    if !matches!(order, "outline" | "custom") {
        return Err(AppError::invalid("plan order must be outline or custom"));
    }
    if conn.execute(
        "UPDATE courses SET plan_order = ?2 WHERE id = ?1",
        params![course_id, order],
    )? == 0
    {
        return Err(AppError::not_found("course"));
    }
    Ok(())
}

/// Open lessons in the order a rebalance should schedule them. With a custom (AI) order, lessons
/// already planned keep their planned sequence; lessons added since follow in outline order.
fn rebalance_order(
    conn: &Connection,
    course_id: &str,
    open: Vec<Lesson>,
) -> AppResult<Vec<Lesson>> {
    if plan_order(conn, course_id)? != "custom" {
        return Ok(open);
    }
    let mut st = conn.prepare(
        "SELECT lesson_id FROM plan_items WHERE course_id = ?1 ORDER BY planned_date, created_at",
    )?;
    let rank: HashMap<String, usize> = st
        .query_map([course_id], |r| r.get::<_, String>(0))?
        .filter_map(Result::ok)
        .enumerate()
        .map(|(i, id)| (id, i))
        .collect();
    let mut v: Vec<(usize, usize, Lesson)> = open
        .into_iter()
        .enumerate()
        .map(|(outline, l)| (rank.get(&l.id).copied().unwrap_or(usize::MAX), outline, l))
        .collect();
    v.sort_by_key(|(r, o, _)| (*r, *o));
    Ok(v.into_iter().map(|x| x.2).collect())
}

/// Re-spreads the course's incomplete lessons from `today` using its stored target, in the
/// course's plan order.
pub fn rebalance(conn: &Connection, course_id: &str, today: NaiveDate) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    let target = target_of(&tx, course_id)?;
    let open: Vec<Lesson> = lessons::list_course_lessons(&tx, course_id)?
        .into_iter()
        .filter(|l| l.completed_at.is_none())
        .collect();
    let open = rebalance_order(&tx, course_id, open)?;
    let base = chrono::Utc::now();
    // Keep history for finished lessons; replace everything still open.
    tx.execute(
        "DELETE FROM plan_items WHERE course_id = ?1 AND lesson_id IN
            (SELECT l.id FROM lessons l JOIN modules m ON m.id = l.module_id
              WHERE m.course_id = ?1 AND l.completed_at IS NULL)",
        [course_id],
    )?;
    if let Some(target) = target {
        let plan: Vec<PlanLesson> = open
            .iter()
            .map(|l| PlanLesson {
                id: l.id.clone(),
                minutes: l.duration_sec.map(|s| s as f64 / 60.0).unwrap_or(0.0),
            })
            .collect();
        let days = weekdays(&get_settings(&tx)?);
        for (i, (lesson_id, date)) in schedule::generate(&plan, &days, today, &target)
            .into_iter()
            .enumerate()
        {
            let ts = seq_ts(&base, i);
            tx.execute(
                "INSERT INTO plan_items (id, course_id, lesson_id, planned_date, done, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)
                 ON CONFLICT(lesson_id) DO UPDATE SET planned_date = excluded.planned_date, done = 0,
                        created_at = excluded.created_at, updated_at = excluded.updated_at",
                params![new_id(), course_id, lesson_id, ymd(date), ts],
            )?;
        }
    } else {
        tx.execute("DELETE FROM plan_items WHERE course_id = ?1", [course_id])?;
    }
    tx.commit()?;
    Ok(())
}

/// Replaces a course's open plan items with explicit dates (AI plan), keeping finished ones,
/// records the last date as the target, and switches the course to the plan's (custom) order.
pub fn apply_plan(
    conn: &Connection,
    course_id: &str,
    items: &[(String, NaiveDate)],
) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM plan_items WHERE course_id = ?1 AND lesson_id IN
            (SELECT l.id FROM lessons l JOIN modules m ON m.id = l.module_id
              WHERE m.course_id = ?1 AND l.completed_at IS NULL)",
        [course_id],
    )?;
    let base = chrono::Utc::now();
    for (i, (lesson_id, date)) in items.iter().enumerate() {
        let ts = seq_ts(&base, i);
        tx.execute(
            "INSERT INTO plan_items (id, course_id, lesson_id, planned_date, done, created_at, updated_at)
             SELECT ?1, ?2, l.id, ?4, 0, ?5, ?5 FROM lessons l JOIN modules m ON m.id = l.module_id
              WHERE l.id = ?3 AND m.course_id = ?2 AND l.completed_at IS NULL
             ON CONFLICT(lesson_id) DO UPDATE SET planned_date = excluded.planned_date, done = 0,
                    created_at = excluded.created_at, updated_at = excluded.updated_at",
            params![new_id(), course_id, lesson_id, ymd(*date), ts],
        )?;
    }
    let ts = now();
    if let Some(last) = items.iter().map(|i| i.1).max() {
        tx.execute(
            "UPDATE courses SET pace_type = 'date', pace_value = NULL, target_finish_date = ?2, updated_at = ?3 WHERE id = ?1",
            params![course_id, ymd(last), ts],
        )?;
    }
    tx.execute(
        "UPDATE courses SET plan_order = 'custom' WHERE id = ?1",
        [course_id],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn study_weekdays(conn: &Connection) -> AppResult<Vec<Weekday>> {
    Ok(weekdays(&get_settings(conn)?))
}

const PLANNED_SELECT: &str =
    "SELECT p.lesson_id, l.title, c.id, c.title, l.duration_sec, p.planned_date,
        (l.completed_at IS NOT NULL), (SELECT x.kind FROM materials x WHERE l.type = 'reading' AND x.lesson_id = l.id AND x.solution_of IS NULL AND x.kind != 'video' ORDER BY x.position LIMIT 1)
   FROM plan_items p JOIN lessons l ON l.id = p.lesson_id JOIN courses c ON c.id = p.course_id";

fn map_planned(r: &rusqlite::Row<'_>) -> rusqlite::Result<PlannedLesson> {
    Ok(PlannedLesson {
        lesson_id: r.get(0)?,
        lesson_title: r.get(1)?,
        course_id: r.get(2)?,
        course_title: r.get(3)?,
        duration_sec: r.get(4)?,
        planned_date: r.get(5)?,
        done: r.get(6)?,
        doc_kind: r.get(7)?,
    })
}

fn planned(
    conn: &Connection,
    filter: &str,
    p: impl rusqlite::Params,
) -> AppResult<Vec<PlannedLesson>> {
    let mut st = conn.prepare(&format!(
        "{PLANNED_SELECT} WHERE {filter} ORDER BY p.planned_date, c.title, p.created_at"
    ))?;
    Ok(st.query_map(p, map_planned)?.collect::<Result<_, _>>()?)
}

fn status_of(items: &[PlannedLesson], today: NaiveDate) -> PlanStatus {
    let v: Vec<(NaiveDate, bool)> = items
        .iter()
        .filter_map(|i| parse_ymd(&i.planned_date).map(|d| (d, i.done)))
        .collect();
    schedule::status(&v, today)
}

pub fn get_course_plan(
    conn: &Connection,
    course_id: &str,
    today: NaiveDate,
) -> AppResult<CoursePlan> {
    let target = target_of(conn, course_id)?;
    let items = planned(conn, "p.course_id = ?1", [course_id])?;
    let status = (!items.is_empty()).then(|| status_of(&items, today));
    let finish_date = items.iter().map(|i| i.planned_date.clone()).max();
    Ok(CoursePlan {
        target,
        items,
        status,
        finish_date,
        order: plan_order(conn, course_id)?,
    })
}

pub fn planned_between(
    conn: &Connection,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<Vec<PlannedLesson>> {
    planned(
        conn,
        "p.planned_date >= ?1 AND p.planned_date <= ?2",
        [ymd(from), ymd(to)],
    )
}

/// Courses with a plan whose status is Behind.
pub fn behind_courses(conn: &Connection, today: NaiveDate) -> AppResult<Vec<BehindCourse>> {
    let mut st = conn.prepare(
        "SELECT DISTINCT p.course_id FROM plan_items p JOIN courses c ON c.id = p.course_id WHERE c.status = 'active'",
    )?;
    let ids: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let mut out = vec![];
    for id in ids {
        let items = planned(conn, "p.course_id = ?1", [&id])?;
        if let PlanStatus::Behind { lessons } = status_of(&items, today) {
            out.push(BehindCourse {
                course_title: items
                    .first()
                    .map(|i| i.course_title.clone())
                    .unwrap_or_default(),
                course_id: id,
                lessons: lessons as i32,
            });
        }
    }
    Ok(out)
}

// ---- dashboards ----

fn count(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> AppResult<i32> {
    Ok(conn.query_row(sql, p, |r| r.get(0))?)
}

pub fn today_view(
    conn: &Connection,
    today: NaiveDate,
    now_utc: DateTime<Utc>,
) -> AppResult<TodayView> {
    let s = get_settings(conn)?;
    let history = day_minutes(conn, today - Duration::days(400))?;
    let streak = streak::compute(&history, s.daily_goal_min as u32, today);
    let minutes_today = day_minutes_exact(conn, today)?
        .get(&today)
        .copied()
        .unwrap_or(0.0);
    let planned = planned(
        conn,
        "p.planned_date <= ?1 AND (l.completed_at IS NULL OR p.planned_date = ?1) AND c.status = 'active'",
        [ymd(today)],
    )?;
    let iso = now_utc.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    Ok(TodayView {
        goal_min: s.daily_goal_min,
        minutes_today,
        streak,
        planned,
        due_cards: count(conn, "SELECT count(*) FROM flashcards WHERE due_at <= ?1", [iso])?,
        overdue_todos: count(
            conn,
            "SELECT count(*) FROM annotations WHERE type = 'todo' AND coalesce(status,'open') != 'done'
               AND due_date IS NOT NULL AND due_date < ?1",
            [ymd(today)],
        )?,
        open_questions: count(
            conn,
            "SELECT count(*) FROM annotations WHERE type = 'question' AND coalesce(status,'open') != 'answered'",
            [],
        )?,
        behind: behind_courses(conn, today)?,
    })
}

fn open_items(
    conn: &Connection,
    filter: &str,
    p: impl rusqlite::Params,
) -> AppResult<Vec<OpenItem>> {
    let mut st = conn.prepare(&format!(
        "SELECT a.id, a.lesson_id, l.title, coalesce(nullif(a.title,''), substr(coalesce(a.body_md,''),1,120)),
                a.start_sec, a.due_date
           FROM annotations a JOIN lessons l ON l.id = a.lesson_id WHERE {filter}
          ORDER BY coalesce(a.due_date, a.created_at) LIMIT 50"
    ))?;
    Ok(st
        .query_map(p, |r| {
            Ok(OpenItem {
                id: r.get(0)?,
                lesson_id: r.get(1)?,
                lesson_title: r.get(2)?,
                title: r.get(3)?,
                start_sec: r.get(4)?,
                due_date: r.get(5)?,
            })
        })?
        .collect::<Result<_, _>>()?)
}

/// Open and done to-dos with a due date in [from, to] (planner calendar).
pub fn todos_between(
    conn: &Connection,
    from: NaiveDate,
    to: NaiveDate,
) -> AppResult<Vec<OpenItem>> {
    open_items(
        conn,
        "a.type = 'todo' AND a.due_date >= ?1 AND a.due_date <= ?2",
        [ymd(from), ymd(to)],
    )
}

pub fn weekly_review(conn: &Connection, week_start: NaiveDate) -> AppResult<WeeklyReview> {
    let end = week_start + Duration::days(6);
    let mins = day_minutes_exact(conn, week_start)?;
    let days: Vec<DayActivity> = (0..7)
        .map(|i| {
            let d = week_start + Duration::days(i);
            DayActivity {
                date: ymd(d),
                minutes: mins.get(&d).copied().unwrap_or(0.0),
            }
        })
        .collect();
    let minutes = days.iter().map(|d| d.minutes).sum();
    // Completed this week (by local day of completed_at).
    let mut st = conn.prepare(
        "SELECT l.id, l.title, c.id, c.title, l.duration_sec, l.completed_at, (SELECT x.kind FROM materials x WHERE l.type = 'reading' AND x.lesson_id = l.id AND x.solution_of IS NULL AND x.kind != 'video' ORDER BY x.position LIMIT 1)
           FROM lessons l JOIN modules m ON m.id = l.module_id JOIN courses c ON c.id = m.course_id
          WHERE l.completed_at >= ?1 ORDER BY l.completed_at",
    )?;
    let since = (week_start - Duration::days(1))
        .format("%Y-%m-%dT00:00:00Z")
        .to_string();
    let lessons_completed = st
        .query_map([since], |r| {
            Ok(PlannedLesson {
                lesson_id: r.get(0)?,
                lesson_title: r.get(1)?,
                course_id: r.get(2)?,
                course_title: r.get(3)?,
                duration_sec: r.get(4)?,
                planned_date: r.get::<_, String>(5)?,
                done: true,
                doc_kind: r.get(6)?,
            })
        })?
        .filter_map(Result::ok)
        .filter_map(|mut p| {
            let d = local_date(&p.planned_date)?;
            (d >= week_start && d <= end).then(|| {
                p.planned_date = ymd(d);
                p
            })
        })
        .collect();
    Ok(WeeklyReview {
        week_start: ymd(week_start),
        days,
        minutes,
        lessons_completed,
        open_questions: open_items(
            conn,
            "a.type = 'question' AND coalesce(a.status,'open') != 'answered'",
            [],
        )?,
        overdue_todos: open_items(
            conn,
            "a.type = 'todo' AND coalesce(a.status,'open') != 'done' AND a.due_date IS NOT NULL AND a.due_date <= ?1",
            [ymd(end)],
        )?,
        next_week: planned_between(conn, end + Duration::days(1), end + Duration::days(7))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::db::repo::import;
    use crate::services::import::{ImportedPlaylist, ImportedVideo};

    fn setup(durations: &[i32]) -> (Connection, String, Vec<String>) {
        let conn = open_in_memory().unwrap();
        let p = ImportedPlaylist {
            playlist_id: "PLstudy12345".into(),
            title: "C".into(),
            description: None,
            channel_title: None,
            thumbnail_url: None,
            videos: durations
                .iter()
                .enumerate()
                .map(|(i, d)| ImportedVideo {
                    video_id: format!("vid{i:08}"),
                    title: format!("L{i}"),
                    description: None,
                    channel_title: None,
                    duration_sec: Some(*d),
                    license: None,
                })
                .collect(),
            skipped: 0,
        };
        let (cid, _) = import::import_playlist(&conn, &p).unwrap();
        let ids = lessons::list_course_lessons(&conn, &cid)
            .unwrap()
            .into_iter()
            .map(|l| l.id)
            .collect();
        (conn, cid, ids)
    }

    fn seg(from: f64, to: f64) -> WatchSegment {
        WatchSegment { from, to }
    }

    #[test]
    fn skipping_to_end_does_not_complete_but_watching_90_percent_does() {
        let (conn, _, ids) = setup(&[600]);
        let t = Utc::now();
        let r = record_watch(
            &conn,
            &ids[0],
            &[seg(0.0, 10.0), seg(590.0, 600.0)],
            20.0,
            t,
        )
        .unwrap();
        assert!(!r.completed_now);
        assert!(
            lessons::get_lesson(&conn, &ids[0])
                .unwrap()
                .completed_at
                .is_none()
        );
        let r = record_watch(&conn, &ids[0], &[seg(10.0, 300.0)], 290.0, t).unwrap();
        assert!(!r.completed_now && r.watched_ratio < 0.9);
        let r = record_watch(&conn, &ids[0], &[seg(300.0, 545.0)], 245.0, t).unwrap();
        assert!(r.completed_now, "ratio {}", r.watched_ratio);
        assert!(
            lessons::get_lesson(&conn, &ids[0])
                .unwrap()
                .completed_at
                .is_some()
        );
        // Never re-completes or un-completes.
        assert!(
            !record_watch(&conn, &ids[0], &[seg(0.0, 600.0)], 1.0, t)
                .unwrap()
                .completed_now
        );
        // Session minutes accumulated in one session row.
        let n: i64 = conn
            .query_row("SELECT count(*) FROM study_sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        let mins = activity(&conn, 1, Local::now().date_naive()).unwrap();
        assert!((mins[0].minutes - 556.0 / 60.0).abs() < 0.05, "{mins:?}");
    }

    #[test]
    fn sessions_split_after_gap() {
        let (conn, _, ids) = setup(&[600]);
        let t = Utc::now();
        add_session_seconds(
            &conn,
            Some(&ids[0]),
            "playback",
            30.0,
            t - Duration::minutes(30),
        )
        .unwrap();
        add_session_seconds(&conn, Some(&ids[0]), "playback", 30.0, t).unwrap();
        record_activity(&conn, None, ActivitySource::Notes, 15.0, t).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM study_sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3);
    }

    #[test]
    fn plan_generation_status_and_rebalance() {
        let (conn, cid, ids) = setup(&[3600, 3600, 3600, 3600]);
        let mut s = get_settings(&conn).unwrap();
        s.study_days = vec![]; // every day
        set_settings(&conn, &s).unwrap();
        let mon = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let plan =
            set_course_plan(&conn, &cid, Some(&Target::LessonsPerWeek { value: 7 }), mon).unwrap();
        assert_eq!(plan.items.len(), 4);
        assert_eq!(plan.items[3].planned_date, "2026-11-12");
        assert_eq!(plan.status, Some(PlanStatus::OnTrack));
        // Two days later, nothing done: behind by 2.
        let wed = mon + Duration::days(2);
        assert_eq!(
            get_course_plan(&conn, &cid, wed).unwrap().status,
            Some(PlanStatus::Behind { lessons: 2 })
        );
        assert_eq!(behind_courses(&conn, wed).unwrap().len(), 1);
        // Completing one, then rebalancing, puts the rest from today.
        lessons::set_completed(&conn, &ids[0], true).unwrap();
        rebalance(&conn, &cid, wed).unwrap();
        let p = get_course_plan(&conn, &cid, wed).unwrap();
        assert_eq!(p.status, Some(PlanStatus::OnTrack));
        assert_eq!(
            p.items
                .iter()
                .filter(|i| !i.done)
                .map(|i| i.planned_date.as_str())
                .min(),
            Some("2026-11-11")
        );
        assert_eq!(p.items.len(), 4, "finished lesson keeps its plan row");
        // Clearing the target removes the plan.
        let cleared = set_course_plan(&conn, &cid, None, wed).unwrap();
        assert!(cleared.items.is_empty() && cleared.target.is_none());
    }

    #[test]
    fn rebalance_keeps_an_ai_plans_order() {
        let (conn, cid, ids) = setup(&[600, 600, 600, 600]);
        let mut s = get_settings(&conn).unwrap();
        s.study_days = vec![];
        set_settings(&conn, &s).unwrap();
        let mon = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        // AI plan: reverse order, the last two on the same day (order within the day matters).
        let items: Vec<(String, NaiveDate)> = vec![
            (ids[3].clone(), mon),
            (ids[2].clone(), mon + Duration::days(1)),
            (ids[1].clone(), mon + Duration::days(2)),
            (ids[0].clone(), mon + Duration::days(2)),
        ];
        apply_plan(&conn, &cid, &items).unwrap();
        let p = get_course_plan(&conn, &cid, mon).unwrap();
        assert_eq!(p.order, "custom");
        let order = |p: &CoursePlan| {
            p.items
                .iter()
                .map(|i| i.lesson_id.clone())
                .collect::<Vec<_>>()
        };
        let reversed: Vec<String> = ids.iter().rev().cloned().collect();
        assert_eq!(order(&p), reversed);

        // A lesson added after the plan goes last.
        let module = lessons::get_lesson(&conn, &ids[0]).unwrap().module_id;
        let extra = lessons::create_lesson(
            &conn,
            &NewLesson {
                module_id: module,
                lesson_type: LessonType::Reading,
                title: "Added later".into(),
                description: None,
                url: None,
            },
        )
        .unwrap()
        .id;
        rebalance(&conn, &cid, mon + Duration::days(1)).unwrap();
        let mut want = reversed.clone();
        want.push(extra.clone());
        assert_eq!(order(&get_course_plan(&conn, &cid, mon).unwrap()), want);

        // Back to outline order.
        set_plan_order(&conn, &cid, "outline").unwrap();
        rebalance(&conn, &cid, mon).unwrap();
        let p = get_course_plan(&conn, &cid, mon).unwrap();
        assert_eq!(p.order, "outline");
        let mut outline = ids.clone();
        outline.push(extra);
        assert_eq!(order(&p), outline);
        assert!(set_plan_order(&conn, &cid, "random").is_err());
    }

    #[test]
    fn today_view_counts() {
        let (conn, cid, _) = setup(&[600, 600]);
        let today = Local::now().date_naive();
        set_course_plan(&conn, &cid, Some(&Target::Date { date: ymd(today) }), today).unwrap();
        let v = today_view(&conn, today, Utc::now()).unwrap();
        assert_eq!(v.planned.len(), 2);
        assert_eq!(v.goal_min, 45);
        assert_eq!(v.streak.current, 0);
        assert_eq!(v.open_questions, 0);
    }

    #[test]
    fn settings_are_clamped() {
        let conn = open_in_memory().unwrap();
        let s = set_settings(
            &conn,
            &StudySettings {
                daily_goal_min: 1,
                completion_threshold: 2.0,
                study_days: vec!["mon".into(), "funday".into()],
                reminders: vec![
                    Reminder {
                        time: "25:00".into(),
                        enabled: true,
                    },
                    Reminder {
                        time: "08:30".into(),
                        enabled: true,
                    },
                ],
                behind_nudge: false,
            },
        )
        .unwrap();
        assert_eq!(s.daily_goal_min, 5);
        assert_eq!(s.completion_threshold, 1.0);
        assert_eq!(s.study_days, vec!["mon"]);
        assert_eq!(s.reminders.len(), 1);
    }
}
