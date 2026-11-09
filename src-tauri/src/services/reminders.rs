//! Desktop reminders (SPEC §7.6): at the user's chosen times, plus a daily "behind schedule"
//! nudge in the evening. Skipped once the day's goal is met.

use std::collections::HashSet;

use chrono::{Local, NaiveDate, NaiveTime, Timelike};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::db::models::{StudySettings, TodayView};
use crate::db::repo::study;
use crate::state::AppState;

/// Hour (local) after which the behind-schedule nudge may fire.
const NUDGE_HOUR: u32 = 18;

/// Reminder times due at `now` that haven't fired today.
pub fn due_reminders(
    s: &StudySettings,
    now: NaiveTime,
    today: NaiveDate,
    fired: &HashSet<(NaiveDate, String)>,
) -> Vec<String> {
    s.reminders
        .iter()
        .filter(|r| r.enabled)
        .filter_map(|r| {
            let (h, m) = study::parse_hm(&r.time)?;
            // Fire within 5 minutes after the set time (the loop ticks every 30 s; sleep may skip).
            let mins_now = now.hour() * 60 + now.minute();
            let at = h * 60 + m;
            (mins_now >= at && mins_now < at + 5 && !fired.contains(&(today, r.time.clone())))
                .then(|| r.time.clone())
        })
        .collect()
}

/// Notification text for a reminder, or None if there's nothing to nudge about.
pub fn reminder_text(v: &TodayView) -> Option<(String, String)> {
    let done = v.minutes_today.floor() as i32;
    if done >= v.goal_min {
        return None;
    }
    let next = v.planned.iter().find(|p| !p.done);
    let title = match next {
        Some(p) => format!("You planned {} today", p.lesson_title),
        None => "Time to study".to_string(),
    };
    let mut body = format!("{done}/{} min done", v.goal_min);
    if v.streak.current > 0 {
        body.push_str(&format!(" · 🔥 {}-day streak", v.streak.current));
    }
    Some((title, body))
}

pub fn behind_text(v: &TodayView) -> Option<(String, String)> {
    let c = v.behind.first()?;
    let more = v.behind.len() - 1;
    Some((
        format!("{} is behind schedule", c.course_title),
        format!(
            "Behind by {} lesson{}{}. Open Lectern to catch up or rebalance.",
            c.lessons,
            if c.lessons == 1 { "" } else { "s" },
            if more > 0 {
                format!(" (+{more} more course{})", if more == 1 { "" } else { "s" })
            } else {
                String::new()
            }
        ),
    ))
}

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("notification failed: {e}");
    }
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut fired: HashSet<(NaiveDate, String)> = HashSet::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let st = app.state::<AppState>();
            let now = Local::now();
            let today = now.date_naive();
            fired.retain(|(d, _)| *d == today);
            let Ok(conn) = st.db() else { continue };
            let Ok(s) = study::get_settings(&conn) else {
                continue;
            };
            let due = due_reminders(&s, now.time(), today, &fired);
            let nudge = s.behind_nudge
                && now.hour() >= NUDGE_HOUR
                && !fired.contains(&(today, "behind".into()));
            if due.is_empty() && !nudge {
                continue;
            }
            let Ok(view) = study::today_view(&conn, today, chrono::Utc::now()) else {
                continue;
            };
            drop(conn);
            for t in due {
                fired.insert((today, t));
                if let Some((title, body)) = reminder_text(&view) {
                    notify(&app, &title, &body);
                }
            }
            if nudge {
                fired.insert((today, "behind".into()));
                if view.minutes_today < f64::from(view.goal_min)
                    && let Some((title, body)) = behind_text(&view)
                {
                    notify(&app, &title, &body);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::{BehindCourse, Reminder};
    use crate::services::progress::streak::Streak;

    fn view(minutes: f64) -> TodayView {
        TodayView {
            goal_min: 45,
            minutes_today: minutes,
            streak: Streak {
                current: 3,
                ..Default::default()
            },
            planned: vec![],
            due_cards: 0,
            overdue_todos: 0,
            open_questions: 0,
            behind: vec![BehindCourse {
                course_id: "c".into(),
                course_title: "6.824".into(),
                lessons: 2,
            }],
        }
    }

    #[test]
    fn fires_once_in_window() {
        let s = StudySettings {
            reminders: vec![
                Reminder {
                    time: "19:00".into(),
                    enabled: true,
                },
                Reminder {
                    time: "08:00".into(),
                    enabled: false,
                },
            ],
            ..Default::default()
        };
        let today = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let t = |h, m| NaiveTime::from_hms_opt(h, m, 0).unwrap();
        let mut fired = HashSet::new();
        assert!(due_reminders(&s, t(18, 59), today, &fired).is_empty());
        assert_eq!(due_reminders(&s, t(19, 2), today, &fired), vec!["19:00"]);
        fired.insert((today, "19:00".to_string()));
        assert!(due_reminders(&s, t(19, 3), today, &fired).is_empty());
        assert!(
            due_reminders(&s, t(8, 0), today, &fired).is_empty(),
            "disabled"
        );
    }

    #[test]
    fn texts() {
        let (title, body) = reminder_text(&view(12.4)).unwrap();
        assert_eq!(title, "Time to study");
        assert_eq!(body, "12/45 min done · 🔥 3-day streak");
        assert!(reminder_text(&view(45.0)).is_none());
        assert_eq!(
            behind_text(&view(0.0)).unwrap().0,
            "6.824 is behind schedule"
        );
    }
}
