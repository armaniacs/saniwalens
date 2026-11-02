//! The nine windowed views' `--help`, end to end through the real binary:
//! each capture pins the help bytes, so the consolidated windowed flag surface
//! (`cli/window.rs`, one declared args group flattened into every view) cannot
//! change a wording, drop a conflict, or reorder the option list without
//! failing here. The diff against the pre-consolidation capture is the PBI's
//! byte-identity criterion; this pin keeps it mechanical after the fact.

/// The nine views the windowed flag surface serves, in capture order.
const VIEWS: [&str; 9] = [
    "usage",
    "trend",
    "inventory",
    "waste",
    "overhead",
    "prompts",
    "failures",
    "stuck",
    "doctor",
];

/// Each view's captured `--help`, in the same order as `VIEWS`.
const HELP: [&str; 9] = [
    USAGE_HELP,
    TREND_HELP,
    INVENTORY_HELP,
    WASTE_HELP,
    OVERHEAD_HELP,
    PROMPTS_HELP,
    FAILURES_HELP,
    STUCK_HELP,
    DOCTOR_HELP,
];

/// The captured `usage --help` bytes: the PBI 40 baseline (2026-11-01).
const USAGE_HELP: &str = r#"How often each skill ran and what it cost — per skill, or over time with --by

Usage: saniwalens usage [OPTIONS]

Options:
      --by <BY>            Bucket usage by time: year | month | week | day | hour (JST)
      --harness <HARNESS>  Which agent harness to analyze. Global so every command resolves its store and input roots through it; the default keeps every existing invocation byte-identical in behavior. See `docs/specs/harness.md`
      --profile <PROFILE>  Named preset from the config file to run under. Global so every command resolves through it; an explicit --harness or --db still wins over whatever the profile sets. See `docs/specs/cli.md`
      --since <DATE>       Only count events on or after this date (YYYY-MM-DD, JST)
      --until <DATE>       Only count events through this date (YYYY-MM-DD, JST) — the whole day counts
      --last <SPAN>        Rolling window ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive with the explicit dates
      --format <FORMAT>    Output format: table | markdown
      --frozen             Read the store as-is; skip the automatic refresh
      --db <DB>            Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db)
  -h, --help               Print help
"#;

/// The captured `trend --help` bytes: the PBI 40 baseline (2026-11-01).
const TREND_HELP: &str = r#"How usage moved: each kind of event, each skill, and the session and subagent totals this window against the equal-length window right before it (default 7d vs the 7d before)

Usage: saniwalens trend [OPTIONS]

Options:
      --harness <HARNESS>  Which agent harness to analyze. Global so every command resolves its store and input roots through it; the default keeps every existing invocation byte-identical in behavior. See `docs/specs/harness.md`
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
      --harness <HARNESS>  Which agent harness to analyze. Global so every command resolves its store and input roots through it; the default keeps every existing invocation byte-identical in behavior. See `docs/specs/harness.md`
      --scope <SCOPE>      Show one config layer only: global (~/.claude) | project (all projects' own config) | project:<slug> (one project's)
      --profile <PROFILE>  Named preset from the config file to run under. Global so every command resolves through it; an explicit --harness or --db still wins over whatever the profile sets. See `docs/specs/cli.md`
      --since <DATE>       Only count events on or after this date (YYYY-MM-DD, JST)
      --until <DATE>       Only count events through this date (YYYY-MM-DD, JST) — the whole day counts
      --last <SPAN>        Rolling window ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive with the explicit dates
      --format <FORMAT>    Output format: table | markdown
      --frozen             Read the store as-is; skip the automatic refresh
      --db <DB>            Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db)
  -h, --help               Print help
"#;

/// The captured `waste --help` bytes: the PBI 40 baseline (2026-11-01).
const WASTE_HELP: &str = r#"Cleanup opportunities: config that is never used, loaded every session while heavy, or rarely worth its cost — ranked by what removing it actually saves

Usage: saniwalens waste [OPTIONS]

Options:
      --harness <HARNESS>  Which agent harness to analyze. Global so every command resolves its store and input roots through it; the default keeps every existing invocation byte-identical in behavior. See `docs/specs/harness.md`
      --scope <SCOPE>      Show one config layer only: global (~/.claude) | project (all projects' own config) | project:<slug> (one project's)
      --profile <PROFILE>  Named preset from the config file to run under. Global so every command resolves through it; an explicit --harness or --db still wins over whatever the profile sets. See `docs/specs/cli.md`
      --since <DATE>       Only count events on or after this date (YYYY-MM-DD, JST)
      --until <DATE>       Only count events through this date (YYYY-MM-DD, JST) — the whole day counts
      --last <SPAN>        Rolling window ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive with the explicit dates
      --format <FORMAT>    Output format: table | markdown
      --frozen             Read the store as-is; skip the automatic refresh
      --db <DB>            Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db)
  -h, --help               Print help
"#;

/// The captured `overhead --help` bytes: the PBI 40 baseline (2026-11-01).
const OVERHEAD_HELP: &str = r#"What every session pays in context before any work starts, and how much of that comes from your own config files vs the harness itself

Usage: saniwalens overhead [OPTIONS]

Options:
      --format <FORMAT>    Output format: table | markdown
      --harness <HARNESS>  Which agent harness to analyze. Global so every command resolves its store and input roots through it; the default keeps every existing invocation byte-identical in behavior. See `docs/specs/harness.md`
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
      --format <FORMAT>    Output format: table | markdown
      --harness <HARNESS>  Which agent harness to analyze. Global so every command resolves its store and input roots through it; the default keeps every existing invocation byte-identical in behavior. See `docs/specs/harness.md`
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
      --harness <HARNESS>  Which agent harness to analyze. Global so every command resolves its store and input roots through it; the default keeps every existing invocation byte-identical in behavior. See `docs/specs/harness.md`
      --scope <SCOPE>      Show one config layer only: global (failures spread across projects — habits), project (each project's own failures), or project:<slug> (everything in that one project)
      --profile <PROFILE>  Named preset from the config file to run under. Global so every command resolves through it; an explicit --harness or --db still wins over whatever the profile sets. See `docs/specs/cli.md`
      --since <DATE>       Only count events on or after this date (YYYY-MM-DD, JST)
      --until <DATE>       Only count events through this date (YYYY-MM-DD, JST) — the whole day counts
      --last <SPAN>        Rolling window ending now: <N>d | <N>h | <N>w (e.g. 7d). Exclusive with the explicit dates
      --format <FORMAT>    Output format: table | markdown
      --frozen             Read the store as-is; skip the automatic refresh
      --db <DB>            Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db)
  -h, --help               Print help
"#;

/// The captured `stuck --help` bytes: the PBI 40 baseline (2026-11-01).
const STUCK_HELP: &str = r#"Files Claude kept re-editing in rapid bursts — where it got stuck retrying instead of making progress

Usage: saniwalens stuck [OPTIONS]

Options:
      --format <FORMAT>    Output format: table | markdown
      --harness <HARNESS>  Which agent harness to analyze. Global so every command resolves its store and input roots through it; the default keeps every existing invocation byte-identical in behavior. See `docs/specs/harness.md`
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
      --harness <HARNESS>  Which agent harness to analyze. Global so every command resolves its store and input roots through it; the default keeps every existing invocation byte-identical in behavior. See `docs/specs/harness.md`
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

#[test]
fn the_nine_windowed_views_help_is_pinned_byte_for_byte() {
    for (view, expected) in VIEWS.iter().zip(HELP.iter()) {
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
            actual, *expected,
            "{view} --help drifted from its capture — actual:\n{actual}"
        );
    }
}
