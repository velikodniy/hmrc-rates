use rust_decimal::Decimal;

use crate::types::{Currency, Period};

/// A resolved HMRC rate: currency units per £1, with exact `Decimal` arithmetic.
///
/// The crate never rounds — callers apply whatever rounding their tax context requires.
///
/// ```
/// use hmrc_rates::Rates;
/// use rust_decimal::Decimal;
///
/// let rates = Rates::new();
/// let rate = rates.monthly_rate("USD", hmrc_rates::Month::new(2025, 8).unwrap())?;
/// let gbp: Decimal = rate.to_gbp(Decimal::from(100));
/// assert!(gbp > Decimal::ZERO);
/// # Ok::<(), hmrc_rates::LookupError>(())
/// ```
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rate {
    units_per_gbp: Decimal,
    currency: Currency,
    period: Period,
}

impl Rate {
    pub(crate) fn new(units_per_gbp: Decimal, currency: Currency, period: Period) -> Rate {
        Rate {
            units_per_gbp,
            currency,
            period,
        }
    }

    /// The canonical HMRC figure: how many currency units £1 buys.
    pub fn units_per_gbp(&self) -> Decimal {
        self.units_per_gbp
    }

    /// Converts an amount in this rate's currency to GBP (`amount / units_per_gbp`).
    pub fn to_gbp(&self, amount: Decimal) -> Decimal {
        amount / self.units_per_gbp
    }

    /// Converts a GBP amount to this rate's currency (`gbp * units_per_gbp`).
    pub fn from_gbp(&self, gbp: Decimal) -> Decimal {
        gbp * self.units_per_gbp
    }

    pub fn currency(&self) -> Currency {
        self.currency
    }

    /// The period the rate was published for — reveals the substituted month
    /// after [`Rates::monthly_rate_or_earlier`](crate::Rates::monthly_rate_or_earlier).
    pub fn period(&self) -> Period {
        self.period
    }
}
