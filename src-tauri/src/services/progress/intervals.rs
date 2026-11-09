//! Watched coverage as merged `[start, end]` intervals (seconds). Skipping ahead adds nothing,
//! so jumping to the end of a video doesn't count as watching it.

pub type Interval = [f64; 2];

/// Gaps up to this size are bridged when merging (timeupdate jitter).
const JOIN_GAP: f64 = 1.0;

/// Sorts, clamps to `[0, duration]` (when known), drops empty pieces and merges overlaps.
pub fn merge(mut v: Vec<Interval>, duration: Option<f64>) -> Vec<Interval> {
    let cap = duration.filter(|d| *d > 0.0).unwrap_or(f64::INFINITY);
    v.retain(|[a, b]| a.is_finite() && b.is_finite());
    for iv in &mut v {
        let (a, b) = (iv[0].min(iv[1]), iv[0].max(iv[1]));
        *iv = [a.max(0.0), b.min(cap)];
    }
    v.retain(|[a, b]| b > a);
    v.sort_by(|x, y| x[0].total_cmp(&y[0]));
    let mut out: Vec<Interval> = Vec::with_capacity(v.len());
    for iv in v {
        match out.last_mut() {
            Some(last) if iv[0] <= last[1] + JOIN_GAP => last[1] = last[1].max(iv[1]),
            _ => out.push(iv),
        }
    }
    out
}

/// Adds watched segments to existing intervals.
pub fn add(existing: Vec<Interval>, segs: &[Interval], duration: Option<f64>) -> Vec<Interval> {
    let mut v = existing;
    v.extend_from_slice(segs);
    merge(v, duration)
}

pub fn total(v: &[Interval]) -> f64 {
    v.iter().map(|[a, b]| b - a).sum()
}

/// Fraction of the video watched, 0..=1 (0 when the duration is unknown).
pub fn coverage(v: &[Interval], duration: Option<f64>) -> f64 {
    match duration.filter(|d| *d > 0.0) {
        Some(d) => (total(v) / d).clamp(0.0, 1.0),
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_overlaps_adjacency_and_order() {
        let m = merge(
            vec![[50.0, 60.0], [0.0, 10.0], [9.0, 20.0], [20.5, 30.0]],
            None,
        );
        assert_eq!(m, vec![[0.0, 30.0], [50.0, 60.0]]);
        assert_eq!(total(&m), 40.0);
    }

    #[test]
    fn clamps_to_duration_and_drops_junk() {
        let m = merge(
            vec![
                [-5.0, 3.0],
                [90.0, 200.0],
                [7.0, 7.0],
                [f64::NAN, 4.0],
                [40.0, 30.0],
            ],
            Some(100.0),
        );
        assert_eq!(m, vec![[0.0, 3.0], [30.0, 40.0], [90.0, 100.0]]);
    }

    #[test]
    fn skipping_to_the_end_is_not_coverage() {
        // Watched 10 s, jumped to the last 5 s of a 600 s video.
        let v = add(vec![], &[[0.0, 10.0], [595.0, 600.0]], Some(600.0));
        assert!(coverage(&v, Some(600.0)) < 0.03);
        // Watching it all reaches 1.0 even when reported in pieces and repeated.
        let v = add(
            v,
            &[[10.0, 300.0], [290.0, 595.0], [0.0, 50.0]],
            Some(600.0),
        );
        assert_eq!(coverage(&v, Some(600.0)), 1.0);
        assert_eq!(coverage(&v, None), 0.0);
    }
}
