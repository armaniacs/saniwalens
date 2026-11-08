//! Harness selection: which agent harness a run analyzes and where its
//! files live. This module owns the per-harness facts as data — the
//! *locations* (upstream conventions, never the formats themselves), the
//! optimize launch shape, and the one catalog traversal that walks those
//! locations for every harness. See `docs/specs/harness.md`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::adapter::config::{
    add_skill_site, read_agent_surfaces, read_claude_md_surfaces, read_command_surfaces,
    read_mcp_server_surfaces, read_rule_surfaces, read_skill_surfaces,
};
use crate::adapter::opencode;
use crate::core::optimize::RunMode;
use crate::core::surface::{Scope, Surface};

/// A headless run-line renderer: one raw stdout line turned into the
/// [`RunEvent`](opencode::RunEvent) the terminal shows. The signature is
/// uniform so the shell's headless loop can run any harness's renderer
/// without knowing its stream.
pub type RunLineRenderer = fn(&str) -> opencode::RunEvent;

/// The agent harness one store holds. One store never mixes harnesses, so
/// this labels a whole run — never a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Opencode,
    Kilocode,
}

/// Counters from one analyze run, for the command's summary line and the
/// freshness header on auto-refreshing reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalyzeStats {
    pub sessions: usize,
    pub skipped: usize,
    pub spans_total: usize,
    pub surface_count: usize,
    pub denials: usize,
}

/// How a [`Site`] is read — the loader a site row names. The signature is
/// uniform so the traversal can run any row without knowing its kind.
pub type SiteLoader = fn(&Site, &Scope, &mut SiteCtx) -> Vec<Surface>;

/// One place a surface kind is read from: **site definitions as data**. A row
/// is where the definitions live plus how to load them; a new surface kind is
/// a loader and a row, never an edit to the traversal
/// (`docs/specs/config-format.md`).
pub struct Site {
    /// The site's location: a directory of definitions, a config file, or the
    /// root whose named files the loader derives.
    pub path: PathBuf,
    /// Extra `(file, label)` pairs for loaders that read named files inside
    /// `path` (the `CLAUDE.md` / `AGENTS.md` convention): the file relative to
    /// `path`, the label the surface is reported under.
    pub payload: &'static [(&'static str, &'static str)],
    /// How to read this site's surfaces for a scope.
    pub load: SiteLoader,
}

impl Site {
    pub fn new(path: PathBuf, load: SiteLoader) -> Self {
        Self {
            path,
            payload: &[],
            load,
        }
    }

    pub fn with_payload(
        path: PathBuf,
        load: SiteLoader,
        payload: &'static [(&'static str, &'static str)],
    ) -> Self {
        Self {
            path,
            payload,
            load,
        }
    }
}

/// What every loader is handed: the environment a site reads through and the
/// dedup set the traversal owns — so a caller of `catalog_surfaces` never
/// builds a `Scope` or carries a `seen` set itself.
pub struct SiteCtx<'a> {
    /// The user's home, for `~/` imports and the home-adjacent skill sites.
    pub home: Option<&'a Path>,
    /// Canonical definition paths already catalogued — skill sites dedup on
    /// it, so one definition reached through two sites counts once.
    pub seen: &'a mut HashSet<PathBuf>,
}

pub(crate) fn load_skills(site: &Site, scope: &Scope, _ctx: &mut SiteCtx) -> Vec<Surface> {
    read_skill_surfaces(&site.path, scope)
}

pub(crate) fn load_skills_deduped(site: &Site, scope: &Scope, ctx: &mut SiteCtx) -> Vec<Surface> {
    let mut surfaces = Vec::new();
    add_skill_site(&site.path, scope, ctx.seen, &mut surfaces);
    surfaces
}

pub(crate) fn load_rules(site: &Site, scope: &Scope, _ctx: &mut SiteCtx) -> Vec<Surface> {
    read_rule_surfaces(&site.path, scope)
}

pub(crate) fn load_agents(site: &Site, scope: &Scope, _ctx: &mut SiteCtx) -> Vec<Surface> {
    read_agent_surfaces(&site.path, scope)
}

pub(crate) fn load_commands(site: &Site, scope: &Scope, _ctx: &mut SiteCtx) -> Vec<Surface> {
    read_command_surfaces(&site.path, scope)
}

pub(crate) fn load_mcp(site: &Site, scope: &Scope, _ctx: &mut SiteCtx) -> Vec<Surface> {
    read_mcp_server_surfaces(&site.path, scope)
}

