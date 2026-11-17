//! Time helpers: the clock, the system timezone, and chrono-compatible
//! calendar math, in one place.
//!
//! The crate moved off chrono (RUSTSEC-2026-0543, unmaintained) to jiff, with
//! one hard constraint: every rendered byte stays identical. Two facts shape
//! this module:
//!
//! * jiff's `Timestamp`/civil range is years -9999..=9999, while the old code's
//!   calendar spans roughly ±262,000 years (`period.rs` names and renders those
//!   extremes, e.g. `--since -200000-01-01`). The wide-range parsing and
//!   rendering below is therefore plain integer civil math (Howard Hinnant's
//!   algorithms), verified case-by-case against chrono's output.
//! * jiff disagrees with chrono on several reachable details (UTC renders as
//!   `Z` vs `+00:00`, fraction digits trimmed vs padded to 3/6/9, millis
//!   truncated vs floored, leap seconds, several accepted/rejected RFC 3339
//!   shapes). The parsers and renderers here replicate chrono exactly; the
//!   unit tests pin the probed outputs byte for byte.
//!
//! Live instants (the clock) and the system timezone come from jiff:
//! [`now_epoch_secs`], [`now_rfc3339_utc`], [`system_offset_at`].

use std::time::{SystemTime, UNIX_EPOCH};

/// The epoch range the old chrono calendar could express —
/// `NaiveDateTime::MIN` / `MAX` timestamps, frozen as literals (the upgrade
/// that moved them is gone; the range is now this module's contract, pinned by
/// `the_calendar_constants_still_match_the_old_chrono_range`).
pub const CALENDAR_MIN_EPOCH: i64 = -8_334_601_228_800;
/// See [`CALENDAR_MIN_EPOCH`].
pub const CALENDAR_MAX_EPOCH: i64 = 8_210_266_876_799;

/// Whether an instant names a date the calendar can express.
pub fn in_calendar(epoch: i64) -> bool {
    (CALENDAR_MIN_EPOCH..=CALENDAR_MAX_EPOCH).contains(&epoch)
}

/// The current instant as UTC epoch seconds (the clock read).
pub fn now_epoch_secs() -> i64 {
    jiff::Timestamp::now().as_second()
}

/// The current instant as total nanoseconds since the Unix epoch, sampled
/// once (splitting the read in two can straddle a second boundary).
pub fn now_nanos() -> i128 {
    let now = jiff::Timestamp::now();
    now.as_second() as i128 * 1_000_000_000 + i128::from(now.subsec_nanosecond())
}

/// The current instant as RFC 3339 UTC text, in the one shape the old code
/// stamped (`chrono::Utc::now().to_rfc3339()`: second-or-finer fraction digits
/// padded to 3/6/9, `+00:00` suffix).
pub fn now_rfc3339_utc() -> String {
    let now = jiff::Timestamp::now();
    render_utc(now.as_second(), now.subsec_nanosecond() as u32)
}

/// Parse an RFC 3339 instant with an offset (the old
/// `DateTime::parse_from_rfc3339`) into whole milliseconds since the Unix
/// epoch, floored exactly like chrono's `timestamp_millis`. `None` on any
/// input chrono rejected.
pub fn parse_rfc3339_millis(s: &str) -> Option<i64> {
    Some(parse_rfc3339_nanos(s)?.div_euclid(1_000_000) as i64)
}

