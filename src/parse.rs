// Shared by build.rs via #[path]; must not use `crate::` paths.
// Parses the canonical committed/downloaded formats into (code, mantissa, scale) rows.

#[path = "date.rs"]
pub(crate) mod date;

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

fn err<T>(reason: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError(reason.into()))
}

/// One parsed rate: `mantissa / 10^scale` currency units per £1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedRate {
    pub code: [u8; 3],
    pub mantissa: u64,
    pub scale: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // consumed by build.rs; the weekly series is never fetched at runtime
pub struct WeeklyRow {
    pub date: (i32, u32, u32), // ISO year, month, day
    pub rate: ParsedRate,
}

/// Parses a positive decimal like "4.9226" into (mantissa, scale), losslessly.
pub fn parse_rate_decimal(s: &str) -> Result<(u64, u8), ParseError> {
    let s = s.trim();
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return err(format!("empty rate '{s}'"));
    }
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return err(format!("malformed rate '{s}'"));
    }
    if frac_part.len() > 9 {
        return err(format!("rate '{s}' has more than 9 decimal places"));
    }
    let mut mantissa: u64 = 0;
    for b in int_part.bytes().chain(frac_part.bytes()) {
        mantissa = mantissa
            .checked_mul(10)
            .and_then(|m| m.checked_add(u64::from(b - b'0')))
            .ok_or_else(|| ParseError(format!("rate '{s}' overflows")))?;
    }
    if mantissa == 0 {
        return err(format!("rate '{s}' is not positive"));
    }
    Ok((mantissa, frac_part.len() as u8))
}

pub fn parse_code(s: &str) -> Result<[u8; 3], ParseError> {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() != 3 || !b.iter().all(|c| c.is_ascii_alphabetic()) {
        return err(format!("bad currency code '{s}'"));
    }
    Ok([
        b[0].to_ascii_uppercase(),
        b[1].to_ascii_uppercase(),
        b[2].to_ascii_uppercase(),
    ])
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn parse_dmy(s: &str) -> Result<(i32, u32, u32), ParseError> {
    // "01/Jul/2026"
    let mut parts = s.trim().split('/');
    let (Some(d), Some(m), Some(y), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return err(format!("bad date '{s}'"));
    };
    let day: u32 = d
        .parse()
        .map_err(|_| ParseError(format!("bad day in '{s}'")))?;
    let month = MONTHS
        .iter()
        .position(|name| name.eq_ignore_ascii_case(m))
        .ok_or_else(|| ParseError(format!("bad month in '{s}'")))? as u32
        + 1;
    let year: i32 = y
        .parse()
        .map_err(|_| ParseError(format!("bad year in '{s}'")))?;
    Ok((year, month, day))
}

/// Parses a monthly `Period` attribute ("01/Jul/2026 to 31/Jul/2026") and
/// checks it spans exactly one calendar month.
pub fn parse_month_period(period: &str) -> Result<(i32, u32), ParseError> {
    let (start, end) = period
        .split_once(" to ")
        .ok_or_else(|| ParseError(format!("bad Period '{period}'")))?;
    let (sy, sm, sd) = parse_dmy(start)?;
    let (ey, em, ed) = parse_dmy(end)?;
    if sd != 1 {
        return err(format!("Period '{period}' does not start on day 1"));
    }
    if (ey, em) != (sy, sm) || ed != date::days_in_month(sy, sm) {
        return err(format!("Period '{period}' does not span exactly one month"));
    }
    Ok((sy, sm))
}

/// Parses HMRC monthly XML; returns the period and raw (possibly duplicated) rates.
pub fn parse_monthly_xml(bytes: &[u8]) -> Result<((i32, u32), Vec<ParsedRate>), ParseError> {
    use quick_xml::events::Event;

    let text = std::str::from_utf8(bytes).map_err(|_| ParseError("XML is not UTF-8".into()))?;
    let mut reader = quick_xml::Reader::from_str(text);
    reader.config_mut().trim_text(true);

    let mut period: Option<(i32, u32)> = None;
    let mut rates = Vec::new();
    let mut field: Option<&'static str> = None;
    let mut code: Option<[u8; 3]> = None;
    let mut rate: Option<(u64, u8)> = None;

    loop {
        match reader.read_event() {
            Err(e) => return err(format!("XML error: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(el)) => match el.local_name().as_ref() {
                b"exchangeRateMonthList" => {
                    for attr in el.attributes() {
                        let attr = attr.map_err(|e| ParseError(format!("bad attribute: {e}")))?;
                        if attr.key.as_ref() == b"Period" {
                            let value = attr
                                .unescape_value()
                                .map_err(|e| ParseError(format!("bad Period: {e}")))?;
                            period = Some(parse_month_period(&value)?);
                        }
                    }
                    if period.is_none() {
                        return err("exchangeRateMonthList has no Period attribute");
                    }
                }
                b"exchangeRate" => {
                    code = None;
                    rate = None;
                }
                b"currencyCode" => field = Some("code"),
                b"rateNew" => field = Some("rate"),
                _ => field = None,
            },
            Ok(Event::Text(t)) => {
                let value = t
                    .unescape()
                    .map_err(|e| ParseError(format!("bad text: {e}")))?;
                match field {
                    Some("code") => code = Some(parse_code(&value)?),
                    Some("rate") => rate = Some(parse_rate_decimal(&value)?),
                    _ => {}
                }
            }
            Ok(Event::End(el)) => {
                if el.local_name().as_ref() == b"exchangeRate" {
                    let (Some(c), Some((mantissa, scale))) = (code.take(), rate.take()) else {
                        return err("exchangeRate record missing currencyCode or rateNew");
                    };
                    rates.push(ParsedRate {
                        code: c,
                        mantissa,
                        scale,
                    });
                }
                field = None;
            }
            Ok(_) => {}
        }
    }

    let period = period.ok_or_else(|| ParseError("no exchangeRateMonthList element".into()))?;
    if rates.is_empty() {
        return err("no exchangeRate records");
    }
    Ok((period, rates))
}

/// Parses the canonical average/spot CSV (`…,Currency Code,…,Currency Units per £1`).
pub fn parse_rates_csv(bytes: &[u8]) -> Result<Vec<ParsedRate>, ParseError> {
    let text = decode_utf8_lossy_bom(bytes);
    let mut reader = csv::Reader::from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|e| ParseError(format!("bad CSV: {e}")))?;

    let col = |needle: &str| {
        headers
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(needle) || h.trim().starts_with(needle))
    };
    let code_col =
        col("Currency Code").ok_or_else(|| ParseError("no 'Currency Code' column".into()))?;
    let rate_col = col("Currency Units per")
        .ok_or_else(|| ParseError("no 'Currency Units per £1' column".into()))?;

    let mut rates = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| ParseError(format!("bad CSV row: {e}")))?;
        let code = record.get(code_col).unwrap_or("");
        let rate = record.get(rate_col).unwrap_or("");
        if code.trim().is_empty() && rate.trim().is_empty() {
            continue; // blank/trailing line
        }
        let (mantissa, scale) = parse_rate_decimal(rate)?;
        rates.push(ParsedRate {
            code: parse_code(code)?,
            mantissa,
            scale,
        });
    }
    if rates.is_empty() {
        return err("CSV has no rate rows");
    }
    Ok(rates)
}

