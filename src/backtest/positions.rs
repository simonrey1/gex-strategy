use chrono::{DateTime, Utc};
use ts_rs::TS;

use crate::config::{BacktestConfig, BarIndex, Ticker, BAR_INTERVAL_MINUTES, strategy_bars_from_1m_count};
use crate::types::{Signal, ToF64};

pub use crate::types::BARS_PER_DAY;
pub use crate::strategy::shared::PositionCash;

use crate::strategy::shared::PreparedEntry;
pub use super::calendar::get_trading_days;
pub use super::costs::TICK_SIZE;

// ─── Position / Trade types ─────────────────────────────────────────────────

/// Exit time, fill, reason, and backtest config for [`Position::close`].
pub struct PositionCloseCtx<'a> {
    pub exit_time: DateTime<Utc>,
    pub raw_exit_price: f64,
    pub exit_reason: &'a str,
    pub ticker: Ticker,
    pub bc: &'a BacktestConfig,
}

impl<'a> PositionCloseCtx<'a> {}

#[derive(Debug, Clone)]
pub struct Position {
    pub signal: Signal,
    pub entry_time: DateTime<Utc>,
    pub raw_entry_price: f64,
    pub entry_price: f64,
    pub shares: u32,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub entry_cost: f64,
    pub entry_commission: f64,
    pub entry_slippage: f64,
    pub entry_atr: f64,
    /// Highest structural put wall seen since entry — used for wall-trailing SL.
    pub highest_put_wall: f64,
    /// IV spike bar index that triggered this entry — for scan matching.
    pub spike_bar: BarIndex,
    /// Highest bar high seen since entry — for max runup tracking.
    pub max_high: f64,
    /// Highest strategy-bar close seen since entry — for gain-based profit floor.
    pub highest_close: f64,
    /// Consecutive strategy bars with Hurst below exhaustion threshold.
    pub hurst_exhaust_bars: u32,
    /// Number of 1-min bars held.
    pub bars_held: i64,
}

impl Position {
    /// Build a position from [`PreparedEntry`] (shares + SL/TP from [`SlotSizing::prepare_entry`](crate::strategy::slot_sizing::SlotSizing::prepare_entry)).
    pub fn open_with_prep(
        prep: &PreparedEntry,
        signal: Signal,
        entry_time: DateTime<Utc>,
        raw_entry_price: f64,
        spike_bar: BarIndex,
        atr: f64,
        bc: &BacktestConfig,
    ) -> Self {
        let entry_slippage = bc.slippage();
        let entry_price = raw_entry_price + entry_slippage;
        let sh = prep.shares.to_f64();
        let entry_cost = sh * entry_price;
        Self {
            signal,
            entry_time,
            raw_entry_price,
            entry_price,
            shares: prep.shares,
            stop_loss: prep.stop_loss,
            take_profit: prep.take_profit,
            entry_cost,
            entry_commission: bc.commission(prep.shares, entry_cost),
            entry_slippage: entry_slippage * sh,
            entry_atr: atr,
            spike_bar,
            highest_put_wall: 0.0,
            max_high: entry_price,
            highest_close: entry_price,
            hurst_exhaust_bars: 0,
            bars_held: 0,
        }
    }

    pub fn close(&self, ctx: &PositionCloseCtx<'_>) -> (Trade, f64) {
        let exit_slippage = ctx.bc.slippage();
        let exit_price = ctx.raw_exit_price - exit_slippage;
        let sh = self.shares.to_f64();
        let exit_value = sh * exit_price;
        let exit_commission = ctx.bc.commission(self.shares, exit_value);

        let total_commission = self.entry_commission + exit_commission;
        let total_slippage = self.entry_slippage + exit_slippage * sh;

        let gross_pnl = (ctx.raw_exit_price - self.raw_entry_price) * sh;
        let net_pnl = gross_pnl - total_commission - total_slippage;
        let return_pct = if self.raw_entry_price > 0.0 {
            (net_pnl / (self.raw_entry_price * sh)) * 100.0
        } else {
            0.0
        };

        let max_runup_atr = if self.entry_atr > 0.0 {
            (self.max_high - self.entry_price) / self.entry_atr
        } else {
            0.0
        };
        let trade = Trade {
            ticker: ctx.ticker,
            signal: self.signal,
            entry_time: self.entry_time.to_rfc3339(),
            entry_price: self.entry_price,
            exit_time: ctx.exit_time.to_rfc3339(),
            exit_price,
            shares: self.shares,
            gross_pnl,
            commission: total_commission,
            slippage: total_slippage,
            net_pnl,
            return_pct,
            exit_reason: ctx.exit_reason.to_string(),
            max_runup_atr,
            bars_held: strategy_bars_from_1m_count(self.bars_held, BAR_INTERVAL_MINUTES),
            spike_bar: self.spike_bar,
            diagnostics: None,
        };

        let capital_returned = exit_value - exit_commission;
        (trade, capital_returned)
    }