/// Parse an RFC 3339 instant into total nanoseconds since the Unix epoch
/// (leap seconds encoded past the minute, as chrono did). Needed where the
/// old code subtracted full-precision instants (`freshness_line`).
pub fn parse_rfc3339_nanos(s: &str) -> Option<i128> {
    let b = s.as_bytes();
    // Strict fixed-width shape: YYYY-MM-DD[Tt ]HH:MM:SS[.frac](Z|z|±HH:MM).
    // chrono rejected 1-digit fields, `+YYYY` years, space after `T`,
    // comma fractions, and any trailing bytes here (its date parser is
    // lenient, but the RFC 3339 entry is not).
    if b.len() < 20 {
        return None;
    }
    let digits = |lo: usize, hi: usize| -> Option<i64> {
        if lo >= hi || hi > b.len() || !b[lo..hi].iter().all(u8::is_ascii_digit) {
            return None;
        }
        s[lo..hi].parse().ok()
    };
    let year: i32 = digits(0, 4)?.try_into().ok()?;
    if b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let month = digits(5, 7)?;
    let day = digits(8, 10)?;
    if !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let hour = digits(11, 13)?;
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let minute = digits(14, 16)?;
    let second = digits(17, 19)?;
    let mut pos = 19;
    // Fraction: at least one digit when the dot is there; chrono kept the
    // first 9 digits and ignored the rest.
    let mut frac_nanos: i64 = 0;
    if pos < b.len() && b[pos] == b'.' {
        pos += 1;
        let start = pos;
        while pos < b.len() && b[pos].is_ascii_digit() {
            pos += 1;
        }
        let len = pos - start;
        if len == 0 {
            return None;
        }
        let mut scale: i64 = 100_000_000;
        for i in 0..len.min(9) {
            frac_nanos += (b[start + i] - b'0') as i64 * scale;
            scale /= 10;
        }
    }
    if !(0..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month as u32) as i64
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    // Offset.
    let offset_secs: i64 = if pos < b.len() && (b[pos] == b'Z' || b[pos] == b'z') {
        pos += 1;
        0
    } else {
        if pos + 6 > b.len() {
            return None;
        }
        let sign = match b[pos] {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let oh = digits(pos + 1, pos + 3)?;
        if b[pos + 3] != b':' {
            return None;
        }
        let om = digits(pos + 4, pos + 6)?;
        if oh > 23 || om > 59 {
            return None;
        }
        pos += 6;
        sign * (oh * 3600 + om * 60)
    };
    if pos != b.len() {
        return None;
    }
    let days = days_from_civil(year, month as u32, day as u32);
    let mut epoch = days * 86_400 + hour * 3600 + minute * 60 + second - offset_secs;
    if !in_calendar(epoch) && !(second == 60 && in_calendar(epoch - 1)) {
        // Every 4-digit-year instant sits inside the calendar; the check only
        // guards the arithmetic, and a leap second renders off its `:59`.
        return None;
    }
    if second == 60 {
        // chrono carried leap seconds as a billionth-plus subsecond on `:59`.
        epoch -= 1;
        return Some(epoch as i128 * 1_000_000_000 + 1_000_000_000 + frac_nanos as i128);
    }
    Some(epoch as i128 * 1_000_000_000 + frac_nanos as i128)
}

/// The RFC 3339 UTC text of an epoch second, in the one format the old code
/// stamped (`DateTime::from_timestamp(...).to_rfc3339()`). Empty outside the
/// calendar, exactly like chrono's `None.unwrap_or_default()`.
pub fn epoch_to_rfc3339_utc(epoch: i64) -> String {
    if !in_calendar(epoch) {
        return String::new();
    }
    render_utc(epoch, 0)
}

/// The RFC 3339 UTC text of epoch milliseconds (`from_timestamp_millis` +
/// `to_rfc3339`). Empty outside the calendar.
pub fn epoch_ms_to_rfc3339(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    if !in_calendar(secs) {
        return String::new();
    }
    render_utc(secs, (ms.rem_euclid(1000) * 1_000_000) as u32)
}

/// Render a UTC instant: `%Y-%m-%dT%H:%M:%S` plus chrono's `AutoSi` fraction
/// (none when zero, else padded to 3/6/9 digits) and the `+00:00` suffix.
fn render_utc(epoch: i64, nanos: u32) -> String {
    let (y, m, d) = civil_from_days(epoch.div_euclid(86_400));
    let secs = epoch.rem_euclid(86_400);
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}{}{}",
        format_year(y),
        m,
        d,
        secs / 3600,
        secs % 3600 / 60,
        secs % 60,
        fraction_suffix(nanos),
        "+00:00"
    )
}

/// chrono's `SecondsFormat::AutoSi` fraction: empty, or `.mmm` / `.mmmmmm` /
/// `.mmmmmmmmm` by magnitude (trailing zeros kept to the 3/6/9 width).
fn fraction_suffix(nanos: u32) -> String {
    if nanos == 0 {
        String::new()
    } else if nanos.is_multiple_of(1_000_000) {
        format!(".{:03}", nanos / 1_000_000)
    } else if nanos.is_multiple_of(1_000) {
        format!(".{:06}", nanos / 1_000)
    } else {
        format!(".{:09}", nanos)
    }
}

