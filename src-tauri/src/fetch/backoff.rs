//! Exponential backoff for failed fetch attempts (see
//! docs/ARCHITECTURE.md §7): 1min -> 5min -> 30min, capped at the
//! feed's own polling interval so a struggling feed is never retried
//! *less* often than it would normally be polled - up to a point.
//!
//! Past [`PERSISTENT_FAILURE_THRESHOLD`] consecutive failures, that
//! floor is dropped: a host that has rejected/timed-out this many
//! attempts in a row is most likely dead or actively blocking us, and
//! continuing to hit it every interval forever only makes that worse.
//! Retries then slow to [`PERSISTENT_RETRY_SECS`] (or the feed's own
//! interval, if longer) until it recovers.

const STEPS_SECS: [i64; 3] = [60, 300, 1800];

const PERSISTENT_FAILURE_THRESHOLD: i64 = 6;
const PERSISTENT_RETRY_SECS: i64 = 6 * 60 * 60; // 6h

/// Seconds to wait before the next attempt, given how many
/// consecutive failures this feed now has (>= 1) and its normal
/// polling interval.
pub fn backoff_secs(consecutive_failures: i64, base_interval_secs: i64) -> i64 {
    let failures = consecutive_failures.max(1);
    if failures > PERSISTENT_FAILURE_THRESHOLD {
        return PERSISTENT_RETRY_SECS.max(base_interval_secs);
    }
    let index = (failures - 1) as usize;
    let step = STEPS_SECS[index.min(STEPS_SECS.len() - 1)];
    step.min(base_interval_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_INTERVAL: i64 = 900; // schema default (15 min)

    #[test]
    fn steps_through_1min_5min_30min() {
        assert_eq!(backoff_secs(1, DEFAULT_INTERVAL), 60);
        assert_eq!(backoff_secs(2, DEFAULT_INTERVAL), 300);
        assert_eq!(
            backoff_secs(3, DEFAULT_INTERVAL),
            1800.min(DEFAULT_INTERVAL)
        );
    }

    #[test]
    fn stays_at_the_last_step_until_the_persistent_threshold() {
        let third = backoff_secs(3, DEFAULT_INTERVAL);
        assert_eq!(backoff_secs(4, DEFAULT_INTERVAL), third);
        assert_eq!(
            backoff_secs(PERSISTENT_FAILURE_THRESHOLD, DEFAULT_INTERVAL),
            third
        );
    }

    #[test]
    fn escalates_past_the_normal_interval_once_persistently_failing() {
        let escalated = backoff_secs(PERSISTENT_FAILURE_THRESHOLD + 1, DEFAULT_INTERVAL);
        assert_eq!(escalated, PERSISTENT_RETRY_SECS);
        // Stays escalated, doesn't creep back down to the base interval.
        assert_eq!(backoff_secs(20, DEFAULT_INTERVAL), escalated);
    }

    #[test]
    fn escalated_retry_never_undercuts_a_longer_base_interval() {
        let long_interval = PERSISTENT_RETRY_SECS * 2;
        assert_eq!(
            backoff_secs(PERSISTENT_FAILURE_THRESHOLD + 1, long_interval),
            long_interval
        );
    }

    #[test]
    fn caps_at_the_feed_base_interval() {
        // 30min (1800s) step would exceed a 900s base interval.
        assert_eq!(backoff_secs(3, 900), 900);
        // Even the 1st step (60s) gets capped if the feed's own
        // interval is shorter than that.
        assert_eq!(backoff_secs(1, 30), 30);
    }

    #[test]
    fn treats_zero_failures_the_same_as_one() {
        // Callers always pass >= 1 (it's "how many failures *now*"),
        // but the clamp means passing 0 degrades gracefully rather
        // than panicking on the `- 1` index arithmetic.
        assert_eq!(
            backoff_secs(0, DEFAULT_INTERVAL),
            backoff_secs(1, DEFAULT_INTERVAL)
        );
    }
}