/// `settings.json` hooks (global or project): one always-measurable surface
/// per hook entry, named `Event:Matcher` like the transcript's `hookName`.
/// Missing or unparsable files read as no hooks — hooks are optional config.
pub(crate) fn load_hooks(site: &Site, scope: &Scope, _ctx: &mut SiteCtx) -> Vec<Surface> {
    crate::adapter::config::read_hook_surfaces(&site.path, scope)
}

/// The `CLAUDE.md`-convention files this site names (`payload`), loaded with
/// their `@import`s resolved through the home dir.
pub(crate) fn load_claude_md(site: &Site, scope: &Scope, ctx: &mut SiteCtx) -> Vec<Surface> {
    let files: Vec<(PathBuf, &str)> = site
        .payload
        .iter()
        .map(|(file, label)| (site.path.join(file), *label))
        .collect();
    let refs: Vec<(&Path, &str)> = files.iter().map(|(p, l)| (p.as_path(), *l)).collect();
    read_claude_md_surfaces(&refs, scope, ctx.home)
}

/// kilo's `kilo.json` `instructions[]`: relative paths resolve against the
/// config file's own directory (`site.path`'s parent).
pub(crate) fn load_kilo_instructions(
    site: &Site,
    scope: &Scope,
    ctx: &mut SiteCtx,
) -> Vec<Surface> {
    let base = site.path.parent().unwrap_or(&site.path);
    crate::adapter::kilocode::read_kilo_instructions(&site.path, base, scope, ctx.home)
}

/// Load one site list under one scope, with the shared dedup set — the piece
/// the per-harness `read_*_surfaces` shims and the traversal below share.
pub(crate) fn load_sites(
    sites: &[Site],
    scope: &Scope,
    home: Option<&Path>,
    seen: &mut HashSet<PathBuf>,
) -> Vec<Surface> {
    let mut ctx = SiteCtx { home, seen };
    sites
        .iter()
        .flat_map(|site| (site.load)(site, scope, &mut ctx))
        .collect()
}

/// The catalog traversal, once for every harness: the global sites, then each
/// surviving root's project sites — `Scope` built here and the dedup `seen`
/// owned here, so callers pass roots and get rows
/// (`pbi/2026-10-29-05-feat-config-catalog-traversal.md`).
pub(crate) fn catalog_sites(
    global: &[Site],
    project_sites: impl Fn(&Path) -> Vec<Site>,
    roots: &[(String, String)],
    home: Option<&Path>,
) -> Vec<Surface> {
    let mut seen = HashSet::new();
    let global_scope = Scope::Global;
    let mut surfaces = load_sites(global, &global_scope, home, &mut seen);
    for (root, project) in roots {
        let root = Path::new(root);
        // A checkout recorded in the store but gone from disk contributes
        // nothing — its rows stay in the store, its live config cannot be read.
        if !root.is_dir() {
            continue;
        }
        let scope = Scope::Project(project.clone());
        surfaces.extend(load_sites(&project_sites(root), &scope, home, &mut seen));
    }
    surfaces
}

/// Every installed surface for one harness across the given project roots —
/// the single traversal entry point `analyze` uses (`docs/specs/harness.md`).
/// Resolves this harness's config root from the environment; the roots come
/// from the store's recorded sessions.
pub fn catalog_surfaces(harness: Harness, roots: &[(String, String)]) -> Result<Vec<Surface>> {
    let home = home_dir().map(Path::new);
    let global = match harness {
        Harness::Claude => {
            let config_dir = crate::adapter::config::claude_home()?;
            crate::adapter::claude::global_sites(&config_dir)
        }
        Harness::Opencode => {
            let config_dir = crate::adapter::opencode::opencode_home()?;
            crate::adapter::opencode::global_sites(&config_dir, home)
        }
        Harness::Kilocode => {
            let config_dir = crate::adapter::kilocode::kilo_home()?;
            crate::adapter::kilocode::global_sites(&config_dir, home)
        }
    };
    let project_sites: fn(&Path) -> Vec<Site> = match harness {
        Harness::Claude => crate::adapter::claude::project_sites,
        Harness::Opencode => crate::adapter::opencode::project_sites,
        Harness::Kilocode => crate::adapter::kilocode::project_sites,
    };
    Ok(catalog_sites(&global, project_sites, roots, home))
}

impl Harness {
    /// Analyze this harness's input into `db` — the one seam the `analyze`
    /// command and the views' auto-refresh call. The per-harness routines
    /// live beside their formats (`adapter::claude`, `adapter::opencode`,
    /// `adapter::kilocode`), so a new harness is a variant plus its adapter
    /// module and `cli/mod.rs` does not change — on the analyze side here or
    /// the launch side (`launch_command`, below) (`docs/specs/harness.md`).
    pub fn analyze(&self, input: Option<PathBuf>, db: &Path) -> Result<AnalyzeStats> {
        match self {
            Harness::Claude => crate::adapter::claude::analyze(input, db),
            Harness::Opencode => crate::adapter::opencode::analyze(input, db),
            Harness::Kilocode => crate::adapter::kilocode::analyze(input, db),
        }
    }