/// The system timezone's offset in effect at `secs`, via jiff (replaces
/// `chrono::Local` resolution).
pub fn system_offset_at(secs: i64) -> i32 {
    jiff::Timestamp::from_second(secs)
        .ok()
        .map(|ts| {
            ts.to_zoned_datetime(jiff::tz::TimeZone::system())
                .offset()
                .seconds()
        })
        .unwrap_or(0)
}

/// Split a `SystemTime` into (epoch seconds, subsecond nanos), including
/// pre-epoch times.
fn split_system_time(st: SystemTime) -> (i64, u32) {
    match st.duration_since(UNIX_EPOCH) {
        Ok(d) => (d.as_secs() as i64, d.subsec_nanos()),
        Err(e) => {
            let d = e.duration();
            let secs = d.as_secs() as i64;
            let nanos = d.subsec_nanos();
            if nanos == 0 {
                (-secs, 0)
            } else {
                (-secs - 1, 1_000_000_000 - nanos)
            }
        }
    }
}

/// A file-mtime display in the system zone (`%Y-%m-%d %H:%M:%S%:z`, the old
/// `DateTime::<Local>::from(modified).format(...)`).
pub fn format_system_time_local(st: SystemTime) -> String {
    let (secs, _) = split_system_time(st);
    let offset = system_offset_at(secs);
    let local = secs + offset as i64;
    let (y, m, d) = civil_from_days(local.div_euclid(86_400));
    let day_secs = local.rem_euclid(86_400);
    format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02}{}",
        format_year(y),
        m,
        d,
        day_secs / 3600,
        day_secs % 3600 / 60,
        day_secs % 60,
        format_offset_colon(offset)
    )
}

/// A file-mtime display as UTC RFC 3339 at second precision with `Z` (the old
/// `DateTime::<Utc>::from(modified).to_rfc3339_opts(Secs, true)`).
pub fn format_system_time_utc_rfc3339(st: SystemTime) -> String {
    let (secs, _) = split_system_time(st);
    if !in_calendar(secs) {
        return String::new();
    }
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let day_secs = secs.rem_euclid(86_400);
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        format_year(y),
        m,
        d,
        day_secs / 3600,
        day_secs % 3600 / 60,
        day_secs % 60
    )
}

/// A fixed UTC offset is usable iff chrono's `FixedOffset::east_opt` accepted
/// it: just under a day either way.
pub fn is_valid_offset(secs: i32) -> bool {
    (-86_399..=86_399).contains(&secs)
}

/// chrono's `%Y`: plain zero-padded years inside 0..=9999, `+`-prefixed above,
/// `-`-prefixed and zero-padded to 4 below zero.
pub fn format_year(y: i32) -> String {
    if (0..=9999).contains(&y) {
        format!("{y:04}")
    } else if y > 9999 {
        format!("+{y}")
    } else {
        format!("-{:-0width$}", y.unsigned_abs(), width = 4)
    }
}

/// chrono's `%:z`: hours and minutes with the sign of the raw offset, seconds
/// rounded to the nearest minute (30 s rounds away from the minute).
pub fn format_offset_colon(secs: i32) -> String {
    let sign = if secs < 0 { '-' } else { '+' };
    let total_minutes = (secs.unsigned_abs() + 30) / 60;
    format!("{sign}{:02}:{:02}", total_minutes / 60, total_minutes % 60)
}

/// Parse a `YYYY-MM-DD` date with chrono's `%Y-%m-%d` leniency: leading
/// whitespace, an optional sign (which lifts the 4-digit year limit),
/// 1-2 digit month/day, and whitespace after each `-`. Anything else —
/// including trailing bytes — is an error, exactly as chrono classified it
/// (the caller maps every failure to the one `invalid --since/--until` line).
/// Skip Unicode whitespace (chrono skips it before the year and after each
/// `-`, including U+3000), returning the next byte position.
fn skip_ws(s: &str, pos: usize) -> usize {
    s[pos..]
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(i, _)| pos + i)
        .unwrap_or(s.len())
}

