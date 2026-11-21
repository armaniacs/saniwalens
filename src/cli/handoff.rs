//! The `--apply` handoff: run the headless advisor, save its plan text, and
//! open the harness's own interactive session on it (`docs/specs/cli.md`).
//!
//! Normal `optimize` never enters this module, so its delivery stays
//! byte-identical with or without `--apply` in the tree. The handoff only
//! runs for headless harnesses: the headless run prints the plan, the `Text`
//! lines are extracted from the kept run log, the plan is saved under the
//! state home with private permissions, and the harness's own TUI opens on
//! an apply-ordering prompt. The flag checks, path resolution, notes, and
//! the extract-save-launch completion are pure and unit-tested; only the
//! headless spawn and the file writes are I/O.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::adapter::harness::{Harness, RunLine, RunLineRenderer, extract_plan_text};
use crate::core::optimize as optimize_mod;
use crate::core::scope::ScopeFilter;
use crate::session::{Session, SessionEnv, Style, state_base};

use super::deliver::{
    BriefingFile, ProcessAdvisor, create_private_dir_all, unique_temp_stem, write_private_tempfile,
};
use super::headless::{Beat, run_headless};
use super::{AdvisorLaunch, write_private_file};

/// The `--apply`-capable harness labels, derived from the launch table: every
/// harness whose [`RunMode`](optimize_mod::RunMode) is headless. Generated —
/// never re-spelled — so a future headless harness appears in the refusal
/// message the moment its `launch_command` arm lands, instead of leaving a
/// stale `(opencode)` behind.
fn headless_harness_labels() -> String {
    Harness::ALL
        .iter()
        .filter(|harness| harness.launch_command().2 == optimize_mod::RunMode::Headless)
        .map(|harness| harness.label())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Fail unless the run mode is headless: `--apply` hands a headless run's
/// plan to an interactive dialog, so an already-interactive session has
/// nothing to hand off. An exhaustive match with no wildcard (like
/// `delivery_mode`): a future third run mode fails to compile here instead
/// of slipping through the handoff at runtime. This is the predicate both
/// the flag pre-check and `run_apply`'s defense-in-depth arm share, so both
/// refuse with the same wording. The parenthetical names the allowed set via
/// [`headless_harness_labels`], so the wording tracks the launch table.
fn require_headless_mode(run_mode: optimize_mod::RunMode, harness_name: &str) -> Result<()> {
    match run_mode {
        optimize_mod::RunMode::Headless => Ok(()),
        optimize_mod::RunMode::Interactive => anyhow::bail!(
            "`--apply` needs a headless harness ({}): the {} session is already \
             interactive and applies its own plan",
            headless_harness_labels(),
            harness_name
        ),
    }
}

/// The flag pre-check's thin wrapper over [`require_headless_mode`]: it pulls
/// the run mode out of the harness's launch shape and refuses with the same
/// wording as `run_apply`'s arm.
fn require_headless(harness: &Harness) -> Result<()> {
    require_headless_mode(harness.launch_command().2, harness.display_name())
}

/// Flag-level pre-checks, before any store work: `--apply` conflicts and the
/// harness-mode gate. Pure over its inputs; the shell calls it at the arm top
/// so a conflict fails before a store is even resolved. A non-apply run
/// always passes, so normal `optimize` keeps its exact behavior.
pub fn check_apply_args(apply: bool, print: bool, verbose: bool, harness: &Harness) -> Result<()> {
    if !apply {
        return Ok(());
    }
    if print {
        anyhow::bail!(
            "`--apply` cannot be combined with `--print`: printing ends the run \
             instead of handing its plan to a dialog session"
        );
    }
    if verbose {
        anyhow::bail!(
            "`--apply` cannot be combined with `--verbose`: the handoff extracts the \
             plan from the logged stream, which verbose mode does not keep"
        );
    }
    require_headless(harness)
}

/// The state-home side both handoff dirs live under:
/// `$XDG_STATE_HOME/saniwalens/`, else `~/.local/state/saniwalens/`.
/// Resolved by session.rs's shared `state_base`, so the XDG rule (absolute
/// value wins, relative ignored, absolute home required) exists once — never
/// cwd-relative.
fn state_saniwalens_dir(state_home: Option<&Path>, home: Option<&Path>) -> Result<PathBuf> {
    Ok(state_base(state_home, home)
        .with_context(|| "resolve the state home for handoff files")?
        .join("saniwalens"))
}

/// The plans directory under the user state dir:
/// `$XDG_STATE_HOME/saniwalens/plans/`, else `~/.local/state/saniwalens/plans/`.
pub fn plans_dir(state_home: Option<&Path>, home: Option<&Path>) -> Result<PathBuf> {
    Ok(state_saniwalens_dir(state_home, home)?.join("plans"))
}

/// The backups directory under the user state dir:
/// `$XDG_STATE_HOME/saniwalens/backups/`, else
/// `~/.local/state/saniwalens/backups/`. The apply sessions create one
/// timestamped generation dir per session here (the prompt's backup step);
/// the cleanup command resolves the same dir to list and prune them.
pub fn backups_dir(state_home: Option<&Path>, home: Option<&Path>) -> Result<PathBuf> {
    Ok(state_saniwalens_dir(state_home, home)?.join("backups"))
}

/// A fresh plan file name, unique per process and call via the shared
/// [`unique_temp_stem`] stem.
fn plan_filename() -> String {
    let stem = unique_temp_stem();
    format!("plan-{:x}-{}-{}.md", stem.nanos, stem.pid, stem.seq)
}

/// Write `plan` to a fresh file under `dir`, for the current user only
/// (`0600`, exclusive creation like the briefing temp files). Creates `dir`
/// when missing with the owner-only dir rule (`0700`, `create_private_dir_all`):
/// plan text may echo config secrets. Returns the saved path.
pub fn save_plan(dir: &Path, plan: &str) -> Result<PathBuf> {
    create_private_dir_all(dir).with_context(|| format!("create plans dir {}", dir.display()))?;
    let path = dir.join(plan_filename());
    write_private_file(&path, plan)?;
    Ok(path)
}

/// The stdout line naming the saved plan: the handoff always leaves a plan
/// file behind and always shows its path.
pub fn saved_note(plan: &Path) -> String {
    format!("plan saved to {}", plan.display())
}

/// The stderr line pointing plan-file pruning at its future owner: plan files
/// accumulate until the cleanup command arrives, and this run leaves them in
/// place.
pub fn cleanup_note(dir: &Path) -> String {
    format!(
        "plan files are kept under {}; pruning them is a separate command's job: \
         this run leaves the file in place",
        dir.display()
    )
}

/// Hard ceiling, in bytes, for the plan text the `--apply` handoff saves and
/// for the raw run-log text it reads as the plan's material. Same value as
/// the briefing's `BRIEFING_MAX_BYTES` (`core::optimize`), whose truncation
/// shape this mirrors: an unbounded plan file would accumulate in the state
/// dir (cleanup prunes generations, not sizes), and an unbounded read would
/// load an arbitrarily verbose advisor's whole stream into memory — the
/// default 60s timeout usually bounds that, but `--timeout 0` is the explicit
/// opt-out, so the limit closes by size, not by time.
pub const PLAN_MAX_BYTES: usize = 64 * 1024;

/// Marker terminating a plan cut at [`PLAN_MAX_BYTES`]. An over-long plan is
/// truncated to fit the budget with this as its last line, so a cut is always
/// visible and never mistaken for the advisor's complete plan. The same
/// marker closes the other cut side: a plan extracted from a raw log read
/// only up to [`PLAN_MAX_BYTES`] also gets it appended, because it may be
/// missing its tail even though it fits the cap. Both cuts firing at once
/// must not stack it twice.
pub const PLAN_TRUNCATION_MARKER: &str = "\n\n[plan truncated at PLAN_MAX_BYTES]\n";

/// Enforce [`PLAN_MAX_BYTES`] on an extracted plan: text within the cap is
/// returned unchanged (byte-identical) with `false`; over-long text is cut at
/// a character boundary with [`PLAN_TRUNCATION_MARKER`] as its last line and
/// `true`, so the saved plan fits the cap and the cut is marked.
fn truncate_plan(text: String) -> (String, bool) {
    if text.len() <= PLAN_MAX_BYTES {
        return (text, false);
    }
    let budget = PLAN_MAX_BYTES.saturating_sub(PLAN_TRUNCATION_MARKER.len());
    let mut end = budget.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = text[..end].to_string();
    out.push_str(PLAN_TRUNCATION_MARKER);
    (out, true)
}

/// The stderr line telling the user the plan was cut: the dialog still opens,
/// but on a truncated plan, so the user must know it is not the advisor's
/// full text.
pub fn plan_truncated_note() -> String {
    format!(
        "the plan exceeded PLAN_MAX_BYTES ({} bytes): saved truncated to the \
         first bytes with a truncation marker, and the dialog opens on it",
        PLAN_MAX_BYTES
    )
}

/// The stderr line telling the user the raw run-log read hit the cap: the
/// plan is extracted from the kept head only, so it may be missing its tail.
pub fn log_truncated_note() -> String {
    format!(
        "the raw run log exceeded PLAN_MAX_BYTES ({} bytes): read truncated \
         there, so the plan extracted from it may be missing its tail",
        PLAN_MAX_BYTES
    )
}

/// Read at most `cap` bytes of `path`, cut at a character boundary, and
/// report whether the cut happened. A whole-file `read_to_string` would load
/// an arbitrarily verbose advisor's stream into memory before the plan cap
/// could act, so the read stops at the cap first: within the cap the text is
/// byte-identical to a full read, and invalid UTF-8 fails exactly like
/// `read_to_string` did.
fn read_capped(path: &Path, cap: usize) -> Result<(String, bool)> {
    let file = fs::File::open(path)
        .with_context(|| format!("read the headless run log {}", path.display()))?;
    // One byte past the cap separates "exactly the cap" (the whole file, not
    // cut) from "over the cap" (cut); `take` bounds the buffering either way.
    let mut bytes = Vec::new();
    file.take(cap as u64 + 1).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > cap;
    if truncated {
        // Cut on the raw bytes, back to a lead byte (a UTF-8 continuation
        // byte is 0b10xxxxxx), so the cap never splits a multi-byte
        // character; the kept prefix is validated afterwards.
        let mut end = cap;
        while end > 0 && (bytes[end] & 0xC0) == 0x80 {
            end -= 1;
        }
        bytes.truncate(end);
    }
    // Validate only what is kept: invalid UTF-8 fails exactly like the
    // previous whole-file `read_to_string` did.
    let text = String::from_utf8(bytes).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )
    })?;
    Ok((text, truncated))
}

