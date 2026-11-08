//! CLI: the `analyze` and `report` commands. This is the thin shell that walks
//! files and renders tables; the analysis itself lives in the pure core. See
//! `docs/specs/cli.md`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::adapter::harness::{Harness, RunLineRenderer, home_dir};
use crate::core::bucket::{Bucket, JST_OFFSET_SECS, bucket_label};
use crate::core::findings;
use crate::core::friction::ErrorCategory;
use crate::core::optimize as optimize_mod;
use crate::core::period::TimeRange;
use crate::core::scope::{ScopeFilter, split_friction};
use crate::core::subagent::SplitCoverage;
use crate::core::surface::{StartupSavings, Wedge, is_usage_measurable};
use crate::core::thrash::detect_thrash;
use crate::session::{
    AnalyzeStats, Session, SessionEnv, Style, env_roots, eprint_freshness, freshness_line,
    guard_harness, resolve_cli,
};
use crate::store::{OpenMode, Store};

pub(crate) mod headless;
pub mod heartbeat;
pub(crate) mod report;
pub(crate) mod views;
pub(crate) mod window;

pub(crate) use report::*;
pub(crate) use views::trend::*;
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
        /// Stop a headless advisor after this long (e.g. 30s, 2m, 500ms).
        #[arg(long, value_name = "DURATION")]
        timeout: Option<String>,
    },
    /// Step-by-step guide to the typical patterns (doctor / waste / failures).
    /// Needs an interactive terminal; without one it refuses instead of hanging.
    Tui,
}

pub fn run() -> Result<()> {
    let argv: Vec<String> = std::env::args().collect();
    if let views::guide::Entry::Guide { harness, profile } = views::guide::guide_entry(&argv)
        && views::guide::terminal_available()
    {
        return views::guide::run_guide_entry(harness.as_deref(), profile.as_deref());
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
    let resolved = resolve_cli(cli.harness.as_deref(), cli.profile.as_deref(), &env)?;
    let harness = resolved.harness;
    let config_db = resolved.config_db;
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
            usage(&opened.sess, by.as_deref(), opened.window, opened.format)
        }
        Command::Trend { pair, format, read } => {
            let opened = dispatch.open_pair(format.as_deref(), &pair, &read, run_analyze)?;
            trend(&opened.sess, opened.pair, opened.format)
        }
        Command::Models {
            period,
            format,
            read,
        } => {
            let opened = dispatch.open(format.as_deref(), &period, &read, run_analyze)?;
            views::models::models(&opened.sess, opened.window, opened.format)
        }
        Command::Sessions {
            sort,
            period,
            format,
            read,
        } => {
            let opened = dispatch.open(format.as_deref(), &period, &read, run_analyze)?;
            let order = views::sessions::parse_sort(sort.as_deref())?;
            views::sessions::sessions(&opened.sess, order, opened.window, opened.format)
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
            inventory(&windowed.sess, &filter, windowed.window, windowed.format)
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
            views::waste::waste(&windowed.sess, &filter, windowed.window, windowed.format)
        }
        Command::Overhead {
            format,
            period,
            read,
        } => {
            let opened = dispatch.open(format.as_deref(), &period, &read, run_analyze)?;
            overhead(&opened.sess, opened.window, opened.format)
        }
        Command::Prompts {
            format,
            period,
            read,
        } => {
            let opened = dispatch.open(format.as_deref(), &period, &read, run_analyze)?;
            views::prompts::prompts(&opened.sess, opened.window, opened.format)
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
            failures(&windowed.sess, &filter, windowed.window, windowed.format)
        }
        Command::Stuck {
            format,
            period,
            read,
        } => {
            let opened = dispatch.open(format.as_deref(), &period, &read, run_analyze)?;
            stuck(&opened.sess, opened.window, opened.format)
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
            doctor(&windowed.sess, &filter, windowed.window, windowed.format)
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
        } => {
            // Resolve (including the legacy-store move) before parsing --scope,
            // then open after — the pre-Session dispatch's exact order, kept so
            // double failures report the same error the old binary did.
            let path = Session::resolve_path(&harness, db, config_db.as_deref(), &env)?;
            let filter = parse_scope(scope.as_deref())?;
            let sess = Session::open_resolved(&harness, path)?;
            let timeout = headless::resolve_timeout(timeout.as_deref())?;
            optimize(&sess, projects, &filter, frozen, print, verbose, timeout)
        }
        Command::Tui => {
            if !views::guide::terminal_available() {
                anyhow::bail!(
                    "`tui` needs an interactive terminal (stdin and stdout must both be TTYs)"
                ); // No-TTY-only arm: TTY input returns via the pre-parse branch above.
            }
            views::guide::run_guide_entry(cli.harness.as_deref(), cli.profile.as_deref())
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

/// How the briefing reaches the advisor — the three deliveries `optimize`
/// picks between (`docs/specs/cli.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryMode {
    Interactive,
    Headless,
    Print,
}

/// `--print` wins over the harness's launch shape.
fn delivery_mode(print: bool, launch: optimize_mod::RunMode) -> DeliveryMode {
    if print {
        DeliveryMode::Print
    } else {
        match launch {
            optimize_mod::RunMode::Interactive => DeliveryMode::Interactive,
            optimize_mod::RunMode::Headless => DeliveryMode::Headless,
        }
    }
}

/// What gets delivered: the full prompt itself (`--print`), or the rendered
/// briefing behind a private pointer file whose argv prompt `pointer` builds
/// once the file's path exists.
enum DeliveryPayload<'a> {
    Full(String),
    Launch {
        briefing: String,
        pointer: Box<dyn FnOnce(&Path) -> String + 'a>,
    },
}

/// One delivery, assembled: mode, launch shape, the headless renderer the
/// harness seam chose, payload. Built by `optimize`, run by `deliver`
/// (`pbi/2026-10-29-06-feat-optimize-delivery-seam.md`).
pub struct DeliveryPlan<'a> {
    mode: DeliveryMode,
    binary: &'static str,
    leading: &'static [&'static str],
    harness_name: &'static str,
    verbose: bool,
    render: Option<RunLineRenderer>,
    timeout: Option<std::time::Duration>,
    payload: DeliveryPayload<'a>,
}

impl<'a> DeliveryPlan<'a> {
    /// `--print`: the prompt itself, destined for stdout — no process, no
    /// temp file (`docs/specs/cli.md`: the exception that opts the user into
    /// seeing the full prompt).
    pub fn print(prompt: String) -> Self {
        Self {
            mode: DeliveryMode::Print,
            binary: "",
            leading: &[],
            harness_name: "",
            verbose: false,
            render: None,
            timeout: None,
            payload: DeliveryPayload::Full(prompt),
        }
    }

