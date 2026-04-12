//! Strategy logic shared by **live** and **backtest** via [`engine::StrategyEngine`].
//!
//! **Both runners** call the same engine pipeline (`process_gex_bar_pipeline`, `warm_up`,
//! wall trail, slot sizing, `bar_ctx` / `build_entry_candidate`, etc.): see `live::runner` and
//! `backtest::runner`.
//!
//! - **Signal path**: `engine` → `entries` / `signals` / `wall_trail` / `indicators` / …
//! - **GEX profile build**: [`gex`] (`OptionsSnapshot::compute_gex_profile`) via `data::gex_builder` and `data::thetadata_live`.
//! - **ET / naive Theta strings**: [`eastern_time`] is only used inside `data::thetadata_hist::parse_theta_timestamp`
//!   (wide-row grouping). That runs for **backtest cache** and **live recovery** GEX (`profiles_from_wide`).
//!   The live **poll** keeps snapshot time as wall-clock `Utc::now()` (see `thetadata_live`).
//!
//! **Backtest-only features** (`iv_scan`, chart spike tooltips) orchestrate extra analysis but reuse the same
//! `BarCtx` / `vf_gates` / `iv_eligibility` types the engine already uses on every bar in both modes.

pub mod can_enter_gate;
pub mod config_stops;
pub mod daily_state;
pub mod entry_candidate_ctx;
pub mod engine;
pub mod entries;
pub mod entry_candidate_data;
pub mod position_cash;
pub mod ranked_candidate;
pub mod exits;
pub mod gex;
pub mod eastern_time;
pub mod hurst;
pub mod indicators;
pub mod shared;
pub mod signals;
pub mod slot_sizing;
pub mod trail_fields;
pub mod process_gex_bar_ctx;
pub mod wall_trail;
pub mod wall_smoother;
pub mod warmup_result;
pub mod zone;
