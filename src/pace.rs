/// Smallest share of the window, in percentage points, treated as time left.
/// Keeps r finite in a window's last moments.
const MIN_REMAINING_PCT: f64 = 1.0;

/// r: the fraction of the nominal spend rate the rest of the window can sustain.
/// 1.0 is on pace; below 1.0 the remaining budget must be spent more slowly.
pub fn ratio(used_pct: f64, remaining_min: f64, window_min: f64) -> f64 {
    let remaining_pct = (remaining_min / window_min * 100.0).max(MIN_REMAINING_PCT);
    ((100.0 - used_pct) / remaining_pct).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEEK: f64 = 10080.0;

    fn remaining(elapsed_pct: f64) -> f64 {
        WEEK * (100.0 - elapsed_pct) / 100.0
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.005
    }

    #[test]
    fn on_pace_is_one() {
        assert!(close(ratio(0.0, WEEK, WEEK), 1.0));
        assert!(close(ratio(50.0, remaining(50.0), WEEK), 1.0));
    }

    #[test]
    fn same_overspend_weighs_more_late() {
        // 10 points over pace
        assert!(close(ratio(25.0, remaining(15.0), WEEK), 0.882));
        assert!(close(ratio(70.0, remaining(60.0), WEEK), 0.75));
        assert!(close(ratio(100.0, remaining(90.0), WEEK), 0.0));
    }

    #[test]
    fn exhausted_is_zero() {
        assert_eq!(ratio(100.0, remaining(40.0), WEEK), 0.0);
        assert_eq!(ratio(104.0, remaining(40.0), WEEK), 0.0);
    }

    #[test]
    fn reset_mid_window_frees_budget() {
        assert!(close(ratio(0.0, remaining(50.0), WEEK), 2.0));
    }

    #[test]
    fn stays_finite_at_window_end() {
        let r = ratio(91.0, 0.0, WEEK);
        assert!(r.is_finite());
        assert!(close(r, 9.0));
    }
}
