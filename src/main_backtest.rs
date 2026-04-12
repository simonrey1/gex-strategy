use anyhow::Result;
use clap::Parser;

use gex_strategy::backtest::dashboard::serve_dashboard;
use gex_strategy::backtest::results::ScanEntryRates;
use gex_strategy::backtest::runner::run_portfolio_backtest;
use gex_strategy::config::strategy::{BacktestConfig, StrategyConfig, StrategyOverrides};
use gex_strategy::config::tickers::Ticker;
use gex_strategy::config::{DEFAULT_END, DEFAULT_START};

#[derive(Parser)]
struct SweepHelper {
    #[command(flatten)]
    strategy: StrategyOverrides,
}

fn parse_sweep_specs(spec: &str) -> Result<Vec<(String, Vec<f64>)>> {
    spec.split_whitespace()
        .map(|part| {
            let (param, vals) = part.split_once('=')
                .ok_or_else(|| anyhow::anyhow!("Sweep format: param=v1,v2,v3 [param2=v4,v5]"))?;
            let values: Vec<f64> = vals.split(',')
                .map(|s| s.trim().parse::<f64>())
                .collect::<std::result::Result<_, _>>()?;
            let flag = format!("--{}", param.replace('_', "-"));
            SweepHelper::try_parse_from(["sweep", &format!("{}=0", flag)])
                .map_err(|_| anyhow::anyhow!("Unknown sweep param: {param}"))?;
            Ok((param.to_string(), values))
        })
        .collect()
}

fn build_sweep_grid(specs: &[(String, Vec<f64>)]) -> Vec<Vec<(String, f64)>> {
    let mut grid = vec![vec![]];
    for (param, values) in specs {
        let mut next = Vec::with_capacity(grid.len() * values.len());
        for row in &grid {
            for &val in values {
                let mut r = row.clone();
                r.push((param.clone(), val));
                next.push(r);
            }
        }
        grid = next;
    }
    grid
}

#[derive(Parser)]
#[command(name = "backtest", about = "Run GEX strategy backtest")]
struct Args {
    /// Ticker to backtest (omit for all)
    #[arg(short, long)]
    ticker: Option<String>,

    /// Start date (YYYY-MM-DD)
    #[arg(short, long, default_value = DEFAULT_START)]
    start: String,

    /// End date (YYYY-MM-DD)
    #[arg(short, long, default_value = DEFAULT_END)]
    end: String,

    /// Port for dashboard server (0 to skip)
    #[arg(short, long, default_value = "0")]
    port: u16,

    /// Write state JSON files (for external analysis) without starting a dashboard server
    #[arg(long)]
    save_json: bool,

    /// Generate parameter robustness report (potential_overfits.md)
    #[arg(long)]
    overfit_report: bool,

    /// Sweep a param: "param=v1,v2,v3" — runs one backtest per value in-process (shares cache)
    #[arg(long)]
    sweep: Option<String>,

    /// Max concurrent open positions (default: min(4, #tickers))
    #[arg(long)]
    max_positions: Option<u32>,

    /// Slippage ticks
    #[arg(long)]
    slippage_ticks: Option<f64>,

    /// Starting capital in USD
    #[arg(long)]
    starting_capital: Option<f64>,

    /// Analyze best entries missed by VF gates (dashboard tab + CLI report)
    #[arg(long)]
    missed_entries: bool,

    /// Export scan data as CSV for ML analysis (data/results/scan_data.csv)
    #[arg(long)]
    export_scan_csv: bool,

    /// Verbosity: 0=sweep line, 1=trades+summary, 2=+OPEN/CLOSE/walls, 3=+signals
    #[arg(short, long, default_value = "1")]
    verbosity: u8,

    #[command(flatten)]
    strategy: StrategyOverrides,
}

