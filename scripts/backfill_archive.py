#!/usr/bin/env python3
"""One-off backfill of historical HMRC rates from the UK Government Web Archive.

Recovers monthly XML (2014), yearly average and spot CSVs (from Dec 2010) and
the 2014-2016 weekly amendment series, and normalizes early API-era CSVs that
lack a currency-code column.
Idempotent; kept for reproducibility.
"""

import csv
import io
import pathlib
import re
import sys
import urllib.request
import xml.etree.ElementTree as ET

UKGWA = "https://webarchive.nationalarchives.gov.uk/ukgwa"
UA = "Mozilla/5.0 (backfill; +https://github.com/velikodniy/hmrc-rates)"
DATA = pathlib.Path("data")
CACHE = pathlib.Path(".backfill-cache")
CANONICAL_HEADER = [
    "Country",
    "Unit Of Currency",
    "Currency Code",
    "Sterling value of Currency Unit £",
    "Currency Units per £1",
]

# (timestamp, original URL) per archived source; timestamps are nearest-capture hints.
MONTHLY_2014 = {
    "2014-02": ("20140603104805", "http://www.hmrc.gov.uk/softwaredevelopers/rates/exrates_monthly_0214.XML"),
    **{
        f"2014-{m:02d}": ("20151202213700", f"http://www.hmrc.gov.uk/softwaredevelopers/rates/exrates-monthly-{m:02d}14.xml")
        for m in range(3, 13)
    },
}

