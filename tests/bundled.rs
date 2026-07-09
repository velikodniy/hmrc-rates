//! Behavior of the bundled dataset through the public API.
#![cfg(feature = "bundled")]
#![allow(clippy::unwrap_used, clippy::panic)]

use chrono::NaiveDate;
use hmrc_rates::{LookupError, Period, RateType, Rates, YearEnd, YearMonth};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[test]
fn bundled_monthly_coverage_is_contiguous_from_2014_02() {
    let rates = Rates::new();
    let months: Vec<YearMonth> = rates.months().collect();
    assert_eq!(months.first().copied(), YearMonth::new(2014, 2));
    assert!(months.last().unwrap() >= &YearMonth::new(2026, 7).unwrap());
    for pair in months.windows(2) {
        assert_eq!(pair[0].next(), pair[1], "gap after {}", pair[0]);
    }
}

#[test]
fn golden_usd_and_eur_august_2025() {
    let rates = Rates::new();
    let usd = rates
        .monthly_rate("USD", YearMonth::new(2025, 8).unwrap())
        .unwrap();
    assert_eq!(usd.units_per_gbp(), dec!(1.3541));
    // 0.1 behavior preserved modulo rounding: £73.85 for $100 after round_dp(2)
    assert_eq!(usd.to_gbp(dec!(100.00)).round_dp(2), dec!(73.85));

    let eur = rates.monthly_rate("EUR", date(2025, 8, 15)).unwrap();
    assert_eq!(eur.units_per_gbp(), dec!(1.1547));
    assert_eq!(eur.to_gbp(dec!(100.00)).round_dp(2), dec!(86.60));
}

#[test]
fn conversions_are_exact_not_rounded() {
    let rates = Rates::new();
    let rate = rates
        .monthly_rate("USD", YearMonth::new(2025, 8).unwrap())
        .unwrap();
    let gbp = rate.to_gbp(dec!(100));
    assert_ne!(gbp, gbp.round_dp(2)); // 100/1.3541 is not a 2dp number
    assert_eq!(rate.from_gbp(gbp).round_dp(10), dec!(100));
}

#[test]
fn strict_monthly_lookup_errors_outside_coverage() {
    let rates = Rates::new();
    for (y, m) in [(2014, 1), (2013, 12), (2035, 1)] {
        let err = rates
            .monthly_rate("USD", YearMonth::new(y, m).unwrap())
            .unwrap_err();
        assert!(
            matches!(
                err,
                LookupError::PeriodNotAvailable {
                    table: RateType::Monthly,
                    ..
                }
            ),
            "{y}-{m}: {err}"
        );
    }
}

#[test]
fn absurd_year_ends_error_instead_of_panicking() {
    let rates = Rates::new();
    for year in [i32::MAX, i32::MIN, 2025 + i32::MIN] {
        let err = rates.spot(YearEnd::march(year));
        assert!(
            matches!(err, Err(LookupError::PeriodNotAvailable { .. })),
            "year {year}: {err:?}"
        );
    }
}

#[test]
fn gbp_identity_everywhere() {
    let rates = Rates::new();
    // Any month, including unpublished ones
    let rate = rates
        .monthly_rate("gbp", YearMonth::new(2035, 1).unwrap())
        .unwrap();
    assert_eq!(rate.units_per_gbp(), Decimal::ONE);
    assert_eq!(rate.to_gbp(dec!(42)), dec!(42));
    // Through tables too
    let table = rates.spot(YearEnd::december(2024)).unwrap();
    assert_eq!(table.rate("GBP").unwrap().units_per_gbp(), Decimal::ONE);
    // The fallback never substitutes a month for GBP: £1 = £1 as requested
    let next = rates.months().next_back().unwrap().next();
    let rate = rates.monthly_rate_or_earlier("GBP", next, 5).unwrap();
    assert_eq!(rate.period(), Period::Month(next));
}

#[test]
fn fallback_walks_back_and_reveals_period() {
    let rates = Rates::new();
    let last = rates.months().next_back().unwrap();
    let next = last.next();
    // Strict lookup fails for the month after the last published one...
    assert!(rates.monthly_rate("USD", next).is_err());
    // ...the explicit fallback resolves to the last month and says so
    let rate = rates.monthly_rate_or_earlier("USD", next, 1).unwrap();
    assert_eq!(rate.period(), Period::Month(last));
    // A zero-step fallback stays strict
    assert!(rates.monthly_rate_or_earlier("USD", next, 0).is_err());
    // Exhausted window is an error for the requested month
    let far = YearMonth::new(2035, 1).unwrap();
    assert!(matches!(
        rates.monthly_rate_or_earlier("USD", far, 12),
        Err(LookupError::PeriodNotAvailable { .. })
    ));
}

