//! CLI: the `analyze` and `report` commands. This is the thin shell that walks
//! files and renders tables; the analysis itself lives in the pure core. See
//! `docs/specs/cli.md`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::adapter::harness::{Harness, home_dir};
use crate::core::findings;
use crate::core::optimize as optimize_mod;
use crate::core::period::TimeRange;
use crate::core::scope::ScopeFilter;
use crate::core::subagent::SplitCoverage;
use crate::core::surface::{StartupSavings, Wedge};
use crate::session::{
    AnalyzeStats, Session, SessionEnv, Style, env_roots, eprint_freshness, freshness_line,
    guard_harness, resolve_cli_with_language,
};
use crate::store::{OpenMode, Store};

pub(crate) mod cleanup;
pub(crate) mod deliver;
pub(crate) mod handoff;
pub(crate) mod headless;
pub mod heartbeat;
pub(crate) mod report;
pub(crate) mod views;
pub(crate) mod window;

pub use deliver::{AdvisorLaunch, DeliveryMode, DeliveryPlan, deliver};
use deliver::{ProcessAdvisor, delivery_mode};
pub(crate) use deliver::{private_temp_path, write_private_file};
pub(crate) use report::*;
pub(crate) use views::trend::*;
pub(crate) use views::{
    doctor::doctor, failures::failures, inventory::inventory, overhead::overhead, stuck::stuck,
    usage::usage, waste::waste,
};
pub(crate) use window::*;