GOVUK = "https://www.gov.uk/government/uploads/system/uploads/attachment_data"
ASSETS = "https://assets.publishing.service.gov.uk"
AVERAGE_SOURCES = [
    # (cache name, timestamp, url, fallback periods when headers carry none);
    # wide files hold two periods (Dec + Mar)
    ("avg2011.csv", "20141213032646", f"{GOVUK}/file/371079/Avg-year-20110331.csv", []),
    ("avg2012.csv", "20141213032646", f"{GOVUK}/file/371056/Avg-year-20120331.csv", []),
    ("avg2013.csv", "20141213032646", f"{GOVUK}/file/371053/Avg-year-20130331.csv", []),
    ("avg2014.csv", "20141213032646", f"{GOVUK}/file/371018/Avg-year-20140331.csv", []),
    ("avg2015.csv", "20160116145828", f"{GOVUK}/file/421061/average310315-1.csv", []),
    ("avg2016.csv", "20170101000000", f"{GOVUK}/file/518917/average_spot_rates_310316.csv", []),
    ("avg2017.csv", "20180101000000", f"{GOVUK}/file/609410/R-of-E-yearly-spot-rate-avg.csv", []),
    ("avg2017dec.csv", "20180104121914", f"{GOVUK}/file/671647/average-year-to-december-2017.csv", ["2017-12"]),
    ("avg2018mar.csv", "20190101000000", f"{ASSETS}/media/5ac3a19eed915d0b7ffac85c/average-year-to-march-2018.csv", ["2018-03"]),
    ("avg2018dec.csv", "20231009170349", f"{ASSETS}/media/5c2dec0be5274a65cc0f5d40/average-year-to-december-2018.csv", ["2018-12", None]),
    ("avg2019mar.csv", "20200101000000", f"{ASSETS}/media/5d0cae3fed915d0935874ae1/Average_for_the_year_to_31_March_2019.csv", ["2019-03"]),
    ("avg2019dec.csv", "20210101000000", f"{ASSETS}/media/5e847c55e90e0706ecc6940a/Average-for-the-year-to-December-2019.csv", ["2019-12"]),
]
SPOT_SOURCES = [
    ("spot2011.csv", "20160116145828", f"{GOVUK}/file/407082/Spot-year-20110331.csv", []),
    ("spot2012.csv", "20160116145828", f"{GOVUK}/file/407083/Spot-year-20120331.csv", []),
    ("spot2013.csv", "20160116145828", f"{GOVUK}/file/407084/Spot-year-20130331.csv", []),
    ("spot2014.csv", "20160116145828", f"{GOVUK}/file/407085/spot-year-20140331.csv", []),
    ("spot2015.csv", "20160116145828", f"{GOVUK}/file/421067/spot_rates_310315.csv", []),
    ("spot2015dec.csv", "20170101000000", f"{GOVUK}/file/490386/spotratesdec15.csv", ["2015-12"]),
    ("spot2016mar.csv", "20170101000000", f"{GOVUK}/file/519274/spot_rates.csv", ["2016-03"]),
    ("spot2017.csv", "20180101000000", f"{GOVUK}/file/609474/spot_rates_31-03-17.csv", []),
    ("spot2017dec.csv", "20180104121914", f"{GOVUK}/file/671649/spot-rates-december-2017.csv", ["2017-12"]),
    ("spot2018mar.csv", "20190101000000", f"{ASSETS}/media/5ac3a47ded915d0b7ffac85d/spot-rates-march-2018.csv", ["2018-03"]),
    ("spot2018dec.csv", "20231009170349", f"{ASSETS}/media/5c2dec88ed915d73360e2d15/spot-rates-december-2018.csv", ["2018-12"]),
    ("spot2019mar.csv", "20231009170349", f"{ASSETS}/media/5ca4dbb540f0b625e50ef58d/spot-rates-year-to-march-2019.csv", ["2018-12", "2019-03"]),
    ("spot2020mar.csv", "20210101000000", f"{ASSETS}/media/5e0b2d80ed915d6a90802327/spot-rates-year-to-march-2020.csv", ["2019-12", "2020-03"]),
    ("spot2020maralt.csv", "20210101000000", f"{ASSETS}/media/5ec38addd3bf7f5d3defffb9/Spot_rates_on_31_March_2020.csv", ["2019-12", "2020-03"]),
    ("spot2020dec.csv", "20210601000000", f"{ASSETS}/media/5ff480a1e90e0776a920843c/Spot_rates_on_31_December_2020.csv", ["2020-12"]),
]
WEEKLY_SOURCES = [
    ("weekly2014.csv", "20171111053606", f"{GOVUK}/file/391353/exrates-amendments.csv", []),
    ("weekly2015.csv", "20201111124706", f"{ASSETS}/government/uploads/system/uploads/attachment_data/file/488697/exrates-weekly-1215.csv", []),
    ("weekly2016.csv", "20201111122926", f"{ASSETS}/government/uploads/system/uploads/attachment_data/file/518746/exrates-weekly-0416.csv", []),
]
# Trade-tariff HTML views: spot periods listed by the API without downloadable files.
SPOT_HTML_PERIODS = ["2021-3", "2021-12", "2022-3", "2022-12", "2023-3"]

# Period-aware code overrides for redenominated/pre-euro currencies (period <=
# cutoff). Country-keyed: unit labels are unreliable (Estonia's kroon-era rows
# say "Euro"), and each country had a single currency before its cutoff.
REDENOMINATIONS = {
    "zambia": ("2013-03", "ZMK"),
    "belarus": ("2016-07", "BYR"),
    "venezuela": ("2018-08", "VEF"),
    "mauritania": ("2017-12", "MRO"),
    "saotomeprincipe": ("2018-03", "STD"),
    "estonia": ("2010-12", "EEK"),
    "latvia": ("2013-12", "LVL"),
    "lithuania": ("2014-12", "LTL"),
    "sierraleone": ("2022-03", "SLL"),
}

# Euro-transition rows whose period straddles adoption: blended, meaningless values.
DROP_ROWS = {
    ("estonia", "2011-03"),
    ("latvia", "2014-03"),
    ("lithuania", "2015-03"),
}

# The row that wins when several countries share a code with differing values.
PRINCIPALS = {
    "USD": {"usa"},
    "EUR": {"eurozone", "europeancommunity"},
    "AED": {"abudhabi"},
    "XCD": {"antigua"},
    "XOF": {"benin"},
    "XAF": {"cameroon"},
    "CHF": {"switzerl"},  # "Switzerland" minus "and"
}

log = lambda *a: print(*a, file=sys.stderr)


def fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return resp.read()


def cached(name: str, url: str) -> bytes:
    CACHE.mkdir(exist_ok=True)
    path = CACHE / name
    if not path.exists():
        log(f"fetching {url}")
        path.write_bytes(fetch(url))
    return path.read_bytes()


def archived(name: str, ts: str, url: str) -> bytes:
    return cached(name, f"{UKGWA}/{ts}mp_/{url}")


def norm(s: str) -> str:
    return re.sub(r"\s+", " ", s).strip().lower()


def match_key(s: str) -> str:
    """Aggressive normalization for name matching: letters only, 'and' dropped."""
    return re.sub(r"[^a-z]", "", s.lower()).replace("and", "")


# --- currency-code resolution ------------------------------------------------

def build_code_map() -> dict:
    """(country, currency-name) and country -> code, from all monthly XMLs (latest wins)."""
    by_pair, by_country, codes = {}, {}, set()
    for path in sorted(DATA.glob("monthly/*.xml")):
        root = ET.parse(path).getroot()
        for entry in root:
            country = match_key(entry.findtext("countryName") or "")
            name = match_key(entry.findtext("currencyName") or "")
            code = (entry.findtext("currencyCode") or "").strip().upper()
            if len(code) != 3:
                continue
            by_pair[(country, name)] = code
            by_country.setdefault(country, set()).add(code)
            codes.add(code)
    codes.update(code for _, code in REDENOMINATIONS.values())
    return {"pair": by_pair, "country": by_country, "codes": codes}


def resolve_code(code_map: dict, country: str, unit: str, period: str) -> tuple[str, int] | None:
    """-> (code, confidence 3..1) or None; None also for dropped transition rows."""
    c, u = match_key(country), match_key(unit)
    for dc, dp in DROP_ROWS:
        if c.startswith(dc) and period == dp:
            return None
    # Explicit code prefix in the unit name ("AUD Dollar") — but names like
    # "CFA Franc" also start with three capitals, so gate on known codes.
    m = re.match(r"^([A-Z]{3})\s+", unit.strip())
    if m and m.group(1) in code_map["codes"]:
        return m.group(1), 3
    for rc, (cutoff, code) in REDENOMINATIONS.items():
        if c.startswith(rc) and period <= cutoff:
            return code, 3
    if (c, u) in code_map["pair"]:
        return code_map["pair"][(c, u)], 2
    # Country match: exact, else unique prefix in either direction.
    countries = [c] if c in code_map["country"] else [
        k for k in code_map["country"] if k and c and (k.startswith(c) or c.startswith(k))
    ]
    if len(countries) == 1:
        codes = code_map["country"][countries[0]]
        if len(codes) == 1:
            return next(iter(codes)), 1
        candidates = {
            v
            for (kc, ku), v in code_map["pair"].items()
            if kc == countries[0] and (ku.startswith(u) or u.startswith(ku))
        }
        if len(candidates) == 1:
            return candidates.pop(), 1
    return None


# --- CSV normalization --------------------------------------------------------

PERIOD_RE = re.compile(r"31\s+(March|December)\s+(\d{4})|31-(Mar|Dec)-(\d{2})", re.IGNORECASE)


def decode(data: bytes) -> str:
    try:
        text = data.decode("utf-8-sig")
    except UnicodeDecodeError:
        text = data.decode("cp1252")
    return text


def find_periods(text: str) -> list[str]:
    out = []
    for m in PERIOD_RE.finditer(text):
        if m.group(1):
            month = "03" if m.group(1).lower() == "march" else "12"
            out.append(f"{m.group(2)}-{month}")
        else:
            month = "03" if m.group(3).lower() == "mar" else "12"
            out.append(f"20{m.group(4)}-{month}")
    return out


def header_period(text: str) -> str | None:
    periods = find_periods(text)
    return periods[0] if periods else None


def locate_header(rows: list[list[str]]) -> tuple[int, list[str]]:
    """Index and content of the header row (files may carry preamble rows)."""
    i = next(i for i, r in enumerate(rows) if r and norm(r[0]) == "country")
    return i, rows[i]


