import datetime

import pytest
from hmrc_rates import Currency, YearMonth, RateType, YearEnd


def test_month_basics():
    m = YearMonth(2025, 8)
    assert (m.year, m.month) == (2025, 8)
    assert str(m) == "2025-08"
    assert repr(m) == "YearMonth(2025, 8)"
    assert m.next() == YearMonth(2025, 9)
    assert m.prev() == YearMonth(2025, 7)
    assert YearMonth(2024, 12).next() == YearMonth(2025, 1)


def test_month_parse_and_from_date():
    assert YearMonth.parse("2025-08") == YearMonth(2025, 8)
    assert YearMonth.from_date(datetime.date(2025, 8, 31)) == YearMonth(2025, 8)
    with pytest.raises(ValueError):
        YearMonth.parse("2025/08")
    with pytest.raises(ValueError):
        YearMonth(2025, 13)


def test_month_ordering_and_hash():
    assert YearMonth(2025, 1) < YearMonth(2025, 2) < YearMonth(2026, 1)
    assert len({YearMonth(2025, 8), YearMonth(2025, 8), YearMonth(2025, 9)}) == 2


def test_year_end():
    march = YearEnd.march(2026)
    assert march.is_march
    assert march.year == 2026
    assert march.end_year_month() == YearMonth(2026, 3)
    assert str(march) == "year ending 2026-03-31"
    december = YearEnd.december(2025)
    assert not december.is_march
    assert december < march
    assert YearEnd.from_year_month(YearMonth(2025, 12)) == december
    assert YearEnd.from_year_month(YearMonth(2025, 5)) is None


def test_currency():
    assert Currency.GBP.code == "GBP"
    assert str(Currency.GBP) == "GBP"
    assert repr(Currency.GBP) == "Currency('GBP')"
    assert len({Currency.GBP, Currency.GBP}) == 1


def test_rate_type_members():
    assert RateType.MONTHLY == RateType.MONTHLY
    assert RateType.MONTHLY != RateType.SPOT
    assert (
        len({RateType.MONTHLY, RateType.SPOT, RateType.AVERAGE, RateType.WEEKLY}) == 4
    )