#[derive(Parser)]
#[command(
    name = "saniwalens",
    // Cargo.toml holds a placeholder that the release workflow and the flake
    // both stamp from the tag before building; build.rs additionally stamps
    // the git tag itself, so `--version` names the release even for a plain
    // source build.
    version = env!("SANIWALENS_VERSION"),
    about = "See where your coding-agent time and tokens go, and which config to fix.\n\
             Start with `saniwalens doctor`; every report explains itself and names the\n\
             follow-up command."
)]
pub struct Cli {
    /// Which agent harness to analyze: claude (default) | opencode |
    /// kilocode. Global so every command resolves its store and input
    /// roots through it. See `docs/specs/harness.md`.
    #[arg(long, global = true)]
    harness: Option<String>,
    /// Named preset from the config file to run under. Global so every
    /// command resolves through it; an explicit --harness or --db still wins
    /// over whatever the profile sets. See `docs/specs/cli.md`.
    #[arg(long, global = true)]
    profile: Option<String>,
    /// Advisor output language: en (the default) | ja. Global so every
    /// command resolves it through the same flag-beats-file order; only
    /// `optimize` renders it into a prompt today. See `docs/specs/cli.md`.
    #[arg(long, global = true)]
    language: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Read your session logs and config into the local store. Runs
    /// automatically before every report — call it directly only to refresh
    /// without reading anything.
    Analyze {
        /// Transcript root (default: $CLAUDE_CONFIG_DIR/projects, or
        /// ~/.claude/projects).
        #[arg(long)]
        projects: Option<PathBuf>,
        /// Output format: table | json (the run's counters).
        #[arg(long)]
        format: Option<String>,
        /// Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db).
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// How often each skill ran and what it cost — per skill, or over time
    /// with --by.
    Usage {
        /// Bucket usage by time: year | month | week | day | hour (JST).
        #[arg(long)]
        by: Option<String>,
        #[command(flatten)]
        period: PeriodArgs,
        #[command(flatten)]
        format: FormatArgs,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// How usage moved: each kind of event, each skill, and the session and
    /// subagent totals this window against the equal-length window right
    /// before it (default 7d vs the 7d before).
    Trend {
        #[command(flatten)]
        pair: PairArgs,
        #[command(flatten)]
        format: FormatArgs,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// Where the tokens went, by model: main-thread usage and subagent runs
    /// grouped per model, costliest first.
    Models {
        #[command(flatten)]
        period: PeriodArgs,
        #[command(flatten)]
        format: FormatArgs,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// Every session: what it spent, how long it ran, and how often tool
    /// calls kept failing inside it — costliest first.
    Sessions {
        /// Sort by: tokens (total output) | duration (wall-clock span) |
        /// friction (tool-error count). Default: tokens.
        #[arg(long)]
        sort: Option<String>,
        #[command(flatten)]
        period: PeriodArgs,
        #[command(flatten)]
        format: FormatArgs,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// Everything installed in your config (skills, rules, agents, MCP
    /// servers, CLAUDE.md), what it weighs, and whether it was ever used.
    Inventory {
        /// Show one config layer only: global (~/.claude) | project (all
        /// projects' own config) | project:<slug> (one project's).
        #[arg(long)]
        scope: Option<String>,
        #[command(flatten)]
        period: PeriodArgs,
        #[command(flatten)]
        format: FormatArgs,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// Cleanup opportunities: config that is never used, loaded every session
    /// while heavy, or rarely worth its cost — ranked by what removing it
    /// actually saves.
    Waste {
        /// Show one config layer only: global (~/.claude) | project (all
        /// projects' own config) | project:<slug> (one project's).
        #[arg(long)]
        scope: Option<String>,
        #[command(flatten)]
        period: PeriodArgs,
        #[command(flatten)]
        format: FormatArgs,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// What every session pays in context before any work starts, and how
    /// much of that comes from your own config files vs the harness itself.
    Overhead {
        #[command(flatten)]
        format: FormatArgs,
        #[command(flatten)]
        period: PeriodArgs,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// How you steer Claude: the mix of instructing, steering ("go ahead"),
    /// correcting ("no, instead…"), and questioning prompts — and what that
    /// mix suggests changing.
    Prompts {
        #[command(flatten)]
        format: FormatArgs,
        #[command(flatten)]
        period: PeriodArgs,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// Tool calls that kept failing the same way — recurring failures that
    /// waste turns and tokens, each with a suggested fix.
    Failures {
        /// Show one config layer only: global (failures spread across
        /// projects — habits), project (each project's own failures), or
        /// project:<slug> (everything in that one project).
        #[arg(long)]
        scope: Option<String>,
        #[command(flatten)]
        period: PeriodArgs,
        #[command(flatten)]
        format: FormatArgs,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// Files Claude kept re-editing in rapid bursts — where it got stuck
    /// retrying instead of making progress.
    Stuck {
        #[command(flatten)]
        format: FormatArgs,
        #[command(flatten)]
        period: PeriodArgs,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// The health check — what to fix first, what your sessions cost, and
    /// what is fine as-is. Start here.
    Doctor {
        /// Show one config layer only: global (~/.claude) | project (all
        /// projects' own config) | project:<slug> (one project's).
        #[arg(long)]
        scope: Option<String>,
        #[command(flatten)]
        period: PeriodArgs,
        #[command(flatten)]
        format: DoctorFormatArgs,
        #[command(flatten)]
        read: ReadArgs,
    },
    /// Ask anything the fixed reports don't cover: run a read-only SQL query
    /// against the analyzed store (as an argument, or piped via stdin).
    /// `SELECT sql FROM sqlite_master` shows the schema.
    Sql {
        /// The SQL to run. If omitted, the query is read from stdin.
        query: Option<String>,
        /// Max result rows (default 10000); 0 means no limit. A truncated
        /// result warns on stderr and names this flag, so a silent partial
        /// slice never passes as complete.
        #[arg(long, default_value_t = 10_000)]
        limit: u64,
        #[command(flatten)]
        format: FormatArgs,
        /// Store path (default: $XDG_STATE_HOME/saniwalens/saniwalens.db).
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Act on the findings: opens an advisor session pre-loaded
    /// with the full analysis, which investigates each problem and proposes
    /// concrete config fixes (interactively under Claude Code, headless
    /// under opencode).
    Optimize {
        /// Transcript root (default: $CLAUDE_CONFIG_DIR/projects, or
        /// ~/.claude/projects).
        #[arg(long)]
        projects: Option<PathBuf>,
        /// Store to analyze into / read from (default:
        /// $XDG_STATE_HOME/saniwalens/saniwalens.db).
        #[arg(long)]
        db: Option<PathBuf>,
        /// Optimize one config layer only: global (~/.claude) | project |
        /// project:<slug>.
        #[arg(long)]
        scope: Option<String>,
        /// Use the existing store as-is; skip the analyze step.
        #[arg(long)]
        frozen: bool,
        /// Print the composed prompt instead of launching the advisor.
        #[arg(long)]
        print: bool,
        /// Stream the advisor's raw output instead of the quiet render
        /// (headless opencode runs log raw output to a file by default).
        #[arg(long)]
        verbose: bool,
        /// Save the headless plan under $XDG_STATE_HOME/saniwalens/plans/ and
        /// open the harness's own interactive session to apply it (headless only).
        #[arg(long)]
        apply: bool,
        /// Stop a headless advisor after this long (e.g. 30s, 2m, 500ms).
        #[arg(long, value_name = "DURATION")]
        timeout: Option<String>,
    },
    /// Step-by-step guide to the typical patterns (doctor / waste / failures).
    /// Needs an interactive terminal; without one it refuses instead of hanging.
    Tui,
    /// List or prune the `--apply` handoff's saved plans and backups.
    /// Needs no store: it only reads (and, with `--keep`, deletes) under
    /// $XDG_STATE_HOME/saniwalens/plans/ and .../backups/.
    Cleanup {
        /// Only list plans and backups with their sizes; delete nothing.
        /// A bare run lists too; the sweep needs an explicit `--keep`.
        #[arg(long, conflicts_with = "keep")]
        list: bool,
        /// Delete all but the newest N generations in each area, showing the
        /// count and total size first. A bare `--keep` keeps 10.
        #[arg(long, num_args(0..=1), default_missing_value = "10")]
        keep: Option<usize>,
        #[command(flatten)]
        format: FormatArgs,
    },
}

pub fn run() -> Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    if let Some(entry) = views::guide::guide_entry_if_terminal(&argv) {
        return views::guide::run_guide_entry(entry);
    }
    // Non-TTY without a subcommand: fall through to `Cli::parse`, whose
    // missing-subcommand error is what scripts see today (behavior unchanged).
    let cli = Cli::parse();
    // One call resolves flags, the config file, and the builtins into the
    // harness plus an optional configured store; see `session.rs`.
    let roots = env_roots();
    let env = SessionEnv {
        state_home: roots.state_home.as_deref(),
        home: home_dir(),
        config_home: roots.config_home.as_deref(),
    };
    let resolved = resolve_cli_with_language(
        cli.harness.as_deref(),
        cli.profile.as_deref(),
        cli.language.as_deref(),
        &env,
    )?;
    let harness = resolved.harness;
    let config_db = resolved.config_db;
    let language = resolved.language;
    // The windowed report arms resolve their flags through `cli/window.rs` —
    // `open` (no scope) or `open_scoped` (scope, format, period) — so an
    // invalid flag fails on its own before any store work (PBI 01's order).
    let dispatch = WindowedDispatch {
        harness: &harness,
        config_db: config_db.as_deref(),
        env: &env,
    };
    match cli.command {
        Command::Analyze {
            projects,
            format,
            db,
        } => {
            let fmt = parse_format(format.as_deref())?;
            let sess = Session::open(&harness, db, config_db.as_deref(), &env)?;
            analyze(&sess, projects, fmt)
        }
        Command::Usage {
            by,
            period,
            format,
            read,
        } => {
            let opened = dispatch.open(format.as_deref(), &period, &read, run_analyze)?;
            let s = &opened.sess;
            usage(
                &s.store,
                &s.harness,
                by.as_deref(),
                opened.window,
                opened.format,
            )
        }
        Command::Trend { pair, format, read } => {
            let opened = dispatch.open_pair(format.as_deref(), &pair, &read, run_analyze)?;
            let s = &opened.sess;
            trend(&s.store, &s.harness, opened.pair, opened.format)
        }
        Command::Models {
            period,
            format,
            read,
        } => {
            let opened = dispatch.open(format.as_deref(), &period, &read, run_analyze)?;
            let s = &opened.sess;
            views::models::models(&s.store, &s.harness, opened.window, opened.format)
        }
        Command::Sessions {
            sort,
            period,
            format,
            read,
        } => {
            let opened = dispatch.open(format.as_deref(), &period, &read, run_analyze)?;
            let order = views::sessions::parse_sort(sort.as_deref())?;
            let s = &opened.sess;
            views::sessions::sessions(&s.store, &s.harness, order, opened.window, opened.format)
        }
        Command::Inventory {
            scope,
            period,
            format,
            read,
        } => {
            let opened = dispatch.open_scoped(
                scope.as_deref(),
                format.as_deref(),
                &period,
                &read,
                run_analyze,
            )?;
            let (windowed, filter) = opened.into_parts();
            let s = &windowed.sess;
            inventory(
                &s.store,
                &s.harness,
                &filter,
                windowed.window,
                windowed.format,
            )
        }
        Command::Waste {
            scope,
            period,
            format,
            read,
        } => {
            let opened = dispatch.open_scoped(
                scope.as_deref(),
                format.as_deref(),
                &period,
                &read,
                run_analyze,
            )?;
            let (windowed, filter) = opened.into_parts();
            let s = &windowed.sess;
            waste(
                &s.store,
                &s.harness,
                &filter,
                windowed.window,
                windowed.format,
            )
        }
        Command::Overhead {
            format,
            period,
            read,
        } => {
            let opened = dispatch.open(format.as_deref(), &period, &read, run_analyze)?;
            let s = &opened.sess;
            overhead(&s.store, &s.harness, opened.window, opened.format)
        }
        Command::Prompts {
            format,
            period,
            read,
        } => {
            let opened = dispatch.open(format.as_deref(), &period, &read, run_analyze)?;
            let s = &opened.sess;
            views::prompts::prompts(&s.store, &s.harness, opened.window, opened.format)
        }
        Command::Failures {
            scope,
            period,
            format,
            read,
        } => {
            let opened = dispatch.open_scoped(
                scope.as_deref(),
                format.as_deref(),
                &period,
                &read,
                run_analyze,
            )?;
            let (windowed, filter) = opened.into_parts();
            let s = &windowed.sess;
            failures(
                &s.store,
                &s.harness,
                &filter,
                windowed.window,
                windowed.format,
            )
        }
        Command::Stuck {
            format,
            period,
            read,
        } => {
            let opened = dispatch.open(format.as_deref(), &period, &read, run_analyze)?;
            let s = &opened.sess;
            stuck(&s.store, &s.harness, opened.window, opened.format)
        }
        Command::Doctor {
            scope,
            period,
            format,
            read,
        } => {
            let opened = dispatch.open_scoped_table_json(
                scope.as_deref(),
                format.as_deref(),
                &period,
                &read,
                run_analyze,
            )?;
            let (windowed, filter) = opened.into_parts();
            let s = &windowed.sess;
            doctor(
                &s.store,
                &s.harness,
                &filter,
                windowed.window,
                windowed.format,
            )
        }
        Command::Sql {
            query,
            limit,
            format,
            db,
        } => {
            let fmt = parse_format(format.as_deref())?;
            let sess = Session::open_readonly(&harness, db, config_db.as_deref(), &env)?;
            sql(&sess, query.as_deref(), limit, fmt)
        }
        Command::Optimize {
            projects,
            db,
            scope,
            frozen,
            print,
            verbose,
            timeout,
            apply,
        } => {
            // The handoff pre-check runs before any store work: a conflict
            // fails fast, and a non-apply run passes straight through, so the
            // order below keeps its exact behavior without `--apply`.
            handoff::check_apply_args(apply, print, verbose, &harness)?;
            // Resolve (including the legacy-store move) before parsing --scope,
            // then open after — the pre-Session dispatch's exact order, kept so
            // double failures report the same error the old binary did.
            let path = Session::resolve_path(&harness, db, config_db.as_deref(), &env)?;
            let filter = parse_scope(scope.as_deref())?;
            let mut sess = Session::open_resolved(&harness, path)?;
            sess.language = language;
            let timeout = headless::resolve_timeout(timeout.as_deref())?;
            if apply {
                return handoff::run_apply(&sess, projects, &filter, frozen, timeout, &env);
            }
            optimize(&sess, projects, &filter, frozen, print, verbose, timeout)
        }
        Command::Cleanup { list, keep, format } => {
            let fmt = parse_format(format.as_deref())?;
            cleanup::run_cleanup(&env, list, keep, fmt)
        }
        Command::Tui => {
            if !views::guide::terminal_available() {
                anyhow::bail!(
                    "`tui` needs an interactive terminal (stdin and stdout must both be TTYs)"
                ); // No-TTY-only arm: TTY input returns via the pre-parse branch above.
            }
            let entry = views::guide::Entry::preset(cli.harness, cli.profile, cli.language);
            views::guide::run_guide_entry(entry)
        }
    }
}

fn parse_scope(value: Option<&str>) -> Result<ScopeFilter> {
    match value {
        None => Ok(ScopeFilter::All),
        Some(v) => ScopeFilter::parse(v)
            .with_context(|| format!("unknown --scope '{v}' (global | project | project:<slug>)")),
    }
}

///
/// The briefing carries concrete paths and error excerpts that may be sensitive,
/// so it is written to a private (`0600`) temp file and only a pointer is passed
/// on argv — never the data, which `ps` would otherwise expose. The file is
/// removed as soon as the session ends. `--print` instead writes the full prompt
/// (briefing inline) to stdout for piping / inspection.
#[allow(clippy::too_many_arguments)]
fn optimize(
    sess: &Session,
    projects: Option<PathBuf>,
    filter: &ScopeFilter,
    frozen: bool,
    print: bool,
    verbose: bool,
    timeout: Option<std::time::Duration>,
) -> Result<()> {
    if !frozen {
        let stats = run_analyze(&sess.harness, projects, &sess.db_path)?;
        eprintln!(
            "{}",
            Style::stderr().dim(&format!(
                "store: refreshed just now ({} {} re-analyzed, {} unchanged)",
                stats.sessions,
                sess.harness.input_unit(),
                stats.skipped
            ))
        );
    }
    // `optimize` takes the whole history — no period flags reach it.
    let findings = collect_findings(&sess.store, TimeRange::ALL)?;

    // The absolute store path so the session's `saniwalens sql --db …` hits the
    // store this run built, wherever it is launched from.
    let db_display = std::fs::canonicalize(&sess.db_path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| sess.db_path.to_string_lossy().into_owned());

    // The harness copy for every prompt and briefing rendered here — the
    // harness assembles its own (adapter side), so the core stays agnostic.
    // The resolved advisor language rides the same copy, so every prompt
    // builder shares the one wording source (`core::optimize`).
    let mut copy = sess.harness.copy();
    copy.language = sess.language;
    // The launch shape is the harness's own data (`Harness::launch_command`,
    // beside its other arms), so the shell names no harness here.
    let (binary, leading, run_mode) = sess.harness.launch_command();

    // One seam from here: the payload (the full prompt, or the rendered
    // briefing behind a private pointer file) and the mode are decided, then
    // `deliver` runs them against the process adapter — the launch shape and
    // run mode coming from the harness's data arms (`docs/specs/harness.md`).
    let plan = match delivery_mode(print, run_mode) {
        DeliveryMode::Print => DeliveryPlan::print(optimize_mod::compose_prompt(
            &findings,
            &db_display,
            filter,
            &copy,
        )),
        mode => DeliveryPlan::launch(
            (binary, leading),
            mode,
            optimize_mod::render_briefing(&findings, filter, &copy),
            sess.harness.display_name(),
            verbose,
            sess.harness.run_line_renderer(),
            timeout,
            |path| {
                optimize_mod::launch_prompt(
                    &path.to_string_lossy(),
                    &db_display,
                    filter,
                    &copy,
                    run_mode,
                )
            },
        ),
    };
    deliver(plan, &mut ProcessAdvisor)?;
    Ok(())
}

/// Gather the COMPLETE analysis the optimization briefing carries: fetch the
/// store's rows and hand them to the pure assembly in `core::findings`. The
/// shell's half is the queries and the store-row → plain-row mapping; every
/// join, cap, and sort lives in the core where it is testable
/// (`pbi/2026-10-29-04-feat-findings-assembly-core.md`).
pub(crate) fn collect_findings(store: &Store, window: TimeRange) -> Result<optimize_mod::Findings> {
    Ok(findings::collect_findings(&findings_input(store, window)?))
}

/// Every row `core::findings` assembles from, fetched in one place.
fn findings_input(store: &Store, window: TimeRange) -> Result<findings::FindingsInput> {
    Ok(findings::FindingsInput {
        // No session start observed means the always-on section is omitted
        // rather than guessed at — the sentinel stays the core's business.
        floor: store.baseline_floor(window)?.into(),
        error_examples: store.error_examples(findings::EXAMPLES_PER_CATEGORY, window)?,
        error_breakdown: store
            .error_tool_breakdown_briefing(window, findings::BRIEFING_GROUPS_LIMIT)?,
        friction_cells: store
            .error_counts_by_project_briefing(window, findings::BRIEFING_GROUPS_LIMIT)?,
        bash_commands: store.work_counts_briefing(
            "bash_cmd",
            window,
            findings::BRIEFING_GROUPS_LIMIT,
        )?,
        file_edit_counts: store.work_counts_briefing(
            "file_edit",
            window,
            findings::BRIEFING_GROUPS_LIMIT,
        )?,
        file_edits: store.file_edits_briefing(window, findings::BRIEFING_EVENTS_LIMIT)?,
        catalog: store
            .effective_catalog_briefing(window, findings::BRIEFING_GROUPS_LIMIT)?
            .into_iter()
            .map(|e| findings::CatalogRow {
                kind: e.kind,
                id: e.id,
                scope: e.scope,
                project: e.project,
                startup_tokens: e.startup_tokens,
                static_tokens: e.static_tokens,
                load_mode: e.load_mode,
                uses: e.uses,
            })
            .collect(),
        session_roots: store.session_roots_briefing(findings::BRIEFING_GROUPS_LIMIT)?,
        prompt_behaviors: store
            .prompt_behavior_counts_briefing(window, findings::BRIEFING_GROUPS_LIMIT)?,
        subagent_totals: store.subagent_totals(window)?,
        agents: store
            .subagent_by_agent_briefing(window, findings::BRIEFING_GROUPS_LIMIT)?
            .into_iter()
            .map(|row| optimize_mod::AgentCost {
                agent: row.agent,
                runs: row.runs,
                out_tokens: row.out_tokens,
            })
            .collect(),
        skill_out_tokens: vec![store.skill_out_tokens_sum(window)?],
        always_on_config_tokens: store.always_on_config_tokens()?,
    })
}

/// Run a read-only SQL query against the store and print the result. The query
/// comes from the argument or, when absent, from stdin — so both
/// `saniwalens sql '…'` and `echo '…' | saniwalens sql` work. The store is
/// opened read-only so an ad-hoc query can never mutate the derived data.
/// `limit` bounds the materialized rows (`0` disables the bound); a truncated
/// result warns on stderr with the opt-out, so the "any slice" contract stays
/// reachable without silently passing a partial slice off as complete.
/// The SQL text itself is unbounded — it is user-supplied input, not an
/// amplification: only the fetched rows balloon memory, and those are capped.
fn sql(sess: &Session, query: Option<&str>, limit: u64, format: Format) -> Result<()> {
    let query = match query {
        Some(q) => q.to_string(),
        None => {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("read SQL from stdin")?;
            buf
        }
    };
    let query = query.trim();
    if query.is_empty() {
        anyhow::bail!("no SQL provided — pass it as an argument or on stdin");
    }

    let store = &sess.store;
    // `sql` never mutates the store, so it cannot auto-refresh — surface the
    // store's age instead so a stale read is at least visible. An old db whose
    // schema predates `meta` just skips the header.
    if let Ok(analyzed_at) = store.meta("analyzed_at") {
        eprint_freshness(&freshness_line(analyzed_at.as_deref()));
    }
    if format == Format::Json {
        // Typed rows as objects — SQLite integers/reals/NULLs survive as JSON
        // numbers/null. Duplicate column names keep the last value. The
        // payload keeps its bare-array shape; truncation rides stderr.
        let (columns, rows, truncated) = store.query_json_limited(query, nonzero_limit(limit))?;
        let objects: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|row| serde_json::Value::Object(columns.iter().cloned().zip(row).collect()))
            .collect();
        warn_truncated(truncated, limit, objects.len());
        return emit_json(&serde_json::Value::Array(objects));
    }
    let (columns, rows, truncated) = store.query_limited(query, nonzero_limit(limit))?;
    warn_truncated(truncated, limit, rows.len());
    let headers: Vec<&str> = columns.iter().map(String::as_str).collect();
    let aligns = vec![Align::Left; columns.len()];
    render(&headers, &aligns, &rows, format);
    note(&format!("{} row(s)", rows.len()));
    Ok(())
}

/// `0` disables the row bound; anything else caps the materialized rows.
fn nonzero_limit(limit: u64) -> Option<usize> {
    (limit > 0).then_some(limit as usize)
}

/// Name a truncated slice on stderr with the opt-out, so a silent cut never
/// passes as complete. No output when nothing was cut.
fn warn_truncated(truncated: bool, limit: u64, shown: usize) {
    if truncated {
        eprintln!(
            "{}",
            Style::stderr().warn(&format!(
                "warning: result truncated at {shown} row(s) (--limit {limit} in effect; pass --limit 0 for all rows)"
            ))
        );
    }
}

/// A dimmed footnote under a table — context, not data.
fn note(text: &str) {
    println!("\n{}", Style::stdout().dim(text));
}

fn analyze(sess: &Session, projects: Option<PathBuf>, format: Format) -> Result<()> {
    let stats = run_analyze(&sess.harness, projects, &sess.db_path)?;
    let store = &sess.store;
    // `analyze` takes the whole history — no period flags reach it.
    let (sub_tokens, sub_agents) = store.subagent_totals(TimeRange::ALL)?;
    if format == Format::Json {
        return emit_json(&serde_json::json!({
            "sessions": stats.sessions,
            "unchanged": stats.skipped,
            "skill_invocations": stats.spans_total,
            "surfaces": stats.surface_count,
            "subagent_tokens": sub_tokens,
            "subagents": sub_agents,
            "permission_denials": stats.denials,
            "db": sess.db_path.display().to_string(),
        }));
    }
    println!(
        "analyzed {} session(s) ({} unchanged), {} skill invocation(s), \
         {} surface(s) catalogued, \
         {sub_tokens} subagent tokens across {sub_agents} subagent(s), \
         {} permission denial(s) -> {}",
        stats.sessions,
        stats.skipped,
        stats.spans_total,
        stats.surface_count,
        stats.denials,
        sess.db_path.display()
    );
    Ok(())
}

/// Analyze raw input into the store, dispatching on the harness. The guard
/// runs first: a store built by another harness refuses the write rather than
/// mixing rows (`docs/specs/harness.md`).
pub(crate) fn run_analyze(
    harness: &Harness,
    projects: Option<PathBuf>,
    db: &Path,
) -> Result<AnalyzeStats> {
    guard_harness(
        &Store::open(db, OpenMode::ReadWrite).context("open store")?,
        harness,
        db,
    )?;
    harness.analyze(projects, db)
}

/// Warn when a report knows a subagent total but not its per-agent split.
///
/// A store written before runs were extracted keeps its session totals, and
/// `--frozen` skips the refresh that would fill the split in — so for that one
/// read the per-agent rows sum to zero against a nonzero total. Reporting the
/// gap is the store's contract; printing the total as if nothing were missing
/// is the substitution it forbids (`docs/specs/storage.md`). It goes to stderr
/// with the other staleness signals, so piped stdout and `--format json` stay
/// clean.
///
/// A *partial* split is not warned about here: it is a property of the data
/// that no refresh can change, so each report labels it in-band beside the
/// rows it qualifies (`SplitCoverage::partial_note`).
///
/// Under a drawn window an empty split is usually the two time axes at work —
/// the total reads the session's stamped start, the rows read each run's own
/// epoch — so the message names the window instead of pointing at a refresh
/// that cannot fill a straddle. Without the flags the refresh hint stays.
fn warn_missing_agent_split(coverage: &SplitCoverage, window: TimeRange) {
    if let Some(message) = missing_split_warning(coverage, window) {
        eprintln!("{}", Style::stderr().warn(&message));
    }
}

/// The missing-split message, if there is one to print: the refresh hint
/// without a window, the window note under one, `None` when the rows cover
/// the total.
fn missing_split_warning(coverage: &SplitCoverage, window: TimeRange) -> Option<String> {
    if !coverage.is_unavailable() {
        return None;
    }
    if window.is_all() {
        Some(
            "store: per-agent subagent split unavailable (store predates it) \
             — run `saniwalens analyze` (or drop --frozen) to fill it in"
                .to_string(),
        )
    } else {
        Some(format!(
            "store: per-agent subagent split unavailable in this window — those \
             {} tokens over {} run(s) fall outside the report window",
            coverage.missing_tokens, coverage.missing_runs
        ))
    }
}

/// Whether a catalog row's `(scope, project)` passes the `--scope` filter.
pub(crate) fn in_scope(filter: &ScopeFilter, scope: &str, project: &str) -> bool {
    if scope == "global" {
        filter.includes_global()
    } else {
        filter.includes_project(project)
    }
}

/// A human-readable scope cell: `global`, or `project:<slug>`.
pub(crate) fn scope_cell(scope: &str, project: &str) -> String {
    if scope == "project" {
        format!("project:{project}")
    } else {
        scope.to_string()
    }
}

/// View-only (waste view): the `saves@start` cell — a description-loaded surface
/// is negligible but measured, and removing it reclaims only its description.
pub(crate) fn savings_cell(savings: StartupSavings, startup_tokens: Option<u64>) -> String {
    match savings {
        StartupSavings::Tokens(n) => format!("{n}/sess"),
        StartupSavings::UnknownSchema => "schema?".to_string(),
        StartupSavings::Declutter => format!("~{}/sess", startup_tokens.unwrap_or(0)),
    }
}

/// View-only (waste view): a suggestion honest about whether removal saves context or
/// only declutters — naming the body cost as an invocation cost so it is not read as a saving.
pub(crate) fn savings_suggestion(
    wedge: Wedge,
    savings: StartupSavings,
    on_demand: Option<u64>,
) -> String {
    let body = match on_demand {
        Some(n) => format!("body is {n} tok when invoked"),
        None => "body is on-demand".to_string(),
    };
    match savings {
        StartupSavings::Tokens(n) => format!("removing saves ~{n} tokens every session"),
        StartupSavings::UnknownSchema => {
            "disable: drops its tool schema from every session (real, unmeasured)".to_string()
        }
        StartupSavings::Declutter => match wedge {
            Wedge::Unused => format!("declutter only: {body}, ~no startup saving"),
            _ => format!("heavy only when invoked ({body}); rarely used"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_flag_prints_the_crate_version() {
        use clap::CommandFactory;
        let err = Cli::command()
            .try_get_matches_from(["saniwalens", "--version"])
            .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
        // The binary reports the build-stamped version (git tag when built
        // from a checkout, the tag-stamped placeholder otherwise), never the
        // raw `0.0.0` placeholder a source build used to print.
        assert_eq!(
            err.to_string(),
            format!("saniwalens {}\n", env!("SANIWALENS_VERSION"))
        );
        assert_ne!(env!("SANIWALENS_VERSION"), "0.0.0");
    }

    fn agent(name: &str, runs: i64, out_tokens: i64) -> optimize_mod::AgentCost {
        optimize_mod::AgentCost {
            agent: Some(name.to_string()),
            runs,
            out_tokens,
        }
    }

    fn findings_with_agents(
        sub_tokens: i64,
        sub_agents: i64,
        agents: Vec<optimize_mod::AgentCost>,
    ) -> optimize_mod::Findings {
        optimize_mod::Findings {
            sub_tokens,
            sub_agents,
            agent_split: SplitCoverage::new(
                sub_tokens,
                sub_agents,
                agents.iter().map(|a| (a.runs, a.out_tokens)),
            ),
            agents,
            ..Default::default()
        }
    }

    #[test]
    fn a_windowed_unavailable_split_names_the_window_not_a_refresh() {
        // A total with no rows behind it under a drawn window is the straddle,
        // not a store that predates the split — `analyze` (or dropping
        // --frozen) cannot fill it, so the message names the window instead.
        let f = findings_with_agents(3_093_466, 375, vec![]);
        let window = TimeRange {
            start: Some(1_790_780_400),
            end: Some(1_791_385_199),
        };
        let warning = missing_split_warning(&f.agent_split, window).unwrap();
        assert!(
            warning.contains("fall outside the report window"),
            "{warning}"
        );
        assert!(!warning.contains("run `saniwalens analyze`"), "{warning}");

        // Without the flags the refresh hint stays, byte for byte.
        assert_eq!(
            missing_split_warning(&f.agent_split, TimeRange::ALL).unwrap(),
            "store: per-agent subagent split unavailable (store predates it) \
             — run `saniwalens analyze` (or drop --frozen) to fill it in"
        );
        // A split the rows cover warns about nothing.
        let complete = findings_with_agents(100, 1, vec![agent("a", 1, 100)]);
        assert_eq!(
            missing_split_warning(&complete.agent_split, TimeRange::ALL),
            None
        );
    }

    #[test]
    fn a_declutter_row_shows_the_description_it_would_reclaim() {
        // The measured startup cost, not the body — deleting an unused skill
        // frees its description, and that is what the column promises.
        assert_eq!(
            savings_cell(StartupSavings::Declutter, Some(7)),
            "~7/sess".to_string()
        );
        assert_eq!(
            savings_cell(StartupSavings::Tokens(922), Some(922)),
            "922/sess".to_string()
        );
    }

    #[test]
    fn an_unused_body_is_suggested_as_an_invocation_cost_not_a_saving() {
        let suggestion = savings_suggestion(Wedge::Unused, StartupSavings::Declutter, Some(1015));
        assert!(suggestion.contains("1015 tok when invoked"));
        assert!(suggestion.contains("declutter only"));
    }

    #[test]
    fn parse_profile_defaults_to_none() {
        // --profile is optional and global, like --sess.harness.
        let cli = Cli::try_parse_from(["saniwalens", "doctor"]).unwrap();
        assert_eq!(cli.profile, None);
        let cli = Cli::try_parse_from(["saniwalens", "--profile", "work", "doctor"]).unwrap();
        assert_eq!(cli.profile.as_deref(), Some("work"));
    }

    // ---- PBI 2026-10-30-01: the period flags ----

    #[test]
    fn the_period_flags_parse_on_every_report_view() {
        // The flags exist on every windowed view (derived from clap's
        // subcommand tree, not listed here) and nothing else.
        for view in windowed_views() {
            let mut argv = vec!["saniwalens", view.as_str()];
            argv.extend(["--since", "2026-10-01", "--until", "2026-10-07"]);
            Cli::try_parse_from(argv).expect("the view takes the period flags");
        }
        // The other commands do not.
        for argv in [
            vec!["saniwalens", "analyze", "--since", "2026-10-01"],
            vec!["saniwalens", "sql", "--last", "7d"],
            vec!["saniwalens", "optimize", "--since", "2026-10-01"],
        ] {
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "the period flags stay on the report views"
            );
        }
    }

    #[test]
    fn last_conflicts_with_the_explicit_dates() {
        let err = Cli::try_parse_from([
            "saniwalens",
            "usage",
            "--last",
            "7d",
            "--since",
            "2026-10-01",
        ])
        .err()
        .expect("the flags are rejected");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
        let err = Cli::try_parse_from([
            "saniwalens",
            "waste",
            "--last",
            "7d",
            "--until",
            "2026-10-07",
        ])
        .err()
        .expect("the flags are rejected");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
        // The trend's pair resolver rejects the combination too; the parser
        // rejects it first — the same `conflicts_with_all` wiring as the
        // single-window views.
        let err = Cli::try_parse_from([
            "saniwalens",
            "trend",
            "--last",
            "7d",
            "--since",
            "2026-10-01",
        ])
        .err()
        .expect("the flags are rejected");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
        let err = Cli::try_parse_from([
            "saniwalens",
            "trend",
            "--last",
            "7d",
            "--until",
            "2026-10-07",
        ])
        .err()
        .expect("the flags are rejected");
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
        // Alone, `--last` parses.
        let cli = Cli::try_parse_from(["saniwalens", "usage", "--last", "7d"]).unwrap();
        let Command::Usage {
            by,
            period,
            format,
            read,
        } = cli.command
        else {
            panic!("the command is `usage`");
        };
        assert_eq!(by, None);
        assert_eq!(period.since, None);
        assert_eq!(period.until, None);
        assert_eq!(period.last.as_deref(), Some("7d"));
        assert!(format.as_deref().is_none());
        assert!(!read.frozen);
        assert_eq!(read.db, None);
    }
}
