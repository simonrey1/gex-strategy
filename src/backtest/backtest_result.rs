use chrono::{Datelike, DateTime};
use ts_rs::TS;

use crate::backtest::et_format::EtFormat;
use crate::backtest::monthly_return::MonthlyReturn;
use crate::config::{Minutes, Ticker};
use crate::types::{AsLenU32, F64Trunc, ToF64};

use crate::backtest::positions::{Trade, WallEvent};

#[derive(Debug, Clone, serde::Serialize, TS)]
#[ts(export, export_to = "shared/generated/")]
pub struct BacktestResult {
    pub ticker: Ticker,
    /// Display label (e.g. "GOOG,AAPL" for portfolio).
    pub label: String,
    #[serde(rename = "startDate")]
    pub start_date: String,
    #[serde(rename = "endDate")]
    pub end_date: String,
    #[serde(rename = "totalBars")]
    pub total_bars: u64,
    #[serde(rename = "totalTrades")]
    pub total_trades: usize,
    pub winners: usize,
    pub losers: usize,
    #[serde(rename = "winRate")]
    pub win_rate: f64,
    #[serde(rename = "grossPnl")]
    pub gross_pnl: f64,
    #[serde(rename = "totalCommission")]
    pub total_commission: f64,
    #[serde(rename = "totalSlippage")]
    pub total_slippage: f64,
    #[serde(rename = "netPnl")]
    pub net_pnl: f64,
    #[serde(rename = "totalReturnPct")]
    pub total_return_pct: f64,
    #[serde(rename = "profitFactor")]
    pub profit_factor: Option<f64>,
    #[serde(rename = "maxDrawdown")]
    pub max_drawdown: f64,
    #[serde(rename = "maxDrawdownPct")]
    pub max_drawdown_pct: f64,
    #[serde(rename = "sharpeRatio")]
    pub sharpe_ratio: f64,
    #[serde(rename = "sortinoRatio")]
    pub sortino_ratio: f64,
    #[serde(rename = "calmarRatio")]
    pub calmar_ratio: f64,
    pub cagr: f64,
    pub expectancy: f64,
    #[serde(rename = "payoffRatio")]
    pub payoff_ratio: Option<f64>,
    #[serde(rename = "ulcerIndex")]
    pub ulcer_index: f64,
    #[serde(rename = "maxDdDurationDays")]
    pub max_dd_duration_days: u32,
    #[serde(rename = "avgTradeDurationMinutes")]
    pub avg_trade_duration_minutes: f64,
    #[serde(rename = "avgWinPct")]
    pub avg_win_pct: f64,
    #[serde(rename = "avgLossPct")]
    pub avg_loss_pct: f64,
    #[serde(rename = "avgTradePct")]
    pub avg_trade_pct: f64,
    #[serde(rename = "buyHoldReturnPct")]
    pub buy_hold_return_pct: f64,
    #[serde(rename = "alphaPct")]
    pub alpha_pct: f64,
    #[serde(rename = "avgCapitalUtilPct")]
    pub avg_capital_util_pct: f64,
    #[serde(rename = "monthlyReturns")]
    pub monthly_returns: Vec<MonthlyReturn>,
    pub trades: Vec<Trade>,
    #[serde(rename = "tradeAnalysis")]
    pub trade_analysis: crate::backtest::trade_analysis::TradeAnalysis,
    #[serde(rename = "wallEvents")]
    pub wall_events: Vec<WallEvent>,
}

