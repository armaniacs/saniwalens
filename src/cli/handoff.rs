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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

use crate::adapter::harness::{Harness, RunLine, RunLineRenderer, extract_plan_text};
use crate::core::optimize as optimize_mod;
use crate::core::period::TimeRange;
use crate::core::scope::ScopeFilter;
use crate::session::{Session, SessionEnv, Style};
use crate::store::create_private_dir_all;

use super::deliver::{BriefingFile, ProcessAdvisor, write_private_tempfile};
use super::headless::{Beat, run_headless};
use super::{AdvisorLaunch, collect_findings, run_analyze, write_private_file};

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
    if harness.launch_command().2 == optimize_mod::RunMode::Interactive {
        anyhow::bail!(
            "`--apply` needs a headless harness (opencode): the {} session is already \
             interactive and applies its own plan",
            harness.display_name()
        );
    }
    Ok(())
}

/// The state-home side both handoff dirs live under:
/// `$XDG_STATE_HOME/saniwalens/`, else `~/.local/state/saniwalens/`.
/// The same XDG rule as the store (`session.rs`): an absolute XDG value wins,
/// a relative one is ignored, and without an absolute home there is no
/// answer. Never cwd-relative.
fn state_saniwalens_dir(state_home: Option<&Path>, home: Option<&Path>) -> Result<PathBuf> {
    // The store's own rule (`session.rs`): both roots must be absolute, so a
    // relative root can never put the handoff dirs back under the current
    // directory.
    let base = match state_home.filter(|dir| dir.is_absolute()) {
        Some(dir) => dir.to_path_buf(),
        None => home
            .filter(|dir| dir.is_absolute())
            .context("HOME is not set to an absolute path")?
            .join(".local")
            .join("state"),
    };
    Ok(base.join("saniwalens"))
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

/// A fresh plan file name, unique per process and call like the private temp
/// paths: the clock guards against a leftover file, the pid and the sequence
/// against two calls landing on the same tick.
fn plan_filename() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("plan-{nanos:x}-{}-{seq}.md", std::process::id())
}