    /// The plain harness data every prompt and briefing renders from —
    /// assembled here so the shell never spells `HarnessCopy` field by field
    /// and the core stays harness-agnostic (`core::optimize::HarnessCopy`).
    pub fn copy(&self) -> crate::core::optimize::HarnessCopy {
        crate::core::optimize::HarnessCopy {
            agent: self.display_name(),
            article: self.article(),
            source: self.source_noun(),
            raw_source: self.raw_source(),
            global_ref: self.global_config_ref(),
            project_ref: self.project_config_ref(),
            instruction_file: self.instruction_file(),
            config_inventory: self.config_inventory(),
        }
    }

    /// Parse `--harness`. `None` for anything else, so the caller can fail
    /// with the valid list.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Harness::Claude),
            "opencode" => Some(Harness::Opencode),
            "kilocode" => Some(Harness::Kilocode),
            _ => None,
        }
    }

    /// The `meta.harness` value and the error-hint vocabulary.
    pub fn label(&self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Opencode => "opencode",
            Harness::Kilocode => "kilocode",
        }
    }

    /// The human-facing name for report copy. Reports name the harness whose
    /// rows they render, so opencode output never claims to be Claude Code's.
    pub fn display_name(&self) -> &'static str {
        match self {
            Harness::Claude => "Claude Code",
            Harness::Opencode => "opencode",
            Harness::Kilocode => "kilocode",
        }
    }

    /// The indefinite article for the display name in prose ("a Claude Code
    /// advisor", "an opencode advisor").
    pub fn article(&self) -> &'static str {
        match self {
            Harness::Claude => "a",
            Harness::Opencode => "an",
            Harness::Kilocode => "a",
        }
    }

    /// The instruction file reports point at: the file a file-map or rule
    /// fix lands in. kilocode reads the shared `AGENTS.md` convention.
    pub fn instruction_file(&self) -> &'static str {
        match self {
            Harness::Claude => "CLAUDE.md",
            Harness::Opencode => "AGENTS.md",
            Harness::Kilocode => "AGENTS.md",
        }
    }

    /// How reports name the project config layer that owns a fix. kilocode's
    /// project-level conventions (`.kilocode/rules` and the like) are not
    /// observed, so the copy names the one instruction file that exists.
    pub fn project_config_ref(&self) -> &'static str {
        match self {
            Harness::Claude => ".claude / CLAUDE.md",
            Harness::Opencode => ".opencode / AGENTS.md",
            Harness::Kilocode => "AGENTS.md",
        }
    }

    /// How reports name the global config layer that owns a fix.
    pub fn global_config_ref(&self) -> &'static str {
        match self {
            Harness::Claude => "~/.claude",
            Harness::Opencode => "~/.config/opencode",
            Harness::Kilocode => "~/.config/kilo",
        }
    }

    /// The noun for analyzed input in prose ("their Claude Code session
    /// transcripts", "their opencode sessions").
    pub fn source_noun(&self) -> &'static str {
        match self {
            Harness::Claude => "session transcripts",
            Harness::Opencode => "sessions",
            Harness::Kilocode => "sessions",
        }
    }

    /// The raw input advisors must not re-parse ("the raw ~/.claude/projects
    /// transcripts", "the raw opencode database").
    pub fn raw_source(&self) -> &'static str {
        match self {
            Harness::Claude => "~/.claude/projects transcripts",
            Harness::Opencode => "opencode database",
            Harness::Kilocode => "kilocode task files",
        }
    }

    /// The config files an advisor opens to pin down a root cause.
    pub fn config_inventory(&self) -> &'static str {
        match self {
            Harness::Claude => "CLAUDE.md, rules, hooks, skills, and settings",
            Harness::Opencode => "AGENTS.md, commands, agents, and opencode.json",
            Harness::Kilocode => "AGENTS.md, commands, skills, and kilo.json",
        }
    }

    /// The readable-config breakdown in the `overhead` view.
    pub fn readable_config(&self) -> &'static str {
        match self {
            Harness::Claude => "CLAUDE.md, always-on rules, skill/agent descriptions",
            Harness::Opencode => "AGENTS.md, commands, agent descriptions",
            Harness::Kilocode => "AGENTS.md, command and skill descriptions",
        }
    }

    /// The unit a refresh re-analyzes ("transcript(s)", "session(s)").
    pub fn input_unit(&self) -> &'static str {
        match self {
            Harness::Claude => "transcript(s)",
            Harness::Opencode => "session(s)",
            Harness::Kilocode => "session(s)",
        }
    }

    /// The launch shape for an optimize delivery: the binary plus any leading
    /// argv, and the [`RunMode`] the seeded session runs in. `claude` opens
    /// interactively on the prompt; `opencode run` investigates headlessly —
    /// JSON events (`--format json`) so the raw tool dumps can be logged while
    /// only the agent's text streams, and a title so the sessions it seeds are
    /// recognizable in the store it feeds. Per-harness data beside the other
    /// arms, so a new harness lands here and the shell's launch side neither
    /// matches nor changes (`docs/specs/harness.md`).
    pub fn launch_command(&self) -> (&'static str, &'static [&'static str], RunMode) {
        match self {
            Harness::Claude => ("claude", &[], RunMode::Interactive),
            Harness::Opencode => (
                "opencode",
                &["run", "--format", "json", "--title", "saniwalens optimize"],
                RunMode::Headless,
            ),
            // The `kilo` binary is the measured agent CLI (Homebrew), and kilo is
            // an OpenCode fork whose TUI takes a prompt: `kilo [project] --prompt
            // <prompt>` opens the interactive session on the pointer prompt — the
            // experience the claude launch gives, with the user present to approve
            // the briefing read and steer (kilo gates every tool call where
            // opencode does not, so a headless run would need `--auto` instead).
            // A bare `kilo <text>` positional is treated as a file to open
            // (measured), not a prompt — `--prompt` is the TUI's message surface.
            Harness::Kilocode => ("kilo", &["--prompt"], RunMode::Interactive),
        }
    }

    /// The renderer for a headless run's raw stdout lines: the adapter fn
    /// turning one raw line into a [`RunEvent`](opencode::RunEvent), handed to
    /// the shell's headless loop as seam data (`run_headless` in `cli/headless.rs`),
    /// so the loop neither names a harness nor matches on one. `opencode run
    /// --format json` streams unversioned JSON events, so the renderer is the
    /// lenient one; kilo is an OpenCode fork and shares the stream, so a
    /// headless kilo run is the same arm. Claude's launch opens the
    /// interactive TUI (`launch_command`), so it has no headless stream to
    /// render. Per-harness data beside the launch shape, so a new harness
    /// lands here and the loop never changes (`docs/specs/harness.md`).
    pub fn run_line_renderer(&self) -> Option<RunLineRenderer> {
        match self {
            Harness::Claude => None,
            Harness::Opencode | Harness::Kilocode => Some(opencode::render_run_line),
        }
    }

    /// The `inventory` framing block. Kind lists differ per harness, so the
    /// whole block is per-harness rather than interpolated.
    pub fn inventory_preamble(&self) -> &'static str {
        match self {
            Harness::Claude => {
                "Everything installed in your config — skills, rules, agents, MCP servers,\n\
             CLAUDE.md — with where it is installed (scope) and whether it was ever used.\n\
             \"start tok\" is what every session pays for having it installed; \"static tok\"\n\
             is the whole definition, which for a skill or agent is paid only on invocation.\n\
             UNUSED marks delete candidates; rules and CLAUDE.md are catalog-only because\n\
             using them leaves no trace in transcripts."
            }
            Harness::Opencode => {
                "Everything installed in your config — commands, agents,\n\
             AGENTS.md — with where it is installed (scope) and whether it was ever used.\n\
             \"start tok\" is what every session pays for having it installed; \"static tok\"\n\
             is the whole definition, which for a command or agent is paid only on invocation.\n\
             UNUSED marks delete candidates; commands and AGENTS.md are catalog-only because\n\
             using them leaves no trace in sessions."
            }
            Harness::Kilocode => {
                "Everything installed in your config — AGENTS.md, .kilorules, commands,\n\
             agents, skills — is listed with where it is installed (scope) and whether it\n\
             was ever used. UNUSED marks delete candidates; commands are catalog-only\n\
             because using them leaves no trace in sessions."
            }
        }
    }

    /// The store file name under the user state dir. Claude keeps the
    /// historical name; every later harness gets its own file, so an old
    /// store is never reinterpreted as another harness's rows.
    pub fn db_file_name(&self) -> &'static str {
        match self {
            Harness::Claude => "saniwalens.db",
            Harness::Opencode => "saniwalens-opencode.db",
            Harness::Kilocode => "saniwalens-kilocode.db",
        }
    }

    /// The editor globalStorage task directories kilocode writes, in the
    /// order the default resolution probes them. The path varies with the
    /// editor install (Code / Cursor / VSCodium) and the platform, so the
    /// list — not one hardcoded path — is the contract; `--projects` always
    /// wins over it.
    pub fn kilocode_tasks_candidates(home: Option<&Path>) -> Vec<PathBuf> {
        let Some(home) = home.filter(|dir| dir.is_absolute()) else {
            return Vec::new();
        };
        let editors: [&str; 3] = if cfg!(target_os = "macos") {
            [
                "Library/Application Support/Code/User/globalStorage",
                "Library/Application Support/Cursor/User/globalStorage",
                "Library/Application Support/VSCodium/User/globalStorage",
            ]
        } else {
            [
                ".config/Code/User/globalStorage",
                ".config/Cursor/User/globalStorage",
                ".config/VSCodium/User/globalStorage",
            ]
        };
        editors
            .iter()
            .map(|editor| home.join(editor).join("kilocode.kilo-code").join("tasks"))
            .collect()
    }

    /// The opencode database: `$XDG_DATA_HOME/opencode/opencode.db`, else
    /// `~/.local/share/opencode/opencode.db`. A non-absolute data home is
    /// invalid per the XDG spec and ignored, mirroring `store_path`.
    pub fn opencode_db_path(data_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
        if let Some(dir) = data_home.filter(|dir| dir.is_absolute()) {
            return Some(dir.join("opencode").join("opencode.db"));
        }
        Some(
            home.filter(|dir| dir.is_absolute())?
                .join(".local")
                .join("share")
                .join("opencode")
                .join("opencode.db"),
        )
    }

    /// The opencode config root: `$XDG_CONFIG_HOME/opencode`, else
    /// `~/.config/opencode`. Same absolute-path rule as the database.
    pub fn opencode_config_dir(config_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
        if let Some(dir) = config_home.filter(|dir| dir.is_absolute()) {
            return Some(dir.join("opencode"));
        }
        Some(
            home.filter(|dir| dir.is_absolute())?
                .join(".config")
                .join("opencode"),
        )
    }

    /// The global kilo config root: `$XDG_CONFIG_HOME/kilo` when that variable
    /// names an absolute path, else `~/.config/kilo` — the XDG rule, mirroring
    /// how the store resolution treats `XDG_STATE_HOME`.
    pub fn kilocode_config_dir(config_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
        if let Some(dir) = config_home.filter(|dir| dir.is_absolute()) {
            return Some(dir.join("kilo"));
        }
        Some(
            home.filter(|dir| dir.is_absolute())?
                .join(".config")
                .join("kilo"),
        )
    }
}

