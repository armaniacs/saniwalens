//! Every windowed view's `--help`, end to end through the real binary: each
//! capture pins the help bytes, so the consolidated windowed flag surface
//! (`cli/window.rs`, one declared args group flattened into every view) cannot
//! change a wording, drop a conflict, or reorder the option list without
//! failing here. The set of views is not listed here either — it is derived
//! from clap's subcommand tree, so a view added to `Command` cannot ship
//! without its capture. The diff against the pre-consolidation capture is the
//! PBI's byte-identity criterion; this pin keeps it mechanical after the fact.

use clap::CommandFactory;

/// One row per windowed view: its name and its captured `--help` bytes. One
/// row is one item, so a name cannot be added without its capture (the old
/// two arrays let exactly that through: `zip` stopped at the shorter side
/// without a word).
const PINNED: [(&str, &str); 9] = [
    ("usage", USAGE_HELP),
    ("trend", TREND_HELP),
    ("inventory", INVENTORY_HELP),
    ("waste", WASTE_HELP),
    ("overhead", OVERHEAD_HELP),
    ("prompts", PROMPTS_HELP),
    ("failures", FAILURES_HELP),
    ("stuck", STUCK_HELP),
    ("doctor", DOCTOR_HELP),
];

/// The captured `usage --help` bytes: the PBI 40 baseline (2026-11-01).
const USAGE_HELP: &str = r#"How often each skill ran and what it cost — per skill, or over time with --by

Usage: saniwalens usage [OPTIONS]

Options:
      --by <BY>            Bucket usage by time: year | month | week | day | hour (JST)
      --harness <HARNESS>  Which agent harness to analyze: claude (default) | opencode | kilocode. Global so every command resolves its store and input roots through it. See `docs/specs/harness.md`
      --profile <PROFILE>  Named preset from the config file to run under. Global so every command resolves through it; an explicit --harness or --db still wins over whatever the profile sets. See `docs/specs/cli.md`
      --since <DATE>       Only count events on or after this date (YYYY-MM-DD, JST)
      --until <DATE>       Only count events through this date (YYYY-MM-DD, JST) — the whole day counts
      --last <SPAN>        Rolling window ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive with the explicit dates
      --format <FORMAT>    Output format: table | markdown | json
      --frozen             Read the store as-is; skip the automatic refresh
      --db <DB>            Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db)
  -h, --help               Print help
"#;

/// The captured `trend --help` bytes: the PBI 40 baseline (2026-11-01).
const TREND_HELP: &str = r#"How usage moved: each kind of event, each skill, and the session and subagent totals this window against the equal-length window right before it (default 7d vs the 7d before)

Usage: saniwalens trend [OPTIONS]

Options:
      --harness <HARNESS>  Which agent harness to analyze: claude (default) | opencode | kilocode. Global so every command resolves its store and input roots through it. See `docs/specs/harness.md`
      --since <DATE>       Current window start (YYYY-MM-DD, JST). The window to compare it with is the same length immediately before it
      --profile <PROFILE>  Named preset from the config file to run under. Global so every command resolves through it; an explicit --harness or --db still wins over whatever the profile sets. See `docs/specs/cli.md`
      --until <DATE>       Current window end (YYYY-MM-DD, JST) — the whole day counts. Needs --since; alone, the window to compare against is undefined
      --last <SPAN>        Rolling pair ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive with the explicit dates
      --format <FORMAT>    Output format: table | markdown | json
      --frozen             Read the store as-is; skip the automatic refresh
      --db <DB>            Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db)
  -h, --help               Print help
"#;

/// The captured `inventory --help` bytes: the PBI 40 baseline (2026-11-01).
const INVENTORY_HELP: &str = r#"Everything installed in your config (skills, rules, agents, MCP servers, CLAUDE.md), what it weighs, and whether it was ever used

Usage: saniwalens inventory [OPTIONS]

Options:
      --harness <HARNESS>  Which agent harness to analyze: claude (default) | opencode | kilocode. Global so every command resolves its store and input roots through it. See `docs/specs/harness.md`
      --scope <SCOPE>      Show one config layer only: global (~/.claude) | project (all projects' own config) | project:<slug> (one project's)
      --profile <PROFILE>  Named preset from the config file to run under. Global so every command resolves through it; an explicit --harness or --db still wins over whatever the profile sets. See `docs/specs/cli.md`
      --since <DATE>       Only count events on or after this date (YYYY-MM-DD, JST)
      --until <DATE>       Only count events through this date (YYYY-MM-DD, JST) — the whole day counts
      --last <SPAN>        Rolling window ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive with the explicit dates
      --format <FORMAT>    Output format: table | markdown | json
      --frozen             Read the store as-is; skip the automatic refresh
      --db <DB>            Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db)
  -h, --help               Print help