pub(crate) fn parse_ymd(s: &str) -> Option<(i32, u32, u32)> {
    let b = s.as_bytes();
    let mut pos = skip_ws(s, 0);
    let mut negative = false;
    let mut signed = false;
    if pos < b.len() && (b[pos] == b'+' || b[pos] == b'-') {
        negative = b[pos] == b'-';
        signed = true;
        pos += 1;
    }
    let start = pos;
    while pos < b.len() && b[pos].is_ascii_digit() {
        pos += 1;
    }
    let digits = pos - start;
    if digits == 0 || (!signed && digits > 4) {
        return None;
    }
    let year: i64 = s[start..pos].parse().ok()?;
    let year = if negative { -year } else { year };
    if !(i64::from(-262_143)..=i64::from(262_142)).contains(&year) {
        return None;
    }
    let mut fields = [0i64; 2];
    for field in fields.iter_mut() {
        if pos >= b.len() || b[pos] != b'-' {
            return None;
        }
        pos = skip_ws(s, pos + 1);
        let start = pos;
        while pos < b.len() && b[pos].is_ascii_digit() {
            pos += 1;
        }
        let digits = pos - start;
        if !(1..=2).contains(&digits) {
            return None;
        }
        *field = s[start..pos].parse().ok()?;
    }
    if pos != b.len() {
        return None;
    }
    let (month, day) = (fields[0], fields[1]);
    let year = year as i32;
    if !(1..=12).contains(&month) || day < 1 || day > i64::from(days_in_month(year, month as u32)) {
        return None;
    }
    Some((year, month as u32, day as u32))
}

/// Days in a month for a proleptic Gregorian year.
pub(crate) fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Proleptic Gregorian leap years, across the whole calendar. Mathematical
/// remainders (`rem_euclid`), as chrono's calendar does: a truncating `%`
/// calls -100 a leap year, the calendar does not.
pub(crate) fn is_leap(y: i32) -> bool {
    y.rem_euclid(4) == 0 && (y.rem_euclid(100) != 0 || y.rem_euclid(400) == 0)
}

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
pub(crate) fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Civil date of days since 1970-01-01 (Howard Hinnant's `civil_from_days`).
pub(crate) fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y } as i32, m as u32, d as u32)
}

/// Monday-based weekday (Mon = 0 .. Sun = 6) of a civil date.
pub(crate) fn weekday_mon0(y: i32, m: u32, d: u32) -> u32 {
    (days_from_civil(y, m, d) + 3).rem_euclid(7) as u32
}

/// 1-based ordinal day of the year.
pub(crate) fn ordinal_day(y: i32, m: u32, d: u32) -> u32 {
    (days_from_civil(y, m, d) - days_from_civil(y, 1, 1) + 1) as u32
}

/// ISO week date: (ISO year, week 01-53) behind chrono's `%G-W%V`.
pub(crate) fn iso_week(y: i32, m: u32, d: u32) -> (i32, u32) {
    let doy = ordinal_day(y, m, d) as i64;
    // Monday = 1 .. Sunday = 7: the `+10` anchor only works 1-based.
    let dow = weekday_mon0(y, m, d) as i64 + 1;
    let mut week = (doy - dow + 10) / 7;
    let mut iso_year = y;
    if week < 1 {
        iso_year -= 1;
        week = weeks_in_iso_year(iso_year) as i64;
    } else {
        let weeks = weeks_in_iso_year(iso_year) as i64;
        if week > weeks {
            iso_year += 1;
            week = 1;
        }
    }
    (iso_year, week as u32)
}

/// 52 or 53 ISO weeks in a year: 53 when Jan 1 is Thursday, or Wednesday in a
/// leap year.
pub(crate) fn weeks_in_iso_year(y: i32) -> u32 {
    let jan1 = weekday_mon0(y, 1, 1);
    if jan1 == 3 || (is_leap(y) && jan1 == 2) {
        53
    } else {
        52
    }
}

