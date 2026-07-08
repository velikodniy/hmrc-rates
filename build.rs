// Build-time codegen: parse data/ and emit static rate tables.
// A bad data file must fail the build, so panics here are the correct tool.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "src/parse.rs"]
mod parse;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use parse::{ParsedRate, dedup_majority};

fn main() {
    println!("cargo:rerun-if-changed=data");
    if std::env::var_os("CARGO_FEATURE_BUNDLED").is_none() {
        return;
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR not set"));

    let monthly = load_monthly(Path::new("data/monthly"));
    let average = load_year_end(Path::new("data/average"), "average");
    let spot = load_year_end(Path::new("data/spot"), "spot");
    let weekly = load_weekly(Path::new("data/weekly"));

    let mut code = String::new();
    emit_series(&mut code, "MONTHLY", &monthly);
    emit_series(&mut code, "SPOT", &spot);
    emit_series(&mut code, "AVERAGE", &average);
    emit_weeks(&mut code, "WEEKLY", &weekly);

    std::fs::write(out.join("bundled.rs"), code).expect("failed to write bundled.rs");
}

/// Sorted (period key, sorted deduped rates) for one series.
type SeriesData = Vec<(i32, Vec<ParsedRate>)>;

fn sorted_files(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new(); // absent directory = empty series
    };
    let mut files: Vec<PathBuf> = entries
        .map(|e| e.expect("unreadable dir entry").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(extension))
        .collect();
    files.sort();
    files
}

/// "YYYY-MM" from a data file name.
fn file_period(path: &Path) -> (i32, u32) {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    parse::parse_year_month(stem)
        .unwrap_or_else(|| panic!("{}: file name is not YYYY-MM", path.display()))
}

fn load_monthly(dir: &Path) -> SeriesData {
    let mut series = SeriesData::new();
    for path in sorted_files(dir, "xml") {
        let (year, month) = file_period(&path);
        let bytes = std::fs::read(&path).expect("unreadable file");
        let ((py, pm), rates) =
            parse::parse_monthly_xml(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(
            (py, pm),
            (year, month),
            "{}: Period does not match file name",
            path.display()
        );
        let rates = dedup_majority(rates).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            rates.len() >= 100,
            "{}: implausibly few rates",
            path.display()
        );
        series.push((year * 12 + month as i32 - 1, rates));
    }
    assert!(!series.is_empty(), "data/monthly is empty");
    series.sort_by_key(|(k, _)| *k);
    for pair in series.windows(2) {
        assert!(
            pair[1].0 == pair[0].0 + 1,
            "gap in monthly data between keys {} and {}",
            pair[0].0,
            pair[1].0
        );
    }
    series
}

fn load_year_end(dir: &Path, label: &str) -> SeriesData {
    let mut series = SeriesData::new();
    for path in sorted_files(dir, "csv") {
        let (year, month) = file_period(&path);
        assert!(
            month == 3 || month == 12,
            "{}: {label} period must end in March or December",
            path.display()
        );
        let bytes = std::fs::read(&path).expect("unreadable file");
        let rates = parse::parse_rates_csv(&bytes)
            .and_then(dedup_majority)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        series.push((year * 2 + (month == 12) as i32, rates));
    }
    series.sort_by_key(|(k, _)| *k);
    series
}

/// Weekly amendments grouped into validity ranges: (start_day, end_day, rates).
fn load_weekly(dir: &Path) -> Vec<(i32, i32, Vec<ParsedRate>)> {
    let mut rows = Vec::new();
    for path in sorted_files(dir, "csv") {
        let bytes = std::fs::read(&path).expect("unreadable file");
        rows.extend(
            parse::parse_weekly_csv(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
        );
    }
    if rows.is_empty() {
        return Vec::new();
    }

    let mut by_day = std::collections::BTreeMap::<i32, Vec<ParsedRate>>::new();
    for row in rows {
        let (y, m, d) = row.date;
        assert!(
            (2014..=2016).contains(&y),
            "weekly amendment date {y}-{m:02}-{d:02} outside the 2014-2016 series"
        );
        by_day
            .entry(parse::date::days_from_civil(y, m, d))
            .or_default()
            .push(row.rate);
    }

    let days: Vec<i32> = by_day.keys().copied().collect();
    by_day
        .into_iter()
        .enumerate()
        .map(|(i, (start, rates))| {
            let rates = dedup_majority(rates)
                .unwrap_or_else(|e| panic!("weekly amendments for day {start}: {e}"));
            // A week runs 7 days unless the next amendment lands sooner
            let end = match days.get(i + 1) {
                Some(next) => (start + 6).min(next - 1),
                None => start + 6,
            };
            (start, end, rates)
        })
        .collect()
}

fn emit_entries(code: &mut String, rates: &[ParsedRate]) {
    for r in rates {
        let c = core::str::from_utf8(&r.code).expect("code is ASCII");
        writeln!(
            code,
            "    crate::store::Entry {{ mantissa: {}, code: *b\"{}\", scale: {} }},",
            r.mantissa, c, r.scale
        )
        .expect("write to String");
    }
}

/// Emits `{name}_ARENA` from the per-period rate slices.
fn emit_arena<'a>(code: &mut String, name: &str, tables: impl Iterator<Item = &'a [ParsedRate]>) {
    writeln!(code, "static {name}_ARENA: &[crate::store::Entry] = &[").expect("write");
    for rates in tables {
        emit_entries(code, rates);
    }
    writeln!(code, "];").expect("write");
}

fn emit_series(code: &mut String, name: &str, series: &SeriesData) {
    emit_arena(code, name, series.iter().map(|(_, r)| r.as_slice()));

    writeln!(code, "static {name}_INDEX: &[crate::store::PeriodIdx] = &[").expect("write");
    let mut end = 0u32;
    for (key, rates) in series {
        end += rates.len() as u32;
        writeln!(
            code,
            "    crate::store::PeriodIdx {{ key: {key}, end: {end} }},"
        )
        .expect("write");
    }
    writeln!(code, "];").expect("write");

    writeln!(
        code,
        "pub(crate) static {name}: crate::store::StaticSeries = \
         crate::store::StaticSeries {{ index: {name}_INDEX, arena: {name}_ARENA }};\n"
    )
    .expect("write");
}

fn emit_weeks(code: &mut String, name: &str, weeks: &[(i32, i32, Vec<ParsedRate>)]) {
    emit_arena(code, name, weeks.iter().map(|(_, _, r)| r.as_slice()));

    writeln!(code, "static {name}_INDEX: &[crate::store::WeekIdx] = &[").expect("write");
    let mut end = 0u32;
    for (start_day, end_day, rates) in weeks {
        end += rates.len() as u32;
        writeln!(
            code,
            "    crate::store::WeekIdx {{ start_day: {start_day}, end_day: {end_day}, end: {end} }},"
        )
        .expect("write");
    }
    writeln!(code, "];").expect("write");

    writeln!(
        code,
        "pub(crate) static {name}: crate::store::StaticWeeks = \
         crate::store::StaticWeeks {{ index: {name}_INDEX, arena: {name}_ARENA }};\n"
    )
    .expect("write");
}
