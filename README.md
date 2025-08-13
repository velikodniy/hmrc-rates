# HMRC Currency Conversion Library

This Rust library provides a simple and efficient way to convert various currencies to GBP using HMRC's monthly exchange rates. It is designed to be easy to use, with a straightforward API that handles the complexities of parsing and looking up exchange rates for you.



## Getting Started

To use this library in your project, simply add the following to your `Cargo.toml` file:

```toml
[dependencies]
hmrc-rates = "0.1.0"
```

## Usage

```rust
use hmrc_rates::{HMRCMonthlyRatesConverter, ConversionError};
use chrono::NaiveDate;
use rust_decimal_macros::dec;

fn main() -> Result<(), ConversionError> {
    // Create a new converter with the default rates.
    let converter = HMRCMonthlyRatesConverter::with_default_rates()?;

    let trade_date = NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();
    let amount = dec!(100.00);

    // Perform the conversion.
    let gbp_value = converter.convert(amount, "USD", trade_date)?;

    println!("{} USD on {} was {}", amount, trade_date, gbp_value);
    // Expected output: 100.00 USD on 2025-08-15 was £73.85

    Ok(())
}
```

## Contributing

Contributions are welcome! If you find any issues or have suggestions for improvements, please open an issue or submit a pull request.
