//! Official HMRC exchange rates with bundled history and exact GBP conversion.
//!
//! One [`Rates`] value holds all four series HMRC has published:
//! monthly customs/VAT rates (from 2014-02),
//! spot and yearly-average rates for years ending 31 March / 31 December (from 2010-12),
//! and the discontinued 2014–2016 weekly amendments.
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
//! Rates are HMRC's figures, i.e. currency units per £1.
//! Conversion divides exactly, and the crate never rounds.
//!
//! Lookups are strict.
//! An unpublished period is an error, never a silently substituted older rate.
//! Fallback is explicit, see [`Rates::monthly_rate_or_earlier`].
//!
//! # Features
//!
//! - `std`, `bundled` (default): the full history compiled in,
//!   no parsing or I/O at startup, ~450 KB of read-only data.
//! - `http`: a blocking `Updater` that fetches newly published periods,
//!   with an on-disk cache.
//! - `serde`: compact string forms (`"2026-07"`, `"USD"`, `"monthly"`).
//! - `cli`: the `hmrc-rates` binary.
//!
//! The core is `no_std` + `alloc`
//! with only `bundled` enabled the crate builds on `wasm32-unknown-unknown`.

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
