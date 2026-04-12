use chrono::NaiveDate;

use crate::types::OhlcBar;

/// Known stock splits: (ticker, effective_date, ratio).
/// IBKR serves bars adjusted for ALL splits, while ThetaData greeks
/// use the actual historical price. For a given date, multiply IBKR
/// bars by the product of all split ratios that occurred AFTER that date.
///
/// **If the backtest warns about a price mismatch, add the missing split here.**
const KNOWN_SPLITS: &[(&str, &str, f64)] = &[
    // AAPL: 7:1 on 2014-06-09, 4:1 on 2020-08-31
    ("AAPL", "2014-06-09", 7.0),
    ("AAPL", "2020-08-31", 4.0),
    // GOOG/GOOGL: 20:1 on 2022-07-15
    ("GOOG", "2022-07-15", 20.0),
    ("GOOGL", "2022-07-15", 20.0),
    // GE: 1:8 reverse split on 2021-08-02
    ("GE", "2021-08-02", 0.125),
    // WMT: 3:1 on 2024-02-26
    ("WMT", "2024-02-26", 3.0),
];

/// Compute the cumulative split ratio for a ticker on a given date.
/// Returns the product of all split ratios that occurred AFTER `date`,
/// since IBKR bars are adjusted to the latest price while ThetaData
/// greeks use the actual historical underlying price.
pub fn split_ratio_for_date(ticker: &str, date: NaiveDate) -> f64 {
    let mut ratio = 1.0;
    for &(t, split_date_str, r) in KNOWN_SPLITS {
        if t != ticker {
            continue;
        }
        if let Ok(split_date) = NaiveDate::parse_from_str(split_date_str, crate::types::DATE_FMT) {
            if date < split_date {
                ratio *= r;
            }
        }
    }
    ratio
}

pub fn apply_split_adjustment(bars: &mut [OhlcBar], ratio: f64) {
    if (ratio - 1.0).abs() < 0.01 || bars.is_empty() {
        return;
    }
    for bar in bars.iter_mut() {
        bar.open *= ratio;
        bar.high *= ratio;
        bar.low *= ratio;
        bar.close *= ratio;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bar(price: f64) -> OhlcBar {
        OhlcBar {
            timestamp: chrono::Utc::now(),
            open: price,
            high: price + 1.0,
            low: price - 1.0,
            close: price,
            volume: 1000.0,
        }
    }

    #[test]
    fn goog_pre_split() {
        let date = NaiveDate::from_ymd_opt(2020, 1, 22).unwrap();
        assert_eq!(split_ratio_for_date("GOOG", date), 20.0);
    }

    #[test]
    fn goog_post_split() {
        let date = NaiveDate::from_ymd_opt(2023, 1, 1).unwrap();
        assert_eq!(split_ratio_for_date("GOOG", date), 1.0);
    }

    #[test]
    fn aapl_before_both_splits() {
        let date = NaiveDate::from_ymd_opt(2013, 1, 1).unwrap();
        assert_eq!(split_ratio_for_date("AAPL", date), 28.0); // 7 * 4
    }

    #[test]
    fn aapl_between_splits() {
        let date = NaiveDate::from_ymd_opt(2019, 1, 1).unwrap();
        assert_eq!(split_ratio_for_date("AAPL", date), 4.0);
    }

    #[test]
    fn aapl_post_split() {
        let date = NaiveDate::from_ymd_opt(2021, 1, 1).unwrap();
        assert_eq!(split_ratio_for_date("AAPL", date), 1.0);
    }

    #[test]
    fn unknown_ticker_returns_one() {
        let date = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
        assert_eq!(split_ratio_for_date("XYZ", date), 1.0);
    }

    // ── apply_split_adjustment ────────────────────────────────────────

    #[test]
    fn ratio_one_is_noop() {
        let mut bars = vec![make_bar(100.0)];
        let orig_close = bars[0].close;
        apply_split_adjustment(&mut bars, 1.0);
        assert_eq!(bars[0].close, orig_close);
    }

    #[test]
    fn ratio_four_scales_all_fields() {
        let mut bars = vec![make_bar(50.0)];
        apply_split_adjustment(&mut bars, 4.0);
        assert_eq!(bars[0].open, 200.0);
        assert_eq!(bars[0].high, 204.0);
        assert_eq!(bars[0].low, 196.0);
        assert_eq!(bars[0].close, 200.0);
    }

    #[test]
    fn empty_bars_is_noop() {
        let mut bars: Vec<OhlcBar> = vec![];
        apply_split_adjustment(&mut bars, 4.0);
        assert!(bars.is_empty());
    }
}
