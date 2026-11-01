//! Evaluation windows for the report views: `--since` / `--until` / `--last`
//! resolved into UTC epoch bounds. A pure transform — the clock and the
//! timezone are injected so tests stay deterministic (`.claude/rules/tdd.md`),
//! dates are interpreted in the presentation zone (JST by convention, like
//! `bucket.rs`) while the bounds they produce are UTC epochs
//! (`docs/specs/cli.md`).

use chrono::{FixedOffset, NaiveDate, NaiveDateTime, TimeZone};

/// A resolved evaluation window in UTC epoch seconds. Both ends are
/// inclusive; `None` is unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TimeRange {
    pub start: Option<i64>,
    pub end: Option<i64>,
}

impl TimeRange {
    /// Every event — the flags-absent default, which must not change a
    /// query's results or a view's output.
    pub const ALL: Self = Self {
        start: None,
        end: None,
    };

    /// Whether the flags drew no window at all.
    pub fn is_all(self) -> bool {
        self.start.is_none() && self.end.is_none()
    }
}

/// Resolve the period flags into a UTC epoch window.
///
/// Dates (`--since` / `--until`, `YYYY-MM-DD`) are interpreted in
/// `tz_offset_secs` — the presentation zone, JST by convention — and `--until`
/// counts the whole day it names, so it resolves to that day's last second.
/// `--last <N>d | <N>h | <N>w` is a rolling window ending at the injected
/// `now`. `--since` after `--until` is an error rather than an empty report,
/// which would read as "nothing was used".
pub fn resolve(
    since: Option<&str>,
    until: Option<&str>,
    last: Option<&str>,
    now: i64,
    tz_offset_secs: i32,
) -> Result<TimeRange, String> {
    if last.is_some() && (since.is_some() || until.is_some()) {
        return Err(
            "--last is a rolling window on its own and cannot be combined with --since or --until"
                .to_string(),
        );
    }
    if let Some(value) = last {
        let secs = span_secs(value)?;
        // A span whose window leaves the calendar has no window to name: its
        // ends are dates that do not exist, so the disclosure line would
        // render an empty `since` and the store would compare `started_at`
        // against an empty string. The same invalid-span error as every other
        // bad `--last` — an unresolvable input is an error, never a report
        // that reads as "nothing was used" (`docs/specs/cli.md`).
        let start = now
            .checked_sub(secs)
            .filter(|start| in_calendar(*start) && in_calendar(now))
            .ok_or_else(|| invalid_span_error(value))?;
        return Ok(TimeRange {
            start: Some(start),
            end: Some(now),
        });
    }
    let start = since
        .map(|value| day_start_epoch(value, "--since", tz_offset_secs))
        .transpose()?;
    let end = until
        .map(|value| day_end_epoch(value, "--until", tz_offset_secs))
        .transpose()?;
    // The named-day window's instants must name dates the calendar can
    // express — the same invariant the rolling path checks above, established
    // here for the date path instead of left to the zone conversion. The day
    // end adds its +86,399 after the conversion, so a west offset (the
    // declared `--tz` zone, `docs/specs/cli.md`) can resolve NaiveDate::MAX's
    // last second past the calendar's ceiling, where the bounds would render
    // an empty string. The input is the date, so the error is the
    // empty-window family naming the flag — never the invalid-`--last`
    // wording a date did not earn.
    for (flag, value, epoch) in [("--since", since, start), ("--until", until, end)] {
        if let (Some(value), Some(epoch)) = (value, epoch)
            && !in_calendar(epoch)
        {
            return Err(format!(
                "empty window: {flag} {value} — the day it names resolves outside the calendar chrono can express"
            ));
        }
    }
    if let (Some(start), Some(end)) = (start, end)
        && start > end
    {
        let since = since.unwrap_or_default();
        let until = until.unwrap_or_default();
        return Err(format!(
            "empty window: --since {since} is after --until {until}"
        ));
    }
    Ok(TimeRange { start, end })
}

/// The two comparison windows of the trend view: the current window and the
/// equal-length window immediately before it. The pair tiles the timeline —
/// the previous window ends one second before the current one starts — so no
/// event is counted in both windows of a comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPair {
    pub current: TimeRange,
    pub previous: TimeRange,
}