def unit_column(header: list[str]) -> int:
    return next(i for i, h in enumerate(header) if "unit" in norm(h) and "sterling" not in norm(h))


def parse_wide_csv(text: str, fallback_periods: list) -> dict[str, list[tuple[str, str, str, str]]]:
    """-> period -> [(country, unit, sterling, units_per_gbp)]; handles wide files and preambles."""
    rows = [r for r in csv.reader(io.StringIO(text))]
    header_i, header = locate_header(rows)
    context = [" ".join(r) for r in rows[max(0, header_i - 3):header_i]]

    unit_col = unit_column(header)
    rate_cols = [i for i, h in enumerate(header) if "units per" in norm(h)]
    sterling_cols = [i for i, h in enumerate(header) if "sterling" in norm(h)]

    periods = [header_period(header[col]) for col in rate_cols]
    if any(p is None for p in periods):
        # Try periods named in preamble rows (in order), then the declared fallback.
        from_context = list(dict.fromkeys(p for line in context for p in find_periods(line)))
        if len(from_context) == len(rate_cols):
            periods = from_context
        elif len(fallback_periods) == len(rate_cols):
            periods = list(fallback_periods)
        elif len(fallback_periods) == 1:
            periods = fallback_periods + [None] * (len(rate_cols) - 1)
        else:
            raise SystemExit(f"cannot infer periods: headers {header!r}")

    out: dict[str, list] = {p: [] for p in periods if p}
    for row in rows[header_i + 1:]:
        if len(row) <= unit_col or not row[0].strip() or norm(row[0]) == "country":
            continue
        country, unit = row[0].strip(), row[unit_col].strip()
        for period, rate_col, sterling_col in zip(periods, rate_cols, sterling_cols):
            if period is None:
                continue
            rate = row[rate_col].strip() if len(row) > rate_col else ""
            sterling = row[sterling_col].strip() if len(row) > sterling_col else ""
            if rate:
                out[period].append((country, unit, sterling, rate))
    return {p: rows_ for p, rows_ in out.items() if rows_}


def monthly_reference(period: str) -> dict[str, float]:
    """code -> monthly rate for cross-checking spot values (empty pre-2014)."""
    path = DATA / "monthly" / f"{period}.xml"
    if not path.exists():
        return {}
    out = {}
    for entry in ET.parse(path).getroot():
        code = (entry.findtext("currencyCode") or "").strip()
        try:
            out[code] = float(entry.findtext("rateNew") or "")
        except ValueError:
            pass
    return out


def write_canonical(kind: str, period: str, rows: list[tuple[str, str, str, str]], code_map: dict) -> None:
    target = DATA / kind / f"{period}.csv"
    dropped = []
    reference = monthly_reference(period) if kind == "spot" else {}

    # code -> best (confidence, principal, precision) row; warn on discarded values.
    by_code: dict[str, tuple] = {}
    for country, unit, sterling, rate in rows:
        resolved = resolve_code(code_map, country, unit, period)
        if resolved is None:
            dropped.append(f"{country}/{unit.strip()}")
            continue
        code, confidence = resolved
        if reference.get(code) and not 0.5 < float(rate) / reference[code] < 2.0:
            dropped.append(f"{country}/{unit.strip()} ({code} {rate} vs monthly {reference[code]})")
            continue
        unit_clean = unit.strip().removeprefix(f"{code} ").strip()
        principal = match_key(country) in PRINCIPALS.get(code, {match_key(country)})
        precision = len(rate.split(".")[1]) if "." in rate else 0
        candidate = (confidence, principal, precision, country, unit_clean, sterling, rate)
        best = by_code.get(code)
        if best is None or candidate[:3] > best[:3]:
            if best is not None and best[6] != rate:
                log(f"  {period} {code}: kept {rate} ({country}), discarded {best[6]} ({best[3]})")
            by_code[code] = candidate
        elif best[6] != rate:
            log(f"  {period} {code}: kept {best[6]} ({best[3]}), discarded {rate} ({country})")

    out = io.StringIO()
    writer = csv.writer(out, lineterminator="\n")
    writer.writerow(CANONICAL_HEADER)
    for code in sorted(by_code):
        _, _, _, country, unit_clean, sterling, rate = by_code[code]
        writer.writerow([country, unit_clean, code, sterling, rate])
    target.write_text(out.getvalue(), encoding="utf-8")
    note = f" (dropped: {', '.join(dropped)})" if dropped else ""
    log(f"wrote {target} ({len(by_code)} rows){note}")


