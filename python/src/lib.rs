//! Python bindings for the `hmrc-rates` crate.
//!
//! Mirrors the Rust API: strict lookups return typed exceptions, money stays
//! exact as `decimal.Decimal`, and the optional [`PyUpdater`] fetches newly
//! published rates with the GIL released.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::NaiveDate;
use pyo3::Borrowed;
use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyOverflowError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyFloat;
use rust_decimal::Decimal;

create_exception!(
    hmrc_rates,
    HmrcRatesError,
    PyException,
    "Base class for all hmrc-rates errors."
);
create_exception!(
    hmrc_rates,
    UnknownCurrencyError,
    HmrcRatesError,
    "The currency code never appears in the requested rate series."
);
create_exception!(
    hmrc_rates,
    PeriodNotAvailableError,
    HmrcRatesError,
    "The series has no table for the requested period."
);
create_exception!(
    hmrc_rates,
    NotInPeriodError,
    HmrcRatesError,
    "The period exists but the currency is absent from it."
);
create_exception!(
    hmrc_rates,
    FetchError,
    HmrcRatesError,
    "Fetching fresh rates from HMRC failed."
);

fn lookup_err(e: hmrc_rates::LookupError) -> PyErr {
    use hmrc_rates::LookupError as E;
    let msg = e.to_string();
    match e {
        E::UnknownCurrency { .. } => UnknownCurrencyError::new_err(msg),
        E::PeriodNotAvailable { .. } => PeriodNotAvailableError::new_err(msg),
        E::NotInPeriod { .. } => NotInPeriodError::new_err(msg),
        _ => HmrcRatesError::new_err(msg),
    }
}

fn fetch_err(e: hmrc_rates::FetchError) -> PyErr {
    FetchError::new_err(e.to_string())
}

/// A calendar month, the key of HMRC monthly rate tables.
#[pyclass(
    name = "YearMonth",
    module = "hmrc_rates",
    frozen,
    eq,
    ord,
    hash,
    str,
    from_py_object
)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PyYearMonth(hmrc_rates::YearMonth);

impl std::fmt::Display for PyYearMonth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[pymethods]
impl PyYearMonth {
    #[new]
    fn new(year: i32, month: u32) -> PyResult<Self> {
        hmrc_rates::YearMonth::new(year, month)
            .map(PyYearMonth)
            .ok_or_else(|| {
                PyValueError::new_err(format!("invalid month: year={year}, month={month}"))
            })
    }

    /// Parses the `"YYYY-MM"` display form.
    #[staticmethod]
    fn parse(s: &str) -> PyResult<Self> {
        s.parse()
            .map(PyYearMonth)
            .map_err(|e: hmrc_rates::ParseYearMonthError| PyValueError::new_err(e.to_string()))
    }

    /// The month a `datetime.date` falls in.
    #[staticmethod]
    fn from_date(date: NaiveDate) -> Self {
        PyYearMonth(date.into())
    }

    #[getter]
    fn year(&self) -> i32 {
        self.0.year()
    }

    #[getter]
    fn month(&self) -> u32 {
        self.0.month()
    }

    /// The following month (saturating).
    fn next(&self) -> Self {
        PyYearMonth(self.0.next())
    }

    /// The preceding month (saturating).
    fn prev(&self) -> Self {
        PyYearMonth(self.0.prev())
    }

    fn __repr__(&self) -> String {
        format!("YearMonth({}, {})", self.0.year(), self.0.month())
    }
}

/// A spot/average rate period ending 31 March or 31 December.
#[pyclass(
    name = "YearEnd",
    module = "hmrc_rates",
    frozen,
    eq,
    ord,
    hash,
    str,
    from_py_object
)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PyYearEnd(hmrc_rates::YearEnd);

impl std::fmt::Display for PyYearEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[pymethods]
impl PyYearEnd {
    /// The year ending 31 March of `year`.
    #[staticmethod]
    fn march(year: i32) -> Self {
        PyYearEnd(hmrc_rates::YearEnd::march(year))
    }

    /// The year ending 31 December of `year`.
    #[staticmethod]
    fn december(year: i32) -> Self {
        PyYearEnd(hmrc_rates::YearEnd::december(year))
    }

