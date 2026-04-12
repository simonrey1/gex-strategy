use std::collections::HashMap;

use anyhow::Result;

use crate::config::{BarIndex, Ticker};
use crate::types::ToF64;

use super::iv_scan::IvScanResult;
use super::metrics::BacktestResult;
use super::positions::{PositionCloseCtx, Trade};
use super::splits::split_ratio_for_date;
use super::state::{ChartData, EquityPoint};
use super::types::{RecordTradeInputs, TickerState};

/// Per-bucket scan entry stats: (entered, total, wins among entered).
#[derive(Debug, Clone, Default)]
pub struct BucketStats {
    pub entered: usize,
    pub total: usize,
    pub wins: usize,
    /// Sum of (trade_entry - scan_best_entry) / ATR for matched entries.
    pub entry_atr_gap_sum: f64,
    pub entry_atr_gap_count: usize,
}

impl BucketStats {
    pub fn entry_pct(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.entered.to_f64() / self.total.to_f64() * 100.0 }
    }
    pub fn win_pct(&self) -> f64 {
        if self.entered == 0 { 0.0 } else { self.wins.to_f64() / self.entered.to_f64() * 100.0 }
    }
    pub fn avg_entry_atr_gap(&self) -> f64 {
        if self.entry_atr_gap_count == 0 {
            0.0
        } else {
            self.entry_atr_gap_sum / self.entry_atr_gap_count.to_f64()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScanEntryRates {
    pub best: BucketStats,
    pub middle: BucketStats,
    pub worst: BucketStats,
}

pub struct PortfolioResult {
    pub per_ticker: Vec<BacktestResult>,
    pub per_ticker_charts: Vec<(Ticker, ChartData)>,
    pub portfolio_result: BacktestResult,
    pub portfolio_equity_curve: Vec<EquityPoint>,
    pub portfolio_max_drawdown: f64,
    pub portfolio_max_drawdown_pct: f64,
    /// Deduped (best entry per spike) — for scan_rates / dashboard JSON.
    pub iv_scan_results: Vec<IvScanResult>,
    pub all_trades: Vec<Trade>,
    pub scan_rates: ScanEntryRates,
}

impl super::runner::RunnerCtx<'_> {
    /// Validate that every ticker received GEX data, and force-close any open positions.
    pub fn finalize_positions(
        &self,
        states: &mut HashMap<Ticker, TickerState>,
    ) -> Result<()> {
        for &ticker in self.tickers {
            let ts = match states.get_mut(&ticker) {
                Some(ts) => ts,
                None => continue,
            };
            let total = ts.engine.total_bars;
            if total > 0 && ts.gex_bars == 0 {
                anyhow::bail!(
                    "{}: {} bars processed but 0 matched GEX data. \
                     Check data/unified/{} for missing or stale GEX cache, \
                     or a missing split/spinoff in splits.rs.",
                    ticker, total, ticker
                );
            }
            if ts.gex_bars > 0 && self.verbosity >= 2 {
                println!("[backtest-{}] {} bars with GEX data", ticker, ts.gex_bars);
            }
            if let (Some(pos), Some(bar)) = (&ts.position, &ts.last_bar) {
                let (mut trade, _returned) = pos.close(&PositionCloseCtx {
                    exit_time: bar.timestamp,
                    raw_exit_price: bar.close,
                    exit_reason: "end_of_backtest",
                    ticker,
                    bc: self.bc,
                });
                trade.diagnostics = ts.position_diag.as_ref().map(|d| d.finalize(None, pos.entry_price));
                if self.verbosity >= 2 {
                    println!(
                        "[backtest-{}] CLOSE end_of_backtest @ ${:.2} | pnl=${:.2}",
                        ticker, trade.exit_price, trade.net_pnl
                    );
                }
                let bar_time_sec = bar.timestamp.timestamp();
                ts.record_trade(RecordTradeInputs { trade, bar_time_sec, starting_capital: self.starting_capital });
                ts.position = None;
            }
        }
        Ok(())
    }

    /// Collect per-ticker metrics, compute buy-hold returns, finalize charts,
    /// and build the unified portfolio result.
    pub fn collect_results(
        &self,
        states: &mut HashMap<Ticker, TickerState>,
        portfolio_equity_curve: Vec<EquityPoint>,
        max_drawdown: f64,
        max_drawdown_pct: f64,
        avg_capital_util_pct: f64,
    ) -> Result<PortfolioResult> {
    let mut per_ticker_results: Vec<BacktestResult> = Vec::new();
    let mut per_ticker_charts: Vec<(Ticker, ChartData)> = Vec::new();
    let mut all_trades: Vec<Trade> = Vec::new();
    let mut all_iv_scan: Vec<IvScanResult> = Vec::new();
    let total_bars: u64 = states.values().map(|ts| ts.engine.total_bars).max().unwrap_or(0);

    for &ticker in self.tickers {
        let ts = match states.remove(&ticker) {
            Some(ts) => ts,
            None => continue,
        };

        let mut result = BacktestResult::from_ticker(
            ticker, self.start_date, self.end_date, ts.engine.total_bars, &ts.trades,
            &ts.equity_timeline, ts.max_drawdown, ts.max_drawdown_pct, self.starting_capital,
        );
        result.wall_events = vec![];

        if let (Some((first_time, first_close)), Some(last_bar)) = (ts.first_trading_bar, &ts.last_bar) {
            let to_naive = |ts: i64| chrono::DateTime::from_timestamp(ts, 0)
                .map(|dt| dt.format(crate::types::DATE_FMT).to_string())
                .and_then(|s| chrono::NaiveDate::parse_from_str(&s, crate::types::DATE_FMT).ok());
            if let (Some(fd), Some(ld)) = (to_naive(first_time), to_naive(last_bar.timestamp.timestamp())) {
                let fc = first_close / split_ratio_for_date(ticker.as_str(), fd);
                let lc = last_bar.close / split_ratio_for_date(ticker.as_str(), ld);
                if fc > 0.0 {
                    result.buy_hold_return_pct = ((lc - fc) / fc) * 100.0;
                    result.alpha_pct = result.total_return_pct - result.buy_hold_return_pct;
                }
            }
        }

        let iv_scan_results = ts.iv_scan_results;
        all_iv_scan.extend(iv_scan_results.iter().cloned());
        let mut chart = ts.chart_data;
        chart.finalize();

        all_trades.extend(ts.trades);

        if self.save_json {
            let state_path = result.save_state(Some(&chart), &iv_scan_results)?;
            if self.verbosity >= 1 {
                println!("  State -> {}", state_path);
            }
        }
        if self.verbosity >= 2 {
            result.print_summary(self.starting_capital, self.verbosity, self.interval);
        }
        per_ticker_charts.push((ticker, chart));
        per_ticker_results.push(result);
    }

    // ── Unified portfolio result ─────────────────────────────────────────
    all_trades.sort_by(|a, b| a.entry_time.cmp(&b.entry_time));

    let is_multi = self.tickers.len() > 1;
    let label = if is_multi {
        self.tickers.iter().map(|t| t.as_str()).collect::<Vec<_>>().join(",")
    } else {
        self.tickers[0].as_str().to_string()
    };

    let portfolio_result = if is_multi {
        let mut r = BacktestResult::from_portfolio(
            &label, self.start_date, self.end_date, total_bars, &all_trades,
            &portfolio_equity_curve, self.starting_capital,
        );
        let n = per_ticker_results.len().to_f64();
        if n > 0.0 {
            r.buy_hold_return_pct = per_ticker_results.iter()
                .map(|t| t.buy_hold_return_pct)
                .sum::<f64>() / n;
            r.alpha_pct = r.total_return_pct - r.buy_hold_return_pct;
        }
        r.avg_capital_util_pct = avg_capital_util_pct;
        r
    } else {
        let mut r = per_ticker_results[0].clone();
        r.label = label.clone();
        r.avg_capital_util_pct = avg_capital_util_pct;
        r
    };

    let trade_by_spike: HashMap<(Ticker, BarIndex), (f64, f64)> = {
        let mut m: HashMap<(Ticker, BarIndex), (f64, f64)> = HashMap::new();
        for t in &all_trades {
            let key = (t.ticker, t.spike_bar);
            let cur = m.entry(key).or_insert((f64::NEG_INFINITY, 0.0));
            if t.return_pct > cur.0 { *cur = (t.return_pct, t.entry_price); }
        }
        m
    };
    let mut scan_rates = ScanEntryRates::default();
    for s in &all_iv_scan {
        let matched = s.spike_bars.iter()
            .filter_map(|sb| trade_by_spike.get(&(s.ticker, *sb)))
            .copied()
            .reduce(|a, b| if a.0 > b.0 { a } else { b });
        let bucket = match s.bucket {
            super::iv_scan::ScanBucket::Best => &mut scan_rates.best,
            super::iv_scan::ScanBucket::Middle => &mut scan_rates.middle,
            super::iv_scan::ScanBucket::Worst => &mut scan_rates.worst,
        };
        bucket.total += 1;
        if let Some((ret, trade_price)) = matched {
            bucket.entered += 1;
            if ret > 0.0 { bucket.wins += 1; }
            if s.atr > 0.0 {
                bucket.entry_atr_gap_sum += (trade_price - s.entry_price) / s.atr;
                bucket.entry_atr_gap_count += 1;
            }
        }
    }

    if self.verbosity >= 1 {
        portfolio_result.print_summary(self.starting_capital, self.verbosity, self.interval);
        if scan_rates.best.total > 0 || scan_rates.worst.total > 0 {
            println!(
                "  Scan entry: Best {}/{}={:.0}% (wr {:.0}%, gap {:.1}ATR) | Worst {}/{}={:.0}% (wr {:.0}%)",
                scan_rates.best.entered, scan_rates.best.total, scan_rates.best.entry_pct(), scan_rates.best.win_pct(), scan_rates.best.avg_entry_atr_gap(),
                scan_rates.worst.entered, scan_rates.worst.total, scan_rates.worst.entry_pct(), scan_rates.worst.win_pct(),
            );
            // Spike → entry timing (bars_since_spike from entry_snapshot)
            let mut best_bars: Vec<i64> = Vec::new();
            let mut worst_bars: Vec<i64> = Vec::new();
            for s in &all_iv_scan {
                let b = s.entry_snapshot.gate.bars_since_spike;
                match s.bucket {
                    super::iv_scan::ScanBucket::Best | super::iv_scan::ScanBucket::Middle => best_bars.push(b),
                    super::iv_scan::ScanBucket::Worst => worst_bars.push(b),
                }
            }
            let avg = |v: &[i64]| if v.is_empty() { 0.0 } else { v.iter().sum::<i64>() as f64 / v.len() as f64 };
            let median = |v: &mut Vec<i64>| -> f64 {
                if v.is_empty() { return 0.0; }
                v.sort_unstable();
                let mid = v.len() / 2;
                if v.len() % 2 == 0 { (v[mid - 1] + v[mid]) as f64 / 2.0 } else { v[mid] as f64 }
            };
            let p25 = |v: &mut Vec<i64>| -> f64 {
                if v.is_empty() { return 0.0; }
                v.sort_unstable();
                v[v.len() / 4] as f64
            };
            let p75 = |v: &mut Vec<i64>| -> f64 {
                if v.is_empty() { return 0.0; }
                v.sort_unstable();
                v[v.len() * 3 / 4] as f64
            };
            println!(
                "  Spike→entry: Best avg={:.1} p25/med/p75={:.0}/{:.0}/{:.0} | Worst avg={:.1} p25/med/p75={:.0}/{:.0}/{:.0}  (bars × 15min)",
                avg(&best_bars), p25(&mut best_bars), median(&mut best_bars), p75(&mut best_bars),
                avg(&worst_bars), p25(&mut worst_bars), median(&mut worst_bars), p75(&mut worst_bars),
            );
        }
    }

    if self.save_json && is_multi {
        use super::state::SavedState;
        let state = SavedState::new(&portfolio_result, None, &all_iv_scan);
        if let Ok(path) = state.write("ALL") {
            if self.verbosity >= 1 {
                println!("  State -> {}", path);
            }
        }
    }

    Ok(PortfolioResult {
        per_ticker: per_ticker_results,
        per_ticker_charts,
        portfolio_result,
        portfolio_equity_curve,
        portfolio_max_drawdown: max_drawdown,
        portfolio_max_drawdown_pct: max_drawdown_pct,
        iv_scan_results: all_iv_scan,
        all_trades,
        scan_rates,
    })
}
}