impl BacktestResult {
    pub fn from_portfolio(
        label: &str,
        start_date: &str,
        end_date: &str,
        total_bars: u64,
        trades: &[Trade],
        equity_curve: &[crate::backtest::state::EquityPoint],
        starting_capital: f64,
    ) -> Self {
        let equity_timeline: Vec<(i64, f64)> = equity_curve.iter().map(|e| (e.time, e.value)).collect();

        let mut peak = starting_capital;
        let mut max_dd: f64 = 0.0;
        let mut max_dd_pct: f64 = 0.0;
        for ep in equity_curve {
            if ep.value > peak { peak = ep.value; }
            let dd = peak - ep.value;
            let dd_pct = if peak > 0.0 { dd / peak } else { 0.0 };
            if dd > max_dd { max_dd = dd; }
            if dd_pct > max_dd_pct { max_dd_pct = dd_pct; }
        }

        let mut r = compute_metrics_inner(
            label, start_date, end_date, total_bars, trades, &equity_timeline,
            max_dd, max_dd_pct, starting_capital,
        );
        r.buy_hold_return_pct = 0.0;
        r.alpha_pct = 0.0;
        r
    }

    pub fn from_ticker(
        ticker: Ticker,
        start_date: &str,
        end_date: &str,
        total_bars: u64,
        trades: &[Trade],
        equity_timeline: &[(i64, f64)],
        max_drawdown: f64,
        max_drawdown_pct: f64,
        starting_capital: f64,
    ) -> Self {
        compute_metrics_inner(
            ticker.as_str(), start_date, end_date, total_bars, trades,
            equity_timeline, max_drawdown, max_drawdown_pct, starting_capital,
        )
    }
}

