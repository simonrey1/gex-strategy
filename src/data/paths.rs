use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Verify that the backtest data directory exists. Called once at backtest startup.
pub fn ensure_backtest_cache() {
    let unified = data_dir().join("unified");
    if unified.exists() {
        return;
    }
    eprintln!(
        "Backtest data not found at {}. Run `git lfs pull` to download parquet files.",
        unified.display()
    );
    std::process::exit(1);
}

/// Resolve the project root (where Cargo.toml, data/, dashboard/ live).
pub fn project_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| PathBuf::from("."))
}

pub fn data_dir() -> PathBuf {
    project_root().join("data")
}

/// List tickers that have `latest-{TICKER}.json` in the results directory,
/// sorted alphabetically with "ALL" first. Shared by live and backtest dashboards.
pub fn list_backtest_tickers() -> Vec<String> {
    let results_dir = data_dir().join("results");
    let mut tickers: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&results_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let s = name.to_string_lossy();
            if let Some(t) = s.strip_prefix("latest-").and_then(|r| r.strip_suffix(".json")) {
                tickers.push(t.to_string());
            }
        }
    }
    sort_tickers_all_first(&mut tickers);
    tickers
}

fn sort_tickers_all_first(tickers: &mut Vec<String>) {
    use crate::config::tickers::ALL_TICKERS_SYMBOL;
    tickers.sort_by(|a, b| {
        if a == ALL_TICKERS_SYMBOL { std::cmp::Ordering::Less }
        else if b == ALL_TICKERS_SYMBOL { std::cmp::Ordering::Greater }
        else { a.cmp(b) }
    });
}

/// Read a backtest state JSON file. Shared by live and backtest dashboards.
pub fn read_backtest_json(ticker: Option<&str>) -> Result<serde_json::Value, &'static str> {
    let results_dir = data_dir().join("results");
    let fp = match ticker {
        Some(t) => results_dir.join(format!("latest-{}.json", t)),
        None => results_dir.join("latest.json"),
    };
    if !fp.exists() { return Err("not_found"); }
    let data = std::fs::read_to_string(&fp).map_err(|_| "read_error")?;
    serde_json::from_str(&data).map_err(|_| "parse_error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_places_all_first() {
        let mut v = vec!["GOOG".into(), "ALL".into(), "AAPL".into()];
        sort_tickers_all_first(&mut v);
        assert_eq!(v, vec!["ALL", "AAPL", "GOOG"]);
    }

    #[test]
    fn sort_no_all() {
        let mut v = vec!["MSFT".into(), "AAPL".into()];
        sort_tickers_all_first(&mut v);
        assert_eq!(v, vec!["AAPL", "MSFT"]);
    }

    #[test]
    fn sort_empty() {
        let mut v: Vec<String> = vec![];
        sort_tickers_all_first(&mut v);
        assert!(v.is_empty());
    }
}
