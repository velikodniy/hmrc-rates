use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use chrono::Datelike;

use crate::parse::{self, ParsedRate};
use crate::rates::Rates;
use crate::store::Entry;
use crate::types::{Month, RateType, YearEnd};

const DEFAULT_BASE_URL: &str =
    "https://www.trade-tariff.service.gov.uk/api/v2/exchange_rates/files";
const USER_AGENT: &str = concat!(
    "hmrc-rates/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/velikodniy/hmrc-rates)"
);
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Why fetching fresh rates failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FetchError {
    /// The request failed at the transport or HTTP level.
    #[error("HTTP request to HMRC failed: {0}")]
    Http(#[from] Box<ureq::Error>),
    /// The endpoint answered, but the payload failed validation.
    #[error("HMRC returned malformed data from {url}: {reason}")]
    BadData { url: String, reason: String },
}

/// Extends the bundled dataset with rates HMRC has published since the crate
/// release, through a verbatim-file disk cache in the system cache directory.
///
/// Past periods never change and are served from disk forever; the current and
/// next month (plus a just-published spot/average period) are re-fetched after
/// a 24-hour TTL to pick up HMRC's rare in-month amendments.
///
/// See [`Updater::refreshed`] for the recommended usage pattern.
pub struct Updater {
    agent: ureq::Agent,
    base_url: String,
    cache_dir: Option<PathBuf>,
}

impl Default for Updater {
    fn default() -> Updater {
        Updater::new()
    }
}

impl Updater {
    /// Infallible: if no system cache directory can be determined, the updater
    /// works without a cache.
    pub fn new() -> Updater {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(USER_AGENT)
            .build()
            .into();
        Updater {
            agent,
            base_url: DEFAULT_BASE_URL.into(),
            cache_dir: default_cache_dir(),
        }
    }

    /// Overrides the cache directory chosen by [`Updater::new`].
    pub fn with_cache_dir(mut self, dir: impl Into<PathBuf>) -> Updater {
        self.cache_dir = Some(dir.into());
        self
    }

    /// Overrides the endpoint — for mirrors and tests.
    pub fn with_base_url(mut self, url: impl Into<String>) -> Updater {
        self.base_url = url.into();
        self
    }

    /// Bundled data plus whatever the disk cache holds. Never touches the
    /// network; unreadable or corrupt cache files are treated as absent.
    pub fn cached(&self) -> Rates {
        let mut rates = Rates::new();
        self.apply_cache(&mut rates);
        rates
    }

    /// Bundled ∪ cache ∪ network: fetches every period that could have been
    /// published since, treating 404 as "not published yet". Blocking.
    ///
    /// On error, fall back explicitly — stale rates in a tax tool should be
    /// a visible choice, not a silent default.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use hmrc_rates::Updater;
    ///
    /// let updater = Updater::new();
    /// let rates = updater.refreshed().unwrap_or_else(|e| {
    ///     eprintln!("warning: possibly stale rates: {e}");
    ///     updater.cached()
    /// });
    /// ```
    pub fn refreshed(&self) -> Result<Rates, FetchError> {
        let mut rates = Rates::new();
        self.apply_cache(&mut rates);

        let today = chrono::Utc::now().date_naive();
        let current = Month::from(today);

        // Monthly: from the first month we lack — a transient 404 must not
        // leave a permanent gap — through next month (HMRC pre-publishes);
        // current and next month may still be amended.
        let first_missing = first_gap(rates.months(), Month::next).unwrap_or(current);
        let mut candidate = first_missing.min(current);
        while candidate <= current.next() {
            let amendable = candidate >= current;
            if !amendable && rates.monthly(candidate).is_ok() {
                candidate = candidate.next();
                continue; // already have an immutable copy
            }
            let name = format!("monthly_xml_{candidate}.xml");
            let entries = self.obtain(&name, amendable, |bytes| {
                validated_monthly(bytes, candidate)
            })?;
            if let Some(entries) = entries {
                rates.set_period(RateType::Monthly, candidate.key(), entries);
            }
            candidate = candidate.next();
        }

        // Spot and average: every 31 Mar / 31 Dec period we lack once it's
        // due; the freshest one stays amendable for 60 days past its end.
        for (rate_type, prefix) in [(RateType::Spot, "spot"), (RateType::Average, "average")] {
            // Snapshot is safe: the loop never revisits a period it sets.
            let periods: Vec<YearEnd> = match rate_type {
                RateType::Spot => rates.spot_periods().collect(),
                _ => rates.average_periods().collect(),
            };
            let Some(first_missing) = first_gap(periods.iter().copied(), next_year_end) else {
                continue;
            };
            // Start no later than the newest period: it may still be amended.
            let mut period = periods
                .last()
                .map_or(first_missing, |n| first_missing.min(*n));
            loop {
                let end = period.end_month();
                if Month::from(today) < end {
                    break; // not due yet
                }
                let name = format!("{prefix}_csv_{}-{:02}.csv", period.year(), end.month());
                let days_past_end =
                    today.num_days_from_ce() - end_of_month(period).num_days_from_ce();
                let amendable = (0..=60).contains(&days_past_end);
                let have = periods.binary_search(&period).is_ok();
                if !have || amendable {
                    let entries = self.obtain(&name, amendable, |bytes| {
                        dedup(parse::parse_rates_csv(bytes)?)
                    })?;
                    if let Some(entries) = entries {
                        rates.set_period(rate_type, period.key(), entries);
                    }
                }
                period = next_year_end(period);
            }
        }

        Ok(rates)
    }

    /// A validated value from cache (when fresh) or network; `None` = 404,
    /// i.e. not published yet. Corrupt cache falls through to the network;
    /// only network-validated bytes are cached.
    fn obtain<T>(
        &self,
        name: &str,
        amendable: bool,
        validate: impl Fn(&[u8]) -> Result<T, parse::ParseError>,
    ) -> Result<Option<T>, FetchError> {
        if let Some(bytes) = self.fresh_cache_bytes(name, amendable) {
            if let Ok(value) = validate(&bytes) {
                return Ok(Some(value));
            }
        }
        let url = format!("{}/{}", self.base_url, name);
        let bytes = match self.agent.get(&url).call() {
            Ok(mut response) => response
                .body_mut()
                .read_to_vec()
                .map_err(|e| FetchError::Http(Box::new(e)))?,
            Err(ureq::Error::StatusCode(404)) => return Ok(None),
            Err(e) => return Err(FetchError::Http(Box::new(e))),
        };
        let value = validate(&bytes).map_err(|e| self.bad_data(name, e))?;
        self.store(name, &bytes);
        Ok(Some(value))
    }

    fn fresh_cache_bytes(&self, name: &str, amendable: bool) -> Option<Vec<u8>> {
        let path = self.cache_path(name)?;
        let metadata = fs::metadata(&path).ok()?;
        let fresh = !amendable
            || metadata
                .modified()
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age < CACHE_TTL);
        if !fresh {
            return None;
        }
        fs::read(&path).ok()
    }

    /// Applies every parseable cache file; failures are silently skipped.
    fn apply_cache(&self, rates: &mut Rates) {
        let Some(dir) = self.cache_dir.as_deref() else {
            return;
        };
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        files.sort();
        for path in files {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Ok(bytes) = fs::read(&path) else { continue };
            self.apply_file(rates, name, &bytes);
        }
    }

    fn apply_file(&self, rates: &mut Rates, name: &str, bytes: &[u8]) -> Option<()> {
        if let Some(rest) = name
            .strip_prefix("monthly_xml_")
            .and_then(|r| r.strip_suffix(".xml"))
        {
            let month: Month = rest.parse().ok()?;
            let entries = validated_monthly(bytes, month).ok()?;
            rates.set_period(RateType::Monthly, month.key(), entries);
            return Some(());
        }
        for (rate_type, prefix) in [
            (RateType::Spot, "spot_csv_"),
            (RateType::Average, "average_csv_"),
        ] {
            if let Some(rest) = name
                .strip_prefix(prefix)
                .and_then(|r| r.strip_suffix(".csv"))
            {
                let year_end = YearEnd::from_month(rest.parse().ok()?)?;
                let entries = dedup(parse::parse_rates_csv(bytes).ok()?).ok()?;
                rates.set_period(rate_type, year_end.key(), entries);
                return Some(());
            }
        }
        None
    }

    /// Atomic best-effort cache write; failure only costs a refetch next run.
    fn store(&self, name: &str, bytes: &[u8]) {
        let Some(path) = self.cache_path(name) else {
            return;
        };
        let Some(dir) = path.parent() else { return };
        if fs::create_dir_all(dir).is_err() {
            return;
        }
        // Unique tmp name: concurrent writers must not truncate each other.
        static TMP_SEQ: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
        let seq = TMP_SEQ.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let tmp = dir.join(format!(".{name}.{}.{seq}.tmp", std::process::id()));
        if fs::write(&tmp, bytes).is_ok() && fs::rename(&tmp, &path).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    }

    fn cache_path(&self, name: &str) -> Option<PathBuf> {
        Some(self.cache_dir.as_deref()?.join(name))
    }

    fn bad_data(&self, name: &str, e: parse::ParseError) -> FetchError {
        FetchError::BadData {
            url: format!("{}/{}", self.base_url, name),
            reason: e.to_string(),
        }
    }
}