fn compute_metrics_inner(
    label: &str,
    start_date: &str,
    end_date: &str,
    total_bars: u64,
    trades: &[Trade],
    equity_timeline: &[(i64, f64)],
    max_drawdown: f64,
    max_drawdown_pct: f64,
    starting_capital: f64,
) -> BacktestResult {
    let winners: Vec<&Trade> = trades.iter().filter(|t| t.net_pnl > 0.0).collect();
    let losers: Vec<&Trade> = trades.iter().filter(|t| t.net_pnl <= 0.0).collect();
    let n_trades = trades.len();
    let n_trades_f = n_trades.to_f64();

    let gross_pnl: f64 = trades.iter().map(|t| t.gross_pnl).sum();
    let total_commission: f64 = trades.iter().map(|t| t.commission).sum();
    let total_slippage: f64 = trades.iter().map(|t| t.slippage).sum();
    let net_pnl: f64 = trades.iter().map(|t| t.net_pnl).sum();
    let total_return_pct = if starting_capital > 0.0 {
        (net_pnl / starting_capital) * 100.0
    } else {
        0.0
    };

    let gross_wins: f64 = winners.iter().map(|t| t.gross_pnl).sum();
    let gross_losses: f64 = losers.iter().map(|t| t.gross_pnl).sum::<f64>().abs();
    let profit_factor: Option<f64> = if gross_losses > 0.0 {
        Some(gross_wins / gross_losses)
    } else if gross_wins > 0.0 {
        None // infinite
    } else {
        Some(0.0)
    };

    let nw = winners.len();
    let nl = losers.len();
    let nw_f = nw.to_f64();
    let nl_f = nl.to_f64();
    let avg_win_pct = if nw > 0 {
        winners.iter().map(|t| t.return_pct).sum::<f64>() / nw_f
    } else {
        0.0
    };
    let avg_loss_pct = if nl > 0 {
        losers.iter().map(|t| t.return_pct).sum::<f64>() / nl_f
    } else {
        0.0
    };
    let avg_trade_pct = if n_trades > 0 {
        trades.iter().map(|t| t.return_pct).sum::<f64>() / n_trades_f
    } else {
        0.0
    };

    // Daily-return Sharpe/Sortino: mean(daily_ret) / std(daily_ret) * sqrt(252)
    //
    // equity_timeline only has entries on trade-close events, so we must
    // forward-fill across every trading day in the backtest range.  Without
    // this, flat days are omitted, inflating mean return and distorting std.
    use std::collections::BTreeMap;
    use chrono::NaiveDate;

    let mut daily_equity: BTreeMap<String, f64> = BTreeMap::new();
    for &(t, v) in equity_timeline {
        if let Some(dt) = chrono::DateTime::from_timestamp(t, 0) {
            let day = dt.format(crate::types::DATE_FMT).to_string();
            daily_equity.insert(day, v);
        }
    }

    // Forward-fill: walk every calendar day from start to end, carrying
    // the last known equity forward through days with no trade events.
    let day_values: Vec<f64> = {
        let start = NaiveDate::parse_from_str(start_date, crate::types::DATE_FMT).ok();
        let end = NaiveDate::parse_from_str(end_date, crate::types::DATE_FMT).ok();

        if let (Some(s), Some(e)) = (start, end) {
            let mut filled: Vec<f64> = Vec::new();
            let mut current = s;
            let mut last_eq = starting_capital;
            while current <= e {
                let weekday = current.weekday();
                if weekday != chrono::Weekday::Sat && weekday != chrono::Weekday::Sun {
                    let key = current.format(crate::types::DATE_FMT).to_string();
                    if let Some(&eq) = daily_equity.get(&key) {
                        last_eq = eq;
                    }
                    filled.push(last_eq);
                }
                current += chrono::Duration::days(1);
            }
            filled
        } else {
            daily_equity.values().copied().collect()
        }
    };
    let n_days = day_values.len();
    let mut daily_returns: Vec<f64> = Vec::with_capacity(n_days);
    for i in 1..n_days {
        if day_values[i - 1] > 0.0 {
            daily_returns.push(day_values[i] / day_values[i - 1] - 1.0);
        }
    }
    let (sharpe_ratio, sortino_ratio) = if daily_returns.len() > 1 {
        let n_dr = daily_returns.len();
        let n_dr_f = n_dr.to_f64();
        let var_denom = n_dr.saturating_sub(1).to_f64();
        let mean = daily_returns.iter().sum::<f64>() / n_dr_f;
        let var = daily_returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / var_denom;
        let std = var.sqrt();
        let sharpe = if std > 0.0 { (mean / std) * (252.0_f64).sqrt() } else { 0.0 };

        let downside_var = daily_returns.iter()
            .filter(|&&r| r < 0.0)
            .map(|r| r.powi(2))
            .sum::<f64>()
            / var_denom;
        let downside_std = downside_var.sqrt();
        let sortino = if downside_std > 0.0 { (mean / downside_std) * (252.0_f64).sqrt() } else { 0.0 };

        (sharpe, sortino)
    } else {
        (0.0, 0.0)
    };

    // ── Extended metrics ────────────────────────────────────────────────

    let win_rate = if n_trades > 0 {
        nw_f / n_trades_f
    } else {
        0.0
    };

    let final_equity = starting_capital + net_pnl;
    // Same f64 as `n_days` when n_days ≥ 1; when 0, use 1.0 for CAGR (matches prior max(1)).
    let n_days_eff = n_days.max(1).to_f64();
    let trading_days = n_days_eff;
    let years = trading_days / 252.0;
    let cagr = if years > 0.01 && starting_capital > 0.0 && final_equity > 0.0 {
        (final_equity / starting_capital).powf(1.0 / years) - 1.0
    } else {
        0.0
    };

    let calmar_ratio = if max_drawdown_pct > 0.0 && cagr > 0.0 {
        cagr / max_drawdown_pct
    } else {
        0.0
    };

    let expectancy = if n_trades > 0 {
        net_pnl / n_trades_f
    } else {
        0.0
    };

    let payoff_ratio = if avg_loss_pct.abs() > 1e-9 && avg_win_pct.abs() > 1e-9 {
        Some(avg_win_pct.abs() / avg_loss_pct.abs())
    } else {
        None
    };

    // Ulcer Index: RMS of percentage drawdown from peak (daily granularity)
    let ulcer_index = if n_days > 1 {
        let mut peak = day_values[0];
        let mut sq_sum = 0.0;
        for &val in &day_values {
            if val > peak { peak = val; }
            let dd_pct = if peak > 0.0 { (peak - val) / peak } else { 0.0 };
            sq_sum += dd_pct * dd_pct;
        }
        (sq_sum / n_days_eff).sqrt()
    } else {
        0.0
    };

    // Max drawdown duration in trading days
    let max_dd_duration_days = if n_days > 1 {
        let mut peak_idx: usize = 0;
        let mut dd_start: usize = 0;
        let mut max_dur: usize = 0;
        for (i, &val) in day_values.iter().enumerate() {
            if val >= day_values[peak_idx] {
                peak_idx = i;
                dd_start = i;
            } else {
                let dur = i - dd_start;
                if dur > max_dur { max_dur = dur; }
            }
        }
        max_dur.as_len_u32()
    } else {
        0
    };

    // Average trade holding duration in minutes
    let avg_trade_duration_minutes = if n_trades > 0 {
        let total_minutes: f64 = trades.iter().filter_map(|t| {
            let entry = DateTime::parse_from_rfc3339(&t.entry_time).ok()?;
            let exit = DateTime::parse_from_rfc3339(&t.exit_time).ok()?;
            Some((exit - entry).num_minutes().to_f64())
        }).sum();
        total_minutes / n_trades_f
    } else {
        0.0
    };

    // Monthly PnL breakdown
    let mut monthly_map: BTreeMap<String, (f64, u32)> = BTreeMap::new();
    for trade in trades {
        if trade.entry_time.len() >= 7 {
            let month = &trade.entry_time[..7];
            let entry = monthly_map.entry(month.to_string()).or_insert((0.0, 0));
            entry.0 += trade.net_pnl;
            entry.1 += 1;
        }
    }
    let monthly_returns: Vec<MonthlyReturn> = monthly_map.into_iter().map(|(month, (pnl, count))| {
        MonthlyReturn {
            month,
            pnl,
            return_pct: if starting_capital > 0.0 { (pnl / starting_capital) * 100.0 } else { 0.0 },
            trades: count,
        }
    }).collect();

    BacktestResult {
        ticker: Ticker::from_str_opt(label).unwrap_or(Ticker::AAPL),
        label: label.to_string(),
        start_date: start_date.to_string(),
        end_date: end_date.to_string(),
        total_bars,
        total_trades: n_trades,
        winners: winners.len(),
        losers: losers.len(),
        win_rate,
        gross_pnl,
        total_commission,
        total_slippage,
        net_pnl,
        total_return_pct,
        profit_factor,
        max_drawdown,
        max_drawdown_pct,
        sharpe_ratio,
        sortino_ratio,
        calmar_ratio,
        cagr,
        expectancy,
        payoff_ratio,
        ulcer_index,
        max_dd_duration_days,
        avg_trade_duration_minutes,
        avg_win_pct,
        avg_loss_pct,
        avg_trade_pct,
        buy_hold_return_pct: 0.0,
        alpha_pct: 0.0,
        avg_capital_util_pct: 0.0,
        monthly_returns,
        trade_analysis: crate::backtest::trade_analysis::TradeAnalysis::from_trades(trades),
        trades: trades.to_vec(),
        wall_events: vec![],
    }
}

