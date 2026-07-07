use alloc::vec::Vec;

use chrono::NaiveDate;
use rust_decimal::Decimal;

use crate::date;
use crate::error::LookupError;
use crate::rate::Rate;
use crate::store::{self, Entry, Series, Weeks};
use crate::types::{Currency, Month, Period, RateType, YearEnd};

// chrono counts day 1 = 0001-01-01; our day 0 = 1970-01-01.
const CE_EPOCH_OFFSET: i32 = 719_163;

fn date_to_day(date: NaiveDate) -> i32 {
    use chrono::Datelike;
    date::days_from_civil(date.year(), date.month(), date.day())
}

fn day_to_date(day: i32) -> Option<NaiveDate> {
    NaiveDate::from_num_days_from_ce_opt(day.checked_add(CE_EPOCH_OFFSET)?)
}

/// All HMRC rate tables: bundled data plus (with the `http` feature) fetched periods.
///
/// `Send + Sync`; cloning is cheap — bundled data is shared statics.
/// Start with [`Rates::new`].
#[derive(Clone)]
pub struct Rates {
    monthly: Series,
    spot: Series,
    average: Series,
    weeks: Weeks,
}

impl core::fmt::Debug for Rates {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Rates")
            .field("months", &self.monthly.keys().len())
            .field("spot_periods", &self.spot.keys().len())
            .field("average_periods", &self.average.keys().len())
            .field("weeks", &self.weeks.index().len())
            .finish()
    }
}

#[cfg(feature = "bundled")]
impl Default for Rates {
    fn default() -> Rates {
        Rates::new()
    }
}

impl Rates {
    /// The bundled dataset. Infallible and effectively free: the tables live
    /// in the binary's read-only data, nothing is parsed or allocated.
    ///
    /// # Examples
    ///
    /// ```
    /// use hmrc_rates::Rates;
    ///
    /// let rates = Rates::new();
    /// assert!(rates.months().count() > 100);
    /// ```
    #[cfg(feature = "bundled")]
    pub fn new() -> Rates {
        Rates {
            monthly: Series::new(crate::bundled::MONTHLY),
            spot: Series::new(crate::bundled::SPOT),
            average: Series::new(crate::bundled::AVERAGE),
            weeks: Weeks::new(crate::bundled::WEEKLY),
        }
    }

    /// A `Rates` with no data at all.
    #[cfg(test)]
    pub(crate) fn empty() -> Rates {
        Rates {
            monthly: Series::new(store::EMPTY_SERIES),
            spot: Series::new(store::EMPTY_SERIES),
            average: Series::new(store::EMPTY_SERIES),
            weeks: Weeks::new(store::EMPTY_WEEKS),
        }
    }

    #[cfg(feature = "http")]
    pub(crate) fn set_period(&mut self, table: RateType, key: i32, entries: Vec<Entry>) {
        match table {
            RateType::Monthly => self.monthly.set(key, entries),
            RateType::Spot => self.spot.set(key, entries),
            RateType::Average => self.average.set(key, entries),
            _ => {}
        }
    }

    /// The monthly rate for `code`, strictly for that month.
    ///
    /// Accepts anything convertible to [`Month`], including `chrono::NaiveDate`.
    /// `"GBP"` (any case) returns the identity rate for any month.
    ///
    /// # Examples
    ///
    /// ```
    /// use hmrc_rates::Rates;
    /// use rust_decimal::Decimal;
    ///
    /// let rates = Rates::new();
    /// let date = chrono::NaiveDate::from_ymd_opt(2025, 8, 15).unwrap();
    /// let rate = rates.monthly_rate("USD", date)?;
    /// let gbp = rate.to_gbp(Decimal::from(100));
    /// # Ok::<(), hmrc_rates::LookupError>(())
    /// ```
    pub fn monthly_rate(&self, code: &str, month: impl Into<Month>) -> Result<Rate, LookupError> {
        let month = month.into();
        // £1 = £1 holds for every month, published or not.
        if Currency::normalize(code) == Some(Currency::GBP.code()) {
            return Ok(Rate::new(Decimal::ONE, Currency::GBP, Period::Month(month)));
        }
        self.monthly(month)?.rate(code)
    }

