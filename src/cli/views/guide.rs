use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

use crate::adapter::harness::{Harness, home_dir};
use crate::core::optimize::{AdvisorLanguage, RunMode};
use crate::core::period::TimeRange;
use crate::core::scope::ScopeFilter;
use crate::session::{Session, SessionEnv, config_file_path, env_roots, resolve_cli_with_language};
use crate::store::Store;

use super::super::report::Format;
use super::super::window::{PeriodArgs, ReadArgs, WindowedDispatch};
use super::super::{doctor, failures, run_analyze};
use super::waste::waste;

/// What `run()` should do with this invocation, decided before clap parses.
pub(crate) enum Entry {
    /// No subcommand (or the explicit `tui`): run the guide. Presets come
    /// from the global flags seen before the decision point.
    Guide {
        harness: Option<String>,
        profile: Option<String>,
        language: Option<String>,
    },
    /// Anything else: normal clap parsing (today's behavior, unchanged).
    Normal,
}

impl Entry {
    /// Build a guide entry from clap's already-parsed globals, for the
    /// explicit `tui` arm (`cli/mod.rs`): the values ride the entry instead
    /// of another parameter, so the shell stays within its line budget.
    pub(crate) fn preset(
        harness: Option<String>,
        profile: Option<String>,
        language: Option<String>,
    ) -> Self {
        Entry::Guide {
            harness,
            profile,
            language,
        }
    }
}

/// Pure argv classifier: `--harness` / `--profile` / `--language` values are
/// presets (both `--flag value` and `--flag=value` forms), `tui`
/// is the explicit entry (flags after it still count as presets), a `-` flag
/// or any other token is a real command.
pub(crate) fn guide_entry(argv: &[String]) -> Entry {
    let mut harness: Option<String> = None;
    let mut profile: Option<String> = None;
    let mut language: Option<String> = None;
    let mut args = argv.iter().skip(1);
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--harness=") {
            if value.is_empty() {
                return Entry::Normal;
            }
            harness = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("--profile=") {
            if value.is_empty() {
                return Entry::Normal;
            }
            profile = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("--language=") {
            if value.is_empty() {
                return Entry::Normal;
            }
            language = Some(value.to_string());
        } else if arg == "--harness" {
            match args.next() {
                // A `-` token in the value position is a flag, not a preset:
                // `--harness --help` must reach clap, not the guide.
                Some(value) if !value.starts_with('-') => harness = Some(value.clone()),
                _ => return Entry::Normal,
            }
        } else if arg == "--profile" {
            match args.next() {
                Some(value) if !value.starts_with('-') => profile = Some(value.clone()),
                _ => return Entry::Normal,
            }
        } else if arg == "--language" {
            match args.next() {
                Some(value) if !value.starts_with('-') => language = Some(value.clone()),
                _ => return Entry::Normal,
            }
        } else if arg != "tui" {
            // A `-` flag (--help, --version, …) or a real subcommand:
            // clap owns this invocation, not the guide.
            return Entry::Normal;
        }
    }
    // End of argv with no subcommand or flag: guide entry. An explicit `tui`
    // needs no separate handling — reaching here means clap has nothing to parse.
    Entry::Guide {
        harness,
        profile,
        language,
    }
}

/// The pre-parse guide check for `run()`: classify argv, and on a guide
/// invocation with both stdio streams on a terminal, hand back the entry to
/// run. `None` means normal clap parsing (today's behavior, unchanged) —
/// either a real command owns the invocation, or there is no terminal and
/// the guide must not start.
pub(crate) fn guide_entry_if_terminal(argv: &[String]) -> Option<Entry> {
    match guide_entry(argv) {
        entry @ Entry::Guide { .. } if terminal_available() => Some(entry),
        _ => None,
    }
}

/// The three reports the guide offers (design doc §2: v0.5.4 scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    Doctor,
    Waste,
    Failures,
}

/// The `--scope` answers the guide offers (design doc Q6: ask once).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopeChoice {
    All,
    Global,
    Project,
}

/// Profile names are TOML bare keys and path-safe segments: ASCII
/// alphanumerics plus `-` and `_` only.
pub(crate) fn profile_name_is_valid(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The reproducible command line for a guided run: printed before the run
/// and reprinted after it, so a guided run is always re-runnable by hand.
/// This string is the guide's machine contract (no JSON golden: the guide
/// has no machine output); the table test above pins every combination.
pub(crate) fn equivalent_command(
    harness: &Harness,
    purpose: Purpose,
    scope: ScopeChoice,
) -> String {
    // The default harness rides bare: every other harness names itself via
    // the single `Harness::label` spelling, so no arm re-spells it.
    let harness_part = if *harness == Harness::Claude {
        String::new()
    } else {
        format!(" --harness {}", harness.label())
    };
    let command = match purpose {
        Purpose::Doctor => "doctor",
        Purpose::Waste => "waste",
        Purpose::Failures => "failures",
    };
    let scope_part = match scope {
        ScopeChoice::All => String::new(),
        ScopeChoice::Global => " --scope global".to_string(),
        ScopeChoice::Project => " --scope project".to_string(),
    };
    format!("saniwalens{harness_part} {command}{scope_part}")
}

/// Whether the guide's inline next step hands off (`--apply`): a headless
/// harness has no dialog partner in the seeded session, so the plan is saved
/// and the harness's own interactive session opens on it; an interactive
/// harness already sits in a dialog, so the plain inline optimize applies its
/// own plan. Reads the harness's run-mode data (`launch_command`), so a new
/// harness lands in the adapter, not in a name match here.
pub(crate) fn next_step_runs_handoff(harness: &Harness) -> bool {
    harness.launch_command().2 == RunMode::Headless
}

/// The post-run next step: `optimize` is out of the guide's scope, but every
/// guided run points at it with the matching `--scope` so the cause can be
/// dug into by hand. Headless-aware: a headless harness hands off (`--apply`),
/// so its line names the flag the inline run actually uses. The table tests
/// below pin all three scopes on both sides of the handoff branch.
pub(crate) fn next_step_line(harness: &Harness, scope: ScopeChoice) -> String {
    let base = match scope {
        ScopeChoice::All => "saniwalens optimize".to_string(),
        ScopeChoice::Global => "saniwalens optimize --scope global".to_string(),
        ScopeChoice::Project => "saniwalens optimize --scope project".to_string(),
    };
    if next_step_runs_handoff(harness) {
        format!("{base} --apply")
    } else {
        base
    }
}

/// One answered prompt: a 0-based pick, or quit (`q`, EOF). SIGINT (Ctrl-C)
/// terminates the process; only `q`/EOF return `Quit` (exit 0).
pub(crate) enum Choice {
    Pick(usize),
    Quit,
}

/// Numbered prompt with a default: empty line takes it, `q` quits, anything
/// else retries with the valid range named. Generic over I/O so tests feed
/// `Cursor` instead of a terminal.
pub(crate) fn ask_choice<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    prompt: &str,
    options: &[&str],
    default: usize,
) -> anyhow::Result<Choice> {
    // Callers guarantee a non-empty menu with a default inside it.
    debug_assert!(!options.is_empty(), "ask_choice needs at least one option");
    debug_assert!(
        default < options.len(),
        "ask_choice default must name an option"
    );
    writeln!(output, "{prompt}")?;
    for (i, option) in options.iter().enumerate() {
        writeln!(output, "[{}] {option}", i + 1)?;
    }
    loop {
        write!(output, "> ")?;
        output.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(Choice::Quit);
        }
        let line = line.trim();
        if line.is_empty() {
            return Ok(Choice::Pick(default));
        }
        if line.eq_ignore_ascii_case("q") {
            return Ok(Choice::Quit);
        }
        match line.parse::<usize>() {
            Ok(n) if (1..=options.len()).contains(&n) => return Ok(Choice::Pick(n - 1)),
            _ => writeln!(output, "1-{} か q で答えてください", options.len())?,
        }
    }
}

