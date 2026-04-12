use chrono::{DateTime, Utc};

use crate::config::StrategyConfig;
use crate::strategy::daily_state::DailyState;
use crate::strategy::eastern_time::et_hhmm;
use crate::strategy::entry_candidate_ctx::EntryCandidateCheckCtx;
use crate::types::{Signal, SignalReason};

/// Arguments for [`crate::strategy::engine::StrategyEngine::can_enter`].
pub struct CanEnterGate<'a> {
    pub signal: Signal,
    pub reason: &'a SignalReason,
    pub position_open: bool,
    pub daily: &'a DailyState,
    pub config: &'a StrategyConfig,
    pub bar_timestamp: DateTime<Utc>,
}

impl<'a> CanEnterGate<'a> {
    pub fn passes(&self) -> bool {
        if !self.reason.is_entry() {
            return false;
        }
        if !self.config.in_entry_time_window(et_hhmm(&self.bar_timestamp)) {
            return false;
        }
        !self.signal.is_flat()
            && !self.position_open
            && self.daily.allows_entry(self.config.max_entries_per_day)
    }
}

impl<'a> From<&'a EntryCandidateCheckCtx<'a>> for CanEnterGate<'a> {
    #[inline]
    fn from(ctx: &'a EntryCandidateCheckCtx<'a>) -> Self {
        Self {
            signal: ctx.build.signal.signal,
            reason: &ctx.build.signal.reason,
            position_open: ctx.position_open,
            daily: ctx.daily,
            config: ctx.build.config,
            bar_timestamp: ctx.build.bar.timestamp,
        }
    }
}
