# Design Document: HMRC Currency Conversion Library

This document outlines the high-level design for a Rust library to convert various currencies to GBP using HMRC monthly exchange rates.

## 1. Core Types

### `HMRCMonthlyRatesConverter`

This is the main entry point for the library.

```rust
pub struct HMRCMonthlyRatesConverter {
    // BTreeMap is chosen for efficient range lookups by date.
    // Key: The start date of the exchange rate period.
    // Value: A map from currency code (e.g., "USD") to its rate.
    rates: BTreeMap<chrono::NaiveDate, BTreeMap<String, rust_decimal::Decimal>>,
}
```

It will expose methods like:

- `new() -> Result<Self, ConversionError>`: Creates an instance by parsing the embedded XML exchange rate data.
- `convert(&self, amount: Decimal, currency: &str, date: NaiveDate) -> Result<GBP, ConversionError>`: Finds the appropriate rate for the given date and currency, and converts the value to GBP.

### `GBP`

A dedicated type to represent money in British Pounds, built on a decimal type for precision.

```rust
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GBP(rust_decimal::Decimal);
```

- It will implement `std::fmt::Display` to format as `£123.45`.
- It will implement standard math traits (`Add`, `Sub`, `Mul`, `Div`) for safe arithmetic operations.
- It will provide methods like `as_decimal() -> &Decimal` for interoperability.

### `ConversionError`

A comprehensive error enum for all library operations. Using the `thiserror` crate is recommended.

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConversionError {
    #[error("Invalid input format: '{0}'. Expected format 'VALUE CURRENCY'.")]
    InvalidInputFormat(String),

    #[error("Currency not found: '{0}' for date {1}.")]
    CurrencyNotFound(String, chrono::NaiveDate),

    #[error("No exchange rate data available for date: {0}.")]
    DateOutOfRange(chrono::NaiveDate),

    #[error("Failed to parse XML data.")]
    XmlParseError(#[from] roxmltree::Error),
}
```

## 2. Data Handling and Storage

### Compile-Time Embedding

- **Source Data:** Raw `.xml` files from HMRC will be stored in a `data/` directory in the repository.
- **Embedding:** The `data` directory will be embedded directly into the library binary at compile time using the `include_dir` crate.
- **Parsing:** When `HMRCMonthlyRatesConverter::new()` is called, it will iterate through the embedded files, parse the XML strings, and populate the `rates` `BTreeMap`. This parsing happens once at runtime during initialization. If multiple files define rates for the same month, the last one parsed will overwrite the previous entries.

## 3. Rate Lookup Logic

The core logic for finding a rate for a specific `date` and `currency`:

1.  Use `BTreeMap::range(..=date)` on the `rates` map and take the last element (`.next_back()`). This efficiently finds the latest period start date that is on or before the requested `date`.
2.  If no such period is found, return `ConversionError::DateOutOfRange`.
3.  If a period is found, get the corresponding inner `BTreeMap<String, Decimal>`.
4.  Look up the `currency` code in this inner map.
5.  If the currency is found, return the rate.
6.  If not found, return `ConversionError::CurrencyNotFound`.

## 4. Usage Example

```rust
use hmrc_rates::{HMRCMonthlyRatesConverter, ConversionError};
use chrono::NaiveDate;
use rust_decimal_macros::dec;

fn main() -> Result<(), ConversionError> {
    // Create a converter by parsing the embedded data.
    let converter = HMRCMonthlyRatesConverter::new()?;

    let trade_date = NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();
    let amount = dec!(100.00);

    // Perform the conversion.
    let gbp_value = converter.convert(amount, "USD", trade_date)?;

    println!("{} USD on {} was {}", amount, trade_date, gbp_value);
    // Expected output: 100.00 USD on 2025-08-15 was £XX.XX

    Ok(())
}
```