    /// The period ending in `year_month` — `None` unless it is a March or December.
    #[staticmethod]
    fn from_year_month(year_month: PyYearMonth) -> Option<Self> {
        hmrc_rates::YearEnd::from_year_month(year_month.0).map(PyYearEnd)
    }

    #[getter]
    fn year(&self) -> i32 {
        self.0.year()
    }

    #[getter]
    fn is_march(&self) -> bool {
        self.0.is_march()
    }

    /// The month the period ends in.
    fn end_year_month(&self) -> PyYearMonth {
        PyYearMonth(self.0.end_year_month())
    }

    fn __repr__(&self) -> String {
        let ctor = if self.0.is_march() {
            "march"
        } else {
            "december"
        };
        format!("YearEnd.{}({})", ctor, self.0.year())
    }
}

/// A three-letter currency code as published by HMRC (not always ISO 4217).
#[pyclass(
    name = "Currency",
    module = "hmrc_rates",
    frozen,
    eq,
    ord,
    hash,
    str,
    skip_from_py_object
)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PyCurrency(hmrc_rates::Currency);

impl std::fmt::Display for PyCurrency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[pymethods]
impl PyCurrency {
    /// Pound sterling, the base of every HMRC rate.
    #[classattr]
    #[allow(non_snake_case)]
    fn GBP() -> Self {
        PyCurrency(hmrc_rates::Currency::GBP)
    }

    /// The code as three uppercase ASCII letters.
    #[getter]
    fn code(&self) -> String {
        self.0.as_str().to_owned()
    }

    fn __repr__(&self) -> String {
        format!("Currency('{}')", self.0)
    }
}

/// The four rate series HMRC has published.
#[pyclass(
    name = "RateType",
    module = "hmrc_rates",
    frozen,
    eq,
    hash,
    from_py_object
)]
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
enum PyRateType {
    #[pyo3(name = "MONTHLY")]
    Monthly,
    #[pyo3(name = "SPOT")]
    Spot,
    #[pyo3(name = "AVERAGE")]
    Average,
    #[pyo3(name = "WEEKLY")]
    Weekly,
}

impl PyRateType {
    fn from_rust(rt: hmrc_rates::RateType) -> PyResult<Self> {
        use hmrc_rates::RateType as R;
        match rt {
            R::Monthly => Ok(PyRateType::Monthly),
            R::Spot => Ok(PyRateType::Spot),
            R::Average => Ok(PyRateType::Average),
            R::Weekly => Ok(PyRateType::Weekly),
            // The Rust enum is #[non_exhaustive]; lockstep versions make this unreachable.
            _ => Err(PyRuntimeError::new_err("unknown rate type")),
        }
    }

    fn to_rust(self) -> hmrc_rates::RateType {
        match self {
            PyRateType::Monthly => hmrc_rates::RateType::Monthly,
            PyRateType::Spot => hmrc_rates::RateType::Spot,
            PyRateType::Average => hmrc_rates::RateType::Average,
            PyRateType::Weekly => hmrc_rates::RateType::Weekly,
        }
    }
}

/// The period a rate or table applies to.
#[pyclass(
    name = "Period",
    module = "hmrc_rates",
    frozen,
    eq,
    hash,
    str,
    skip_from_py_object
)]
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
struct PyPeriod(hmrc_rates::Period);

impl std::fmt::Display for PyPeriod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[pymethods]
impl PyPeriod {
    /// `"year_month"`, `"year_end"` or `"week"`.
    #[getter]
    fn kind(&self) -> PyResult<&'static str> {
        match self.0 {
            hmrc_rates::Period::YearMonth(_) => Ok("year_month"),
            hmrc_rates::Period::YearEnd(_) => Ok("year_end"),
            hmrc_rates::Period::Week { .. } => Ok("week"),
            _ => Err(PyRuntimeError::new_err("unknown period kind")),
        }
    }

    #[getter]
    fn year_month(&self) -> Option<PyYearMonth> {
        match self.0 {
            hmrc_rates::Period::YearMonth(m) => Some(PyYearMonth(m)),
            _ => None,
        }
    }

    #[getter]
    fn year_end(&self) -> Option<PyYearEnd> {
        match self.0 {
            hmrc_rates::Period::YearEnd(ye) => Some(PyYearEnd(ye)),
            _ => None,
        }
    }

    #[getter]
    fn start(&self) -> Option<NaiveDate> {
        match self.0 {
            hmrc_rates::Period::Week { start, .. } => Some(start),
            _ => None,
        }
    }

    #[getter]
    fn end(&self) -> Option<NaiveDate> {
        match self.0 {
            hmrc_rates::Period::Week { end, .. } => Some(end),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        format!("Period('{}')", self.0)
    }
}

