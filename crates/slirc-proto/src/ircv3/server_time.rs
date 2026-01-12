//! Server-time formatting for IRCv3 server-time capability.

use std::time::{SystemTime, UNIX_EPOCH};

/// Format the current time as an IRCv3 server-time string.
///
/// Returns an ISO 8601 timestamp like `2023-01-01T12:00:00.000Z`.
pub fn format_server_time() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    let secs = now.as_secs();
    format_timestamp(secs)
}

/// Format a Unix timestamp as an IRCv3 server-time string.
///
/// Returns an ISO 8601 timestamp like `2023-01-01T12:00:00.000Z`.
pub fn format_timestamp(unix_secs: u64) -> String {
    if let Some(datetime) = chrono::DateTime::from_timestamp(unix_secs as i64, 0) {
        datetime.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    } else {
        "1970-01-01T00:00:00.000Z".to_string()
    }
}

/// Parse an IRCv3 server-time string to nanoseconds since Unix epoch.
///
/// Accepts RFC 3339 formatted timestamps like `2023-01-01T12:00:00.000Z`.
/// Returns 0 if parsing fails.
pub fn parse_server_time(ts: &str) -> i64 {
    use chrono::DateTime;

    DateTime::parse_from_rfc3339(ts)
        .ok()
        .and_then(|dt| dt.timestamp_nanos_opt())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_timestamp_epoch() {
        let result = format_timestamp(0);
        assert_eq!(result, "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn test_format_timestamp_known_date() {
        // 2023-01-01 00:00:00 UTC = 1672531200
        let result = format_timestamp(1672531200);
        assert_eq!(result, "2023-01-01T00:00:00.000Z");
    }

    #[test]
    fn test_parse_server_time_valid() {
        let ts = "2023-01-01T12:00:00.000Z";
        let nanos = parse_server_time(ts);
        // 2023-01-01 12:00:00 UTC = 1672574400 seconds
        assert_eq!(nanos, 1672574400_000_000_000);
    }

    #[test]
    fn test_parse_server_time_invalid() {
        let ts = "not a timestamp";
        let nanos = parse_server_time(ts);
        assert_eq!(nanos, 0);
    }

    #[test]
    fn test_parse_server_time_empty() {
        let nanos = parse_server_time("");
        assert_eq!(nanos, 0);
    }

    #[test]
    fn test_roundtrip() {
        let ts = 1672531200u64; // 2023-01-01 00:00:00 UTC
        let formatted = format_timestamp(ts);
        let parsed = parse_server_time(&formatted);
        // Should recover same timestamp (in nanos)
        assert_eq!(parsed, (ts as i64) * 1_000_000_000);
    }

    #[test]
    fn test_format_server_time_is_valid_rfc3339() {
        let ts = format_server_time();
        // Should be parseable
        assert!(chrono::DateTime::parse_from_rfc3339(&ts).is_ok());
        // Should end with Z (UTC)
        assert!(ts.ends_with('Z'));
    }
}