/// Everything `complete_handoff` needs beyond the log text and the advisor.
pub struct Handoff<'a> {
    /// The renderer the harness seam chose for the headless stream.
    pub render: Option<RunLineRenderer>,
    /// The plans directory the plan file is saved under.
    pub plans: &'a Path,
    /// The analyzed store's display path, for the dialog's `--db`.
    pub db_display: &'a str,
    /// The scope the handoff run was filtered to.
    pub filter: &'a ScopeFilter,
    /// The harness copy the apply prompt renders from.
    pub copy: &'a optimize_mod::HarnessCopy,
    /// The harness whose TUI opens on the saved plan.
    pub harness: Harness,
}

/// The seam between the headless run and the dialog: turn one kept run log's
/// text into the saved plan and open the harness's own interactive session on
/// it. `log_truncated` is the capped read's verdict on that log text: when it
/// says the raw log was cut, the extracted plan may be missing its tail even
/// though it fits the cap, so the saved plan is marked (never doubly — a
/// plan-size cut already ends in the marker). Pure except the plan-file write
/// and the `direct` spawn, so a double covers every branch without spawning:
/// an empty plan writes nothing and fails loud, and a failed launch names the
/// kept plan file.
pub fn complete_handoff(
    log_text: &str,
    log_truncated: bool,
    handoff: &Handoff<'_>,
    advisor: &mut impl AdvisorLaunch,
) -> Result<PathBuf> {
    let rendered: Vec<RunLine> = log_text
        .lines()
        .map(|line| {
            handoff
                .render
                .map(|render| render(line))
                .unwrap_or(RunLine::Silent)
        })
        .collect();
    let plan = extract_plan_text(&rendered);
    if plan.trim().is_empty() {
        // Nothing is written and nothing is removed here: the input is the
        // in-memory log text, so claiming a removed log would name a file
        // that never existed in this function's scope.
        anyhow::bail!("the advisor produced no readable text, so no plan file was written");
    }
    let (mut plan, plan_truncated) = truncate_plan(plan);
    if log_truncated && !plan_truncated {
        // A log-cut plan always fits the cap (its bytes are a subset of the
        // capped log's), so the plan-size cut never fired: append the marker
        // here, or the dialog reads a silently incomplete plan.
        plan.push_str(PLAN_TRUNCATION_MARKER);
    }
    if plan_truncated {
        eprintln!("{}", Style::stderr().warn(&plan_truncated_note()));
    }
    let plan_path = save_plan(handoff.plans, &plan)?;
    let prompt = optimize_mod::apply_prompt(
        &plan_path.to_string_lossy(),
        handoff.db_display,
        handoff.filter,
        handoff.copy,
    );
    let (binary, leading) = handoff.harness.interactive_launch_command();
    let status = advisor.direct(binary, leading, &prompt).with_context(|| {
        format!(
            "launch `{binary} {}`: is {} installed and on PATH? the plan file is kept at {}",
            leading.join(" ").trim_end(),
            handoff.harness.display_name(),
            plan_path.display()
        )
    })?;
    if !status.success() {
        anyhow::bail!(
            "{binary} exited with {status}: the plan file is kept at {}",
            plan_path.display()
        );
    }
    Ok(plan_path)
}