    /// Like [`Rates::monthly_rate`], but walks back to the nearest earlier
    /// published month, at most `max_months_back` steps.
    ///
    /// This is the crate's only fallback, and it is opt-in.
    /// [`Rate::period`] reveals which month was actually used.
    ///
    /// # Examples
    ///
    /// ```
    /// use hmrc_rates::{Period, Rates};
    ///
    /// let rates = Rates::new();
    /// let next = rates.months().next_back().unwrap().next(); // not published yet
    /// assert!(rates.monthly_rate("USD", next).is_err()); // strict lookup fails
    /// let rate = rates.monthly_rate_or_earlier("USD", next, 1)?;
    /// assert_ne!(rate.period(), Period::Month(next)); // the substitution is visible
    /// # Ok::<(), hmrc_rates::LookupError>(())
    /// ```
    pub fn monthly_rate_or_earlier(
        &self,
        code: &str,
        month: impl Into<Month>,
        max_months_back: u32,
    ) -> Result<Rate, LookupError> {
        let requested = month.into();
        let mut candidate = requested;
        for _ in 0..=max_months_back {
            if self.monthly.table(candidate.key()).is_some() {
                return self.monthly(candidate)?.rate(code);
            }
            candidate = candidate.prev();
        }
        if let Some(code) = Currency::normalize(code) {
            if code == Currency::GBP.code() {
                return Ok(Rate::new(
                    Decimal::ONE,
                    Currency::GBP,
                    Period::Month(requested),
                ));
            }
        }
        Err(self.period_missing(RateType::Monthly, Period::Month(requested)))
    }

    /// The whole monthly table for one month.
    pub fn monthly(&self, month: impl Into<Month>) -> Result<Table<'_>, LookupError> {
        let month = month.into();
        let period = Period::Month(month);
        match self.monthly.table(month.key()) {
            Some(entries) => Ok(Table {
                rate_type: RateType::Monthly,
                period,
                entries,
                known: Known::Series(&self.monthly),
            }),
            None => Err(self.period_missing(RateType::Monthly, period)),
        }
    }

    /// The spot table for a 31 March / 31 December period.
    ///
    /// # Examples
    ///
    /// ```
    /// use hmrc_rates::{Rates, YearEnd};
    ///
    /// let rates = Rates::new();
    /// let usd = rates.spot(YearEnd::december(2024))?.rate("USD")?;
    /// # Ok::<(), hmrc_rates::LookupError>(())
    /// ```
    pub fn spot(&self, period: YearEnd) -> Result<Table<'_>, LookupError> {
        Self::year_end_table(&self.spot, RateType::Spot, period)
    }

    /// The yearly-average table for a 31 March / 31 December period.
    ///
    /// # Examples
    ///
    /// ```
    /// use hmrc_rates::{Rates, YearEnd};
    ///
    /// let rates = Rates::new();
    /// // Self Assessment style: the average for the year to 31 March 2025.
    /// let eur = rates.average(YearEnd::march(2025))?.rate("EUR")?;
    /// # Ok::<(), hmrc_rates::LookupError>(())
    /// ```
    pub fn average(&self, period: YearEnd) -> Result<Table<'_>, LookupError> {
        Self::year_end_table(&self.average, RateType::Average, period)
    }

    /// The weekly-amendment table whose validity range contains `date`.
    ///
    /// Weekly files list only the currencies HMRC amended that week
    /// (series ran 2014-01 to 2016-04, then was discontinued).
    ///
    /// # Examples
    ///
    /// ```
    /// use hmrc_rates::Rates;
    ///
    /// let rates = Rates::new();
    /// let date = chrono::NaiveDate::from_ymd_opt(2014, 1, 10).unwrap();
    /// let lira = rates.weekly(date)?.rate("TRY")?;
    /// # Ok::<(), hmrc_rates::LookupError>(())
    /// ```
    pub fn weekly(&self, date: NaiveDate) -> Result<Table<'_>, LookupError> {
        let day = date_to_day(date);
        if let Some((week, entries)) = self.weeks.containing(day) {
            if let (Some(start), Some(end)) =
                (day_to_date(week.start_day), day_to_date(week.end_day))
            {
                return Ok(Table {
                    rate_type: RateType::Weekly,
                    period: Period::Week { start, end },
                    entries,
                    known: Known::Weeks(&self.weeks),
                });
            }
        }
        let available = {
            let idx = self.weeks.index();
            match (idx.first(), idx.last()) {
                (Some(first), Some(last)) => {
                    match (
                        day_to_date(first.start_day),
                        day_to_date(first.end_day),
                        day_to_date(last.start_day),
                        day_to_date(last.end_day),
                    ) {
                        (Some(fs), Some(fe), Some(ls), Some(le)) => Some((
                            Period::Week { start: fs, end: fe },
                            Period::Week { start: ls, end: le },
                        )),
                        _ => None,
                    }
                }
                _ => None,
            }
        };
        Err(LookupError::PeriodNotAvailable {
            table: RateType::Weekly,
            period: Period::Week {
                start: date,
                end: date,
            },
            available,
        })
    }

    /// All published months, ascending.
    pub fn months(&self) -> impl DoubleEndedIterator<Item = Month> + use<'_> {
        self.monthly.keys().into_iter().map(Month::from_key)
    }

    /// All published spot periods, ascending.
    pub fn spot_periods(&self) -> impl DoubleEndedIterator<Item = YearEnd> + use<'_> {
        self.spot.keys().into_iter().map(YearEnd::from_key)
    }

    /// All published yearly-average periods, ascending.
    pub fn average_periods(&self) -> impl DoubleEndedIterator<Item = YearEnd> + use<'_> {
        self.average.keys().into_iter().map(YearEnd::from_key)
    }

    /// All weekly-amendment validity ranges, ascending, as [`Period::Week`] items.
    pub fn weeks(&self) -> impl DoubleEndedIterator<Item = Period> + use<'_> {
        self.weeks.index().iter().filter_map(|w| {
            Some(Period::Week {
                start: day_to_date(w.start_day)?,
                end: day_to_date(w.end_day)?,
            })
        })
    }

    /// Every currency that appears anywhere in the given series, ascending.
    pub fn currencies(&self, table: RateType) -> impl Iterator<Item = Currency> + use<'_> {
        let codes = match table {
            RateType::Monthly => self.monthly.codes(),
            RateType::Spot => self.spot.codes(),
            RateType::Average => self.average.codes(),
            RateType::Weekly => self.weekly_codes(),
        };
        codes.into_iter().map(Currency::from_code)
    }

    fn weekly_codes(&self) -> Vec<[u8; 3]> {
        let mut codes: Vec<[u8; 3]> = self.weeks.arena().iter().map(|e| e.code).collect();
        codes.sort_unstable();
        codes.dedup();
        codes
    }

    fn year_end_table<'a>(
        series: &'a Series,
        rate_type: RateType,
        period: YearEnd,
    ) -> Result<Table<'a>, LookupError> {
        match series.table(period.key()) {
            Some(entries) => Ok(Table {
                rate_type,
                period: Period::YearEnd(period),
                entries,
                known: Known::Series(series),
            }),
            None => Err(LookupError::PeriodNotAvailable {
                table: rate_type,
                period: Period::YearEnd(period),
                available: series.first_last().map(|(f, l)| {
                    (
                        Period::YearEnd(YearEnd::from_key(f)),
                        Period::YearEnd(YearEnd::from_key(l)),
                    )
                }),
            }),
        }
    }

    fn period_missing(&self, table: RateType, period: Period) -> LookupError {
        let available = self.monthly.first_last().map(|(f, l)| {
            (
                Period::Month(Month::from_key(f)),
                Period::Month(Month::from_key(l)),
            )
        });
        LookupError::PeriodNotAvailable {
            table,
            period,
            available,
        }
    }
}

