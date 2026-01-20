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