/// A month given as `YearMonth`, `datetime.date`, or a `"YYYY-MM"` string.
#[derive(FromPyObject)]
enum YearMonthArg {
    YearMonth(PyYearMonth),
    Date(NaiveDate),
    Str(String),
}

impl YearMonthArg {
    fn into_year_month(self) -> PyResult<hmrc_rates::YearMonth> {
        match self {
            YearMonthArg::YearMonth(m) => Ok(m.0),
            YearMonthArg::Date(d) => Ok(d.into()),
            YearMonthArg::Str(s) => s.parse().map_err(|_| {
                PyValueError::new_err(format!("invalid month '{s}', expected YYYY-MM"))
            }),
        }
    }
}

/// An exact amount: `decimal.Decimal` or `int`. Floats are rejected — they
/// would silently break the crate's exactness contract.
struct Amount(Decimal);

impl<'a, 'py> FromPyObject<'a, 'py> for Amount {
    type Error = PyErr;

    fn extract(ob: Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        if ob.is_instance_of::<PyFloat>() {
            return Err(PyTypeError::new_err(
                "amount must be decimal.Decimal or int, not float; use Decimal(str(x)) if intended",
            ));
        }
        if let Ok(d) = ob.extract::<Decimal>() {
            return Ok(Amount(d));
        }
        if let Ok(i) = ob.extract::<i128>() {
            return Decimal::try_from_i128_with_scale(i, 0)
                .map(Amount)
                .map_err(|_| PyOverflowError::new_err("integer amount out of Decimal range"));
        }
        Err(PyTypeError::new_err(
            "amount must be decimal.Decimal or int",
        ))
    }
}

/// A resolved rate: currency units per £1, exact `decimal.Decimal` arithmetic.
#[pyclass(name = "Rate", module = "hmrc_rates", frozen, eq, skip_from_py_object)]
#[derive(Copy, Clone, PartialEq, Eq)]
struct PyRate(hmrc_rates::Rate);

// to_gbp/from_gbp mirror the Rust API; pymethods cannot take self by value.
#[allow(clippy::wrong_self_convention)]
#[pymethods]
impl PyRate {
    /// The canonical HMRC figure: how many currency units £1 buys.
    #[getter]
    fn units_per_gbp(&self) -> Decimal {
        self.0.units_per_gbp()
    }

    #[getter]
    fn currency(&self) -> PyCurrency {
        PyCurrency(self.0.currency())
    }

    /// The period the rate was published for — reveals the substituted month
    /// after `Rates.monthly_rate_or_earlier`.
    #[getter]
    fn period(&self) -> PyPeriod {
        PyPeriod(self.0.period())
    }

    /// Converts an amount in this rate's currency to GBP (exact, unrounded).
    fn to_gbp(&self, amount: Amount) -> PyResult<Decimal> {
        amount
            .0
            .checked_div(self.0.units_per_gbp())
            .ok_or_else(|| PyOverflowError::new_err("decimal overflow in to_gbp"))
    }

    /// Converts a GBP amount to this rate's currency (exact, unrounded).
    fn from_gbp(&self, gbp: Amount) -> PyResult<Decimal> {
        gbp.0
            .checked_mul(self.0.units_per_gbp())
            .ok_or_else(|| PyOverflowError::new_err("decimal overflow in from_gbp"))
    }

    fn __repr__(&self) -> String {
        format!(
            "Rate('{}', {}, '{}')",
            self.0.currency(),
            self.0.units_per_gbp(),
            self.0.period()
        )
    }
}

#[derive(Copy, Clone)]
enum TableKey {
    YearMonth(hmrc_rates::YearMonth),
    Spot(hmrc_rates::YearEnd),
    Average(hmrc_rates::YearEnd),
    Week(NaiveDate),
}