#[derive(Copy, Clone)]
enum Known<'a> {
    Series(&'a Series),
    Weeks(&'a Weeks),
}

impl Known<'_> {
    fn knows(&self, code: [u8; 3]) -> bool {
        match self {
            Known::Series(s) => s.knows(code),
            Known::Weeks(w) => w.knows(code),
        }
    }
}

/// A borrowed view of one period's table — resolve once, convert many times.
#[derive(Copy, Clone)]
pub struct Table<'a> {
    rate_type: RateType,
    period: Period,
    entries: &'a [Entry],
    known: Known<'a>,
}

impl core::fmt::Debug for Table<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Table")
            .field("rate_type", &self.rate_type)
            .field("period", &self.period)
            .field("len", &self.entries.len())
            .finish()
    }
}

impl<'a> Table<'a> {
    /// The period this table was published for.
    pub fn period(&self) -> Period {
        self.period
    }

    /// The series this table belongs to.
    pub fn rate_type(&self) -> RateType {
        self.rate_type
    }

    /// The rate for `code` in this period. `"GBP"` always resolves to the identity rate.
    ///
    /// Errors distinguish a code the series has never published
    /// ([`LookupError::UnknownCurrency`]) from one merely absent this period
    /// ([`LookupError::NotInPeriod`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use hmrc_rates::{Month, Rates};
    /// use rust_decimal::Decimal;
    ///
    /// let rates = Rates::new();
    /// let table = rates.monthly(Month::new(2025, 8).unwrap())?;
    /// let eur = table.rate("EUR")?;
    /// let total: Decimal = [1200, 450, 80]
    ///     .into_iter()
    ///     .map(|amount| eur.to_gbp(Decimal::from(amount)))
    ///     .sum();
    /// # Ok::<(), hmrc_rates::LookupError>(())
    /// ```
    pub fn rate(&self, code: &str) -> Result<Rate, LookupError> {
        let Some(normalized) = Currency::normalize(code) else {
            return Err(LookupError::UnknownCurrency {
                code: code.trim().into(),
                table: self.rate_type,
            });
        };
        if normalized == Currency::GBP.code() {
            return Ok(Rate::new(Decimal::ONE, Currency::GBP, self.period));
        }
        match store::lookup(self.entries, normalized) {
            Some(entry) => Ok(Rate::new(
                entry.decimal(),
                Currency::from_code(normalized),
                self.period,
            )),
            None if self.known.knows(normalized) => Err(LookupError::NotInPeriod {
                currency: Currency::from_code(normalized),
                table: self.rate_type,
                period: self.period,
            }),
            None => Err(LookupError::UnknownCurrency {
                code: code.trim().into(),
                table: self.rate_type,
            }),
        }
    }