/// New file text with `default_harness` set. Line-based edit: everything else
/// (comments, profiles, blank lines) is preserved verbatim. No Serialize
/// round-trip ― `FileConfig` is Deserialize-only, and a rewrite would eat
/// the user's comments.
pub(crate) fn render_default_harness(existing: Option<&str>, harness: &str) -> String {
    const HEADER: &str = "# saniwalens defaults (written by the guided TUI).";
    let line = format!("default_harness = \"{harness}\"");
    let Some(text) = existing else {
        return format!("{HEADER}\n{line}\n");
    };
    let mut out: Vec<String> = Vec::new();
    let mut done = false;
    let mut in_table = false;
    for raw in text.lines() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with('[') {
            in_table = true;
        }
        let is_target = !done
            && !in_table
            && trimmed.starts_with("default_harness")
            && trimmed["default_harness".len()..]
                .trim_start()
                .starts_with('=');
        out.push(if is_target {
            line.clone()
        } else {
            raw.to_string()
        });
        done = done || is_target;
    }
    if !done {
        let pos = out
            .iter()
            .position(|l| l.trim_start().starts_with('['))
            .unwrap_or(out.len());
        out.insert(pos, line);
    }
    let mut rendered = out.join("\n");
    rendered.push('\n');
    rendered
}

/// A `[table]` header's dotted path, tolerant of inner whitespace
/// (`[ profiles.work ]`). Array-of-tables (`[[x]]`) and malformed lines
/// are not headers: they still close an open section but never match one.
fn table_header(line: &str) -> Option<&str> {
    let inner = line.trim_start().strip_prefix('[')?.strip_suffix(']')?;
    if inner.starts_with('[') || inner.trim().is_empty() {
        return None;
    }
    Some(inner.trim())
}

/// New file text with `[profiles.<name>]` carrying `harness`. Same
/// line-preserving contract as `render_default_harness`.
pub(crate) fn render_profile(existing: Option<&str>, name: &str, harness: &str) -> String {
    const HEADER: &str = "# saniwalens defaults (written by the guided TUI).";
    let header = format!("[profiles.{name}]");
    let target = format!("profiles.{name}");
    let line = format!("harness = \"{harness}\"");
    let Some(text) = existing else {
        return format!("{HEADER}\n{header}\n{line}\n");
    };
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut done = false;
    for raw in text.lines() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with('[') {
            // Close the section at the next header; insert the harness line
            // if the section had none.
            if in_section && !done {
                out.push(line.clone());
                done = true;
            }
            in_section = table_header(raw).is_some_and(|path| path == target);
        }
        let is_target = in_section
            && !done
            && trimmed.starts_with("harness")
            && trimmed["harness".len()..].trim_start().starts_with('=');
        out.push(if is_target {
            line.clone()
        } else {
            raw.to_string()
        });
        done = done || is_target;
    }
    if in_section && !done {
        out.push(line.clone());
        done = true;
    }
    if !done {
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(header);
        out.push(line);
    }
    let mut rendered = out.join("\n");
    rendered.push('\n');
    rendered
}

/// The post-run save conversation: 3-choice (skip / default / profile),
/// profile-name prompt with validation retry, path + rendered text shown,
/// y/N confirm. Returns the text to write, or `None` for "don't save".
/// No filesystem access here ― the caller reads before and writes after ―
/// so `Cursor`-driven tests cover every branch without touching disk.
pub(crate) fn ask_save<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    existing: Option<&str>,
    harness: &str,
    path: &Path,
) -> anyhow::Result<Option<String>> {
    writeln!(
        output,
        "使っているエージェント ({harness}) の設定を保存しますか? (config: {})",
        path.display()
    )?;
    let pick = ask_choice(
        input,
        output,
        "保存先を選んでください",
        &[
            "保存しない",
            "既定のharnessにする (default_harness)",
            "名前付きで保存する (--profile 名 で呼び出せる)",
        ],
        0,
    )?;
    let rendered = match pick {
        Choice::Quit | Choice::Pick(0) => return Ok(None),
        Choice::Pick(1) => render_default_harness(existing, harness),
        _ => {
            let name = ask_profile_name(input, output)?;
            let Some(name) = name else {
                return Ok(None);
            };
            render_profile(existing, &name, harness)
        }
    };
    writeln!(output, "書き込む内容:\n{rendered}")?;
    writeln!(output, "保存しますか? [y/N]")?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    if line.trim().eq_ignore_ascii_case("y") {
        Ok(Some(rendered))
    } else {
        Ok(None)
    }
}