    /// A launch: the briefing travels behind a private pointer file, and
    /// `pointer` assembles the argv prompt for that file's path. The launch
    /// shape — the binary plus its leading argv — travels as the one fact the
    /// harness seam hands over (`Harness::launch_command`).
    #[allow(clippy::too_many_arguments)]
    pub fn launch(
        (binary, leading): (&'static str, &'static [&'static str]),
        mode: DeliveryMode,
        briefing: String,
        harness_name: &'static str,
        verbose: bool,
        render: Option<RunLineRenderer>,
        timeout: Option<std::time::Duration>,
        pointer: impl FnOnce(&Path) -> String + 'a,
    ) -> Self {
        Self {
            mode,
            binary,
            leading,
            harness_name,
            verbose,
            render,
            timeout,
            payload: DeliveryPayload::Launch {
                briefing,
                pointer: Box::new(pointer),
            },
        }
    }
}

/// The process side of a launch — the adapter `deliver` runs through, so a
/// test substitutes a double and asserts the argv instead of spawning.
pub trait AdvisorLaunch {
    /// Spawn with stdio inherited: the interactive TUI, and `--verbose`'s raw
    /// streaming for a headless harness.
    fn direct(
        &mut self,
        binary: &str,
        leading: &[&str],
        prompt: &str,
    ) -> Result<std::process::ExitStatus>;

    /// Spawn headless: the raw stream goes to a private log, the agent's text
    /// to stdout, rendered line by line through the `render` the harness seam
    /// chose. Returns the exit status and the log path.
    fn headless(
        &mut self,
        binary: &str,
        leading: &[&str],
        prompt: &str,
        render: Option<RunLineRenderer>,
        timeout: Option<std::time::Duration>,
    ) -> Result<(std::process::ExitStatus, PathBuf)>;
}

/// The real process side: an inherited-stdio spawn, or `run_headless`'s
/// logged pipe.
struct ProcessAdvisor;

impl AdvisorLaunch for ProcessAdvisor {
    fn direct(
        &mut self,
        binary: &str,
        leading: &[&str],
        prompt: &str,
    ) -> Result<std::process::ExitStatus> {
        Ok(std::process::Command::new(binary)
            .args(leading)
            .arg(prompt)
            .status()?)
    }

    fn headless(
        &mut self,
        binary: &str,
        leading: &[&str],
        prompt: &str,
        render: Option<RunLineRenderer>,
        timeout: Option<std::time::Duration>,
    ) -> Result<(std::process::ExitStatus, PathBuf)> {
        headless::run_headless(
            binary,
            leading,
            prompt,
            render,
            headless::Beat::default(),
            timeout,
            headless::keep_run_log(),
        )
    }
}

/// Ownership of the briefing pointer file: removing it is a scope fact, not
/// a last line, so every exit — error return, failure bail, or panic unwind —
/// deletes the secret file. Modeled on `HeadlessRun` (`headless.rs`), which
/// owns the run log and the child the same way.
struct BriefingFile {
    path: Option<PathBuf>,
}

impl BriefingFile {
    /// Take ownership of the pointer file at `path`.
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    /// The path the argv pointer closes over. The guard must outlive the
    /// advisor run: dropping it first would pull the file while the advisor
    /// still needs it.
    fn path(&self) -> &Path {
        self.path.as_ref().expect("the briefing file is owned")
    }
}

impl Drop for BriefingFile {
    fn drop(&mut self) {
        // Best-effort like the run-log removal (`headless.rs`): the run is
        // over, so a removal failure warns instead of failing the delivery —
        // but unlike the old `let _ = remove_file`, it is never swallowed.
        if let Some(path) = self.path.take()
            && let Err(err) = fs::remove_file(&path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "{}",
                Style::stderr().warn(&format!(
                    "warning: could not remove briefing temp file {} ({err})",
                    path.display()
                ))
            );
        }
    }
}

/// Deliver `plan` — the one seam between the briefing and the advisor.
/// Launch modes write the briefing to a private (`0600`) pointer file owned
/// by a `BriefingFile` guard, hand the spawn to the `advisor` adapter, and
/// drop the guard when the run ends whatever the outcome; `Print` writes the
/// prompt to stdout. The briefing itself never rides argv — a process
/// argument is world-visible (`ps`) — and only a data-free pointer does.
pub fn deliver(plan: DeliveryPlan<'_>, advisor: &mut impl AdvisorLaunch) -> Result<()> {
    let DeliveryPlan {
        mode,
        binary,
        leading,
        harness_name,
        verbose,
        render,
        timeout,
        payload,
    } = plan;
    let (prompt, _briefing) = match payload {
        DeliveryPayload::Full(prompt) => {
            println!("{prompt}");
            return Ok(());
        }
        DeliveryPayload::Launch { briefing, pointer } => {
            let path = write_private_tempfile(&briefing).context("write briefing temp file")?;
            let briefing = BriefingFile::new(path);
            let prompt = pointer(briefing.path());
            (prompt, briefing)
        }
    };

    let launch_note = format!(
        "launch `{binary} {}` — is {harness_name} installed and on PATH?",
        leading.join(" ").trim_end()
    );
    // Headless harnesses run piped unless --verbose: the raw stream is logged
    // to a private file; text streams on stdout, verbose keeps the raw stream.
    // A success removes the run log (`SANIWALENS_KEEP_RUN_LOG=1` keeps it) —
    // only a failure leaves it behind for the bail below to name.
    let (result, run_log) = if mode == DeliveryMode::Headless && !verbose {
        match advisor.headless(binary, leading, &prompt, render, timeout) {
            Ok((status, log)) => (Ok(status), Some(log)),
            Err(err) => (Err(err), None),
        }
    } else {
        (advisor.direct(binary, leading, &prompt), None)
    };

    // Headless errors already name the raw log and, for a spawn failure, the
    // launch hint; the interactive path keeps the launch hint as its context.
    // The briefing guard drops here, after the run: every exit above,
    // including an early return or a panic unwind, removes the file.
    let status = match (mode, verbose) {
        (DeliveryMode::Headless, false) => result?,
        _ => result.with_context(move || launch_note)?,
    };
    if !status.success() {
        match run_log {
            Some(log) => anyhow::bail!(
                "{binary} exited with {status} — raw output: {}",
                log.display()
            ),
            None => anyhow::bail!("{binary} exited with {status}"),
        }
    }
    Ok(())
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
    let copy = sess.harness.copy();
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

/// A fresh private temp path for `prefix`, unique per process and call.
pub(crate) fn private_temp_path(prefix: &str, ext: &str) -> PathBuf {
    // The clock is here to avoid a collision with a leftover file, not to be
    // unguessable — exclusive creation is what makes guessing the name useless.
    // The sequence is what makes two calls unique: parallel tests share the
    // pid and can land on the same clock tick, and without it the loser fails
    // with `AlreadyExists`.
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{nanos:x}-{seq}.{ext}",
        std::process::id()
    ))
}

/// Write `contents` to a fresh file in the temp dir. Used for the optimization
/// briefing, which may hold sensitive paths/excerpts and must not sit on argv or
/// be world-readable.
fn write_private_tempfile(contents: &str) -> Result<PathBuf> {
    let path = private_temp_path("saniwalens-briefing", "md");
    write_private_file(&path, contents)?;
    Ok(path)
}

/// Create `path` and write `contents` to it, for the current user only.
///
/// The temp dir is shared on unix, so the file has to be **created
/// exclusively**: a same-user process that pre-creates the path — as a symlink
/// to somewhere it can read — would otherwise capture the briefing, and no
/// permission bits set afterwards would help. Losing that race now fails the
/// open instead of following what is already there.
///
/// `0600` narrows it further on unix. Windows has no mode to set here; the
/// default per-user `%LOCALAPPDATA%\Temp` is what keeps other unprivileged
/// users out, so the guarantee there rests on the directory rather than the
/// file. Setting an explicit ACL would mean a new dependency for a second lock
/// on a door that is already locked.
pub(crate) fn write_private_file(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write;

    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)?.write_all(contents.as_bytes())?;
    Ok(())
}

/// Gather the COMPLETE analysis the optimization briefing carries: fetch the
/// store's rows and hand them to the pure assembly in `core::findings`. The
/// shell's half is the queries and the store-row → plain-row mapping; every
/// join, cap, and sort lives in the core where it is testable
/// (`pbi/2026-10-29-04-feat-findings-assembly-core.md`).
fn collect_findings(store: &Store, window: TimeRange) -> Result<optimize_mod::Findings> {
    Ok(findings::collect_findings(&findings_input(store, window)?))
}

