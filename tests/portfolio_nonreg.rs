use gex_strategy::backtest::metrics::BacktestResult;
use gex_strategy::backtest::runner::run_portfolio_backtest;
use gex_strategy::config::strategy::{BacktestConfig, StrategyConfig};
use gex_strategy::config::tickers::Ticker;
use gex_strategy::config::{DEFAULT_END, DEFAULT_START};
use serial_test::serial;

const TICKERS: &[Ticker] = Ticker::STRATEGY;

const YEARS: &[(&str, &str)] = &[
    (DEFAULT_START, "2019-01-01"),
    ("2019-01-01", "2020-01-01"),
    ("2020-01-01", "2021-01-01"),
    ("2021-01-01", "2022-01-01"),
    ("2022-01-01", "2023-01-01"),
    ("2023-01-01", "2024-01-01"),
    ("2024-01-01", "2025-01-01"),
    ("2025-01-01", DEFAULT_END),
];

fn project_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

async fn run_with_config(start: &str, end: &str, sc: &StrategyConfig) -> Option<BacktestResult> {
    std::env::set_current_dir(project_root()).expect("chdir");
    let bc = BacktestConfig::default();
    run_portfolio_backtest(TICKERS, start, end, sc, &bc, 0, false, false)
        .await
        .ok()
        .map(|r| r.portfolio_result)
}

async fn run_portfolio(start: &str, end: &str) -> Option<BacktestResult> {
    run_with_config(start, end, &StrategyConfig::default()).await
}

async fn run_single(ticker: Ticker, start: &str, end: &str) -> Option<BacktestResult> {
    std::env::set_current_dir(project_root()).expect("chdir");
    let mut sc = StrategyConfig::default();
    sc.max_open_positions = 1;
    let bc = BacktestConfig::default();
    run_portfolio_backtest(&[ticker], start, end, &sc, &bc, 0, false, false)
        .await
        .ok()
        .map(|r| r.portfolio_result)
}

fn default_max_pos() -> u32 { StrategyConfig::default_max_open_positions() }

fn assert_mdd(r: &BacktestResult, label: &str, max_positions: u32) {
    let limit = if max_positions <= 1 { 28.0 } else { 12.0 };
    let mdd = r.max_drawdown_pct * 100.0;
    assert!(
        mdd <= limit,
        "{label}: MDD {mdd:.2}% exceeds {limit}% hard limit (max_positions={max_positions})",
    );
}

fn period_label(start: &str, end: &str) -> String {
    let s = &start[..4];
    let e_year: u32 = end[..4].parse().unwrap_or(0);
    let s_year: u32 = s.parse().unwrap_or(0);
    if e_year > s_year + 1 || (e_year == s_year + 1 && &end[4..] != "-01-01") {
        format!("{}-{}", s, &end[2..4])
    } else {
        s.to_string()
    }
}

fn fmt(r: &BacktestResult) -> String {
    format!(
        "trades={:<3}  pnl={:>9.1}  sharpe={:>6.2}  sortino={:>6.2}  mdd={:>5.2}%  bnh={:>8.2}%  alpha={:>8.2}%",
        r.total_trades, r.net_pnl, r.sharpe_ratio, r.sortino_ratio, r.max_drawdown_pct * 100.0,
        r.buy_hold_return_pct, r.alpha_pct,
    )
}

// ── Portfolio: full + yearly ─────────────────────────────────────────