"#;

/// The captured `waste --help` bytes: the PBI 40 baseline (2026-11-01).
const WASTE_HELP: &str = r#"Cleanup opportunities: config that is never used, loaded every session while heavy, or rarely worth its cost — ranked by what removing it actually saves

Usage: saniwalens waste [OPTIONS]

Options:
      --harness <HARNESS>  Which agent harness to analyze: claude (default) | opencode | kilocode. Global so every command resolves its store and input roots through it. See `docs/specs/harness.md`
      --scope <SCOPE>      Show one config layer only: global (~/.claude) | project (all projects' own config) | project:<slug> (one project's)
      --profile <PROFILE>  Named preset from the config file to run under. Global so every command resolves through it; an explicit --harness or --db still wins over whatever the profile sets. See `docs/specs/cli.md`
      --since <DATE>       Only count events on or after this date (YYYY-MM-DD, JST)
      --until <DATE>       Only count events through this date (YYYY-MM-DD, JST) — the whole day counts
      --last <SPAN>        Rolling window ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive with the explicit dates
      --format <FORMAT>    Output format: table | markdown | json
      --frozen             Read the store as-is; skip the automatic refresh
      --db <DB>            Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db)
  -h, --help               Print help
"#;

/// The captured `overhead --help` bytes: the PBI 40 baseline (2026-11-01).
const OVERHEAD_HELP: &str = r#"What every session pays in context before any work starts, and how much of that comes from your own config files vs the harness itself

Usage: saniwalens overhead [OPTIONS]

Options:
      --format <FORMAT>    Output format: table | markdown | json
      --harness <HARNESS>  Which agent harness to analyze: claude (default) | opencode | kilocode. Global so every command resolves its store and input roots through it. See `docs/specs/harness.md`
      --profile <PROFILE>  Named preset from the config file to run under. Global so every command resolves through it; an explicit --harness or --db still wins over whatever the profile sets. See `docs/specs/cli.md`
      --since <DATE>       Only count events on or after this date (YYYY-MM-DD, JST)
      --until <DATE>       Only count events through this date (YYYY-MM-DD, JST) — the whole day counts
      --last <SPAN>        Rolling window ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive with the explicit dates
      --frozen             Read the store as-is; skip the automatic refresh
      --db <DB>            Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db)
  -h, --help               Print help
"#;

/// The captured `prompts --help` bytes: the PBI 40 baseline (2026-11-01).
const PROMPTS_HELP: &str = r#"How you steer Claude: the mix of instructing, steering ("go ahead"), correcting ("no, instead…"), and questioning prompts — and what that mix suggests changing

Usage: saniwalens prompts [OPTIONS]

Options:
      --format <FORMAT>    Output format: table | markdown | json
      --harness <HARNESS>  Which agent harness to analyze: claude (default) | opencode | kilocode. Global so every command resolves its store and input roots through it. See `docs/specs/harness.md`
      --profile <PROFILE>  Named preset from the config file to run under. Global so every command resolves through it; an explicit --harness or --db still wins over whatever the profile sets. See `docs/specs/cli.md`
      --since <DATE>       Only count events on or after this date (YYYY-MM-DD, JST)
      --until <DATE>       Only count events through this date (YYYY-MM-DD, JST) — the whole day counts
      --last <SPAN>        Rolling window ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive with the explicit dates
      --frozen             Read the store as-is; skip the automatic refresh
      --db <DB>            Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db)
  -h, --help               Print help
"#;

/// The captured `failures --help` bytes: the PBI 40 baseline (2026-11-01).
const FAILURES_HELP: &str = r#"Tool calls that kept failing the same way — recurring failures that waste turns and tokens, each with a suggested fix

Usage: saniwalens failures [OPTIONS]

Options:
      --harness <HARNESS>  Which agent harness to analyze: claude (default) | opencode | kilocode. Global so every command resolves its store and input roots through it. See `docs/specs/harness.md`
      --scope <SCOPE>      Show one config layer only: global (failures spread across projects — habits), project (each project's own failures), or project:<slug> (everything in that one project)
      --profile <PROFILE>  Named preset from the config file to run under. Global so every command resolves through it; an explicit --harness or --db still wins over whatever the profile sets. See `docs/specs/cli.md`
      --since <DATE>       Only count events on or after this date (YYYY-MM-DD, JST)
      --until <DATE>       Only count events through this date (YYYY-MM-DD, JST) — the whole day counts
      --last <SPAN>        Rolling window ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive with the explicit dates
      --format <FORMAT>    Output format: table | markdown | json
      --frozen             Read the store as-is; skip the automatic refresh
      --db <DB>            Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db)
  -h, --help               Print help
