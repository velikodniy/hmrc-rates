// Shared by build.rs via #[path]; must stay dependency-free and crate-path-free.

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
#[allow(dead_code)] // used by the crate::date instantiation, not the parse.rs one
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i32 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = i64::from((month + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) as i32
}

/// The number of days in a month (proleptic Gregorian).
#[allow(dead_code)] // used by the parse.rs instantiation, not the crate::date one
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            if leap { 29 } else { 28 }
        }
    }
}