# --- series backfills ----------------------------------------------------------

def backfill_monthly() -> None:
    for period, (ts, url) in MONTHLY_2014.items():
        target = DATA / "monthly" / f"{period}.xml"
        if target.exists():
            continue
        data = archived(f"monthly{period}.xml", ts, url)
        root = ET.fromstring(data)
        assert root.tag == "exchangeRateMonthList", f"{period}: bad root"
        assert len(root) >= 100, f"{period}: too few entries"
        y, m = period.split("-")
        assert f"/{y}" in root.attrib["Period"], f"{period}: period mismatch"
        target.write_bytes(data)
        log(f"wrote {target} ({len(root)} entries)")


def backfill_csvs(kind: str, sources: list, code_map: dict, force: bool = False) -> None:
    for name, ts, url, fallback in sources:
        text = decode(archived(name, ts, url))
        for period, rows in parse_wide_csv(text, fallback).items():
            target = DATA / kind / f"{period}.csv"
            if target.exists() and not force:
                continue
            write_canonical(kind, period, rows, code_map)


def normalize_api_era(kind: str, code_map: dict) -> None:
    """Rewrite any file whose header deviates from the canonical shape."""
    canonical = ",".join(CANONICAL_HEADER)
    for path in sorted((DATA / kind).glob("*.csv")):
        text = decode(path.read_bytes())
        first = (text.splitlines()[0] if text else "").strip()
        if first == canonical:
            continue
        period = path.stem
        rows = extract_rows(text, period)
        write_canonical(kind, period, rows, code_map)


def extract_rows(text: str, period: str) -> list[tuple[str, str, str, str]]:
    """Rows for `period` from a possibly code-bearing, possibly wide CSV."""
    rows = [r for r in csv.reader(io.StringIO(text))]
    header_i, header = locate_header(rows)
    code_col = next((i for i, h in enumerate(header) if "code" in norm(h)), None)
    if code_col is None:
        parsed = parse_wide_csv(text, [period])
        return parsed.get(period) or next(iter(parsed.values()))
    unit_col = unit_column(header)
    rate_col = max(i for i, h in enumerate(header) if "units per" in norm(h))
    sterling_col = max(i for i, h in enumerate(header) if "sterling" in norm(h))
    out = []
    for row in rows[header_i + 1:]:
        if len(row) <= rate_col or not row[0].strip() or not row[rate_col].strip():
            continue
        # Prefix the unit with the source code so resolve_code() keeps it verbatim.
        out.append((row[0].strip(), f"{row[code_col].strip()} {row[unit_col].strip()}", row[sterling_col].strip(), row[rate_col].strip()))
    return out


def backfill_spot_html(code_map: dict) -> None:
    for raw in SPOT_HTML_PERIODS:
        year, month = raw.split("-")
        period = f"{year}-{int(month):02d}"
        target = DATA / "spot" / f"{period}.csv"
        if target.exists():
            continue
        html = decode(cached(f"tt{raw}.html", f"https://www.trade-tariff.service.gov.uk/exchange_rates/view/{raw}?type=spot"))
        cells = re.findall(r'<td class="govuk-table__cell[^"]*">\s*([^<]*?)\s*</td>', html)
        rows = []
        for i in range(0, len(cells) - 3, 6):
            country, unit, code, rate = cells[i], cells[i + 1], cells[i + 2], cells[i + 3]
            if re.fullmatch(r"[A-Z]{3}", code) and re.fullmatch(r"[0-9.]+", rate):
                rows.append((country, f"{code} {unit}", "", rate))
        assert len(rows) >= 8, f"{period}: scraped only {len(rows)} rows"
        write_canonical("spot", period, rows, code_map)


