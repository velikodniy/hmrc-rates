//! Convert amounts to GBP with bundled rates — no network, no setup.
//!
//! Run with: cargo run --example convert
#![allow(clippy::unwrap_used)] // examples favour brevity

use hmrc_rates::{Month, RateType, Rates, YearEnd};
use rust_decimal::Decimal;

fn main() -> Result<(), hmrc_rates::LookupError> {
    let rates = Rates::new();

    // The everyday case: an amount on a date, at that month's rate.
    let date = chrono::NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();
    let rate = rates.monthly_rate("USD", date)?;
    let gbp = rate.to_gbp(Decimal::from(2500));
    println!("$2500 on {date} = £{} (exact: £{gbp})", gbp.round_dp(2));

    // Many conversions in one month: resolve the table once.
    let table = rates.monthly(Month::new(2025, 8).unwrap())?;
    let eur = table.rate("EUR")?;
    let total: Decimal = [1200, 450, 80]
        .into_iter()
        .map(|amount| eur.to_gbp(Decimal::from(amount)))
        .sum();
    println!("three EUR invoices = £{}", total.round_dp(2));

    // Self Assessment style: the yearly average to 31 March.
    let avg = rates.average(YearEnd::march(2025))?.rate("EUR")?;
    println!(
        "EUR average for the year to 31 Mar 2025: {}",
        avg.units_per_gbp()
    );

    // What's available?
    let months = rates.months().count();
    let currencies = rates.currencies(RateType::Monthly).count();
    println!("{months} months bundled, {currencies} currencies");
    Ok(())
}
