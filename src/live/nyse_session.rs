use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::America::New_York;
use nyse_holiday_cal::HolidayCal;

const MARKET_OPEN_MIN: u32 = 9 * 60 + 30; // 9:30 ET
const MARKET_CLOSE_MIN: u32 = 16 * 60; // 16:00 ET

fn et_minute_of_day(now: &DateTime<Utc>) -> u32 {
    let et = now.with_timezone(&New_York);
    et.hour() * 60 + et.minute()
}

/// NYSE regular-session calendar: hours, US/Eastern date, holidays.
pub struct NyseSession;

impl NyseSession {
    /// True when NYSE is open for trading right now (weekday, not a holiday, within hours).
    pub fn is_open(now: &DateTime<Utc>) -> bool {
        if Self::is_closed_all_day(now) {
            return false;
        }
        let min = et_minute_of_day(now);
        (MARKET_OPEN_MIN..MARKET_CLOSE_MIN).contains(&min)
    }

    /// Weekend or NYSE holiday — no session at all today.
    pub fn is_closed_all_day(now: &DateTime<Utc>) -> bool {
        let et = now.with_timezone(&New_York);
        let date = et.date_naive();
        match date.is_busday() {
            Ok(is_bus) => !is_bus,
            Err(_) => {
                let dow = date.weekday().num_days_from_sunday();
                dow == 0 || dow == 6
            }
        }
    }

    /// Calendar date in America/New_York (same style as [`crate::types::DATE_FMT`]).
    pub fn et_date_str(now: &DateTime<Utc>) -> String {
        let et = now.with_timezone(&New_York);
        et.format(crate::types::DATE_FMT).to_string()
    }

    pub fn minutes_until_open(now: &DateTime<Utc>) -> i64 {
        let min = et_minute_of_day(now) as i64;
        let open = MARKET_OPEN_MIN as i64;
        let close = MARKET_CLOSE_MIN as i64;

        if min < open {
            open - min
        } else if min < close {
            0
        } else {
            (24 * 60 - min) + open
        }
    }

    pub async fn wait_until_open(now: &DateTime<Utc>) {
        if Self::is_closed_all_day(now) {
            return;
        }
        let wait_mins = Self::minutes_until_open(now);
        if wait_mins <= 0 {
            return;
        }
        super::log_debug!(
            "[live] Market opens in {:.1} hours, waiting...",
            wait_mins as f64 / 60.0
        );
        tokio::time::sleep(tokio::time::Duration::from_secs(wait_mins as u64 * 60)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn market_open_during_hours() {
        let ts = Utc.with_ymd_and_hms(2025, 2, 12, 15, 0, 0).unwrap(); // 10:00 AM ET (EST)
        assert!(NyseSession::is_open(&ts));
    }

    #[test]
    fn market_closed_before_open() {
        let ts = Utc.with_ymd_and_hms(2025, 2, 12, 14, 0, 0).unwrap(); // 9:00 AM ET
        assert!(!NyseSession::is_open(&ts));
    }

    #[test]
    fn market_closed_after_close() {
        let ts = Utc.with_ymd_and_hms(2025, 2, 12, 21, 30, 0).unwrap(); // 4:30 PM ET
        assert!(!NyseSession::is_open(&ts));
    }

    #[test]
    fn market_closed_on_weekend_saturday() {
        let ts = Utc.with_ymd_and_hms(2025, 2, 15, 15, 0, 0).unwrap(); // Saturday
        assert!(!NyseSession::is_open(&ts));
        assert!(NyseSession::is_closed_all_day(&ts));
    }

    #[test]
    fn market_closed_on_weekend_sunday() {
        let ts = Utc.with_ymd_and_hms(2025, 2, 16, 15, 0, 0).unwrap(); // Sunday
        assert!(!NyseSession::is_open(&ts));
        assert!(NyseSession::is_closed_all_day(&ts));
    }

    #[test]
    fn weekday_not_closed_all_day() {
        let ts = Utc.with_ymd_and_hms(2025, 2, 12, 15, 0, 0).unwrap(); // Wednesday
        assert!(!NyseSession::is_closed_all_day(&ts));
    }

    #[test]
    fn good_friday_2026_closed() {
        let ts = Utc.with_ymd_and_hms(2026, 4, 3, 14, 0, 0).unwrap();
        assert!(NyseSession::is_closed_all_day(&ts));
        assert!(!NyseSession::is_open(&ts));
    }

    #[test]
    fn today_date_str_format() {
        let ts = Utc.with_ymd_and_hms(2025, 2, 12, 15, 0, 0).unwrap();
        let s = NyseSession::et_date_str(&ts);
        assert_eq!(s, "2025-02-12");
    }

    #[test]
    fn minutes_until_open_before_930() {
        let ts = Utc.with_ymd_and_hms(2025, 2, 12, 14, 0, 0).unwrap(); // 9:00 AM ET
        assert_eq!(NyseSession::minutes_until_open(&ts), 30);
    }

    #[test]
    fn minutes_until_open_during_market() {
        let ts = Utc.with_ymd_and_hms(2025, 2, 12, 15, 0, 0).unwrap(); // 10:00 AM ET
        assert_eq!(NyseSession::minutes_until_open(&ts), 0);
    }

    #[test]
    fn minutes_until_open_after_close() {
        let ts = Utc.with_ymd_and_hms(2025, 2, 12, 21, 30, 0).unwrap();
        let mins = NyseSession::minutes_until_open(&ts);
        assert_eq!(mins, 1020);
    }

    #[test]
    fn minutes_until_open_at_midnight_et() {
        let ts = Utc.with_ymd_and_hms(2025, 2, 12, 5, 0, 0).unwrap();
        let mins = NyseSession::minutes_until_open(&ts);
        assert_eq!(mins, 570);
    }
}
