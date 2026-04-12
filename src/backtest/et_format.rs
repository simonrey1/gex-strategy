use chrono::{DateTime, Utc};
use chrono_tz::America::New_York;

/// Eastern Time strings for logs and dashboard (UTC timestamps in → `YYYY-MM-DDTHH:MM` ET out).
pub struct EtFormat;

impl EtFormat {
    /// Format a UTC instant as Eastern for console display.
    pub fn utc(dt: &DateTime<Utc>) -> String {
        dt.with_timezone(&New_York).format("%Y-%m-%dT%H:%M").to_string()
    }

    /// Parse an RFC 3339 string (stored in UTC) and display in Eastern Time.
    pub fn from_rfc3339(s: &str) -> String {
        chrono::DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.with_timezone(&New_York).format("%Y-%m-%dT%H:%M").to_string())
            .unwrap_or_else(|_| s[..16.min(s.len())].to_string())
    }
}
