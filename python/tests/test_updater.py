from decimal import Decimal

import pytest
from hmrc_rates import YearMonth, Rates, Updater


def test_cached_works_offline(tmp_path):
    # an empty cache dir means bundled data only; no network is touched
    updater = Updater(cache_dir=str(tmp_path))
    rates = updater.cached()
    assert isinstance(rates, Rates)
    assert rates.monthly_rate("USD", YearMonth(2025, 8)).units_per_gbp > Decimal(0)


@pytest.mark.network
def test_refreshed_hits_live_endpoint(tmp_path):
    updater = Updater(cache_dir=str(tmp_path))
    rates = updater.refreshed()
    assert rates.months()
