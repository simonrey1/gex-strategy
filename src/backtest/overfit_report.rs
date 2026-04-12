use anyhow::Result;
use std::fmt::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::config::strategy::{BacktestConfig, StrategyConfig};
use crate::config::tickers::Ticker;
use crate::config::DEFAULT_START;

use super::runner::run_portfolio_backtest;

// ── Types ───────────────────────────────────────────────────────────────

struct ParamSweep {
    name: &'static str,
    default_display: String,
    default_val: f64,
    values: Vec<f64>,
    apply: fn(&mut StrategyConfig, f64),
}

#[derive(Debug)]
struct SweepPoint {
    param: String,
    value: f64,
    ticker: Option<Ticker>,
    period: String,
    sharpe: f64,
}

struct BaselinePoint {
    period: String,
    display: String,
    sharpe: f64,
    sortino: f64,
    mdd_pct: f64,
    net: f64,
    trades: usize,
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn is_default(val: f64, default: f64) -> bool {
    (val - default).abs() < 1e-4
}

fn fmt_val(val: f64, default_display: &str) -> String {
    let prec = default_display
        .find('.')
        .map_or(0, |p| default_display.len() - p - 1);
    format!("{:.prec$}", val, prec = prec)
}

fn period_display(label: &str, start: &str, end: &str) -> String {
    format!("{} ({}–{})", label, &start[..4], &end[..4])
}

fn get_commit_hash() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn portfolio_sweep(
    results: &[SweepPoint],
    param: &str,
    period: &str,
) -> Vec<(f64, f64)> {
    results
        .iter()
        .filter(|r| r.ticker.is_none() && r.param == param && r.period == period)
        .map(|r| (r.value, r.sharpe))
        .collect()
}

fn ticker_sweep(results: &[SweepPoint], ticker: Ticker, param: &str) -> Vec<(f64, f64)> {
    results
        .iter()
        .filter(|r| r.ticker == Some(ticker) && r.param == param)
        .map(|r| (r.value, r.sharpe))
        .collect()
}

fn find_peak(entries: &[(f64, f64)]) -> (f64, f64) {
    entries
        .iter()
        .copied()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or((0.0, 0.0))
}

// ── Sweep definitions ──────────────────────────────────────────────────

fn fmt_default(v: f64) -> String {
    if v == v.round() && v.abs() < 1000.0 {
        format!("{}", v as i64)
    } else {
        format!("{v:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn build_sweeps() -> Vec<ParamSweep> {
    let d = StrategyConfig::default();

    // Builds a ParamSweep from a config field.
    // Automatically reads the default from StrategyConfig::default(),
    // inserts it into `values` (sorted) if missing, and formats the display string.
    macro_rules! sw {
        ($field:ident, [$($v:expr),+ $(,)?], $apply:expr) => {{
            let dv = d.$field as f64;
            let mut vals = vec![$($v),+];
            if !vals.iter().any(|v| (*v - dv).abs() < 1e-9) {
                vals.push(dv);
                vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
            }
            ParamSweep {
                name: stringify!($field),
                default_display: fmt_default(dv),
                default_val: dv,
                values: vals,
                apply: $apply,
            }
        }};
    }

    vec![
        sw!(exit_width_atr,          [3.0, 3.5, 4.0, 4.5, 5.0, 5.5, 6.0],              |c, v| c.exit_width_atr = v),
        sw!(sl_tsi_adapt,            [0.0, 1.0, 1.5, 1.625, 1.75, 2.0, 2.5, 3.0, 4.0],        |c, v| c.sl_tsi_adapt = v),
        sw!(hurst_exhaust_threshold, [0.30, 0.35, 0.40, 0.45, 0.50, 0.55, 0.60],        |c, v| c.hurst_exhaust_threshold = v),
        sw!(vf_max_wall_spread_atr,  [5.0, 7.0, 8.0, 10.0, 12.0, 15.0, 20.0],          |c, v| c.vf_max_wall_spread_atr = v),
        sw!(vf_dead_zone_width,      [15.0, 20.0, 25.0, 30.0, 35.0, 40.0],              |c, v| c.vf_dead_zone_width = v),
        sw!(vf_cw_scw_persist_bars,  [2.0, 4.0, 6.0, 8.0, 10.0, 12.0],                  |c, v| c.vf_cw_scw_persist_bars = v as u32),
        sw!(vf_min_pw_spw_atr,      [-4.0, -3.0, -2.0, -1.5, -1.0, -0.5, 0.0],         |c, v| c.vf_min_pw_spw_atr = v),
        sw!(vf_min_atr_pct,         [0.10, 0.15, 0.20, 0.25, 0.30, 0.35, 0.40],         |c, v| c.vf_min_atr_pct = v),
        sw!(vf_max_atr_pct,         [0.0, 0.35, 0.40, 0.45, 0.50, 0.55, 0.60],          |c, v| c.vf_max_atr_pct = v),
        sw!(vf_max_slow_atr_pct,    [0.0, 0.40, 0.45, 0.50, 0.55, 0.60, 0.65],          |c, v| c.vf_max_slow_atr_pct = v),
        sw!(spike_min_atr_pct,      [0.10, 0.20, 0.25, 0.30, 0.35, 0.40, 0.50],         |c, v| c.spike_min_atr_pct = v),
        sw!(iv_spike_mult,          [2.0, 2.5, 3.0, 3.5, 4.0, 5.0],                     |c, v| c.iv_spike_mult = v),
        sw!(vf_compress_tsi_max,    [-5.0, 0.0, 5.0, 10.0, 15.0, 20.0],                 |c, v| c.vf_compress_tsi_max = v),
        sw!(vf_max_gex_norm,         [1.0, 1.5, 1.75, 2.0, 2.25, 2.5, 3.0, 10.0],        |c, v| c.vf_max_gex_norm = v),
        sw!(wall_trail_cushion_atr,  [2.0, 2.5, 3.0, 3.5, 4.0],                          |c, v| c.wall_trail_cushion_atr = v),
        sw!(tp_proximity_trigger,   [0.0, 0.75, 0.80, 0.85, 0.90, 0.95],               |c, v| c.tp_proximity_trigger = v),
        sw!(spread_smooth_halflife,  [10.0, 15.0, 20.0, 25.0, 30.0, 35.0],               |c, v| c.spread_smooth_halflife = v as usize),
        // WB params: only some tickers uses WB but can affect portfolio Sharpe
        sw!(wb_min_zone_score,       [1.0, 1.25, 1.5, 1.75, 2.0],                        |c, v| c.wb_min_zone_score = v),
        sw!(wb_min_wall_spread_atr,  [3.0, 4.0, 5.0, 6.0, 7.0],                          |c, v| c.wb_min_wall_spread_atr = v),
        sw!(wb_tp_spread_mult,       [1.5, 1.75, 2.0, 2.25, 2.5],                        |c, v| c.wb_tp_spread_mult = v),
    ]
}


type Period = (String, String, String);

fn build_periods(end_date: &str) -> Vec<Period> {
    [
        ("FULL", DEFAULT_START, end_date),
        ("H1",   DEFAULT_START, "2022-01-01"),
        ("H2",   "2022-01-01", end_date),
        ("P1",   DEFAULT_START, "2020-01-01"),
        ("P2",   "2020-01-01", "2022-01-01"),
        ("P3",   "2022-01-01", "2024-01-01"),
        ("P4",   "2024-01-01", end_date),
    ]
    .into_iter()
    .map(|(l, s, e)| (l.into(), s.into(), e.into()))
    .collect()
}

// ── Entry point ─────────────────────────────────────────────────────────

pub async fn run_overfit_report(
    tickers: &[Ticker],
    end_date: &str,
    strategy_config: &StrategyConfig,
    backtest_config: &BacktestConfig,
) -> Result<()> {
    let sweeps = build_sweeps();
    let periods = build_periods(end_date);

    eprintln!("[overfit] Running {} baselines...", periods.len());
    let mut baselines = Vec::new();
    for (label, start, end) in &periods {
        match run_portfolio_backtest(
            tickers, start, end, strategy_config, backtest_config, 0, false, false,
        ).await {
            Ok(r) => {
                let pr = &r.portfolio_result;
                baselines.push(BaselinePoint {
                    period: label.clone(),
                    display: period_display(label, start, end),
                    sharpe: pr.sharpe_ratio,
                    sortino: pr.sortino_ratio,
                    mdd_pct: pr.max_drawdown_pct,
                    net: pr.net_pnl,
                    trades: pr.total_trades,
                });
            }
            Err(e) => {
                eprintln!("[overfit] WARN: baseline {} failed: {} — skipping", label, e);
            }
        }
    }

    // 3. Spawn all sweep tasks
    let portfolio_count: usize = sweeps
        .iter()
        .map(|s| {
            s.values
                .iter()
                .filter(|v| !is_default(**v, s.default_val))
                .count()
                * periods.len()
        })
        .sum();
    let wb_params = ["wb_min_zone_score", "wb_min_wall_spread_atr", "wb_tp_spread_mult"];
    let wb_ticker_count = tickers.iter()
        .filter(|t| t.is_wb_enabled())
        .count();
    let wb_sweep_values: usize = sweeps.iter()
        .filter(|s| wb_params.contains(&s.name))
        .map(|s| s.values.len())
        .sum();
    let non_wb_sweep_values: usize = sweeps.iter()
        .filter(|s| !wb_params.contains(&s.name))
        .map(|s| s.values.len())
        .sum();
    let ticker_count: usize =
        tickers.len() * non_wb_sweep_values + wb_ticker_count * wb_sweep_values;
    let total = portfolio_count + ticker_count;
    eprintln!(
        "[overfit] Spawning {} tasks ({} portfolio + {} per-ticker)...",
        total, portfolio_count, ticker_count
    );

    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let parallelism = cores.max(4);
    let sem = Arc::new(Semaphore::new(parallelism));
    let progress = Arc::new(AtomicUsize::new(0));
    let mut tasks: JoinSet<Result<SweepPoint>> = JoinSet::new();

    // Portfolio sweep: skip default (we have baselines)
    for sweep in &sweeps {
        for &val in &sweep.values {
            if is_default(val, sweep.default_val) {
                continue;
            }
            for (plabel, pstart, pend) in &periods {
                let sem = sem.clone();
                let prog = progress.clone();
                let mut cfg = strategy_config.clone();
                (sweep.apply)(&mut cfg, val);
                cfg.validate_and_clamp_quiet(true);
                let bc = backtest_config.clone();
                let tks = tickers.to_vec();
                let param = sweep.name.to_string();
                let plabel = plabel.clone();
                let pstart = pstart.clone();
                let pend = pend.clone();

                tasks.spawn(async move {
                    let _permit = sem.acquire_owned().await.unwrap();
                    let r = run_portfolio_backtest(
                        &tks, &pstart, &pend, &cfg, &bc, 0, false, false,
                    )
                    .await?;
                    prog.fetch_add(1, Ordering::Relaxed);
                    Ok(SweepPoint {
                        param,
                        value: val,
                        ticker: None,
                        period: plabel,
                        sharpe: r.portfolio_result.sharpe_ratio,
                    })
                });
            }
        }
    }

    // Per-ticker sweep: FULL period only, include default values
    for &ticker in tickers {
        let has_wb = ticker.is_wb_enabled();
        for sweep in &sweeps {
            // Skip WB params for tickers without WB enabled
            if !has_wb && wb_params.contains(&sweep.name) {
                continue;
            }
            for &val in &sweep.values {
                let sem = sem.clone();
                let prog = progress.clone();
                let mut cfg = strategy_config.clone();
                (sweep.apply)(&mut cfg, val);
                cfg.validate_and_clamp_quiet(true);
                let bc = backtest_config.clone();
                let param = sweep.name.to_string();
                let end = end_date.to_string();

                tasks.spawn(async move {
                    let _permit = sem.acquire_owned().await.unwrap();
                    let r = run_portfolio_backtest(
                        &[ticker], DEFAULT_START, &end, &cfg, &bc, 0, false, false,
                    )
                    .await?;
                    prog.fetch_add(1, Ordering::Relaxed);
                    Ok(SweepPoint {
                        param,
                        value: val,
                        ticker: Some(ticker),
                        period: "FULL".into(),
                        sharpe: r.portfolio_result.sharpe_ratio,
                    })
                });
            }
        }

    }

    // Collect results with inline progress
    let mut results = Vec::with_capacity(total);
    let mut errors = 0usize;
    let start = std::time::Instant::now();
    let mut last_report = 0usize;
    while let Some(res) = tasks.join_next().await {
        match res {
            Ok(Ok(point)) => results.push(point),
            Ok(Err(_)) => errors += 1,
            Err(_) => errors += 1,
        }
        let done = results.len() + errors;
        if done >= last_report + 50 || done == total {
            let elapsed = start.elapsed().as_secs_f64();
            let rate = done as f64 / elapsed;
            let eta = if rate > 0.0 { (total - done) as f64 / rate } else { 0.0 };
            eprintln!(
                "[overfit] [{}/{}] {:.0}%  {:.1}/s  ETA {:.0}s",
                done, total, done as f64 / total as f64 * 100.0, rate, eta
            );
            last_report = done;
        }
    }
    eprintln!(
        "[overfit] Sweep complete: {} results, {} errors.",
        results.len(),
        errors
    );

    // 4. Generate report
    let md = format_report(&baselines, &results, &sweeps, tickers);
    std::fs::write("potential_overfits.md", &md)?;
    eprintln!("[overfit] Written potential_overfits.md ({} bytes)", md.len());

    Ok(())
}

// ── Report generation ───────────────────────────────────────────────────

fn format_report(
    baselines: &[BaselinePoint],
    results: &[SweepPoint],
    sweeps: &[ParamSweep],
    tickers: &[Ticker],
) -> String {
    let mut md = String::with_capacity(64_000);
    let commit = get_commit_hash();
    let timestamp = chrono_or_fallback();

    writeln!(md, "# Parameter Robustness Report\n").unwrap();
    writeln!(md, "Commit: `{}` | Generated: {}\n", commit, timestamp).unwrap();
    writeln!(md, "Regenerate: `cargo run --release --bin backtest -- --overfit-report`\n").unwrap();

    write_baselines(&mut md, baselines);
    write_half_period(&mut md, baselines, results, sweeps);
    write_4period(&mut md, baselines, results, sweeps);
    write_cross_ticker(&mut md, results, sweeps, tickers);
    write_per_ticker(&mut md, results, sweeps, tickers);

    md
}

fn chrono_or_fallback() -> String {
    std::process::Command::new("date")
        .args(["+%Y-%m-%d %H:%M"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

// ── Baseline ────────────────────────────────────────────────────────────

fn write_baselines(md: &mut String, baselines: &[BaselinePoint]) {
    writeln!(md, "## Baseline\n").unwrap();
    writeln!(md, "| Period | Sharpe | Sortino | MDD | Net | Trades |").unwrap();
    writeln!(md, "| --- | ---: | ---: | ---: | ---: | ---: |").unwrap();
    for b in baselines {
        writeln!(
            md,
            "| {} | {:.2} | {:.2} | {:.1}% | ${:.0} | {} |",
            b.display,
            b.sharpe,
            b.sortino,
            b.mdd_pct * 100.0,
            b.net,
            b.trades
        )
        .unwrap();
    }
    writeln!(md).unwrap();
}

// ── Half-period stability ────────────────────────────────────────────────

fn write_half_period(
    md: &mut String,
    baselines: &[BaselinePoint],
    results: &[SweepPoint],
    sweeps: &[ParamSweep],
) {
    let h1_bl = baselines.iter().find(|b| b.period == "H1").unwrap();
    let h2_bl = baselines.iter().find(|b| b.period == "H2").unwrap();

    writeln!(md, "## Half-Period Parameter Sweep\n").unwrap();
    writeln!(
        md,
        "H1 = 2018–2022, H2 = 2022–2026. Params were tuned on the full period, so this is a stability check, not a true holdout. Each param swept independently.\n"
    )
    .unwrap();
    writeln!(
        md,
        "| Param (default) | H1-opt | H1 Sharpe | H2 Sharpe | Transfer |"
    )
    .unwrap();
    writeln!(md, "| --- | ---: | ---: | ---: | --- |").unwrap();

    for sw in sweeps {
        let h1_entries = portfolio_sweep(results, sw.name, "H1");
        let mut all_h1 = vec![(sw.default_val, h1_bl.sharpe)];
        all_h1.extend(&h1_entries);
        let (h1_opt, h1_sharpe) = find_peak(&all_h1);

        let h2_sharpe = if is_default(h1_opt, sw.default_val) {
            h2_bl.sharpe
        } else {
            portfolio_sweep(results, sw.name, "H2")
                .iter()
                .find(|(v, _)| is_default(*v, h1_opt))
                .map(|(_, s)| *s)
                .unwrap_or(h2_bl.sharpe)
        };

        let h1_delta = h1_sharpe - h1_bl.sharpe;
        let transfer = if is_default(h1_opt, sw.default_val) || h1_delta < 0.05 {
            "Stable"
        } else if h2_sharpe >= h2_bl.sharpe - 0.05 {
            "Good"
        } else {
            "Overfit"
        };

        writeln!(
            md,
            "| `{}` ({}) | {} | {:.2} | {:.2} | {} |",
            sw.name,
            sw.default_display,
            fmt_val(h1_opt, &sw.default_display),
            h1_sharpe,
            h2_sharpe,
            transfer,
        )
        .unwrap();
    }
    writeln!(md).unwrap();
}

// ── 4-Period Stability ──────────────────────────────────────────────────

fn write_4period(
    md: &mut String,
    baselines: &[BaselinePoint],
    results: &[SweepPoint],
    sweeps: &[ParamSweep],
) {
    writeln!(md, "## 4-Period Stability\n").unwrap();
    writeln!(
        md,
        "P1=2018–2020, P2=2020–2022, P3=2022–2024, P4=2024–2026."
    )
    .unwrap();
    writeln!(
        md,
        "Each param swept independently. **Bold** = peak at default. Δ = Sharpe gain over default.\n"
    )
    .unwrap();
    writeln!(
        md,
        "| Param (default) | FULL | P1 | P2 | P3 | P4 | Status |"
    )
    .unwrap();
    writeln!(md, "| --- | --- | --- | --- | --- | --- | --- |").unwrap();

    let period_labels = ["FULL", "P1", "P2", "P3", "P4"];

    for sw in sweeps {
        let mut cells = Vec::new();
        let mut divergences = Vec::new();
        let mut max_delta = 0.0_f64;

        for &plabel in &period_labels {
            let bl_sharpe = baselines
                .iter()
                .find(|b| b.period == plabel)
                .map(|b| b.sharpe)
                .unwrap_or(0.0);

            let entries = portfolio_sweep(results, sw.name, plabel);
            let mut all = vec![(sw.default_val, bl_sharpe)];
            all.extend(&entries);
            let (peak_val, peak_sharpe) = find_peak(&all);
            let delta = peak_sharpe - bl_sharpe;

            if is_default(peak_val, sw.default_val) || delta.abs() < 0.005 {
                cells.push(format!("**{}**", sw.default_display));
            } else {
                cells.push(format!(
                    "{} (Δ+{:.2})",
                    fmt_val(peak_val, &sw.default_display),
                    delta
                ));
                divergences.push(plabel);
                max_delta = max_delta.max(delta);
            }
        }

        let n_divg = divergences.len();
        let status = if divergences.is_empty() {
            "✅ All default".into()
        } else if max_delta < 0.10 {
            format!("✅ {}/{} diverge, max Δ+{:.2}", n_divg, period_labels.len(), max_delta)
        } else {
            format!("⚠️ {}/{} diverge, max Δ+{:.2}", n_divg, period_labels.len(), max_delta)
        };

        writeln!(
            md,
            "| `{}` ({}) | {} | {} |",
            sw.name,
            sw.default_display,
            cells.join(" | "),
            status
        )
        .unwrap();
    }
    writeln!(md).unwrap();
}

// ── Cross-ticker alignment ──────────────────────────────────────────────

fn write_cross_ticker(
    md: &mut String,
    results: &[SweepPoint],
    sweeps: &[ParamSweep],
    tickers: &[Ticker],
) {
    writeln!(md, "## Cross-Ticker Alignment (FULL Period)\n").unwrap();
    writeln!(
        md,
        "Per-ticker solo Sharpe peak vs portfolio default. Sorted by avg Δ (highest first).\n"
    )
    .unwrap();
    writeln!(
        md,
        "| Param (default) | At default | Diverged | Avg Δ | Max Δ | Worst offenders |"
    )
    .unwrap();
    writeln!(md, "| --- | ---: | ---: | ---: | ---: | --- |").unwrap();

    struct ParamRow {
        name: &'static str,
        dd: String,
        at_def: usize,
        diverged: usize,
        avg_delta: f64,
        max_delta: f64,
        offenders: Vec<(Ticker, f64, f64)>,
    }

    let n = tickers.len();
    let mut rows: Vec<ParamRow> = Vec::new();

    for sw in sweeps {
        let mut at_def = 0;
        let mut deltas = Vec::new();
        let mut offenders = Vec::new();

        for &ticker in tickers {
            let entries = ticker_sweep(results, ticker, sw.name);
            if entries.is_empty() {
                continue;
            }
            let (peak_val, peak_sharpe) = find_peak(&entries);
            let def_sharpe = entries
                .iter()
                .find(|(v, _)| is_default(*v, sw.default_val))
                .map(|(_, s)| *s)
                .unwrap_or(peak_sharpe);
            let delta = peak_sharpe - def_sharpe;
            if is_default(peak_val, sw.default_val) || delta.abs() < 0.005 {
                at_def += 1;
            } else {
                deltas.push(delta);
                offenders.push((ticker, peak_val, delta));
            }
        }

        let diverged = deltas.len();
        let avg_delta = if diverged > 0 {
            deltas.iter().sum::<f64>() / diverged as f64
        } else {
            0.0
        };
        let max_delta = deltas.iter().copied().fold(0.0_f64, f64::max);

        rows.push(ParamRow {
            name: sw.name,
            dd: sw.default_display.clone(),
            at_def,
            diverged,
            avg_delta,
            max_delta,
            offenders,
        });
    }

    rows.sort_by(|a, b| {
        b.avg_delta
            .partial_cmp(&a.avg_delta)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.diverged.cmp(&a.diverged))
    });

    for row in &rows {
        let mut off_sorted = row.offenders.clone();
        off_sorted.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        let off_str: String = off_sorted
            .iter()
            .take(4)
            .map(|(t, v, d)| {
                format!(
                    "{}: wants {} (Sharpe +{:.2})",
                    t,
                    fmt_val(*v, &row.dd),
                    d
                )
            })
            .collect::<Vec<_>>()
            .join("<br>");

        writeln!(
            md,
            "| `{}` ({}) | {}/{} | {}/{} | {:.2} | {:.2} | {} |",
            row.name, row.dd, row.at_def, n, row.diverged, n, row.avg_delta, row.max_delta, off_str
        )
        .unwrap();
    }
    writeln!(md).unwrap();
}

// ── Per-ticker tables ───────────────────────────────────────────────────

fn write_per_ticker(
    md: &mut String,
    results: &[SweepPoint],
    sweeps: &[ParamSweep],
    tickers: &[Ticker],
) {
    writeln!(md, "## Per-Ticker Sweep (FULL Period)\n").unwrap();

    for &ticker in tickers {
        writeln!(md, "### {}\n", ticker).unwrap();
        writeln!(md, "| Param (default) | Peak | Sharpe | Δ | Status |").unwrap();
        writeln!(md, "| --- | ---: | ---: | ---: | --- |").unwrap();

        for sw in sweeps {
            let entries = ticker_sweep(results, ticker, sw.name);
            if entries.is_empty() {
                writeln!(md, "| `{}` ({}) | — | — | — | — |", sw.name, sw.default_display)
                    .unwrap();
                continue;
            }
            let (peak_val, peak_sharpe) = find_peak(&entries);
            let def_sharpe = entries
                .iter()
                .find(|(v, _)| is_default(*v, sw.default_val))
                .map(|(_, s)| *s)
                .unwrap_or(peak_sharpe);
            let delta = peak_sharpe - def_sharpe;

            let (peak_display, status) =
                if is_default(peak_val, sw.default_val) || delta.abs() < 0.005 {
                    (format!("**{}**", sw.default_display), "✅".into())
                } else if delta < 0.10 {
                    (
                        fmt_val(peak_val, &sw.default_display),
                        "✅ Δ<0.10".into(),
                    )
                } else {
                    (
                        fmt_val(peak_val, &sw.default_display),
                        "⚠️".to_string(),
                    )
                };

            writeln!(
                md,
                "| `{}` ({}) | {} | {:.2} | {:+.2} | {} |",
                sw.name, sw.default_display, peak_display, peak_sharpe, delta, status
            )
            .unwrap();
        }
        writeln!(md).unwrap();
    }
}