impl BacktestResult {
pub fn print_summary(&self, starting_capital: f64, verbosity: u8, bar_interval: Minutes) {
    let label = &self.label;
    if self.total_trades == 0 {
        println!("\n[backtest] === RESULTS: {} ===", label);
        println!("  Trades: 0");
        return;
    }

    // Trade log (v2+)
    if verbosity < 2 {
        // skip to summary
    } else {
    println!("\n  -- Trade Log (all times ET) --");
    let is_portfolio = self.label.contains(',') || self.label == "PORTFOLIO";
    for t in &self.trades {
        let dir = if t.net_pnl >= 0.0 { "+" } else { "-" };
        let ticker_prefix = if is_portfolio { format!("{:<5} ", t.ticker) } else { String::new() };
        let diag_suffix = t.diagnostics.as_ref().map(|d| {
            format!(
                " | sig={} c={:.2} atr={:.2} adx={:.1} tsi={:.1} | {}",
                d.entry.signal_bar_ts, d.entry.signal_bar_close,
                d.entry.entry_atr, d.entry.entry_adx, d.entry.entry_tsi,
                d.entry.entry_reason,
            )
        }).unwrap_or_default();
        println!(
            "  [{}] {}{:15} {} -> {} | ${:.2}->${:.2} | ${:.0} ({}{:.2}%) | {} | runup={:.1}atr bars={}{}",
            dir,
            ticker_prefix,
            t.signal.as_str(),
            EtFormat::from_rfc3339(&t.entry_time),
            EtFormat::from_rfc3339(&t.exit_time),
            t.entry_price,
            t.exit_price,
            t.net_pnl,
            if t.return_pct >= 0.0 { "+" } else { "" },
            t.return_pct,
            t.exit_reason,
            t.max_runup_atr,
            t.bars_held,
            diag_suffix,
        );
    }
    } // end v2+ trade log

    println!("\n[backtest] === RESULTS: {} ===", label);
    println!("  Period:       {} -> {}", self.start_date, self.end_date);
    println!("  Bars:         {} ({}-min)", self.total_bars, bar_interval);
    println!("  Trades:       {}", self.total_trades);
    println!(
        "  Win/Loss:     {}W / {}L  ({:.1}%)",
        self.winners,
        self.losers,
        self.win_rate * 100.0
    );
    println!("  Gross PnL:    ${:.2}", self.gross_pnl);
    let n_trades_f = self.total_trades.to_f64();
    let avg_comm = self.total_commission / n_trades_f;
    println!(
        "  Commission:   -${:.2} (~${:.2}/trade)",
        self.total_commission, avg_comm
    );
    println!("  Slippage:     -${:.2}", self.total_slippage);
    println!("  Net PnL:      ${:.2}", self.net_pnl);
    let sign = |v: f64| if v >= 0.0 { "+" } else { "" };
    let strat_line = format!(
        "Strategy:   {}{:.2}% on ${}",
        sign(self.total_return_pct),
        self.total_return_pct,
        starting_capital.trunc_u64(),
    );
    let has_bnh = self.buy_hold_return_pct.abs() > 0.001 || self.alpha_pct.abs() > 0.001;
    let bnh_line = format!("Buy & Hold: {}{:.2}%", sign(self.buy_hold_return_pct), self.buy_hold_return_pct);
    let alpha_line = format!("Alpha:      {}{:.2}%", sign(self.alpha_pct), self.alpha_pct);
    let lines: Vec<&str> = if has_bnh {
        vec![&strat_line, &bnh_line, &alpha_line]
    } else {
        vec![&strat_line]
    };
    let w = lines.iter().map(|l| l.len()).max().unwrap_or(0) + 4;
    println!("  ┌{}┐", "─".repeat(w));
    for line in &lines {
        println!("  │  {:<width$}│", line, width = w - 2);
    }
    println!("  └{}┘", "─".repeat(w));
    match self.profit_factor {
        Some(pf) => println!("  Profit Factor: {:.2}", pf),
        None => println!("  Profit Factor: inf"),
    }
    println!(
        "  Max Drawdown: ${:.2} ({:.2}%)",
        self.max_drawdown,
        self.max_drawdown_pct * 100.0
    );
    println!("  Sharpe:       {:.2}", self.sharpe_ratio);
    println!("  Sortino:      {:.2}", self.sortino_ratio);
    println!("  Calmar:       {:.2}", self.calmar_ratio);
    println!("  CAGR:         {:.2}%", self.cagr * 100.0);
    println!("  Ulcer Index:  {:.4}", self.ulcer_index);
    println!(
        "  Expectancy:   ${:.2}/trade  |  Payoff: {}",
        self.expectancy,
        self.payoff_ratio.map_or("n/a".to_string(), |p| format!("{:.2}", p)),
    );
    println!(
        "  Max DD Dur:   {} trading days  |  Avg Hold: {:.0} min  |  Avg Capital Util: {:.1}%",
        self.max_dd_duration_days,
        self.avg_trade_duration_minutes,
        self.avg_capital_util_pct * 100.0,
    );
    println!(
        "  Avg Win:      +{:.2}%  |  Avg Loss: {:.2}%  |  Avg Trade: {}{:.2}%",
        self.avg_win_pct,
        self.avg_loss_pct,
        if self.avg_trade_pct >= 0.0 { "+" } else { "" },
        self.avg_trade_pct
    );

    if !self.monthly_returns.is_empty() && verbosity >= 2 {
        println!("\n  -- Monthly Returns --");
        for mr in &self.monthly_returns {
            println!(
                "     {} | {:>3} trades | ${:>8.2} | {}{:.2}%",
                mr.month, mr.trades, mr.pnl,
                if mr.return_pct >= 0.0 { "+" } else { "" },
                mr.return_pct,
            );
        }
    }

    if verbosity >= 2 && !self.wall_events.is_empty() {
        let all_bounces: Vec<_> = self.wall_events.iter()
            .filter(|e| e.event_type == "PUT_BOUNCE" || e.event_type == "PUT_BOUNCE_BLOCKED")
            .collect();
        let entered = all_bounces.iter().filter(|e| e.event_type == "PUT_BOUNCE").count();
        let blocked = all_bounces.iter().filter(|e| e.event_type == "PUT_BOUNCE_BLOCKED").count();
        let breaks = self.wall_events.iter().filter(|e| e.event_type == "PUT_BREAK").count();
        let reached = self.wall_events.iter().filter(|e| e.event_type == "CALL_REACHED").count();
        println!(
            "\n  -- Wall Events: {} bounces ({} entered, {} blocked), {} breaks, {} call reached --",
            all_bounces.len(), entered, blocked, breaks, reached
        );
        for ev in &all_bounces {
            let status = if ev.event_type == "PUT_BOUNCE" {
                "ENTERED".to_string()
            } else {
                ev.blocked.clone()
            };
            println!(
                "     {} {} pw=${:.0} close=${:.1} | {}",
                if ev.event_type == "PUT_BOUNCE" { ">>>" } else { "   " },
                EtFormat::from_rfc3339(&ev.time), ev.wall, ev.close, status
            );
        }
    }
}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Ticker;
    use crate::types::Signal;