fn default_cache_dir() -> Option<PathBuf> {
    use etcetera::BaseStrategy;
    let strategy = etcetera::choose_base_strategy().ok()?;
    Some(strategy.cache_dir().join("hmrc-rates").join("v1"))
}

fn dedup(raw: Vec<ParsedRate>) -> Result<Vec<Entry>, parse::ParseError> {
    Ok(parse::dedup_majority(raw)?
        .into_iter()
        .map(|r| Entry {
            mantissa: r.mantissa,
            code: r.code,
            scale: r.scale,
        })
        .collect())
}

/// Parse, period-check and dedup one monthly XML payload.
fn validated_monthly(bytes: &[u8], expected: Month) -> Result<Vec<Entry>, parse::ParseError> {
    let ((y, m), raw) = parse::parse_monthly_xml(bytes)?;
    if Month::new(y, m) != Some(expected) {
        return Err(parse::ParseError("period mismatch".into()));
    }
    dedup(raw)
}

/// The first period missing from an ascending run, or `None` for an empty series.
fn first_gap<T: Copy + PartialEq>(
    mut periods: impl Iterator<Item = T>,
    next: impl Fn(T) -> T,
) -> Option<T> {
    let mut expected = next(periods.next()?);
    for period in periods {
        if period != expected {
            break;
        }
        expected = next(period);
    }
    Some(expected)
}

