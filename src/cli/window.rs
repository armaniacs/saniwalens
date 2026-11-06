//! The windowed flag surface every report view shares, in one place: the
//! period flags declared once (clap args groups flattened into each command),
//! resolved ahead of the store through one session-open dispatch with an entry
//! per view shape, and the window's disclosure on stderr, in the JSON payloads,
//! and in a table's framing block.
//!
//! The vocabulary is carried by the return type, not by a flag bag: a view
//! without `--scope` gets [`WindowedRead`] and no filter to ignore, one with
//! `--scope` gets [`ScopedRead`], and doctor — whose `--format` set is
//! narrower than the table views' — has its own entry. Adding a view cannot
//! quietly acquire a filter it never shows or a format it never renders.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::adapter::harness::Harness;
use crate::cli::{Format, ViewFormat};
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

/// The one `--format` declaration for the table/markdown/json views. Flattened
/// into every view that accepts all three, so the wording cannot drift view by
/// view and a new view cannot omit the flag.
#[derive(clap::Args)]
pub(crate) struct FormatArgs {
    /// Output format: table | markdown | json.
    #[arg(long)]
    pub(crate) format: Option<String>,
}

impl FormatArgs {
    pub(crate) fn as_deref(&self) -> Option<&str> {
        self.format.as_deref()
    }
}

/// Doctor's narrower `--format`: its human form is its own text layout, not a
/// table, so it accepts only table or json.
#[derive(clap::Args)]
pub(crate) struct DoctorFormatArgs {
    /// Output format: table (human text) | json (the routed findings).
    #[arg(long)]
    pub(crate) format: Option<String>,
}

impl DoctorFormatArgs {
    pub(crate) fn as_deref(&self) -> Option<&str> {
        self.format.as_deref()
    }
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

/// What one unscoped windowed report arm's open call resolves: the opened
/// session and the flags every windowed view has. The four views without
/// `--scope` (usage, overhead, prompts, stuck) take this and nothing else, so
/// there is no filter for them to leave unread.
#[derive(Debug)]
pub(crate) struct WindowedRead {
    pub(crate) sess: Session,
    pub(crate) format: Format,
    pub(crate) window: TimeRange,
}

/// What one scoped windowed report arm's open call resolves: a
/// [`WindowedRead`] plus the config layer `--scope` selected. Only the three
/// views that share the default scoped entry (inventory, waste, failures)
/// take this, and only through [`WindowedDispatch::open_scoped`]; doctor has
/// its own `open_scoped_table_json` entry for the narrower `--format` set.
#[derive(Debug)]
pub(crate) struct ScopedRead {
    pub(crate) read: WindowedRead,
    pub(crate) filter: ScopeFilter,
}

impl ScopedRead {
    /// The two halves, for an arm that hands each to its view separately: the
    /// windowed read every view takes, and the layer only the scoped four
    /// have. A method rather than a struct pattern at the call site because
    /// rustfmt expands `let ScopedRead { .. } = <multi-line call>` to one line
    /// per field, and `src/cli/mod.rs` may not grow past its budget
    /// (`tests/cli_mod_line_budget.rs`).
    pub(crate) fn into_parts(self) -> (WindowedRead, ScopeFilter) {
        (self.read, self.filter)
    }
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
    /// The one open for the four windowed views without `--scope` (usage,
    /// overhead, prompts, stuck): format, then the period, then the store
    /// opens. The invariant — every flag resolves before the store opens, so
    /// an invalid flag fails on its own with no store error or freshness line
    /// ahead of it (`docs/specs/cli.md`) — is pinned here once.
    pub(crate) fn open(
        &self,
        format: Option<&str>,
        period: &PeriodArgs,
        read: &ReadArgs,
        analyze: impl FnOnce(&Harness, Option<PathBuf>, &Path) -> Result<AnalyzeStats>,
    ) -> Result<WindowedRead> {
        self.open_in(ViewFormat::TableMarkdownJson, format, period, read, analyze)
    }

    /// The one open for the three windowed views with `--scope` that take the
    /// default `--format` vocabulary (inventory, waste, failures); doctor
    /// reaches the same body through `open_scoped_table_json`. Scope, then the
    /// format, then the period, then the store: scope is resolved **here**,
    /// before delegating, which is what keeps it winning over a bad format
    /// without any arm ordering it by hand
    /// (`the_scoped_open_resolves_flags_in_the_arms_order`).
    pub(crate) fn open_scoped(
        &self,
        scope: Option<&str>,
        format: Option<&str>,
        period: &PeriodArgs,
        read: &ReadArgs,
        analyze: impl FnOnce(&Harness, Option<PathBuf>, &Path) -> Result<AnalyzeStats>,
    ) -> Result<ScopedRead> {
        self.open_scoped_in(
            ViewFormat::TableMarkdownJson,
            scope,
            format,
            period,
            read,
            analyze,
        )
    }

