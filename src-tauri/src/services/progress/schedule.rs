//! Study-plan generation and on-track status (SPEC §7.6, §9). Lesson order is preserved.

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone)]
pub struct PlanLesson {
    pub id: String,
    pub minutes: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    /// Finish by this date (inclusive, `YYYY-MM-DD`).
    Date {
        date: String,
    },
    LessonsPerWeek {
        value: u32,
    },
    MinutesPerWeek {
        value: u32,
    },
}

/// Minutes assumed for lessons with no known duration.
pub const DEFAULT_LESSON_MIN: f64 = 10.0;

/// Upcoming study days from `start` (inclusive). An empty set means every day.
fn study_days(start: NaiveDate, days: &[Weekday]) -> impl Iterator<Item = NaiveDate> + '_ {
    (0..)
        .map(move |i| start + Duration::days(i))
        .filter(move |d| days.is_empty() || days.contains(&d.weekday()))
}

fn days_per_week(days: &[Weekday]) -> f64 {
    if days.is_empty() {
        7.0
    } else {
        days.len() as f64
    }
}

/// Assigns each lesson to a study day. Lessons keep their order and are spread so each day's
/// load is even: by minutes for date and minutes-per-week targets, by count for lessons-per-week.
pub fn generate(
    lessons: &[PlanLesson],
    days: &[Weekday],
    start: NaiveDate,
    target: &Target,
) -> Vec<(String, NaiveDate)> {
    if lessons.is_empty() {
        return vec![];
    }
    // A deadline with no study days before it (e.g. "today" on a weekend) uses every day.
    let all_days: [Weekday; 0] = [];
    let days = match target {
        Target::Date { date } => {
            let end = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap_or(start);
            if study_days(start, days).take(400).any(|d| d <= end) || end < start {
                days
            } else {
                &all_days[..]
            }
        }
        _ => days,
    };
    let mins: Vec<f64> = lessons
        .iter()
        .map(|l| {
            if l.minutes > 0.0 {
                l.minutes
            } else {
                DEFAULT_LESSON_MIN
            }
        })
        .collect();
    let total: f64 = mins.iter().sum();
    // Day slot for each lesson (index into the sequence of study days).
    let slots: Vec<usize> = match *target {
        Target::Date { ref date } => {
            let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap_or(start);
            let n = study_days(start, days)
                .take(3650)
                .take_while(|d| *d <= date)
                .count()
                .max(1);
            let mut cum = 0.0;
            mins.iter()
                .map(|m| {
                    let mid = cum + m / 2.0;
                    cum += m;
                    ((mid / total * n as f64) as usize).min(n - 1)
                })
                .collect()
        }
        Target::LessonsPerWeek { value } => {
            let per_day = value.max(1) as f64 / days_per_week(days);
            (0..lessons.len())
                .map(|k| (k as f64 / per_day) as usize)
                .collect()
        }
        Target::MinutesPerWeek { value } => {
            let budget = (value.max(1) as f64 / days_per_week(days)).max(1.0);
            // Greedy: fill a day up to the budget; a lesson longer than the budget gets its own day.
            let (mut day, mut load) = (0usize, 0.0);
            mins.iter()
                .map(|m| {
                    if load > 0.0 && load + m > budget + 0.01 {
                        day += 1;
                        load = 0.0;
                    }
                    load += m;
                    day
                })
                .collect()
        }
    };
    let last = slots.iter().copied().max().unwrap_or(0);
    let dates: Vec<NaiveDate> = study_days(start, days).take(last + 1).collect();
    lessons
        .iter()
        .zip(slots)
        .map(|(l, s)| (l.id.clone(), dates[s]))
        .collect()
}