def backfill_weekly() -> None:
    target = DATA / "weekly" / "amendments.csv"
    if target.exists():
        return
    date_formats = {
        "weekly2014.csv": "%d/%m/%Y",
        "weekly2015.csv": "%d-%b-%y",
        "weekly2016.csv": "%d %B %Y",
    }
    from datetime import datetime

    entries = []
    for name, ts, url, _ in WEEKLY_SOURCES:
        text = decode(archived(name, ts, url))
        reader = csv.DictReader(io.StringIO(text))
        fields = {norm(k): k for k in reader.fieldnames or []}
        for row in reader:
            raw_date = row[fields["date of change"]].strip()
            if not raw_date:
                continue
            date = datetime.strptime(raw_date, date_formats[name]).date()
            code = row[fields["currency code"]].strip().upper()
            rate = row[fields["rate"]].strip()
            assert re.fullmatch(r"[A-Z]{3}", code), f"bad code {code!r}"
            assert float(rate) > 0, f"bad rate {rate!r}"
            entries.append((date.isoformat(), row[fields["country name"]].strip(), row[fields["currency name"]].strip(), code, rate))
    entries.sort()
    target.parent.mkdir(exist_ok=True)
    with target.open("w", encoding="utf-8", newline="") as fh:
        writer = csv.writer(fh, lineterminator="\n")
        writer.writerow(["Date", "Country", "Currency Name", "Currency Code", "Rate"])
        writer.writerows(entries)
    log(f"wrote {target} ({len(entries)} amendments, {entries[0][0]}..{entries[-1][0]})")


def validate() -> None:
    problems = []
    expected = {f"{y}-12" for y in range(2010, 2026)} | {f"{y}-03" for y in range(2011, 2027)}
    for kind in ("average", "spot"):
        have = {p.stem for p in DATA.glob(f"{kind}/*.csv")}
        if missing := sorted(expected - have):
            problems.append(f"{kind} missing: {missing}")
    for path in sorted(DATA.glob("*/*.csv")):
        if path.parent.name == "weekly":
            continue
        with path.open(encoding="utf-8") as fh:
            rows = list(csv.DictReader(fh))
        for row in rows:
            code = row.get("Currency Code", "")
            rate = row.get("Currency Units per £1", "")
            if not re.fullmatch(r"[A-Z]{3}", code or ""):
                problems.append(f"{path}: bad code {code!r}")
            if not (rate and float(rate) > 0):
                problems.append(f"{path}: bad rate {rate!r} for {code}")
        floor = 8 if path.parent.name == "spot" else 140
        if len(rows) < floor:
            problems.append(f"{path}: only {len(rows)} rows")
    if problems:
        raise SystemExit("VALIDATION FAILED:\n" + "\n".join(problems[:40]))
    log("validation OK")


def sanity() -> None:
    import decimal
    for period, lo, hi in [("2020-12", 1.2, 1.5), ("2011-03", 1.4, 1.8)]:
        path = DATA / "spot" / f"{period}.csv"
        with path.open(encoding="utf-8") as fh:
            for row in csv.DictReader(fh):
                if row["Currency Code"] == "USD":
                    rate = decimal.Decimal(row["Currency Units per £1"])
                    log(f"sanity: spot {period} USD {rate} (expected {lo}-{hi})")
                    assert lo < rate < hi, f"USD spot {period} out of range: {rate}"


def main() -> None:
    force = "--force" in sys.argv
    (DATA / "weekly").mkdir(exist_ok=True)
    backfill_monthly()
    code_map = build_code_map()
    normalize_api_era("average", code_map)
    normalize_api_era("spot", code_map)
    backfill_csvs("average", AVERAGE_SOURCES, code_map, force)
    backfill_csvs("spot", SPOT_SOURCES, code_map, force)
    backfill_spot_html(code_map)
    backfill_weekly()
    validate()
    sanity()
    log("done")


if __name__ == "__main__":
    main()