    const STARTING_CAPITAL: f64 = 100_000.0;

    fn make_trade(overrides: Option<(f64, f64, f64, f64, f64)>) -> Trade {
        let (gross_pnl, net_pnl, commission, slippage, return_pct) =
            overrides.unwrap_or((200.0, 190.0, 10.0, 4.0, 2.0));
        Trade {
            ticker: Ticker::AAPL,
            signal: Signal::LongVannaFlip,
            entry_time: "2025-02-12T14:30:00+00:00".to_string(),
            entry_price: 100.0,
            exit_time: "2025-02-12T15:00:00+00:00".to_string(),
            exit_price: 102.0,
            shares: 100,
            gross_pnl,
            commission,
            slippage,
            net_pnl,
            return_pct,
            exit_reason: "signal_exit".to_string(),
            max_runup_atr: 0.0,
            bars_held: 0,
            spike_bar: 0,
            diagnostics: None,
        }
    }

    fn equity() -> Vec<(i64, f64)> {
        vec![
            (1739369400, 100_000.0),
            (1739371200, 100_190.0),
        ]
    }

    #[test]
    fn trade_count_and_win_loss() {
        let trades = vec![
            make_trade(Some((100.0, 100.0, 0.0, 0.0, 1.0))),
            make_trade(Some((-50.0, -50.0, 0.0, 0.0, -0.5))),
            make_trade(Some((80.0, 80.0, 0.0, 0.0, 0.8))),
        ];
        let r = BacktestResult::from_ticker(Ticker::AAPL, "2025-02-12", "2025-02-12", 390, &trades, &equity(), 0.0, 0.0, STARTING_CAPITAL);
        assert_eq!(r.total_trades, 3);
        assert_eq!(r.winners, 2);
        assert_eq!(r.losers, 1);
    }