    /// Combined SL/TP check for a single bar.
    /// When both trigger on the same bar, uses proximity heuristic.
    pub fn check_sltp(&self, bar: &crate::types::OhlcBar) -> Option<(f64, &'static str)> {
        if let Some(sl) = check_stop_loss(bar.open, bar.low, self.stop_loss) {
            if bar.open >= self.take_profit {
                return Some((bar.open, "take_profit_gap"));
            }
            let tp_hit = bar.high >= self.take_profit;
            if tp_hit && (bar.open - self.stop_loss) > (self.take_profit - bar.open) {
                return Some((self.take_profit, "take_profit"));
            }
            return Some(sl);
        }
        if bar.open >= self.take_profit {
            return Some((bar.open, "take_profit_gap"));
        }
        if bar.high >= self.take_profit {
            return Some((self.take_profit, "take_profit"));
        }
        None
    }
}

impl crate::strategy::engine::HasTrailFields for Position {
    fn trail_fields(&mut self) -> crate::strategy::engine::TrailFields<'_> {
        crate::strategy::engine::TrailFields {
            stop_loss: &mut self.stop_loss,
            highest_put_wall: &mut self.highest_put_wall,
            highest_close: &mut self.highest_close,
            hurst_exhaust_bars: &mut self.hurst_exhaust_bars,
            entry_price: self.entry_price,
            tp: self.take_profit,
            signal: self.signal,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct TradeDiagnostics {
    #[serde(flatten)]
    #[ts(flatten)]
    pub entry: super::types::EntryDiag,
    #[serde(rename = "exitPutWall")]
    pub exit_put_wall: Option<f64>,
    #[serde(rename = "exitCallWall")]
    pub exit_call_wall: Option<f64>,
    #[serde(rename = "exitNetGex")]
    pub exit_net_gex: f64,
    #[serde(rename = "callWallBelowEntry")]
    pub call_wall_below_entry: bool,
}

#[derive(Debug, Clone, serde::Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct Trade {
    pub ticker: Ticker,
    pub signal: Signal,
    #[serde(rename = "entryTime")]
    pub entry_time: String,
    #[serde(rename = "entryPrice")]
    pub entry_price: f64,
    #[serde(rename = "exitTime")]
    pub exit_time: String,
    #[serde(rename = "exitPrice")]
    pub exit_price: f64,
    pub shares: u32,
    #[serde(rename = "grossPnl")]
    pub gross_pnl: f64,
    pub commission: f64,
    pub slippage: f64,
    #[serde(rename = "netPnl")]
    pub net_pnl: f64,
    #[serde(rename = "returnPct")]
    pub return_pct: f64,
    #[serde(rename = "exitReason")]
    pub exit_reason: String,
    /// (max_high - entry_price) / entry_atr — peak unrealised gain in ATR units.
    #[serde(rename = "maxRunupAtr")]
    pub max_runup_atr: f64,
    /// Strategy-bar count held (1‑minute `bars_held` / configured bar interval).
    #[serde(rename = "barsHeld")]
    pub bars_held: i64,
    /// IV spike bar index that triggered this entry — for scan matching.
    #[serde(rename = "spikeBar")]
    pub spike_bar: BarIndex,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<TradeDiagnostics>,
}

#[derive(Debug, Clone, serde::Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct WallEvent {
    pub time: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub wall: f64,
    pub low: f64,
    pub high: f64,
    pub close: f64,
    #[serde(rename = "movePct")]
    pub move_pct: f64,
    pub blocked: String,
}