/// Places pre-grouped weeks (e.g. from the AI planner) onto study days: week k uses the k-th
/// calendar week from `start`, and its lessons are spread evenly over that week's study days.
pub fn schedule_weeks(
    weeks: &[Vec<String>],
    days: &[Weekday],
    start: NaiveDate,
) -> Vec<(String, NaiveDate)> {
    let mut out = vec![];
    // Week boundaries are Monday-based; the first week is whatever remains of this one.
    let first_monday = start - Duration::days(start.weekday().num_days_from_monday() as i64);
    for (k, ids) in weeks.iter().enumerate() {
        let wk_start = (first_monday + Duration::days(7 * k as i64)).max(start);
        let wk_end = first_monday + Duration::days(7 * k as i64 + 6);
        let mut avail: Vec<NaiveDate> = study_days(wk_start, days)
            .take_while(|d| *d <= wk_end)
            .collect();
        if avail.is_empty() {
            avail = (0..=(wk_end - wk_start).num_days())
                .map(|i| wk_start + Duration::days(i))
                .collect();
        }
        let n = ids.len().max(1);
        for (i, id) in ids.iter().enumerate() {
            let slot = (i * avail.len() / n).min(avail.len() - 1);
            out.push((id.clone(), avail[slot]));
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PlanStatus {
    OnTrack,
    Behind { lessons: u32 },
    Ahead { lessons: u32 },
}

/// `items` are `(planned_date, done)`. Lessons planned before today and still open count as
/// behind; finishing more than is planned through today counts as ahead.
pub fn status(items: &[(NaiveDate, bool)], today: NaiveDate) -> PlanStatus {
    let overdue = items.iter().filter(|(d, done)| *d < today && !done).count() as u32;
    if overdue > 0 {
        return PlanStatus::Behind { lessons: overdue };
    }
    let done = items.iter().filter(|(_, done)| *done).count() as u32;
    let due = items.iter().filter(|(d, _)| *d <= today).count() as u32;
    if done > due {
        PlanStatus::Ahead {
            lessons: done - due,
        }
    } else {
        PlanStatus::OnTrack
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Weekday::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
    fn lessons(mins: &[f64]) -> Vec<PlanLesson> {
        mins.iter()
            .enumerate()
            .map(|(i, m)| PlanLesson {
                id: format!("l{i}"),
                minutes: *m,
            })
            .collect()
    }
    const WEEKDAYS: [Weekday; 5] = [Mon, Tue, Wed, Thu, Fri];

    #[test]
    fn date_target_spreads_evenly_and_finishes_on_time() {
        // Mon 2026-11-09 .. Fri 2026-11-20: 10 weekdays, 20 one-hour lectures → 2 per day.
        let plan = generate(
            &lessons(&[60.0; 20]),
            &WEEKDAYS,
            d("2026-11-09"),
            &Target::Date {
                date: "2026-11-20".into(),
            },
        );
        assert_eq!(plan.len(), 20);
        assert_eq!(plan.last().unwrap().1, d("2026-11-20"));
        assert!(
            plan.iter()
                .all(|(_, day)| !matches!(day.weekday(), Sat | Sun))
        );
        let mut per_day = std::collections::BTreeMap::new();
        for (_, day) in &plan {
            *per_day.entry(*day).or_insert(0) += 1;
        }
        assert!(per_day.values().all(|n| *n == 2), "{per_day:?}");
        // Order preserved.
        assert!(plan.windows(2).all(|w| w[0].1 <= w[1].1));
    }

    #[test]
    fn date_in_the_past_puts_everything_on_the_first_day() {
        let plan = generate(
            &lessons(&[30.0, 30.0]),
            &[],
            d("2026-11-09"),
            &Target::Date {
                date: "2026-01-01".into(),
            },
        );
        assert!(plan.iter().all(|(_, day)| *day == d("2026-11-09")));
    }

    #[test]
    fn deadline_without_study_days_uses_every_day() {
        // Sunday deadline = today, weekday-only schedule.
        let plan = generate(
            &lessons(&[30.0, 30.0]),
            &WEEKDAYS,
            d("2026-11-08"),
            &Target::Date {
                date: "2026-11-08".into(),
            },
        );
        assert!(plan.iter().all(|(_, day)| *day == d("2026-11-08")));
    }

    #[test]
    fn lessons_per_week() {
        // 3 lessons/week on Mon/Wed/Fri → one per study day.
        let plan = generate(
            &lessons(&[80.0; 4]),
            &[Mon, Wed, Fri],
            d("2026-11-09"),
            &Target::LessonsPerWeek { value: 3 },
        );
        let days: Vec<_> = plan.iter().map(|p| p.1).collect();
        assert_eq!(
            days,
            vec![
                d("2026-11-09"),
                d("2026-11-11"),
                d("2026-11-13"),
                d("2026-11-16")
            ]
        );
    }

    #[test]
    fn minutes_per_week_respects_budget_but_never_splits_a_lesson() {
        // 300 min/week over 5 days = 60/day. 30+30 fit, 90 gets its own day, then 20.
        let plan = generate(
            &lessons(&[30.0, 30.0, 90.0, 20.0]),
            &WEEKDAYS,
            d("2026-11-09"),
            &Target::MinutesPerWeek { value: 300 },
        );
        let days: Vec<_> = plan.iter().map(|p| p.1).collect();
        assert_eq!(
            days,
            vec![
                d("2026-11-09"),
                d("2026-11-09"),
                d("2026-11-10"),
                d("2026-11-11")
            ]
        );
    }

    #[test]
    fn unknown_durations_use_default() {
        let plan = generate(
            &lessons(&[0.0, 0.0, 0.0]),
            &[],
            d("2026-11-09"),
            &Target::MinutesPerWeek { value: 140 },
        );
        // 20 min/day budget, 10 min lessons → two per day.
        assert_eq!(plan[1].1, d("2026-11-09"));
        assert_eq!(plan[2].1, d("2026-11-10"));
    }

    #[test]
    fn weeks_map_onto_study_days() {
        // Wednesday start: week 1 is Wed–Fri, week 2 is the next Mon–Fri.
        let weeks = vec![
            vec!["a".into(), "b".into(), "c".into()],
            vec!["d".into(), "e".into()],
        ];
        let p = schedule_weeks(&weeks, &WEEKDAYS, d("2026-11-11"));
        let days: Vec<_> = p.iter().map(|x| x.1).collect();
        assert_eq!(
            days,
            vec![
                d("2026-11-11"),
                d("2026-11-12"),
                d("2026-11-13"),
                d("2026-11-16"),
                d("2026-11-18")
            ]
        );
        // A week with no study days left (Saturday start, weekdays only) still gets dates.
        let p = schedule_weeks(&[vec!["x".into()]], &WEEKDAYS, d("2026-11-14"));
        assert_eq!(p[0].1, d("2026-11-14"));
    }

    #[test]
    fn status_rules() {
        let today = d("2026-11-11");
        let items = vec![
            (d("2026-11-09"), true),
            (d("2026-11-10"), false),
            (d("2026-11-11"), false),
            (d("2026-11-12"), false),
        ];
        assert_eq!(status(&items, today), PlanStatus::Behind { lessons: 1 });
        let items = vec![(d("2026-11-10"), true), (d("2026-11-11"), false)];
        assert_eq!(status(&items, today), PlanStatus::OnTrack);
        let items = vec![
            (d("2026-11-10"), true),
            (d("2026-11-12"), true),
            (d("2026-11-13"), true),
        ];
        assert_eq!(status(&items, today), PlanStatus::Ahead { lessons: 2 });
    }
}