/// Write `plan` to a fresh file under `dir`, for the current user only
/// (`0600`, exclusive creation like the briefing temp files). Creates `dir`
/// when missing with the store's owner-only rule (`0700`, like the store
/// dir): plan text may echo config secrets. Returns the saved path.
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
/// it. Pure except the plan-file write and the `direct` spawn, so a double
/// covers every branch without spawning: an empty plan writes nothing and
/// fails loud, and a failed launch names the kept plan file.
pub fn complete_handoff(
    log_text: &str,
    handoff: &Handoff<'_>,
    advisor: &mut impl AdvisorLaunch,
) -> Result<PathBuf> {
    let rendered: Vec<RunLine> = log_text
        .lines()
        .map(|line| {
            handoff
                .render
                .map_or(RunLine::Silent, |render| render(line))
        })
        .collect();
    let plan = extract_plan_text(&rendered);
    if plan.trim().is_empty() {
        anyhow::bail!(
            "the advisor produced no readable text, so no plan file was written \
             (raw log removed)"
        );
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
/// the log path, and the log's text. A success removes the log (the raw
/// stream may echo secrets; the plan file carries the content from here on);
/// a failure keeps it and names it in the error, like the normal headless
/// delivery. The error states that no plan file was written yet.
pub fn capture_headless_log(
    binary: &str,
    leading: &[&str],
    prompt: &str,
    render: Option<RunLineRenderer>,
    timeout: Option<std::time::Duration>,
) -> Result<(std::process::ExitStatus, PathBuf, String)> {
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
    let text = fs::read_to_string(&log_path)
        .with_context(|| format!("read the headless run log {}", log_path.display()))?;
    // The raw stream may echo secrets; the plan file carries the content from
    // here on, so the log goes now.
    let _ = fs::remove_file(&log_path);
    Ok((status, log_path, text))
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
    if run_mode != optimize_mod::RunMode::Headless {
        anyhow::bail!(
            "`--apply` needs a headless harness (opencode): the {} session is already \
             interactive and applies its own plan",
            sess.harness.display_name()
        );
    }
    // Fail fast before the expensive store refresh: without a writable plans
    // home there is nothing to hand off to.
    let plans = plans_dir(env.state_home, env.home.map(Path::new))?;
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

    // The harness copy for every prompt rendered here — the harness assembles
    // its own (adapter side), so the core stays agnostic. The resolved
    // advisor language rides the same copy, like the normal `optimize` path.
    let mut copy = sess.harness.copy();
    copy.language = sess.language;

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
    let (_, _, log_text) = capture_headless_log(
        binary,
        leading,
        &prompt,
        sess.harness.run_line_renderer(),
        timeout,
    )?;
    let handoff = Handoff {
        render: sess.harness.run_line_renderer(),
        plans: &plans,
        db_display: &db_display,
        filter,
        copy: &copy,
        harness: sess.harness,
    };
    let mut advisor = ProcessAdvisor;
    let plan_path = complete_handoff(&log_text, &handoff, &mut advisor)?;
    println!("{}", saved_note(&plan_path));
    eprintln!("{}", Style::stderr().dim(&cleanup_note(&plans)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{OpenMode, Store};

    /// A synthetic `--format json` tool-event line: the noise a headless run
    /// streams around the agent's text. The real envelope shape, never a
    /// harvested line.
    const TOOL_LINE: &str = r#"{"type":"tool_use","timestamp":1792904532992,"sessionID":"ses_demo","part":{"partID":"prt_demo","type":"tool","tool":"read","state":{"status":"completed"}}}"#;

    /// A synthetic text-event line carrying plan prose.
    const TEXT_LINE: &str = r#"{"type":"text","timestamp":1792904882135,"sessionID":"ses_demo","part":{"id":"prt_demo","type":"text","text":"1. Add a file map to AGENTS.md"}}"#;

    fn opencode_copy() -> optimize_mod::HarnessCopy {
        Harness::Opencode.copy()
    }

    /// A plans sandbox under the temp dir, removed on drop so a failure does
    /// not leak plan files into the runner's temp dir.
    struct PlansDir(PathBuf);

    impl PlansDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "saniwalens-handoff-test-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&dir);
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for PlansDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
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
        let dir = PlansDir::new("unique");
        let first = save_plan(dir.path(), "plan one").unwrap();
        let second = save_plan(dir.path(), "plan two").unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read_to_string(&first).unwrap(), "plan one");
        assert_eq!(fs::read_to_string(&second).unwrap(), "plan two");
    }

    #[cfg(unix)]
    #[test]
    fn save_plan_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let dir = PlansDir::new("mode");
        let path = save_plan(dir.path(), "secret plan").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn save_plan_creates_the_plans_dir_owner_only() {
        // Plan text may echo config secrets, so the plans dir follows the
        // store's rule: freshly created dirs are 0700, like the store dir.
        use std::os::unix::fs::PermissionsExt;

        let base = PlansDir::new("dir-mode");
        let nested = base.path().join("nested").join("plans");
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
        let dir = PlansDir::new("complete");
        let filter = ScopeFilter::All;
        let copy = opencode_copy();
        let ctx = handoff(dir.path(), &filter, &copy);
        let log_text = format!("{TOOL_LINE}\n{TEXT_LINE}\nnot json at all\n");
        let mut advisor = DialogRecorder {
            launches: Vec::new(),
            status: success_status(),
        };

        let plan_path = complete_handoff(&log_text, &ctx, &mut advisor).unwrap();

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
        let dir = PlansDir::new("empty");
        let filter = ScopeFilter::All;
        let copy = opencode_copy();
        let ctx = handoff(dir.path(), &filter, &copy);
        let mut advisor = DialogRecorder {
            launches: Vec::new(),
            status: success_status(),
        };

        let err = complete_handoff(TOOL_LINE, &ctx, &mut advisor).unwrap_err();

        assert!(format!("{err:#}").contains("no readable text"), "{err:#}");
        assert!(format!("{err:#}").contains("no plan file"), "{err:#}");
        assert!(advisor.launches.is_empty(), "the dialog never opened");
        assert!(
            fs::read_dir(dir.path())
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
        let dir = PlansDir::new("dialog-fail");
        let filter = ScopeFilter::All;
        let copy = opencode_copy();
        let ctx = handoff(dir.path(), &filter, &copy);
        let mut advisor = DialogRecorder {
            launches: Vec::new(),
            status: failure_status(),
        };

        let err = complete_handoff(TEXT_LINE, &ctx, &mut advisor).unwrap_err();

        let rendered = format!("{err:#}");
        assert!(rendered.contains("kept"), "{rendered}");
        let saved: PathBuf = fs::read_dir(dir.path())
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

    #[test]
    fn run_apply_refuses_interactive_harnesses_before_any_spawn() {
        // Defense in depth behind the arm's pre-check: an interactive harness
        // fails here without spawning anything and without touching the store.
        let dir = PlansDir::new("refuse");
        fs::create_dir_all(dir.path()).unwrap();
        let db = dir.path().join("s.db");
        let store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let sess = Session {
            harness: Harness::Claude,
            db_path: db,
            store,
            language: optimize_mod::AdvisorLanguage::default(),
        };
        let env = SessionEnv {
            state_home: None,
            home: None,
            config_home: None,
        };
        let err = run_apply(&sess, None, &ScopeFilter::All, true, None, &env).unwrap_err();
        assert!(format!("{err:#}").contains("headless"), "{err:#}");
    }

    #[test]
    fn run_apply_resolves_the_plans_home_before_any_spawn() {
        // Fail fast before the expensive headless run: without a plans home
        // there is nothing to hand off to. The short timeout is only a
        // backstop — a regression that spawns anyway must still end the test.
        let dir = PlansDir::new("nohome");
        fs::create_dir_all(dir.path()).unwrap();
        let db = dir.path().join("s.db");
        let store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let sess = Session {
            harness: Harness::Opencode,
            db_path: db,
            store,
            language: optimize_mod::AdvisorLanguage::default(),
        };
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
        let dir = PlansDir::new("nohome-unfrozen");
        fs::create_dir_all(dir.path()).unwrap();
        let db = dir.path().join("s.db");
        let store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let sess = Session {
            harness: Harness::Opencode,
            db_path: db,
            store,
            language: optimize_mod::AdvisorLanguage::default(),
        };
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
        let (status, log_path, text) = capture_headless_log(
            "/bin/sh",
            &["-c", &script],
            "ignored",
            Harness::Opencode.run_line_renderer(),
            None,
        )
        .expect("the synthetic advisor streams");
        assert!(status.success(), "the synthetic exit status: {status}");
        assert!(text.contains(TEXT_LINE), "the raw line reached the text");
        assert!(text.contains(TOOL_LINE), "every raw line is captured");
        assert!(!log_path.exists(), "the raw log survived its run");
    }
}