/// Parses the normalized weekly amendments CSV (`Date,Country,Currency Name,Currency Code,Rate`).
#[allow(dead_code)] // consumed by build.rs; the weekly series is never fetched at runtime
pub fn parse_weekly_csv(bytes: &[u8]) -> Result<Vec<WeeklyRow>, ParseError> {
    let text = decode_utf8_lossy_bom(bytes);
    let mut reader = csv::Reader::from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|e| ParseError(format!("bad CSV: {e}")))?;
    let find = |name: &str| {
        headers
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
    };
    let (Some(date_col), Some(code_col), Some(rate_col)) =
        (find("Date"), find("Currency Code"), find("Rate"))
    else {
        return err("weekly CSV missing Date/Currency Code/Rate columns");
    };

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| ParseError(format!("bad CSV row: {e}")))?;
        let date = parse_iso_date(record.get(date_col).unwrap_or(""))?;
        let code = parse_code(record.get(code_col).unwrap_or(""))?;
        let (mantissa, scale) = parse_rate_decimal(record.get(rate_col).unwrap_or(""))?;
        rows.push(WeeklyRow {
            date,
            rate: ParsedRate {
                code,
                mantissa,
                scale,
            },
        });
    }
    if rows.is_empty() {
        return err("weekly CSV has no rows");
    }
    Ok(rows)
}

