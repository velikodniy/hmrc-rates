import datetime

import pytest
from hmrc_rates import Currency, Month, RateType, YearEnd


def test_month_basics():
    m = Month(2025, 8)
    assert (m.year, m.month) == (2025, 8)
    assert str(m) == "2025-08"
    assert repr(m) == "Month(2025, 8)"
    assert m.next() == Month(2025, 9)
    assert m.prev() == Month(2025, 7)
    assert Month(2024, 12).next() == Month(2025, 1)


def test_month_parse_and_from_date():
    assert Month.parse("2025-08") == Month(2025, 8)
    assert Month.from_date(datetime.date(2025, 8, 31)) == Month(2025, 8)
    with pytest.raises(ValueError):
        Month.parse("2025/08")
    with pytest.raises(ValueError):
        Month(2025, 13)


def test_month_ordering_and_hash():
    assert Month(2025, 1) < Month(2025, 2) < Month(2026, 1)
    assert len({Month(2025, 8), Month(2025, 8), Month(2025, 9)}) == 2


def test_year_end():
    march = YearEnd.march(2026)
    assert march.is_march
    assert march.year == 2026
    assert march.end_month() == Month(2026, 3)
    assert str(march) == "year ending 2026-03-31"
    december = YearEnd.december(2025)
    assert not december.is_march
    assert december < march
    assert YearEnd.from_month(Month(2025, 12)) == december
    assert YearEnd.from_month(Month(2025, 5)) is None


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
