import datetime
from decimal import Decimal

import pytest
from hmrc_rates import Currency, Month, Rate, Rates, RateType, YearEnd

rates = Rates()


def test_monthly_rate_smoke():
    rate = rates.monthly_rate("USD", Month(2025, 8))
    assert isinstance(rate, Rate)
    assert rate.currency.code == "USD"
    assert rate.units_per_gbp > 0


def test_money_is_decimal_never_float():
    rate = rates.monthly_rate("USD", Month(2025, 8))
    assert type(rate.units_per_gbp) is Decimal
    assert type(rate.to_gbp(Decimal("2500"))) is Decimal
    assert type(rate.from_gbp(100)) is Decimal


def test_exact_arithmetic():
    rate = rates.monthly_rate("USD", Month(2025, 8))
    # x / x is exactly 1 in any decimal arithmetic
    assert rate.to_gbp(rate.units_per_gbp) == Decimal(1)
    # multiplication terminates well within both precisions
    assert rate.from_gbp(Decimal("100")) == Decimal("100") * rate.units_per_gbp
    # int amounts behave like the equivalent Decimal
    assert rate.from_gbp(100) == rate.from_gbp(Decimal("100"))


def test_month_like_arguments_agree():
    by_month = rates.monthly_rate("USD", Month(2025, 8))
    by_date = rates.monthly_rate("USD", datetime.date(2025, 8, 15))
    by_str = rates.monthly_rate("USD", "2025-08")
    assert by_month == by_date == by_str


def test_gbp_identity():
    rate = rates.monthly_rate("GBP", Month(2025, 8))
    assert rate.units_per_gbp == Decimal(1)
    assert rate.currency == Currency.GBP


def test_lookup_is_case_insensitive():
    assert rates.monthly_rate("usd", Month(2025, 8)) == rates.monthly_rate(
        "USD", Month(2025, 8)
    )


def test_monthly_rate_or_earlier_reveals_substitution():
    newest = rates.months()[-1]
    rate = rates.monthly_rate_or_earlier("USD", newest.next(), 1)
    assert rate.period.month == newest


def test_monthly_table():
    table = rates.monthly(Month(2025, 8))
    assert len(table) > 100
    assert table.rate_type == RateType.MONTHLY
    assert table.period.kind == "month"
    assert table.period.month == Month(2025, 8)
    entries = dict(table)
    assert len(entries) == len(table)
    currency, rate = next(iter(table))
    assert isinstance(currency, Currency)
    assert isinstance(rate, Rate)
    assert table.get("XXX") is None
    usd = table.rate("USD")
    assert table.get("USD") == usd


def test_spot_average_weekly_tables():
    spot = rates.spot(YearEnd.december(2024))
    assert spot.rate_type == RateType.SPOT
    average = rates.average(YearEnd.march(2025))
    assert average.rate_type == RateType.AVERAGE
    assert average.rate("EUR").units_per_gbp > 0
    weekly = rates.weekly(datetime.date(2015, 6, 15))
    assert weekly.rate_type == RateType.WEEKLY
    assert weekly.period.kind == "week"
    assert weekly.period.start <= datetime.date(2015, 6, 15) <= weekly.period.end


def test_period_listings():
    months = rates.months()
    assert months == sorted(months)
    assert Month(2025, 8) in months
    assert rates.spot_periods() == sorted(rates.spot_periods())
    assert YearEnd.march(2025) in rates.average_periods()
    assert rates.weeks()
    currencies = rates.currencies(RateType.MONTHLY)
    assert "USD" in [c.code for c in currencies]


def test_float_amounts_rejected():
    rate = rates.monthly_rate("USD", Month(2025, 8))
    with pytest.raises(TypeError, match="float"):
        rate.to_gbp(2500.0)
    with pytest.raises(TypeError, match="float"):
        rate.from_gbp(100.0)