fn parse_iso_date(s: &str) -> Result<(i32, u32, u32), ParseError> {
    let mut parts = s.trim().split('-');
    let (Some(y), Some(m), Some(d), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return err(format!("bad ISO date '{s}'"));
    };
    let year: i32 = y
        .parse()
        .map_err(|_| ParseError(format!("bad ISO date '{s}'")))?;
    let month: u32 = m
        .parse()
        .map_err(|_| ParseError(format!("bad ISO date '{s}'")))?;
    let day: u32 = d
        .parse()
        .map_err(|_| ParseError(format!("bad ISO date '{s}'")))?;
    if !(1..=12).contains(&month) || day == 0 || day > date::days_in_month(year, month) {
        return err(format!("invalid date '{s}'"));
    }
    Ok((year, month, day))
}

/// Dedups multi-country rows (EUR appears ~19 times). Conflicts resolve by
/// majority; ties resolve to the most precise value when the tied values agree
/// after rounding to the lower precision (e.g. USA 1.5958134 vs Liberia
/// 1.595813), otherwise error. Output is sorted by code.
pub fn dedup_majority(mut rates: Vec<ParsedRate>) -> Result<Vec<ParsedRate>, ParseError> {
    rates.sort_unstable_by_key(|r| (r.code, r.mantissa, r.scale));
    let mut out: Vec<ParsedRate> = Vec::with_capacity(rates.len());
    let mut group_start = 0;
    while group_start < rates.len() {
        let code = rates[group_start].code;
        let group: Vec<ParsedRate> = rates[group_start..]
            .iter()
            .take_while(|r| r.code == code)
            .copied()
            .collect();
        group_start += group.len();

        let mut distinct: Vec<(ParsedRate, usize)> = Vec::new();
        for rate in &group {
            match distinct
                .iter_mut()
                .find(|(r, _)| (r.mantissa, r.scale) == (rate.mantissa, rate.scale))
            {
                Some((_, count)) => *count += 1,
                None => distinct.push((*rate, 1)),
            }
        }
        let best_count = distinct.iter().map(|(_, c)| *c).max().unwrap_or(0);
        let mut tied: Vec<ParsedRate> = distinct
            .iter()
            .filter(|(_, c)| *c == best_count)
            .map(|(r, _)| *r)
            .collect();
        tied.sort_unstable_by_key(|r| r.scale);
        let winner = match tied.as_slice() {
            [single] => *single,
            [] => continue,
            _ => {
                let least_precise = tied[0];
                let consistent = tied.iter().all(|r| {
                    rounded_to(r.mantissa, r.scale, least_precise.scale)
                        == Some(least_precise.mantissa)
                });
                if !consistent {
                    let code_str = String::from_utf8_lossy(&code).into_owned();
                    return err(format!(
                        "conflicting duplicate rates for '{code_str}' with no majority"
                    ));
                }
                tied[tied.len() - 1] // most precise of the agreeing values
            }
        };
        out.push(winner);
    }
    Ok(out)
}

/// Rounds `mantissa` at `scale` half-up to `target` decimal places.
fn rounded_to(mantissa: u64, scale: u8, target: u8) -> Option<u64> {
    if scale <= target {
        return mantissa.checked_mul(10u64.checked_pow(u32::from(target - scale))?);
    }
    let divisor = 10u64.checked_pow(u32::from(scale - target))?;
    Some((mantissa + divisor / 2) / divisor)
}

