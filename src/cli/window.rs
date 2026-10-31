//! The windowed flag surface every report view shares, in one place: the
//! period flags declared once (clap args groups flattened into each command),
//! resolved ahead of the store through one session-open dispatch, and the
//! window's disclosure on stderr, in the JSON payloads, and in a table's
//! framing block.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::adapter::harness::Harness;
use crate::cli::{Format, parse_format};
use crate::core::bucket::JST_OFFSET_SECS;
use crate::core::period::{self, TimeRange, WindowPair};
use crate::core::scope::ScopeFilter;
use crate::session::{AnalyzeStats, Session, SessionEnv, Style};

/// The period flags the eight single-window views share, declared once here
/// and flattened into each command. `--last`'s conflicts are pinned on this
/// one declaration — flatten keeps each field's arg id (the field name), so
/// the ids resolve inside every command that flattens the group.
#[derive(clap::Args)]
pub(crate) struct PeriodArgs {
    /// Only count events on or after this date (YYYY-MM-DD, JST).
    #[arg(long, value_name = "DATE")]
    pub(crate) since: Option<String>,
    /// Only count events through this date (YYYY-MM-DD, JST) — the
    /// whole day counts.
    #[arg(long, value_name = "DATE")]
    pub(crate) until: Option<String>,
    /// Rolling window ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive
    /// with the explicit dates.
    #[arg(long, value_name = "SPAN", conflicts_with_all = ["since", "until"])]
    pub(crate) last: Option<String>,
}

/// trend's comparison pair: the three period flags with the pair's own
/// wording, which the shared single-window declaration cannot carry (the help
/// capture pins hold every view's help byte-identical).
#[derive(clap::Args)]
pub(crate) struct PairArgs {
    /// Current window start (YYYY-MM-DD, JST). The window to compare it
    /// with is the same length immediately before it.
    #[arg(long, value_name = "DATE")]
    pub(crate) since: Option<String>,
    /// Current window end (YYYY-MM-DD, JST) — the whole day counts.
    /// Needs --since; alone, the window to compare against is undefined.
    #[arg(long, value_name = "DATE")]
    pub(crate) until: Option<String>,
    /// Rolling pair ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive
    /// with the explicit dates.
    #[arg(long, value_name = "SPAN", conflicts_with_all = ["since", "until"])]
    pub(crate) last: Option<String>,
}

/// The read-path flags every report view shares: read the store as-is
/// (`--frozen`) or pick the store (`--db`).
#[derive(clap::Args)]
pub(crate) struct ReadArgs {
    /// Read the store as-is; skip the automatic refresh.
    #[arg(long)]
    pub(crate) frozen: bool,
    /// Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db).
    #[arg(long)]
    pub(crate) db: Option<PathBuf>,
}

/// What one windowed report arm's open call resolves: the opened session and
/// the flags, resolved in the arm's order.
#[derive(Debug)]
pub(crate) struct WindowedRead {
    pub(crate) sess: Session,
    pub(crate) filter: ScopeFilter,
    pub(crate) format: Format,
    pub(crate) window: TimeRange,
}

/// What the trend arm's open call resolves: the opened session, the format,
/// and the comparison pair.
#[derive(Debug)]
pub(crate) struct WindowedPairRead {
    pub(crate) sess: Session,
    pub(crate) format: Format,
    pub(crate) pair: WindowPair,
}

/// What every windowed report arm resolves through: the harness, the
/// configured store, and the invocation environment. Built once by `run()`,
/// so an arm is one open call and the ordering invariant lives in this
/// module instead of nine copies of a per-arm comment.
pub(crate) struct WindowedDispatch<'a> {
    pub(crate) harness: &'a Harness,
    pub(crate) config_db: Option<&'a Path>,
    pub(crate) env: &'a SessionEnv<'a>,
}

