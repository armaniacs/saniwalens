//! Delivery: how the composed briefing reaches the advisor (`docs/specs/cli.md`).
//!
//! `optimize` assembles a `DeliveryPlan`; `deliver` runs it against the
//! `AdvisorLaunch` process adapter. Launch modes write the briefing to a
//! private (`0600`) pointer file owned by the `BriefingFile` guard, so the
//! full text never rides argv. Split out of the `cli/mod.rs` shell (PBI C6);
//! the shell keeps the assembly call and the public re-export.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

use crate::adapter::harness::RunLineRenderer;
use crate::core::optimize as optimize_mod;
use crate::session::Style;

use super::headless;

/// How the briefing reaches the advisor — the three deliveries `optimize`
/// picks between (`docs/specs/cli.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryMode {
    Interactive,
    Headless,
    Print,
}

/// `--print` wins over the harness's launch shape.
pub(crate) fn delivery_mode(print: bool, launch: optimize_mod::RunMode) -> DeliveryMode {
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

    /// Spawn headless with `--verbose`: the same logged pipe — run log,
    /// heartbeat, and the kill/reap deadline — with the raw stream relayed
    /// verbatim to the user's stdout instead of rendered. No renderer rides
    /// this route: the raw bytes are the display.
    fn headless_verbose(
        &mut self,
        binary: &str,
        leading: &[&str],
        prompt: &str,
        timeout: Option<std::time::Duration>,
    ) -> Result<(std::process::ExitStatus, PathBuf)> {
        // A double that never serves the verbose headless route keeps only
        // the required methods; reaching this default is a route/double
        // mismatch a test must fix, never a behavior to fall back to (the
        // old fallback — the direct spawn — is the unbounded bug this route
        // exists to close).
        let _ = (binary, leading, prompt, timeout);
        unimplemented!("this advisor double does not implement the verbose headless route")
    }
}

