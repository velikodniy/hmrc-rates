import pytest
from hmrc_rates import (
    HmrcRatesError,
    Month,
    NotInPeriodError,
    PeriodNotAvailableError,
    Rates,
    RateType,
    UnknownCurrencyError,
)

rates = Rates()


def test_unknown_currency():
    with pytest.raises(UnknownCurrencyError) as excinfo:
        rates.monthly_rate("ZZZ", Month(2025, 8))
    assert isinstance(excinfo.value, HmrcRatesError)
    # must not shadow or subclass the Python builtin
    assert not isinstance(excinfo.value, LookupError)
    assert "ZZZ" in str(excinfo.value)


def test_period_not_available():
    with pytest.raises(PeriodNotAvailableError) as excinfo:
        rates.monthly(Month(2100, 1))
    # the message carries the available range
    assert "available" in str(excinfo.value)


def test_not_in_period():
    # find a currency that joined the monthly series after its first month
    first_table = rates.monthly(rates.months()[0])
    late_joiner = next(
        (
            c
            for c in rates.currencies(RateType.MONTHLY)
            if first_table.get(c.code) is None
        ),
        None,
    )
    if late_joiner is None:
        pytest.skip("every monthly currency present since the first month")
    with pytest.raises(NotInPeriodError):
        first_table.rate(late_joiner.code)


def test_table_rate_unknown_currency():
    table = rates.monthly(Month(2025, 8))
    with pytest.raises(UnknownCurrencyError):
        table.rate("ZZZ")


def test_invalid_month_string():
    with pytest.raises(ValueError, match="YYYY-MM"):
        rates.monthly_rate("USD", "August 2025")