#[test]
fn unknown_currency_vs_not_in_period() {
    let rates = Rates::new();
    // Garbage input and never-published codes fold into UnknownCurrency
    for code in ["XXX", "", "US DOLLARS", "usd extra"] {
        let err = rates
            .monthly_rate(code, YearMonth::new(2025, 8).unwrap())
            .unwrap_err();
        assert!(
            matches!(err, LookupError::UnknownCurrency { .. }),
            "{code:?}: {err}"
        );
    }
    // GHS appears in the full-list Dec 2015 spot table but not in Dec 2024
    let err = rates
        .spot(YearEnd::december(2024))
        .unwrap()
        .rate("GHS")
        .unwrap_err();
    assert!(
        matches!(
            err,
            LookupError::NotInPeriod {
                table: RateType::Spot,
                ..
            }
        ),
        "{err}"
    );
    // KMF never appears in any spot table
    let err = rates
        .spot(YearEnd::december(2024))
        .unwrap()
        .rate("KMF")
        .unwrap_err();
    assert!(
        matches!(
            err,
            LookupError::UnknownCurrency {
                table: RateType::Spot,
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn spot_and_average_periods() {
    let rates = Rates::new();
    let spots: Vec<YearEnd> = rates.spot_periods().collect();
    assert_eq!(spots.first().copied(), Some(YearEnd::december(2010)));
    assert!(spots.contains(&YearEnd::march(2026)));

    let averages: Vec<YearEnd> = rates.average_periods().collect();
    assert_eq!(averages.first().copied(), Some(YearEnd::december(2010)));

    // USA row wins over USD-pegged countries in the messy Dec 2015 file
    let usd = rates
        .spot(YearEnd::december(2015))
        .unwrap()
        .rate("USD")
        .unwrap();
    assert_eq!(usd.units_per_gbp(), dec!(1.4833));

    let eur = rates
        .average(YearEnd::march(2026))
        .unwrap()
        .rate("EUR")
        .unwrap();
    assert!(eur.units_per_gbp() > Decimal::ONE);

    // Pre-euro history keeps its historical codes
    let eek = rates
        .average(YearEnd::december(2010))
        .unwrap()
        .rate("EEK")
        .unwrap();
    assert!(eek.units_per_gbp() > dec!(15));

    let err = rates.spot(YearEnd::march(2010)).unwrap_err();
    assert!(matches!(
        err,
        LookupError::PeriodNotAvailable {
            table: RateType::Spot,
            ..
        }
    ));
}

#[test]
fn weekly_amendments_by_containing_date() {
    let rates = Rates::new();
    // First-ever amendment: 2014-01-08, TRY 3.5418; valid for the following week
    let table = rates.weekly(date(2014, 1, 10)).unwrap();
    assert_eq!(table.rate("TRY").unwrap().units_per_gbp(), dec!(3.5418));
    let Period::Week { start, .. } = table.period() else {
        panic!("weekly table has a non-week period")
    };
    assert_eq!(start, date(2014, 1, 8));

    // Last-ever amendment week
    let last = rates.weekly(date(2016, 4, 27)).unwrap();
    assert_eq!(last.rate("ZMW").unwrap().units_per_gbp(), dec!(13.04));

    // Outside the series' life
    assert!(matches!(
        rates.weekly(date(2017, 1, 1)),
        Err(LookupError::PeriodNotAvailable {
            table: RateType::Weekly,
            ..
        })
    ));
    assert!(rates.weekly(date(2013, 12, 31)).is_err());

    let weeks: Vec<Period> = rates.weeks().collect();
    assert!(weeks.len() > 100);
}

#[test]
fn discovery_iterators() {
    let rates = Rates::new();
    let monthly_currencies: Vec<_> = rates.currencies(RateType::Monthly).collect();
    assert!(monthly_currencies.iter().any(|c| c.as_str() == "USD"));
    assert!(monthly_currencies.len() > 150);

    let spot_currencies: Vec<_> = rates.currencies(RateType::Spot).collect();
    assert!(spot_currencies.iter().any(|c| c.as_str() == "USD"));

    let table = rates.monthly(YearMonth::new(2026, 6).unwrap()).unwrap();
    assert_eq!(table.iter().len(), table.len());
    assert!(!table.is_empty());
    let (currency, rate) = table.iter().next().unwrap();
    assert_eq!(rate.currency(), currency);
    assert_eq!(rate.period(), table.period());
}

#[test]
fn errors_render_helpful_messages() {
    let rates = Rates::new();
    let err = rates
        .monthly_rate("USD", YearMonth::new(2013, 1).unwrap())
        .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("2013-01"), "{message}");
    assert!(message.contains("available 2014-02"), "{message}");

    let err = rates
        .monthly_rate("XXX", YearMonth::new(2025, 8).unwrap())
        .unwrap_err();
    assert!(err.to_string().contains("'XXX'"));
}