    /// Like [`Table::rate`] but `None` on any miss, for when absence isn't exceptional.
    pub fn get(&self, code: &str) -> Option<Rate> {
        self.rate(code).ok()
    }

    /// All `(currency, rate)` pairs in this table, ascending by code.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (Currency, Rate)> + use<'a> {
        let (rate_type, period) = (self.rate_type, self.period);
        let _ = rate_type;
        self.entries.iter().map(move |e| {
            let currency = Currency::from_code(e.code);
            (currency, Rate::new(e.decimal(), currency, period))
        })
    }

    /// The number of currencies in this table.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` if the table has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod empty_tests {
    use super::*;

    #[test]
    fn empty_rates_report_no_data_loaded() {
        let rates = Rates::empty();
        let month = Month::new(2025, 8);
        let Some(month) = month else { return };
        let result = rates.monthly_rate("USD", month);
        assert!(
            matches!(
                result,
                Err(LookupError::PeriodNotAvailable {
                    available: None,
                    ..
                })
            ),
            "unexpected: {result:?}"
        );
        assert!(rates.months().next().is_none());
        assert_eq!(rates.currencies(RateType::Spot).count(), 0);
        // GBP identity still holds with no data at all.
        assert!(rates.monthly_rate("GBP", month).is_ok());
    }
}

#[cfg(all(test, feature = "bundled"))]
#[allow(clippy::unwrap_used)]
mod bundled_tests {
    use super::*;

    #[test]
    fn statics_hold_codegen_invariants() {
        for series in [
            &crate::bundled::MONTHLY,
            &crate::bundled::SPOT,
            &crate::bundled::AVERAGE,
        ] {
            let mut start = 0usize;
            for pair in series.index.windows(2) {
                assert!(
                    pair[0].key < pair[1].key,
                    "index keys not strictly ascending"
                );
            }
            for idx in series.index {
                let table = &series.arena[start..idx.end as usize];
                start = idx.end as usize;
                assert!(!table.is_empty());
                for entry in table {
                    assert!(entry.mantissa > 0);
                    assert!(entry.scale <= 9);
                    assert!(entry.code.iter().all(u8::is_ascii_uppercase));
                }
                for pair in table.windows(2) {
                    assert!(pair[0].code < pair[1].code, "codes not sorted/deduped");
                }
            }
            assert_eq!(start, series.arena.len(), "index does not cover the arena");
        }
        for pair in crate::bundled::WEEKLY.index.windows(2) {
            assert!(pair[0].end_day < pair[1].start_day, "overlapping weeks");
        }
    }

    // Round-trip: the generated statics must match a fresh parse of the source file.
    #[cfg(feature = "std")]
    #[test]
    fn codegen_matches_fresh_parse() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/data/monthly/2025-08.xml");
        let bytes = std::fs::read(path).unwrap();
        let ((year, month), raw) = crate::parse::parse_monthly_xml(&bytes).unwrap();
        let parsed = crate::parse::dedup_majority(raw).unwrap();

        let rates = Rates::new();
        let table = rates.monthly(Month::new(year, month).unwrap()).unwrap();
        assert_eq!(table.len(), parsed.len());
        for rate in &parsed {
            let entry = crate::store::lookup(table.entries, rate.code).unwrap();
            assert_eq!((entry.mantissa, entry.scale), (rate.mantissa, rate.scale));
        }
    }
}