/// Resolve the trend flags into the current window and the same-length window
/// immediately before it.
///
/// With no flags at all the current window is the last 7 days and the previous
/// one the 7 days before them. `--last <N>d | <N>h | <N>w` is a rolling pair
/// ending at the injected `now`: current `(now-N, now]`, previous
/// `(now-2N, now-N]`. `--since X` (+ optional `--until Y`, inclusive per
/// [`resolve`]) fixes the current window at `[X, Y or now]` — a `--since`
/// without `--until` runs to the injected now — and derives the previous one
/// as the equal-length window ending one second before `X`. The date and span
/// errors of `resolve` carry over; `--until` without `--since` is an error
/// because the current window's start is undefined, and a `--since` after the
/// injected now leaves no current window to compare. A pair whose windows
/// leave the calendar chrono can express is refused: a `--last` whose derived
/// previous window does fails as the invalid-span error, and a date-derived
/// current window whose equal-length previous does as the empty-window error
/// naming `--since`.
pub fn resolve_pair(
    since: Option<&str>,
    until: Option<&str>,
    last: Option<&str>,
    now: i64,
    tz_offset_secs: i32,
) -> Result<WindowPair, String> {
    // `resolve` alone accepts `--until` without `--since`; the trend cannot —
    // the comparison needs the current window's start to derive the previous
    // one from.
    if since.is_none() && until.is_some() {
        return Err(
            "--until requires --since — without a start the current window (and the window to compare it with) is undefined"
                .to_string(),
        );
    }
    // `--last` — and the flags-absent default of 7d — is a rolling pair on its
    // own; `resolve` carries the span parse errors and the
    // cannot-be-combined-with-dates rule.
    if let Some(span) = last.or_else(|| (since.is_none() && until.is_none()).then_some("7d")) {
        let resolved = resolve(since, until, Some(span), now, tz_offset_secs)?;
        let secs = span_secs(span)?;
        // The rolling current is (now-N, now] — resolve's inclusive start
        // moved one second in — so the previous window, the same length
        // ending one second earlier, shares no event with it.
        let start = resolved.start.expect("a rolling window has a start") + 1;
        // A span whose doubled window leaves i64 — or, before that, names a
        // date the calendar cannot express: the derived previous window
        // doubles the reach back, so a span whose current window still
        // resolves can start it below chrono's floor (e.g. `--last
        // 50000000d`), where the disclosure would render an empty `since`.
        // The same invalid-span error as every other bad `--last`, not an
        // overflow panic or an empty previous window.
        let previous_start = start
            .checked_sub(secs)
            .filter(|previous| in_calendar(*previous))
            .ok_or_else(|| invalid_span_error(span))?;
        return Ok(WindowPair {
            current: TimeRange {
                start: Some(start),
                end: Some(now),
            },
            previous: TimeRange {
                start: Some(previous_start),
                end: Some(start - 1),
            },
        });
    }
    let mut current = resolve(since, until, None, now, tz_offset_secs)?;
    if current.end.is_none() {
        // `--since` without `--until`: the current window runs to the injected
        // now, so the previous window has a definite length.
        current.end = Some(now);
    }
    let start = current.start.expect("--since is present in this branch");
    let end = current.end.expect("the current end is clamped just above");
    if start > end {
        let since = since.unwrap_or_default();
        return Err(format!("empty window: --since {since} is after now"));
    }
    let length = end - start + 1;
    // `start` and `length` are date-derived — `--since`/`--until` parse as
    // NaiveDate, bounded to the calendar's range — so this subtraction cannot
    // leave i64 the way a huge `--last` span's doubled window can. The
    // equal-length window derived before the current one can still reach
    // past the calendar's floor when the current window itself spans most of
    // it (e.g. `--since=-200000-01-01`): its start would name a date that
    // does not exist, so the disclosure would render an empty `since`. The
    // input is the date, so the error names `--since` — the empty-window
    // family, never the invalid-`--last` wording a date did not earn. The
    // previous window's end needs no check of its own: it is the current
    // start minus one, inside the calendar whenever that start is.
    let previous_start = start - length;
    if !in_calendar(previous_start) {
        let since = since.unwrap_or_default();
        return Err(format!(
            "empty window: --since {since} — the equal-length window before it reaches outside the calendar chrono can express"
        ));
    }
    Ok(WindowPair {
        current,
        previous: TimeRange {
            start: Some(previous_start),
            end: Some(start - 1),
        },
    })
}

/// The whole day `--until` names, inclusive: the day's last local second.
fn day_end_epoch(value: &str, flag: &str, tz_offset_secs: i32) -> Result<i64, String> {
    Ok(day_start_epoch(value, flag, tz_offset_secs)? + 86_400 - 1)
}

/// Local midnight of the `YYYY-MM-DD` date, in `tz_offset_secs`.
fn day_start_epoch(value: &str, flag: &str, tz_offset_secs: i32) -> Result<i64, String> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
        format!("invalid {flag} '{value}' — expected a YYYY-MM-DD date (e.g. 2026-10-01)")
    })?;
    let midnight = date.and_hms_opt(0, 0, 0).expect("midnight always exists");
    let tz = FixedOffset::east_opt(tz_offset_secs).expect("valid tz offset");
    let local = tz
        .from_local_datetime(&midnight)
        .single()
        .ok_or_else(|| format!("invalid {flag} '{value}' — not a local time in this zone"))?;
    Ok(local.timestamp())
}

/// The seconds in a `<N>d | <N>h | <N>w` rolling window.
/// The invalid-`--last` error — shared by `span_secs` and the pair
/// resolution, so a span with no derivable previous window fails the same way
/// a span that cannot be parsed does.
fn invalid_span_error(value: &str) -> String {
    format!(
        "invalid --last '{value}' — expected <N>d | <N>h | <N>w with a positive N (e.g. 7d, 24h, 2w)"
    )
}

fn span_secs(value: &str) -> Result<i64, String> {
    let invalid = || invalid_span_error(value);
    let mut chars = value.chars();
    let unit = chars.next_back().ok_or_else(invalid)?;
    let n: i64 = chars.as_str().parse().map_err(|_| invalid())?;
    if n <= 0 {
        return Err(invalid());
    }
    let secs = match unit {
        'd' => 86_400,
        'h' => 3_600,
        'w' => 7 * 86_400,
        _ => return Err(invalid()),
    };
    n.checked_mul(secs).ok_or_else(invalid)
}

/// The epoch range chrono's calendar can express — `NaiveDateTime::MIN` /
/// `MAX`'s timestamps, the dates `NaiveDate` can name (roughly ±262,000
/// years). Const-evaluated from chrono's own constants, so a chrono upgrade
/// that moves the range moves it here too.
const CALENDAR_MIN_EPOCH: i64 = NaiveDateTime::MIN.and_utc().timestamp();
const CALENDAR_MAX_EPOCH: i64 = NaiveDateTime::MAX.and_utc().timestamp();

/// Whether an instant names a date the calendar can express. Outside the
/// range there is no date: the bounds would render an empty string and the
/// store would compare against an empty one.
fn in_calendar(epoch: i64) -> bool {
    (CALENDAR_MIN_EPOCH..=CALENDAR_MAX_EPOCH).contains(&epoch)
}

