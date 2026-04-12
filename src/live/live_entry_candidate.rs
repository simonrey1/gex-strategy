use crate::config::Ticker;
use crate::strategy::engine::{EntryCandidateData, RankedCandidate};

/// Entry candidate (ranked across tickers each poll cycle).
pub struct LiveEntryCandidate {
    pub ticker: Ticker,
    pub data: EntryCandidateData,
}

impl RankedCandidate for LiveEntryCandidate {
    fn ticker(&self) -> Ticker { self.ticker }
    fn rank_score(&self) -> f64 { self.data.tsi() }
}