"#;

/// The captured `stuck --help` bytes: the PBI 40 baseline (2026-11-01).
const STUCK_HELP: &str = r#"Files Claude kept re-editing in rapid bursts — where it got stuck retrying instead of making progress

Usage: saniwalens stuck [OPTIONS]

Options:
      --format <FORMAT>    Output format: table | markdown | json
      --harness <HARNESS>  Which agent harness to analyze: claude (default) | opencode | kilocode. Global so every command resolves its store and input roots through it. See `docs/specs/harness.md`
      --profile <PROFILE>  Named preset from the config file to run under. Global so every command resolves through it; an explicit --harness or --db still wins over whatever the profile sets. See `docs/specs/cli.md`
      --since <DATE>       Only count events on or after this date (YYYY-MM-DD, JST)
      --until <DATE>       Only count events through this date (YYYY-MM-DD, JST) — the whole day counts
      --last <SPAN>        Rolling window ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive with the explicit dates
      --frozen             Read the store as-is; skip the automatic refresh
      --db <DB>            Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db)
  -h, --help               Print help
"#;

/// The captured `doctor --help` bytes: the PBI 40 baseline (2026-11-01).
const DOCTOR_HELP: &str = r#"The health check — what to fix first, what your sessions cost, and what is fine as-is. Start here

Usage: saniwalens doctor [OPTIONS]

Options:
      --harness <HARNESS>  Which agent harness to analyze: claude (default) | opencode | kilocode. Global so every command resolves its store and input roots through it. See `docs/specs/harness.md`
      --scope <SCOPE>      Show one config layer only: global (~/.claude) | project (all projects' own config) | project:<slug> (one project's)
      --profile <PROFILE>  Named preset from the config file to run under. Global so every command resolves through it; an explicit --harness or --db still wins over whatever the profile sets. See `docs/specs/cli.md`
      --since <DATE>       Only count events on or after this date (YYYY-MM-DD, JST)
      --until <DATE>       Only count events through this date (YYYY-MM-DD, JST) — the whole day counts
      --last <SPAN>        Rolling window ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive with the explicit dates
      --format <FORMAT>    Output format: table (human text) | json (the routed findings)
      --frozen             Read the store as-is; skip the automatic refresh
      --db <DB>            Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db)
  -h, --help               Print help
"#;

/// The windowed views, derived from the subcommand tree: a subcommand is a
/// windowed view iff it carries `--last` (the period group's discriminator;
/// trend's pair declares its own). The tree is the single source of truth — a
/// view added to `Command` appears here without a list to update.
/// `Cli::command()` is deliberately not `build()`t: build pushes the `help`
/// subcommand in, which would land in the partition check below. Keep in sync
/// with `cli::window::windowed_views()` in `src/cli/window.rs` (the same
/// derivation, for the conflict and period pins).
fn windowed_views() -> Vec<String> {
    saniwalens::cli::Cli::command()
        .get_subcommands()
        .filter(|command| {
            command
                .get_arguments()
                .any(|arg| arg.get_long() == Some("last"))
        })
        .map(|command| command.get_name().to_string())
        .collect()
}

/// The commands `docs/specs/cli.md` (the Period section) declares
/// whole-history: they take no period flag. Fixed by that contract, so the
/// partition below can tell a command that is not a view from a view that
/// carries the shared group.
const WHOLE_HISTORY: [&str; 4] = ["analyze", "sql", "optimize", "tui"];

/// The count the discriminator resolves to today. Beside the set comparison,
/// this pins the set's size: deleting a view and its capture row together
/// agrees with itself, but not with the CLI contract the spec states, so the
/// count forces that change to be deliberate.
const WINDOWED_VIEW_COUNT: usize = 9;

/// The remedy half of the view-set tripwire's failure message, pinned here so
/// its wording cannot drift away from what it prescribes.
const VIEW_SET_REMEDY: &str = "Add one row to `PINNED` in this file for each \
     missing view, and delete the row of each stale one. See \
     `.claude/rules/cli-view-addition.md`.";