/// Free-text profile-name prompt with validation retry; `q`/EOF quits the
/// save (not the guide).
fn ask_profile_name<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
) -> anyhow::Result<Option<String>> {
    loop {
        write!(output, "profile名 (英数・-・_)? [qでやめる] ")?;
        output.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let name = line.trim();
        if name.eq_ignore_ascii_case("q") {
            return Ok(None);
        }
        if profile_name_is_valid(name) {
            return Ok(Some(name.to_string()));
        }
        writeln!(output, "使えるのは英数・-・_だけです")?;
    }
}

/// y/N confirmation: `y` (any case) means yes; anything else — `n`,
/// empty, `q`, EOF — means no. `q` never quits the guide here; only the
/// y/N answer matters. Generic over I/O so `Cursor`-driven tests cover
/// every answer without touching stdin.
pub(crate) fn ask_yes_no<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    prompt: &str,
) -> anyhow::Result<bool> {
    writeln!(output, "{prompt}")?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Ok(false);
    }
    Ok(line.trim().eq_ignore_ascii_case("y"))
}

/// Thin filesystem wrapper: create parents, compare against the snapshot the
/// dialog showed, write to a pid-unique sibling temp file, then rename into
/// place — so a mid-write kill leaves either the old or the new config, never
/// a truncation. `expected` is the file's content when the dialog read it
/// (`None` when absent): a mismatch means someone else wrote first, and the
/// write aborts with a conflict error instead of silently clobbering it.
/// A crash leaves the unique `.toml.tmp` sibling behind (deliberate residue,
/// self-evident on next run; unique names never collide across processes).
fn persist_config(path: &Path, text: &str, expected: Option<&str>) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let current = match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    if current.as_deref() != expected {
        anyhow::bail!(
            "refusing to save {}: it changed since the dialog read it (re-run the guide to merge)",
            path.display()
        );
    }
    // Pid + clock + sequence: two guides (or two test threads in one
    // process) never share a sibling, unlike the old fixed `.toml.tmp` name
    // (`private_temp_path` in `super::super` is the same idea for temp dirs).
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let sibling = format!("{}.{}.{}.toml.tmp", std::process::id(), nanos, seq);
    let tmp = path.with_extension(sibling);
    std::fs::write(&tmp, text)?;
    // Unix rename replaces atomically, so no remove-first window exists in
    // which a crash would leave no config at all. Windows rename cannot
    // replace, so the remove stays there (documented platform gap).
    #[cfg(windows)]
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Shared open + report call for the scoped table views (waste, failures).
/// Doctor keeps its own open: its `--format` set is narrower (table|json).
/// Returns the scope filter the report used, so the guide's inline optimize
/// reuses the identical filter without re-parsing.
fn run_scoped(
    dispatch: &WindowedDispatch,
    scope_flag: Option<&str>,
    period: &PeriodArgs,
    read: &ReadArgs,
    report: fn(&Store, &Harness, &ScopeFilter, TimeRange, Format) -> anyhow::Result<()>,
) -> anyhow::Result<ScopeFilter> {
    let opened = dispatch.open_scoped(scope_flag, None, period, read, run_analyze)?;
    let (windowed, filter) = opened.into_parts();
    report(
        &windowed.sess.store,
        &windowed.sess.harness,
        &filter,
        windowed.window,
        windowed.format,
    )?;
    Ok(filter)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_guide<R: BufRead, W: Write>(
    initial: &Harness,
    language: AdvisorLanguage,
    has_preset: bool,
    config_db: Option<&Path>,
    env: &SessionEnv,
    input: &mut R,
    output: &mut W,
    config_path: Option<&Path>,
) -> anyhow::Result<()> {
    // Step 1 (design §2: harness → purpose → scope → execution): with no
    // preset the harness is an asked choice defaulting to the resolved value;
    // a preset skips the question and keeps the resolved value.
    let mut current = *initial;
    if !has_preset {
        let options: Vec<&str> = Harness::ALL.iter().map(|harness| harness.label()).collect();
        let default = Harness::ALL
            .iter()
            .position(|harness| harness == initial)
            .unwrap_or(0);
        match ask_choice(
            input,
            output,
            "使っているエージェントを選んでください（q で終了）",
            &options,
            default,
        )? {
            Choice::Quit => return Ok(()),
            Choice::Pick(i) => current = Harness::ALL[i],
        }
    }
    writeln!(output, "使っているエージェント: {}", current.label())?;

    let period = PeriodArgs {
        since: None,
        until: None,
        last: None,
    };
    let read = ReadArgs {
        frozen: false,
        db: None,
    };
    loop {
        // Step 2: purpose.
        let purpose = match ask_choice(
            input,
            output,
            "やりたいことを選んでください（q で終了）",
            &[
                "健康診断 (doctor)",
                "無駄を探す (waste)",
                "失敗を調べる (failures)",
            ],
            0,
        )? {
            Choice::Quit => return Ok(()),
            Choice::Pick(0) => Purpose::Doctor,
            Choice::Pick(1) => Purpose::Waste,
            _ => Purpose::Failures,
        };
        // Step 3: scope.
        let scope = match ask_choice(
            input,
            output,
            "範囲を選んでください",
            &["全体", "global", "project"],
            0,
        )? {
            Choice::Quit => return Ok(()),
            Choice::Pick(0) => ScopeChoice::All,
            Choice::Pick(1) => ScopeChoice::Global,
            _ => ScopeChoice::Project,
        };
        // Step 4: show the command, run it, reprint it.
        let command = equivalent_command(&current, purpose, scope);
        writeln!(output, "実行: {command}")?;
        let scope_flag: Option<&str> = match scope {
            ScopeChoice::All => None,
            ScopeChoice::Global => Some("global"),
            ScopeChoice::Project => Some("project"),
        };
        // The selected harness owns the report: a fresh dispatch per run
        // borrows it, so the store guard sees the chosen harness, not the
        // one resolved before the question.
        let dispatch = WindowedDispatch {
            harness: &current,
            config_db,
            env,
        };
        let filter: ScopeFilter = match purpose {
            Purpose::Doctor => {
                let opened = dispatch.open_scoped_table_json(
                    scope_flag,
                    None,
                    &period,
                    &read,
                    run_analyze,
                )?;
                let (windowed, filter) = opened.into_parts();
                doctor(
                    &windowed.sess.store,
                    &windowed.sess.harness,
                    &filter,
                    windowed.window,
                    windowed.format,
                )?;
                filter
            }
            Purpose::Waste => run_scoped(&dispatch, scope_flag, &period, &read, waste)?,
            Purpose::Failures => run_scoped(&dispatch, scope_flag, &period, &read, failures)?,
        };
        writeln!(output, "実行したコマンド: {command}")?;
        writeln!(
            output,
            "次の一手: {} で原因の深掘りと対策提案",
            next_step_line(&current, scope)
        )?;
        if ask_yes_no(input, output, "次の一手を実行しますか? [y/N]")? {
            let line = next_step_line(&current, scope);
            writeln!(output, "実行: {line}")?;
            // Same harness, same scope, and same language as the resolved
            // invocation: the report already refreshed the store, so frozen
            // skips a second analyze. A headless harness has no dialog partner
            // in the seeded session, so the next step hands off (--apply): the
            // plan is saved and the harness's own interactive session opens on
            // it. An interactive harness already sits in a dialog, so the
            // plain inline optimize applies its own plan. The branch reads
            // the harness's run-mode data, never its name. A failed launch
            // must not end the guide.
            let attempted = guide_session(&current, language, config_db, env).and_then(|sess| {
                if next_step_runs_handoff(&current) {
                    super::super::handoff::run_apply(&sess, None, &filter, true, None, env)
                } else {
                    super::super::optimize(&sess, None, &filter, true, false, false, None)
                }
            });
            if let Err(e) = attempted {
                writeln!(output, "optimize に失敗しました: {e:#}")?;
            }
        }
        if let Some(path) = config_path {
            let existing = match std::fs::read_to_string(path) {
                Ok(text) => Some(text),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
            };
            if let Some(text) = ask_save(input, output, existing.as_deref(), current.label(), path)?
            {
                persist_config(path, &text, existing.as_deref())?;
            }
        }
        writeln!(output, "もう1件見る? [y/N]")?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        if !line.trim().eq_ignore_ascii_case("y") {
            return Ok(());
        }
    }
}

/// Whether the guide UI may start: both stdio streams are TTYs, or the
/// test-only override is set. `SANIWALENS_TEST_FORCE_TTY` exists so the
/// scripted E2E suite (`tests/guide_flow.rs`) can drive the real binary
/// without a pty; it is unsupported and must never be set by users.
pub(crate) fn terminal_available() -> bool {
    std::env::var_os("SANIWALENS_TEST_FORCE_TTY").is_some()
        || (std::io::stdin().is_terminal() && std::io::stdout().is_terminal())
}

/// The guide's inline next-step session: the same resolve-then-open shape as
/// the normal `optimize` arm (`cli/mod.rs`) — resolve the store path, open it,
/// then carry the resolved advisor language on the session, so the inline run
/// renders the same language without naming the flag.
fn guide_session(
    current: &Harness,
    language: AdvisorLanguage,
    config_db: Option<&Path>,
    env: &SessionEnv,
) -> anyhow::Result<Session> {
    let path = Session::resolve_path(current, None, config_db, env)?;
    let mut sess = Session::open_resolved(current, path)?;
    sess.language = language;
    Ok(sess)
}

pub(crate) fn run_guide_entry(entry: Entry) -> anyhow::Result<()> {
    let Entry::Guide {
        harness,
        profile,
        language,
    } = entry
    else {
        anyhow::bail!("run_guide_entry needs a guide entry");
    };
    let roots = env_roots();
    let env = SessionEnv {
        state_home: roots.state_home.as_deref(),
        home: home_dir(),
        config_home: roots.config_home.as_deref(),
    };
    let resolved = resolve_cli_with_language(
        harness.as_deref(),
        profile.as_deref(),
        language.as_deref(),
        &env,
    )?;
    let has_preset = harness.is_some() || profile.is_some();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let config_path = config_file_path(roots.config_home.as_deref(), home_dir());
    run_guide(
        &resolved.harness,
        resolved.language,
        has_preset,
        resolved.config_db.as_deref(),
        &env,
        &mut stdin.lock(),
        &mut stdout.lock(),
        config_path.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_invocation_is_a_guide_with_no_preset() {
        let argv = vec!["saniwalens".to_string()];
        assert!(matches!(
            guide_entry(&argv),
            Entry::Guide {
                harness: None,
                profile: None,
                language: None
            }
        ));
    }

    #[test]
    fn harness_flag_becomes_the_preset_not_a_subcommand() {
        let argv = vec![
            "saniwalens".to_string(),
            "--harness".to_string(),
            "kilocode".to_string(),
        ];
        assert!(matches!(
            guide_entry(&argv),
            Entry::Guide { harness: Some(h), profile: None, language: None } if h == "kilocode"
        ));
    }

    #[test]
    fn tui_is_an_explicit_guide_entry() {
        let argv = vec!["saniwalens".to_string(), "tui".to_string()];
        assert!(matches!(guide_entry(&argv), Entry::Guide { .. }));
    }

    #[test]
    fn flags_after_tui_still_count_as_presets() {
        let argv = vec![
            "saniwalens".to_string(),
            "tui".to_string(),
            "--harness".to_string(),
            "kilocode".to_string(),
        ];
        assert!(matches!(
            guide_entry(&argv),
            Entry::Guide { harness: Some(h), profile: None, language: None } if h == "kilocode"
        ));
    }

    #[test]
    fn flags_around_tui_combine_into_both_presets() {
        let argv = vec![
            "saniwalens".to_string(),
            "--harness".to_string(),
            "claude".to_string(),
            "tui".to_string(),
            "--profile".to_string(),
            "w".to_string(),
        ];
        assert!(matches!(
            guide_entry(&argv),
            Entry::Guide { harness: Some(h), profile: Some(p), language: None } if h == "claude" && p == "w"
        ));
    }

    #[test]
    fn a_subcommand_after_tui_is_not_a_guide() {
        let argv = vec![
            "saniwalens".to_string(),
            "tui".to_string(),
            "doctor".to_string(),
        ];
        assert!(matches!(guide_entry(&argv), Entry::Normal));
    }

    #[test]
    fn an_unknown_flag_after_tui_is_not_a_guide() {
        let argv = vec![
            "saniwalens".to_string(),
            "tui".to_string(),
            "--db".to_string(),
            "x".to_string(),
        ];
        assert!(matches!(guide_entry(&argv), Entry::Normal));
    }

    #[test]
    fn a_real_subcommand_is_not_a_guide() {
        for argv in [
            vec!["saniwalens", "doctor"],
            vec!["saniwalens", "--help"],
            vec!["saniwalens", "doctor", "--scope", "global"],
        ] {
            let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
            assert!(matches!(guide_entry(&owned), Entry::Normal), "{argv:?}");
        }
    }

    #[test]
    fn trailing_harness_without_a_value_is_normal() {
        let argv = vec!["saniwalens".to_string(), "--harness".to_string()];
        assert!(matches!(guide_entry(&argv), Entry::Normal));
    }

    #[test]
    fn trailing_profile_without_a_value_is_normal() {
        let argv = vec!["saniwalens".to_string(), "--profile".to_string()];
        assert!(matches!(guide_entry(&argv), Entry::Normal));
    }

    #[test]
    fn harness_equals_form_becomes_the_preset() {
        let argv = vec!["saniwalens".to_string(), "--harness=kilocode".to_string()];
        assert!(matches!(
            guide_entry(&argv),
            Entry::Guide { harness: Some(h), profile: None, language: None } if h == "kilocode"
        ));
    }

    #[test]
    fn profile_equals_form_after_tui_becomes_the_preset() {
        let argv = vec![
            "saniwalens".to_string(),
            "tui".to_string(),
            "--profile=w".to_string(),
        ];
        assert!(matches!(
            guide_entry(&argv),
            Entry::Guide { harness: None, profile: Some(p), language: None } if p == "w"
        ));
    }

    #[test]
    fn empty_harness_equals_value_is_normal() {
        let argv = vec!["saniwalens".to_string(), "--harness=".to_string()];
        assert!(matches!(guide_entry(&argv), Entry::Normal));
    }

    #[test]
    fn language_flag_becomes_the_preset_not_a_subcommand() {
        let argv = vec![
            "saniwalens".to_string(),
            "--language".to_string(),
            "ja".to_string(),
        ];
        assert!(matches!(
            guide_entry(&argv),
            Entry::Guide { harness: None, profile: None, language: Some(l) } if l == "ja"
        ));
    }

    #[test]
    fn language_equals_form_after_tui_becomes_the_preset() {
        let argv = vec![
            "saniwalens".to_string(),
            "tui".to_string(),
            "--language=ja".to_string(),
        ];
        assert!(matches!(
            guide_entry(&argv),
            Entry::Guide { harness: None, profile: None, language: Some(l) } if l == "ja"
        ));
    }

    #[test]
    fn language_combines_with_the_harness_preset() {
        let argv = vec![
            "saniwalens".to_string(),
            "--harness".to_string(),
            "kilocode".to_string(),
            "--language".to_string(),
            "ja".to_string(),
        ];
        assert!(matches!(
            guide_entry(&argv),
            Entry::Guide { harness: Some(h), profile: None, language: Some(l) }
                if h == "kilocode" && l == "ja"
        ));
    }

    #[test]
    fn trailing_language_without_a_value_is_normal() {
        let argv = vec!["saniwalens".to_string(), "--language".to_string()];
        assert!(matches!(guide_entry(&argv), Entry::Normal));
    }

    #[test]
    fn empty_language_equals_value_is_normal() {
        let argv = vec!["saniwalens".to_string(), "--language=".to_string()];
        assert!(matches!(guide_entry(&argv), Entry::Normal));
    }

    #[test]
    fn a_flag_in_the_harness_value_position_is_normal() {
        // `--harness --help` must reach clap's help, not the guide: a value
        // starting with `-` is a flag, not a preset.
        for argv in [
            vec!["saniwalens", "--harness", "--help"],
            vec!["saniwalens", "--profile", "--version"],
            vec!["saniwalens", "--language", "--help"],
            vec!["saniwalens", "--harness", "-x"],
            vec!["saniwalens", "tui", "--harness", "--help"],
        ] {
            let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
            assert!(matches!(guide_entry(&owned), Entry::Normal), "{argv:?}");
        }
    }

    #[test]
    fn equivalent_command_covers_the_full_matrix() {
        let harnesses = [
            (Harness::Claude, ""),
            (Harness::Opencode, " --harness opencode"),
            (Harness::Kilocode, " --harness kilocode"),
        ];
        let purposes = [
            ("doctor", Purpose::Doctor),
            ("waste", Purpose::Waste),
            ("failures", Purpose::Failures),
        ];
        let scopes = [
            ("", ScopeChoice::All),
            (" --scope global", ScopeChoice::Global),
            (" --scope project", ScopeChoice::Project),
        ];
        let mut n = 0;
        for (harness, h) in &harnesses {
            for (command, purpose) in &purposes {
                for (s, scope) in &scopes {
                    n += 1;
                    assert_eq!(
                        equivalent_command(harness, *purpose, *scope),
                        format!("saniwalens{h} {command}{s}"),
                    );
                }
            }
        }
        assert_eq!(n, 27, "3 harnesses x 3 purposes x 3 scopes");
    }

    #[test]
    fn equivalent_command_literal_goldens_pin_the_harness_spelling() {
        // Byte-level pins for the harness prefix: the refactor routes this
        // through `Harness::label`, and these literals must not move.
        assert_eq!(
            equivalent_command(&Harness::Claude, Purpose::Doctor, ScopeChoice::All),
            "saniwalens doctor"
        );
        assert_eq!(
            equivalent_command(&Harness::Opencode, Purpose::Waste, ScopeChoice::Global),
            "saniwalens --harness opencode waste --scope global"
        );
        assert_eq!(
            equivalent_command(&Harness::Kilocode, Purpose::Failures, ScopeChoice::Project),
            "saniwalens --harness kilocode failures --scope project"
        );
    }

    #[test]
    fn next_step_line_points_at_optimize_with_the_matching_scope() {
        for harness in [&Harness::Claude, &Harness::Kilocode] {
            assert_eq!(
                next_step_line(harness, ScopeChoice::All),
                "saniwalens optimize"
            );
            assert_eq!(
                next_step_line(harness, ScopeChoice::Global),
                "saniwalens optimize --scope global"
            );
            assert_eq!(
                next_step_line(harness, ScopeChoice::Project),
                "saniwalens optimize --scope project"
            );
        }
    }

    #[test]
    fn next_step_line_appends_apply_for_a_headless_harness() {
        assert_eq!(
            next_step_line(&Harness::Opencode, ScopeChoice::All),
            "saniwalens optimize --apply"
        );
        assert_eq!(
            next_step_line(&Harness::Opencode, ScopeChoice::Global),
            "saniwalens optimize --scope global --apply"
        );
        assert_eq!(
            next_step_line(&Harness::Opencode, ScopeChoice::Project),
            "saniwalens optimize --scope project --apply"
        );
    }

    #[test]
    fn next_step_handoff_follows_the_harness_run_mode() {
        // PBI BDD: a headless harness hands off (--apply), an interactive one
        // runs the plain inline optimize. The branch reads the harness's
        // run-mode data, never its name: one arm per mode, pinned for every
        // harness.
        assert!(next_step_runs_handoff(&Harness::Opencode));
        assert!(!next_step_runs_handoff(&Harness::Claude));
        assert!(!next_step_runs_handoff(&Harness::Kilocode));
    }

    #[test]
    fn empty_answer_takes_the_default() {
        let mut input = std::io::Cursor::new("\n");
        let mut output = Vec::new();
        assert!(matches!(
            ask_choice(&mut input, &mut output, "prompt", &["a", "b"], 1).unwrap(),
            Choice::Pick(1)
        ));
    }

    #[test]
    fn q_quits_and_eof_quits() {
        let mut output = Vec::new();
        for stdin in ["q\n", "Q\n", ""] {
            let mut input = std::io::Cursor::new(stdin);
            assert!(
                matches!(
                    ask_choice(&mut input, &mut output, "p", &["a"], 0).unwrap(),
                    Choice::Quit
                ),
                "{stdin:?}"
            );
        }
    }

    #[test]
    fn out_of_range_retries_then_accepts() {
        let mut input = std::io::Cursor::new("9\n2\n");
        let mut output = Vec::new();
        assert!(matches!(
            ask_choice(&mut input, &mut output, "p", &["a", "b"], 0).unwrap(),
            Choice::Pick(1)
        ));
        let shown = String::from_utf8(output).unwrap();
        assert!(
            shown.contains("1-2"),
            "retry hint names the valid range: {shown}"
        );
    }

    #[test]
    fn no_preset_asks_harness_first_and_q_quits_before_any_report() {
        // With no preset the guide asks which harness to use first; `q`
        // there quits before any report or store work.
        let harness = Harness::Claude;
        let env = SessionEnv {
            state_home: None,
            home: Some("/nonexistent"),
            config_home: None,
        };
        let mut input = std::io::Cursor::new("q\n");
        let mut output = Vec::new();
        run_guide(
            &harness,
            AdvisorLanguage::En,
            false,
            None,
            &env,
            &mut input,
            &mut output,
            None,
        )
        .unwrap();
        let shown = String::from_utf8(output).unwrap();
        assert!(
            shown.contains("エージェントを選んで"),
            "the harness question comes first: {shown}"
        );
        assert!(!shown.contains("実行:"), "no report runs: {shown}");
    }

    #[test]
    fn no_preset_pick_opencode_then_q_at_purpose_runs_nothing() {
        // Choosing 2 (opencode) then `q` at purpose quits without running;
        // the harness prompt appeared and no command was echoed.
        let harness = Harness::Claude;
        let env = SessionEnv {
            state_home: None,
            home: Some("/nonexistent"),
            config_home: None,
        };
        let mut input = std::io::Cursor::new("2\nq\n");
        let mut output = Vec::new();
        run_guide(
            &harness,
            AdvisorLanguage::En,
            false,
            None,
            &env,
            &mut input,
            &mut output,
            None,
        )
        .unwrap();
        let shown = String::from_utf8(output).unwrap();
        assert!(
            shown.contains("エージェントを選んで"),
            "the harness prompt appeared: {shown}"
        );
        assert!(
            shown.contains("使っているエージェント: opencode"),
            "the selected harness is shown: {shown}"
        );
        assert!(!shown.contains("実行:"), "no report runs: {shown}");
    }

    #[test]
    fn preset_skips_the_harness_question() {
        // With a preset the guide uses the resolved harness directly:
        // no question is asked, only the display line is shown.
        let harness = Harness::Kilocode;
        let env = SessionEnv {
            state_home: None,
            home: Some("/nonexistent"),
            config_home: None,
        };
        let mut input = std::io::Cursor::new("q\n");
        let mut output = Vec::new();
        run_guide(
            &harness,
            AdvisorLanguage::En,
            true,
            None,
            &env,
            &mut input,
            &mut output,
            None,
        )
        .unwrap();
        let shown = String::from_utf8(output).unwrap();
        assert!(
            !shown.contains("エージェントを選んで"),
            "no harness question with a preset: {shown}"
        );
        assert!(
            shown.contains("使っているエージェント: kilocode"),
            "the resolved harness is shown: {shown}"
        );
        assert!(
            shown.contains("健康診断"),
            "the purpose prompt is reached: {shown}"
        );
        assert!(!shown.contains("実行:"), "no report runs: {shown}");
    }

    #[test]
    fn quitting_at_the_first_prompt_runs_no_report() {
        // No store, no harness roots, no dispatch work: the harness default
        // then `q` at purpose exits before any report I/O. The store value
        // is never read on this path.
        let harness = Harness::Claude;
        let env = SessionEnv {
            state_home: None,
            home: Some("/nonexistent"),
            config_home: None,
        };
        let mut input = std::io::Cursor::new("\nq\n");
        let mut output = Vec::new();
        run_guide(
            &harness,
            AdvisorLanguage::En,
            false,
            None,
            &env,
            &mut input,
            &mut output,
            None,
        )
        .unwrap();
        let shown = String::from_utf8(output).unwrap();
        assert!(shown.contains("健康診断"));
        assert!(!shown.contains("実行:"));
    }

    #[test]
    fn non_claude_default_is_shown_when_empty_input_keeps_it() {
        // The harness prompt defaults to the resolved value, not the first
        // choice: empty input with an opencode resolution shows opencode
        // and quits at purpose before any report.
        let harness = Harness::Opencode;
        let env = SessionEnv {
            state_home: None,
            home: Some("/nonexistent"),
            config_home: None,
        };
        let mut input = std::io::Cursor::new("\nq\n");
        let mut output = Vec::new();
        run_guide(
            &harness,
            AdvisorLanguage::En,
            false,
            None,
            &env,
            &mut input,
            &mut output,
            None,
        )
        .unwrap();
        let shown = String::from_utf8(output).unwrap();
        assert!(
            shown.contains("使っているエージェント: opencode"),
            "the resolved default is kept: {shown}"
        );
        assert!(!shown.contains("実行:"), "no report runs: {shown}");
    }

    #[test]
    fn default_harness_save_builds_a_new_file_from_nothing() {
        assert_eq!(
            render_default_harness(None, "kilocode"),
            "# saniwalens defaults (written by the guided TUI).\ndefault_harness = \"kilocode\"\n",
        );
    }

    #[test]
    fn default_harness_save_replaces_only_its_own_line() {
        let existing =
            "# keep me\ndefault_harness = \"claude\"\n[profiles.work]\nharness = \"opencode\"\n";
        let out = render_default_harness(Some(existing), "kilocode");
        assert!(out.contains("# keep me\n"), "comments survive: {out}");
        assert!(
            out.contains("default_harness = \"kilocode\"\n"),
            "value replaced: {out}"
        );
        assert!(
            out.contains("[profiles.work]\nharness = \"opencode\"\n"),
            "other sections untouched: {out}"
        );
        assert_eq!(
            out.matches("default_harness").count(),
            1,
            "no duplication: {out}"
        );
    }

    #[test]
    fn default_harness_save_inserts_before_the_first_table() {
        let existing = "[profiles.work]\nharness = \"opencode\"\n";
        let out = render_default_harness(Some(existing), "claude");
        assert!(
            out.starts_with("default_harness = \"claude\"\n"),
            "top-level, before [profiles]: {out}"
        );
    }

    #[test]
    fn profile_save_appends_a_new_section() {
        let out = render_profile(None, "work", "kilocode");
        assert!(
            out.contains("[profiles.work]\nharness = \"kilocode\"\n"),
            "{out}"
        );
    }

    #[test]
    fn profile_save_updates_the_harness_line_inside_its_section() {
        let existing = "default_harness = \"claude\"\n[profiles.work]\nharness = \"opencode\"\n[profiles.home]\nharness = \"claude\"\n";
        let out = render_profile(Some(existing), "work", "kilocode");
        assert!(
            out.contains("[profiles.work]\nharness = \"kilocode\"\n"),
            "{out}"
        );
        assert!(
            out.contains("[profiles.home]\nharness = \"claude\"\n"),
            "other profiles untouched: {out}"
        );
    }

    #[test]
    fn profile_save_updates_a_whitespace_padded_header_in_place() {
        let existing = "[ profiles.work ]\nharness = \"opencode\"\n";
        let out = render_profile(Some(existing), "work", "kilocode");
        assert!(
            out.contains("harness = \"kilocode\"\n"),
            "harness line updated: {out}"
        );
        assert_eq!(
            out.matches("[profiles.work]").count() + out.matches("[ profiles.work ]").count(),
            1,
            "no duplicate table: {out}"
        );
    }

    #[test]
    fn profile_names_allow_only_ascii_word_chars_and_dashes() {
        assert!(profile_name_is_valid("work"));
        assert!(profile_name_is_valid("my-profile_2"));
        assert!(!profile_name_is_valid(""));
        assert!(!profile_name_is_valid("my profile"));
        assert!(!profile_name_is_valid("../../etc"));
    }

    #[test]
    fn ask_save_prompt_names_the_harness_label() {
        // The save prompt must name whose setting it is: after a long plan
        // output "この設定" alone reads as the plan just shown, but the
        // prompt means the harness choice.
        for harness in ["claude", "opencode", "kilocode"] {
            let mut input = std::io::Cursor::new("1\n");
            let mut output = Vec::new();
            let path = std::path::Path::new("/tmp/saniwalens-test-config.toml");
            ask_save(&mut input, &mut output, None, harness, path).unwrap();
            let shown = String::from_utf8(output).unwrap();
            assert!(
                shown.contains(harness),
                "the prompt names the harness ({harness}): {shown}"
            );
        }
    }

    #[test]
    fn ask_save_skip_returns_none() {
        let mut input = std::io::Cursor::new("1\n");
        let mut output = Vec::new();
        let path = std::path::Path::new("/tmp/saniwalens-test-config.toml");
        let out = ask_save(&mut input, &mut output, None, "kilocode", path).unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn ask_save_default_choice_with_y_writes_default_harness() {
        let mut input = std::io::Cursor::new("2\ny\n");
        let mut output = Vec::new();
        let path = std::path::Path::new("/tmp/saniwalens-test-config.toml");
        let out = ask_save(&mut input, &mut output, None, "kilocode", path)
            .unwrap()
            .expect("y confirms the save");
        assert!(out.contains("default_harness = \"kilocode\"\n"), "{out}");
    }

    #[test]
    fn ask_save_profile_choice_retries_an_invalid_name() {
        let mut input = std::io::Cursor::new("3\nbad name\nwork\ny\n");
        let mut output = Vec::new();
        let path = std::path::Path::new("/tmp/saniwalens-test-config.toml");
        let out = ask_save(&mut input, &mut output, None, "kilocode", path)
            .unwrap()
            .expect("y confirms the save");
        assert!(
            out.contains("[profiles.work]\nharness = \"kilocode\"\n"),
            "{out}"
        );
        let shown = String::from_utf8(output).unwrap();
        assert!(shown.contains("使えるのは英数"), "{shown}");
    }

    #[test]
    fn ask_save_q_at_choice_returns_none() {
        let mut input = std::io::Cursor::new("q\n");
        let mut output = Vec::new();
        let path = std::path::Path::new("/tmp/saniwalens-test-config.toml");
        let out = ask_save(&mut input, &mut output, None, "kilocode", path).unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn ask_save_n_at_confirm_returns_none() {
        let mut input = std::io::Cursor::new("2\nn\n");
        let mut output = Vec::new();
        let path = std::path::Path::new("/tmp/saniwalens-test-config.toml");
        let out = ask_save(&mut input, &mut output, None, "kilocode", path).unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn ask_save_profile_choice_then_q_at_name_returns_none() {
        // Picks "profile", then quits at the name prompt: no write text and
        // no confirm shown.
        let mut input = std::io::Cursor::new("3\nq\n");
        let mut output = Vec::new();
        let path = std::path::Path::new("/tmp/saniwalens-test-config.toml");
        let out = ask_save(&mut input, &mut output, None, "kilocode", path).unwrap();
        assert!(out.is_none());
        let shown = String::from_utf8(output).unwrap();
        assert!(!shown.contains("保存しますか? [y/N]"), "{shown}");
    }

    #[test]
    fn ask_save_profile_choice_then_eof_at_confirm_returns_none() {
        // Picks "profile", gives a valid name, then EOF at the y/N confirm:
        // nothing is written.
        let mut input = std::io::Cursor::new("3\nwork\n");
        let mut output = Vec::new();
        let path = std::path::Path::new("/tmp/saniwalens-test-config.toml");
        let out = ask_save(&mut input, &mut output, None, "kilocode", path).unwrap();
        assert!(out.is_none());
        let shown = String::from_utf8(output).unwrap();
        assert!(shown.contains("保存しますか? [y/N]"), "{shown}");
    }

    #[test]
    fn ask_yes_no_y_means_yes() {
        for stdin in ["y\n", "Y\n", " y \n"] {
            let mut input = std::io::Cursor::new(stdin);
            let mut output = Vec::new();
            assert!(
                ask_yes_no(&mut input, &mut output, "次の一手を実行しますか? [y/N]").unwrap(),
                "{stdin:?} must mean yes"
            );
        }
    }

    #[test]
    fn ask_yes_no_n_empty_eof_and_q_mean_no() {
        for stdin in ["n\n", "N\n", "\n", "q\n", "Q\n", ""] {
            let mut input = std::io::Cursor::new(stdin);
            let mut output = Vec::new();
            assert!(
                !ask_yes_no(&mut input, &mut output, "次の一手を実行しますか? [y/N]").unwrap(),
                "{stdin:?} must mean no"
            );
        }
    }

    #[test]
    fn persist_config_round_trip_through_a_pid_unique_dir() {
        let dir =
            std::env::temp_dir().join(format!("saniwalens-guide-test-{}", std::process::id()));
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);
        persist_config(&path, "default_harness = \"kilocode\"\n", None).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "default_harness = \"kilocode\"\n"
        );
        persist_config(
            &path,
            "default_harness = \"opencode\"\n",
            Some("default_harness = \"kilocode\"\n"),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "default_harness = \"opencode\"\n"
        );
        assert!(
            !path.with_extension("toml.tmp").exists(),
            "no temp sibling left behind"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn persist_config_refuses_to_clobber_an_external_edit() {
        // The dialog read v1; someone else wrote v2 meanwhile: the write
        // aborts and v2 survives untouched.
        let dir = std::env::temp_dir().join(format!(
            "saniwalens-guide-test-clobber-{}",
            std::process::id()
        ));
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);
        persist_config(&path, "v1\n", None).unwrap();
        std::fs::write(&path, "v2\n").unwrap();
        let err = persist_config(&path, "v3\n", Some("v1\n")).expect_err("must refuse");
        assert!(
            err.to_string().contains("changed since the dialog read it"),
            "{err}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "v2\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn persist_config_refuses_when_the_file_appeared_or_vanished() {
        // Created meanwhile (expected absent) and deleted meanwhile
        // (expected present) are both conflicts, not silent overwrites.
        let dir = std::env::temp_dir().join(format!(
            "saniwalens-guide-test-appear-{}",
            std::process::id()
        ));
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "meanwhile\n").unwrap();
        persist_config(&path, "mine\n", None).expect_err("created meanwhile must refuse");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "meanwhile\n");
        std::fs::remove_file(&path).unwrap();
        persist_config(&path, "mine\n", Some("was here\n"))
            .expect_err("deleted meanwhile must refuse");
        assert!(!path.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn guide_inline_next_step_carries_a_ja_setting_into_the_prompt() {
        // PBI 2026-11-10-08 BDD "ガイドでも ja が効く": with
        // `language = "ja"` resolved, the prompt the guide's inline next step
        // composes must carry the single language sentence. The session is
        // built through the guide's own seam (`guide_session`), the same
        // resolve-open-carry shape as the normal `optimize` arm.
        use crate::core::optimize::{self as optimize_mod, AdvisorLanguage};
        use crate::session::resolve_cli_with_language;

        let dir =
            std::env::temp_dir().join(format!("saniwalens-guide-lang-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let state = dir.join("state");
        let env = SessionEnv {
            state_home: Some(state.as_path()),
            home: None,
            config_home: None,
        };
        let resolved = resolve_cli_with_language(None, None, Some("ja"), &env).unwrap();
        assert_eq!(resolved.language, AdvisorLanguage::Ja);
        let sess = guide_session(&resolved.harness, resolved.language, None, &env).unwrap();
        assert_eq!(sess.language, AdvisorLanguage::Ja);
        let mut copy = sess.harness.copy();
        copy.language = sess.language;
        let prompt = optimize_mod::compose_prompt(
            &optimize_mod::Findings::default(),
            "s.db",
            &ScopeFilter::All,
            &copy,
        );
        let sentence = AdvisorLanguage::Ja.instruction_sentence().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            prompt.contains(sentence),
            "the ja setting is lost on the guide path:\n{prompt}"
        );
    }

    #[test]
    fn guide_inline_next_step_without_a_setting_carries_no_sentence() {
        // PBI 2026-11-10-08 BDD "未設定では従来どおり": with no language
        // resolved, the guide's inline prompt carries no language sentence.
        use crate::core::optimize::{self as optimize_mod, AdvisorLanguage};
        use crate::session::resolve_cli_with_language;

        let dir = std::env::temp_dir().join(format!(
            "saniwalens-guide-lang-unset-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let state = dir.join("state");
        let env = SessionEnv {
            state_home: Some(state.as_path()),
            home: None,
            config_home: None,
        };
        let resolved = resolve_cli_with_language(None, None, None, &env).unwrap();
        assert_eq!(resolved.language, AdvisorLanguage::En);
        let sess = guide_session(&resolved.harness, resolved.language, None, &env).unwrap();
        assert_eq!(sess.language, AdvisorLanguage::En);
        let mut copy = sess.harness.copy();
        copy.language = sess.language;
        let prompt = optimize_mod::compose_prompt(
            &optimize_mod::Findings::default(),
            "s.db",
            &ScopeFilter::All,
            &copy,
        );
        let sentence = AdvisorLanguage::Ja.instruction_sentence().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            !prompt.contains(sentence),
            "unset output gained a sentence:\n{prompt}"
        );
    }
}
