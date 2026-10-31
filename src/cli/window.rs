//! The evaluation-window plumbing every report view shares: the period flags
//! resolved ahead of the store, and the window's disclosure on stderr, in the
//! JSON payloads, and in a table's framing block.

use anyhow::Result;

use crate::core::bucket::JST_OFFSET_SECS;
use crate::core::period::{self, TimeRange, WindowPair};
use crate::session::Style;

/// Resolve the period flags before the session is built — the same
/// ahead-of-store evaluation `parse_scope`/`parse_format` have, so an
/// unparseable date fails on its own. The clock is read here: this is the
/// thin shell around the pure transform in `core::period`.
pub(crate) fn parse_period(
    since: Option<&str>,
    until: Option<&str>,
    last: Option<&str>,
) -> Result<TimeRange> {
    let now = chrono::Utc::now().timestamp();
    period::resolve(since, until, last, now, JST_OFFSET_SECS).map_err(anyhow::Error::msg)
}

/// Resolve the trend's period flags into the two comparison windows before
/// the session is built — the same ahead-of-store evaluation `parse_period`
/// gives the other views, so an unparseable date, a `--until` without a
/// `--since`, or an inverted window fails as a period error before any store
/// work or freshness line. The clock is read here: the thin shell around the
/// pure transform in `core::period`.
pub(crate) fn parse_pair(
    since: Option<&str>,
    until: Option<&str>,
    last: Option<&str>,
) -> Result<WindowPair> {
    let now = chrono::Utc::now().timestamp();
    period::resolve_pair(since, until, last, now, JST_OFFSET_SECS).map_err(anyhow::Error::msg)
}

/// The evaluation-window disclosure — stderr (every format) and the framing
/// block's last line (table only) render it. `None` without the period flags,
/// which is what keeps every existing output byte-identical.
pub(crate) fn window_line(window: TimeRange) -> Option<String> {
    (!window.is_all()).then(|| {
        format!(
            "report window: {}",
            period::display(window, JST_OFFSET_SECS)
        )
    })
}

/// The JSON `window` object — added only when the flags drew a window, so the
/// JSON contract without the flags is untouched.
pub(crate) fn window_json(window: TimeRange) -> Option<serde_json::Value> {
    (!window.is_all()).then(|| {
        let (since, until) = period::bounds(window, JST_OFFSET_SECS);
        serde_json::json!({ "since": since, "until": until })
    })
}

/// Attach the evaluation window to a JSON payload — only when the flags drew
/// one, so the JSON contract without the flags is untouched.
pub(crate) fn with_window(mut payload: serde_json::Value, window: TimeRange) -> serde_json::Value {
    if let Some(w) = window_json(window) {
        payload["window"] = w;
    }
    payload
}

/// Print the evaluation window beside the freshness line (stderr, every
/// format) — piped stdout and `--format json` stay clean.
pub(crate) fn eprint_window(window: TimeRange) {
    if let Some(line) = window_line(window) {
        eprintln!("{}", Style::stderr().dim(&line));
    }
}

/// A framing block with the evaluation window appended (table only — markdown
/// is the bare table, json the machine contract).
pub(crate) fn framed(text: &str, window: TimeRange) -> String {
    match window_line(window) {
        Some(line) => format!("{text}\n{line}"),
        None => text.to_string(),
    }
}

/// An empty report's wording when the flags drew a window: the action part
/// ("run analyze") would misread — the store may hold history outside the
/// window, and the observation stands within it. The window itself is on
/// stderr beside the freshness line.
pub(crate) fn empty_in_window(message: &str, window: TimeRange) -> String {
    match window.is_all() {
        true => message.to_string(),
        false => {
            let head = message.split(" — ").next().unwrap_or(message);
            format!("{head} in the evaluation window")
        }
    }
}