/// One period's rates. Owns a snapshot for iteration; `rate()` re-resolves
/// against the source `Rates` so error semantics match the Rust crate exactly.
#[pyclass(name = "Table", module = "hmrc_rates", frozen)]
struct PyTable {
    rates: Arc<hmrc_rates::Rates>,
    key: TableKey,
    period: hmrc_rates::Period,
    rate_type: hmrc_rates::RateType,
    entries: Vec<(hmrc_rates::Currency, hmrc_rates::Rate)>,
}

impl PyTable {
    fn build(rates: &Arc<hmrc_rates::Rates>, key: TableKey) -> PyResult<Self> {
        let table = Self::resolve(rates, key).map_err(lookup_err)?;
        Ok(PyTable {
            rates: Arc::clone(rates),
            key,
            period: table.period(),
            rate_type: table.rate_type(),
            entries: table.iter().collect(),
        })
    }

    fn resolve(
        rates: &hmrc_rates::Rates,
        key: TableKey,
    ) -> Result<hmrc_rates::Table<'_>, hmrc_rates::LookupError> {
        match key {
            TableKey::YearMonth(m) => rates.monthly(m),
            TableKey::Spot(p) => rates.spot(p),
            TableKey::Average(p) => rates.average(p),
            TableKey::Week(d) => rates.weekly(d),
        }
    }
}

#[pymethods]
impl PyTable {
    #[getter]
    fn period(&self) -> PyPeriod {
        PyPeriod(self.period)
    }

    #[getter]
    fn rate_type(&self) -> PyResult<PyRateType> {
        PyRateType::from_rust(self.rate_type)
    }

    /// Strict lookup; `"GBP"` returns the identity rate.
    fn rate(&self, code: &str) -> PyResult<PyRate> {
        let table = Self::resolve(&self.rates, self.key).map_err(lookup_err)?;
        table.rate(code).map(PyRate).map_err(lookup_err)
    }

    /// Like `rate()` but returns `None` instead of raising.
    fn get(&self, code: &str) -> Option<PyRate> {
        Self::resolve(&self.rates, self.key)
            .ok()
            .and_then(|t| t.get(code).map(PyRate))
    }

    fn __len__(&self) -> usize {
        self.entries.len()
    }

    fn __iter__(&self) -> TableIter {
        TableIter {
            entries: self.entries.clone(),
            next: 0,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Table({}, '{}', {} currencies)",
            self.rate_type,
            self.period,
            self.entries.len()
        )
    }
}

/// Iterator over `(Currency, Rate)` pairs of a `Table`.
#[pyclass(module = "hmrc_rates")]
struct TableIter {
    entries: Vec<(hmrc_rates::Currency, hmrc_rates::Rate)>,
    next: usize,
}

#[pymethods]
impl TableIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self) -> Option<(PyCurrency, PyRate)> {
        let (currency, rate) = *self.entries.get(self.next)?;
        self.next += 1;
        Some((PyCurrency(currency), PyRate(rate)))
    }
}

/// All bundled HMRC rate tables. Constructing is free.
#[pyclass(name = "Rates", module = "hmrc_rates", frozen)]
struct PyRates {
    inner: Arc<hmrc_rates::Rates>,
}

#[pymethods]
impl PyRates {
    #[new]
    fn new() -> Self {
        PyRates {
            inner: Arc::new(hmrc_rates::Rates::new()),
        }
    }

    /// The monthly rate for `code` in `year_month` (a `YearMonth`, `datetime.date`, or `"YYYY-MM"`).
    fn monthly_rate(&self, code: &str, year_month: YearMonthArg) -> PyResult<PyRate> {
        self.inner
            .monthly_rate(code, year_month.into_year_month()?)
            .map(PyRate)
            .map_err(lookup_err)
    }

    /// Like `monthly_rate`, falling back up to `max_months_back` earlier months.
    fn monthly_rate_or_earlier(
        &self,
        code: &str,
        year_month: YearMonthArg,
        max_months_back: u32,
    ) -> PyResult<PyRate> {
        self.inner
            .monthly_rate_or_earlier(code, year_month.into_year_month()?, max_months_back)
            .map(PyRate)
            .map_err(lookup_err)
    }