/// The user's home directory, resolved once per run.
///
/// `HOME` is preferred, so unix behaves exactly as before; Windows does not set
/// it, hence `USERPROFILE`. The wrinkle is that a Windows shell like Git Bash
/// *does* export `HOME` — as a POSIX path (`/c/Users/…`) that a native binary
/// cannot resolve. Taking it at face value would send every lookup to a
/// directory that does not exist and report "no transcripts" instead of an
/// error, so on Windows a candidate has to name a real directory to win. The
/// check stays off on unix, where a home that does not exist is worth surfacing
/// as itself rather than silently falling through to another variable.
pub(crate) fn home_dir() -> Option<&'static str> {
    static HOME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        ["HOME", "USERPROFILE"]
            .into_iter()
            .filter_map(|key| std::env::var(key).ok())
            .filter(|value| !value.is_empty())
            .find(|value| !cfg!(windows) || Path::new(value).is_dir())
    })
    .as_deref()
}

/// A file's `(mtime epoch nanos, size)` — the cheap change detector for
/// incremental ingest. `None` when the file cannot be stat'ed (it is then
/// ingested unconditionally and not recorded).
///
/// Nanosecond granularity: whole-second truncation made a same-second,
/// same-size edit invisible, so the transcript was skipped as unchanged until
/// something else moved the fingerprint (`docs/specs/storage.md`). Epoch
/// nanos fit in an `i64` until the year 2262.
pub(crate) fn file_fingerprint(path: &Path) -> Option<(i64, i64)> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos() as i64;
    Some((mtime, meta.len() as i64))
}

