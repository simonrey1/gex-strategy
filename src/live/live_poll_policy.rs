use crate::types::{F64Trunc, ToF64};

/// Live polling / backoff policy (constants + derived delays).
pub struct LivePollPolicy;

impl LivePollPolicy {
    pub const POLL_INTERVAL_MS: u64 = 60_000;
    pub const FETCH_TIMEOUT_MS: u64 = 30_000;
    pub const MAX_CONSECUTIVE_FAILURES: u32 = 5;
    pub const MAX_BACKOFF_MS: u64 = 5 * 60_000;

    pub fn backoff_ms(consecutive_failures: u32) -> u64 {
        let base = Self::POLL_INTERVAL_MS.to_f64()
            * 2.0_f64.powi(consecutive_failures as i32 - 1);
        base.trunc_u64().min(Self::MAX_BACKOFF_MS)
    }

    pub fn should_emergency_close(consecutive_failures: u32, has_position: bool) -> bool {
        consecutive_failures >= Self::MAX_CONSECUTIVE_FAILURES && has_position
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles() {
        let b1 = LivePollPolicy::backoff_ms(1);
        let b2 = LivePollPolicy::backoff_ms(2);
        assert_eq!(b1, LivePollPolicy::POLL_INTERVAL_MS);
        assert_eq!(b2, LivePollPolicy::POLL_INTERVAL_MS * 2);
    }

    #[test]
    fn backoff_capped() {
        assert_eq!(LivePollPolicy::backoff_ms(20), LivePollPolicy::MAX_BACKOFF_MS);
    }

    #[test]
    fn emergency_close_requires_both() {
        assert!(!LivePollPolicy::should_emergency_close(LivePollPolicy::MAX_CONSECUTIVE_FAILURES, false));
        assert!(!LivePollPolicy::should_emergency_close(LivePollPolicy::MAX_CONSECUTIVE_FAILURES - 1, true));
        assert!(LivePollPolicy::should_emergency_close(LivePollPolicy::MAX_CONSECUTIVE_FAILURES, true));
    }
}
