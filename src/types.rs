use core::fmt;

use chrono::{Datelike, NaiveDate};

/// A calendar month, the key of HMRC monthly rate tables.
///
/// ```
/// use hmrc_rates::Month;
/// let m = Month::new(2025, 8).unwrap();
/// assert_eq!(m.to_string(), "2025-08");
/// assert_eq!(m.next().month(), 9);
/// ```
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Month(i32); // year * 12 + (month - 1)

impl Month {
    /// Returns `None` unless `month` is in `1..=12`.
    pub fn new(year: i32, month: u32) -> Option<Month> {
        if !(1..=12).contains(&month) {
            return None;
        }
        let key = year.checked_mul(12)?.checked_add(month as i32 - 1)?;
        Some(Month(key))
    }

    pub fn year(self) -> i32 {
        self.0.div_euclid(12)
    }

    /// 1..=12
    pub fn month(self) -> u32 {
        (self.0.rem_euclid(12) + 1) as u32
    }

    pub fn next(self) -> Month {
        Month(self.0.saturating_add(1))
    }

    pub fn prev(self) -> Month {
        Month(self.0.saturating_sub(1))
    }

    pub(crate) fn key(self) -> i32 {
        self.0
    }

    pub(crate) fn from_key(key: i32) -> Month {
        Month(key)
    }
}

impl From<NaiveDate> for Month {
    fn from(date: NaiveDate) -> Month {
        Month(date.year() * 12 + date.month() as i32 - 1)
    }
}

impl fmt::Display for Month {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}", self.year(), self.month())
    }
}

/// A spot/average rate period: HMRC publishes these only for years ending
/// 31 March or 31 December, so other dates are unrepresentable.
///
/// ```
/// use hmrc_rates::YearEnd;
/// let ye = YearEnd::march(2026);
/// assert!(ye.is_march());
/// assert_eq!(ye.to_string(), "year ending 2026-03-31");
/// ```
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct YearEnd {
    year: i32,
    december: bool, // false = 31 March, true = 31 December; Mar < Dec within a year
}

impl YearEnd {
    /// The year ending 31 March of `year`.
    pub fn march(year: i32) -> YearEnd {
        YearEnd { year, december: false }
    }

    /// The year ending 31 December of `year`.
    pub fn december(year: i32) -> YearEnd {
        YearEnd { year, december: true }
    }

    pub fn year(self) -> i32 {
        self.year
    }

    pub fn is_march(self) -> bool {
        !self.december
    }

    /// The month the period ends in (March or December of [`YearEnd::year`]).
    pub fn end_month(self) -> Month {
        let month = if self.december { 12 } else { 3 };
        Month(self.year * 12 + month - 1)
    }

    pub(crate) fn key(self) -> i32 {
        self.year * 2 + self.december as i32
    }

    pub(crate) fn from_key(key: i32) -> YearEnd {
        YearEnd { year: key.div_euclid(2), december: key.rem_euclid(2) == 1 }
    }
}

impl fmt::Display for YearEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let month = if self.december { 12 } else { 3 };
        write!(f, "year ending {:04}-{:02}-31", self.year, month)
    }
}

/// An ISO 4217-style currency code as published by HMRC (three ASCII letters).
///
/// Lookups accept plain `&str` (case-insensitive); the library returns `Currency`.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Currency([u8; 3]);

impl Currency {
    /// Pound sterling, the base of every HMRC rate.
    pub const GBP: Currency = Currency(*b"GBP");

    pub fn as_str(&self) -> &str {
        // Invariant: always three ASCII uppercase letters.
        core::str::from_utf8(&self.0).unwrap_or("???")
    }

    pub(crate) fn from_code(code: [u8; 3]) -> Currency {
        Currency(code)
    }

    pub(crate) fn code(&self) -> [u8; 3] {
        self.0
    }

    /// Trims and uppercases `s`; `None` unless the result is three ASCII letters.
    pub(crate) fn normalize(s: &str) -> Option<[u8; 3]> {
        let s = s.trim();
        let bytes = s.as_bytes();
        if bytes.len() != 3 || !bytes.iter().all(|b| b.is_ascii_alphabetic()) {
            return None;
        }
        Some([
            bytes[0].to_ascii_uppercase(),
            bytes[1].to_ascii_uppercase(),
            bytes[2].to_ascii_uppercase(),
        ])
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The four rate series HMRC has published.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
#[non_exhaustive]
pub enum RateType {
    /// Monthly customs/VAT rates (2014-02 onwards).
    Monthly,
    /// Spot rates on 31 March / 31 December (2010-12 onwards, major currencies).
    Spot,
    /// Yearly average rates to 31 March / 31 December (2010-12 onwards).
    Average,
    /// Weekly amendment series (2014-01 to 2016-04, then discontinued).
    Weekly,
}

impl fmt::Display for RateType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RateType::Monthly => "monthly",
            RateType::Spot => "spot",
            RateType::Average => "average",
            RateType::Weekly => "weekly",
        })
    }
}