    #[test]
    fn win_rate() {
        let trades = vec![
            make_trade(Some((100.0, 100.0, 0.0, 0.0, 1.0))),
            make_trade(Some((-50.0, -50.0, 0.0, 0.0, -0.5))),
        ];
        let r = BacktestResult::from_ticker(Ticker::AAPL, "2025-02-12", "2025-02-12", 390, &trades, &equity(), 0.0, 0.0, STARTING_CAPITAL);
        assert!((r.win_rate - 0.5).abs() < 1e-4);
    }

    #[test]
    fn profit_factor_computed() {
        let trades = vec![
            make_trade(Some((300.0, 290.0, 5.0, 5.0, 3.0))),
            make_trade(Some((-100.0, -110.0, 5.0, 5.0, -1.0))),
        ];
        let r = BacktestResult::from_ticker(Ticker::AAPL, "2025-02-12", "2025-02-12", 390, &trades, &equity(), 0.0, 0.0, STARTING_CAPITAL);
        assert!((r.profit_factor.unwrap() - 3.0).abs() < 1e-4);
    }

    #[test]
    fn profit_factor_infinity_when_no_losses() {
        let trades = vec![make_trade(Some((300.0, 290.0, 5.0, 5.0, 3.0)))];
        let r = BacktestResult::from_ticker(Ticker::AAPL, "2025-02-12", "2025-02-12", 390, &trades, &equity(), 0.0, 0.0, STARTING_CAPITAL);
        assert!(r.profit_factor.is_none());
    }

