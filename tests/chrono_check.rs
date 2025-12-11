use chrono::{TimeZone, Utc};

#[test]
fn test_chrono_rounding() {
    let nanos = 473_500_000;
    let dt = Utc.timestamp_opt(0, nanos).unwrap();
    println!("Nanos: {}", nanos);
    let formatted = dt.format("%Y-%m-%dT%H:%M:%S.%3fZ").to_string();
    println!("Formatted: {}", formatted);

    let nanos2 = 473_499_999;
    let dt2 = Utc.timestamp_opt(0, nanos2).unwrap();
    println!("Nanos: {}", nanos2);
    let formatted2 = dt2.format("%Y-%m-%dT%H:%M:%S.%3fZ").to_string();
    println!("Formatted: {}", formatted2);
}