/// Every row `core::findings` assembles from, fetched in one place.
fn findings_input(store: &Store, window: TimeRange) -> Result<findings::FindingsInput> {
    Ok(findings::FindingsInput {
        // No session start observed means the always-on section is omitted
        // rather than guessed at — the sentinel stays the core's business.
        floor: store.baseline_floor(window)?,
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

/// One-screen health check: the few most actionable findings from every view,
/// split by the config layer that owns each fix — the global picture first,
/// then each project's own findings ("optimize my global setup" and "optimize
/// this project" are different tasks). `--scope` narrows to one layer.
fn doctor(sess: &Session, filter: &ScopeFilter, window: TimeRange, format: Format) -> Result<()> {
    let store = &sess.store;
    eprint_window(window);
    let f = collect_findings(store, window)?;

    if format == Format::Json {
        // The routed findings, filtered the same way the text form is: the
        // global picture (minus the projects list) and each visible project.
        let global = filter.includes_global().then(|| {
            let mut value = serde_json::to_value(&f).expect("findings serialize");
            value.as_object_mut().expect("object").remove("projects");
            value
        });
        let projects: Vec<&optimize_mod::ProjectFindings> = f
            .projects
            .iter()
            .filter(|p| filter.includes_project(&p.project))
            .collect();
        return emit_json(&with_window(
            serde_json::json!({
                "global": global,
                "projects": projects,
            }),
            window,
        ));
    }

    // The text form is written for someone who has never seen saniwalens's
    // vocabulary: every item says what happened, why it costs, and what to do
    // — prioritized, so the report reads top to bottom as a to-do list.
    let style = Style::stdout();
    let (session_count, project_count) = store.session_stats(window)?;
    if session_count == 0 {
        println!(
            "{}",
            empty_in_window(
                "nothing analyzed yet — run `saniwalens analyze` first",
                window
            )
        );
        return Ok(());
    }
    println!(
        "{}",
        style.heading(&format!(
            "{} usage health check — {session_count} sessions across \
             {project_count} projects",
            sess.harness.display_name()
        ))
    );
    // The evaluation window, in-band for a text reader: the session count and
    // every section below are already window-filtered.
    if let Some(line) = window_line(window) {
        println!("{}", style.dim(&line));
    }

    let visible: Vec<_> = f
        .projects
        .iter()
        .filter(|p| filter.includes_project(&p.project))
        .collect();
    let display = |p: &optimize_mod::ProjectFindings| {
        if p.root.is_empty() {
            p.project.clone()
        } else {
            tilde_path(&p.root)
        }
    };

    // ---- WHAT TO FIX FIRST: recurring problems, biggest first. -------------
    struct Item {
        weight: i64,
        lines: Vec<String>,
    }
    let mut items: Vec<Item> = Vec::new();
    for p in &visible {
        if p.total_friction() == 0 {
            continue;
        }
        let mut lines = vec![format!(
            "In {}, {} tool calls failed in recurring ways:",
            style.bold(&display(p)),
            style.warn(&p.total_friction().to_string())
        )];
        for cat in p.friction.iter().take(3) {
            lines.push(format!(
                "  {:>4}× {} {}",
                cat.count,
                cat.label,
                style.dim(&format!(
                    "— {}",
                    ErrorCategory::from_label(&cat.label)
                        .suggestion(sess.harness.instruction_file())
                ))
            ));
        }
        if let Some(worst) = p.thrash.first() {
            lines.push(format!(
                "  …and the agent got stuck re-editing {} ({} edits in {}m).",
                worst.file,
                worst.edits,
                worst.span_secs.div_euclid(60).max(1)
            ));
        }
        lines.push(format!(
            "  Fix in this project's own {} — details: {}",
            sess.harness.project_config_ref(),
            style.command(&format!(
                "saniwalens failures --scope project:{}",
                p.project
            ))
        ));
        items.push(Item {
            weight: p.total_friction(),
            lines,
        });
    }
    if filter.includes_global() {
        for cat in &f.friction_global {
            items.push(Item {
                weight: cat.count,
                lines: vec![
                    format!(
                        "The same failure hit {} different projects ({}× total): {} {}",
                        cat.projects,
                        style.warn(&cat.count.to_string()),
                        style.bold(&cat.label),
                        style.dim(&format!(
                            "— {}",
                            ErrorCategory::from_label(&cat.label)
                                .suggestion(sess.harness.instruction_file())
                        ))
                    ),
                    format!(
                        "  No single project owns it, so the fix belongs in {} — \
                         details: {}",
                        sess.harness.global_config_ref(),
                        style.command("saniwalens failures --scope global")
                    ),
                ],
            });
        }
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.weight));
    let dropped_items = items.len().saturating_sub(4);
    items.truncate(4);
    if filter.includes_global() {
        if let Some(cd_pct) = f.cd_pct.filter(|pct| *pct >= 25) {
            items.push(Item {
                weight: 0,
                lines: vec![format!(
                    "{} of all Bash commands are just `cd` — a working-directory \
                     convention in your global {} would cut the churn.",
                    style.warn(&format!("{cd_pct}%")),
                    sess.harness.instruction_file()
                )],
            });
        }
        if let (Some(steer), Some(correct)) = (f.steer_pct, f.correct_pct)
            && (correct >= 10 || steer >= 25)
        {
            items.push(Item {
                weight: 0,
                lines: vec![format!(
                    "{steer}% of your prompts steer and {correct}% correct — rework that \
                     clearer upfront specs could cut. Details: {}",
                    style.command("saniwalens prompts")
                )],
            });
        }
    }

    println!("\n{}", style.heading("WHAT TO FIX FIRST"));
    if items.is_empty() {
        println!("  {}", style.good("nothing recurring found — looking good"));
    }
    for (i, item) in items.iter().enumerate() {
        let (first, rest) = item.lines.split_first().expect("item has lines");
        println!("  {}. {first}", i + 1);
        for line in rest {
            println!("   {line}");
        }
    }
    // Never truncate silently (reporting honesty, cli.md).
    if dropped_items > 0 {
        println!(
            "  {}",
            style.dim(&format!(
                "… and {dropped_items} smaller recurring problem(s) — see `saniwalens failures --scope project`"
            ))
        );
    }

    // ---- COST: what a session pays, in plain words. -------------------------
    if filter.includes_global() {
        println!("\n{}", style.heading("COST (token estimates, not billing)"));
        if f.floor > 0 {
            let residual = (f.floor - f.config_tokens).max(0);
            println!(
                "  Every session starts with ~{} tokens of context before any work:",
                style.bold(&fmt_tokens(f.floor))
            );
            if residual > 0 {
                println!(
                    "  {} comes from your own global config files; the other {} is {}",
                    fmt_tokens(f.config_tokens),
                    fmt_tokens(residual),
                    sess.harness.display_name()
                );
                println!(
                    "  {}",
                    style.dim(
                        "itself (system prompt, built-in tools, MCP schemas) plus each \
                         project's own config (AGENTS.md, skill descriptions) — `saniwalens \
                         overhead` splits it per project."
                    )
                );
            } else {
                // The global config estimate already covers the observed floor —
                // attributing a "0" to the harness would read as a claim.
                println!(
                    "  ~all of it is accounted for by your own config files (skill \
                     descriptions, AGENTS.md) — nothing is left over to attribute; \
                     `saniwalens overhead` splits it per project."
                );
            }
        }
        println!(
            "  Output written so far: {} tokens by skills on the main thread, {} by {} subagent runs.",
            fmt_tokens(f.main_out),
            fmt_tokens(f.sub_tokens),
            f.sub_agents
        );
        warn_missing_agent_split(&f.agent_split, window);
        for line in doctor_agent_lines(&f, &style.command("saniwalens usage"), window) {
            if line.starts_with('…') {
                println!("  {}", style.dim(&line));
            } else {
                println!("  {line}");
            }
        }
    }

    // ---- CONFIG WORTH PRUNING: installed-but-dead weight, by owner. ---------
    let mut pruning: Vec<String> = Vec::new();
    if filter.includes_global() && (!f.unused.is_empty() || !f.always_on_heavy.is_empty()) {
        pruning.push(format!(
            "  {}: {} → {}",
            sess.harness.global_config_ref(),
            prune_phrase(&f.unused, &f.always_on_heavy),
            style.command("saniwalens waste --scope global")
        ));
    }
    let mut prunable: Vec<_> = visible
        .iter()
        .filter(|p| !p.unused.is_empty() || !p.always_on_heavy.is_empty())
        .collect();
    prunable.sort_by_key(|p| std::cmp::Reverse(p.unused.len() + p.always_on_heavy.len()));
    for p in prunable.iter().take(3) {
        pruning.push(format!(
            "  {}: {} → {}",
            display(p),
            prune_phrase(&p.unused, &p.always_on_heavy),
            style.command(&format!("saniwalens waste --scope project:{}", p.project))
        ));
    }
    if !pruning.is_empty() {
        println!("\n{}", style.heading("CONFIG WORTH PRUNING"));
        println!(
            "  {}",
            style.dim("Installed config that costs attention (or context) without earning it:")
        );
        for line in pruning {
            println!("{line}");
        }
        if prunable.len() > 3 {
            println!(
                "  {}",
                style.dim(&format!(
                    "… and {} more project(s) with prunable config — see `saniwalens waste --scope project`",
                    prunable.len() - 3
                ))
            );
        }
    }

    // ---- LOOKS HEALTHY: say explicitly what needs no action. ----------------
    if filter.includes_global() {
        let mut healthy: Vec<String> = Vec::new();
        if let (Some(steer), Some(correct)) = (f.steer_pct, f.correct_pct)
            && correct < 10
            && steer < 25
        {
            healthy.push(format!(
                "prompting mix ({steer}% steering / {correct}% corrections) — good autonomy balance"
            ));
        }
        if f.friction_global.is_empty() {
            healthy.push("no failure habit spans multiple projects".to_string());
        }
        if f.always_on_heavy.is_empty() && f.floor > 0 {
            healthy.push(format!(
                "your global config is lean ({} of the ~{} startup tokens)",
                fmt_tokens(f.config_tokens),
                fmt_tokens(f.floor)
            ));
        }
        if !healthy.is_empty() {
            println!("\n{}", style.heading("LOOKS HEALTHY"));
            for line in healthy {
                println!("  {}", style.good(&line));
            }
        }
    }
    Ok(())
}

/// View-only (doctor view): `"3 unused surfaces, 4 always-on files costing ~8.5k
/// tokens/session"` — the pruning line's plain-language phrase, zero parts
/// dropped; it moves with the doctor view, not with the shell.
fn prune_phrase(
    unused: &[optimize_mod::SurfaceRef],
    always_on_heavy: &[optimize_mod::SurfaceRef],
) -> String {
    let mut parts = Vec::new();
    if !unused.is_empty() {
        parts.push(format!(
            "{} installed but never used",
            plural(unused.len(), "surface")
        ));
    }
    if !always_on_heavy.is_empty() {
        let tokens: i64 = always_on_heavy
            .iter()
            .filter_map(|s| s.startup_tokens)
            .sum();
        parts.push(format!(
            "{} loaded every session (~{} tokens each time)",
            plural(always_on_heavy.len(), "heavy file"),
            fmt_tokens(tokens)
        ));
    }
    parts.join(", ")
}

/// View-only (doctor view): the singular/plural phrasing behind `prune_phrase` alone.
fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// The doctor's lines naming the agents behind the subagent figure — "which
/// agent" is the actionable half of it. The top rows are introduced as "most
/// of" the output only when they actually exceed half of it; when the rows
/// are a partial split (`SplitCoverage`), the gap is stated first so the
/// names are not read as the whole bill. `full_split` is the styled command
/// that shows every row. An unavailable split yields nothing: that case is
/// reported on stderr with its refresh hint. Under a drawn window the gap is
/// the two time axes at work, so the sentence names the window instead of the
/// pruned transcripts (`window.is_all()` keeps the full-history wording).
fn doctor_agent_lines(
    f: &optimize_mod::Findings,
    full_split: &str,
    window: TimeRange,
) -> Vec<String> {
    let mut lines = Vec::new();
    let top: Vec<String> = f
        .agents
        .iter()
        .take(3)
        .map(|a| {
            format!(
                "{} {} ({} run{})",
                a.label(),
                fmt_tokens(a.out_tokens),
                a.runs,
                if a.runs == 1 { "" } else { "s" }
            )
        })
        .collect();
    if top.is_empty() {
        return lines;
    }
    let coverage = &f.agent_split;
    let shown: i64 = f.agents.iter().take(top.len()).map(|a| a.out_tokens).sum();
    let intro = if coverage.is_partial() {
        if window.is_all() {
            lines.push(format!(
                "Only {} of those tokens ({} of {} runs) have a per-agent split — the rest were \
                 analyzed before runs were extracted and their transcripts are gone.",
                fmt_tokens(coverage.known_tokens),
                coverage.known_runs,
                coverage.total_runs
            ));
        } else {
            lines.push(format!(
                "Only {} of those tokens ({} of {} runs) have a per-agent split — the rest \
                 fall outside the report window.",
                fmt_tokens(coverage.known_tokens),
                coverage.known_runs,
                coverage.total_runs
            ));
        }
        "Among the runs that have one"
    } else if coverage.is_majority(shown) {
        "Most of that subagent output"
    } else {
        "The largest agent types"
    };
    lines.push(format!(
        "{intro}: {}. Full split: {full_split}",
        top.join(", ")
    ));
    // Never truncate silently (reporting honesty, cli.md).
    if f.agents.len() > top.len() {
        lines.push(format!(
            "… and {} more agent type(s) not shown",
            f.agents.len() - top.len()
        ));
    }
    lines
}

/// View-only (doctor view): humanize a token count: `6467406` → `6.5M`, `32139` → `32.1k`.
fn fmt_tokens(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

/// View-only (doctor view): shorten an absolute path under the home directory to `~/…` for display.
fn tilde_path(path: &str) -> String {
    shorten_to_tilde(path, home_dir())
}

/// View-only (doctor view, via `tilde_path`): the shortening itself, separated from
/// the environment lookup so it stays pure and testable. A separator must follow the
/// prefix, so `/home/metadata` is not mistaken for something under `/home/me`; either
/// separator counts, so a Windows path shortens against a Windows home.
fn shorten_to_tilde(path: &str, home: Option<&str>) -> String {
    let Some(home) = home.filter(|home| !home.is_empty()) else {
        return path.to_string();
    };
    // The two sides come from different places — `home` from the environment,
    // `path` from the cwd a transcript recorded — so on Windows they can spell
    // the same directory differently; the OS accepts either. Only the
    // comparison is folded. The suffix is sliced from the original at the same
    // offset, which stays valid because folding swaps one ASCII byte for
    // another, so the display keeps whatever spelling the path really had.
    let fold = |s: &str| s.replace('\\', "/");
    match fold(path).strip_prefix(&fold(home)) {
        Some("") => "~".to_string(),
        Some(rest) if rest.starts_with('/') => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

/// Thrash bursts: a file edited many times in a short window — where the agent
/// got stuck and kept retrying, as opposed to a hotspot's healthy spread-out
/// edits. An episode is one session's edits to one path, so parallel work on
/// a same-named file is never presented as one agent looping (`core::thrash`).
fn stuck(sess: &Session, window: TimeRange, format: Format) -> Result<()> {
    const GAP_SECS: i64 = 5 * 60;
    const MIN_EDITS: u32 = 4;
    let store = &sess.store;
    eprint_window(window);
    let episodes = detect_thrash(&store.file_edits(window)?, GAP_SECS, MIN_EDITS);
    if format == Format::Json {
        let episodes: Vec<serde_json::Value> = episodes
            .iter()
            .map(|e| {
                serde_json::json!({
                    "project": e.project,
                    "session_id": e.session_id,
                    "file": e.file(),
                    "path": e.path,
                    "edits": e.edits,
                    "span_secs": e.span_secs(),
                })
            })
            .collect();
        return emit_json(&with_window(
            serde_json::json!({ "episodes": episodes }),
            window,
        ));
    }
    if episodes.is_empty() {
        println!(
            "{}",
            empty_in_window(
                &format!("no thrash episodes (>= {MIN_EDITS} rapid re-edits) — run analyze first"),
                window
            )
        );
        return Ok(());
    }
    preamble(
        format,
        &framed(
            "Files the agent kept re-editing in rapid bursts — the signature of getting stuck\n\
             and retrying. Worth checking what blocked it there (a rule? missing context?).",
            window,
        ),
    );
    let rows: Vec<Vec<String>> = episodes
        .iter()
        .take(20)
        .map(|e| {
            let span = e.span_secs();
            vec![
                e.project.clone(),
                e.file().to_string(),
                e.edits.to_string(),
                format!("{}m{}s", span / 60, span % 60),
            ]
        })
        .collect();
    render(
        &["project", "file", "edits", "within"],
        &[Align::Left, Align::Left, Align::Right, Align::Right],
        &rows,
        format,
    );
    note(&format!(
        "bursts of >= {MIN_EDITS} edits to one file within {}m by a single session —\n\
         likely where the agent got stuck. `--format json` carries the session id and\n\
         full path to investigate with.",
        GAP_SECS / 60
    ));
    Ok(())
}

/// A dimmed footnote under a table — context, not data.
fn note(text: &str) {
    println!("\n{}", Style::stdout().dim(text));
}

/// Where the work stumbles: recurring tool failures by category, ranked, each
/// with what it suggests fixing. This is about the work, not the config —
/// recurring failures are fixable friction that wastes turns and tokens.
/// `--scope` routes by ownership (`core::scope`): `global` shows the failures
/// no project owns, `project` each project's majority-owned failures, and
/// `project:<slug>` everything that happened in that one project.
fn failures(sess: &Session, filter: &ScopeFilter, window: TimeRange, format: Format) -> Result<()> {
    struct FrictionRow {
        project: Option<String>,
        category: String,
        count: i64,
        /// How many projects the count spreads across (global scope only).
        spread: Option<usize>,
    }

    let store = &sess.store;
    eprint_window(window);
    let cells = store.error_counts_by_project(window)?;

    let counts: Vec<FrictionRow> = match filter {
        ScopeFilter::All => store
            .error_counts(window)?
            .into_iter()
            .map(|(category, count)| FrictionRow {
                project: None,
                category,
                count,
                spread: None,
            })
            .collect(),
        ScopeFilter::Global => split_friction(&cells)
            .global
            .into_iter()
            .map(|g| FrictionRow {
                project: None,
                category: g.category,
                count: g.total,
                spread: Some(g.projects),
            })
            .collect(),
        ScopeFilter::Project(None) => split_friction(&cells)
            .per_project
            .into_iter()
            .flat_map(|(project, cats)| {
                cats.into_iter().map(move |(category, count)| FrictionRow {
                    project: Some(project.clone()),
                    category,
                    count,
                    spread: None,
                })
            })
            .collect(),
        ScopeFilter::Project(Some(slug)) => cells
            .into_iter()
            .filter(|(project, _, _)| project == slug)
            .map(|(project, category, count)| FrictionRow {
                project: Some(project),
                category,
                count,
                spread: None,
            })
            .collect(),
    };
    let total: i64 = counts.iter().map(|row| row.count).sum();

    // Unknown stored labels (typos, schema drift) are still listed — nothing
    // is dropped — but their unknown-ness warns instead of silently joining
    // `Other`'s suggestion. Warn-once shape mirrors the waste view.
    let mut unknown_labels: Vec<String> = counts
        .iter()
        .filter(|row| !ErrorCategory::is_known_label(&row.category))
        .map(|row| row.category.clone())
        .collect();
    unknown_labels.sort();
    unknown_labels.dedup();
    if !unknown_labels.is_empty() {
        eprintln!(
            "{}",
            Style::stderr().warn(&format!(
                "warning: {} stored categor{} unknown ({}); shown with the unclassified suggestion",
                unknown_labels.len(),
                if unknown_labels.len() == 1 {
                    "y is"
                } else {
                    "ies are"
                },
                unknown_labels.join(", ")
            ))
        );
    }

    if format == Format::Json {
        let failures: Vec<serde_json::Value> = counts
            .iter()
            .map(|row| {
                serde_json::json!({
                    "project": row.project,
                    "category": row.category,
                    "count": row.count,
                    "spread_projects": row.spread,
                    "suggestion": ErrorCategory::from_label(&row.category)
                        .suggestion(sess.harness.instruction_file()),
                })
            })
            .collect();
        return emit_json(&with_window(
            serde_json::json!({
                "total": total,
                "failures": failures,
                "unknown_labels": {
                    "count": unknown_labels.len(),
                    "labels": unknown_labels,
                },
            }),
            window,
        ));
    }
    if total == 0 {
        println!(
            "{}",
            empty_in_window(
                "no tool failures found for this scope — run `saniwalens analyze` first?",
                window
            )
        );
        return Ok(());
    }

    preamble(
        format,
        &framed(
            &match filter {
                ScopeFilter::All => {
                    "Tool calls that kept failing the same way, grouped by kind of failure.\n\
                 Every row wastes turns and tokens; the suggestion says what usually fixes it."
                        .to_string()
                }
                ScopeFilter::Global => {
                    format!(
                        "Failures spread across several projects — no single project owns them, so\n\
                     they read as habits or global-config problems. Fix these in {}.",
                        sess.harness.global_config_ref()
                    )
                }
                ScopeFilter::Project(None) => {
                    format!(
                        "Failures each project owns (it holds the majority of that kind). Fix each\n\
                     in that project's own {}, not globally.",
                        sess.harness.project_config_ref()
                    )
                }
                ScopeFilter::Project(Some(_)) => {
                    format!(
                        "Every recurring failure inside this one project, most frequent first.\n\
                     Fix them in this project's own {}.",
                        sess.harness.project_config_ref()
                    )
                }
            },
            window,
        ),
    );
    let with_project = matches!(filter, ScopeFilter::Project(None));
    let rows: Vec<Vec<String>> = counts
        .iter()
        .map(|row| {
            let mut cells = Vec::new();
            if with_project {
                cells.push(row.project.clone().unwrap_or_default());
            }
            let error = match row.spread {
                Some(spread) => format!("{} (across {spread} projects)", row.category),
                None => row.category.clone(),
            };
            cells.extend([
                error,
                row.count.to_string(),
                ErrorCategory::from_label(&row.category)
                    .suggestion(sess.harness.instruction_file()),
            ]);
            cells
        })
        .collect();
    let (headers, aligns): (Vec<&str>, Vec<Align>) = if with_project {
        (
            vec!["project", "error", "count", "suggestion"],
            vec![Align::Left, Align::Left, Align::Right, Align::Left],
        )
    } else {
        (
            vec!["error", "count", "suggestion"],
            vec![Align::Left, Align::Right, Align::Left],
        )
    };
    render(&headers, &aligns, &rows, format);
    note(&format!(
        "{total} tool failures (denials from the transcript's own marker where present; \
         everything else is a lexical heuristic)"
    ));
    Ok(())
}

/// Reconcile the empirical always-on floor against readable config. The floor is
/// what every session actually starts with; subtracting the config we can read
/// leaves the residual — system prompt, built-in tools, and MCP tool schemas the
/// catalog cannot weigh. The per-project floors let you infer an MCP server's
/// real cost by comparing a project that enables it against one that does not.
fn overhead(sess: &Session, window: TimeRange, format: Format) -> Result<()> {
    let store = &sess.store;
    eprint_window(window);
    let floor = store.baseline_floor(window)?;
    let config = store.always_on_config_tokens()?;
    let residual = floor.map(|floor| (floor - config).max(0));
    let per_project = store.baseline_floor_per_project(window)?;

    if format == Format::Json {
        let per_project: Vec<serde_json::Value> = per_project
            .iter()
            .map(|(project, floor, sessions)| {
                serde_json::json!({
                    "project": project,
                    "floor": floor,
                    "sessions": sessions,
                })
            })
            .collect();
        return emit_json(&with_window(
            serde_json::json!({
                "floor": floor,
                "config_tokens": config,
                "residual": residual,
                "per_project": per_project,
            }),
            window,
        ));
    }
    // No session start observed — say so rather than substituting a mid-session
    // context that would read as a startup cost.
    let Some(floor) = floor else {
        let (sessions, _) = store.session_stats(window)?;
        if sessions == 0 {
            println!(
                "{}",
                empty_in_window("no data — run `saniwalens analyze` first", window)
            );
        } else {
            println!(
                "always-on floor unavailable — none of the {sessions} analyzed session(s)\n\
                 carries an observable session start. A store written before session-start\n\
                 tracking keeps its old rows (analyze never re-reads a closed session), so\n\
                 the floor appears as newer sessions are analyzed. Discarding the store\n\
                 would backfill it from the {} still on disk, at the cost of every\n\
                 session whose {} {} has already pruned.",
                sess.harness.raw_source(),
                sess.harness.source_noun(),
                sess.harness.display_name()
            );
        }
        return Ok(());
    };
    let residual = (floor - config).max(0);

    preamble(
        format,
        &framed(
            "What every session pays in context before any work starts, and how much of\n\
             it your own config files account for.",
            window,
        ),
    );
    let style = Style::stdout();
    println!(
        "Observed always-on floor (leanest session-start context): {}",
        style.bold(&format!("{floor} tokens"))
    );
    println!(
        "  readable config ({}): {config} tokens",
        sess.harness.readable_config()
    );
    println!(
        "  residual (system prompt + built-in tools + MCP schemas):                {residual} tokens"
    );
    println!(
        "{}",
        style.dim(
            "  -> the residual is what you cannot read from files; compare projects below\n\
             \x20    to see an MCP server's marginal cost (a project that enables it starts higher)."
        )
    );
    println!();

    let rows: Vec<Vec<String>> = per_project
        .into_iter()
        .map(|(project, floor, sessions)| vec![project, floor.to_string(), sessions.to_string()])
        .collect();
    render(
        &["project", "floor_tokens", "sessions"],
        &[Align::Left, Align::Right, Align::Right],
        &rows,
        format,
    );
    note(
        "each floor is the leanest start across that project's sessions — a project with\n\
         few sessions may not have caught a fresh one, so compare like counts.",
    );
    Ok(())
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
fn run_analyze(harness: &Harness, projects: Option<PathBuf>, db: &Path) -> Result<AnalyzeStats> {
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
fn usage(sess: &Session, by: Option<&str>, window: TimeRange, format: Format) -> Result<()> {
    let store = &sess.store;
    eprint_window(window);

    if let Some(by) = by {
        let bucket = Bucket::parse(by)
            .with_context(|| format!("unknown --by value '{by}' (year|month|week|day|hour)"))?;
        return usage_by_time(store, bucket, window, format);
    }

    let skills = store.skill_usage(window)?;
    // Where the tokens actually go: main-thread skill output vs subagents. The
    // subagent figure is usually the larger by far — worth seeing before reading
    // the per-skill table.
    let main_out: i64 = skills.iter().map(|row| row.out_tokens).sum();
    let (sub_tokens, sub_agents) = store.subagent_totals(window)?;
    let agents = store.subagent_by_agent(window)?;
    // How much of the total the rows actually account for: a store that kept
    // totals for sessions whose transcripts were pruned before their runs were
    // extracted has rows for only part of it, and the rows must not pass for
    // the whole. It travels with the rows in every format.
    let agent_split = SplitCoverage::new(
        sub_tokens,
        sub_agents,
        agents.iter().map(|a| (a.runs, a.out_tokens)),
    );
    // Ahead of the format branches: a machine consumer reading an empty `agents`
    // beside a nonzero total needs the reason as much as a human does, and it
    // goes to stderr precisely so it can be given to both.
    warn_missing_agent_split(&agent_split, window);
    if format == Format::Json {
        return emit_json(&with_window(
            serde_json::json!({
                "tokens": {
                    "main_out": main_out,
                    "sub_tokens": sub_tokens,
                    "sub_agents": sub_agents,
                },
                "agent_split": agent_split,
                "agents": agents,
                "skills": skills,
            }),
            window,
        ));
    }
    // A subagent total with neither table behind it is not "nothing happened" —
    // it is a store whose per-agent split has not been extracted yet, and saying
    // "no usage" there would be false.
    if skills.is_empty() && agents.is_empty() && sub_tokens == 0 {
        println!(
            "{}",
            empty_in_window(
                "no skill usage found — run `saniwalens analyze` first",
                window
            )
        );
        return Ok(());
    }
    preamble(
        format,
        &framed(
            "What ran and what it cost. First the subagent bill per agent type\n\
             (costliest first), then how often each skill ran — output tokens written,\n\
             context growth while it ran, and wall-clock seconds (most-invoked first).\n\
             A section with nothing to show is omitted.",
            window,
        ),
    );
    println!(
        "tokens: main-thread skill output {main_out}, subagents {sub_tokens} ({sub_agents} agents)"
    );
    // Stated between the total and the table, so the table below is read as
    // the part it is. Under a drawn window the gap is the straddle, so the
    // note names the window rather than the pruned transcripts.
    if let Some(note) = agent_split.partial_note_in(window) {
        println!("{note}");
    }
    println!();

    // The subagent figure is usually the larger one, so break it down by agent
    // type before the per-skill table: a spawn count alone cannot tell an
    // expensive agent from a cheap one that runs often.
    if !agents.is_empty() {
        let rows: Vec<Vec<String>> = agents
            .iter()
            .map(|row| {
                vec![
                    row.agent.clone().unwrap_or_else(|| "(unknown)".to_string()),
                    row.runs.to_string(),
                    row.out_tokens.to_string(),
                ]
            })
            .collect();
        render(
            &["agent", "runs", "out tokens"],
            &[Align::Left, Align::Right, Align::Right],
            &rows,
            format,
        );
        println!();
    }

    if skills.is_empty() {
        return Ok(());
    }
    let rows: Vec<Vec<String>> = skills
        .iter()
        .map(|row| {
            vec![
                row.skill.clone(),
                row.invocations.to_string(),
                row.out_tokens.to_string(),
                row.ctx_growth.to_string(),
                format!("{:.0}", row.duration_sec),
            ]
        })
        .collect();
    render(
        &["skill", "runs", "out tokens", "ctx growth", "seconds"],
        &[
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        &rows,
        format,
    );
    Ok(())
}

/// Skill usage rolled up per time bucket (JST). A long span is assigned whole to
/// its start bucket (`docs/specs/cli.md`).
fn usage_by_time(store: &Store, bucket: Bucket, window: TimeRange, format: Format) -> Result<()> {
    let events = store.skill_event_costs(window)?;

    let mut totals: std::collections::BTreeMap<String, (i64, i64, i64, f64)> =
        std::collections::BTreeMap::new();
    for event in &events {
        let label = bucket_label(event.started_epoch, bucket, JST_OFFSET_SECS);
        let row = totals.entry(label).or_default();
        row.0 += 1;
        row.1 += event.out_tokens;
        row.2 += event.ctx_growth;
        row.3 += event.duration_sec;
    }

    if format == Format::Json {
        let buckets: Vec<serde_json::Value> = totals
            .iter()
            .map(|(label, (count, out_tokens, ctx_growth, duration_sec))| {
                serde_json::json!({
                    "bucket": label,
                    "count": count,
                    "out_tokens": out_tokens,
                    "ctx_growth": ctx_growth,
                    "duration_sec": duration_sec,
                })
            })
            .collect();
        return emit_json(&with_window(
            serde_json::json!({ "buckets": buckets }),
            window,
        ));
    }
    if events.is_empty() {
        println!(
            "{}",
            empty_in_window(
                "no skill usage found — run `saniwalens analyze` first",
                window
            )
        );
        return Ok(());
    }

    preamble(
        format,
        &framed(
            "Skill activity per time bucket (times in JST). A long-running skill counts\n\
             whole in the bucket it started in, so fine buckets can show spikes.",
            window,
        ),
    );
    let rows: Vec<Vec<String>> = totals
        .into_iter()
        .map(|(label, (count, out_tokens, ctx_growth, duration_sec))| {
            vec![
                label,
                count.to_string(),
                out_tokens.to_string(),
                ctx_growth.to_string(),
                format!("{duration_sec:.0}"),
            ]
        })
        .collect();
    render(
        &["bucket", "runs", "out tokens", "ctx growth", "seconds"],
        &[
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        &rows,
        format,
    );
    Ok(())
}

/// Join the catalogued surfaces against usage: each installed surface with its
/// static cost and (for usage-measurable kinds) invocation count, unused ones
/// flagged. Usage with no matching surface is shown as orphaned.
fn inventory(
    sess: &Session,
    filter: &ScopeFilter,
    window: TimeRange,
    format: Format,
) -> Result<()> {
    let store = &sess.store;
    eprint_window(window);
    let catalog: Vec<_> = store
        .effective_catalog(window)?
        .into_iter()
        .filter(|e| in_scope(filter, &e.scope, &e.project))
        .collect();
    let usage = store.usage_counts(window)?;

    if format == Format::Json {
        let catalogued: std::collections::HashSet<(&str, &str)> = catalog
            .iter()
            .map(|e| (e.kind.as_str(), e.id.as_str()))
            .collect();
        let mut items: Vec<serde_json::Value> = catalog
            .iter()
            .map(|entry| {
                let measurable = is_usage_measurable(&entry.kind);
                serde_json::json!({
                    "kind": entry.kind,
                    "id": entry.id,
                    "scope": entry.scope,
                    "project": if entry.project.is_empty() { None } else { Some(&entry.project) },
                    "static_tokens": entry.static_tokens,
                    "startup_tokens": entry.startup_tokens,
                    "uses": if measurable { Some(entry.uses) } else { None },
                    "load_mode": entry.load_mode,
                    "status": if measurable && entry.uses == 0 {
                        if window.is_all() { "unused" } else { "unused-in-window" }
                    } else if !measurable { "catalog-only" } else { "" },
                })
            })
            .collect();
        if *filter == ScopeFilter::All {
            for (kind, id, count) in &usage {
                if !catalogued.contains(&(kind.as_str(), id.as_str())) {
                    items.push(serde_json::json!({
                        "kind": kind, "id": id, "scope": null, "project": null,
                        "static_tokens": null, "startup_tokens": null,
                        "uses": count, "load_mode": null,
                        "status": "orphaned",
                    }));
                }
            }
        }
        return emit_json(&with_window(
            serde_json::json!({ "surfaces": items }),
            window,
        ));
    }
    if catalog.is_empty() && usage.is_empty() {
        println!("nothing catalogued — run `saniwalens analyze` first");
        return Ok(());
    }

    preamble(format, &framed(sess.harness.inventory_preamble(), window));
    let mut rows: Vec<Vec<String>> = Vec::new();
    for entry in &catalog {
        let measurable = is_usage_measurable(&entry.kind);
        // "unused" is only meaningful for usage-measurable kinds; catalog-only
        // kinds (rules, hooks, CLAUDE.md) emit no events, so 0 means nothing.
        let (uses_cell, status) = if measurable {
            (
                entry.uses.to_string(),
                if entry.uses == 0 {
                    unused_label(window)
                } else {
                    ""
                },
            )
        } else {
            ("-".to_string(), "(catalog-only)")
        };
        let tokens = |value: Option<i64>| value.map_or_else(|| "?".to_string(), |t| t.to_string());
        rows.push(vec![
            entry.kind.clone(),
            entry.id.clone(),
            scope_cell(&entry.scope, &entry.project),
            tokens(entry.startup_tokens),
            tokens(entry.static_tokens),
            uses_cell,
            entry.load_mode.clone(),
            status.to_string(),
        ]);
    }

    // Orphaned usage has no scope to filter on; it appears in the full view only.
    let catalogued: std::collections::HashSet<(&str, &str)> = catalog
        .iter()
        .map(|e| (e.kind.as_str(), e.id.as_str()))
        .collect();
    for (kind, id, count) in &usage {
        if *filter != ScopeFilter::All {
            break;
        }
        if !catalogued.contains(&(kind.as_str(), id.as_str())) {
            rows.push(vec![
                kind.clone(),
                id.clone(),
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
                count.to_string(),
                "-".to_string(),
                "ORPHANED".to_string(),
            ]);
        }
    }

    render(
        &[
            "kind",
            "id",
            "scope",
            "start tok",
            "static tok",
            "uses",
            "load mode",
            "status",
        ],
        &[
            Align::Left,
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Left,
            Align::Left,
        ],
        &rows,
        format,
    );
    Ok(())
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
    fn doctor_calls_the_top_rows_most_of_the_output_only_when_they_are() {
        let f = findings_with_agents(
            1_000_000,
            10,
            vec![agent("a", 4, 700_000), agent("b", 6, 300_000)],
        );
        let lines = doctor_agent_lines(&f, "saniwalens usage", TimeRange::ALL);
        assert_eq!(
            lines,
            vec![
                "Most of that subagent output: a 700.0k (4 runs), b 300.0k (6 runs). \
                 Full split: saniwalens usage"
            ]
        );
    }

    #[test]
    fn doctor_does_not_call_a_minority_of_the_output_most_of_it() {
        // A long tail: ten types at a tenth each, so the three shown cover 30%
        // of the output, and "most" would be false even with a complete split.
        let agents: Vec<_> = (0..10)
            .map(|i| agent(&format!("t{i}"), 1, 100_000))
            .collect();
        let f = findings_with_agents(1_000_000, 10, agents);
        let lines = doctor_agent_lines(&f, "saniwalens usage", TimeRange::ALL);
        assert_eq!(
            lines,
            vec![
                "The largest agent types: t0 100.0k (1 run), t1 100.0k (1 run), t2 100.0k \
                 (1 run). Full split: saniwalens usage",
                "… and 7 more agent type(s) not shown",
            ]
        );
    }

    #[test]
    fn doctor_labels_a_partial_split_before_naming_its_agents() {
        // The rows explain 12% of the total: saying "most of that output" here
        // would blame the listed types for tokens no row accounts for.
        let f = findings_with_agents(
            3_093_466,
            375,
            vec![
                agent("general-purpose", 11, 315_663),
                agent("codex:codex-rescue", 14, 62_067),
            ],
        );
        let lines = doctor_agent_lines(&f, "saniwalens usage", TimeRange::ALL);
        assert_eq!(
            lines,
            vec![
                "Only 377.7k of those tokens (25 of 375 runs) have a per-agent split — the \
                 rest were analyzed before runs were extracted and their transcripts are gone.",
                "Among the runs that have one: general-purpose 315.7k (11 runs), \
                 codex:codex-rescue 62.1k (14 runs). Full split: saniwalens usage",
            ]
        );
    }

    #[test]
    fn doctor_labels_a_windowed_partial_split_with_the_window() {
        // Under a drawn window the gap is the straddle — the total reads the
        // session's stamped start, the rows read each run's own epoch — so the
        // doctor names the window instead of the pruned transcripts.
        let f = findings_with_agents(
            3_093_466,
            375,
            vec![
                agent("general-purpose", 11, 315_663),
                agent("codex:codex-rescue", 14, 62_067),
            ],
        );
        let window = TimeRange {
            start: Some(1_790_780_400),
            end: Some(1_791_385_199),
        };
        let lines = doctor_agent_lines(&f, "saniwalens usage", window);
        assert!(
            lines[0].contains("fall outside the report window"),
            "{}",
            lines[0]
        );
        assert!(
            !lines[0].contains("analyzed before runs were extracted"),
            "{}",
            lines[0]
        );
        // Without a window the pruned-transcript wording stays, byte for byte.
        let lines = doctor_agent_lines(&f, "saniwalens usage", TimeRange::ALL);
        assert!(
            lines[0].contains("analyzed before runs were extracted"),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn doctor_names_no_agents_when_the_split_is_unavailable() {
        // The gap is reported on stderr with its refresh hint; there is
        // nothing to list in-band.
        let f = findings_with_agents(3_093_466, 375, vec![]);
        assert!(doctor_agent_lines(&f, "saniwalens usage", TimeRange::ALL).is_empty());
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

    /// A path in the temp dir that no other test uses, removed on drop so a
    /// failure does not leak a file into the runner's temp dir.
    struct TempPath(PathBuf);

    impl TempPath {
        fn new(tag: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("saniwalens-test-{}-{tag}", std::process::id()));
            let _ = fs::remove_file(&path);
            Self(path)
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn write_private_file_refuses_a_path_that_already_exists() {
        // The temp dir is shared, so a same-user process can pre-create the
        // path — as a symlink to somewhere it can read. Writing the briefing
        // into whatever is already there is the leak this prevents.
        let path = TempPath::new("preexisting.md");
        fs::write(&path.0, "squatted").unwrap();
        assert!(write_private_file(&path.0, "briefing").is_err());
        assert_eq!(fs::read_to_string(&path.0).unwrap(), "squatted");
    }

    #[test]
    fn write_private_file_writes_the_contents_to_a_fresh_path() {
        let path = TempPath::new("fresh.md");
        write_private_file(&path.0, "briefing").unwrap();
        assert_eq!(fs::read_to_string(&path.0).unwrap(), "briefing");
    }

    #[cfg(unix)]
    #[test]
    fn write_private_file_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let path = TempPath::new("mode.md");
        write_private_file(&path.0, "briefing").unwrap();
        let mode = fs::metadata(&path.0).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn shorten_to_tilde_replaces_the_home_prefix() {
        assert_eq!(
            shorten_to_tilde("/tmp/example/me/.claude", Some("/tmp/example/me")),
            "~/.claude"
        );
        // Home itself collapses to a bare tilde.
        assert_eq!(
            shorten_to_tilde("/tmp/example/me", Some("/tmp/example/me")),
            "~"
        );
    }

    #[test]
    fn shorten_to_tilde_accepts_a_backslash_separator() {
        assert_eq!(
            shorten_to_tilde(r"C:\example\me\.claude", Some(r"C:\example\me")),
            r"~\.claude"
        );
    }

    #[test]
    fn shorten_to_tilde_matches_a_home_spelled_with_the_other_separator() {
        // Windows takes either separator, and the home comes from the
        // environment while the path comes from a recorded cwd — so the two can
        // disagree on spelling while naming the same directory.
        assert_eq!(
            shorten_to_tilde("C:/example/me/.claude", Some(r"C:\example\me")),
            "~/.claude"
        );
        assert_eq!(
            shorten_to_tilde(r"C:\example\me\.claude", Some("C:/example/me")),
            r"~\.claude"
        );
        // The suffix keeps the spelling the path actually had, not the folded
        // one used for the comparison.
        assert_eq!(
            shorten_to_tilde(r"C:\example\me\a/b\c", Some("C:/example/me")),
            r"~\a/b\c"
        );
    }

    #[test]
    fn shorten_to_tilde_leaves_a_path_outside_home_alone() {
        assert_eq!(
            shorten_to_tilde("/tmp/other", Some("/tmp/example/me")),
            "/tmp/other"
        );
        // A sibling that merely shares the prefix is not under home.
        assert_eq!(
            shorten_to_tilde("/tmp/example/metadata", Some("/tmp/example/me")),
            "/tmp/example/metadata"
        );
    }

    #[test]
    fn shorten_to_tilde_passes_the_path_through_without_a_home() {
        assert_eq!(shorten_to_tilde("/tmp/example/me", None), "/tmp/example/me");
        assert_eq!(
            shorten_to_tilde("/tmp/example/me", Some("")),
            "/tmp/example/me"
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

    // ---- PBI 06: the delivery seam ----

    /// A delivery double: records what would have been launched instead of
    /// spawning anything, so the briefing's argv can be asserted against a
    /// mock (BDD: "従来と同一の引数で claude が起動する").
    #[cfg(unix)]
    struct RecordingAdvisor {
        direct: Vec<(String, Vec<String>, String)>,
        headless: Vec<(String, Vec<String>, String)>,
    }

    #[cfg(unix)]
    fn success_status() -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0)
    }

    #[cfg(unix)]
    impl RecordingAdvisor {
        fn new() -> Self {
            Self {
                direct: Vec::new(),
                headless: Vec::new(),
            }
        }
    }

    #[cfg(unix)]
    impl AdvisorLaunch for RecordingAdvisor {
        fn direct(
            &mut self,
            binary: &str,
            leading: &[&str],
            prompt: &str,
        ) -> Result<std::process::ExitStatus> {
            self.direct.push((
                binary.to_string(),
                leading.iter().map(|s| s.to_string()).collect(),
                prompt.to_string(),
            ));
            Ok(success_status())
        }

        fn headless(
            &mut self,
            binary: &str,
            leading: &[&str],
            prompt: &str,
            _render: Option<RunLineRenderer>,
            _timeout: Option<std::time::Duration>,
        ) -> Result<(std::process::ExitStatus, PathBuf)> {
            self.headless.push((
                binary.to_string(),
                leading.iter().map(|s| s.to_string()).collect(),
                prompt.to_string(),
            ));
            Ok((success_status(), PathBuf::from("/tmp/never-opened.log")))
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_print_delivery_writes_to_stdout_and_launches_nothing() {
        let plan = DeliveryPlan::print("FULL PROMPT".to_string());
        let mut advisor = RecordingAdvisor::new();

        deliver(plan, &mut advisor).unwrap();

        assert!(advisor.direct.is_empty(), "print never spawns");
        assert!(advisor.headless.is_empty(), "print never spawns");
    }

    #[cfg(unix)]
    #[test]
    fn the_interactive_delivery_hands_the_double_the_measured_launch() {
        use std::cell::RefCell;
        let seen_path: RefCell<Option<PathBuf>> = RefCell::new(None);
        let plan = DeliveryPlan::launch(
            ("claude", &[]),
            DeliveryMode::Interactive,
            "the rendered briefing".to_string(),
            "Claude Code",
            false,
            None,
            None,
            |path| {
                *seen_path.borrow_mut() = Some(path.to_path_buf());
                "POINTER PROMPT".to_string()
            },
        );
        let mut advisor = RecordingAdvisor::new();

        deliver(plan, &mut advisor).unwrap();

        assert_eq!(advisor.direct.len(), 1);
        assert_eq!(advisor.direct[0].0, "claude");
        assert!(advisor.direct[0].1.is_empty());
        assert_eq!(advisor.direct[0].2, "POINTER PROMPT");
        // The private pointer file is removed the moment the run ends.
        let path = seen_path.borrow().clone().expect("pointer got the path");
        assert!(!path.exists(), "briefing file outlived its run");
    }

    #[cfg(unix)]
    #[test]
    fn headless_routes_through_the_logged_adapter_unless_verbose() {
        let headless_plan = DeliveryPlan::launch(
            ("opencode", &["run", "--format", "json"]),
            DeliveryMode::Headless,
            "briefing".to_string(),
            "opencode",
            false,
            Harness::Opencode.run_line_renderer(),
            None,
            |_| "pointer".to_string(),
        );
        let mut advisor = RecordingAdvisor::new();
        deliver(headless_plan, &mut advisor).unwrap();
        assert_eq!(advisor.headless.len(), 1, "quiet headless logs its stream");
        assert!(advisor.direct.is_empty());

        // `--verbose` inherits stdio for the raw streaming instead
        // (`docs/specs/harness.md`).
        let verbose_plan = DeliveryPlan::launch(
            ("opencode", &["run", "--format", "json"]),
            DeliveryMode::Headless,
            "briefing".to_string(),
            "opencode",
            true,
            Harness::Opencode.run_line_renderer(),
            None,
            |_| "pointer".to_string(),
        );
        let mut advisor = RecordingAdvisor::new();
        deliver(verbose_plan, &mut advisor).unwrap();
        assert_eq!(advisor.direct.len(), 1);
        assert!(advisor.headless.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn headless_delivery_removes_the_briefing_file_like_the_other_launches() {
        use std::cell::RefCell;
        let seen_path: RefCell<Option<PathBuf>> = RefCell::new(None);
        let plan = DeliveryPlan::launch(
            ("opencode", &["run", "--format", "json"]),
            DeliveryMode::Headless,
            "the rendered briefing".to_string(),
            "opencode",
            false,
            Harness::Opencode.run_line_renderer(),
            None,
            |path| {
                *seen_path.borrow_mut() = Some(path.to_path_buf());
                "POINTER PROMPT".to_string()
            },
        );
        let mut advisor = RecordingAdvisor::new();

        deliver(plan, &mut advisor).unwrap();

        let path = seen_path.borrow().clone().expect("pointer got the path");
        assert!(!path.exists(), "briefing file outlived its headless run");
    }

    #[test]
    fn the_delivery_mode_is_the_flag_over_the_launch_shape() {
        assert_eq!(
            delivery_mode(true, optimize_mod::RunMode::Interactive),
            DeliveryMode::Print
        );
        assert_eq!(
            delivery_mode(false, optimize_mod::RunMode::Interactive),
            DeliveryMode::Interactive
        );
        assert_eq!(
            delivery_mode(false, optimize_mod::RunMode::Headless),
            DeliveryMode::Headless
        );
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