    /// doctor's open: the same scope-first order, with the two-value `--format`
    /// set whose `markdown` refusal lands before the store opens — the view's
    /// human form is its own text layout, not a pasted table.
    pub(crate) fn open_scoped_table_json(
        &self,
        scope: Option<&str>,
        format: Option<&str>,
        period: &PeriodArgs,
        read: &ReadArgs,
        analyze: impl FnOnce(&Harness, Option<PathBuf>, &Path) -> Result<AnalyzeStats>,
    ) -> Result<ScopedRead> {
        self.open_scoped_in(ViewFormat::TableJson, scope, format, period, read, analyze)
    }

    /// The scoped views' shared body: the view's vocabulary decides what
    /// `--format` may be; [`WindowedDispatch::open_in`] keeps the rest of the
    /// flags-before-store order.
    fn open_scoped_in(
        &self,
        view: ViewFormat,
        scope: Option<&str>,
        format: Option<&str>,
        period: &PeriodArgs,
        read: &ReadArgs,
        analyze: impl FnOnce(&Harness, Option<PathBuf>, &Path) -> Result<AnalyzeStats>,
    ) -> Result<ScopedRead> {
        let filter = super::parse_scope(scope)?;
        let read = self.open_in(view, format, period, read, analyze)?;
        Ok(ScopedRead { read, filter })
    }

