use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc, Weekday};
use chrono_tz::America::New_York;

/// US Eastern DST: EDT (UTC-4) from 2nd Sunday of March to 1st Sunday of November.
pub fn is_edt(date: NaiveDate) -> bool {
    let year = date.year();
    let edt_start = nth_weekday_of_month(year, 3, Weekday::Sun, 2);
    let edt_end = nth_weekday_of_month(year, 11, Weekday::Sun, 1);
    date >= edt_start && date < edt_end
}

fn nth_weekday_of_month(year: i32, month: u32, weekday: Weekday, n: u32) -> NaiveDate {
    let first = NaiveDate::from_ymd_opt(year, month, 1)
        .expect("invalid month in nth_weekday_of_month");
    let first_wd = first.weekday().num_days_from_sunday();
    let target_wd = weekday.num_days_from_sunday();
    let offset = (target_wd + 7 - first_wd) % 7;
    let day = 1 + offset + 7 * (n - 1);
    NaiveDate::from_ymd_opt(year, month, day)
        .expect("computed day out of range in nth_weekday_of_month")
}

/// Convert a UTC `DateTime` to Eastern Time HHMM (e.g. 1030 = 10:30 AM ET).
#[inline]
pub fn et_hhmm(ts: &DateTime<Utc>) -> u32 {
    let et = ts.with_timezone(&New_York);
    et.hour() * 100 + et.minute()
}

/// Convert a Unix-seconds timestamp to Eastern Time HHMM. Returns `None` for invalid timestamps.
pub fn et_hhmm_from_epoch(epoch_sec: i64) -> Option<u32> {
    use chrono::TimeZone;
    Utc.timestamp_opt(epoch_sec, 0).single().map(|ts| et_hhmm(&ts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn january_is_est() {
        assert!(!is_edt(NaiveDate::from_ymd_opt(2025, 1, 15).unwrap()));
    }

    #[test]
    fn july_is_edt() {
        assert!(is_edt(NaiveDate::from_ymd_opt(2025, 7, 15).unwrap()));
    }

    #[test]
    fn edt_boundary_spring_2025() {
        // 2025: 2nd Sunday of March = March 9
        let before = NaiveDate::from_ymd_opt(2025, 3, 8).unwrap();
        let on = NaiveDate::from_ymd_opt(2025, 3, 9).unwrap();
        assert!(!is_edt(before));
        assert!(is_edt(on));
    }

    #[test]
    fn edt_boundary_fall_2025() {
        // 2025: 1st Sunday of November = November 2
        let before = NaiveDate::from_ymd_opt(2025, 11, 1).unwrap();
        let on = NaiveDate::from_ymd_opt(2025, 11, 2).unwrap();
        assert!(is_edt(before));
        assert!(!is_edt(on));
    }

    #[test]
    fn et_hhmm_edt() {
        // 2024-03-15 14:50 UTC = 10:50 AM EDT
        assert_eq!(et_hhmm_from_epoch(1710514200), Some(1050));
    }

    #[test]
    fn et_hhmm_est() {
        // 2024-01-15 15:30 UTC = 10:30 AM EST
        assert_eq!(et_hhmm_from_epoch(1705332600), Some(1030));
    }
}
