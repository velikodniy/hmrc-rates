//! Round-trips for the optional serde feature.
#![cfg(all(feature = "serde", feature = "bundled"))]
#![allow(clippy::unwrap_used)]

use hmrc_rates::{Currency, Month, Period, Rate, RateType, Rates, YearEnd};

fn roundtrip<T>(value: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    serde_json::from_str(&serde_json::to_string(value).unwrap()).unwrap()
}

#[test]
fn compact_string_forms() {
    let month = Month::new(2026, 7).unwrap();
    assert_eq!(serde_json::to_string(&month).unwrap(), r#""2026-07""#);
    assert_eq!(roundtrip(&month), month);

    let year_end = YearEnd::march(2026);
    assert_eq!(serde_json::to_string(&year_end).unwrap(), r#""2026-03""#);
    assert_eq!(roundtrip(&year_end), year_end);
    assert_eq!(roundtrip(&YearEnd::december(2025)), YearEnd::december(2025));

    assert_eq!(serde_json::to_string(&Currency::GBP).unwrap(), r#""GBP""#);
    assert_eq!(roundtrip(&Currency::GBP), Currency::GBP);

    assert_eq!(serde_json::to_string(&RateType::Monthly).unwrap(), r#""monthly""#);
    assert_eq!(roundtrip(&RateType::Weekly), RateType::Weekly);
}

#[test]
fn invalid_strings_are_rejected() {
    assert!(serde_json::from_str::<Month>(r#""2026-13""#).is_err());
    assert!(serde_json::from_str::<Month>(r#""not a month""#).is_err());
    assert!(serde_json::from_str::<YearEnd>(r#""2026-07""#).is_err());
    assert!(serde_json::from_str::<Currency>(r#""US""#).is_err());
}

#[test]
fn period_and_rate_roundtrip() {
    let rates = Rates::new();
    let rate = rates.monthly_rate("USD", Month::new(2025, 8).unwrap()).unwrap();
    let back: Rate = roundtrip(&rate);
    assert_eq!(back, rate);
    assert_eq!(back.units_per_gbp(), rate.units_per_gbp());

    let period = Period::Month(Month::new(2025, 8).unwrap());
    assert_eq!(serde_json::to_string(&period).unwrap(), r#"{"month":"2025-08"}"#);
    assert_eq!(roundtrip(&period), period);

    let week = rates.weeks().next().unwrap();
    assert_eq!(roundtrip(&week), week);
}