#[tokio::test]
#[serial]
async fn nonreg_portfolio() {
    let mut snap = String::new();
    let r = run_portfolio(DEFAULT_START, DEFAULT_END).await.expect("FULL portfolio should succeed");
    assert_mdd(&r, "FULL", default_max_pos());
    snap.push_str(&format!("FULL       {}\n", fmt(&r)));
    for (start, end) in YEARS {
        let r = match run_portfolio(start, end).await {
            Some(r) => r,
            None => { snap.push_str(&format!("{:<5}  SKIPPED (warmup data missing)\n", period_label(start, end))); continue; }
        };
        assert_mdd(&r, start, default_max_pos());
        snap.push_str(&format!("{:<5}  {}\n", period_label(start, end), fmt(&r)));
    }
    insta::assert_snapshot!(snap, @"
    FULL       trades=493  pnl= 122146.0  sharpe=  2.82  sortino=  4.94  mdd= 9.03%  bnh=  235.59%  alpha=  985.87%
    2018   trades=58   pnl=   1392.8  sharpe=  1.29  sortino=  2.03  mdd= 7.63%  bnh=   -6.43%  alpha=   20.36%
    2019   trades=42   pnl=   3435.0  sharpe=  3.47  sortino=  7.28  mdd= 4.75%  bnh=   30.51%  alpha=    3.84%
    2020   trades=57   pnl=   4195.0  sharpe=  2.94  sortino=  5.62  mdd= 4.70%  bnh=   19.31%  alpha=   22.64%
    2021   trades=70   pnl=   4357.3  sharpe=  3.50  sortino=  6.07  mdd= 7.12%  bnh=   26.24%  alpha=   17.33%
    2022   trades=74   pnl=   3093.0  sharpe=  2.02  sortino=  3.18  mdd= 9.01%  bnh=  -15.59%  alpha=   46.52%
    2023   trades=58   pnl=   3368.6  sharpe=  2.89  sortino=  4.97  mdd= 5.98%  bnh=   21.44%  alpha=   12.24%
    2024   trades=60   pnl=   1593.8  sharpe=  1.61  sortino=  2.44  mdd= 7.65%  bnh=   31.76%  alpha=  -15.82%
    2025-26  trades=66   pnl=   5202.1  sharpe=  3.27  sortino=  6.05  mdd= 5.05%  bnh=   26.27%  alpha=   25.75%
    ");
}

// ── Per-ticker: full + yearly ────────────────────────────────────────

#[tokio::test]
#[serial]
async fn nonreg_per_ticker() {
    let mut snap = String::new();
    for ticker in TICKERS {
        let label = ticker.as_str();
        let r = match run_single(*ticker, DEFAULT_START, DEFAULT_END).await {
            Some(r) => r,
            None => { snap.push_str(&format!("{:<4} FULL   SKIPPED (warmup data missing)\n", label)); continue; }
        };
        assert_mdd(&r, &format!("{label} FULL"), 1);
        snap.push_str(&format!("{:<4} FULL   {}\n", label, fmt(&r)));
        for (start, end) in YEARS {
            let r = match run_single(*ticker, start, end).await {
                Some(r) => r,
                None => { snap.push_str(&format!("{:<4} {:<5}  SKIPPED (warmup data missing)\n", label, period_label(start, end))); continue; }
            };
            let pl = period_label(start, end);
            assert_mdd(&r, &format!("{label} {pl}"), 1);
            snap.push_str(&format!("{:<4} {:<5}  {}\n", label, pl, fmt(&r)));
        }
    }
    insta::assert_snapshot!(snap, @"
    AAPL FULL   trades=59   pnl=  16616.8  sharpe=  0.86  sortino=  2.64  mdd=10.90%  bnh=  473.42%  alpha= -307.26%
    AAPL 2018   trades=10   pnl=   -267.8  sharpe= -0.17  sortino= -0.40  mdd= 9.42%  bnh=  -11.77%  alpha=    9.10%
    AAPL 2019   trades=6    pnl=   3151.3  sharpe=  1.64  sortino= 11.22  mdd= 3.54%  bnh=   94.70%  alpha=  -63.19%
    AAPL 2020   trades=5    pnl=   1880.4  sharpe=  1.15  sortino=  5.60  mdd= 4.45%  bnh=   70.13%  alpha=  -51.32%
    AAPL 2021   trades=10   pnl=   3489.2  sharpe=  1.50  sortino=  6.05  mdd= 5.69%  bnh=   37.75%  alpha=   -2.85%
    AAPL 2022   trades=7    pnl=   -379.8  sharpe= -0.45  sortino= -0.66  mdd= 6.99%  bnh=  -27.16%  alpha=   23.37%
    AAPL 2023   trades=5    pnl=   1539.2  sharpe=  1.23  sortino=  6.24  mdd= 1.89%  bnh=   45.61%  alpha=  -30.22%
    AAPL 2024   trades=8    pnl=   -527.9  sharpe= -0.46  sortino= -0.90  mdd= 7.77%  bnh=   37.39%  alpha=  -42.67%
    AAPL 2025-26  trades=8    pnl=   2243.1  sharpe=  1.14  sortino=  2.54  mdd= 8.57%  bnh=   10.61%  alpha=   11.82%
    GOOG FULL   trades=57   pnl=  12311.6  sharpe=  0.85  sortino=  2.86  mdd= 6.48%  bnh=  418.26%  alpha= -295.14%
    GOOG 2018   trades=4    pnl=    334.4  sharpe=  0.76  sortino=  2.02  mdd= 2.33%  bnh=   -9.15%  alpha=   12.50%
    GOOG 2019   trades=4    pnl=   -308.4  sharpe= -1.07  sortino= -1.20  mdd= 3.08%  bnh=   27.10%  alpha=  -30.18%
    GOOG 2020   trades=4    pnl=    102.1  sharpe=  0.21  sortino=  0.52  mdd= 3.76%  bnh=   22.21%  alpha=  -21.19%
    GOOG 2021   trades=9    pnl=   1909.2  sharpe=  1.39  sortino=  7.32  mdd= 2.41%  bnh=   66.73%  alpha=  -47.64%
    GOOG 2022   trades=5    pnl=   -512.6  sharpe= -1.23  sortino= -1.27  mdd= 5.38%  bnh=  -38.50%  alpha=   33.38%
    GOOG 2023   trades=8    pnl=   2003.4  sharpe=  1.54  sortino=  7.22  mdd= 2.97%  bnh=   54.58%  alpha=  -34.55%
    GOOG 2024   trades=9    pnl=   1537.4  sharpe=  1.21  sortino=  3.06  mdd= 7.52%  bnh=   32.31%  alpha=  -16.93%
    GOOG 2025-26  trades=11   pnl=   2442.2  sharpe=  1.06  sortino=  4.03  mdd= 6.47%  bnh=   55.36%  alpha=  -30.94%
    MSFT FULL   trades=46   pnl=   6010.3  sharpe=  0.62  sortino=  1.94  mdd=12.10%  bnh=  307.29%  alpha= -247.18%
    MSFT 2018   trades=5    pnl=   -346.1  sharpe= -1.08  sortino= -1.16  mdd= 3.46%  bnh=   11.77%  alpha=  -15.23%
    MSFT 2019   trades=4    pnl=    316.0  sharpe=  0.64  sortino=  1.47  mdd= 3.08%  bnh=   54.38%  alpha=  -51.22%
    MSFT 2020   trades=5    pnl=    771.0  sharpe=  1.11  sortino=  4.05  mdd= 1.53%  bnh=   36.57%  alpha=  -28.86%
    MSFT 2021   trades=4    pnl=   3472.1  sharpe=  1.91  sortino=  0.00  mdd= 0.00%  bnh=   57.19%  alpha=  -22.47%
    MSFT 2022   trades=7    pnl=   -706.4  sharpe= -1.61  sortino= -1.78  mdd= 7.06%  bnh=  -25.51%  alpha=   18.44%
    MSFT 2023   trades=7    pnl=   2427.5  sharpe=  1.39  sortino=  8.41  mdd= 1.97%  bnh=   59.38%  alpha=  -35.11%
    MSFT 2024   trades=8    pnl=    289.8  sharpe=  0.38  sortino=  0.85  mdd= 6.78%  bnh=    8.08%  alpha=   -5.18%
    MSFT 2025-26  trades=4    pnl=   -342.1  sharpe= -0.77  sortino= -0.90  mdd= 5.39%  bnh=  -11.22%  alpha=    7.80%
    JPM  FULL   trades=162  pnl=   6841.0  sharpe=  0.56  sortino=  1.25  mdd=20.58%  bnh=  161.11%  alpha=  -92.70%
    JPM  2018   trades=24   pnl=   -866.9  sharpe= -0.65  sortino= -1.38  mdd=11.18%  bnh=  -14.22%  alpha=    5.55%
    JPM  2019   trades=21   pnl=   -330.6  sharpe= -0.28  sortino= -0.50  mdd=11.48%  bnh=   38.75%  alpha=  -42.06%
    JPM  2020   trades=14   pnl=   1031.7  sharpe=  0.75  sortino=  2.10  mdd= 5.99%  bnh=   -7.17%  alpha=   17.49%
    JPM  2021   trades=24   pnl=    476.2  sharpe=  0.40  sortino=  0.82  mdd= 7.56%  bnh=   14.13%  alpha=   -9.37%
    JPM  2022   trades=24   pnl=    172.7  sharpe=  0.20  sortino=  0.36  mdd=15.99%  bnh=  -21.41%  alpha=   23.13%
    JPM  2023   trades=22   pnl=   -807.5  sharpe= -0.97  sortino= -1.37  mdd=14.24%  bnh=   23.95%  alpha=  -32.03%
    JPM  2024   trades=11   pnl=   1170.3  sharpe=  1.25  sortino=  4.56  mdd= 3.30%  bnh=   45.28%  alpha=  -33.57%
    JPM  2025-26  trades=22   pnl=   3398.1  sharpe=  1.91  sortino=  5.46  mdd= 4.58%  bnh=   21.55%  alpha=   12.43%
    GS   FULL   trades=45   pnl=   6713.0  sharpe=  0.61  sortino=  2.00  mdd=14.19%  bnh=  229.59%  alpha= -162.46%
    GS   2018   trades=6    pnl=     65.8  sharpe=  0.15  sortino=  0.24  mdd= 2.23%  bnh=  -36.36%  alpha=   37.02%
    GS   2019   trades=4    pnl=   -483.7  sharpe= -1.37  sortino= -1.42  mdd= 4.84%  bnh=   29.84%  alpha=  -34.68%
    GS   2020   trades=7    pnl=   2054.3  sharpe=  1.22  sortino=  5.63  mdd= 5.42%  bnh=    7.52%  alpha=   13.02%
    GS   2021   trades=11   pnl=   1469.1  sharpe=  0.96  sortino=  3.37  mdd= 8.08%  bnh=   27.69%  alpha=  -12.99%
    GS   2022   trades=5    pnl=   -762.2  sharpe= -1.56  sortino= -1.56  mdd= 7.62%  bnh=  -13.63%  alpha=    6.01%
    GS   2023   trades=4    pnl=    834.0  sharpe=  0.65  sortino=  3.44  mdd= 4.38%  bnh=    5.58%  alpha=    2.76%
    GS   2024   trades=5    pnl=    988.6  sharpe=  1.08  sortino=  5.35  mdd= 2.55%  bnh=   51.98%  alpha=  -42.10%
    GS   2025-26  trades=3    pnl=    633.8  sharpe=  0.64  sortino=  3.43  mdd= 1.45%  bnh=   52.46%  alpha=  -46.13%
    WMT  FULL   trades=34   pnl=  12265.2  sharpe=  0.93  sortino=  4.32  mdd= 5.18%  bnh=  267.72%  alpha= -145.07%
    WMT  2018   trades=5    pnl=    315.0  sharpe=  0.32  sortino=  0.99  mdd= 5.09%  bnh=   -8.73%  alpha=   11.88%
    WMT  2019   trades=3    pnl=   2382.3  sharpe=  1.65  sortino=  0.00  mdd= 0.00%  bnh=   24.83%  alpha=   -1.01%
    WMT  2020   trades=5    pnl=   2228.5  sharpe=  1.43  sortino= 11.70  mdd= 2.49%  bnh=   23.58%  alpha=   -1.29%
    WMT  2021   trades=3    pnl=     48.6  sharpe=  0.14  sortino=  0.27  mdd= 2.90%  bnh=   -2.56%  alpha=    3.05%
    WMT  2022   trades=9    pnl=   3022.5  sharpe=  1.66  sortino=  9.28  mdd= 2.34%  bnh=   -1.74%  alpha=   31.96%
    WMT  2023   trades=1    pnl=    346.0  sharpe=  1.00  sortino=  0.00  mdd= 0.00%  bnh=    9.16%  alpha=   -5.70%
    WMT  2024   trades=4    pnl=    729.5  sharpe=  0.73  sortino=  3.55  mdd= 2.53%  bnh=   67.28%  alpha=  -59.98%
    WMT  2025-26  trades=3    pnl=   -520.6  sharpe= -1.50  sortino= -1.49  mdd= 5.21%  bnh=   35.64%  alpha=  -40.84%
    HD   FULL   trades=29   pnl=   5443.4  sharpe=  0.65  sortino=  2.49  mdd= 4.66%  bnh=   66.43%  alpha=  -12.00%
    HD   2018   trades=4    pnl=   1212.1  sharpe=  1.31  sortino=  6.09  mdd= 1.94%  bnh=  -13.19%  alpha=   25.31%
    HD   2019   trades=2    pnl=   -365.5  sharpe= -1.41  sortino= -1.41  mdd= 3.65%  bnh=   22.58%  alpha=  -26.24%
    HD   2020   trades=5    pnl=   1047.5  sharpe=  0.95  sortino=  4.46  mdd= 2.06%  bnh=   18.25%  alpha=   -7.78%
    HD   2021   trades=3    pnl=   2860.1  sharpe=  1.54  sortino=  0.00  mdd= 0.00%  bnh=   48.51%  alpha=  -19.91%
    HD   2022   trades=4    pnl=    377.2  sharpe=  0.70  sortino=  1.74  mdd= 2.97%  bnh=  -19.65%  alpha=   23.42%
    HD   2023   trades=4    pnl=    102.3  sharpe=  0.24  sortino=  0.43  mdd= 1.96%  bnh=    3.98%  alpha=   -2.95%
    HD   2024   trades=0    pnl=     -0.0  sharpe=  0.00  sortino=  0.00  mdd= 0.00%  bnh=    8.67%  alpha=   -8.67%
    HD   2025-26  trades=5    pnl=    -97.7  sharpe= -0.21  sortino= -0.31  mdd= 2.99%  bnh=  -15.83%  alpha=   14.85%
    DIS  FULL   trades=47   pnl=   4889.3  sharpe=  0.42  sortino=  1.43  mdd=15.70%  bnh=  -14.20%  alpha=   63.10%
    DIS  2018   trades=6    pnl=   -241.2  sharpe= -0.55  sortino= -0.83  mdd= 5.40%  bnh=   -3.25%  alpha=    0.84%
    DIS  2019   trades=4    pnl=   1427.2  sharpe=  0.86  sortino=  6.80  mdd= 2.23%  bnh=   28.68%  alpha=  -14.41%
    DIS  2020   trades=8    pnl=   2519.5  sharpe=  1.28  sortino=  7.73  mdd= 3.69%  bnh=   26.33%  alpha=   -1.13%
    DIS  2021   trades=8    pnl=  -1327.7  sharpe= -1.85  sortino= -1.99  mdd=15.71%  bnh=  -11.16%  alpha=   -2.11%
    DIS  2022   trades=7    pnl=    447.0  sharpe=  0.35  sortino=  0.80  mdd= 7.71%  bnh=  -45.92%  alpha=   50.39%
    DIS  2023   trades=6    pnl=    814.9  sharpe=  0.63  sortino=  1.96  mdd= 6.05%  bnh=   -8.23%  alpha=   16.38%
    DIS  2024   trades=2    pnl=   -331.7  sharpe= -1.39  sortino= -1.38  mdd= 3.32%  bnh=   23.62%  alpha=  -26.93%
    DIS  2025-26  trades=6    pnl=    758.6  sharpe=  0.44  sortino=  1.98  mdd= 4.67%  bnh=  -10.81%  alpha=   18.40%
    KO   FULL   trades=27   pnl=   5218.7  sharpe=  0.68  sortino=  2.36  mdd= 4.31%  bnh=   64.24%  alpha=  -12.06%
    KO   2018   trades=3    pnl=   1118.1  sharpe=  1.10  sortino=  7.81  mdd= 1.42%  bnh=    1.77%  alpha=    9.41%
    KO   2019   trades=0    pnl=     -0.0  sharpe=  0.00  sortino=  0.00  mdd= 0.00%  bnh=   16.60%  alpha=  -16.60%
    KO   2020   trades=6    pnl=   1231.9  sharpe=  1.05  sortino=  3.21  mdd= 4.70%  bnh=   -2.35%  alpha=   14.66%
    KO   2021   trades=3    pnl=    455.2  sharpe=  1.69  sortino=  0.00  mdd= 0.00%  bnh=   17.57%  alpha=  -13.02%
    KO   2022   trades=8    pnl=   1454.4  sharpe=  1.06  sortino=  3.93  mdd= 4.06%  bnh=    4.34%  alpha=   10.20%
    KO   2023   trades=4    pnl=    619.3  sharpe=  0.79  sortino=  2.78  mdd= 1.78%  bnh=   -3.07%  alpha=    9.26%
    KO   2024   trades=0    pnl=     -0.0  sharpe=  0.00  sortino=  0.00  mdd= 0.00%  bnh=    2.92%  alpha=   -2.92%
    KO   2025-26  trades=3    pnl=   -433.1  sharpe= -1.52  sortino= -1.51  mdd= 4.33%  bnh=   24.91%  alpha=  -29.24%
    CAT  FULL   trades=47   pnl=  14566.8  sharpe=  0.78  sortino=  2.84  mdd=18.57%  bnh=  325.17%  alpha= -179.51%
    CAT  2018   trades=7    pnl=    164.5  sharpe=  0.20  sortino=  0.55  mdd= 8.45%  bnh=  -26.43%  alpha=   28.08%
    CAT  2019   trades=4    pnl=   1089.5  sharpe=  1.04  sortino=  5.15  mdd= 1.57%  bnh=   12.89%  alpha=   -1.99%
    CAT  2020   trades=1    pnl=   1363.8  sharpe=  1.00  sortino=  0.00  mdd= 0.00%  bnh=   23.64%  alpha=  -10.00%
    CAT  2021   trades=6    pnl=   -516.7  sharpe= -0.91  sortino= -1.25  mdd= 8.72%  bnh=    3.86%  alpha=   -9.02%
    CAT  2022   trades=6    pnl=   5569.3  sharpe=  1.96  sortino= 22.79  mdd= 2.06%  bnh=    6.67%  alpha=   49.03%
    CAT  2023   trades=9    pnl=   1507.3  sharpe=  0.84  sortino=  4.15  mdd= 5.90%  bnh=   15.37%  alpha=   -0.29%
    CAT  2024   trades=6    pnl=     97.0  sharpe=  0.14  sortino=  0.23  mdd=10.54%  bnh=   26.19%  alpha=  -25.22%
    CAT  2025-26  trades=7    pnl=    752.6  sharpe=  0.55  sortino=  2.04  mdd= 4.26%  bnh=  105.94%  alpha=  -98.41%
    MS   FULL   trades=50   pnl=   3044.4  sharpe=  0.37  sortino=  0.93  mdd=18.85%  bnh=  197.35%  alpha= -166.91%
    MS   2018   trades=8    pnl=   -179.5  sharpe= -0.14  sortino= -0.26  mdd= 6.24%  bnh=  -29.66%  alpha=   27.86%
    MS   2019   trades=5    pnl=   1009.8  sharpe=  0.78  sortino=  2.75  mdd= 3.52%  bnh=   22.39%  alpha=  -12.29%
    MS   2020   trades=3    pnl=    628.3  sharpe=  0.92  sortino=  3.44  mdd= 1.82%  bnh=   30.53%  alpha=  -24.25%
    MS   2021   trades=12   pnl=   -448.5  sharpe= -0.50  sortino= -0.79  mdd= 8.75%  bnh=   30.02%  alpha=  -34.51%
    MS   2022   trades=9    pnl=  -1176.4  sharpe= -2.35  sortino= -2.42  mdd=13.04%  bnh=  -18.86%  alpha=    7.10%
    MS   2023   trades=2    pnl=   -481.7  sharpe= -1.41  sortino= -1.40  mdd= 4.82%  bnh=    4.85%  alpha=   -9.67%
    MS   2024   trades=4    pnl=    955.3  sharpe=  0.96  sortino=  4.23  mdd= 1.65%  bnh=   44.56%  alpha=  -35.00%
    MS   2025-26  trades=7    pnl=   1107.6  sharpe=  0.86  sortino=  3.23  mdd= 4.70%  bnh=   34.86%  alpha=  -23.79%
    NRG  FULL   trades=8    pnl=    426.6  sharpe=  0.18  sortino=  0.39  mdd= 2.69%  bnh=  427.34%  alpha= -423.07%
    NRG  2018   trades=1    pnl=   -224.8  sharpe= -1.00  sortino= -1.00  mdd= 2.25%  bnh=   38.78%  alpha=  -41.02%
    NRG  2019   trades=3    pnl=    618.8  sharpe=  1.05  sortino=  4.51  mdd= 1.36%  bnh=   -0.58%  alpha=    6.77%
    NRG  2020   trades=2    pnl=    224.3  sharpe=  0.41  sortino=  0.88  mdd= 2.69%  bnh=   -0.21%  alpha=    2.46%
    NRG  2021   trades=1    pnl=   -132.2  sharpe= -1.00  sortino= -1.00  mdd= 1.32%  bnh=    5.38%  alpha=   -6.70%
    NRG  2022   trades=0    pnl=     -0.0  sharpe=  0.00  sortino=  0.00  mdd= 0.00%  bnh=  -23.68%  alpha=   23.68%
    NRG  2023   trades=1    pnl=    -43.6  sharpe= -1.00  sortino= -1.00  mdd= 0.44%  bnh=   61.77%  alpha=  -62.21%
    NRG  2024   trades=0    pnl=     -0.0  sharpe=  0.00  sortino=  0.00  mdd= 0.00%  bnh=   79.27%  alpha=  -79.27%
    NRG  2025-26  trades=0    pnl=     -0.0  sharpe=  0.00  sortino=  0.00  mdd= 0.00%  bnh=   54.16%  alpha=  -54.16%
    SO   FULL   trades=28   pnl=   5988.4  sharpe=  0.62  sortino=  2.40  mdd= 9.40%  bnh=  114.87%  alpha=  -54.99%
    SO   2018   trades=5    pnl=    645.8  sharpe=  0.85  sortino=  2.55  mdd= 1.98%  bnh=   -3.35%  alpha=    9.81%
    SO   2019   trades=0    pnl=     -0.0  sharpe=  0.00  sortino=  0.00  mdd= 0.00%  bnh=   36.73%  alpha=  -36.73%
    SO   2020   trades=3    pnl=   1828.3  sharpe=  1.11  sortino=  9.66  mdd= 1.85%  bnh=   -4.78%  alpha=   23.06%
    SO   2021   trades=1    pnl=     64.9  sharpe=  1.00  sortino=  0.00  mdd= 0.00%  bnh=   15.80%  alpha=  -15.15%
    SO   2022   trades=8    pnl=   -604.3  sharpe= -0.60  sortino= -1.07  mdd= 9.57%  bnh=    4.59%  alpha=  -10.64%
    SO   2023   trades=5    pnl=    288.0  sharpe=  0.34  sortino=  1.10  mdd= 3.10%  bnh=    0.39%  alpha=    2.49%
    SO   2024   trades=1    pnl=   -166.7  sharpe= -1.00  sortino= -1.00  mdd= 1.67%  bnh=   15.61%  alpha=  -17.28%
    SO   2025-26  trades=4    pnl=   1829.0  sharpe=  1.30  sortino=  9.63  mdd= 1.62%  bnh=   19.41%  alpha=   -1.12%
    MCD  FULL   trades=25   pnl=   4295.9  sharpe=  0.68  sortino=  2.23  mdd= 8.40%  bnh=   76.80%  alpha=  -33.84%
    MCD  2018   trades=7    pnl=   1697.3  sharpe=  2.03  sortino=  9.41  mdd= 1.70%  bnh=    1.81%  alpha=   15.16%
    MCD  2019   trades=0    pnl=     -0.0  sharpe=  0.00  sortino=  0.00  mdd= 0.00%  bnh=    8.77%  alpha=   -8.77%
    MCD  2020   SKIPPED (warmup data missing)
    MCD  2021   trades=1    pnl=     63.7  sharpe=  1.00  sortino=  0.00  mdd= 0.00%  bnh=   27.15%  alpha=  -26.52%
    MCD  2022   trades=7    pnl=   -330.1  sharpe= -0.52  sortino= -0.75  mdd= 6.81%  bnh=    0.14%  alpha=   -3.44%
    MCD  2023   trades=2    pnl=    769.7  sharpe=  0.89  sortino=  8.36  mdd= 0.93%  bnh=   11.22%  alpha=   -3.53%
    MCD  2024   trades=2    pnl=    477.4  sharpe=  0.71  sortino=  2.80  mdd= 1.74%  bnh=   -1.32%  alpha=    6.09%
    MCD  2025-26  trades=4    pnl=     68.0  sharpe=  0.22  sortino=  0.38  mdd= 2.37%  bnh=    9.45%  alpha=   -8.77%
    COST FULL   trades=25   pnl=   4746.2  sharpe=  0.75  sortino=  2.74  mdd= 3.92%  bnh=  418.42%  alpha= -370.96%
    COST 2018   trades=7    pnl=    175.8  sharpe=  0.35  sortino=  0.65  mdd= 4.28%  bnh=    5.54%  alpha=   -3.78%
    COST 2019   trades=2    pnl=   2072.5  sharpe=  1.41  sortino=  0.00  mdd= 0.00%  bnh=   40.03%  alpha=  -19.30%
    COST 2020   trades=4    pnl=   1225.0  sharpe=  1.27  sortino=  6.12  mdd= 1.95%  bnh=   26.13%  alpha=  -13.88%
    COST 2021   trades=0    pnl=     -0.0  sharpe=  0.00  sortino=  0.00  mdd= 0.00%  bnh=   55.61%  alpha=  -55.61%
    COST 2022   trades=4    pnl=    633.6  sharpe=  0.83  sortino=  2.44  mdd= 3.64%  bnh=  -13.48%  alpha=   19.82%
    COST 2023   trades=3    pnl=    745.7  sharpe=  1.10  sortino=  7.86  mdd= 0.94%  bnh=   37.11%  alpha=  -29.65%
    COST 2024   trades=3    pnl=    252.3  sharpe=  0.46  sortino=  1.34  mdd= 2.75%  bnh=   34.51%  alpha=  -31.99%
    COST 2025-26  trades=3    pnl=    106.3  sharpe=  0.29  sortino=  0.71  mdd= 1.00%  bnh=    7.61%  alpha=   -6.55%
    ");
}
