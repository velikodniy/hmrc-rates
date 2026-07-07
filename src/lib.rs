//! Official HMRC exchange rates with bundled history and exact GBP conversion.
//!
//! Four rate series, looked up through one [`Rates`] value:
//! monthly customs/VAT rates, spot and yearly-average rates (31 March /
//! 31 December), and the discontinued 2014–2016 weekly amendments.
//!
//! ```
//! use hmrc_rates::{Rates, Month};
//! use rust_decimal::Decimal;
//!
//! let rates = Rates::new();
//! let rate = rates.monthly_rate("USD", Month::new(2025, 8).unwrap())?;
//! let gbp = rate.to_gbp(Decimal::from(100)); // exact, unrounded
//! # Ok::<(), hmrc_rates::LookupError>(())
//! ```
//!
//! Rates are `rateNew` figures — currency units per £1; conversion divides.
//! Lookups are strict: a period HMRC never published is an error, never a
//! silently substituted older rate.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod date;
mod error;
mod rate;
mod rates;
mod store;
mod types;

#[cfg(feature = "bundled")]
mod bundled;

#[cfg(any(feature = "http", all(test, feature = "std")))]
#[allow(clippy::duplicate_mod)] // parse.rs carries its own date.rs copy for build.rs
mod parse;

#[cfg(feature = "http")]
mod http;

pub use error::LookupError;
pub use rate::Rate;
pub use rates::{Rates, Table};
pub use types::{Currency, Month, Period, RateType, YearEnd};

#[cfg(feature = "http")]
pub use http::{FetchError, Updater};
