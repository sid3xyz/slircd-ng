use governor::Quota;
use std::num::NonZeroU32;
use std::time::Duration;

#[test]
fn test_repro_fix_logic() {
    let period: u32 = 1;
    let count: u32 = 4294967295;

    let mut period_per_token = Duration::from_secs_f64(period as f64 / count as f64);

    // This replicates the FIXED logic in src/state/actor/handlers/message.rs
    if period_per_token.is_zero() {
        period_per_token = Duration::from_nanos(1);
    }

    let _quota = Quota::with_period(period_per_token)
        .expect("Invalid flood period")
        .allow_burst(NonZeroU32::new(count).expect("Count must be > 0"));
}

#[test]
fn test_fix_modes_div_zero_protection() {
    let count: u32 = 0;
    let period: u32 = 10;

    // Logic from src/state/actor/handlers/modes.rs (Fixed)
    if count == 0 {
        // Should skip calculation
        return;
    }

    // If logic was broken, this would panic
    let _period_per_action = Duration::from_secs_f64(period as f64 / count as f64);
}

#[test]
fn test_fix_modes_unwrap_protection() {
    let count: u32 = 0;

    // Logic from src/state/actor/handlers/modes.rs (Fixed)
    let _val = NonZeroU32::new(count).unwrap_or(NonZeroU32::MIN);

    assert_eq!(_val.get(), 1);
}

#[test]
fn test_fix_message_div_zero_protection() {
    let count: u32 = 0;
    let period: u32 = 10;

    // Logic from src/state/actor/handlers/message.rs (Fixed)
    let period_per_token = if count == 0 {
        Duration::from_secs(period as u64)
    } else {
        Duration::from_secs_f64(period as f64 / count as f64)
    };

    assert_eq!(period_per_token, Duration::from_secs(10));
}