    /// format, then the period, then the store — the half every windowed view
    /// shares, so the order has one counterpart instead of a copy per arm.
    fn open_in(
        &self,
        view: ViewFormat,
        format: Option<&str>,
        period: &PeriodArgs,
        read: &ReadArgs,
        analyze: impl FnOnce(&Harness, Option<PathBuf>, &Path) -> Result<AnalyzeStats>,
    ) -> Result<WindowedRead> {
        let format = view.parse(format)?;
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
        let format = ViewFormat::TableMarkdownJson.parse(format)?;
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
/// ahead-of-store evaluation `parse_scope` and [`ViewFormat::parse`] have, so
/// an unparseable date fails on its own. The clock is read here: this is the
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

/// The windowed views, derived from clap's subcommand tree: a subcommand is
/// windowed iff it carries `--last` (the period group's discriminator;
/// trend's pair declares its own). Derived, not listed: the conflict and
/// period tests below follow a view added to `Command` without a list to
/// update. `Cli::command()` is deliberately not `build()`t — build pushes the
/// `help` subcommand in, and the partition test in `tests/cli_help_golden.rs`
/// would then see a command it cannot classify. Keep in sync with
/// `windowed_views()` there (the same derivation, for the capture tripwire).
#[cfg(test)]
pub(crate) fn windowed_views() -> Vec<String> {
    use clap::CommandFactory;

    crate::cli::Cli::command()
        .get_subcommands()
        .filter(|command| {
            command
                .get_arguments()
                .any(|arg| arg.get_long() == Some("last"))
        })
        .map(|command| command.get_name().to_string())
        .collect()
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

    /// The windowed and read-path flags one parsed command holds, extracted
    /// per view: (since, until, last, frozen, db). The match is the flatten
    /// demonstration — every view carries the shared groups as plain fields,
    /// trend's pair included.
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
            Command::Trend { pair, read, .. } => {
                (pair.since, pair.until, pair.last, read.frozen, read.db)
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
            Command::Analyze { .. } | Command::Sql { .. } | Command::Optimize { .. } | Command::Tui => {
                unreachable!("the whole-history commands take no period flags")
            }
        }
    }

    #[test]
    fn the_shared_period_args_conflict_and_parse_on_every_windowed_view() {
        // One declaration carries the conflicts for the eight single-window
        // views: flatten keeps each field's arg id (the field name), so
        // `--last`'s `conflicts_with_all` — pinned once on the shared group —
        // resolves inside every command that flattens it. This mechanizes the
        // conflicts for all eight (PBI 40: the six views the older conflict
        // pin left uncovered — inventory, overhead, prompts, failures, stuck,
        // doctor — are asserted here rather than left to the core's second
        // defense, whose wording and exit code differ from the clap
        // conflict's). trend's pair carries the same conflicts on its own
        // declaration; the loop below runs on every windowed view (the
        // derived set, trend included) so the set stays clap's, and the flag
        // extraction below exercises the pair arm too.
        for view in windowed_views() {
            let view = view.as_str();
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

    fn panic_analyzer()
    -> impl FnOnce(&Harness, Option<PathBuf>, &Path) -> anyhow::Result<AnalyzeStats> {
        |_, _, _| panic!("the analyzer must not run on this path")
    }

    #[test]
    fn the_unscoped_open_resolves_flags_in_the_arms_order() {
        // The four views without --scope resolve format, then the period,
        // then the store — a flag failure never opens the store or runs the
        // analyzer, and the resolved read carries no filter to ignore.
        // The scratch guard is declared before `opened`: locals drop in
        // reverse declaration order, so the store's handle closes before its
        // dir is removed (a directory cannot be removed under an open store
        // on Windows).
        let dir = crate::scratch::ScratchDir::new("saniwalens-window", "windowed-open");
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

        // format before the period: a bad format wins over a bad date.
        let bad_period = PeriodArgs {
            since: Some("2026/10/01".to_string()),
            until: None,
            last: None,
        };
        let err = dispatch
            .open(Some("nonsense"), &bad_period, &read, panic_analyzer())
            .expect_err("a bad format must fail");
        assert!(
            err.to_string().contains("unknown --format"),
            "the format error must come before the period: {err}"
        );

        // the period before the store opens: a bad date fails with the
        // analyzer never run — no store work, no freshness line.
        let err = dispatch
            .open(None, &bad_period, &read, panic_analyzer())
            .expect_err("a bad period must fail");
        assert!(
            err.to_string().contains("invalid --since '2026/10/01'"),
            "the period error must come before the open: {err}"
        );

        // All flags valid: the session opens (the frozen read never runs the
        // analyzer) and the resolved values come back resolved.
        let period = PeriodArgs {
            since: None,
            until: None,
            last: None,
        };
        let opened = dispatch
            .open(None, &period, &read, panic_analyzer())
            .expect("the flags resolve and the store opens");
        assert_eq!(opened.format, Format::Table);
        assert_eq!(opened.window, TimeRange::ALL);
        assert_eq!(opened.sess.db_path, dir.join("store.db"));
    }

    #[test]
    fn the_scoped_open_resolves_flags_in_the_arms_order() {
        // The four views with --scope resolve scope, then format, then the
        // period, then the store. Scope is parsed inside the entry, so a bad
        // scope wins over a bad format without the arm ordering the two
        // calls by hand.
        let dir = crate::scratch::ScratchDir::new("saniwalens-window", "scoped-open");
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
        let period = PeriodArgs {
            since: None,
            until: None,
            last: None,
        };

        // scope first: a bad scope wins over a bad format.
        let err = dispatch
            .open_scoped(
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
            .open_scoped(None, Some("nonsense"), &bad_period, &read, panic_analyzer())
            .expect_err("a bad format must fail");
        assert!(
            err.to_string().contains("unknown --format"),
            "the format error must come before the period: {err}"
        );

        // the period before the store opens: a bad date fails with the
        // analyzer never run — no store work, no freshness line.
        let err = dispatch
            .open_scoped(None, None, &bad_period, &read, panic_analyzer())
            .expect_err("a bad period must fail");
        assert!(
            err.to_string().contains("invalid --since '2026/10/01'"),
            "the period error must come before the open: {err}"
        );

        // All flags valid: the filter comes back resolved beside the read.
        let opened = dispatch
            .open_scoped(Some("global"), None, &period, &read, panic_analyzer())
            .expect("the flags resolve and the store opens");
        assert_eq!(opened.filter, ScopeFilter::Global);
        assert_eq!(opened.read.format, Format::Table);
        assert_eq!(opened.read.window, TimeRange::ALL);
        assert_eq!(opened.read.sess.db_path, dir.join("store.db"));

        // No --scope means both layers, not an absent filter.
        let opened = dispatch
            .open_scoped(None, None, &period, &read, panic_analyzer())
            .expect("the flags resolve and the store opens");
        assert_eq!(opened.filter, ScopeFilter::All);
    }

    #[test]
    fn doctors_open_refuses_markdown_before_the_store_opens() {
        // doctor's --format set is table|json: the refusal is part of the
        // open, so it lands before the analyzer and the store's creation.
        // `md` resolves to markdown elsewhere; here it is simply unaccepted.
        let dir = crate::scratch::ScratchDir::new("saniwalens-window", "doctor-open");
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
        let period = PeriodArgs {
            since: None,
            until: None,
            last: None,
        };

        for value in ["markdown", "md"] {
            let err = dispatch
                .open_scoped_table_json(None, Some(value), &period, &read, panic_analyzer())
                .expect_err("doctor must refuse markdown");
            assert_eq!(
                err.to_string(),
                format!("unknown --format '{value}' (table|json)")
            );
            assert!(
                !read.db.as_ref().expect("a db path").exists(),
                "the store was created before {value} was refused"
            );
        }

        // The two values it does take resolve to the formats doctor renders.
        for (value, expected) in [(None, Format::Table), (Some("json"), Format::Json)] {
            let opened = dispatch
                .open_scoped_table_json(Some("project"), value, &period, &read, panic_analyzer())
                .expect("doctor's own values must resolve");
            assert_eq!(opened.read.format, expected);
            assert_eq!(opened.filter, ScopeFilter::Project(None));
        }
    }

    #[test]
    fn the_pair_open_resolves_flags_in_the_arms_order() {
        // trend's open: format, then the pair, then the store — the same
        // flags-before-store order, with the pair resolution where the
        // single-window views' period sits.
        let dir = crate::scratch::ScratchDir::new("saniwalens-window", "pair-open");
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
    }
}