/// The period a [`Rate`](crate::Rate) or table applies to.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[non_exhaustive]
pub enum Period {
    Month(Month),
    YearEnd(YearEnd),
    /// An inclusive weekly-amendment validity range.
    Week { start: NaiveDate, end: NaiveDate },
}

impl fmt::Display for Period {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Period::Month(m) => m.fmt(f),
            Period::YearEnd(ye) => ye.fmt(f),
            Period::Week { start, end } => write!(f, "week {start} to {end}"),
        }
    }
}

// Compact string forms: Month "2026-07", YearEnd "2026-03"/"2025-12", Currency "USD".
#[cfg(feature = "serde")]
mod serde_impls {
    use super::{Currency, Month, YearEnd};
    use alloc::format;
    use alloc::string::String;
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    impl Serialize for Month {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_str(self)
        }
    }

    impl<'de> Deserialize<'de> for Month {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Month, D::Error> {
            let s = String::deserialize(deserializer)?;
            let parse = || {
                let (y, m) = s.split_once('-')?;
                Month::new(y.parse().ok()?, m.parse().ok()?)
            };
            parse().ok_or_else(|| D::Error::custom(format!("invalid month '{s}', expected YYYY-MM")))
        }
    }

    impl Serialize for YearEnd {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let month = if self.is_march() { 3 } else { 12 };
            serializer.collect_str(&format_args!("{:04}-{:02}", self.year(), month))
        }
    }

    impl<'de> Deserialize<'de> for YearEnd {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<YearEnd, D::Error> {
            let month = Month::deserialize(deserializer)?;
            match month.month() {
                3 => Ok(YearEnd::march(month.year())),
                12 => Ok(YearEnd::december(month.year())),
                _ => Err(D::Error::custom(format!(
                    "invalid year end '{month}', expected March or December"
                ))),
            }
        }
    }

    impl Serialize for Currency {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_str(self.as_str())
        }
    }

    impl<'de> Deserialize<'de> for Currency {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Currency, D::Error> {
            let s = String::deserialize(deserializer)?;
            Currency::normalize(&s)
                .map(Currency::from_code)
                .ok_or_else(|| D::Error::custom(format!("invalid currency code '{s}'")))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn month_roundtrip_and_arithmetic() {
        let m = Month::new(2025, 1).unwrap();
        assert_eq!((m.year(), m.month()), (2025, 1));
        assert_eq!(m.prev(), Month::new(2024, 12).unwrap());
        assert_eq!(m.next(), Month::new(2025, 2).unwrap());
        assert_eq!(Month::new(2025, 12).unwrap().next(), Month::new(2026, 1).unwrap());
        assert!(Month::new(2025, 0).is_none());
        assert!(Month::new(2025, 13).is_none());
        assert_eq!(m.to_string(), "2025-01");
    }

    #[test]
    fn month_from_date() {
        let date = NaiveDate::from_ymd_opt(2025, 8, 31).unwrap();
        assert_eq!(Month::from(date), Month::new(2025, 8).unwrap());
    }

    #[test]
    fn year_end_ordering_and_display() {
        assert!(YearEnd::march(2025) < YearEnd::december(2025));
        assert!(YearEnd::december(2024) < YearEnd::march(2025));
        assert_eq!(YearEnd::march(2026).end_month(), Month::new(2026, 3).unwrap());
        assert_eq!(YearEnd::december(2025).to_string(), "year ending 2025-12-31");
        assert_eq!(YearEnd::from_key(YearEnd::march(2026).key()), YearEnd::march(2026));
    }

    #[test]
    fn currency_normalization() {
        assert_eq!(Currency::normalize(" usd "), Some(*b"USD"));
        assert_eq!(Currency::normalize("EuR"), Some(*b"EUR"));
        assert_eq!(Currency::normalize(""), None);
        assert_eq!(Currency::normalize("US"), None);
        assert_eq!(Currency::normalize("USDX"), None);
        assert_eq!(Currency::normalize("U5D"), None);
        assert_eq!(Currency::GBP.as_str(), "GBP");
    }
}
