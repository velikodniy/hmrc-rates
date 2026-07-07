use alloc::boxed::Box;

use crate::types::{Currency, Period, RateType};

/// Why a rate lookup failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LookupError {
    /// The code never appears anywhere in this rate series (or is not a valid code).
    #[error("currency '{code}' is not published in HMRC {table} rates")]
    UnknownCurrency { code: Box<str>, table: RateType },

    /// The series has no table for this period; `available` gives the loaded range.
    #[error("no HMRC {table} rates for {period}{}", available_range(.available))]
    PeriodNotAvailable {
        table: RateType,
        period: Period,
        available: Option<(Period, Period)>,
    },

    /// The period exists but this currency is absent from it.
    #[error("'{currency}' has no HMRC {table} rate for {period}")]
    NotInPeriod {
        currency: Currency,
        table: RateType,
        period: Period,
    },
}

fn available_range(available: &Option<(Period, Period)>) -> alloc::string::String {
    use alloc::format;
    match available {
        Some((first, last)) => format!(" (available {first} to {last})"),
        None => alloc::string::String::from(" (no data loaded)"),
    }
}
