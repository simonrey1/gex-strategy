use chrono::{Datelike, NaiveDate};
use nyse_holiday_cal::HolidayCal;

use crate::config::Ticker;

/// ThetaData returns corrupt/missing options data on these dates (all tickers).
const BROKEN_DATES: &[&str] = &[
    "2022-01-03",
    "2022-01-04",
];

/// Per-ticker dates where ThetaData returns persistent 500s / corrupt data.
pub const TICKER_BAD_DATES: &[(Ticker, &str)] = &[
    (Ticker::COST, "2022-11-03"),
    (Ticker::COST,  "2023-11-01"),
    (Ticker::MCD,  "2023-11-01"),
];

pub fn is_ticker_bad_date(ticker: Ticker, date: &str) -> bool {
    TICKER_BAD_DATES.iter().any(|(t, d)| *t == ticker && *d == date)
}

pub fn get_trading_days(start_date: &str, end_date: &str) -> Vec<String> {
    let start = NaiveDate::parse_from_str(start_date, crate::types::DATE_FMT)
        .unwrap_or_else(|e| panic!("invalid start_date '{}': {}", start_date, e));
    let end = NaiveDate::parse_from_str(end_date, crate::types::DATE_FMT)
        .unwrap_or_else(|e| panic!("invalid end_date '{}': {}", end_date, e));
    let mut days = Vec::new();
    let mut current = start;
    while current <= end {
        let dow = current.weekday().num_days_from_sunday();
        let s = current.format(crate::types::DATE_FMT).to_string();
        // is_busday returns Err for out-of-range years → treat as trading day
        let is_bus = current.is_busday().unwrap_or(dow != 0 && dow != 6);
        if is_bus && !BROKEN_DATES.contains(&s.as_str()) {
            days.push(s);
        }
        current = current.succ_opt().expect("date overflow in trading day iteration");
    }
    days
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trading_days_excludes_weekends() {
        let days = get_trading_days("2025-02-10", "2025-02-16");
        assert_eq!(
            days,
            vec!["2025-02-10", "2025-02-11", "2025-02-12", "2025-02-13", "2025-02-14"]
        );
    }

    #[test]
    fn trading_days_empty_for_weekend_only() {
        let days = get_trading_days("2025-02-15", "2025-02-16");
        assert!(days.is_empty());
    }

    #[test]
    fn trading_days_single_day() {
        let days = get_trading_days("2025-02-10", "2025-02-10");
        assert_eq!(days, vec!["2025-02-10"]);
    }

    #[test]
    fn excludes_good_friday_2026() {
        let days = get_trading_days("2026-03-30", "2026-04-06");
        assert!(!days.contains(&"2026-04-03".to_string()));
        assert!(days.contains(&"2026-04-02".to_string()));
        assert!(days.contains(&"2026-04-06".to_string()));
    }

    #[test]
    fn excludes_mlk_day_2025() {
        let days = get_trading_days("2025-01-17", "2025-01-21");
        assert!(!days.contains(&"2025-01-20".to_string()));
    }

    #[test]
    fn excludes_thanksgiving_2025() {
        let days = get_trading_days("2025-11-24", "2025-11-28");
        assert!(!days.contains(&"2025-11-27".to_string()));
    }
}