/// The UTC wall-clock year of an instant, for the RFC 3339 year-range check.
#[cfg(test)]
pub(crate) fn utc_year(epoch: i64) -> Option<i32> {
    if !in_calendar(epoch) {
        return None;
    }
    Some(civil_from_days(epoch.div_euclid(86_400)).0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Frozen goldens: probed chrono 0.4.45 outputs, byte for byte. ----

    #[test]
    fn offset_colon_renders_hours_minutes_with_rounding() {
        for (secs, want) in [
            (0, "+00:00"),
            (60, "+00:01"),
            (-60, "-00:01"),
            (5400, "+01:30"),
            (-5400, "-01:30"),
            (32400, "+09:00"),
            (-28800, "-08:00"),
            (86340, "+23:59"),
            (-86340, "-23:59"),
            (45, "+00:01"),
            (-45, "-00:01"),
            (5445, "+01:31"),
            (30, "+00:01"),
            (90, "+00:02"),
            (3599, "+01:00"),
            (-3599, "-01:00"),
            (1500, "+00:25"),
            (-1500, "-00:25"),
            (-30, "-00:01"),
            (-90, "-00:02"),
            (29, "+00:00"),
            (31, "+00:01"),
            (86399, "+24:00"),
            (-86399, "-24:00"),
            (1, "+00:00"),
            (-1, "-00:00"),
            (59, "+00:01"),
            (-59, "-00:01"),
            (89, "+00:01"),
            (-89, "-00:01"),
            (91, "+00:02"),
            (-91, "-00:02"),
        ] {
            assert_eq!(format_offset_colon(secs), want, "offset {secs}");
            assert!(is_valid_offset(secs), "offset {secs} must be usable");
        }
        for secs in [86400, -86400, 100000, -100000] {
            assert!(!is_valid_offset(secs), "offset {secs} must be refused");
        }
    }

    #[test]
    fn year_renders_like_chrono_percent_y() {
        for (year, want) in [
            (-262143, "-262143"),
            (-200000, "-200000"),
            (-9999, "-9999"),
            (-100, "-0100"),
            (-42, "-0042"),
            (-1, "-0001"),
            (0, "0000"),
            (1, "0001"),
            (42, "0042"),
            (2026, "2026"),
            (9999, "9999"),
            (10000, "+10000"),
            (100000, "+100000"),
            (262142, "+262142"),
        ] {
            assert_eq!(format_year(year), want, "year {year}");
        }
    }

    #[test]
    fn ymd_parses_with_chrono_leniency() {
        for (input, want) in [
            ("2026-10-01", (2026, 10, 1)),
            ("2026-1-1", (2026, 1, 1)),
            ("2026-01-1", (2026, 1, 1)),
            ("2026-1-01", (2026, 1, 1)),
            ("+2026-10-01", (2026, 10, 1)),
            (" 2026-10-01", (2026, 10, 1)),
            ("\t2026-10-01", (2026, 10, 1)),
            ("\n2026-10-01", (2026, 10, 1)),
            ("　2026-10-01", (2026, 10, 1)),
            ("2026- 10-01", (2026, 10, 1)),
            ("2026-10- 01", (2026, 10, 1)),
            ("0000-01-01", (0, 1, 1)),
            ("-000001-01-01", (-1, 1, 1)),
            ("-200000-01-01", (-200000, 1, 1)),
            ("+262142-12-31", (262142, 12, 31)),
            ("-262143-01-01", (-262143, 1, 1)),
            ("2024-02-29", (2024, 2, 29)),
            ("+1-01-01", (1, 1, 1)),
            ("1-01-01", (1, 1, 1)),
            ("12-01-01", (12, 1, 1)),
            ("123-01-01", (123, 1, 1)),
        ] {
            assert_eq!(parse_ymd(input), Some(want), "{input:?}");
        }
    }

    #[test]
    fn century_leap_years_follow_the_calendar_on_both_eras() {
        // Divisible by 100 but not 400 is common — including negative years,
        // where a truncating remainder would wrongly call -100 a leap year.
        for leap in ["2000-02-29", "2024-02-29", "-0004-02-29", "-0400-02-29"] {
            assert!(parse_ymd(leap).is_some(), "{leap:?}");
        }
        for common in [
            "1900-02-29",
            "2023-02-29",
            "-0100-02-29",
            "-0200-02-29",
            "-0300-02-29",
        ] {
            assert_eq!(parse_ymd(common), None, "{common:?}");
        }
    }

    #[test]
    fn ymd_rejects_what_chrono_rejected() {
        for input in [
            "2026/10/01",
            "2026.10.01",
            "02026-10-01",
            "2026-010-01",
            "10000-01-01",
            "12345-01-01",
            "99999-01-01",
            "-262144-12-31",
            "+262143-01-01",
            "2026-00-01",
            "2026-13-01",
            "2026-01-00",
            "2026-01-32",
            "2023-02-29",
            "2026-10-01T00:00:00",
            "",
            "2026",
            "2026-10",
            "--10-01",
            "2026--01",
            "abcd-ef-gh",
            "-",
            "+",
            "2026 -10-01",
            "2026-10 -01",
            "2026-10-01 ",
            "2026-10-01\n",
            "2026 - 10 - 01",
            "+ 2026-10-01",
            "- 200000-01-01",
        ] {
            assert_eq!(parse_ymd(input), None, "{input:?}");
        }
    }

    #[test]
    fn epoch_renders_match_chrono_utc() {
        assert_eq!(epoch_to_rfc3339_utc(-8_334_601_228_801), "");
        assert_eq!(
            epoch_to_rfc3339_utc(-8_334_601_228_800),
            "-262143-01-01T00:00:00+00:00"
        );
        assert_eq!(epoch_to_rfc3339_utc(-1), "1969-12-31T23:59:59+00:00");
        assert_eq!(epoch_to_rfc3339_utc(0), "1970-01-01T00:00:00+00:00");
        assert_eq!(epoch_to_rfc3339_utc(1), "1970-01-01T00:00:01+00:00");
        assert_eq!(
            epoch_to_rfc3339_utc(8_210_266_876_799),
            "+262142-12-31T23:59:59+00:00"
        );
        assert_eq!(epoch_to_rfc3339_utc(8_210_266_876_800), "");
    }

    #[test]
    fn epoch_millis_renders_match_chrono_utc() {
        assert_eq!(epoch_ms_to_rfc3339(-8_334_601_228_800_001), "");
        assert_eq!(
            epoch_ms_to_rfc3339(-8_334_601_228_800_000),
            "-262143-01-01T00:00:00+00:00"
        );
        assert_eq!(epoch_ms_to_rfc3339(0), "1970-01-01T00:00:00+00:00");
        assert_eq!(epoch_ms_to_rfc3339(-1), "1969-12-31T23:59:59.999+00:00");
        assert_eq!(
            epoch_ms_to_rfc3339(8_210_266_876_799_999),
            "+262142-12-31T23:59:59.999+00:00"
        );
        assert_eq!(epoch_ms_to_rfc3339(8_210_266_876_800_000), "");
    }

    #[test]
    fn rfc3339_parses_values_like_chrono() {
        let cases: &[(&str, i64, i128)] = &[
            ("2026-10-01T00:00:00+09:00", 1_790_780_400, 0),
            ("2026-10-01T00:00:00Z", 1_790_812_800, 0),
            (
                "2026-10-01T00:00:00.123456789+09:00",
                1_790_780_400,
                123_456_789,
            ),
            ("2026-10-01T00:00:00.123Z", 1_790_812_800, 123_000_000),
            (
                "2026-10-01T00:00:00.123456789999Z",
                1_790_812_800,
                123_456_789,
            ),
            ("2026-10-01T00:00:00.0000000001Z", 1_790_812_800, 0),
            ("0000-01-01T00:00:00Z", -62_167_219_200, 0),
            ("9999-12-31T23:59:59Z", 253_402_300_799, 0),
            ("2026-10-01 00:00:00Z", 1_790_812_800, 0),
            ("2026-10-01t00:00:00z", 1_790_812_800, 0),
            ("1969-12-31T23:59:59.999Z", -1, 999_000_000),
            ("2026-10-01T00:00:00-00:01", 1_790_812_860, 0),
            ("2026-10-01T00:00:00+23:59", 1_790_726_460, 0),
            ("2026-10-01T00:00:00.000000000Z", 1_790_812_800, 0),
            ("2026-10-01T00:00:00.000000001Z", 1_790_812_800, 1),
            ("2026-10-01T12:34:56.789+09:00", 1_790_825_696, 789_000_000),
        ];
        for &(input, secs, nanos) in cases {
            assert_eq!(
                parse_rfc3339_nanos(input),
                Some(secs as i128 * 1_000_000_000 + nanos),
                "{input:?}"
            );
        }
        // Leap seconds ride on `:59` with a billion-plus subsecond, as chrono.
        assert_eq!(
            parse_rfc3339_nanos("2026-10-01T23:59:60Z"),
            Some(1_790_899_199i128 * 1_000_000_000 + 1_000_000_000)
        );
        assert_eq!(
            parse_rfc3339_nanos("2026-10-01T00:00:60.5Z"),
            Some(1_790_812_859i128 * 1_000_000_000 + 1_500_000_000)
        );
    }

    #[test]
    fn rfc3339_millis_floor_like_chrono() {
        // chrono floored toward negative infinity; jiff's `as_millisecond`
        // truncates, so these two pre-1970 cases pin the difference.
        assert_eq!(parse_rfc3339_millis("1969-12-31T23:59:59.5Z"), Some(-500));
        assert_eq!(
            parse_rfc3339_millis("1969-12-31T23:59:59.4995Z"),
            Some(-501)
        );
        assert_eq!(parse_rfc3339_millis("1970-01-01T00:00:00.5Z"), Some(500));
        assert_eq!(
            parse_rfc3339_millis("2026-10-01T23:59:60Z"),
            Some(1_790_899_200_000)
        );
    }

    #[test]
    fn rfc3339_rejects_what_chrono_rejected() {
        for input in [
            "2026-10-01T00:00:00",
            "2026-10-01T00:00:00+0900",
            "2026-10-01T00:00:00+09",
            "2026-10-01T00:00:00Z ",
            " 2026-10-01T00:00:00Z",
            "2026-10-01T00:00:00+24:00",
            "2026-10-01T00:00:00+09:00:00",
            "2026-10-01T00:00:00+09:99",
            "2026-10-01T00:60:00Z",
            "2026-10-01T00:00:00+23:59:59",
            "2026-1-1T00:00:00Z",
            "999-01-01T00:00:00Z",
            "+2026-10-01T00:00:00Z",
            "2026-01-01T0:00:00Z",
            "2026-01-01T00:00:00.Z",
            "2026-01-01T00:00:00,5Z",
            "2026-01-01T00:00:00.5 Z",
            "2026-01-01T00:00:00+00:0Z",
            "2026-01-01T00:00:00+0:00Z",
            "2026-01-01T24:00:00Z",
            "2026-13-01T00:00:00Z",
            "2023-02-29T00:00:00Z",
            "+262142-12-31T00:00:00+09:00",
            "-262143-01-01T00:00:00+09:00",
            "2026-01-01T 00:00:00Z",
            "2026-01-01T00:00:0Z",
        ] {
            assert_eq!(parse_rfc3339_nanos(input), None, "{input:?}");
        }
    }

    #[test]
    fn iso_weeks_match_chrono_across_eras() {
        for (y, m, d, want_iy, want_w) in [
            (1970, 1, 1, 1970, 1),
            (2026, 10, 1, 2026, 40),
            (1, 1, 1, 1, 1),
            (0, 12, 31, 0, 52),
            (-1, 1, 1, -2, 53),
            (262142, 12, 31, 262143, 1),
            (-262143, 1, 1, -262143, 1),
            (10000, 6, 15, 10000, 24),
        ] {
            let (iy, w) = iso_week(y, m, d);
            assert_eq!((iy, w), (want_iy, want_w), "{y}-{m}-{d}");
            // The rendered `%G-W%V` spells the ISO year with `%Y` rules.
            let rendered = format!("{}-W{:02}", format_year(iy), w);
            let want_rendered = format!("{}-W{:02}", format_year(want_iy), want_w);
            assert_eq!(rendered, want_rendered, "{y}-{m}-{d}");
        }
    }

    #[test]
    fn the_calendar_constants_still_match_the_old_chrono_range() {
        assert_eq!(CALENDAR_MIN_EPOCH, -8_334_601_228_800);
        assert_eq!(CALENDAR_MAX_EPOCH, 8_210_266_876_799);
    }

    #[test]
    fn civil_roundtrips_across_the_calendar() {
        for days in (-3_600_000..=3_600_000).step_by(997) {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "roundtrip {days}");
        }
        for days in [
            CALENDAR_MIN_EPOCH.div_euclid(86_400),
            CALENDAR_MAX_EPOCH.div_euclid(86_400),
        ] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "roundtrip {days}");
        }
    }
}