#[tokio::main]
async fn main() -> Result<()> {
    gex_strategy::data::paths::ensure_backtest_cache();
    let args = Args::parse();

    let tickers: Vec<Ticker> = if let Some(ref t) = args.ticker {
        let mut v = Vec::new();
        for part in t.split(',') {
            let part = part.trim();
            match Ticker::from_str_opt(part) {
                Some(ticker) => v.push(ticker),
                None => {
                    eprintln!("Unknown ticker: {}. Available: {:?}", part, Ticker::ALL);
                    std::process::exit(1);
                }
            }
        }
        v
    } else {
        Ticker::STRATEGY.to_vec()
    };

    gex_strategy::data::bin_cache::ensure_binary_cache(&tickers);
    gex_strategy::data::hist::preload_all(&tickers);

    let mut strategy_config = StrategyConfig::default();
    let mut backtest_config = BacktestConfig::default();

    args.strategy.apply(&mut strategy_config);
    if let Some(v) = args.max_positions { strategy_config.max_open_positions = v; }
    if let Some(v) = args.slippage_ticks { backtest_config.slippage_ticks = v; }
    if let Some(v) = args.starting_capital { backtest_config.starting_capital = v; }

    let verb: u8 = args.verbosity.min(3);

    if args.overfit_report {
        gex_strategy::backtest::overfit_report::run_overfit_report(
            &tickers, &args.end, &strategy_config, &backtest_config,
        )
        .await?;
        return Ok(());
    }

    if let Some(ref sweep_spec) = args.sweep {
        let specs = parse_sweep_specs(sweep_spec)?;
        let grid = build_sweep_grid(&specs);
        let is_1d = specs.len() == 1;

        let configs: Vec<(Vec<(String, f64)>, StrategyConfig)> = grid.into_iter().map(|combo| {
            let mut cfg = strategy_config.clone();
            for (param, val) in &combo {
                let flag = format!("--{}", param.replace('_', "-"));
                let arg = format!("{}={}", flag, val);
                let helper = SweepHelper::try_parse_from(["sweep", &arg]).unwrap();
                helper.strategy.apply(&mut cfg);
            }
            (combo, cfg)
        }).collect();

        let mut results: Vec<(Vec<(String, f64)>, gex_strategy::backtest::metrics::BacktestResult, ScanEntryRates)> =
            Vec::with_capacity(configs.len());

        let sweep_scan = args.missed_entries;
        // First run warms cache
        let (first_combo, first_cfg) = &configs[0];
        let r = run_portfolio_backtest(
            &tickers, &args.start, &args.end, first_cfg, &backtest_config, 0, false, sweep_scan,
        ).await?;
        results.push((first_combo.clone(), r.portfolio_result, r.scan_rates));

        if configs.len() > 1 {
            let mut tasks = tokio::task::JoinSet::new();
            for (combo, cfg) in configs[1..].iter().cloned() {
                let t = tickers.clone();
                let s = args.start.clone();
                let e = args.end.clone();
                let bc = backtest_config.clone();
                let ss = sweep_scan;
                tasks.spawn(async move {
                    let r = run_portfolio_backtest(&t, &s, &e, &cfg, &bc, 0, false, ss).await?;
                    Ok::<_, anyhow::Error>((combo, r.portfolio_result, r.scan_rates))
                });
            }
            while let Some(res) = tasks.join_next().await {
                results.push(res??);
            }
        }

        // Sort by first param, then second, etc.
        results.sort_by(|a, b| {
            for (av, bv) in a.0.iter().zip(b.0.iter()) {
                match av.1.partial_cmp(&bv.1) {
                    Some(std::cmp::Ordering::Equal) | None => continue,
                    Some(ord) => return ord,
                }
            }
            std::cmp::Ordering::Equal
        });

        for (combo, pr, sr) in &results {
            let label = if is_1d {
                format!("{}={:<8}", combo[0].0, format!("{}", combo[0].1))
            } else {
                combo.iter().map(|(p, v)| format!("{}={}", p, v)).collect::<Vec<_>>().join("  ")
            };
            let scan_col = if sweep_scan {
                let true_bw = if sr.worst.total > 0 {
                    sr.best.total as f64 / sr.worst.total as f64
                } else { 0.0 };
                format!(" best={}  mid={}  worst={}  b/w={:.2}",
                    sr.best.total, sr.middle.total, sr.worst.total, true_bw)
            } else {
                String::new()
            };
            println!(
                "{label:<30} trades={:<4} wr={:.1}% net=${:<10.0} mdd={:.1}% sharpe={:.2} sortino={:.2}{scan_col}",
                pr.total_trades, pr.win_rate * 100.0, pr.net_pnl,
                pr.max_drawdown_pct * 100.0, pr.sharpe_ratio, pr.sortino_ratio,
            );
        }
        return Ok(());
    }

    if verb >= 1 {
        println!("[backtest] Tickers: {:?}", tickers);
        println!("[backtest] Period: {} -> {}", args.start, args.end);
    }

    let save_json = args.save_json || args.port > 0;
    let enable_iv_scan = save_json || args.missed_entries;
    let result = run_portfolio_backtest(
        &tickers,
        &args.start,
        &args.end,
        &strategy_config,
        &backtest_config,
        verb,
        save_json,
        enable_iv_scan,
    )
    .await?;

    if verb == 0 {
        let r = &result.portfolio_result;
        let is_full_strategy = tickers.len() == Ticker::STRATEGY.len()
            && tickers.iter().all(|t| Ticker::STRATEGY.contains(t));
        let label = if is_full_strategy {
            String::new()
        } else {
            format!("{} | ", r.label)
        };
        println!(
            "{}trades={} wr={:.1}% net=${:.0} mdd={:.1}% sharpe={:.2} sortino={:.2}",
            label,
            r.total_trades,
            r.win_rate * 100.0,
            r.net_pnl,
            r.max_drawdown_pct * 100.0,
            r.sharpe_ratio,
            r.sortino_ratio,
        );
    }

    if args.missed_entries {
        use gex_strategy::backtest::iv_scan::{ScanBucket, scan_best_gate_failures};
        use gex_strategy::backtest::missed_entries::build_missed_entries_report;
        let best_n = result.iv_scan_results.iter().filter(|r| r.bucket == ScanBucket::Best).count();
        let failures = scan_best_gate_failures(&result.iv_scan_results, &strategy_config);
        if !failures.is_empty() {
            println!("\n  Scan best gate failures ({best_n} entries, avg +{:.1}%):",
                result.iv_scan_results.iter()
                    .filter(|r| r.bucket == ScanBucket::Best)
                    .map(|r| r.profit_pct).sum::<f64>() / best_n.max(1) as f64);
            for f in &failures {
                let sole = if f.sole_count > 0 {
                    format!("  sole={} +{:.0}%", f.sole_count, f.sole_profit_sum)
                } else {
                    String::new()
                };
                println!("    {:>4}/{best_n}  {:<20} (missed +{:.0}% total){sole}",
                    f.count, f.gate, f.profit_sum);
            }
        }

        if let Some(report) = build_missed_entries_report(
            &result.iv_scan_results,
            &result.per_ticker_charts,
            &strategy_config,
        ) {
            let dir = gex_strategy::data::paths::data_dir().join("results");
            std::fs::create_dir_all(&dir).ok();
            let path = dir.join("missed-entries.json");
            if let Ok(json) = serde_json::to_string(&report) {
                std::fs::write(&path, &json).ok();
                println!("  Missed entries report -> {} ({} entries)", path.display(), report.entries.len());
            }
        }
    }

    if args.export_scan_csv {
        let dir = gex_strategy::data::paths::data_dir().join("results");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("scan_data.csv");
        let n = gex_strategy::backtest::iv_scan::export_scan_csv(&result.iv_scan_results, &path)?;
        println!("  Exported {n} scan entries -> {}", path.display());
    }

    if args.port > 0 {
        serve_dashboard(args.port, &tickers).await;
    }

    Ok(())
}
