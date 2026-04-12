use std::collections::HashSet;

use crate::config::Ticker;
use crate::strategy::slot_sizing::SlotSizing;

pub trait RankedCandidate {
    fn ticker(&self) -> Ticker;
    fn rank_score(&self) -> f64;
}

/// Sort candidates by rank_score (descending) and keep only the best per ticker.
pub fn rank_and_dedup<T: RankedCandidate>(candidates: &mut Vec<T>) {
    candidates.sort_by(|a, b| crate::types::cmp_f64(b.rank_score(), a.rank_score()));
    let mut seen = HashSet::new();
    candidates.retain(|c| seen.insert(c.ticker()));
}

/// [`rank_and_dedup`] then portfolio capacity for new entries (live `rank_and_execute` / backtest schedule).
#[inline]
pub fn rank_dedup_and_remaining_slots<T: RankedCandidate>(
    candidates: &mut Vec<T>,
    max_pos: usize,
    slots_used: usize,
) -> usize {
    rank_and_dedup(candidates);
    SlotSizing::remaining_slots(max_pos, slots_used)
}