/// The window's resolved instants as `%Y-%m-%dT%H:%M:%S%:z` strings — RFC 3339
/// with the offset — in the given zone: `(since, until)`, the bounds that exist
/// only. At the calendar's own ends the rendered local date sits up to one zone
/// width outside the calendar while the instant stays exact (RFC 3339 names
/// years 0000..=9999 only), so the render is documented there rather than
/// refused.
pub fn bounds(range: TimeRange, tz_offset_secs: i32) -> (Option<String>, Option<String>) {
    let tz = FixedOffset::east_opt(tz_offset_secs).expect("valid tz offset");
    let instant = |epoch: i64| {
        // Every TimeRange is built by resolve / resolve_pair, which validate
        // both ends against the calendar, so an epoch they accept renders in
        // any valid offset: `timestamp_opt` yields None only outside the
        // calendar or for an ambiguous local time, and a fixed offset cannot
        // have one. There is no unrenderable instant to fall back on — the
        // empty-string branch is gone, so an out-of-calendar bound cannot be
        // silently absorbed here. The instant is in-calendar, but the local
        // date it renders at a calendar end can leave the calendar by up to one
        // zone width: that presentation spill is pinned by the sweep
        // (`mod tests`), never absorbed.
        tz.timestamp_opt(epoch, 0)
            .single()
            .expect("TimeRange holds an in-calendar epoch: resolve validates both ends")
            .format("%Y-%m-%dT%H:%M:%S%:z")
            .to_string()
    };
    (range.start.map(instant), range.end.map(instant))
}