fn decode_utf8_lossy_bom(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn rate(code: &[u8; 3], mantissa: u64, scale: u8) -> ParsedRate {
        ParsedRate {
            code: *code,
            mantissa,
            scale,
        }
    }

    #[test]
    fn decimal_parsing() {
        assert_eq!(parse_rate_decimal("4.9226").unwrap(), (49226, 4));
        assert_eq!(parse_rate_decimal("13").unwrap(), (13, 0));
        assert_eq!(parse_rate_decimal(" 0.5 ").unwrap(), (5, 1));
        assert_eq!(
            parse_rate_decimal("6413696.5388").unwrap(),
            (64136965388, 4)
        );
        assert!(parse_rate_decimal("0").is_err());
        assert!(parse_rate_decimal("0.000").is_err());
        assert!(parse_rate_decimal("-1.5").is_err());
        assert!(parse_rate_decimal("1.2.3").is_err());
        assert!(parse_rate_decimal("1.0123456789").is_err()); // > 9 dp
        assert!(parse_rate_decimal("").is_err());
    }

    #[test]
    fn month_period_validation() {
        assert_eq!(
            parse_month_period("01/Jul/2026 to 31/Jul/2026").unwrap(),
            (2026, 7)
        );
        assert_eq!(
            parse_month_period("01/Feb/2014 to 28/Feb/2014").unwrap(),
            (2014, 2)
        );
        assert_eq!(
            parse_month_period("01/Feb/2016 to 29/Feb/2016").unwrap(),
            (2016, 2)
        );
        assert!(parse_month_period("02/Aug/2025 to 31/Aug/2025").is_err()); // not day 1
        assert!(parse_month_period("01/Aug/2025 to 30/Aug/2025").is_err()); // wrong end
        assert!(parse_month_period("01/Feb/2015 to 29/Feb/2015").is_err()); // not a leap year
        assert!(parse_month_period("01/Aug/2025").is_err());
    }

    #[test]
    fn monthly_xml_happy_path() {
        let xml = r#"<?xml version="1.0"?>
            <exchangeRateMonthList Period="01/Aug/2025 to 31/Aug/2025">
              <exchangeRate><countryName>USA</countryName><currencyCode>USD</currencyCode><rateNew>1.3541</rateNew></exchangeRate>
              <exchangeRate><currencyCode>eur</currencyCode><rateNew>1.1547</rateNew></exchangeRate>
            </exchangeRateMonthList>"#;
        let ((year, month), rates) = parse_monthly_xml(xml.as_bytes()).unwrap();
        assert_eq!((year, month), (2025, 8));
        assert_eq!(rates, vec![rate(b"USD", 13541, 4), rate(b"EUR", 11547, 4)]);
    }

    #[test]
    fn monthly_xml_rejects_bad_input() {
        assert!(parse_monthly_xml(b"<exchangeRateMonthList/>").is_err()); // no Period
        let bad_period = br#"<exchangeRateMonthList Period="02/Aug/2025 to 31/Aug/2025"/>"#;
        assert!(parse_monthly_xml(bad_period).is_err());
        let missing_rate = br#"<exchangeRateMonthList Period="01/Aug/2025 to 31/Aug/2025">
            <exchangeRate><currencyCode>USD</currencyCode></exchangeRate>
            </exchangeRateMonthList>"#;
        assert!(parse_monthly_xml(missing_rate).is_err());
    }

    #[test]
    fn rates_csv_with_bom_and_quotes() {
        let csv = "\u{feff}Country,Unit Of Currency,Currency Code,Sterling value of Currency Unit £,Currency Units per £1\n\
                   \"Bonaire, Saba\",Dollar (US),USD,0.7386,1.3541\n\
                   Eurozone,Euro,EUR,0.8660,1.1547\n";
        let rates = parse_rates_csv(csv.as_bytes()).unwrap();
        assert_eq!(rates, vec![rate(b"USD", 13541, 4), rate(b"EUR", 11547, 4)]);
    }

    #[test]
    fn weekly_csv_parses_iso_rows() {
        let csv = "Date,Country,Currency Name,Currency Code,Rate\n\
                   2014-01-08,Turkey,Turkish Lira,TRY,3.5418\n\
                   2014-01-22,Argentina,Peso,ARS,10.976\n";
        let rows = parse_weekly_csv(csv.as_bytes()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].date, (2014, 1, 8));
        assert_eq!(rows[0].rate, rate(b"TRY", 35418, 4));
        assert!(parse_weekly_csv(b"Date,Currency Code,Rate\n2014-02-30,TRY,1").is_err());
    }

    #[test]
    fn dedup_collapses_identical_values() {
        let out = dedup_majority(vec![
            rate(b"EUR", 11547, 4),
            rate(b"AED", 49226, 4),
            rate(b"EUR", 11547, 4),
        ])
        .unwrap();
        assert_eq!(out, vec![rate(b"AED", 49226, 4), rate(b"EUR", 11547, 4)]);
    }

    #[test]
    fn dedup_majority_wins() {
        // The real XCD April 2015 case: six rows at 3.9831, one at 3.983.
        let mut rows = vec![rate(b"XCD", 39831, 4); 6];
        rows.push(rate(b"XCD", 3983, 3));
        assert_eq!(dedup_majority(rows).unwrap(), vec![rate(b"XCD", 39831, 4)]);
    }

    #[test]
    fn dedup_tie_prefers_precision_when_consistent() {
        // The real USD 2012-03 case: USA 1.5958134 vs Liberia 1.595813.
        let out =
            dedup_majority(vec![rate(b"USD", 1595813, 6), rate(b"USD", 15958134, 7)]).unwrap();
        assert_eq!(out, vec![rate(b"USD", 15958134, 7)]);
    }

    #[test]
    fn dedup_tie_with_conflicting_values_errors() {
        assert!(dedup_majority(vec![rate(b"USD", 1376045, 6), rate(b"USD", 1385863, 6)]).is_err());
    }
}
