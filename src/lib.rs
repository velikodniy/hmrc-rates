//! Official HMRC exchange rates with bundled history and exact GBP conversion.
//!
//! Four rate series, looked up through one [`Rates`] value: monthly
//! customs/VAT rates (2014-02 onwards), spot and yearly-average rates for
//! years ending 31 March / 31 December (December 2010 onwards), and the
//! discontinued 2014–2016 weekly amendments.
//!
//! ```
//! use hmrc_rates::{Rates, Month};
//! use rust_decimal::Decimal;
//!
//! let rates = Rates::new(); // free: the tables are compiled-in statics
//! let rate = rates.monthly_rate("USD", Month::new(2025, 8).unwrap())?;
//! let gbp = rate.to_gbp(Decimal::from(100)); // exact, unrounded
//! # Ok::<(), hmrc_rates::LookupError>(())
//! ```
//!
//! Rates are HMRC's `rateNew` figures — currency units per £1; conversion
//! divides using exact [`rust_decimal`] arithmetic, and the crate never
//! rounds. Lookups are strict: a period HMRC never published is an error,
//! never a silently substituted older rate. Fallback is explicit — see
//! [`Rates::monthly_rate_or_earlier`].
//!
//! # Features
//!
//! - `std`, `bundled` (default): the full published history compiled into
//!   the binary by `build.rs` — no parsing or I/O at startup, ~450 KB of
//!   read-only data.
//! - `http`: a blocking `Updater` that fetches periods HMRC has published
//!   since the crate release, with an on-disk cache.
//! - `serde`: compact string forms (`"2026-07"`, `"USD"`, `"monthly"`).
//! - `cli`: the `hmrc-rates` binary.
//!
//! The core is `no_std` + `alloc`: with only `bundled` enabled the crate
//! builds on `wasm32-unknown-unknown`.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod error;
mod rate;
mod rates;
mod store;
mod types;

#[cfg(feature = "bundled")]
mod bundled;

#[cfg(any(feature = "http", all(test, feature = "std")))]
mod parse;

#[cfg(feature = "http")]
mod http;

pub use error::LookupError;
pub use rate::Rate;
pub use rates::{Rates, Table};
pub use types::{Currency, Month, ParseMonthError, Period, RateType, YearEnd};

#[cfg(feature = "http")]
pub use http::{FetchError, Updater};