impl WindowedDispatch<'_> {
    /// The one windowed session open for the eight single-window views:
    /// scope (where the view has `--scope`), then format, then the period,
    /// then the store opens. The order is the arms' own, kept so a double
    /// failure reports the same error the current binary does; the invariant —
    /// every flag resolves before the store opens, so an invalid flag fails
    /// on its own with no store error or freshness line ahead of it
    /// (`docs/specs/cli.md`) — is pinned here once.
    pub(crate) fn open(
        &self,
        scope: Option<&str>,
        format: Option<&str>,
        period: &PeriodArgs,
        read: &ReadArgs,
        analyze: impl FnOnce(&Harness, Option<PathBuf>, &Path) -> Result<AnalyzeStats>,
    ) -> Result<WindowedRead> {
        let filter = super::parse_scope(scope)?;
        let format = parse_format(format)?;
        let window = parse_period(
            period.since.as_deref(),
            period.until.as_deref(),
            period.last.as_deref(),
        )?;
        let sess = Session::open_read(
            self.harness,
            read.db.clone(),
            self.config_db,
            read.frozen,
            self.env,
            analyze,
        )?;
        Ok(WindowedRead {
            sess,
            filter,
            format,
            window,
        })
    }

    /// The trend arm's one open call: format, then the pair, then the store
    /// opens — the same flags-before-store order, with the pair resolution
    /// where the single-window views' period sits.
    pub(crate) fn open_pair(
        &self,
        format: Option<&str>,
        pair: &PairArgs,
        read: &ReadArgs,
        analyze: impl FnOnce(&Harness, Option<PathBuf>, &Path) -> Result<AnalyzeStats>,
    ) -> Result<WindowedPairRead> {
        let format = parse_format(format)?;
        let pair = parse_pair(
            pair.since.as_deref(),
            pair.until.as_deref(),
            pair.last.as_deref(),
        )?;
        let sess = Session::open_read(
            self.harness,
            read.db.clone(),
            self.config_db,
            read.frozen,
            self.env,
            analyze,
        )?;
        Ok(WindowedPairRead { sess, format, pair })
    }
}

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
            pair: PairArgs { since, until, last },
            ..
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
        let Command::Trend {
            pair: PairArgs { last, .. },
            ..
        } = cli.command
        else {
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
                pair: PairArgs { since, until, last },
                ..
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

    // ---- PBI 2026-11-01-40: the consolidated windowed flag surface ----

    /// The eight single-window views that flatten the shared period args;
    /// trend's pair keeps its own declaration beside them.
    const SINGLE_WINDOW_VIEWS: [&str; 8] = [
        "usage",
        "inventory",
        "waste",
        "overhead",
        "prompts",
        "failures",
        "stuck",
        "doctor",
    ];

    /// The windowed and read-path flags one parsed command holds, extracted
    /// per view: (since, until, last, frozen, db). The match is the flatten
    /// demonstration — every view carries the shared groups as plain fields.
    fn windowed_flags(
        command: Command,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        bool,
        Option<std::path::PathBuf>,
    ) {
        match command {
            Command::Usage { period, read, .. } => (
                period.since,
                period.until,
                period.last,
                read.frozen,
                read.db,
            ),
            Command::Trend { .. } => {
                unreachable!("the trend pair keeps its own declaration")
            }
            Command::Inventory { period, read, .. } => (
                period.since,
                period.until,
                period.last,
                read.frozen,
                read.db,
            ),
            Command::Waste { period, read, .. } => (
                period.since,
                period.until,
                period.last,
                read.frozen,
                read.db,
            ),
            Command::Overhead { period, read, .. } => (
                period.since,
                period.until,
                period.last,
                read.frozen,
                read.db,
            ),
            Command::Prompts { period, read, .. } => (
                period.since,
                period.until,
                period.last,
                read.frozen,
                read.db,
            ),
            Command::Failures { period, read, .. } => (
                period.since,
                period.until,
                period.last,
                read.frozen,
                read.db,
            ),
            Command::Stuck { period, read, .. } => (
                period.since,
                period.until,
                period.last,
                read.frozen,
                read.db,
            ),
            Command::Doctor { period, read, .. } => (
                period.since,
                period.until,
                period.last,
                read.frozen,
                read.db,
            ),
            Command::Analyze { .. } | Command::Sql { .. } | Command::Optimize { .. } => {
                unreachable!("the whole-history commands take no period flags")
            }
        }
    }

    #[test]
    fn the_shared_period_args_conflict_and_parse_on_every_single_window_view() {
        // One declaration carries the conflicts: flatten keeps each field's
        // arg id (the field name), so `--last`'s `conflicts_with_all` —
        // pinned once on the shared group — resolves inside every command
        // that flattens it. This mechanizes the conflicts for all eight
        // single-window views (PBI 40: the six views the older conflict pin
        // left uncovered — inventory, overhead, prompts, failures, stuck,
        // doctor — are asserted here rather than left to the core's second
        // defense, whose wording and exit code differ from the clap
        // conflict's).
        for view in SINGLE_WINDOW_VIEWS {
            let err =
                Cli::try_parse_from(["saniwalens", view, "--last", "7d", "--since", "2026-10-01"])
                    .err()
                    .expect("the flags are rejected");
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::ArgumentConflict,
                "{view}: --last must conflict with --since"
            );
            let err =
                Cli::try_parse_from(["saniwalens", view, "--last", "7d", "--until", "2026-10-07"])
                    .err()
                    .expect("the flags are rejected");
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::ArgumentConflict,
                "{view}: --last must conflict with --until"
            );

            // Alone, the flags land in the shared groups: the period flags
            // and the read-path flags arrive as plain struct fields, with
            // nothing lost between the groups.
            let cli = Cli::try_parse_from([
                "saniwalens",
                view,
                "--last",
                "7d",
                "--frozen",
                "--db",
                "s.db",
            ])
            .unwrap_or_else(|err| panic!("{view}: the flags must parse: {err}"));
            let (since, until, last, frozen, db) = windowed_flags(cli.command);
            assert_eq!(since, None, "{view}");
            assert_eq!(until, None, "{view}");
            assert_eq!(last.as_deref(), Some("7d"), "{view}");
            assert!(frozen, "{view}");
            assert_eq!(
                db.as_deref().map(|p| p.display().to_string()),
                Some("s.db".to_string()),
                "{view}"
            );
        }
    }

    /// A temp-dir environment for the dispatch pins: state home under the
    /// dir, no config file — synthetic only.
    fn example_dir(sub: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("saniwalens-window-{sub}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn panic_analyzer()
    -> impl FnOnce(&Harness, Option<PathBuf>, &Path) -> anyhow::Result<AnalyzeStats> {
        |_, _, _| panic!("the analyzer must not run on this path")
    }

    #[test]
    fn the_windowed_open_resolves_flags_in_the_arms_order() {
        // The dispatch resolves scope, then format, then the period, then the
        // store — the arms' own order, so a double failure reports the same
        // error the current binary does and a flag failure never opens the
        // store or runs the analyzer. Reordering inside the open fails here,
        // not in a user's terminal.
        let dir = example_dir("windowed-open");
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: Some(&dir),
        };
        let dispatch = WindowedDispatch {
            harness: &Harness::Claude,
            config_db: None,
            env: &env,
        };
        let read = ReadArgs {
            frozen: true,
            db: Some(dir.join("store.db")),
        };

        // scope first, where the view has --scope: a bad scope wins over a
        // bad format, exactly as the scoped arms parse today.
        let period = PeriodArgs {
            since: None,
            until: None,
            last: None,
        };
        let err = dispatch
            .open(
                Some("nonsense"),
                Some("nonsense"),
                &period,
                &read,
                panic_analyzer(),
            )
            .expect_err("a bad scope must fail");
        assert!(
            err.to_string().contains("unknown --scope"),
            "the scope error must come first: {err}"
        );

        // format before the period: a bad format wins over a bad date.
        let bad_period = PeriodArgs {
            since: Some("2026/10/01".to_string()),
            until: None,
            last: None,
        };
        let err = dispatch
            .open(None, Some("nonsense"), &bad_period, &read, panic_analyzer())
            .expect_err("a bad format must fail");
        assert!(
            err.to_string().contains("unknown --format"),
            "the format error must come before the period: {err}"
        );

        // the period before the store opens: a bad date fails with the
        // analyzer never run — no store work, no freshness line.
        let err = dispatch
            .open(None, None, &bad_period, &read, panic_analyzer())
            .expect_err("a bad period must fail");
        assert!(
            err.to_string().contains("invalid --since '2026/10/01'"),
            "the period error must come before the open: {err}"
        );

        // All flags valid: the session opens (the frozen read never runs the
        // analyzer) and the resolved values come back resolved.
        let opened = dispatch
            .open(Some("global"), None, &period, &read, panic_analyzer())
            .expect("the flags resolve and the store opens");
        assert_eq!(opened.filter, ScopeFilter::Global);
        assert_eq!(opened.format, Format::Table);
        assert_eq!(opened.window, TimeRange::ALL);
        assert_eq!(opened.sess.db_path, dir.join("store.db"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_pair_open_resolves_flags_in_the_arms_order() {
        // trend's open: format, then the pair, then the store — the same
        // flags-before-store order, with the pair resolution where the
        // single-window views' period sits.
        let dir = example_dir("pair-open");
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: Some(&dir),
        };
        let dispatch = WindowedDispatch {
            harness: &Harness::Claude,
            config_db: None,
            env: &env,
        };
        let read = ReadArgs {
            frozen: true,
            db: Some(dir.join("store.db")),
        };

        // format before the pair.
        let bad_pair = PairArgs {
            since: None,
            until: Some("2026-10-01".to_string()),
            last: None,
        };
        let err = dispatch
            .open_pair(Some("nonsense"), &bad_pair, &read, panic_analyzer())
            .expect_err("a bad format must fail");
        assert!(
            err.to_string().contains("unknown --format"),
            "the format error must come before the pair: {err}"
        );

        // the pair before the store opens: the analyzer never runs.
        let err = dispatch
            .open_pair(None, &bad_pair, &read, panic_analyzer())
            .expect_err("a bad pair must fail");
        assert!(
            err.to_string().contains("--until requires --since"),
            "the pair error must come before the open: {err}"
        );

        // All flags valid: the pair resolves and the session opens.
        let pair = PairArgs {
            since: None,
            until: None,
            last: None,
        };
        let opened = dispatch
            .open_pair(None, &pair, &read, panic_analyzer())
            .expect("the flags resolve and the store opens");
        assert_eq!(opened.format, Format::Table);
        assert_eq!(
            opened.pair.previous.end.unwrap() + 1,
            opened.pair.current.start.unwrap()
        );
        assert_eq!(opened.sess.db_path, dir.join("store.db"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