/// The real process side: an inherited-stdio spawn, or `run_headless`'s
/// logged pipe.
pub(crate) struct ProcessAdvisor;

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

    fn headless_verbose(
        &mut self,
        binary: &str,
        leading: &[&str],
        prompt: &str,
        timeout: Option<std::time::Duration>,
    ) -> Result<(std::process::ExitStatus, PathBuf)> {
        headless::run_headless_streaming(
            binary,
            leading,
            prompt,
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
pub(crate) struct BriefingFile {
    path: Option<PathBuf>,
}

impl BriefingFile {
    /// Take ownership of the pointer file at `path`.
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    /// The path the argv pointer closes over. The guard must outlive the
    /// advisor run: dropping it first would pull the file while the advisor
    /// still needs it.
    pub(crate) fn path(&self) -> &Path {
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
    // The direct route never applies a deadline — the advisor owns the
    // terminal for a live session, and killing it on a timer would cut an
    // active conversation — so an explicit non-zero `--timeout` would be
    // silently discarded there. The default (absent flag) stays silent:
    // nothing the user asked for is being dropped. An explicit zero is
    // already covered by `resolve_timeout`'s own warning, and an explicit
    // value equal to the default is indistinguishable here and stays silent.
    if mode == DeliveryMode::Interactive
        && let Some(warning) = direct_timeout_warning(timeout)
    {
        eprintln!("{}", Style::stderr().warn(&warning));
    }
    // Headless harnesses run piped with or without --verbose: the raw stream
    // is logged to a private file either way, and `--verbose` adds the raw
    // bytes relayed verbatim to stdout instead of the rendered lines. A
    // success removes the run log (`SANIWALENS_KEEP_RUN_LOG=1` keeps it) —
    // only a failure leaves it behind for the bail below to name.
    let (result, run_log) = match (mode, verbose) {
        (DeliveryMode::Headless, false) => {
            match advisor.headless(binary, leading, &prompt, render, timeout) {
                Ok((status, log)) => (Ok(status), Some(log)),
                Err(err) => (Err(err), None),
            }
        }
        (DeliveryMode::Headless, true) => {
            match advisor.headless_verbose(binary, leading, &prompt, timeout) {
                Ok((status, log)) => (Ok(status), Some(log)),
                Err(err) => (Err(err), None),
            }
        }
        _ => (advisor.direct(binary, leading, &prompt), None),
    };

    // Headless errors already name the raw log and, for a spawn failure, the
    // launch hint — quiet and verbose alike; the interactive path keeps the
    // launch hint as its context. The briefing guard drops here, after the
    // run: every exit above, including an early return or a panic unwind,
    // removes the file.
    let status = match (mode, verbose) {
        (DeliveryMode::Headless, _) => result?,
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

/// The stderr warning for an explicit non-zero `--timeout` on the direct
/// (interactive) route, where no deadline applies. `None` when the delivery
/// must stay silent: no timeout at all, or the default the absent flag
/// resolves to — an interactive run must not warn about a flag it never
/// received.
fn direct_timeout_warning(timeout: Option<std::time::Duration>) -> Option<String> {
    let limit = timeout?;
    if limit == headless::DEFAULT_BRIEFING_TIMEOUT {
        return None;
    }
    Some(format!(
        "--timeout {limit:?} bounds headless runs only: the interactive session runs without a deadline"
    ))
}

/// The collision-resistance components of every private temp name: the pid,
/// the clock nanos, and a per-process call sequence. Callers format these
/// with their own component order, separators, and base so each site's
/// established name stays byte-identical.
///
/// The clock is here to avoid a collision with a leftover file, not to be
/// unguessable — exclusive creation is what makes guessing the name useless.
/// The sequence is what makes two calls unique: parallel tests share the
/// pid and can land on the same clock tick, and without it the loser fails
/// with `AlreadyExists`.
pub(crate) struct UniqueTempStem {
    /// Separates concurrent processes sharing the same directory.
    pub pid: u32,
    /// Separates a process from its own leftovers from earlier runs.
    pub nanos: u128,
    /// Separates two calls landing on the same clock tick.
    pub seq: u64,
}

/// A fresh stem, unique per process and call, for private temp names. The
/// one place the pid + clock + sequence idiom lives; format locally.
pub(crate) fn unique_temp_stem() -> UniqueTempStem {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    UniqueTempStem {
        pid: std::process::id(),
        nanos: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos()),
        seq: SEQ.fetch_add(1, Ordering::Relaxed),
    }
}

/// A fresh private temp path for `prefix`, unique per process and call.
pub(crate) fn private_temp_path(prefix: &str, ext: &str) -> PathBuf {
    let stem = unique_temp_stem();
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{:x}-{}.{}",
        stem.pid, stem.nanos, stem.seq, ext
    ))
}

/// Write `contents` to a fresh file in the temp dir. Used for the optimization
/// briefing, which may hold sensitive paths/excerpts and must not sit on argv or
/// be world-readable.
pub(crate) fn write_private_tempfile(contents: &str) -> Result<PathBuf> {
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

/// Create `dir` and any missing parents, for the current user only.
///
/// `create_dir_all` with owner-only permissions on every directory it creates —
/// the mode the XDG Base Directory spec prescribes for a directory a program
/// creates on the user's behalf. Directories that already exist keep theirs.
/// Like `write_private_file`, this makes cli-owned state artifacts (plans,
/// backups) born private instead of created under the umask and narrowed
/// afterwards.
pub(crate) fn create_private_dir_all(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    // `Harness::run_line_renderer` only appears in the `#[cfg(unix)]` delivery
    // tests below (process-spawn doubles need `ExitStatusExt`); ungated, the
    // import is unused on Windows and `-D warnings` fails the lint job.
    #[cfg(unix)]
    use crate::adapter::harness::Harness;
    #[cfg(unix)]
    use std::time::Duration;

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

    // ---- PBI 06: the delivery seam ----

    /// A delivery double: records what would have been launched instead of
    /// spawning anything, so the briefing's argv can be asserted against a
    /// mock (BDD: "従来と同一の引数で claude が起動する").
    #[cfg(unix)]
    struct RecordingAdvisor {
        direct: Vec<(String, Vec<String>, String)>,
        headless: Vec<(String, Vec<String>, String)>,
        verbose_headless: Vec<(String, Vec<String>, String, Option<std::time::Duration>)>,
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
                verbose_headless: Vec::new(),
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

        fn headless_verbose(
            &mut self,
            binary: &str,
            leading: &[&str],
            prompt: &str,
            timeout: Option<std::time::Duration>,
        ) -> Result<(std::process::ExitStatus, PathBuf)> {
            self.verbose_headless.push((
                binary.to_string(),
                leading.iter().map(|s| s.to_string()).collect(),
                prompt.to_string(),
                timeout,
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
    fn headless_routes_through_the_logged_adapter_with_or_without_verbose() {
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

        // `--verbose` keeps the logged transport — the run log and the
        // kill/reap deadline stay in force — and relays the raw stream
        // verbatim instead of rendering it (`docs/specs/harness.md`).
        let verbose_plan = DeliveryPlan::launch(
            ("opencode", &["run", "--format", "json"]),
            DeliveryMode::Headless,
            "briefing".to_string(),
            "opencode",
            true,
            Harness::Opencode.run_line_renderer(),
            Some(Duration::from_secs(5)),
            |_| "pointer".to_string(),
        );
        let mut advisor = RecordingAdvisor::new();
        deliver(verbose_plan, &mut advisor).unwrap();
        assert_eq!(
            advisor.verbose_headless.len(),
            1,
            "verbose headless keeps the logged transport"
        );
        assert_eq!(
            advisor.verbose_headless[0].3,
            Some(Duration::from_secs(5)),
            "the explicit timeout reaches the verbose route too"
        );
        assert!(
            advisor.direct.is_empty(),
            "verbose headless is not the unbounded direct spawn"
        );
    }

    #[test]
    fn the_direct_route_warns_only_on_an_explicit_non_default_timeout() {
        // No flag: the default resolves upstream; warning about a timeout the
        // user never passed would be noise on every interactive run.
        assert_eq!(direct_timeout_warning(None), None);
        assert_eq!(
            direct_timeout_warning(Some(headless::DEFAULT_BRIEFING_TIMEOUT)),
            None
        );
        // An explicit non-zero value the route cannot honor is named.
        let warning = direct_timeout_warning(Some(std::time::Duration::from_secs(5)))
            .expect("an explicit timeout warns on the direct route");
        assert!(
            warning.contains("--timeout 5s") && warning.contains("interactive"),
            "the warning names the value and the route: {warning}"
        );
        // An explicit zero never reaches this warning: resolve_timeout maps
        // it to None, whose existing warning already covers it — no double.
        assert_eq!(direct_timeout_warning(None), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_verbose_headless_route_still_bounds_the_run_with_the_timeout() {
        // A hung advisor on `--verbose` must hit the same deadline as a quiet
        // run: the timeout reaches the verbose headless route, which is not
        // the unbounded direct spawn.
        let plan = DeliveryPlan::launch(
            ("opencode", &["run", "--format", "json"]),
            DeliveryMode::Headless,
            "briefing".to_string(),
            "opencode",
            true,
            Harness::Opencode.run_line_renderer(),
            Some(Duration::from_secs(5)),
            |_| "pointer".to_string(),
        );
        let mut advisor = RecordingAdvisor::new();

        deliver(plan, &mut advisor).unwrap();

        assert_eq!(
            advisor.verbose_headless.len(),
            1,
            "verbose headless keeps the logged transport"
        );
        assert_eq!(
            advisor.verbose_headless[0].3,
            Some(Duration::from_secs(5)),
            "the explicit timeout reaches the verbose headless route"
        );
        assert!(
            advisor.direct.is_empty(),
            "verbose headless is not the unbounded direct spawn"
        );
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
}
