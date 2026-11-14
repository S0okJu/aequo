//! Exponential backoff for failed fetch attempts (see
//! docs/ARCHITECTURE.md §7): 1min -> 5min -> 30min, capped at the
//! feed's own polling interval so a struggling feed is never retried
//! *less* often than it would normally be polled.

const STEPS_SECS: [i64; 3] = [60, 300, 1800];

/// Seconds to wait before the next attempt, given how many
/// consecutive failures this feed now has (>= 1) and its normal
/// polling interval.
pub fn backoff_secs(consecutive_failures: i64, base_interval_secs: i64) -> i64 {
    let index = (consecutive_failures.max(1) - 1) as usize;
    let step = STEPS_SECS[index.min(STEPS_SECS.len() - 1)];
    step.min(base_interval_secs)
}