/// The window as one human-readable line, instants rendered in
/// `tz_offset_secs` as RFC 3339 with their offset — e.g.
/// `since 2026-10-01T00:00:00+09:00, until 2026-10-07T23:59:59+09:00`.
pub fn display(range: TimeRange, tz_offset_secs: i32) -> String {
    let (since, until) = bounds(range, tz_offset_secs);
    let mut parts = Vec::new();
    if let Some(since) = since {
        parts.push(format!("since {since}"));
    }
    if let Some(until) = until {
        parts.push(format!("until {until}"));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::bucket::JST_OFFSET_SECS;
    use chrono::{DateTime, Datelike};

    const NOW: i64 = 1_000_000;
    /// The west offset (`-08:00`) the date path's ceiling check exists for: its
    /// sign is what can push a named day's last second past the calendar.
    const WEST_8H: i32 = -28_800;

    fn resolve(
        since: Option<&str>,
        until: Option<&str>,
        last: Option<&str>,
    ) -> Result<TimeRange, String> {
        super::resolve(since, until, last, NOW, JST_OFFSET_SECS)
    }

    // 2026-10-01T00:00:00Z — JST 2026-10-01 09:00.
    const OCT_1_UTC: i64 = 1_790_812_800;
    // 2026-10-08T00:00:00+09:00 == 2026-10-07T15:00:00Z.
    const OCT_8_JST_MIDNIGHT: i64 = OCT_1_UTC + 6 * 86_400 + 15 * 3_600;

    #[test]
    fn since_resolves_to_the_jst_day_start() {
        // 2026-10-01 00:00 JST is 2026-09-30T15:00:00Z.
        let range = resolve(Some("2026-10-01"), None, None).unwrap();
        assert_eq!(range.start, Some(OCT_1_UTC - 9 * 3600));
        assert_eq!(range.end, None);
    }

    #[test]
    fn until_counts_the_whole_day_it_names() {
        // Inclusive to the last second of 2026-10-07 JST.
        let range = resolve(None, Some("2026-10-07"), None).unwrap();
        assert_eq!(range.start, None);
        assert_eq!(range.end, Some(OCT_8_JST_MIDNIGHT - 1));
    }

    #[test]
    fn an_event_at_utc_21h_belongs_to_the_next_jst_day() {
        // 2026-10-07T21:00:00Z is JST 2026-10-08 06:00 — outside --until
        // 2026-10-07 even though the UTC date is the 7th.
        let range = resolve(None, Some("2026-10-07"), None).unwrap();
        let event = OCT_1_UTC + 6 * 86_400 + 21 * 3600;
        assert!(
            event > range.end.unwrap(),
            "the edge event must be excluded"
        );
    }

    #[test]
    fn since_and_until_bound_a_week() {
        let range = resolve(Some("2026-10-01"), Some("2026-10-07"), None).unwrap();
        assert_eq!(range.start, Some(OCT_1_UTC - 9 * 3600));
        assert_eq!(range.end, Some(OCT_8_JST_MIDNIGHT - 1));
    }

    #[test]
    fn last_rolls_a_window_back_from_the_injected_now() {
        assert_eq!(
            resolve(None, None, Some("7d")).unwrap(),
            TimeRange {
                start: Some(NOW - 7 * 86_400),
                end: Some(NOW)
            }
        );
        assert_eq!(
            resolve(None, None, Some("24h")).unwrap(),
            TimeRange {
                start: Some(NOW - 86_400),
                end: Some(NOW)
            }
        );
        assert_eq!(
            resolve(None, None, Some("2w")).unwrap(),
            TimeRange {
                start: Some(NOW - 14 * 86_400),
                end: Some(NOW)
            }
        );
    }

    #[test]
    fn the_month_end_resolves_without_rolling_over() {
        let range = resolve(Some("2026-10-31"), None, None).unwrap();
        assert_eq!(range.start, Some(OCT_1_UTC + 30 * 86_400 - 9 * 3600));
        let range = resolve(None, Some("2026-10-31"), None).unwrap();
        // 2026-11-01T00:00+09:00 minus one second.
        assert_eq!(range.end, Some(OCT_1_UTC + 31 * 86_400 - 9 * 3600 - 1));
    }

    #[test]
    fn no_flags_mean_every_event() {
        assert_eq!(resolve(None, None, None).unwrap(), TimeRange::ALL);
        assert!(TimeRange::ALL.is_all());
        assert!(!resolve(None, None, Some("1d")).unwrap().is_all());
    }

    #[test]
    fn a_date_spelled_with_slashes_is_an_error() {
        let err = resolve(Some("2026/10/01"), None, None).unwrap_err();
        assert!(err.contains("--since"), "{err}");
        assert!(err.contains("2026/10/01"), "{err}");
        let err = resolve(None, Some("2026.10.01"), None).unwrap_err();
        assert!(err.contains("--until"), "{err}");
    }

    #[test]
    fn a_span_without_a_unit_is_an_error() {
        assert!(resolve(None, None, Some("7")).is_err());
        assert!(resolve(None, None, Some("d")).is_err());
        assert!(resolve(None, None, Some("")).is_err());
        assert!(resolve(None, None, Some("1y")).is_err());
    }

    #[test]
    fn a_non_positive_span_is_an_error() {
        assert!(resolve(None, None, Some("0d")).is_err());
        assert!(resolve(None, None, Some("-3d")).is_err());
    }

    #[test]
    fn last_cannot_be_combined_with_the_dates() {
        let err = resolve(Some("2026-10-01"), None, Some("7d")).unwrap_err();
        assert!(err.contains("--last"), "{err}");
        let err = resolve(None, Some("2026-10-01"), Some("7d")).unwrap_err();
        assert!(err.contains("--last"), "{err}");
    }

    #[test]
    fn since_after_until_is_an_empty_window_error() {
        let err = resolve(Some("2026-10-07"), Some("2026-10-01"), None).unwrap_err();
        assert!(err.contains("2026-10-07"), "{err}");
        assert!(err.contains("2026-10-01"), "{err}");
    }

    #[test]
    fn display_renders_the_resolved_instants_with_their_offset() {
        let range = resolve(Some("2026-10-01"), Some("2026-10-07"), None).unwrap();
        let text = display(range, JST_OFFSET_SECS);
        assert!(
            text.contains("since 2026-10-01T00:00:00+09:00"),
            "the JST presentation of the day start: {text}"
        );
        assert!(text.contains("until 2026-10-07T23:59:59+09:00"), "{text}");
    }

    // ---- PBI 2026-10-30-02: the trend view's two windows ----

    // 2026-10-01T00:00:00+09:00 == 2026-09-30T15:00:00Z.
    const OCT_1_JST_START: i64 = OCT_1_UTC - 9 * 3_600;

    fn resolve_pair(
        since: Option<&str>,
        until: Option<&str>,
        last: Option<&str>,
    ) -> Result<WindowPair, String> {
        super::resolve_pair(since, until, last, NOW, JST_OFFSET_SECS)
    }

    #[test]
    fn trend_defaults_to_the_last_7_days_and_the_7_before_them() {
        // (now-7d, now] vs (now-14d, now-7d] — the seam second belongs to the
        // previous window only, so no event is compared against itself.
        assert_eq!(
            resolve_pair(None, None, None).unwrap(),
            WindowPair {
                current: TimeRange {
                    start: Some(NOW - 7 * 86_400 + 1),
                    end: Some(NOW)
                },
                previous: TimeRange {
                    start: Some(NOW - 14 * 86_400 + 1),
                    end: Some(NOW - 7 * 86_400)
                }
            }
        );
    }

    #[test]
    fn trend_last_rolls_both_windows_back_from_the_injected_now() {
        let pair = resolve_pair(None, None, Some("24h")).unwrap();
        assert_eq!(
            pair.current,
            TimeRange {
                start: Some(NOW - 86_400 + 1),
                end: Some(NOW)
            }
        );
        assert_eq!(
            pair.previous,
            TimeRange {
                start: Some(NOW - 2 * 86_400 + 1),
                end: Some(NOW - 86_400)
            }
        );
        // Adjacency: the previous window ends exactly where the current one
        // starts minus a second — same length, immediately before.
        assert_eq!(pair.previous.end.unwrap(), pair.current.start.unwrap() - 1);
        // 2w works too.
        assert_eq!(
            resolve_pair(None, None, Some("2w")).unwrap().previous,
            TimeRange {
                start: Some(NOW - 4 * 7 * 86_400 + 1),
                end: Some(NOW - 2 * 7 * 86_400)
            }
        );
    }

    #[test]
    fn trend_since_and_until_derive_the_equal_length_window_before_the_start() {
        let pair = resolve_pair(Some("2026-10-01"), Some("2026-10-07"), None).unwrap();
        assert_eq!(pair.current.start, Some(OCT_1_JST_START));
        assert_eq!(pair.current.end, Some(OCT_8_JST_MIDNIGHT - 1));
        // The 7 days before the current window: 2026-09-24 .. 2026-09-30 JST,
        // inclusive to the second — one less than the current window's start.
        assert_eq!(
            pair.previous,
            TimeRange {
                start: Some(OCT_1_JST_START - 7 * 86_400),
                end: Some(OCT_1_JST_START - 1),
            }
        );
    }

    #[test]
    fn trend_since_only_clamps_the_current_end_to_the_injected_now() {
        // The clock is injected: run it to 2026-10-03 23:59:59 JST.
        let now = OCT_1_JST_START + 3 * 86_400 - 1;
        let pair =
            super::resolve_pair(Some("2026-10-01"), None, None, now, JST_OFFSET_SECS).unwrap();
        assert_eq!(
            pair.current,
            TimeRange {
                start: Some(OCT_1_JST_START),
                end: Some(now)
            }
        );
        // The three days before the current window: 2026-09-28 .. 09-30 JST.
        assert_eq!(
            pair.previous,
            TimeRange {
                start: Some(OCT_1_JST_START - 3 * 86_400),
                end: Some(OCT_1_JST_START - 1),
            }
        );
    }

    #[test]
    fn trend_until_without_since_is_an_error() {
        let err = resolve_pair(None, Some("2026-10-01"), None).unwrap_err();
        assert!(err.contains("--until"), "{err}");
        assert!(err.contains("--since"), "{err}");
    }

    #[test]
    fn trend_since_after_until_is_an_error() {
        let err = resolve_pair(Some("2026-10-07"), Some("2026-10-01"), None).unwrap_err();
        assert!(err.contains("2026-10-07"), "{err}");
        assert!(err.contains("2026-10-01"), "{err}");
    }

    #[test]
    fn trend_since_after_the_injected_now_is_an_empty_window_error() {
        // NOW = 1_000_000 is 1970: a 2026 --since leaves no current window and
        // no previous one to derive from it.
        let err = resolve_pair(Some("2026-10-01"), None, None).unwrap_err();
        assert!(err.contains("2026-10-01"), "{err}");
    }

    #[test]
    fn trend_last_cannot_be_combined_with_the_dates() {
        assert!(resolve_pair(Some("2026-10-01"), None, Some("7d")).is_err());
        assert!(resolve_pair(None, Some("2026-10-01"), Some("7d")).is_err());
    }

    #[test]
    fn trend_carries_the_date_and_span_parse_errors() {
        assert!(resolve_pair(Some("2026/10/01"), None, None).is_err());
        assert!(resolve_pair(Some("2026-10-01"), Some("2026/10/07"), None).is_err());
        assert!(resolve_pair(None, None, Some("7")).is_err());
    }

    #[test]
    fn trend_last_beyond_the_i64_range_is_a_span_error() {
        // `92233720368547d` parses (the span itself fits i64) but its doubled
        // window does not: the previous window's start would leave i64. That
        // must fail as the invalid-span error — not panic on overflow in
        // debug, not silently wrap into an n/a previous window in release.
        let err = resolve_pair(None, None, Some("92233720368547d")).unwrap_err();
        assert!(err.contains("invalid --last"), "{err}");
        assert!(err.contains("92233720368547d"), "{err}");
    }

    // ---- PBI 2026-10-31-29: a `--last` whose window leaves the calendar ----

    #[test]
    fn a_span_beyond_the_calendar_is_an_error() {
        // The span parses (it fits i64) but its window names dates that do
        // not exist: resolve must refuse it as the invalid-span error, not
        // return a window the disclosure line renders as an empty since.
        let err = resolve(None, None, Some("92233720368547d")).unwrap_err();
        assert!(err.contains("invalid --last"), "{err}");
        assert!(err.contains("92233720368547d"), "{err}");
        // Fits i64, outside the calendar all the same: 100000000d reaches
        // roughly 273 thousand years back, past every date NaiveDate can name.
        let err = resolve(None, None, Some("100000000d")).unwrap_err();
        assert!(err.contains("invalid --last"), "{err}");
        assert!(err.contains("100000000d"), "{err}");
    }

    #[test]
    fn trend_rejects_a_span_whose_current_window_leaves_the_calendar() {
        // The doubled window still fits i64, so the pair's overflow guard
        // passes it — the calendar check at the shared resolve entry is what
        // refuses it, before any current window is built.
        let err = resolve_pair(None, None, Some("50000000000000d")).unwrap_err();
        assert!(err.contains("invalid --last"), "{err}");
        assert!(err.contains("50000000000000d"), "{err}");
    }

    // ---- PBI 2026-10-31-33: the pair's derived previous window ----

    #[test]
    fn trend_last_whose_derived_previous_leaves_the_calendar_is_a_span_error() {
        // The current window alone resolves (its start is inside the
        // calendar), but the pair doubles the reach: the derived previous
        // window's start would sit below the calendar's floor, where no date
        // exists to name. The same invalid-span error as every other bad
        // `--last` — not an Ok pair the disclosure renders with an empty
        // previous since.
        let err = resolve_pair(None, None, Some("50000000d")).unwrap_err();
        assert!(err.contains("invalid --last"), "{err}");
        assert!(err.contains("50000000d"), "{err}");
    }

    #[test]
    fn trend_since_whose_derived_previous_leaves_the_calendar_is_an_empty_window_error() {
        // A date-derived input: the current window resolves, but the
        // equal-length window derived before it reaches past the calendar's
        // floor. The error names `--since` — the empty-window family, never
        // the invalid-`--last` wording a date did not earn.
        let err = resolve_pair(Some("-200000-01-01"), None, None).unwrap_err();
        assert!(err.contains("empty window"), "{err}");
        assert!(err.contains("--since"), "{err}");
        assert!(err.contains("-200000-01-01"), "{err}");
        assert!(!err.contains("invalid --last"), "{err}");
    }

    #[test]
    fn the_calendar_edges_are_the_limits_of_a_rolling_window() {
        // chrono's calendar range, `NaiveDateTime::MIN`/`MAX`'s timestamps:
        // -8,334,601,228,800 (-262143-01-01) .. 8,210,266,876,799 (+262142-12-31).
        const CAL_MIN: i64 = -8_334_601_228_800;
        const CAL_MAX: i64 = 8_210_266_876_799;
        // The widest in-calendar window: its start sits exactly on the
        // calendar's floor.
        let now = CAL_MIN + 2 * 86_400;
        let range = super::resolve(None, None, Some("2d"), now, JST_OFFSET_SECS).unwrap();
        assert_eq!(range.start, Some(CAL_MIN));
        // One day wider and the start leaves the calendar.
        assert!(super::resolve(None, None, Some("3d"), now, JST_OFFSET_SECS).is_err());
        // The top edge: the rolling end is the clock itself, so a clock at
        // the calendar's ceiling still resolves — one second beyond it does
        // not, which is what makes the check a both-ends one.
        let range = super::resolve(None, None, Some("1d"), CAL_MAX, JST_OFFSET_SECS).unwrap();
        assert_eq!(range.end, Some(CAL_MAX));
        assert!(super::resolve(None, None, Some("1d"), CAL_MAX + 1, JST_OFFSET_SECS).is_err());
    }

    #[test]
    fn the_calendar_edges_are_the_limits_of_a_rolling_pair() {
        // The pair version of the single-window edges test: the derived
        // previous window doubles the reach back, so the floor boundary sits
        // at half the span a single window would allow — the pair's check is
        // the previous window's start, transitively with the current side.
        const CAL_MIN: i64 = -8_334_601_228_800;
        const CAL_MAX: i64 = 8_210_266_876_799;
        // Floor: with the clock two days in from it, a 1d pair keeps the
        // previous window's start exactly one second above the floor — the
        // widest pair that still names every date. A 2d pair, whose current
        // window alone still resolves, starts the previous window below the
        // floor.
        let now = CAL_MIN + 2 * 86_400;
        let pair = super::resolve_pair(None, None, Some("1d"), now, JST_OFFSET_SECS).unwrap();
        assert_eq!(pair.previous.start, Some(CAL_MIN + 1));
        assert_eq!(pair.previous.end, Some(CAL_MIN + 86_400));
        let err = super::resolve_pair(None, None, Some("2d"), now, JST_OFFSET_SECS).unwrap_err();
        assert!(err.contains("invalid --last '2d'"), "{err}");
        // The date-derived previous has the same floor: with `--since` two
        // hundred thousand years back and the clock injected so the
        // equal-length window before it ends exactly on the floor, the pair
        // resolves; one second more of current window and the previous
        // window's start would sit below the floor — refused with the error
        // that names `--since`.
        let start = super::resolve(Some("-200000-01-01"), None, None, 0, JST_OFFSET_SECS)
            .unwrap()
            .start
            .expect("a --since has a start");
        let now = 2 * start - CAL_MIN - 1;
        let pair =
            super::resolve_pair(Some("-200000-01-01"), None, None, now, JST_OFFSET_SECS).unwrap();
        assert_eq!(pair.previous.start, Some(CAL_MIN));
        let err = super::resolve_pair(Some("-200000-01-01"), None, None, now + 1, JST_OFFSET_SECS)
            .unwrap_err();
        assert!(err.contains("empty window: --since -200000-01-01"), "{err}");
        // Ceiling: the pair's ends sit below the clock, so a clock at the
        // calendar's ceiling still resolves; one second beyond it the
        // current side refuses the pair, as it refuses a single window.
        let pair = super::resolve_pair(None, None, Some("1d"), CAL_MAX, JST_OFFSET_SECS).unwrap();
        assert_eq!(pair.current.end, Some(CAL_MAX));
        assert_eq!(pair.previous.end, Some(CAL_MAX - 86_400));
        assert!(super::resolve_pair(None, None, Some("1d"), CAL_MAX + 1, JST_OFFSET_SECS).is_err());
    }

    #[test]
    fn the_calendar_constants_track_chronos_own_range() {
        // The constants are const-evaluated from chrono's own; pinning the
        // values turns a chrono upgrade that moves the range into a visible
        // change instead of a silent one.
        assert_eq!(CALENDAR_MIN_EPOCH, -8_334_601_228_800);
        assert_eq!(CALENDAR_MAX_EPOCH, 8_210_266_876_799);
    }

    #[test]
    fn the_calendar_edges_are_the_limits_of_the_date_path() {
        // The date version of the rolling edges tests: the named-day window's
        // resolved instants must name dates the calendar can express too. In
        // JST the floor is closed by the zone conversion — NaiveDate::MIN's
        // local midnight sits below the calendar's floor, so the conversion
        // refuses it before any window is built — and the ceiling stays one
        // zone-width below it. The date path's own hole is the day end: it
        // adds its +86,399 after the conversion, so a west offset — the
        // declared `--tz` zone (Time bucketing), unwired today — resolves
        // NaiveDate::MAX's last second past the calendar's ceiling, where the
        // bounds would render an empty until. The entry's calendar check
        // refuses it with the empty-window error naming the flag — the
        // date-derived error family, never the invalid-`--last` wording a
        // date did not earn.
        const CAL_MIN: i64 = -8_334_601_228_800;
        const CAL_MAX: i64 = 8_210_266_876_799;
        // Floor, JST: the local midnight of NaiveDate::MIN is below the
        // calendar's floor — refused as a period error, no window drawn.
        let err = resolve(Some("-262143-01-01"), None, None).unwrap_err();
        assert!(err.contains("--since"), "{err}");
        assert!(err.contains("not a local time"), "{err}");
        // One day in, the window resolves inside the calendar: the local
        // midnight is CAL_MIN + 15h.
        let range = resolve(Some("-262143-01-02"), None, None).unwrap();
        assert_eq!(range.start, Some(CAL_MIN + 15 * 3_600));
        // Ceiling, JST: the whole day NaiveDate::MAX names ends CAL_MAX - 9h,
        // inside the calendar.
        let range = resolve(None, Some("+262142-12-31"), None).unwrap();
        assert_eq!(range.end, Some(CAL_MAX - 9 * 3_600));
        // The day end under a west offset: the conversion keeps NaiveDate::MAX's
        // midnight inside the calendar, but the +86,399 after it leaves the
        // ceiling — refused with the empty-window error naming `--until`.
        let err = super::resolve(None, Some("+262142-12-31"), None, 0, WEST_8H).unwrap_err();
        assert!(err.contains("empty window: --until +262142-12-31"), "{err}");
        assert!(
            err.contains("outside the calendar chrono can express"),
            "{err}"
        );
        assert!(!err.contains("invalid --last"), "{err}");
        // The pair builds its current window through the same resolve entry,
        // so the west-offset ceiling refusal carries over to `trend` — with
        // the `--since` the trend requires naming the same error family.
        let err = super::resolve_pair(
            Some("+262142-12-30"),
            Some("+262142-12-31"),
            None,
            0,
            WEST_8H,
        )
        .unwrap_err();
        assert!(err.contains("empty window: --until +262142-12-31"), "{err}");
        // The west offset widens the band the other way: NaiveDate::MIN's
        // named day, which JST cannot resolve (its midnight sits below the
        // floor), resolves inside the calendar under the same west offset.
        let range = super::resolve(Some("-262143-01-01"), None, None, 0, WEST_8H).unwrap();
        assert_eq!(range.start, Some(CAL_MIN + 8 * 3_600));
        assert!(in_calendar(range.start.expect("the start resolves")));
    }

    /// The offset a bound ends with. `%:z` renders hours and minutes, never
    /// seconds, and rounds a seconds remainder to the nearest minute — which is
    /// why the `--tz` domain is minute-granular (`docs/specs/cli.md`, Time
    /// bucketing).
    fn offset_suffix(zone: i32) -> String {
        let sign = if zone < 0 { '-' } else { '+' };
        let abs = zone.abs();
        format!("{sign}{:02}:{:02}", abs / 3_600, abs % 3_600 / 60)
    }

    /// Whether RFC 3339 can name the local wall clock's year: `date-fullyear`
    /// is four digits, 0000..=9999. The calendar's own edge years (-262143 and
    /// +262142) are outside it, so those renders are checked through the naive
    /// wall clock instead.
    fn rfc3339_names_year(local: i64) -> bool {
        DateTime::from_timestamp(local, 0).is_some_and(|wall| (0..=9999).contains(&wall.year()))
    }

    /// The bytes a presentation spill must render. The wall clock is
    /// `epoch + zone`, and a spill is an in-calendar instant within one zone
    /// width of a calendar end, so its local date is exactly one day past
    /// `NaiveDate::MAX` (east offset) or one day before `NaiveDate::MIN` (west
    /// offset). Built from those edge dates and the wall-clock time of day
    /// rather than from the renderer, so a chrono format change shows up as a
    /// diff instead of moving both sides together.
    fn presentation_spill(epoch: i64, zone: i32) -> String {
        let local = epoch + i64::from(zone);
        let date = if zone > 0 {
            "+262143-01-01" // the day after NaiveDate::MAX
        } else {
            "-262144-12-31" // the day before NaiveDate::MIN
        };
        let secs = local.rem_euclid(86_400);
        format!(
            "{date}T{:02}:{:02}:{:02}{}",
            secs / 3_600,
            secs % 3_600 / 60,
            secs % 60,
            offset_suffix(zone)
        )
    }

    /// The oracle every drawn bound is checked against: it names the instant it
    /// was drawn for, not merely a non-empty string with the requested offset.
    /// An in-calendar render round-trips — through RFC 3339 when the local year
    /// is one RFC 3339 can name, through the naive wall clock at the calendar's
    /// edge years. A render whose wall clock leaves the calendar names a local
    /// date that does not exist: that is the presentation spill, pinned byte
    /// for byte.
    fn assert_bound_names_the_instant(name: &str, side: &str, epoch: i64, zone: i32, bound: &str) {
        let local = epoch + i64::from(zone);
        if !in_calendar(local) {
            assert_eq!(
                presentation_spill(epoch, zone),
                bound,
                "the presentation spill changed for {name} {side}"
            );
            return;
        }
        if rfc3339_names_year(local) {
            let parsed = DateTime::parse_from_rfc3339(bound)
                .unwrap_or_else(|e| panic!("{name} {side} is not RFC 3339: {bound} ({e})"));
            assert_eq!(
                parsed.timestamp(),
                epoch,
                "the bound names another instant for {name} {side}: {bound}"
            );
        } else {
            let suffix = offset_suffix(zone);
            let wall = bound
                .strip_suffix(&suffix)
                .unwrap_or_else(|| panic!("the offset is missing for {name} {side}: {bound}"));
            let naive = NaiveDateTime::parse_from_str(wall, "%Y-%m-%dT%H:%M:%S")
                .unwrap_or_else(|e| panic!("{name} {side} is not a wall clock: {bound} ({e})"));
            assert_eq!(
                naive.and_utc().timestamp(),
                local,
                "the bound names another wall clock for {name} {side}: {bound}"
            );
        }
    }

    #[test]
    fn the_presentation_spill_at_a_calendar_end_is_pinned_byte_for_byte() {
        // The calendar's floor and ceiling are instants the resolve entry
        // accepts, but a non-zero presentation zone pushes the rendered local
        // date one day outside the calendar, where `NaiveDate` has no date to
        // name. The instant stays exact and no real clock reaches these years,
        // so the render is a documented limit rather than a refused window: the
        // bytes are pinned so a chrono change in the extended-year format or in
        // the offset rendering shows up as a diff.
        const CAL_MIN: i64 = -8_334_601_228_800;
        const CAL_MAX: i64 = 8_210_266_876_799;
        let (_, ceiling) = bounds(
            TimeRange {
                start: None,
                end: Some(CAL_MAX),
            },
            JST_OFFSET_SECS,
        );
        assert_eq!(
            ceiling.as_deref(),
            Some("+262143-01-01T08:59:59+09:00"),
            "the JST spill past the calendar's ceiling changed"
        );
        let (floor, _) = bounds(
            TimeRange {
                start: Some(CAL_MIN),
                end: None,
            },
            WEST_8H,
        );
        assert_eq!(
            floor.as_deref(),
            Some("-262144-12-31T16:00:00-08:00"),
            "the west spill below the calendar's floor changed"
        );
    }

    #[test]
    fn every_accepted_window_renders_bounds_that_name_their_instant() {
        // The disclosure line and the JSON window must never carry an empty
        // since/until, and each bound must name the instant it was drawn for:
        // out-of-calendar input is refused at the resolve entry and at the
        // pair's derived previous, so anything accepted is in-calendar, and
        // the sweep checks what that means instead of only that the string is
        // non-empty with the requested offset. Clocks are injected at both
        // calendar ends — where the UTC bound meets the presentation zone, the
        // edge a render-only spill shows at — and the date path is swept over
        // the representative zones below. The renderer has no `Result`: a bound
        // is either the instant it was drawn for, or (within one zone width of
        // a calendar end) the pinned presentation spill.
        const CAL_MIN: i64 = -8_334_601_228_800;
        const CAL_MAX: i64 = 8_210_266_876_799;
        let clocks = [CAL_MIN, CAL_MIN + 2 * 86_400, NOW, CAL_MAX];
        // Ten representative zones: JST (the presentation zone every caller
        // passes today, which is the +32,400 point), the west offset whose sign
        // can push the day end past the ceiling, UTC, the smallest east/west
        // minute, a minute remainder on both sides, -09:00, and the domain's
        // ends. `%:z` rounds to the nearest minute, so a finer grid asserts
        // nothing this one cannot; the whole minute-granularity domain is swept
        // in its own test below.
        let zones = [
            JST_OFFSET_SECS, // +09:00 — the presentation zone every caller passes
            WEST_8H,         // -08:00 — the offset the day-end check exists for
            0,               // UTC
            60,              // +00:01 — the smallest east minute
            -60,             // -00:01 — the smallest west minute
            5_400,           // +01:30 — a minute remainder
            -5_400,          // -01:30
            -32_400,         // -09:00 — the day-length offset the other way
            86_340,          // +23:59 — the domain's east end
            -86_340,         // -23:59 — the domain's west end
        ];
        let spans = [
            "1h",
            "7d",
            "24h",
            "2w",
            "1000w",
            "50000000d",
            "100000000d",
            "92233720368547d",
        ];
        let dates = [
            "-262143-01-01", // NaiveDate::MIN — the calendar's floor
            "-262143-01-02", // one day in
            "2026-10-01",
            "2026-10-07",
            "+262142-12-31", // NaiveDate::MAX — the calendar's ceiling
        ];
        let check = |name: &str, zone: i32, range: TimeRange| {
            let (since, until) = bounds(range, zone);
            let suffix = offset_suffix(zone);
            // No None-skip: each side is asserted in both directions — a drawn
            // bound names its instant (never empty, offset carried), an undrawn
            // one renders nothing.
            for (side, epoch, bound) in [("since", range.start, since), ("until", range.end, until)]
            {
                match epoch {
                    Some(epoch) => {
                        let bound = bound.expect("a drawn bound renders");
                        assert!(!bound.is_empty(), "empty {side} for {name}");
                        assert!(
                            bound.ends_with(&suffix),
                            "the offset is missing for {name}: {bound}"
                        );
                        assert_bound_names_the_instant(name, side, epoch, zone, &bound);
                    }
                    None => assert!(
                        bound.is_none(),
                        "an undrawn {side} rendered for {name}: {bound:?}"
                    ),
                }
            }
        };
        for now in clocks {
            for span in spans {
                match super::resolve(None, None, Some(span), now, JST_OFFSET_SECS) {
                    Ok(range) => check(span, JST_OFFSET_SECS, range),
                    Err(err) => assert!(err.contains("invalid --last"), "{err}"),
                }
                match super::resolve_pair(None, None, Some(span), now, JST_OFFSET_SECS) {
                    Ok(pair) => {
                        check(&format!("{span} current"), JST_OFFSET_SECS, pair.current);
                        check(&format!("{span} previous"), JST_OFFSET_SECS, pair.previous);
                    }
                    Err(err) => assert!(err.contains("invalid --last"), "{err}"),
                }
            }
            for zone in zones {
                for date in dates {
                    // The named-day window the date path builds: one flag at a
                    // time, every representative zone.
                    match super::resolve(Some(date), None, None, now, zone) {
                        Ok(range) => check(&format!("--since {date}"), zone, range),
                        Err(err) => assert!(err.contains("--since"), "{err}"),
                    }
                    match super::resolve(None, Some(date), None, now, zone) {
                        Ok(range) => check(&format!("--until {date}"), zone, range),
                        Err(err) => assert!(err.contains("--until"), "{err}"),
                    }
                    // The pair's current window is built through the same
                    // resolve entry; its derived previous has its own guard.
                    // Every date-derived error names the date flag — the
                    // empty-window family and the zone conversion's
                    // invalid-date wording alike.
                    match super::resolve_pair(Some(date), None, None, now, zone) {
                        Ok(pair) => {
                            check(&format!("{date} current"), zone, pair.current);
                            check(&format!("{date} previous"), zone, pair.previous);
                        }
                        Err(err) => assert!(err.contains("--since"), "{err}"),
                    }
                }
            }
        }
    }

    #[test]
    fn every_minute_granular_offset_renders_the_calendar_ends() {
        // The `--tz` domain is minute-granular: -86,340..=86,340 seconds in
        // 60-second steps, 2,879 offsets (`docs/specs/cli.md`, Time
        // bucketing). `bounds` is called directly at the calendar's four ends
        // for every one of them: the renderer must not panic, the bound must be
        // non-empty, and it must name its instant — or, within one zone width
        // of an end, the pinned presentation spill. The seconds-granularity
        // domain (172,799 offsets) is deliberately not swept: `%:z` rounds to
        // the nearest minute, so it reaches no render this grid does not, at
        // ~48 s.
        const CAL_MIN: i64 = -8_334_601_228_800;
        const CAL_MAX: i64 = 8_210_266_876_799;
        let ends = [CAL_MIN, CAL_MIN + 32_400, CAL_MAX - 32_400, CAL_MAX];
        let mut bounds_checked = 0usize;
        for minutes in -1_439..=1_439 {
            let zone = minutes * 60;
            for epoch in ends {
                let range = TimeRange {
                    start: Some(epoch),
                    end: Some(epoch),
                };
                let (since, until) = bounds(range, zone);
                for (side, bound) in [("since", since), ("until", until)] {
                    let bound = bound.expect("a calendar-end instant renders");
                    assert!(
                        !bound.is_empty(),
                        "empty {side} at epoch {epoch}, zone {zone}"
                    );
                    assert_bound_names_the_instant("calendar end", side, epoch, zone, &bound);
                    bounds_checked += 1;
                }
            }
        }
        assert_eq!(bounds_checked, 2 * 4 * 2_879);
    }
}
