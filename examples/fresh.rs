//! Fetch this month's rates from HMRC, falling back to bundled/cached data offline.
//!
//! Run with: cargo run --example fresh --features http
#![allow(clippy::unwrap_used)] // examples favour brevity

use hmrc_rates::Updater;
use rust_decimal::Decimal;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let updater = Updater::new();

    // Staleness must be a visible choice, so the fallback is explicit.
    let rates = updater.refreshed().unwrap_or_else(|e| {
        eprintln!("warning: possibly stale rates: {e}");
        updater.cached()
    });

    let today = chrono::Utc::now().date_naive();
    let rate = rates.monthly_rate("EUR", today)?;
    println!(
        "€100 today = £{} at {} EUR/£ ({})",
        rate.to_gbp(Decimal::from(100)).round_dp(2),
        rate.units_per_gbp(),
        rate.period(),
    );
    Ok(())
}
