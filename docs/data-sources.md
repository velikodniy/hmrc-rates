# Data sources

Where every rate in `data/` comes from, and how the files are kept current.

## Live source: HMRC via the UK Trade Tariff API

Base: `https://www.trade-tariff.service.gov.uk/api/v2/exchange_rates/`

| Series | File pattern | Available |
|---|---|---|
| monthly | `files/monthly_xml_YYYY-MM.xml` | 2021-01 → present |
| average | `files/average_csv_YYYY-MM.csv` | 2020-03 → present (March & December only) |
| spot | `files/spot_csv_YYYY-MM.csv` | 2023-12 → present (March & December only) |

- Discovery: `period_lists?filter[type]={monthly|average|spot}&year=YYYY` (JSON:API; used by `scripts/update_rates.py`, not by the crate — the runtime client computes file URLs and treats 404 as "not published yet").
- `rateNew` / "Currency Units per £1" = currency units per £1; divide an amount by it to get GBP.
- HMRC publishes monthly rates on the penultimate Thursday of each month, for the following month. Rare in-month amendments replace a file.
- `scripts/update_rates.py` (run daily by `.github/workflows/update-rates.yaml`) downloads anything new into `data/{monthly,average,spot}/YYYY-MM.{xml,csv}`.

## Archived history: UK Government Web Archive (one-off backfill)

Everything older than the API era was recovered once by `scripts/backfill_archive.py`
(its URL tables are inline in the script) from `webarchive.nationalarchives.gov.uk` snapshots of
withdrawn gov.uk publications and the pre-2014 `hmrc.gov.uk/softwaredevelopers` XMLs.
The live asset URLs are dead; the archive is the only source.

| Series | Bundled coverage | Notes |
|---|---|---|
| monthly | 2014-02 → present | Jan 2014 and earlier were never published as XML |
| average | 2010-12 → present | pre-2020 files had no code column and drifting headers |
| spot | 2010-12 → present | 2021-03..2023-03 scraped from trade-tariff HTML views (no files exist) |
| weekly | 2014-01-08 → 2016-04-27 | the discontinued amendment series, complete |

Archived CSVs were normalized into the current API shape
(`Country,Unit Of Currency,Currency Code,Sterling value of Currency Unit £,Currency Units per £1`,
one file per period, UTF-8). Normalization rules worth knowing:

- **Currency codes** were resolved from monthly XML data by (country, unit) name matching,
  with period-aware overrides for redenominations and euro adoption (ZMK→ZMW 2013,
  BYR→BYN 2016, VEF→VES 2018, MRO→MRU 2018, STD→STN 2018, SLL→SLE 2022, EEK→EUR 2011,
  LVL→EUR 2014, LTL→EUR 2015). Pre-changeover rows keep their historical codes.
- **Euro-transition averages** whose period straddles an adoption date (Estonia 2011-03,
  Latvia 2014-03, Lithuania 2015-03) are blended, meaningless values and were dropped.
- **Duplicate countries sharing a code** (EUR ×19, XCD ×6, …): one row per code is kept;
  when published values conflict, the currency's principal country wins (USA for USD,
  Eurozone for EUR, …), then the more precise value.
- **Corrupt source rows** in the Dec 2015 spot file (four USD-pegged countries and SEK/KMF
  shifted by whole columns) were dropped after cross-checking against the same month's
  monthly rates (±2× tolerance).
- Codes are **as published by HMRC**, which is not always ISO 4217 — e.g. Ecuador appears
  as `ECS` long after dollarization.

## Build-time validation

`build.rs` re-parses everything on every build and fails on: non-calendar-month periods,
gaps in the monthly series, malformed codes, non-positive rates, out-of-range mantissas,
non-March/December spot or average periods, or conflicting duplicates with no majority.
A bad data file cannot ship.
