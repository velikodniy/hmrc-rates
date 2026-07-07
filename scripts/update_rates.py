#!/usr/bin/env python3
"""Download new HMRC exchange rate files from the Trade Tariff API into data/.

Usage: update_rates.py [--data-dir data] [--verbose]
Emits GitHub Actions outputs on stdout: has-new, release-body, downloaded, skipped, failed.
"""

import argparse
import calendar
import json
import pathlib
import re
import sys
import urllib.error
import urllib.request

HOST = "https://www.trade-tariff.service.gov.uk"
API = f"{HOST}/api/v2/exchange_rates"
USER_AGENT = "hmrc-rates-updater (+https://github.com/velikodniy/hmrc-rates)"
TIMEOUT = 30.0

# type -> (wanted format, local subdir)
SERIES = {
    "monthly": ("xml", "monthly"),
    "average": ("csv", "average"),
    "spot": ("csv", "spot"),
}
FILE_RE = re.compile(r"/(monthly|average|spot)_(xml|csv)_(\d{4})-(\d{1,2})\.(xml|csv)$")


def fetch(url: str) -> bytes | None:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            return resp.read()
    except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError):
        return None


def get_json(url: str) -> dict | None:
    data = fetch(url)
    return json.loads(data) if data else None


def discover_files(rate_type: str) -> dict[tuple[int, int], str]:
    """Map (year, month) -> API file path, across all published years."""
    fmt, _ = SERIES[rate_type]
    doc = get_json(f"{API}/period_lists?filter%5Btype%5D={rate_type}")
    if doc is None:
        return {}
    years = [
        int(y["id"].split("-", 1)[0])
        for y in doc["data"]["relationships"]["exchange_rate_years"]["data"]
    ]
    found: dict[tuple[int, int], str] = {}
    for year in years:
        ydoc = get_json(f"{API}/period_lists?filter%5Btype%5D={rate_type}&year={year}")
        if ydoc is None:
            continue
        for inc in ydoc.get("included", []):
            if inc["type"] != "exchange_rate_file":
                continue
            m = FILE_RE.search(inc["attributes"]["file_path"])
            if m and m.group(1) == rate_type and m.group(2) == fmt:
                found[(int(m.group(3)), int(m.group(4)))] = inc["attributes"]["file_path"]
    return found


def looks_valid(payload: bytes, fmt: str) -> bool:
    head = payload.lstrip()[:64]
    return head.startswith(b"<?xml") if fmt == "xml" else b"," in head


def update_series(rate_type: str, data_dir: pathlib.Path, verbose: bool):
    fmt, subdir = SERIES[rate_type]
    target_dir = data_dir / subdir
    target_dir.mkdir(parents=True, exist_ok=True)

    downloaded, skipped, failed = [], 0, 0
    for (year, month), path in sorted(discover_files(rate_type).items()):
        target = target_dir / f"{year}-{month:02d}.{fmt}"
        if target.exists():
            skipped += 1
            continue
        payload = fetch(HOST + path)
        if payload is None or not looks_valid(payload, fmt):
            failed += 1
            if verbose:
                print(f"failed {rate_type} {year}-{month:02d}", file=sys.stderr)
            continue
        target.write_bytes(payload)
        downloaded.append((rate_type, year, month))
        if verbose:
            print(f"downloaded {target}", file=sys.stderr)
    return downloaded, skipped, failed


def release_body(downloaded: list[tuple[str, int, int]]) -> str:
    if not downloaded:
        return ""
    labels = [f"{t} {calendar.month_name[m]} {y}" for t, y, m in sorted(downloaded)]
    return f"Updated HMRC rates: {', '.join(labels)}"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--data-dir", type=pathlib.Path, default=pathlib.Path("data"))
    parser.add_argument("--verbose", action="store_true")
    args = parser.parse_args()

    downloaded, skipped, failed = [], 0, 0
    for rate_type in SERIES:
        d, s, f = update_series(rate_type, args.data_dir, args.verbose)
        downloaded += d
        skipped += s
        failed += f

    print(f"has-new={'true' if downloaded else 'false'}")
    print(f"release-body={release_body(downloaded)}")
    print(f"downloaded={len(downloaded)}")
    print(f"skipped={skipped}")
    print(f"failed={failed}")


if __name__ == "__main__":
    main()