/// Run the headless advisor with its raw log kept, and hand back the status,
/// the log path, the log's text, and whether the read was cut at the cap —
/// the verdict `complete_handoff` marks the plan with, so a log cut stays
/// visible where the plan is consumed. A success removes the log (the raw
/// stream may echo secrets; the plan file carries the content from here on);
/// a failure keeps it and names it in the error, like the normal headless
/// delivery. The error states that no plan file was written yet.
pub fn capture_headless_log(
    binary: &str,
    leading: &[&str],
    prompt: &str,
    render: Option<RunLineRenderer>,
    timeout: Option<std::time::Duration>,
) -> Result<(std::process::ExitStatus, PathBuf, String, bool)> {
    let (status, log_path) = run_headless(
        binary,
        leading,
        prompt,
        render,
        Beat::default(),
        timeout,
        true,
    )
    .context("--apply: the headless run failed; no plan file was written")?;
    if !status.success() {
        anyhow::bail!(
            "{binary} exited with {status}; raw output: {} (no plan file was written)",
            log_path.display()
        );
    }
    let (text, log_truncated) = read_capped(&log_path, PLAN_MAX_BYTES)?;
    if log_truncated {
        eprintln!("{}", Style::stderr().warn(&log_truncated_note()));
    }
    // The raw stream may echo secrets; the plan file carries the content from
    // here on, so the log goes now.
    let _ = fs::remove_file(&log_path);
    Ok((status, log_path, text, log_truncated))
}