/// SL check: gap-open or intra-bar. Used by backtest execution and IV scan.
pub fn check_stop_loss(open: f64, low: f64, stop_loss: f64) -> Option<(f64, &'static str)> {
    if open <= stop_loss { return Some((open, "stop_loss_gap")); }
    if low <= stop_loss { return Some((stop_loss, "stop_loss")); }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::OhlcBar;
    use chrono::TimeZone;

    fn test_bc() -> crate::config::BacktestConfig {
        crate::config::BacktestConfig {
            starting_capital: 100_000.0,
            commission_per_share: 0.005,
            commission_min: 1.00,
            slippage_ticks: 2.0,
            execution_delay_bars: 3,
        }
    }

    fn make_position() -> Position {
        Position {
            signal: Signal::LongVannaFlip,
            entry_time: Utc.with_ymd_and_hms(2025, 2, 12, 14, 30, 0).unwrap(),
            raw_entry_price: 99.98,
            entry_price: 100.0,
            shares: 400,
            stop_loss: 98.0,
            take_profit: 104.0,
            entry_cost: 40_000.0,
            entry_commission: 2.0,
            entry_slippage: 8.0,
            entry_atr: 1.5,
            spike_bar: 0,
            highest_put_wall: 0.0,
            max_high: 100.0,
            highest_close: 100.0,
            hurst_exhaust_bars: 0,
            bars_held: 0,
        }
    }

    #[test]
    fn close_applies_slippage() {
        let bc = test_bc();
        let pos = make_position();
        let exit_time = Utc.with_ymd_and_hms(2025, 2, 12, 15, 0, 0).unwrap();
        let (trade, _) = pos.close(&PositionCloseCtx { exit_time, raw_exit_price: 105.0, exit_reason: "take_profit", ticker: Ticker::AAPL, bc: &bc });
        let expected_exit = 105.0 - bc.slippage_ticks * TICK_SIZE;
        assert!((trade.exit_price - expected_exit).abs() < 1e-6);
    }

    #[test]
    fn close_gross_pnl_uses_raw_prices() {
        let bc = test_bc();
        let mut pos = make_position();
        pos.raw_entry_price = 100.0;
        pos.entry_price = 100.02;
        pos.shares = 400;
        let exit_time = Utc.with_ymd_and_hms(2025, 2, 12, 15, 0, 0).unwrap();
        let (trade, _) = pos.close(&PositionCloseCtx { exit_time, raw_exit_price: 105.0, exit_reason: "exit", ticker: Ticker::AAPL, bc: &bc });
        assert!((trade.gross_pnl - (105.0 - 100.0) * 400.0).abs() < 1e-4);
    }

    #[test]
    fn close_net_pnl_deducts_costs() {
        let bc = test_bc();
        let pos = make_position();
        let exit_time = Utc.with_ymd_and_hms(2025, 2, 12, 15, 0, 0).unwrap();
        let (trade, _) = pos.close(&PositionCloseCtx { exit_time, raw_exit_price: 105.0, exit_reason: "exit", ticker: Ticker::AAPL, bc: &bc });
        assert!((trade.net_pnl - (trade.gross_pnl - trade.commission - trade.slippage)).abs() < 1e-4);
    }

    #[test]
    fn close_winning_trade() {
        let bc = test_bc();
        let pos = make_position();
        let exit_time = Utc.with_ymd_and_hms(2025, 2, 12, 15, 0, 0).unwrap();
        let (trade, _) = pos.close(&PositionCloseCtx { exit_time, raw_exit_price: 105.0, exit_reason: "take_profit", ticker: Ticker::AAPL, bc: &bc });
        assert!(trade.gross_pnl > 0.0);
        assert!(trade.net_pnl > 0.0);
        assert!(trade.return_pct > 0.0);
    }

    #[test]
    fn close_losing_trade() {
        let bc = test_bc();
        let pos = make_position();
        let exit_time = Utc.with_ymd_and_hms(2025, 2, 12, 15, 0, 0).unwrap();
        let (trade, _) = pos.close(&PositionCloseCtx { exit_time, raw_exit_price: 95.0, exit_reason: "stop_loss", ticker: Ticker::AAPL, bc: &bc });
        assert!(trade.gross_pnl < 0.0);
        assert!(trade.return_pct < 0.0);
    }

    #[test]
    fn check_sltp_gap_down() {
        let bar = OhlcBar {
            timestamp: Utc::now(),
            open: 97.0, high: 98.0, low: 96.0, close: 97.5, volume: 1000.0,
        };
        let pos = make_position();
        let result = pos.check_sltp(&bar);
        assert!(result.is_some());
        let (price, reason) = result.unwrap();
        assert!((price - 97.0).abs() < 1e-6);
        assert_eq!(reason, "stop_loss_gap");
    }

    #[test]
    fn check_sltp_hit() {
        let bar = OhlcBar {
            timestamp: Utc::now(),
            open: 99.0, high: 100.0, low: 97.5, close: 98.0, volume: 1000.0,
        };
        let pos = make_position();
        let result = pos.check_sltp(&bar);
        assert!(result.is_some());
        let (price, reason) = result.unwrap();
        assert!((price - 98.0).abs() < 1e-6);
        assert_eq!(reason, "stop_loss");
    }

    #[test]
    fn check_sltp_gap_up() {
        let bar = OhlcBar {
            timestamp: Utc::now(),
            open: 105.0, high: 106.0, low: 104.5, close: 105.5, volume: 1000.0,
        };
        let pos = make_position();
        let result = pos.check_sltp(&bar);
        assert!(result.is_some());
        let (price, reason) = result.unwrap();
        assert!((price - 105.0).abs() < 1e-6);
        assert_eq!(reason, "take_profit_gap");
    }

    #[test]
    fn check_sltp_none() {
        let bar = OhlcBar {
            timestamp: Utc::now(),
            open: 100.0, high: 101.0, low: 99.0, close: 100.5, volume: 1000.0,
        };
        let pos = make_position();
        assert!(pos.check_sltp(&bar).is_none());
    }

}
