//! Barrel re-exports for types and helpers used across live, backtest, and IV scan.

pub use crate::types::{AtrRegimeTsi, BarPriceAtr, BarVolRegime, EntryAtrTsi, opt_finite, safe_ratio};
pub use crate::strategy::entries::iv_eligibility::{
    IvCompressionInputs, SpikeApplyMut, SpikeCheckInputs, SpotWallAtrRegime,
};
pub use crate::strategy::entries::{SmoothedWalls, SpreadBandInputs};
pub use crate::strategy::config_stops::{PlainStopInputs, StopBracketInputs};
pub use crate::strategy::can_enter_gate::CanEnterGate;
pub use crate::strategy::daily_state::DailyState;
pub use crate::strategy::entry_candidate_ctx::{EntryCandidateBuildCtx, EntryCandidateCheckCtx};
pub use crate::strategy::process_gex_bar_ctx::{GexPipelineBar, ProcessGexBarCtx};
pub use crate::strategy::entry_candidate_data::EntryCandidateData;
pub use crate::strategy::position_cash::{EntrySharesInputs, ExitPnlInputs, PositionCash};
pub use crate::strategy::signals::{WallAtrDiffs, WallDiffInputs};
pub use crate::strategy::zone::{WallTrackParams, ZoneLevelWidth, ZoneTickBar};
pub use crate::strategy::slot_sizing::{
    CloseLineQuote, EntryOpenDailyLog, EntryOpenLineQuote, EntryOpenLogKind, EntryPrepareCtx,
    EntryPrepareInputs, EntryPriceConfig, EntryRegimeFields, PortfolioSlotSizing, PrepareEntryError,
    PrepareOutcomeExt, PreparedEntry, PreparedOpenLineCtx, RunnerMode, RunnerTickerLog, SizedEntryBrackets,
    SlotSizing, StopsRegimeCtx,
};
pub use crate::strategy::ranked_candidate::{RankedCandidate, rank_and_dedup, rank_dedup_and_remaining_slots};
pub use crate::strategy::trail_fields::{HasTrailFields, TrailFields, trail_fields_for_position};
pub use crate::strategy::entries::vf_gates::{RegimeCtx, VfCompressParams, VfGateCtx};
pub use crate::strategy::wall_trail::{
    TrailCheckInputs, WallTrailOutcome, WallTrailRatchetInputs, check_trail, wall_trail_sl,
};
pub use crate::strategy::wall_smoother::SmoothedWallLevels;