/// Run the `--apply` handoff for one resolved session: refresh unless frozen,
/// run the headless advisor, save its plan, print where it landed, and open
/// the harness's own interactive session on it. Only called for headless
/// harnesses (the arm checks first); normal `optimize` never enters here.
pub fn run_apply(
    sess: &Session,
    projects: Option<PathBuf>,
    filter: &ScopeFilter,
    frozen: bool,
    timeout: Option<std::time::Duration>,
    env: &SessionEnv,
) -> Result<()> {
    let (binary, leading, run_mode) = sess.harness.launch_command();
    require_headless_mode(run_mode, sess.harness.display_name())?;
    // Fail fast before the expensive store refresh: without a writable plans
    // home there is nothing to hand off to.
    let plans = plans_dir(env.state_home, env.home.map(Path::new))?;
    let (findings, db_display, copy) = super::prepare_optimize(sess, projects, frozen)?;

    let briefing = optimize_mod::render_briefing(&findings, filter, &copy);
    let briefing_path = write_private_tempfile(&briefing).context("write briefing temp file")?;
    let briefing_file = BriefingFile::new(briefing_path);
    let prompt = optimize_mod::launch_prompt(
        &briefing_file.path().to_string_lossy(),
        &db_display,
        filter,
        &copy,
        optimize_mod::RunMode::Headless,
    );
    let render = sess.harness.run_line_renderer();
    let (_, _, log_text, log_truncated) =
        capture_headless_log(binary, leading, &prompt, render, timeout)?;
    let handoff = Handoff {
        render,
        plans: &plans,
        db_display: &db_display,
        filter,
        copy: &copy,
        harness: sess.harness,
    };
    let mut advisor = ProcessAdvisor;
    let plan_path = complete_handoff(&log_text, log_truncated, &handoff, &mut advisor)?;
    println!("{}", saved_note(&plan_path));
    eprintln!("{}", Style::stderr().dim(&cleanup_note(&plans)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::ScratchDir;
    use crate::store::{OpenMode, Store};

    /// A synthetic `--format json` tool-event line: the noise a headless run
    /// streams around the agent's text. The real envelope shape, never a
    /// harvested line.
    #[cfg(unix)]
    const TOOL_LINE: &str = r#"{"type":"tool_use","timestamp":1792904532992,"sessionID":"ses_demo","part":{"partID":"prt_demo","type":"tool","tool":"read","state":{"status":"completed"}}}"#;

    /// A synthetic text-event line carrying plan prose.
    #[cfg(unix)]
    const TEXT_LINE: &str = r#"{"type":"text","timestamp":1792904882135,"sessionID":"ses_demo","part":{"id":"prt_demo","type":"text","text":"1. Add a file map to AGENTS.md"}}"#;

    #[cfg(unix)]
    fn opencode_copy() -> optimize_mod::HarnessCopy {
        Harness::Opencode.copy()
    }

    #[test]
    fn require_headless_mode_accepts_only_the_headless_run() {
        // The gate's contract, pinned directly: Headless passes, Interactive
        // fails. A future third run mode must extend this table — and the
        // exhaustive match below forces that extension at compile time.
        assert!(require_headless_mode(optimize_mod::RunMode::Headless, "opencode").is_ok());
        let err = require_headless_mode(optimize_mod::RunMode::Interactive, "claude").unwrap_err();
        assert!(format!("{err:#}").contains("headless"), "{err:#}");
    }

    #[test]
    fn refusal_message_names_the_allowed_headless_harnesses() {
        // BDD: "許可される headless 群が文言と一致する". The allowed set is
        // the launch table's truth; the message must name every headless
        // harness, so a future headless arm without a message update fails
        // here instead of shipping a stale parenthetical.
        let expected: Vec<&str> = Harness::ALL
            .iter()
            .filter(|harness| harness.launch_command().2 == optimize_mod::RunMode::Headless)
            .map(|harness| harness.label())
            .collect();
        assert!(!expected.is_empty(), "the gate allows no headless harness");
        let err = require_headless_mode(optimize_mod::RunMode::Interactive, "claude").unwrap_err();
        let rendered = format!("{err:#}");
        for label in expected {
            assert!(rendered.contains(label), "{rendered}");
        }
    }

    #[test]
    fn check_apply_args_passes_a_normal_run_through_untouched() {
        // No flag, no opinion: whatever else is set, a normal run passes, so
        // the pre-check cannot change `optimize` without `--apply`.
        assert!(check_apply_args(false, false, false, &Harness::Claude).is_ok());
        assert!(check_apply_args(false, true, true, &Harness::Opencode).is_ok());
    }

    #[test]
    fn check_apply_args_refuses_print() {
        // Printing ends the run instead of handing it off: there is no dialog
        // to open the saved plan with.
        let err = check_apply_args(true, true, false, &Harness::Opencode).unwrap_err();
        assert!(format!("{err:#}").contains("--print"), "{err:#}");
    }

    #[test]
    fn check_apply_args_refuses_verbose() {
        // Verbose streams the raw output instead of the logged pipe, so the
        // handoff would have no run log to extract the plan from.
        let err = check_apply_args(true, false, true, &Harness::Opencode).unwrap_err();
        assert!(format!("{err:#}").contains("--verbose"), "{err:#}");
    }

    #[test]
    fn check_apply_args_refuses_interactive_harnesses() {
        // An interactive session already applies its own plan: there is no
        // headless output to hand off.
        for harness in [Harness::Claude, Harness::Kilocode] {
            let err = check_apply_args(true, false, false, &harness).unwrap_err();
            assert!(
                format!("{err:#}").contains("headless"),
                "{}: {err:#}",
                harness.label()
            );
        }
    }

    #[test]
    fn check_apply_args_accepts_the_headless_handoff() {
        assert!(check_apply_args(true, false, false, &Harness::Opencode).is_ok());
    }

    /// Fixture roots from the temp dir, which is absolute on every OS: a
    /// spelled-out `/tmp/...` is not absolute on Windows, and these resolvers
    /// ignore non-absolute inputs. Expectations derive from the same roots
    /// with `join`, never from a second spelled-out literal.
    fn fixture_root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("saniwalens-handoff-fixture-{tag}"))
    }

    #[test]
    fn plans_dir_prefers_an_absolute_state_home() {
        let state = fixture_root("state");
        let home = fixture_root("home");
        assert_eq!(
            plans_dir(Some(&state), Some(&home)).unwrap(),
            state.join("saniwalens").join("plans")
        );
    }

    #[test]
    fn plans_dir_falls_back_to_the_home_state_dir() {
        let home = fixture_root("home");
        assert_eq!(
            plans_dir(None, Some(&home)).unwrap(),
            home.join(".local")
                .join("state")
                .join("saniwalens")
                .join("plans")
        );
    }

    #[test]
    fn plans_dir_ignores_a_relative_state_home() {
        // XDG: a non-absolute XDG_STATE_HOME is invalid and must be ignored —
        // honoring it would put the plans back under the current directory.
        let home = fixture_root("home");
        assert_eq!(
            plans_dir(Some(Path::new("relative/state")), Some(&home)).unwrap(),
            home.join(".local")
                .join("state")
                .join("saniwalens")
                .join("plans")
        );
    }

    #[test]
    fn plans_dir_without_a_home_is_an_error() {
        assert!(plans_dir(None, None).is_err());
        assert!(plans_dir(Some(Path::new("relative/state")), None).is_err());
    }

    #[test]
    fn backups_dir_mirrors_the_plans_rule() {
        // The sweep must resolve the same base as the saver: an absolute XDG
        // value wins, a relative one is ignored, and without an absolute
        // home there is no answer. Never cwd-relative.
        let state = fixture_root("state");
        let home = fixture_root("home");
        assert_eq!(
            backups_dir(Some(&state), Some(&home)).unwrap(),
            state.join("saniwalens").join("backups")
        );
        assert_eq!(
            backups_dir(None, Some(&home)).unwrap(),
            home.join(".local")
                .join("state")
                .join("saniwalens")
                .join("backups")
        );
        assert_eq!(
            backups_dir(Some(Path::new("relative/state")), Some(&home)).unwrap(),
            home.join(".local")
                .join("state")
                .join("saniwalens")
                .join("backups")
        );
        assert!(backups_dir(None, None).is_err());
    }

    #[test]
    fn plans_and_backups_share_their_parent() {
        // The list/sweep pair the cleanup command prints is one state home
        // seen twice: the two dirs differ only in the last component.
        let state = fixture_root("state");
        let home = fixture_root("home");
        let plans = plans_dir(Some(&state), Some(&home)).unwrap();
        let backups = backups_dir(Some(&state), Some(&home)).unwrap();
        assert_eq!(plans.parent(), backups.parent());
        assert_eq!(plans.parent().unwrap(), state.join("saniwalens").as_path());
    }

    #[test]
    fn plan_filenames_are_unique_per_call() {
        // Exclusive creation fails the second save on a reused name, so two
        // saves into one directory must land on two files.
        let dir = ScratchDir::new("saniwalens-handoff", "unique");
        let first = save_plan(&dir, "plan one").unwrap();
        let second = save_plan(&dir, "plan two").unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read_to_string(&first).unwrap(), "plan one");
        assert_eq!(fs::read_to_string(&second).unwrap(), "plan two");
    }

    #[cfg(unix)]
    #[test]
    fn save_plan_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let dir = ScratchDir::new("saniwalens-handoff", "mode");
        let path = save_plan(&dir, "secret plan").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn save_plan_creates_the_plans_dir_owner_only() {
        // Plan text may echo config secrets, so the plans dir follows the
        // store's rule: freshly created dirs are 0700, like the store dir.
        use std::os::unix::fs::PermissionsExt;

        let base = ScratchDir::new("saniwalens-handoff", "dir-mode");
        let nested = base.join("nested").join("plans");
        save_plan(&nested, "secret plan").unwrap();
        let mode = fs::metadata(&nested).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "the plans dir must be owner-only");
    }

    #[test]
    fn the_saved_note_names_the_plan_file() {
        let note = saved_note(Path::new("/tmp/example/state/saniwalens/plans/plan-x.md"));
        assert!(note.contains("/tmp/example/state/saniwalens/plans/plan-x.md"));
    }

    #[test]
    fn the_cleanup_note_defers_pruning_to_its_own_command() {
        // BDD: "掃除は別コマンドに任せることが分かる表示がある".
        let note = cleanup_note(Path::new("/tmp/example/state/saniwalens/plans"));
        assert!(note.contains("/tmp/example/state/saniwalens/plans"));
        assert!(note.contains("separate command"), "{note}");
    }

    /// The dialog-side double: records the interactive launch instead of
    /// spawning, with a configurable exit, so the completion's argv and its
    /// failure branches are asserted without a TUI.
    #[cfg(unix)]
    struct DialogRecorder {
        launches: Vec<(String, Vec<String>, String)>,
        status: std::process::ExitStatus,
    }

    #[cfg(unix)]
    fn success_status() -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0)
    }

    #[cfg(unix)]
    fn failure_status() -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0x100)
    }

    #[cfg(unix)]
    impl AdvisorLaunch for DialogRecorder {
        fn direct(
            &mut self,
            binary: &str,
            leading: &[&str],
            prompt: &str,
        ) -> Result<std::process::ExitStatus> {
            self.launches.push((
                binary.to_string(),
                leading.iter().map(|s| s.to_string()).collect(),
                prompt.to_string(),
            ));
            Ok(self.status)
        }

        fn headless(
            &mut self,
            _binary: &str,
            _leading: &[&str],
            _prompt: &str,
            _render: Option<RunLineRenderer>,
            _timeout: Option<std::time::Duration>,
        ) -> Result<(std::process::ExitStatus, PathBuf)> {
            panic!("the completion never runs headless: the log text arrives as input")
        }
    }

    #[cfg(unix)]
    fn handoff<'a>(
        plans: &'a Path,
        filter: &'a ScopeFilter,
        copy: &'a optimize_mod::HarnessCopy,
    ) -> Handoff<'a> {
        Handoff {
            render: Harness::Opencode.run_line_renderer(),
            plans,
            db_display: "/tmp/oc.db",
            filter,
            copy,
            harness: Harness::Opencode,
        }
    }

    #[cfg(unix)]
    #[test]
    fn complete_handoff_saves_text_only_and_launches_the_dialog() {
        // BDD: the headless plan is saved and the dialog opens on it. The
        // raw log mixes JSON event lines with the agent's text; only the
        // Text lines reach the plan file.
        let dir = ScratchDir::new("saniwalens-handoff", "complete");
        let filter = ScopeFilter::All;
        let copy = opencode_copy();
        let ctx = handoff(&dir, &filter, &copy);
        let log_text = format!("{TOOL_LINE}\n{TEXT_LINE}\nnot json at all\n");
        let mut advisor = DialogRecorder {
            launches: Vec::new(),
            status: success_status(),
        };

        let plan_path = complete_handoff(&log_text, false, &ctx, &mut advisor).unwrap();

        assert_eq!(
            fs::read_to_string(&plan_path).unwrap(),
            "1. Add a file map to AGENTS.md"
        );
        assert_eq!(advisor.launches.len(), 1);
        let (binary, leading, prompt) = &advisor.launches[0];
        // The dialog shape is the harness's own interactive one.
        assert_eq!(binary, "opencode");
        assert_eq!(leading, &vec!["--prompt".to_string()]);
        // The opening prompt orders reading the saved file and applying it.
        assert!(
            prompt.contains(&plan_path.display().to_string()),
            "{prompt}"
        );
        assert!(prompt.contains("apply"), "{prompt}");
        assert!(prompt.contains("--db /tmp/oc.db"), "{prompt}");
    }

    #[cfg(unix)]
    #[test]
    fn complete_handoff_refuses_an_empty_plan() {
        // JSON-only output yields no plan text: nothing is saved and the
        // error says so, instead of leaving an empty plan file behind.
        let dir = ScratchDir::new("saniwalens-handoff", "empty");
        let filter = ScopeFilter::All;
        let copy = opencode_copy();
        let ctx = handoff(&dir, &filter, &copy);
        let mut advisor = DialogRecorder {
            launches: Vec::new(),
            status: success_status(),
        };

        let err = complete_handoff(TOOL_LINE, false, &ctx, &mut advisor).unwrap_err();

        let rendered = format!("{err:#}");
        assert!(rendered.contains("no readable text"), "{rendered}");
        assert!(rendered.contains("no plan file"), "{rendered}");
        // The input is in-memory log text: this function removes no file, so
        // the error must not claim a removal.
        assert!(!rendered.contains("removed"), "{rendered}");
        assert!(advisor.launches.is_empty(), "the dialog never opened");
        assert!(
            fs::read_dir(&*dir)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(true),
            "an empty plan left a trace behind"
        );
    }

    #[cfg(unix)]
    #[test]
    fn complete_handoff_failure_keeps_the_plan_and_names_it() {
        // BDD: the dialog launch fails. The saved plan stays, and the error
        // names its path and says it is kept.
        let dir = ScratchDir::new("saniwalens-handoff", "dialog-fail");
        let filter = ScopeFilter::All;
        let copy = opencode_copy();
        let ctx = handoff(&dir, &filter, &copy);
        let mut advisor = DialogRecorder {
            launches: Vec::new(),
            status: failure_status(),
        };

        let err = complete_handoff(TEXT_LINE, false, &ctx, &mut advisor).unwrap_err();

        let rendered = format!("{err:#}");
        assert!(rendered.contains("kept"), "{rendered}");
        let saved: PathBuf = fs::read_dir(&*dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>()
            .pop()
            .expect("the plan file stays");
        assert!(
            rendered.contains(&saved.display().to_string()),
            "{rendered}"
        );
        assert_eq!(
            fs::read_to_string(&saved).unwrap(),
            "1. Add a file map to AGENTS.md"
        );
    }

    #[test]
    fn capture_headless_log_reports_a_missing_binary() {
        // E2E-adjacent refusal path through the real spawn seam: a harness
        // binary that is not installed fails with the launch hint, and the
        // error states that no plan file was written.
        let err = capture_headless_log(
            "/nonexistent-saniwalens-apply-binary",
            &[],
            "ignored",
            None,
            None,
        )
        .unwrap_err();
        let rendered = format!("{err:#}");
        assert!(rendered.contains("raw output:"), "{rendered}");
        assert!(rendered.contains("no plan file"), "{rendered}");
    }

    /// A session over a scratch store under `dir`, for the `run_apply`
    /// refusal tests: they fail before any spawn, so the store's contents
    /// never matter — only the harness and the env do.
    fn session_with_store(harness: Harness, dir: &Path) -> Session {
        let db = dir.join("s.db");
        let store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        Session {
            harness,
            db_path: db,
            store,
            language: optimize_mod::AdvisorLanguage::default(),
            language_source: crate::session::LanguageSource::Default,
        }
    }

    #[test]
    fn run_apply_refuses_interactive_harnesses_before_any_spawn() {
        // Defense in depth behind the arm's pre-check: an interactive
        // harness fails here without spawning anything and without touching
        // the store. Both interactive harnesses are wired: if a harness's
        // `launch_command` mapping ever drifted toward Headless, this table
        // (not just the mode predicate's own test) would catch it. A new
        // harness must extend this table.
        for harness in [Harness::Claude, Harness::Kilocode] {
            let dir = ScratchDir::new("saniwalens-handoff", "refuse");
            let sess = session_with_store(harness, &dir);
            let env = SessionEnv {
                state_home: None,
                home: None,
                config_home: None,
            };
            let err = run_apply(&sess, None, &ScopeFilter::All, true, None, &env).unwrap_err();
            assert!(
                format!("{err:#}").contains("headless"),
                "{}: {err:#}",
                harness.label()
            );
        }
    }

    #[test]
    fn run_apply_resolves_the_plans_home_before_any_spawn() {
        // Fail fast before the expensive headless run: without a plans home
        // there is nothing to hand off to. The short timeout is only a
        // backstop — a regression that spawns anyway must still end the test.
        let dir = ScratchDir::new("saniwalens-handoff", "nohome");
        let sess = session_with_store(Harness::Opencode, &dir);
        let env = SessionEnv {
            state_home: Some(Path::new("relative/state")),
            home: None,
            config_home: None,
        };
        let err = run_apply(
            &sess,
            None,
            &ScopeFilter::All,
            true,
            Some(std::time::Duration::from_secs(5)),
            &env,
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("HOME is not set"), "{err:#}");
    }

    #[test]
    fn run_apply_resolves_the_plans_home_before_the_refresh() {
        // Fail fast on the unfrozen path too: plans-dir resolution precedes
        // the store refresh, so an unresolvable home errors before analyze
        // runs. The bogus projects path would fail the refresh with "no
        // opencode database" if it ran — the HOME error proves it never did.
        let dir = ScratchDir::new("saniwalens-handoff", "nohome-unfrozen");
        let sess = session_with_store(Harness::Opencode, &dir);
        let env = SessionEnv {
            state_home: Some(Path::new("relative/state")),
            home: None,
            config_home: None,
        };
        let err = run_apply(
            &sess,
            Some(PathBuf::from("/nonexistent-saniwalens-projects-dir")),
            &ScopeFilter::All,
            false,
            Some(std::time::Duration::from_secs(5)),
            &env,
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("HOME is not set"), "{err:#}");
    }

    #[cfg(unix)]
    #[test]
    fn capture_headless_log_returns_the_streamed_text() {
        // The kept log's raw lines come back as text: a synthetic advisor
        // stands in for the harness binary (real spawn, synthetic JSON).
        let script = format!("printf '%s\\n%s\\n' '{TOOL_LINE}' '{TEXT_LINE}'");
        let (status, log_path, text, log_truncated) = capture_headless_log(
            "/bin/sh",
            &["-c", &script],
            "ignored",
            Harness::Opencode.run_line_renderer(),
            None,
        )
        .expect("the synthetic advisor streams");
        assert!(status.success(), "the synthetic exit status: {status}");
        assert!(!log_truncated, "a tiny log is not cut");
        assert!(text.contains(TEXT_LINE), "the raw line reached the text");
        assert!(text.contains(TOOL_LINE), "every raw line is captured");
        assert!(!log_path.exists(), "the raw log survived its run");
    }

    // ---- the plan byte cap ----

    #[test]
    fn truncate_plan_passes_within_cap_text_through_byte_identical() {
        // BDD: "上限内の plan は従来どおりバイト同一で保存される" — at and
        // under the cap the text passes through unchanged and unflagged.
        let under = "x".repeat(PLAN_MAX_BYTES - 1);
        let (out, truncated) = truncate_plan(under.clone());
        assert!(!truncated);
        assert_eq!(out, under);
        let exact = "x".repeat(PLAN_MAX_BYTES);
        let (out, truncated) = truncate_plan(exact.clone());
        assert!(!truncated);
        assert_eq!(out, exact);
    }

    #[test]
    fn truncate_plan_cuts_an_over_cap_plan_to_head_plus_marker() {
        // BDD: "上限超過の plan は先頭 N + 末尾マーカーで打ち切られる" — one
        // byte over the cap already cuts, the result fits the cap with the
        // marker as its last line, and the head is kept whole.
        let text = "x".repeat(PLAN_MAX_BYTES + 1);
        let (out, truncated) = truncate_plan(text);
        assert!(truncated);
        assert!(out.len() <= PLAN_MAX_BYTES, "{} bytes", out.len());
        assert!(out.ends_with(PLAN_TRUNCATION_MARKER), "{out:?}");
        let kept = out.strip_suffix(PLAN_TRUNCATION_MARKER).unwrap();
        assert_eq!(kept.len(), PLAN_MAX_BYTES - PLAN_TRUNCATION_MARKER.len());
        assert!(kept.chars().all(|c| c == 'x'));
    }

    #[test]
    fn truncate_plan_cuts_on_a_character_boundary() {
        // UTF-8 境界またぎ: the leading `a` shifts the alignment so the raw
        // budget lands inside an `あ` (3 bytes); the cut falls back to a whole
        // character instead of splitting one.
        let text = format!("a{}", "あ".repeat(PLAN_MAX_BYTES));
        let (out, truncated) = truncate_plan(text.clone());
        assert!(truncated);
        assert!(out.len() <= PLAN_MAX_BYTES, "{} bytes", out.len());
        assert!(out.ends_with(PLAN_TRUNCATION_MARKER));
        let kept = out.strip_suffix(PLAN_TRUNCATION_MARKER).unwrap();
        assert!(text.starts_with(kept), "the head is kept");
        assert_eq!(kept.len() % 3, 1, "a + whole あ characters: {}", kept.len());
        assert_eq!(kept.chars().last(), Some('あ'));
    }

    #[test]
    fn the_plan_truncation_note_names_the_cap_and_the_cut() {
        // BDD: "stderr に plan が上限を超えたことが明示される" — stderr is
        // not capturable in-process, so the wording the handoff prints is
        // pinned here.
        let note = plan_truncated_note();
        assert!(note.contains("PLAN_MAX_BYTES"), "{note}");
        assert!(note.contains("truncated"), "{note}");
        assert!(note.contains("marker"), "{note}");
    }

    #[test]
    fn the_log_truncation_note_names_the_cap_and_the_read_cut() {
        // BDD: "打ち切られたことが plan 生成側の表示に引き継がれる" — the
        // read cut surfaces on the handoff's stderr with the cap named.
        let note = log_truncated_note();
        assert!(note.contains("PLAN_MAX_BYTES"), "{note}");
        assert!(note.contains("truncated"), "{note}");
    }

    #[cfg(unix)]
    #[test]
    fn complete_handoff_caps_an_over_long_plan_and_still_opens_the_dialog() {
        // BDD: 上限超過の合成入力 → the plan file is the head plus the
        // marker, and the handoff itself continues to the dialog launch. The
        // stderr wording is pinned by `the_plan_truncation_note_*` above.
        let dir = ScratchDir::new("saniwalens-handoff", "plan-cap");
        let filter = ScopeFilter::All;
        let copy = opencode_copy();
        let ctx = handoff(&dir, &filter, &copy);
        let big = "x".repeat(PLAN_MAX_BYTES);
        let big_line = format!(
            r#"{{"type":"text","timestamp":1792904882135,"sessionID":"ses_demo","part":{{"id":"prt_demo","type":"text","text":"{big}"}}}}"#
        );
        let log_text = format!("{TOOL_LINE}\n{TEXT_LINE}\n{big_line}\n");
        let uncapped = format!("1. Add a file map to AGENTS.md\n{big}");
        let mut advisor = DialogRecorder {
            launches: Vec::new(),
            status: success_status(),
        };

        let plan_path = complete_handoff(&log_text, false, &ctx, &mut advisor).unwrap();

        let saved = fs::read_to_string(&plan_path).unwrap();
        assert!(saved.len() <= PLAN_MAX_BYTES, "{} bytes", saved.len());
        assert!(saved.ends_with(PLAN_TRUNCATION_MARKER), "the cut is marked");
        let kept = saved.strip_suffix(PLAN_TRUNCATION_MARKER).unwrap();
        assert!(uncapped.starts_with(kept), "the head is kept");
        assert!(kept.len() < uncapped.len(), "the tail was cut, not kept");
        assert_eq!(
            advisor.launches.len(),
            1,
            "the dialog still opens on the truncated plan"
        );
        let (_, _, prompt) = &advisor.launches[0];
        assert!(
            prompt.contains(&plan_path.display().to_string()),
            "{prompt}"
        );
    }

    #[test]
    fn read_capped_keeps_a_file_within_the_cap_whole() {
        // 上限ちょうど: an exact-cap file is the whole file, not a cut.
        let dir = ScratchDir::new("saniwalens-handoff", "read-cap-exact");
        let path = dir.join("run.log");
        fs::write(&path, "x".repeat(PLAN_MAX_BYTES)).unwrap();
        let (text, truncated) = read_capped(&path, PLAN_MAX_BYTES).unwrap();
        assert!(!truncated);
        assert_eq!(text, "x".repeat(PLAN_MAX_BYTES));
    }

    #[test]
    fn read_capped_cuts_an_over_cap_file_at_a_character_boundary() {
        // 1 バイト超過 + UTF-8 境界またぎ: 65536 % 3 == 1, so the raw cap
        // lands inside an `あ` and the cut must fall back to a whole
        // character; the tail beyond the cap never reaches the text.
        let dir = ScratchDir::new("saniwalens-handoff", "read-cap-over");
        let path = dir.join("run.log");
        fs::write(&path, "あ".repeat(PLAN_MAX_BYTES / 3 + 2)).unwrap();
        let (text, truncated) = read_capped(&path, PLAN_MAX_BYTES).unwrap();
        assert!(truncated);
        assert!(text.len() <= PLAN_MAX_BYTES, "{} bytes", text.len());
        assert_eq!(text.len() % 3, 0, "the cut kept whole characters");
        assert!(text.chars().all(|c| c == 'あ'));

        fs::write(&path, format!("{}TAIL", "x".repeat(PLAN_MAX_BYTES + 5))).unwrap();
        let (text, truncated) = read_capped(&path, PLAN_MAX_BYTES).unwrap();
        assert!(truncated);
        assert_eq!(text.len(), PLAN_MAX_BYTES);
        assert!(
            !text.contains("TAIL"),
            "the tail beyond the cap never arrived"
        );
    }

    #[test]
    fn read_capped_fails_on_invalid_utf8_like_read_to_string() {
        // Preserved failure mode: junk bytes fail the read exactly as the
        // previous whole-file `read_to_string` did.
        let dir = ScratchDir::new("saniwalens-handoff", "read-cap-junk");
        let path = dir.join("run.log");
        fs::write(&path, [0xFFu8, 0xFE]).unwrap();
        let err = read_capped(&path, PLAN_MAX_BYTES).unwrap_err();
        assert!(format!("{err:#}").contains("valid UTF-8"), "{err:#}");
    }

    #[cfg(unix)]
    #[test]
    fn capture_headless_log_reads_the_run_log_only_up_to_the_cap() {
        // E2E: a real spawn with a synthetic advisor whose raw log exceeds
        // PLAN_MAX_BYTES — the read stops at the cap, so the log's tail never
        // reaches the handoff's plan material, and the capture completes.
        let line = "a".repeat(300);
        let script = format!("yes '{line}' | head -n 600; echo TAILMARKER");
        let (status, log_path, text, log_truncated) =
            capture_headless_log("/bin/sh", &["-c", &script], "ignored", None, None)
                .expect("the synthetic advisor streams");
        assert!(status.success(), "the synthetic exit status: {status}");
        assert!(log_truncated, "the over-cap log must report its cut");
        assert!(text.len() <= PLAN_MAX_BYTES, "{} bytes", text.len());
        assert!(
            !text.contains("TAILMARKER"),
            "the tail beyond the cap never arrived"
        );
        assert!(!log_path.exists(), "the raw log survived its run");
    }

    // ---- the log-cut flag: PBI 2026-11-21-24 ----
    // 観測 (2026-11-21, this branch): with a synthetic >PLAN_MAX_BYTES log the
    // plan file was saved with NO marker and truncate_plan returned false in
    // both fixtures (cap mid-Text-line / cap between Text lines) — the
    // finding held. The flag now travels to the plan assembly, so these tests
    // pin the visible cut instead.

    /// A synthetic text-event line carrying `text` as its plan prose — the
    /// real envelope shape, never a harvested line.
    #[cfg(unix)]
    fn big_text_line(text: &str) -> String {
        format!(
            r#"{{"type":"text","timestamp":1792904882135,"sessionID":"ses_demo","part":{{"id":"prt_demo","type":"text","text":"{text}"}}}}"#
        )
    }

    #[test]
    fn capture_headless_log_returns_whether_the_log_was_cut() {
        // The propagation's start: the capped read's cut verdict rides in the
        // capture's return. Exactly the cap is the whole file — not a cut —
        // and over the cap cuts. (The logged run rewrites each stream line
        // newline-terminated, so the fixture's lines carry the newline: 8192
        // lines of 8 bytes are exactly PLAN_MAX_BYTES.)
        let (_, _, text, cut) = capture_headless_log(
            "/bin/sh",
            &["-c", "yes 0123456 | head -n 8192"],
            "ignored",
            None,
            None,
        )
        .expect("the synthetic advisor streams");
        assert_eq!(text.len(), PLAN_MAX_BYTES);
        assert!(!cut, "an exact-cap log is the whole file, not a cut");

        let (_, _, text, cut) = capture_headless_log(
            "/bin/sh",
            &["-c", "yes 0123456 | head -n 8193"],
            "ignored",
            None,
            None,
        )
        .expect("the synthetic advisor streams");
        assert_eq!(text.len(), PLAN_MAX_BYTES);
        assert!(cut, "over the cap is a cut");
    }

    #[cfg(unix)]
    #[test]
    fn log_cut_mid_text_line_marks_the_saved_plan() {
        // BDD: 生ログの打ち切りが plan ファイルに視認可能に引き継がれる —
        // the cap lands mid-JSON-line (that line renders Silent, so its whole
        // text vanishes) and a distinctive tail Text line beyond the cap
        // never arrives; the saved plan must say so instead of reading as
        // complete. The stderr side is the capture-time note, pinned by
        // `the_log_truncation_note_*`.
        let filler = big_text_line(&"x".repeat(400));
        let tail = big_text_line("9. The tail line that must not be lost");
        let script = format!(
            "printf '%s\\n' '{TOOL_LINE}' '{TEXT_LINE}'; \
             yes '{filler}' | head -n 200; \
             printf '%s\\n' '{tail}'"
        );
        let (status, _, text, log_truncated) =
            capture_headless_log("/bin/sh", &["-c", &script], "ignored", None, None)
                .expect("the synthetic advisor streams");
        assert!(status.success(), "the synthetic exit status: {status}");
        assert!(log_truncated, "the fixture's log is over the cap");
        assert!(
            !text.contains("The tail line that must not be lost"),
            "the tail beyond the cap never arrived"
        );

        let dir = ScratchDir::new("saniwalens-handoff", "log-cut-mid-line");
        let filter = ScopeFilter::All;
        let copy = opencode_copy();
        let ctx = handoff(&dir, &filter, &copy);
        let mut advisor = DialogRecorder {
            launches: Vec::new(),
            status: success_status(),
        };

        let plan_path = complete_handoff(&text, log_truncated, &ctx, &mut advisor).unwrap();

        let saved = fs::read_to_string(&plan_path).unwrap();
        assert!(
            !saved.contains("The tail line that must not be lost"),
            "the tail must not reach the plan"
        );
        assert!(
            saved.contains("1. Add a file map to AGENTS.md"),
            "the head text is kept"
        );
        assert!(
            saved.ends_with(PLAN_TRUNCATION_MARKER),
            "the log cut is visible at the plan's end"
        );
        assert_eq!(
            saved.matches(PLAN_TRUNCATION_MARKER).count(),
            1,
            "exactly one marker"
        );
        assert_eq!(advisor.launches.len(), 1, "the dialog still opens");
    }

    #[cfg(unix)]
    #[test]
    fn log_cut_between_text_lines_marks_the_saved_plan() {
        // The second fixture: the cap lands in the tool-noise region, so no
        // Text line is itself cut; whole Text lines beyond the cap are
        // silently absent and the plan reads as complete — the quiet case the
        // marker must expose.
        let filler = big_text_line(&"x".repeat(400));
        let script = format!(
            "printf '%s\\n' '{TOOL_LINE}'; \
             yes '{filler}' | head -n 120; \
             yes '{TOOL_LINE}' | head -n 100; \
             printf '%s\\n' '{TEXT_LINE}'"
        );
        let (status, _, text, log_truncated) =
            capture_headless_log("/bin/sh", &["-c", &script], "ignored", None, None)
                .expect("the synthetic advisor streams");
        assert!(status.success(), "the synthetic exit status: {status}");
        assert!(log_truncated, "the fixture's log is over the cap");
        assert!(
            !text.contains("1. Add a file map to AGENTS.md"),
            "the final Text line beyond the cap never arrived"
        );

        let dir = ScratchDir::new("saniwalens-handoff", "log-cut-between");
        let filter = ScopeFilter::All;
        let copy = opencode_copy();
        let ctx = handoff(&dir, &filter, &copy);
        let mut advisor = DialogRecorder {
            launches: Vec::new(),
            status: success_status(),
        };

        let plan_path = complete_handoff(&text, log_truncated, &ctx, &mut advisor).unwrap();

        let saved = fs::read_to_string(&plan_path).unwrap();
        assert!(
            saved.ends_with(PLAN_TRUNCATION_MARKER),
            "the quiet missing tail is visible at the plan's end"
        );
        assert!(
            !saved.contains("1. Add a file map to AGENTS.md"),
            "the tail is gone but now marked"
        );
        assert_eq!(advisor.launches.len(), 1, "the dialog still opens");
    }

    #[cfg(unix)]
    #[test]
    fn complete_handoff_without_a_log_cut_saves_the_plan_untouched() {
        // The flag is false on every normal run: the saved plan must be
        // byte-identical to the extracted text, with no marker — the
        // no-cut contract, pinned at the completion's own boundary.
        let dir = ScratchDir::new("saniwalens-handoff", "no-cut");
        let filter = ScopeFilter::All;
        let copy = opencode_copy();
        let ctx = handoff(&dir, &filter, &copy);
        let mut advisor = DialogRecorder {
            launches: Vec::new(),
            status: success_status(),
        };
        let log_text = format!("{TOOL_LINE}\n{TEXT_LINE}\n");

        let plan_path =
            complete_handoff(&log_text, false, &ctx, &mut advisor).expect("a plain plan saves");

        assert_eq!(
            fs::read_to_string(&plan_path).unwrap(),
            "1. Add a file map to AGENTS.md"
        );
        assert!(
            !fs::read_to_string(&plan_path)
                .unwrap()
                .contains(PLAN_TRUNCATION_MARKER)
        );
    }

    #[cfg(unix)]
    #[test]
    fn complete_handoff_never_doubles_the_marker_when_both_cuts_fire() {
        // A log-cut flag on a plan that also exceeds the cap on its own
        // (only reachable by a direct call with uncapped text): one marker,
        // not two.
        let dir = ScratchDir::new("saniwalens-handoff", "both-cuts");
        let filter = ScopeFilter::All;
        let copy = opencode_copy();
        let ctx = handoff(&dir, &filter, &copy);
        let mut advisor = DialogRecorder {
            launches: Vec::new(),
            status: success_status(),
        };
        let big = "x".repeat(PLAN_MAX_BYTES);
        let big_line = format!(
            r#"{{"type":"text","timestamp":1792904882135,"sessionID":"ses_demo","part":{{"id":"prt_demo","type":"text","text":"{big}"}}}}"#
        );
        let log_text = format!("{TOOL_LINE}\n{TEXT_LINE}\n{big_line}\n");

        let plan_path =
            complete_handoff(&log_text, true, &ctx, &mut advisor).expect("the dialog opens");

        let saved = fs::read_to_string(&plan_path).unwrap();
        assert!(saved.ends_with(PLAN_TRUNCATION_MARKER));
        assert_eq!(
            saved.matches(PLAN_TRUNCATION_MARKER).count(),
            1,
            "the two cuts must not stack two markers"
        );
    }

    #[cfg(unix)]
    #[test]
    fn complete_handoff_still_refuses_an_empty_plan_with_a_log_cut() {
        // The empty-plan refusal precedes the marker: a cut log whose kept
        // head carries no Text still saves nothing and fails loud.
        let dir = ScratchDir::new("saniwalens-handoff", "cut-empty");
        let filter = ScopeFilter::All;
        let copy = opencode_copy();
        let ctx = handoff(&dir, &filter, &copy);
        let mut advisor = DialogRecorder {
            launches: Vec::new(),
            status: success_status(),
        };

        let err = complete_handoff(TOOL_LINE, true, &ctx, &mut advisor).unwrap_err();

        let rendered = format!("{err:#}");
        assert!(rendered.contains("no readable text"), "{rendered}");
        assert!(advisor.launches.is_empty(), "the dialog never opened");
    }
}
