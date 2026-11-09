//! SM-2 spaced repetition (SPEC §7.8, §9): Again=0, Hard=3, Good=4, Easy=5; ease floor 1.3.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Grade {
    Again,
    Hard,
    Good,
    Easy,
}

impl Grade {
    pub fn quality(self) -> u8 {
        match self {
            Self::Again => 0,
            Self::Hard => 3,
            Self::Good => 4,
            Self::Easy => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardState {
    pub ease: f64,
    pub interval_days: i32,
    pub repetitions: i32,
}

impl Default for CardState {
    fn default() -> Self {
        Self {
            ease: 2.5,
            interval_days: 0,
            repetitions: 0,
        }
    }
}

pub const MIN_EASE: f64 = 1.3;

/// Next state after a review. A failed recall (quality < 3) restarts repetitions with a 1-day
/// interval (the card is also shown again in the same session by the UI).
pub fn review(s: CardState, grade: Grade) -> CardState {
    let q = f64::from(grade.quality());
    let ease = (s.ease + (0.1 - (5.0 - q) * (0.08 + (5.0 - q) * 0.02))).max(MIN_EASE);
    if grade.quality() < 3 {
        return CardState {
            ease,
            interval_days: 1,
            repetitions: 0,
        };
    }
    let repetitions = s.repetitions + 1;
    let interval_days = match repetitions {
        1 => 1,
        2 => 6,
        _ => ((s.interval_days.max(1) as f64) * ease).round() as i32,
    };
    CardState {
        ease,
        interval_days: interval_days.clamp(1, 36500),
        repetitions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_progression() {
        let s = review(CardState::default(), Grade::Good);
        assert_eq!((s.interval_days, s.repetitions), (1, 1));
        assert!((s.ease - 2.5).abs() < 1e-9, "Good keeps ease: {}", s.ease);
        let s = review(s, Grade::Good);
        assert_eq!(s.interval_days, 6);
        let s = review(s, Grade::Good);
        assert_eq!(s.interval_days, 15); // 6 × 2.5
        let e = review(s, Grade::Easy);
        assert!(e.ease > s.ease && e.interval_days > 15);
    }

    #[test]
    fn again_resets_and_ease_has_a_floor() {
        let mut s = CardState {
            ease: 2.5,
            interval_days: 30,
            repetitions: 5,
        };
        s = review(s, Grade::Again);
        assert_eq!((s.interval_days, s.repetitions), (1, 0));
        for _ in 0..20 {
            s = review(s, Grade::Again);
        }
        assert_eq!(s.ease, MIN_EASE);
        let h = review(CardState::default(), Grade::Hard);
        assert!(h.ease < 2.5 && h.repetitions == 1);
    }
}