    #[test]
    fn zero_trades_handled() {
        let r = BacktestResult::from_ticker(Ticker::AAPL, "2025-02-12", "2025-02-12", 390, &[], &equity(), 0.0, 0.0, STARTING_CAPITAL);
        assert_eq!(r.total_trades, 0);
        assert_eq!(r.win_rate, 0.0);
        assert_eq!(r.profit_factor, Some(0.0));
        assert_eq!(r.sharpe_ratio, 0.0);
        assert_eq!(r.sortino_ratio, 0.0);
        assert_eq!(r.net_pnl, 0.0);
    }

    #[test]
    fn total_return_pct() {
        let trades = vec![make_trade(Some((5010.0, 5000.0, 5.0, 5.0, 5.0)))];
        let r = BacktestResult::from_ticker(Ticker::AAPL, "2025-02-12", "2025-02-12", 390, &trades, &equity(), 0.0, 0.0, STARTING_CAPITAL);
        assert!((r.total_return_pct - 5.0).abs() < 0.1);
    }

    #[test]
    fn sums_commission_and_slippage() {
        let trades = vec![
            make_trade(Some((200.0, 186.0, 10.0, 4.0, 2.0))),
            make_trade(Some((200.0, 179.0, 15.0, 6.0, 2.0))),
        ];
        let r = BacktestResult::from_ticker(Ticker::AAPL, "2025-02-12", "2025-02-12", 390, &trades, &equity(), 0.0, 0.0, STARTING_CAPITAL);
        assert!((r.total_commission - 25.0).abs() < 1e-4);
        assert!((r.total_slippage - 10.0).abs() < 1e-4);
    }

    #[test]
    fn drawdown_passthrough() {
        let r = BacktestResult::from_ticker(Ticker::AAPL, "2025-02-12", "2025-02-14", 1170, &[], &equity(), 500.0, 0.005, STARTING_CAPITAL);
        assert!((r.max_drawdown - 500.0).abs() < 1e-4);
        assert!((r.max_drawdown_pct - 0.005).abs() < 1e-6);
    }
}
