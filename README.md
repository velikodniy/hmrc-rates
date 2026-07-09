# hmrc-rates

[![crates.io](https://img.shields.io/crates/v/hmrc-rates.svg)](https://crates.io/crates/hmrc-rates)
[![docs.rs](https://img.shields.io/docsrs/hmrc-rates)](https://docs.rs/hmrc-rates)
[![PyPI](https://img.shields.io/pypi/v/hmrc-rates.svg)](https://pypi.org/project/hmrc-rates/)

Official HMRC exchange rates as a Rust library.
The full published history is compiled into your binary (~450 KB of read-only data), so `Rates::new()` is free and infallible — no parsing, no I/O, no startup cost.
Conversions use exact `rust_decimal` arithmetic and are never rounded.

## Install

```toml
[dependencies]
hmrc-rates = "0.3"
rust_decimal = "1"
```

## Quick start

```rust
use hmrc_rates::{YearMonth, Rates, YearEnd};
use rust_decimal::Decimal;

fn main() -> Result<(), hmrc_rates::LookupError> {
    let rates = Rates::new();

    // Monthly customs/VAT rate; also accepts a chrono::NaiveDate
    let rate = rates.monthly_rate("USD", YearMonth::new(2025, 8).unwrap())?;
    let gbp = rate.to_gbp(Decimal::from(2500)); // exact — you choose the rounding
    println!("$2500 in Aug 2025 = £{}", gbp.round_dp(2));

    // Self Assessment style: the yearly average to 31 March 2025
    let eur = rates.average(YearEnd::march(2025))?.rate("EUR")?;
    println!("EUR average, year to 31 Mar 2025: {}", eur.units_per_gbp());
    Ok(())
}
```

## Data coverage

One `Rates` value holds all four series HMRC has published:

| Series | Coverage | Lookup |
| --- | --- | --- |
| Monthly customs/VAT | 2014-02 — present, no gaps | `monthly_rate(code, month)` |
| Spot | Dec 2010 — present, years ending 31 Mar / 31 Dec | `spot(YearEnd)` |
| Yearly average | Dec 2010 — present, years ending 31 Mar / 31 Dec | `average(YearEnd)` |
| Weekly amendments | 2014-01 — 2016-04, complete (discontinued by HMRC) | `weekly(date)` |

Lookups are strict: a period HMRC never published is an error, never a silently substituted older rate.
The error says why: unknown currency, period not available (with the loaded range), or currency absent from that period.
Fallback is opt-in and visible:

```rust,ignore
let rate = rates.monthly_rate_or_earlier("USD", month, 2)?;
rate.period() // reveals which month was actually used
```

Currency codes are as published by HMRC, which is not always ISO 4217
E.g., Ecuador appears as `ECS`.
See [docs/data-sources.md](docs/data-sources.md) for where every rate comes from.

## Features

| Feature | Default | Adds |
| --- | --- | --- |
| `std` | yes | — |
| `bundled` | yes | the compiled-in history and `Rates::new()` |
| `http` | no | blocking `Updater` (ureq) with an on-disk cache |
| `serde` | no | compact string forms: `"2026-07"`, `"USD"`, `"monthly"` |
| `cli` | no | the `hmrc-rates` binary |

The core is `no_std` + `alloc`: `default-features = false, features = ["bundled"]` builds on `wasm32-unknown-unknown`.

## Fresh rates (`http`)

`Updater` fetches whatever HMRC has published since the crate release and caches the files verbatim in the system cache directory.
Past periods are served from disk forever; amendable periods get a 24-hour TTL.

```rust,ignore
use hmrc_rates::Updater;

let updater = Updater::new();
// Offline fallback is explicit: stale rates should be a visible choice
let rates = updater.refreshed().unwrap_or_else(|e| {
    eprintln!("warning: possibly stale rates: {e}");
    updater.cached()
});
```

## Python

The same library is on PyPI as [`hmrc-rates`](https://pypi.org/project/hmrc-rates/) (Python 3.10+):

```sh
pip install hmrc-rates
```

```python
from decimal import Decimal
from hmrc_rates import YearMonth, Rates, YearEnd

rates = Rates()
rate = rates.monthly_rate("USD", YearMonth(2025, 8))  # also accepts datetime.date or "2025-08"
gbp = rate.to_gbp(Decimal("2500"))                # exact decimal.Decimal, you choose the rounding
eur = rates.average(YearEnd.march(2025)).rate("EUR")
```

See [python/README.md](python/README.md) for the full Python API.

## Documentation

- API reference: [docs.rs/hmrc-rates](https://docs.rs/hmrc-rates)
- Data provenance: [docs/data-sources.md](docs/data-sources.md)
- Runnable examples: [`examples/convert.rs`](examples/convert.rs), [`examples/fresh.rs`](examples/fresh.rs)

## MSRV and licence

Rust 1.85. Licensed under [MIT](LICENSE).