/// The two ways the capture table and the CLI can disagree: a windowed view
/// with no capture (`missing`), or a capture for a view the CLI no longer has
/// (`stale`). `None` when the sets agree, so the message itself is testable
/// without tripping the tripwire.
fn view_set_mismatch(derived: &[String], pinned: &[String]) -> Option<String> {
    let missing: Vec<&str> = derived
        .iter()
        .filter(|name| !pinned.contains(name))
        .map(String::as_str)
        .collect();
    let stale: Vec<&str> = pinned
        .iter()
        .filter(|name| !derived.contains(name))
        .map(String::as_str)
        .collect();
    (!missing.is_empty() || !stale.is_empty()).then(|| {
        format!(
            "the --help captures and the CLI disagree: missing from the hand list: \
             {missing:?}; stale in the hand list: {stale:?}. {VIEW_SET_REMEDY}"
        )
    })
}

#[test]
fn every_windowed_view_has_a_capture_and_the_capture_set_is_complete() {
    let derived = windowed_views();
    let pinned: Vec<String> = PINNED.iter().map(|(name, _)| (*name).to_string()).collect();
    if let Some(message) = view_set_mismatch(&derived, &pinned) {
        panic!("{message}");
    }
    assert_eq!(
        derived.len(),
        WINDOWED_VIEW_COUNT,
        "the windowed view set resolved to {derived:?}, not {WINDOWED_VIEW_COUNT}: adding or \
         removing a view is a contract change, so update this count and `docs/specs/cli.md` \
         together, or give the view the shared `--last`. {VIEW_SET_REMEDY}"
    );
}

#[test]
fn the_view_set_mismatch_message_names_missing_stale_and_the_rule() {
    let derived = vec!["usage".to_string(), "cleanup".to_string()];
    let pinned = vec!["usage".to_string(), "retired".to_string()];
    let message = view_set_mismatch(&derived, &pinned).expect("a mismatch reports a message");
    assert!(
        message.contains("missing from the hand list"),
        "the message must name the missing side: {message}"
    );
    assert!(
        message.contains("stale in the hand list"),
        "the message must name the stale side: {message}"
    );
    assert!(
        message.contains("`.claude/rules/cli-view-addition.md`"),
        "the message must point at the view-addition rule: {message}"
    );
    assert_eq!(
        view_set_mismatch(&derived, &derived),
        None,
        "agreeing sets report nothing"
    );
}

#[test]
fn every_subcommand_is_either_windowed_or_whole_history() {
    let mut all: Vec<String> = saniwalens::cli::Cli::command()
        .get_subcommands()
        .map(|command| command.get_name().to_string())
        .collect();
    let mut covered = windowed_views();
    for command in WHOLE_HISTORY {
        assert!(
            !covered.iter().any(|view| view == command),
            "{command} carries `--last` and is also declared whole-history"
        );
        covered.push(command.to_string());
    }
    all.sort();
    covered.sort();
    // Name the two ways the partition can fail, so the failure itself says
    // which side to fix: a subcommand no set claims (uncovered), or a name
    // `WHOLE_HISTORY` lists that the CLI no longer has (stale).
    let uncovered: Vec<&String> = all.iter().filter(|name| !covered.contains(name)).collect();
    let stale: Vec<&String> = covered.iter().filter(|name| !all.contains(name)).collect();
    assert_eq!(
        covered, all,
        "the subcommand partition is incomplete: {uncovered:?} is neither windowed (it carries \
         no `--last`) nor whole-history; {stale:?} is listed but is no longer a subcommand. \
         Give the command the shared period group, or add it to `WHOLE_HISTORY` in this file. \
         See `.claude/rules/cli-view-addition.md`."
    );
}

#[test]
fn every_windowed_views_help_is_pinned_byte_for_byte() {
    for (view, expected) in PINNED {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_saniwalens"))
            .args([view, "--help"])
            .output()
            .expect("the binary runs");
        assert!(
            out.status.success(),
            "{view} --help failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let actual = String::from_utf8(out.stdout).unwrap();
        assert_eq!(
            actual, expected,
            "{view} --help drifted from its capture — actual:\n{actual}"
        );
    }
}

#[test]
fn each_windowed_view_declares_format_once() {
    let windowed = windowed_views();
    for subcommand in saniwalens::cli::Cli::command().get_subcommands() {
        let count = subcommand
            .get_arguments()
            .filter(|arg| arg.get_long() == Some("format"))
            .count();
        if windowed.iter().any(|view| view == subcommand.get_name()) {
            assert_eq!(
                count,
                1,
                "{} must declare --format exactly once",
                subcommand.get_name()
            );
        } else {
            assert!(
                count <= 1,
                "{} declares --format more than once",
                subcommand.get_name()
            );
        }
    }
}
