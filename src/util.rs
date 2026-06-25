//! Date helpers, Persian text cleanup, and small utilities.

use time::{Date, Month, OffsetDateTime};

/// Replicates the JS `cleanFa`: strips zero-width characters and normalizes
/// the Arabic kaf/yeh to their Persian forms.
pub fn clean_fa(s: &str) -> String {
    s.replace('\u{200B}', "") // zero-width space
        .replace('\u{200C}', " ") // zero-width non-joiner -> space (the JS optionally trims surrounding spaces; collapse handled by trim() at call sites)
        .replace(['\u{200D}', '\u{FEFF}'], "") // zero-width no-break space
        .replace('\u{0643}', "\u{06A9}") // ك -> ک
        .replace('\u{064A}', "\u{06CC}") // ي -> ی
}

/// `Date` -> "YYYYMMDD" string (JS `dateToStr`).
pub fn date_to_str(d: Date) -> String {
    format!("{:04}{:02}{:02}", d.year(), d.month() as u8, d.day())
}

/// Today's local date as `Date`.
pub fn today() -> Date {
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .date()
}

fn month_from_u8(m: u8) -> Month {
    Month::try_from(m).unwrap_or(Month::January)
}

/// "YYYYMMDD" -> `Date` (JS `strToDate`).
pub fn str_to_date(s: &str) -> Option<Date> {
    if s.len() < 8 {
        return None;
    }
    let y: i32 = s[0..4].parse().ok()?;
    let m: u8 = s[4..6].parse().ok()?;
    let d: u8 = s[6..8].parse().ok()?;
    Date::from_calendar_date(y, month_from_u8(m), d).ok()
}

/// Gregorian "YYYYMMDD" -> Jalali "YYYYMMDD" (JS `gregToShamsi`).
/// `ptime` uses 0-based months for both input and output.
pub fn greg_to_shamsi(s: &str) -> String {
    if s.len() < 8 {
        return s.to_string();
    }
    let y: i32 = s[0..4].parse().unwrap_or(0);
    let m: i32 = s[4..6].parse().unwrap_or(0);
    let d: i32 = s[6..8].parse().unwrap_or(0);
    match ptime::from_gregorian_date(y, m - 1, d) {
        Some(pt) => format!("{:04}{:02}{:02}", pt.tm_year, pt.tm_mon + 1, pt.tm_mday),
        None => s.to_string(),
    }
}

/// Jalali "YYYYMMDD" -> Gregorian "YYYYMMDD" (JS `shamsiToGreg`).
pub fn shamsi_to_greg(s: &str) -> String {
    if s.len() < 8 {
        return s.to_string();
    }
    let y: i32 = s[0..4].parse().unwrap_or(0);
    let m: i32 = s[4..6].parse().unwrap_or(0);
    let d: i32 = s[6..8].parse().unwrap_or(0);
    match ptime::from_persian_date(y, m - 1, d) {
        Some(pt) => {
            let g = pt.to_gregorian();
            format!("{:04}{:02}{:02}", g.tm_year + 1900, g.tm_mon + 1, g.tm_mday)
        }
        None => s.to_string(),
    }
}

/// Absolute day difference between two "YYYYMMDD" strings (JS `dayDiff`).
pub fn day_diff(s1: &str, s2: &str) -> i64 {
    let (d1, d2) = match (str_to_date(s1), str_to_date(s2)) {
        (Some(a), Some(b)) => (a, b),
        _ => return 0,
    };
    (d2 - d1).whole_days().abs()
}

/// Split a slice into chunks of `size` (JS `splitArr`).
pub fn split_chunks<T: Clone>(items: &[T], size: usize) -> Vec<Vec<T>> {
    if size == 0 {
        return vec![];
    }
    items.chunks(size).map(|c| c.to_vec()).collect()
}

/// time::Weekday as JS `Date.getDay()` (Sunday = 0 .. Saturday = 6).
fn js_weekday(d: Date) -> u8 {
    // time::Weekday::number_days_from_sunday() returns 0..=6 with Sunday=0
    d.weekday().number_days_from_sunday()
}

const UPDATE_INTERVAL: i64 = 1;
const TRADING_SESSION_END_HOUR: u8 = 16;

/// Replicates the JS `shouldUpdate`.
pub fn should_update(deven: &str, last_possible_deven: &str) -> bool {
    if deven.is_empty() || deven == "0" {
        return true;
    }
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let today_d = now.date();
    let today_deven = date_to_str(today_d);
    let days_passed = day_diff(last_possible_deven, deven);
    let in_weekend = matches!(js_weekday(today_d), 4 | 5); // Thursday=4, Friday=5
    let last_update_weekday = str_to_date(last_possible_deven)
        .map(js_weekday)
        .unwrap_or(0);

    days_passed >= UPDATE_INTERVAL
        && (if today_deven == last_possible_deven {
            now.hour() > TRADING_SESSION_END_HOUR
        } else {
            true
        })
        && !(in_weekend && last_update_weekday != 3 && days_passed <= 3)
}
