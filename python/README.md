# hmrc-rates

Official HMRC exchange rates for Python, backed by the [`hmrc-rates`](https://crates.io/crates/hmrc-rates) Rust crate.
The full published history ships inside the wheel, so `Rates()` is free and needs no network, parsing or I/O.
All money values are `decimal.Decimal` and conversions are exact — the library never rounds.

## Install

```sh
python3 -m pip install hmrc-rates
```

Python ≥ 3.10.
Wheels ship for Linux (glibc and musl, x86_64 and aarch64), macOS and Windows.
The sdist builds anywhere with a Rust toolchain.

## Quick start

```python
import datetime
from decimal import Decimal

from hmrc_rates import Month, Rates, YearEnd

rates = Rates()

# Monthly customs/VAT rate; months can be Month, datetime.date, or "YYYY-MM"
rate = rates.monthly_rate("USD", Month(2025, 8))
gbp = rate.to_gbp(Decimal("2500"))  # exact — you choose the rounding
print(f"$2500 in Aug 2025 = £{gbp.quantize(Decimal('0.01'))}")

# Self Assessment style: the yearly average to 31 March 2025
eur = rates.average(YearEnd.march(2025)).rate("EUR")
print(f"EUR average, year to 31 Mar 2025: {eur.units_per_gbp}")

# Whole tables iterate as (Currency, Rate) pairs
table = rates.monthly(datetime.date(2025, 8, 15))
print(f"{len(table)} currencies in {table.period}")
```

`"GBP"` always resolves to the identity rate, and code lookups are case-insensitive.

## Data coverage

One `Rates` value holds all four series HMRC has published:

| Series | Coverage | Lookup |
| --- | --- | --- |
| Monthly customs/VAT | 2014-02 — present, no gaps | `monthly_rate(code, month)` |
| Spot | Dec 2010 — present, years ending 31 Mar / 31 Dec | `spot(year_end)` |
| Yearly average | Dec 2010 — present, years ending 31 Mar / 31 Dec | `average(year_end)` |
| Weekly amendments | 2014-01 — 2016-04, complete (discontinued by HMRC) | `weekly(date)` |

Currency codes are as published by HMRC, which is not always ISO 4217.
E.g. Ecuador appears as `ECS`.

## Strict lookups and exceptions

A period HMRC never published is an exception, never a silently substituted older rate.
Every exception derives from `HmrcRatesError` (not the builtin `LookupError`):

- `UnknownCurrencyError` — the code never appears in that series
- `PeriodNotAvailableError` — no table for that period (the message includes the available range)
- `NotInPeriodError` — the period exists but the currency is absent from it
- `FetchError` — fetching fresh rates failed

Fallback is opt-in and visible:

```python
rate = rates.monthly_rate_or_earlier("USD", month, 2)
rate.period  # reveals which month was actually used
```

## Exactness

Amounts must be `decimal.Decimal` or `int`: floats raise `TypeError` because they are inexact.
Results come back as `decimal.Decimal`, unrounded.
Apply whatever rounding your tax context requires.

## Fresh rates

`Updater` fetches whatever HMRC has published since this release and caches the files verbatim in the system cache directory.
`refreshed()` performs blocking HTTP with the GIL released.
`cached()` never touches the network.

```python
from hmrc_rates import FetchError, Updater

updater = Updater()
try:
    rates = updater.refreshed()
except FetchError as e:
    print(f"warning: possibly stale rates: {e}")
    rates = updater.cached()  # offline fallback is an explicit choice
```

New HMRC publications also trigger a new patch release of this package automatically.
So pinning the latest version is an alternative to runtime fetching.

## Development

```sh
cd python
uv venv && uv pip install maturin pytest
VIRTUAL_ENV=$PWD/.venv .venv/bin/maturin develop
.venv/bin/pytest
```

The package is a [PyO3](https://pyo3.rs) binding built with [Maturin](https://maturin.rs).
The Rust sources live in the [repository root](https://github.com/velikodniy/hmrc-rates).

## Licence

[MIT](https://github.com/velikodniy/hmrc-rates/blob/main/LICENSE).
