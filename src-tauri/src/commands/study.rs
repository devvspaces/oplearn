//! Phase 4 commands: watch tracking, study minutes, plans, dashboards, reminders.

use chrono::{Datelike, Duration, Local, NaiveDate, Utc};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::db::models::*;
use crate::db::repo::study;
use crate::error::{AppError, AppResult};
use crate::services::progress::schedule::Target;
use crate::state::AppState;

/// Emitted when watching auto-completes a lesson, so outlines and Today refresh.
#[derive(Debug, Clone, Serialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct LessonCompleted {
    pub lesson_id: String,
}

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn parse_date(s: &str) -> AppResult<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| AppError::invalid("dates must be YYYY-MM-DD"))
}

#[tauri::command]
#[specta::specta]
pub async fn get_study_settings(state: State<'_, AppState>) -> AppResult<StudySettings> {
    study::get_settings(&*state.db()?)
}

#[tauri::command]
#[specta::specta]
pub async fn set_study_settings(
    state: State<'_, AppState>,
    settings: StudySettings,
) -> AppResult<StudySettings> {
    study::set_settings(&*state.db()?, &settings)
}

/// `wall_secs` is real time spent playing (segments are in video time, which runs faster at 2×).
#[tauri::command]
#[specta::specta]
pub async fn record_watch(
    app: AppHandle,
    state: State<'_, AppState>,
    lesson_id: String,
    segments: Vec<WatchSegment>,
    wall_secs: f64,
) -> AppResult<WatchResult> {
    let r = study::record_watch(&*state.db()?, &lesson_id, &segments, wall_secs, Utc::now())?;
    if r.completed_now {
        let _ = LessonCompleted { lesson_id }.emit(&app);
    }
    Ok(r)
}

#[tauri::command]
#[specta::specta]
pub async fn record_activity(
    state: State<'_, AppState>,
    lesson_id: Option<String>,
    source: ActivitySource,
    seconds: f64,
) -> AppResult<()> {
    study::record_activity(
        &*state.db()?,
        lesson_id.as_deref(),
        source,
        seconds,
        Utc::now(),
    )
}

#[tauri::command]
#[specta::specta]
pub async fn get_today(state: State<'_, AppState>) -> AppResult<TodayView> {
    study::today_view(&*state.db()?, today(), Utc::now())
}

/// Daily minutes for the last `days` days (heatmap), oldest first.
#[tauri::command]
#[specta::specta]
pub async fn get_activity(state: State<'_, AppState>, days: i32) -> AppResult<Vec<DayActivity>> {
    study::activity(&*state.db()?, days.clamp(1, 800) as u32, today())
}

/// Weekly review for the ISO week containing `date` (default: this week).
#[tauri::command]
#[specta::specta]
pub async fn get_weekly_review(
    state: State<'_, AppState>,
    date: Option<String>,
) -> AppResult<WeeklyReview> {
    let d = match date {
        Some(s) => parse_date(&s)?,
        None => today(),
    };
    let monday = d - Duration::days(d.weekday().num_days_from_monday() as i64);
    study::weekly_review(&*state.db()?, monday)
}

#[tauri::command]
#[specta::specta]
pub async fn get_course_plan(
    state: State<'_, AppState>,
    course_id: String,
) -> AppResult<CoursePlan> {
    study::get_course_plan(&*state.db()?, &course_id, today())
}

/// Sets or clears (`null`) a course's target and regenerates its schedule from today.
#[tauri::command]
#[specta::specta]
pub async fn set_course_plan(
    state: State<'_, AppState>,
    course_id: String,
    target: Option<Target>,
) -> AppResult<CoursePlan> {
    study::set_course_plan(&*state.db()?, &course_id, target.as_ref(), today())
}

#[tauri::command]
#[specta::specta]
pub async fn rebalance_plan(
    state: State<'_, AppState>,
    course_id: String,
) -> AppResult<CoursePlan> {
    let conn = state.db()?;
    study::rebalance(&conn, &course_id, today())?;
    study::get_course_plan(&conn, &course_id, today())
}

/// "outline" re-spreads by the course outline; "custom" keeps an applied AI plan's order.
/// Rebalances from today so the change shows at once.
#[tauri::command]
#[specta::specta]
pub async fn set_plan_order(
    state: State<'_, AppState>,
    course_id: String,
    order: String,
) -> AppResult<CoursePlan> {
    let conn = state.db()?;
    study::set_plan_order(&conn, &course_id, &order)?;
    study::rebalance(&conn, &course_id, today())?;
    study::get_course_plan(&conn, &course_id, today())
}

/// Planned lessons across courses between two dates (inclusive), for the planner calendar.
#[tauri::command]
#[specta::specta]
pub async fn get_planned(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> AppResult<Vec<PlannedLesson>> {
    study::planned_between(&*state.db()?, parse_date(&from)?, parse_date(&to)?)
}

/// Shows a sample reminder now (Settings → "Test").
#[tauri::command]
#[specta::specta]
pub async fn test_reminder(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let v = study::today_view(&*state.db()?, today(), Utc::now())?;
    let (title, body) = crate::services::reminders::reminder_text(&v).unwrap_or_else(|| {
        (
            "Daily goal met 🎉".into(),
            format!("{} min studied today", v.minutes_today.floor()),
        )
    });
    crate::services::reminders::notify(&app, &title, &body);
    Ok(())
}

/// To-dos due between two dates (inclusive), for the planner calendar.
#[tauri::command]
#[specta::specta]
pub async fn get_todos_between(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> AppResult<Vec<OpenItem>> {
    study::todos_between(&*state.db()?, parse_date(&from)?, parse_date(&to)?)
}