/// Whether a post-read re-stat agrees with the pre-read fingerprint: the
/// stamp records "this content was ingested", so it may only be written when
/// the source did not move while it was read. Any drift — or a stat failing
/// on either side — refuses the stamp, and the next run re-ingests instead
/// of skipping on a stale stamp (`pbi/2026-11-09-02-fix-fingerprint-restat.md`).
pub(crate) fn restat_matches(pre: Option<(i64, i64)>, post: Option<(i64, i64)>) -> bool {
    match (pre, post) {
        (Some(before), Some(after)) => before == after,
        _ => false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::surface::LoadMode;

    #[test]
    fn parses_all_three_harnesses_and_rejects_the_rest() {
        assert_eq!(Harness::parse("claude"), Some(Harness::Claude));
        assert_eq!(Harness::parse("opencode"), Some(Harness::Opencode));
        assert_eq!(Harness::parse("kilocode"), Some(Harness::Kilocode));
        assert_eq!(Harness::parse("auto"), None);
        assert_eq!(Harness::parse(""), None);
    }

    #[test]
    fn labels_round_trip_through_parse() {
        for harness in [Harness::Claude, Harness::Opencode, Harness::Kilocode] {
            assert_eq!(Harness::parse(harness.label()), Some(harness));
        }
    }

    #[test]
    fn display_names_name_the_harness() {
        assert_eq!(Harness::Claude.display_name(), "Claude Code");
        assert_eq!(Harness::Opencode.display_name(), "opencode");
        assert_eq!(Harness::Kilocode.display_name(), "kilocode");
        assert_eq!(Harness::Claude.article(), "a");
        assert_eq!(Harness::Opencode.article(), "an");
        assert_eq!(Harness::Kilocode.article(), "a");
    }

    #[test]
    fn report_copy_names_each_harness_own_config() {
        assert_eq!(Harness::Claude.instruction_file(), "CLAUDE.md");
        assert_eq!(Harness::Opencode.instruction_file(), "AGENTS.md");
        assert_eq!(Harness::Claude.project_config_ref(), ".claude / CLAUDE.md");
        assert_eq!(
            Harness::Opencode.project_config_ref(),
            ".opencode / AGENTS.md"
        );
        assert_eq!(Harness::Claude.global_config_ref(), "~/.claude");
        assert_eq!(Harness::Opencode.global_config_ref(), "~/.config/opencode");
        assert_eq!(Harness::Claude.source_noun(), "session transcripts");
        assert_eq!(Harness::Opencode.source_noun(), "sessions");
        assert_eq!(
            Harness::Claude.raw_source(),
            "~/.claude/projects transcripts"
        );
        assert_eq!(Harness::Opencode.raw_source(), "opencode database");
        assert_eq!(Harness::Claude.input_unit(), "transcript(s)");
        assert_eq!(Harness::Opencode.input_unit(), "session(s)");
        assert!(Harness::Opencode.inventory_preamble().contains("AGENTS.md"));
        assert!(!Harness::Opencode.inventory_preamble().contains("CLAUDE.md"));
    }

    #[test]
    fn claude_keeps_the_historical_db_name() {
        assert_eq!(Harness::Claude.db_file_name(), "saniwalens.db");
        assert_eq!(Harness::Opencode.db_file_name(), "saniwalens-opencode.db");
        assert_eq!(Harness::Kilocode.db_file_name(), "saniwalens-kilocode.db");
    }

    // ---- PBI 38: the launch shape as data ----

    #[test]
    fn launch_command_runs_each_harness_own_agent() {
        // `claude` opens interactively on the prompt; `opencode run` runs the
        // advisor headless on JSON events (raw tool dumps loggable) with a
        // recognizable session title (see `run_instructions`). kilo is an
        // OpenCode fork, but its TUI takes a prompt rather than the opencode
        // run surface: `kilo --prompt <prompt>` opens the interactive session
        // on the pointer prompt, the experience the claude launch gives
        // (`docs/specs/harness.md`, "`optimize` under opencode").
        assert_eq!(
            Harness::Claude.launch_command(),
            ("claude", &[] as &[&str], RunMode::Interactive)
        );
        assert_eq!(
            Harness::Opencode.launch_command(),
            (
                "opencode",
                &["run", "--format", "json", "--title", "saniwalens optimize"] as &[&str],
                RunMode::Headless
            )
        );
        assert_eq!(
            Harness::Kilocode.launch_command(),
            ("kilo", &["--prompt"] as &[&str], RunMode::Interactive)
        );
    }

    // ---- PBI 42: the headless run-line renderer as data ----

    #[test]
    fn run_line_renderer_hands_each_harness_own_stream() {
        // `opencode run --format json` streams unversioned JSON events, so the
        // seam hands the shell the adapter's lenient renderer; kilo is an
        // OpenCode fork and shares the stream, so a headless kilo run needs no
        // shell edit. Claude's launch opens the interactive TUI
        // (`launch_command`), so it has no headless stream to render.
        let line = r#"{"type":"tool_use","part":{"type":"tool","tool":"read"}}"#;
        let opencode = Harness::Opencode
            .run_line_renderer()
            .expect("headless stream");
        assert_eq!(
            opencode(line),
            opencode::RunEvent::Progress("read".to_string())
        );
        let kilo = Harness::Kilocode
            .run_line_renderer()
            .expect("headless stream");
        assert_eq!(kilo(line), opencode::RunEvent::Progress("read".to_string()));
        assert!(Harness::Claude.run_line_renderer().is_none());
    }

    #[test]
    fn kilocode_tasks_candidates_cover_the_known_editors() {
        // The tasks dir varies with the editor install (Code / Cursor /
        // VSCodium) and the platform, so the list is what the shell probes —
        // in a fixed order, `--projects` always winning over it.
        let candidates = Harness::kilocode_tasks_candidates(Some(Path::new("/tmp/example/home")));
        assert_eq!(candidates.len(), 3);
        for candidate in &candidates {
            assert!(
                candidate
                    .to_string_lossy()
                    .ends_with("kilocode.kilo-code/tasks")
            );
            assert!(candidate.starts_with("/tmp/example/home"));
        }
    }

    #[test]
    fn kilocode_tasks_candidates_without_a_home_are_empty() {
        assert!(Harness::kilocode_tasks_candidates(None).is_empty());
        assert!(Harness::kilocode_tasks_candidates(Some(Path::new("relative/home"))).is_empty());
    }

    #[test]
    fn opencode_db_prefers_an_absolute_data_home() {
        assert_eq!(
            Harness::opencode_db_path(
                Some(Path::new("/tmp/example/data")),
                Some(Path::new("/tmp/example/home"))
            ),
            Some(PathBuf::from("/tmp/example/data/opencode/opencode.db"))
        );
    }

    #[test]
    fn opencode_db_falls_back_to_the_home_share_dir() {
        assert_eq!(
            Harness::opencode_db_path(None, Some(Path::new("/tmp/example/home"))),
            Some(PathBuf::from(
                "/tmp/example/home/.local/share/opencode/opencode.db"
            ))
        );
    }

    #[test]
    fn opencode_db_ignores_a_relative_data_home() {
        assert_eq!(
            Harness::opencode_db_path(
                Some(Path::new("relative/data")),
                Some(Path::new("/tmp/example/home"))
            ),
            Some(PathBuf::from(
                "/tmp/example/home/.local/share/opencode/opencode.db"
            ))
        );
    }

    #[test]
    fn opencode_db_without_a_home_has_no_answer() {
        assert_eq!(Harness::opencode_db_path(None, None), None);
    }

    #[test]
    fn opencode_config_prefers_an_absolute_config_home() {
        assert_eq!(
            Harness::opencode_config_dir(
                Some(Path::new("/tmp/example/config")),
                Some(Path::new("/tmp/example/home"))
            ),
            Some(PathBuf::from("/tmp/example/config/opencode"))
        );
    }

    #[test]
    fn opencode_config_falls_back_to_home_dot_config() {
        assert_eq!(
            Harness::opencode_config_dir(None, Some(Path::new("/tmp/example/home"))),
            Some(PathBuf::from("/tmp/example/home/.config/opencode"))
        );
        assert_eq!(Harness::opencode_config_dir(None, None), None);
    }

    // ---- PBI 05: catalog traversal as data ----

    #[test]
    fn a_new_surface_kind_registers_as_a_site_row() {
        // A kind is a `Site` row: the traversal — the global pass, each root's
        // project pass, and the Scope built for each — never changes when one
        // is added (`pbi/2026-10-29-05-feat-config-catalog-traversal.md`).
        fn brand_new_site(site: &Site, scope: &Scope, _ctx: &mut SiteCtx) -> Vec<Surface> {
            vec![Surface {
                kind: "brand_new".to_string(),
                id: site.path.display().to_string(),
                scope: scope.clone(),
                config_path: site.path.display().to_string(),
                static_tokens: Some(1),
                startup_tokens: Some(1),
                load_mode: LoadMode::OnDemand,
                paths: Vec::new(),
                description: None,
            }]
        }

        let root =
            std::env::temp_dir().join(format!("saniwalens-site-row-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("checkout")).unwrap();
        let checkout = root.join("checkout");

        let global = vec![Site::new(root.join("brand.json"), brand_new_site)];
        let surfaces = catalog_sites(
            &global,
            |root| vec![Site::new(root.join("brand.json"), brand_new_site)],
            &[(checkout.display().to_string(), "alpha".to_string())],
            None,
        );

        assert_eq!(surfaces.len(), 2, "global row + project row");
        assert_eq!(surfaces[0].kind, "brand_new");
        assert_eq!(surfaces[0].scope, Scope::Global);
        assert_eq!(surfaces[1].scope, Scope::Project("alpha".to_string()));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_traversal_skips_roots_that_are_gone_and_loads_the_rest() {
        // Roots come from the store's history; a checkout that has since been
        // deleted contributes nothing, an existing one is loaded once.
        fn label_site(site: &Site, scope: &Scope, _ctx: &mut SiteCtx) -> Vec<Surface> {
            vec![Surface {
                kind: "rule".to_string(),
                id: site.path.display().to_string(),
                scope: scope.clone(),
                config_path: site.path.display().to_string(),
                static_tokens: None,
                startup_tokens: None,
                load_mode: LoadMode::OnDemand,
                paths: Vec::new(),
                description: None,
            }]
        }

        let root =
            std::env::temp_dir().join(format!("saniwalens-site-roots-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("here")).unwrap();

        let surfaces = catalog_sites(
            &[],
            |r| vec![Site::new(r.join("x"), label_site)],
            &[
                (root.join("here").display().to_string(), "a".to_string()),
                (root.join("gone").display().to_string(), "b".to_string()),
            ],
            None,
        );

        assert_eq!(surfaces.len(), 1);
        assert_eq!(surfaces[0].scope, Scope::Project("a".to_string()));
        std::fs::remove_dir_all(&root).unwrap();
    }

    // ---- PBI 10: ingest fingerprints distinguish sub-second edits ----

    /// Stamp `path` with an exact mtime — the production equivalent is the
    /// filesystem clock moving while a transcript is edited.
    fn set_mtime(path: &Path, mtime: std::time::SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    fn fingerprint_fixture(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "saniwalens-fingerprint-test-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_same_second_same_size_edit_changes_the_fingerprint() {
        // Regression pin for the stale skip: whole-second truncation made an
        // edit within the same second invisible when the size did not move,
        // so the transcript was skipped until something else moved the
        // fingerprint (`pbi/2026-11-08-10-fix-fingerprint-stale-skip.md`).
        // Synthetic temp files only — never a real transcript
        // (`.claude/rules/session-data-privacy.md`).
        let dir = fingerprint_fixture("same-second");
        let path = dir.join("transcript.jsonl");
        std::fs::write(&path, "aaaa").unwrap();
        let base = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::new(1_700_000_000, 100_000_000);
        set_mtime(&path, base);
        let before = file_fingerprint(&path).unwrap();
        // Same size, same whole second, sub-second mtime moved.
        std::fs::write(&path, "bbbb").unwrap();
        set_mtime(&path, base + std::time::Duration::from_millis(500));
        let after = file_fingerprint(&path).unwrap();
        assert_ne!(
            before, after,
            "a same-second same-size edit must move the fingerprint"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_untouched_file_keeps_its_fingerprint() {
        // The fast incremental path depends on stability: an unchanged file
        // must fingerprint identically so the skip still fires.
        let dir = fingerprint_fixture("untouched");
        let path = dir.join("transcript.jsonl");
        std::fs::write(&path, "aaaa").unwrap();
        let stamp = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::new(1_700_000_000, 100_000_000);
        set_mtime(&path, stamp);
        assert_eq!(
            file_fingerprint(&path),
            file_fingerprint(&path),
            "an unchanged file must keep its fingerprint"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unstattable_file_has_no_fingerprint() {
        // A file that cannot be stat'ed is ingested unconditionally and never
        // recorded — pinning the `None` contract the skip relies on.
        let missing = std::env::temp_dir().join(format!(
            "saniwalens-fingerprint-test-missing-{}-{:?}.jsonl",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&missing);
        assert_eq!(file_fingerprint(&missing), None);
    }

    // ---- PBI C11: the post-read re-stat gate ----

    #[test]
    fn a_matching_restat_agrees_to_stamp() {
        // No drift between the pre-read stat and the post-read re-stat: the
        // stamp may record what was read.
        assert!(restat_matches(Some((100, 4)), Some((100, 4))));
    }

    #[test]
    fn an_mtime_drift_refuses_the_stamp() {
        // The file moved while it was read: stamping would pin content older
        // than the fingerprint, so the next run must re-ingest instead.
        assert!(!restat_matches(Some((100, 4)), Some((200, 4))));
    }

    #[test]
    fn a_size_drift_refuses_the_stamp() {
        // An append that kept the mtime (coarse clock, racing writer) still
        // refuses: either half of the pair drifting aborts the stamp.
        assert!(!restat_matches(Some((100, 4)), Some((100, 9))));
    }

    #[test]
    fn a_failed_stat_on_either_side_refuses_the_stamp() {
        // Fail closed: a file that cannot be stat'ed before or after the read
        // is never stamped, so it is re-ingested next run, never skipped.
        assert!(!restat_matches(None, Some((100, 4))));
        assert!(!restat_matches(Some((100, 4)), None));
        assert!(!restat_matches(None, None));
    }
}