/// The catalog row's unused label: a windowed read means "unused *within the
/// window*" — the catalog is current config, the usage half is historical.
pub(crate) fn unused_label(window: TimeRange) -> &'static str {
    match window.is_all() {
        true => "UNUSED",
        false => "UNUSED (window)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use clap::Parser;

    use crate::cli::{Cli, Command};

    #[test]
    fn the_window_helpers_say_nothing_without_the_flags() {
        let all = TimeRange::ALL;
        assert_eq!(window_line(all), None);
        assert_eq!(window_json(all), None);
        assert!(framed("text", all) == "text");
        assert_eq!(
            empty_in_window("no skill usage found — run `saniwalens analyze` first", all),
            "no skill usage found — run `saniwalens analyze` first"
        );
        assert_eq!(unused_label(all), "UNUSED");
    }

    #[test]
    fn the_window_helpers_disclose_a_drawn_window() {
        let window = TimeRange {
            start: Some(1_790_780_400),
            end: Some(1_791_385_199),
        };
        assert_eq!(
            window_line(window).as_deref(),
            Some("report window: since 2026-10-01T00:00:00+09:00, until 2026-10-07T23:59:59+09:00")
        );
        assert_eq!(
            window_json(window),
            Some(serde_json::json!({
                "since": "2026-10-01T00:00:00+09:00",
                "until": "2026-10-07T23:59:59+09:00",
            }))
        );
        // The framing block appends the line to the text; the JSON payload
        // gains the `window` key without touching the rest.
        assert!(framed("text", window).contains("text\nreport window: since"));
        let payload = with_window(serde_json::json!({ "total": 3 }), window);
        assert_eq!(payload["window"]["since"], "2026-10-01T00:00:00+09:00");
        assert_eq!(payload["total"], 3);
    }

    #[test]
    fn an_empty_report_reads_within_the_window() {
        let window = TimeRange {
            start: Some(1),
            end: Some(2),
        };
        assert_eq!(
            empty_in_window(
                "no skill usage found — run `saniwalens analyze` first",
                window
            ),
            "no skill usage found in the evaluation window"
        );
        assert_eq!(
            empty_in_window("no data — run `saniwalens analyze` first", window),
            "no data in the evaluation window"
        );
        assert_eq!(unused_label(window), "UNUSED (window)");
    }

    #[test]
    fn trend_resolves_a_rolling_pair_through_the_flag_surface() {
        // No flags: 7d against the 7d right before it, the two tiling the
        // timeline with no shared second.
        let cli = Cli::try_parse_from(["saniwalens", "trend"]).unwrap();
        let Command::Trend {
            since, until, last, ..
        } = cli.command
        else {
            panic!("expected the trend command");
        };
        let pair = parse_pair(since.as_deref(), until.as_deref(), last.as_deref()).unwrap();
        let day = 86_400;
        assert_eq!(
            pair.current.end.unwrap() - pair.current.start.unwrap() + 1,
            7 * day
        );
        assert_eq!(
            pair.previous.end.unwrap() - pair.previous.start.unwrap() + 1,
            7 * day
        );
        assert_eq!(pair.previous.end.unwrap() + 1, pair.current.start.unwrap());

        // --last 24h: the same shape at 24h (86_400 seconds).
        let cli = Cli::try_parse_from(["saniwalens", "trend", "--last", "24h"]).unwrap();
        let Command::Trend { last, .. } = cli.command else {
            panic!("expected the trend command");
        };
        let pair = parse_pair(None, None, last.as_deref()).unwrap();
        assert_eq!(
            pair.current.end.unwrap() - pair.current.start.unwrap() + 1,
            24 * 3_600
        );
        assert_eq!(pair.previous.end.unwrap() + 1, pair.current.start.unwrap());
    }

    #[test]
    fn trend_period_errors_fail_before_the_store_opens() {
        // The dispatch resolves the pair before Session::open_read, so these
        // fail as period errors — non-zero, before any store work or
        // freshness line. The clock is read inside parse_pair; none of the
        // errors depend on it.
        let cases: [(&[&str], &str); 4] = [
            (
                &["saniwalens", "trend", "--since", "2026/10/01"],
                "invalid --since '2026/10/01'",
            ),
            (
                &["saniwalens", "trend", "--until", "2026-10-01"],
                "--until requires --since",
            ),
            (
                &[
                    "saniwalens",
                    "trend",
                    "--since",
                    "2026-10-07",
                    "--until",
                    "2026-10-01",
                ],
                "empty window: --since 2026-10-07 is after --until 2026-10-01",
            ),
            // The span parses (it fits i64) but its doubled window does not —
            // the pair resolution's overflow guard must reject it as the
            // invalid-span error, not panic.
            (
                &["saniwalens", "trend", "--last", "92233720368547d"],
                "invalid --last '92233720368547d'",
            ),
        ];
        for (args, expected) in cases {
            let cli = Cli::try_parse_from(args).unwrap();
            let Command::Trend {
                since, until, last, ..
            } = cli.command
            else {
                panic!("expected the trend command for {args:?}");
            };
            let err = parse_pair(since.as_deref(), until.as_deref(), last.as_deref())
                .expect_err("a bad period must fail");
            assert!(
                err.to_string().contains(expected),
                "expected {expected:?} in {err:?}"
            );
        }
    }
}