fn next_year_end(period: YearEnd) -> YearEnd {
    if period.is_march() {
        YearEnd::december(period.year())
    } else {
        YearEnd::march(period.year() + 1)
    }
}

fn end_of_month(period: YearEnd) -> chrono::NaiveDate {
    let month = period.end_month();
    let last_day = parse::date::days_in_month(period.year(), month.month());
    chrono::NaiveDate::from_ymd_opt(period.year(), month.month(), last_day).unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn first_gap_resumes_at_the_first_missing_period() {
        let months = |keys: &[i32]| keys.iter().map(|k| Month::from_key(*k)).collect::<Vec<_>>();
        // Contiguous run: the gap is right after the end.
        let run = months(&[10, 11, 12]);
        assert_eq!(
            first_gap(run.into_iter(), Month::next),
            Some(Month::from_key(13))
        );
        // A hole in the middle must win over the newest period.
        let holed = months(&[10, 11, 14]);
        assert_eq!(
            first_gap(holed.into_iter(), Month::next),
            Some(Month::from_key(12))
        );
        // Empty series has no gap to resume from.
        assert_eq!(first_gap(core::iter::empty::<Month>(), Month::next), None);
    }

    #[test]
    fn year_end_sequence_alternates() {
        let periods = [
            YearEnd::march(2025),
            YearEnd::december(2025),
            YearEnd::march(2026),
        ];
        assert_eq!(
            first_gap(periods.into_iter(), next_year_end),
            Some(YearEnd::december(2026))
        );
        let gapped = [YearEnd::march(2025), YearEnd::march(2026)];
        assert_eq!(
            first_gap(gapped.into_iter(), next_year_end),
            Some(YearEnd::december(2025))
        );
    }
}
