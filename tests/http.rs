//! Updater behavior against a mock Trade Tariff endpoint.
#![cfg(feature = "http")]
#![allow(clippy::unwrap_used)]

use chrono::Utc;
use hmrc_rates::{Month, Rates, Updater};
use httpmock::prelude::*;
use rust_decimal_macros::dec;

/// The month after the current one — always probed, never yet bundled.
fn next_month() -> Month {
    Month::from(Utc::now().date_naive()).next()
}

fn monthly_xml(month: Month, usd_rate: &str) -> String {
    let first = chrono::NaiveDate::from_ymd_opt(month.year(), month.month(), 1).unwrap();
    let last = first + chrono::Months::new(1) - chrono::Days::new(1);
    format!(
        r#"<?xml version="1.0"?>
<exchangeRateMonthList Period="{} to {}">
  <exchangeRate><countryName>USA</countryName><currencyCode>USD</currencyCode><rateNew>{usd_rate}</rateNew></exchangeRate>
</exchangeRateMonthList>"#,
        first.format("%d/%b/%Y"),
        last.format("%d/%b/%Y"),
    )
}

fn updater(server: &MockServer, cache: &tempfile::TempDir) -> Updater {
    Updater::new()
        .with_base_url(server.base_url())
        .with_cache_dir(cache.path())
}

/// Mocks that make every candidate URL 404 (nothing new published).
fn mock_all_missing(server: &MockServer) {
    server.mock(|when, then| {
        when.any_request();
        then.status(404);
    });
}

#[test]
fn refreshed_fetches_new_months_and_caches_verbatim() {
    let server = MockServer::start();
    let cache = tempfile::tempdir().unwrap();
    let next = next_month();

    let body = monthly_xml(next, "9.9999");
    let next_mock = server.mock(|when, then| {
        when.method(GET).path(format!("/monthly_xml_{next}.xml"));
        then.status(200).body(&body);
    });
    mock_all_missing(&server);

    let updater = updater(&server, &cache);
    let rates = updater.refreshed().unwrap();
    assert_eq!(
        rates.monthly_rate("USD", next).unwrap().units_per_gbp(),
        dec!(9.9999)
    );
    next_mock.assert_hits(1);

    // The response body was cached byte-for-byte under the upstream name.
    let cached = std::fs::read(cache.path().join(format!("monthly_xml_{next}.xml"))).unwrap();
    assert_eq!(cached, body.as_bytes());
}

#[test]
fn second_refresh_within_ttl_hits_cache_not_network() {
    let server = MockServer::start();
    let cache = tempfile::tempdir().unwrap();
    let next = next_month();

    let next_mock = server.mock(|when, then| {
        when.method(GET).path(format!("/monthly_xml_{next}.xml"));
        then.status(200).body(monthly_xml(next, "8.8888"));
    });
    mock_all_missing(&server);

    let updater = updater(&server, &cache);
    updater.refreshed().unwrap();
    let again = updater.refreshed().unwrap();
    assert_eq!(
        again.monthly_rate("USD", next).unwrap().units_per_gbp(),
        dec!(8.8888)
    );
    next_mock.assert_hits(1); // second run served from the fresh cache
}

#[test]
fn stale_amendable_cache_is_refetched_and_replaced() {
    let server = MockServer::start();
    let cache = tempfile::tempdir().unwrap();
    let next = next_month();
    let name = format!("monthly_xml_{next}.xml");

    // A stale cached copy of an amendable month...
    let path = cache.path().join(&name);
    std::fs::write(&path, monthly_xml(next, "7.0001")).unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(25 * 60 * 60);
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(old))
        .unwrap();

    // ...and an amended upstream file.
    let amended = server.mock(|when, then| {
        when.method(GET).path(format!("/{name}"));
        then.status(200).body(monthly_xml(next, "7.0002"));
    });
    mock_all_missing(&server);

    let rates = updater(&server, &cache).refreshed().unwrap();
    assert_eq!(
        rates.monthly_rate("USD", next).unwrap().units_per_gbp(),
        dec!(7.0002)
    );
    amended.assert_hits(1);
    // Cache now holds the amended copy.
    let cached = std::fs::read(&path).unwrap();
    assert!(String::from_utf8(cached).unwrap().contains("7.0002"));
}

#[test]
fn nothing_published_is_not_an_error() {
    let server = MockServer::start();
    let cache = tempfile::tempdir().unwrap();
    mock_all_missing(&server);

    let rates = updater(&server, &cache).refreshed().unwrap();
    let bundled = Rates::new();
    assert_eq!(
        rates.months().next_back().unwrap(),
        bundled.months().next_back().unwrap()
    );
}

#[test]
fn malformed_body_is_bad_data() {
    let server = MockServer::start();
    let cache = tempfile::tempdir().unwrap();
    server.mock(|when, then| {
        when.any_request();
        then.status(200).body("<html>maintenance page</html>");
    });

    let err = updater(&server, &cache).refreshed().unwrap_err();
    let message = err.to_string();
    assert!(message.contains("malformed"), "{message}");
    // Nothing bogus was cached.
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);
}

#[test]
fn corrupt_cache_files_are_ignored_and_refetched() {
    let server = MockServer::start();
    let cache = tempfile::tempdir().unwrap();
    let next = next_month();
    let name = format!("monthly_xml_{next}.xml");
    std::fs::write(cache.path().join(&name), "not xml at all").unwrap();
    // Corrupt + amendable: cached() ignores it entirely.
    let updater_offline = Updater::new().with_cache_dir(cache.path());
    let rates = updater_offline.cached();
    assert!(rates.monthly_rate("USD", next).is_err());

    // refreshed() replaces it: even a fresh-mtime corrupt file for an
    // amendable month gets refetched because parsing failed at apply time.
    server.mock(|when, then| {
        when.method(GET).path(format!("/{name}"));
        then.status(200).body(monthly_xml(next, "6.5432"));
    });
    mock_all_missing(&server);
    let rates = updater(&server, &cache).refreshed().unwrap();
    assert_eq!(
        rates.monthly_rate("USD", next).unwrap().units_per_gbp(),
        dec!(6.5432)
    );
}

#[test]
fn cached_works_fully_offline() {
    let cache = tempfile::tempdir().unwrap();
    let next = next_month();
    std::fs::write(
        cache.path().join(format!("monthly_xml_{next}.xml")),
        monthly_xml(next, "5.5555"),
    )
    .unwrap();

    // No server involved at all.
    let updater = Updater::new().with_cache_dir(cache.path());
    let rates = updater.cached();
    assert_eq!(
        rates.monthly_rate("USD", next).unwrap().units_per_gbp(),
        dec!(5.5555)
    );
    // Bundled data still present underneath.
    assert!(
        rates
            .monthly_rate("USD", Month::new(2025, 8).unwrap())
            .is_ok()
    );
}

/// Live smoke test against the real endpoint; run manually or from the cron
/// workflow with `cargo test --features http -- --ignored`.
#[test]
#[ignore]
fn live_endpoint_smoke() {
    let cache = tempfile::tempdir().unwrap();
    let updater = Updater::new().with_cache_dir(cache.path());
    let rates = updater.refreshed().unwrap();
    let current = Month::from(Utc::now().date_naive());
    assert!(rates.monthly_rate("USD", current).is_ok());
}
