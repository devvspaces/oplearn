//! Daily-goal streaks with a weekly freeze (SPEC §9).
//!
//! A day counts when its active minutes reach the goal. One missed day per ISO week is bridged
//! automatically by a "freeze", but never two misses in a row. Today never breaks a streak: it
//! only adds to it once the goal is met.

use std::collections::{BTreeMap, HashSet};

use chrono::{Datelike, Duration, NaiveDate};
use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Streak {
    pub current: u32,
    pub longest: u32,
    /// The freeze for the current ISO week has been spent.
    pub freeze_used_this_week: bool,
    pub today_met: bool,
}

pub fn compute(day_minutes: &BTreeMap<NaiveDate, u32>, goal: u32, today: NaiveDate) -> Streak {
    let goal = goal.max(1);
    let met = |d: &NaiveDate| day_minutes.get(d).is_some_and(|m| *m >= goal);
    let Some(first) = day_minutes
        .iter()
        .find(|(_, m)| **m >= goal)
        .map(|(d, _)| *d)
    else {
        return Streak::default();
    };
    let mut freezes: HashSet<(i32, u32)> = HashSet::new();
    let (mut cur, mut longest, mut prev_frozen) = (0u32, 0u32, false);
    let mut day = first;
    while day < today {
        if met(&day) {
            cur += 1;
            prev_frozen = false;
        } else {
            let wk = (day.iso_week().year(), day.iso_week().week());
            if cur > 0 && !prev_frozen && !freezes.contains(&wk) {
                freezes.insert(wk);
                prev_frozen = true;
            } else {
                cur = 0;
                prev_frozen = false;
            }
        }
        longest = longest.max(cur);
        day += Duration::days(1);
    }
    // A freeze on the last day before today only holds if today is still winnable; a trailing
    // freeze followed by nothing is fine (today isn't over).
    let today_met = met(&today);
    if today_met {
        cur += 1;
        longest = longest.max(cur);
    }
    let wk = (today.iso_week().year(), today.iso_week().week());
    Streak {
        current: cur,
        longest,
        freeze_used_this_week: freezes.contains(&wk),
        today_met,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
    fn days(v: &[(&str, u32)]) -> BTreeMap<NaiveDate, u32> {
        v.iter().map(|(s, m)| (d(s), *m)).collect()
    }

    #[test]
    fn counts_consecutive_days_and_today_does_not_break() {
        let m = days(&[("2026-11-04", 50), ("2026-11-05", 45), ("2026-11-06", 60)]);
        let s = compute(&m, 45, d("2026-11-07"));
        assert_eq!((s.current, s.longest, s.today_met), (3, 3, false));
        let mut m2 = m.clone();
        m2.insert(d("2026-11-07"), 45);
        assert_eq!(compute(&m2, 45, d("2026-11-07")).current, 4);
    }

    #[test]
    fn one_freeze_per_week_bridges_a_single_miss() {
        // Mon, Tue met; Wed missed (freeze); Thu met.
        let m = days(&[
            ("2026-11-09", 50),
            ("2026-11-10", 50),
            ("2026-11-11", 5),
            ("2026-11-12", 50),
        ]);
        let s = compute(&m, 45, d("2026-11-13"));
        assert_eq!(s.current, 3);
        assert!(s.freeze_used_this_week);
    }

    #[test]
    fn two_misses_break_and_second_freeze_in_week_breaks() {
        let m = days(&[("2026-11-09", 50), ("2026-11-12", 50)]);
        assert_eq!(
            compute(&m, 45, d("2026-11-13")).current,
            1,
            "two missed days in a row"
        );
        // Mon met, Tue miss (freeze), Wed met, Thu miss (no freeze left), Fri met.
        let m = days(&[("2026-11-09", 50), ("2026-11-11", 50), ("2026-11-13", 50)]);
        let s = compute(&m, 45, d("2026-11-14"));
        assert_eq!((s.current, s.longest), (1, 2));
    }

    #[test]
    fn freeze_resets_next_iso_week() {
        // Sun 2026-11-15 ends week 46; Mon 2026-11-16 starts week 47.
        let m = days(&[
            ("2026-11-12", 50),
            ("2026-11-13", 1),
            ("2026-11-14", 50),
            ("2026-11-15", 50),
            ("2026-11-17", 50),
        ]);
        let s = compute(&m, 45, d("2026-11-18"));
        assert_eq!(s.current, 4);
        assert!(s.freeze_used_this_week);
    }

    #[test]
    fn empty_history() {
        assert_eq!(
            compute(&BTreeMap::new(), 45, d("2026-11-07")),
            Streak::default()
        );
    }
}
