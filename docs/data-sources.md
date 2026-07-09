# Data sources

Where every rate in `data/` comes from, and how the files are kept current.

## Live source: HMRC via the UK Trade Tariff API

Base: `https://www.trade-tariff.service.gov.uk/api/v2/exchange_rates/`

| Series | File pattern | Available |
|---|---|---|
| monthly | `files/monthly_xml_YYYY-MM.xml` | 2021-01 - present |
| average | `files/average_csv_YYYY-MM.csv` | 2020-03 - present (March & December only) |
| spot | `files/spot_csv_YYYY-MM.csv` | 2023-12 - present (March & December only) |

- Discovery: `period_lists?filter[type]={monthly|average|spot}&year=YYYY` (JSON:API), used only by the update script; the crate computes file URLs and treats 404 as "not published yet".
- `rateNew` / "Currency Units per £1" = currency units per £1; divide an amount by it to get GBP.
- HMRC publishes monthly rates on the penultimate Thursday of each month, for the following month.
- Rare in-month amendments replace a file.
- A daily CI job runs the update script, which downloads anything new into `data/` and re-checks amendable months.

## Archived history: UK Government Web Archive

Everything older than the API era was recovered once by the backfill script from `webarchive.nationalarchives.gov.uk` snapshots of withdrawn gov.uk publications and the old `hmrc.gov.uk/softwaredevelopers` XMLs.
The gov.uk CSV asset URLs are dead, and the legacy `hmrc.gov.uk/softwaredevelopers/rates/exrates-monthly-MMYY.xml` endpoint is a zombie: it still serves roughly 2015 to mid-2023 but is missing the edges and gets no new files, so the archive snapshots are the canonical source.
The exact archive URLs live in the script itself.

| Series | Bundled coverage | Notes |
|---|---|---|
| monthly | 2014-02 - present | Jan 2014 and earlier were never published as XML |
| average | 2010-12 - present | pre-2020 files had no code column and drifting headers |
| spot | 2010-12 - present | 2021-03..2023-03 scraped from trade-tariff HTML views (no files exist) |
| weekly | 2014-01-08 - 2016-04-27 | the discontinued amendment series, complete |

Archived CSVs were normalized into the current API shape: one file per period, UTF-8, header `Country,Unit Of Currency,Currency Code,Sterling value of Currency Unit £,Currency Units per £1`.
Normalization rules worth knowing:

- Currency codes were resolved by (country, unit) name matching against the monthly XMLs, with period-aware overrides for redenominations and euro adoption (ZMK->ZMW 2013, BYR->BYN 2016, VEF->VES 2018, MRO->MRU 2018, STD->STN 2018, SLL->SLE 2022, EEK->EUR 2011, LVL->EUR 2014, LTL->EUR 2015).
  Pre-changeover rows keep their historical codes.
- Euro-transition averages whose period straddles an adoption date (Estonia 2011-03, Latvia 2014-03, Lithuania 2015-03) are blended, meaningless values and were dropped.
- When several countries share a code with conflicting values, the currency's principal country wins (USA for USD, Eurozone for EUR, and so on), then the more precise value.
- Corrupt rows in the Dec 2015 spot file were dropped after cross-checking against the same month's monthly rates (anything off by more than 2x was dropped).
- Codes are as published by HMRC, which is not always ISO 4217 — Ecuador appears as `ECS` long after dollarization.

## Build-time validation

The build script re-parses everything on every build and fails on: non-calendar-month periods, gaps in the monthly series, malformed codes, non-positive rates, out-of-range mantissas, non-March/December spot or average periods, and conflicting duplicates with no majority.
A bad data file cannot ship.