    /// The full monthly table for a month.
    fn monthly(&self, year_month: YearMonthArg) -> PyResult<PyTable> {
        PyTable::build(
            &self.inner,
            TableKey::YearMonth(year_month.into_year_month()?),
        )
    }

    /// The spot table for a year end.
    fn spot(&self, period: PyYearEnd) -> PyResult<PyTable> {
        PyTable::build(&self.inner, TableKey::Spot(period.0))
    }

    /// The yearly-average table for a year end.
    fn average(&self, period: PyYearEnd) -> PyResult<PyTable> {
        PyTable::build(&self.inner, TableKey::Average(period.0))
    }

    /// The weekly-amendment table covering `date`.
    fn weekly(&self, date: NaiveDate) -> PyResult<PyTable> {
        PyTable::build(&self.inner, TableKey::Week(date))
    }

    /// All months with monthly tables, ascending.
    fn months(&self) -> Vec<PyYearMonth> {
        self.inner.months().map(PyYearMonth).collect()
    }

    /// All year ends with spot tables, ascending.
    fn spot_periods(&self) -> Vec<PyYearEnd> {
        self.inner.spot_periods().map(PyYearEnd).collect()
    }

    /// All year ends with average tables, ascending.
    fn average_periods(&self) -> Vec<PyYearEnd> {
        self.inner.average_periods().map(PyYearEnd).collect()
    }

    /// All weekly-amendment validity ranges, ascending.
    fn weeks(&self) -> Vec<PyPeriod> {
        self.inner.weeks().map(PyPeriod).collect()
    }

    /// Every currency that appears anywhere in a series.
    fn currencies(&self, table: PyRateType) -> Vec<PyCurrency> {
        self.inner
            .currencies(table.to_rust())
            .map(PyCurrency)
            .collect()
    }

    fn __repr__(&self) -> String {
        format!("Rates({} monthly tables)", self.inner.months().count())
    }
}

/// Fetches newly published rates, caching files on disk.
#[pyclass(name = "Updater", module = "hmrc_rates", frozen)]
struct PyUpdater(hmrc_rates::Updater);

#[pymethods]
impl PyUpdater {
    #[new]
    #[pyo3(signature = (*, cache_dir=None, base_url=None))]
    fn new(cache_dir: Option<PathBuf>, base_url: Option<String>) -> Self {
        let mut updater = hmrc_rates::Updater::new();
        if let Some(dir) = cache_dir {
            updater = updater.with_cache_dir(dir);
        }
        if let Some(url) = base_url {
            updater = updater.with_base_url(url);
        }
        PyUpdater(updater)
    }

    /// Bundled data overlaid with the disk cache; never touches the network.
    fn cached(&self, py: Python<'_>) -> PyRates {
        let rates = py.detach(|| self.0.cached());
        PyRates {
            inner: Arc::new(rates),
        }
    }

    /// Bundled data, cache and network. Blocking HTTP with the GIL released.
    fn refreshed(&self, py: Python<'_>) -> PyResult<PyRates> {
        let rates = py.detach(|| self.0.refreshed()).map_err(fetch_err)?;
        Ok(PyRates {
            inner: Arc::new(rates),
        })
    }
}

/// Official HMRC exchange rates with bundled history and exact GBP conversion.
#[pymodule(name = "hmrc_rates")]
fn hmrc_rates_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyRates>()?;
    m.add_class::<PyTable>()?;
    m.add_class::<PyRate>()?;
    m.add_class::<PyYearMonth>()?;
    m.add_class::<PyYearEnd>()?;
    m.add_class::<PyCurrency>()?;
    m.add_class::<PyRateType>()?;
    m.add_class::<PyPeriod>()?;
    m.add_class::<PyUpdater>()?;
    m.add("HmrcRatesError", m.py().get_type::<HmrcRatesError>())?;
    m.add(
        "UnknownCurrencyError",
        m.py().get_type::<UnknownCurrencyError>(),
    )?;
    m.add(
        "PeriodNotAvailableError",
        m.py().get_type::<PeriodNotAvailableError>(),
    )?;
    m.add("NotInPeriodError", m.py().get_type::<NotInPeriodError>())?;
    m.add("FetchError", m.py().get_type::<FetchError>())?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
