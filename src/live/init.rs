use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;

use crate::config::{StrategyConfig, Ticker};
use crate::strategy::engine::DailyState;

use super::dashboard::{TickerHealth, TickerIndicators, update_health, SharedHealthState};
use super::log_debug;
use super::recovery::{startup_recovery, RecoveredSignalPipeline};
use super::state::{load_live_state, LiveTradingState};
use super::ticker_state::LiveTickerState;

pub use crate::live::live_context::LiveContext;

/// Initialize per-ticker state from saved state files + startup recovery.
pub async fn init_ticker_states(
    tickers: &[Ticker],
    config: &StrategyConfig,
    ibkr_client: &Arc<ibapi::Client>,
    health_state: &SharedHealthState,
    initial_equity: f64,
    spot_prices: &HashMap<Ticker, f64>,
) -> Result<HashMap<Ticker, LiveTickerState>> {
    let rctx = super::recovery::RecoveryCtx {
        ibkr_client,
        health: health_state,
    };
    let mut states: HashMap<Ticker, LiveTickerState> = HashMap::new();

    let shared_cfg = Arc::new(config.clone());

    for &ticker in tickers {
        let saved = load_live_state(ticker);
        let spot = spot_prices.get(&ticker).copied().unwrap_or(0.0);
        let pipe = startup_recovery(ticker, &shared_cfg, &rctx, saved.as_ref(), spot).await?;
        let RecoveredSignalPipeline { engine, wall_smoother, hurst, last_processed_ms } = pipe;

        let position = saved.as_ref().and_then(|s| s.position.clone());
        let bracket_ids = saved.as_ref().and_then(LiveTradingState::bracket_order_ids);

        match (&position, &bracket_ids) {
            (Some(_), Some(bi)) => {
                log_debug!(
                    "[live-{}] Restored position with bracket SL={} TP={}",
                    ticker, bi.stop_loss_id, bi.take_profit_id
                );
            }
            (Some(_), None) => {
                eprintln!(
                    "[live-{}] WARNING: restored position WITHOUT bracket IDs — will reconcile from IBKR",
                    ticker
                );
            }
            _ => {}
        }

        let last_known_equity = initial_equity;
        let daily_pnl = saved.as_ref().map(|s| s.daily_realized_pnl).unwrap_or(0.0);
        let mut daily = DailyState::new(config.daily_loss_limit_pct);
        daily.realized_pnl = daily_pnl;
        daily.entries = saved.as_ref().map(|s| s.daily_entries).unwrap_or(0);
        daily.loss_limit_hit = daily_pnl <= -(last_known_equity * config.daily_loss_limit_pct);
        if daily.loss_limit_hit {
            eprintln!("[live-{}] WARNING: daily loss limit already hit on restart (PnL {:.2})", ticker, daily_pnl);
        }

        let init_indicators = engine
            .last_indicator_values
            .as_ref()
            .map(TickerIndicators::from_values);
        let warmup_done = init_indicators.is_some();
        update_health(
            health_state,
            ticker.as_str(),
            TickerHealth {
                last_poll_ms: crate::types::now_ms(),
                position: position.is_some(),
                signal: Some(engine.signal_state.holding),
                equity: last_known_equity,
                spot_price: spot,
                indicators: init_indicators,
                warmup_status: if warmup_done { None } else { Some("Need more history".to_string()) },
                ..Default::default()
            },
        );

        states.insert(ticker, LiveTickerState {
            ticker,
            engine,
            config: shared_cfg.clone(),
            position,
            bracket_ids,
            daily,
            last_processed_ms,
            spot_price: spot,
            last_fresh_data_ms: crate::types::now_ms(),
            consecutive_failures: 0,
            last_known_equity,
            had_new_data: false,
            hurst,
            wall_smoother,
        });
    }

    Ok(states)
}
