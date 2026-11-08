//! The headless advisor run: a logged pipe around the harness's launch, with
//! one guard owning the child and the heartbeat for the run's whole lifetime.
//!
//! `run_headless` used to keep the stop/join on the normal path's last lines,
//! so any of its five `?` returns left the heartbeat running and the child
//! unreaped (`Child`'s drop neither kills nor waits). The guard here makes the
//! lifetime a scope fact: every exit, including an error return, stops the beat
//! and reaps the child. The raw log path is carried into the error so a failed
//! run still names its full output, while a successful run removes it (it may
//! echo secrets) unless `SANIWALENS_KEEP_RUN_LOG=1` keeps it
//! (`docs/specs/harness.md`).

use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::adapter::harness::{RunLine, RunLineRenderer};
use crate::session::Style;

use super::heartbeat;
pub(crate) use super::heartbeat::Beat;
use super::{private_temp_path, write_private_file};

/// One headless run's resources: the private log, the child's stderr handle,
/// the child itself, and the heartbeat thread. The guard owns all of them, so
/// dropping it on any exit path stops the beat and reaps the child.
pub(crate) struct HeadlessRun {
    stop: Arc<AtomicBool>,
    events: Arc<AtomicUsize>,
    /// The receiver keeps the channel alive for the thread's sends; the run
    /// never reads it (the beat is the user-facing side), but dropping it
    /// before the thread would make every send fail.
    _beats: Receiver<(u64, usize)>,
    log_path: PathBuf,
    log: fs::File,
    child_err: Option<fs::File>,
    child: Option<Child>,
    reader: Option<JoinHandle<()>>,
    heartbeat: Option<JoinHandle<()>>,
}

impl HeadlessRun {
    /// Create the private log and start the beat. The child is adopted after a
    /// successful spawn; until then the guard still owns the thread, so a
    /// spawn failure stops it on the way out.
    fn start(beat: Beat) -> Result<Self> {
        let log_path = private_temp_path("saniwalens-run", "log");
        write_private_file(&log_path, "").context("create run log")?;
        let log = fs::OpenOptions::new()
            .append(true)
            .open(&log_path)
            .context("open run log")?;
        let child_err = fs::OpenOptions::new()
            .append(true)
            .open(&log_path)
            .context("open run log for the advisor")?;

        eprintln!(
            "{}",
            Style::stderr().dim(&format!(
                "advisor running headlessly — raw progress: {}",
                log_path.display()
            ))
        );

        // A dim beat every cadence while the advisor works, so silence reads
        // as waiting, not a hang — the quiet-stretch logic lives in
        // `heartbeat`.
        let events = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (heartbeat, beats) = heartbeat::spawn(Arc::clone(&stop), Arc::clone(&events), beat);

        Ok(Self {
            stop,
            events,
            _beats: beats,
            log_path,
            log,
            child_err: Some(child_err),
            child: None,
            reader: None,
            heartbeat: Some(heartbeat),
        })
    }

    /// Take the spawned child. From here on the guard owns its lifetime.
    fn adopt(&mut self, child: Child) {
        self.child = Some(child);
    }

    /// The child's stderr handle, moved into the spawned command.
    fn take_child_err(&mut self) -> Option<fs::File> {
        self.child_err.take()
    }

    /// The child's piped stdout, taken before the read loop.
    fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.as_mut().and_then(|child| child.stdout.take())
    }

    /// The thread draining stdout. The guard joins it after killing the child,
    /// so a timeout cannot leave a reader blocked on a dead pipe.
    fn adopt_reader(&mut self, reader: JoinHandle<()>) {
        self.reader = Some(reader);
    }

    /// Stop owning the reader. A timeout can leave a grandchild holding the
    /// stdout pipe, so joining here would wait for that grandchild; detaching
    /// lets the timeout return while the reader exits when the pipe closes.
    fn abandon_reader(&mut self) {
        self.reader.take();
    }

    /// Count one streamed line for the beat.
    fn count_event(&self) {
        self.events.fetch_add(1, Ordering::Relaxed);
    }

    fn log(&mut self) -> &mut fs::File {
        &mut self.log
    }

    fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Wait for the child and hand back its status. The child stays in the
    /// guard until the wait succeeds, so a failed wait still leaves it for
    /// `Drop` to kill and reap; only a waited child is taken out.
    fn wait_for_status(&mut self, deadline: Option<Instant>) -> Result<ExitStatus> {
        let Some(deadline) = deadline else {
            let status = {
                let child = self
                    .child
                    .as_mut()
                    .context("the advisor child is missing")?;
                child.wait().context("wait for the advisor")
            };
            if status.is_ok() {
                self.child = None;
            }
            return status;
        };

        loop {
            let status = {
                let child = self
                    .child
                    .as_mut()
                    .context("the advisor child is missing")?;
                child.try_wait().context("wait for the advisor")?
            };
            if let Some(status) = status {
                self.child = None;
                return Ok(status);
            }
            if Instant::now() >= deadline {
                anyhow::bail!("the advisor did not exit before the timeout");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for HeadlessRun {
    fn drop(&mut self) {
        // No panic here: this runs during unwinds, where a second panic aborts
        // the process. Every teardown step is best-effort.
        self.stop.store(true, Ordering::Relaxed);
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.heartbeat.take() {
            let _ = handle.join();
        }
    }
}

/// The headless wait when `--timeout` is absent: a hung advisor is an error,
/// not an indefinite wait. An explicit `--timeout 0` opts back into no
/// deadline, with a warning on stderr.
pub(crate) const DEFAULT_BRIEFING_TIMEOUT: Duration = Duration::from_secs(60);

/// Parse `--timeout`: a positive number with an optional `ms`, `s`, or `m`
/// suffix; a bare number is seconds. Zero (any unit) disables the deadline —
/// the explicit infinite opt-out. Negatives and garbage are refused, since a
/// negative deadline would fail before the advisor could start.
pub(crate) fn parse_timeout(value: Option<&str>) -> Result<Option<Duration>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    let (number, unit_secs) = if let Some(rest) = value.strip_suffix("ms") {
        (rest, 0.001)
    } else if let Some(rest) = value.strip_suffix('s') {
        (rest, 1.0)
    } else if let Some(rest) = value.strip_suffix('m') {
        (rest, 60.0)
    } else {
        (value, 1.0)
    };
    let amount: f64 = number
        .parse()
        .with_context(|| format!("invalid --timeout '{value}' (e.g. 30s, 2m, 500ms)"))?;
    if !amount.is_finite() || amount < 0.0 {
        anyhow::bail!("--timeout must not be negative (got '{value}')");
    }
    if amount == 0.0 {
        return Ok(None);
    }
    let secs = amount * unit_secs;
    // `from_secs_f64` panics past `Duration`'s range; the fallible form
    // turns a gigantic-but-finite input into a regular error instead.
    // (`amount` and the unit are both positive and finite, so `secs` can
    // only be finite or +inf — never NaN — and only overflow can fail.)
    Ok(Some(Duration::try_from_secs_f64(secs).with_context(
        || format!("--timeout too large: '{value}'"),
    )?))
}

/// Resolve the `--timeout` flag to the wait the delivery uses: an absent flag
/// means the default, an explicit zero means no deadline (with a warning — a
/// hung advisor then waits forever), anything else parses as-is. The single
/// choke point, so absence and infinity never share a bare `None`.
pub(crate) fn resolve_timeout(value: Option<&str>) -> Result<Option<Duration>> {
    let Some(raw) = value else {
        return Ok(Some(DEFAULT_BRIEFING_TIMEOUT));
    };
    let parsed = parse_timeout(Some(raw))?;
    if parsed.is_none() {
        eprintln!(
            "{}",
            Style::stderr().warn("--timeout 0 disables the deadline: a hung advisor waits forever")
        );
    }
    Ok(parsed)
}

/// Run the headless advisor with its raw output logged: spawn with stdout
/// piped, write every raw line (and the child's stderr) to a private log, and
/// surface only what the agent says — text parts on stdout (so a pipe like
/// `| pbcopy` receives the narration and the plan), one-line progress markers
/// on stderr. The `render_line` is the harness seam's choice
/// (`Harness::run_line_renderer`), so this loop names no harness. A heartbeat
/// covers quiet stretches (a long tool call, model thinking) so the run never
/// looks hung. Returns the status and the log path: a failure keeps the log
/// and the caller points at it, while a success removes it (the raw stream
/// may echo secrets) unless `keep_log` — wired to `SANIWALENS_KEEP_RUN_LOG=1`
/// — keeps it. The raw event shapes are unversioned upstream, so rendering is
/// lenient: a line with no recognizable text stays in the log, and a run that
/// renders nothing readable says so instead of leaving silence.
/// The context prefix that carries the raw log's path. One constant so the
/// failure path and the test-side reader cannot drift apart.
const RAW_LOG_CONTEXT: &str = "raw output";

/// The environment variable that keeps the advisor's raw run log after a
/// successful headless run, for post-mortem inspection. Mirrors
/// `SANIWALENS_KEEP_SCRATCH=1` (`src/scratch.rs`): the value must be exactly
/// `1`, anything else (unset included) removes the log on success. Failures
/// always keep the log — the error names it — so this only affects success.
pub(crate) const KEEP_RUN_LOG_ENV: &str = "SANIWALENS_KEEP_RUN_LOG";

/// Whether the KEEP opt-in is set for the run log.
pub(crate) fn keep_run_log() -> bool {
    std::env::var_os(KEEP_RUN_LOG_ENV).as_deref() == Some(std::ffi::OsStr::new("1"))
}

/// Whether the run log is removed: only a success without the KEEP opt-in.
/// Every other combination keeps it — a failure's error names the path, and
/// an opted-in success keeps it for inspection.
fn should_remove_run_log(success: bool, keep: bool) -> bool {
    success && !keep
}

pub(crate) fn run_headless(
    binary: &str,
    leading: &[&str],
    prompt: &str,
    render_line: Option<RunLineRenderer>,
    beat: Beat,
    timeout: Option<Duration>,
    keep_log: bool,
) -> Result<(ExitStatus, PathBuf)> {
    let mut run = HeadlessRun::start(beat)?;
    let log_path = run.log_path().to_path_buf();
    let deadline = timeout.map(|limit| Instant::now() + limit);
    let (status, readable) = match drive(&mut run, binary, leading, prompt, render_line, deadline) {
        Ok(driven) => driven,
        Err(err) => {
            return Err(err).with_context(|| format!("{RAW_LOG_CONTEXT}: {}", log_path.display()));
        }
    };
    drop(run);
    if should_remove_run_log(status.success(), keep_log) {
        // Best-effort like the briefing's `let _ = fs::remove_file(&path)`
        // (`cli/mod.rs`): the run is done, and a removal failure must not
        // fail the delivery. The no-readable-text note stays path-less here
        // so it never names a file that is already gone.
        let _ = fs::remove_file(&log_path);
        if !readable {
            eprintln!(
                "{}",
                Style::stderr().dim("the advisor produced no readable text — run log removed")
            );
        }
    } else if status.success() && !readable {
        eprintln!(
            "{}",
            Style::stderr().dim(&format!(
                "the advisor produced no readable text — full output: {}",
                log_path.display()
            ))
        );
    }
    Ok((status, log_path))
}

/// The I/O half: spawn, adopt, stream, and wait. Every `?` here returns to
/// `run_headless`, whose guard does the teardown on the way out.
fn drive(
    run: &mut HeadlessRun,
    binary: &str,
    leading: &[&str],
    prompt: &str,
    render_line: Option<RunLineRenderer>,
    deadline: Option<Instant>,
) -> Result<(ExitStatus, bool)> {
    let mut launch = Command::new(binary);
    launch.args(leading).arg(prompt);
    launch.stdout(Stdio::piped());
    if let Some(child_err) = run.take_child_err() {
        launch.stderr(Stdio::from(child_err));
    }
    let child = launch.spawn().with_context(|| {
        format!(
            "launch `{binary} {}` — is it installed and on PATH?",
            leading.join(" ")
        )
    })?;
    run.adopt(child);
    let stdout = run.take_stdout().context("pipe the advisor's stdout")?;

    // A reader thread makes the deadline reach the blocking `read_line`: the
    // main loop waits on the channel, so a hung child cannot pin it.
    let (line_tx, line_rx) = channel();
    let reader = std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            if line_tx.send(line).is_err() {
                break;
            }
        }
    });
    run.adopt_reader(reader);

    let mut readable = false;
    loop {
        let line = match next_line(&line_rx, deadline) {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(err) => {
                run.abandon_reader();
                return Err(err);
            }
        };
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                run.abandon_reader();
                return Err(err).context("read the advisor's output");
            }
        };
        run.count_event();
        writeln!(run.log(), "{line}").context("write the run log")?;
        match render_line.map_or(RunLine::Silent, |render| render(&line)) {
            RunLine::Text(text) => {
                readable = true;
                println!("{text}");
            }
            RunLine::Progress(tool) => {
                eprintln!("{}", Style::stderr().dim(&format!("  → {tool}")));
            }
            RunLine::Silent => {}
        }
    }
    let status = run.wait_for_status(deadline)?;
    Ok((status, readable))
}

/// The next raw line, or `None` when the reader disconnected. With a deadline,
/// a silent child becomes a timeout instead of an indefinite wait.
fn next_line(
    lines: &Receiver<std::io::Result<String>>,
    deadline: Option<Instant>,
) -> Result<Option<std::io::Result<String>>> {
    let Some(deadline) = deadline else {
        return match lines.recv() {
            Ok(line) => Ok(Some(line)),
            Err(_) => Ok(None),
        };
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        anyhow::bail!("the advisor did not finish before the timeout");
    }
    match lines.recv_timeout(remaining) {
        Ok(line) => Ok(Some(line)),
        Err(RecvTimeoutError::Timeout) => {
            anyhow::bail!("the advisor did not finish before the timeout")
        }
        Err(RecvTimeoutError::Disconnected) => Ok(None),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn fast_beat() -> Beat {
        Beat::new(Duration::from_millis(1), Duration::from_millis(5))
            .expect("the fast test beat is non-zero")
    }

    /// The raw log path carried by the failure context. The outermost
    /// `to_string()` would lose it, so walk the whole chain.
    fn raw_log_path(err: &anyhow::Error) -> Option<PathBuf> {
        err.chain().find_map(|cause| {
            let text = cause.to_string();
            let rest = text.strip_prefix(RAW_LOG_CONTEXT)?;
            let path = rest.strip_prefix(": ")?;
            Some(PathBuf::from(path))
        })
    }

    #[test]
    fn the_raw_log_path_is_found_through_the_context_chain() {
        let err = anyhow::anyhow!("boom").context("raw output: /tmp/saniwalens-run-example.log");
        assert_eq!(
            raw_log_path(&err),
            Some(PathBuf::from("/tmp/saniwalens-run-example.log"))
        );
    }

    #[test]
    fn the_raw_log_path_is_absent_without_the_context() {
        let err = anyhow::anyhow!("boom");
        assert_eq!(raw_log_path(&err), None);
    }

    #[test]
    fn should_remove_run_log_only_on_success_without_the_keep_opt_in() {
        assert!(should_remove_run_log(true, false));
        assert!(!should_remove_run_log(true, true));
        assert!(!should_remove_run_log(false, false));
        assert!(!should_remove_run_log(false, true));
    }

    #[test]
    fn a_bad_utf8_line_reaps_the_child_and_names_the_log() {
        let pid_file =
            std::env::temp_dir().join(format!("saniwalens-headless-pid-{}", std::process::id()));
        let script = format!(
            "echo $$ > {}; printf '\\377\\n'; sleep 5",
            pid_file.display()
        );

        let err = run_headless(
            "/bin/sh",
            &["-c", script.as_str()],
            "ignored",
            None,
            fast_beat(),
            None,
            false,
        )
        .expect_err("the invalid UTF-8 line fails the read");
        let log = raw_log_path(&err).expect("the failure names the raw log");
        let _ = std::fs::remove_file(log);

        let pid = std::fs::read_to_string(&pid_file).expect("the child wrote its pid");
        let _ = std::fs::remove_file(&pid_file);
        let alive = Command::new("kill")
            .arg("-0")
            .arg(pid.trim())
            .stderr(Stdio::null())
            .status()
            .expect("kill -0 runs");
        assert!(
            !alive.success(),
            "the child survived the failed run: {}",
            err
        );
        assert!(
            err.to_string().contains("raw output:"),
            "the error does not name the raw log: {err}"
        );
    }

    #[test]
    fn a_timeout_kills_the_child_and_names_the_log() {
        let pid_file =
            std::env::temp_dir().join(format!("saniwalens-timeout-pid-{}", std::process::id()));
        let script = format!("echo $$ > '{}'; exec sleep 5", pid_file.display());
        let started = Instant::now();
        let err = run_headless(
            "/bin/sh",
            &["-c", script.as_str()],
            "ignored",
            None,
            fast_beat(),
            Some(Duration::from_millis(100)),
            false,
        )
        .expect_err("the silent child times out");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the timeout did not return promptly: {:?}",
            started.elapsed()
        );
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("timeout"),
            "the failure does not name the timeout: {rendered}"
        );
        // Best-effort cleanup: the branch predates the shared log-path helper.
        if let Some(log) = err.chain().find_map(|cause| {
            cause
                .to_string()
                .strip_prefix("raw output: ")
                .map(PathBuf::from)
        }) {
            let _ = std::fs::remove_file(log);
        }
        let pid = std::fs::read_to_string(&pid_file).expect("the child wrote its pid");
        let _ = std::fs::remove_file(&pid_file);
        let alive = Command::new("kill")
            .arg("-0")
            .arg(pid.trim())
            .stderr(Stdio::null())
            .status()
            .expect("kill -0 runs");
        assert!(!alive.success(), "the child survived the timeout");
    }

    #[test]
    fn a_spawn_failure_leaves_no_child_and_no_beat() {
        let err = run_headless(
            "/nonexistent-saniwalens-headless-binary",
            &[],
            "ignored",
            None,
            fast_beat(),
            None,
            false,
        )
        .expect_err("a missing binary fails the spawn");
        let log = raw_log_path(&err).expect("the failure names the raw log");
        let _ = std::fs::remove_file(log);
        assert!(
            err.to_string().contains("raw output:"),
            "the spawn error does not name the raw log: {err}"
        );
    }

    #[test]
    fn the_headless_success_removes_the_run_log() {
        let (status, log_path) = run_headless(
            "/bin/sh",
            &["-c", "printf 'hello\\n'"],
            "ignored",
            None,
            fast_beat(),
            None,
            false,
        )
        .expect("the child succeeds");
        assert!(status.success(), "the child exit status: {status}");
        assert!(
            !log_path.exists(),
            "the run log survived its successful run: {log_path:?}"
        );
    }

    #[test]
    fn the_headless_keep_opt_in_preserves_the_run_log_on_success() {
        let (status, log_path) = run_headless(
            "/bin/sh",
            &["-c", "printf 'hello\\n'"],
            "ignored",
            None,
            fast_beat(),
            None,
            true,
        )
        .expect("the child succeeds");
        assert!(status.success(), "the child exit status: {status}");
        let log = std::fs::read_to_string(&log_path).expect("the kept log is readable");
        assert_eq!(log, "hello\n", "the raw line reached the kept log");
        let _ = std::fs::remove_file(&log_path);
    }

    #[test]
    fn the_headless_failure_keeps_the_run_log_for_the_bail() {
        let (status, log_path) = run_headless(
            "/bin/sh",
            &["-c", "printf 'boom\\n'; exit 3"],
            "ignored",
            None,
            fast_beat(),
            None,
            false,
        )
        .expect("a non-zero exit is still a completed run");
        assert!(!status.success(), "the child exit status: {status}");
        let log = std::fs::read_to_string(&log_path).expect("the failed run keeps its log");
        assert_eq!(log, "boom\n", "the raw line reached the kept log");
        let _ = std::fs::remove_file(&log_path);
    }

    #[test]
    fn dropping_the_guard_stops_the_beat() {
        let run = HeadlessRun::start(fast_beat()).expect("the log and beat start");
        let log_path = run.log_path().to_path_buf();
        let started = Instant::now();
        drop(run);
        let _ = std::fs::remove_file(log_path);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the heartbeat did not stop within a poll: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn timeout_accepts_seconds_minutes_and_milliseconds() {
        assert_eq!(
            parse_timeout(Some("30s")).unwrap(),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            parse_timeout(Some("2m")).unwrap(),
            Some(Duration::from_secs(120))
        );
        assert_eq!(
            parse_timeout(Some("500ms")).unwrap(),
            Some(Duration::from_millis(500))
        );
        assert_eq!(
            parse_timeout(Some("7")).unwrap(),
            Some(Duration::from_secs(7))
        );
        assert_eq!(parse_timeout(None).unwrap(), None);
    }

    #[test]
    fn timeout_rejects_negatives_and_garbage() {
        assert!(parse_timeout(Some("-1s")).is_err());
        assert!(parse_timeout(Some("soon")).is_err());
    }

    #[test]
    fn timeout_zero_disables_the_deadline() {
        // The explicit infinite opt-out: zero in any unit parses to no
        // deadline, while the choke point below warns about it.
        for value in ["0", "0s", "0m", "0ms", "0.0s"] {
            assert_eq!(
                parse_timeout(Some(value)).unwrap(),
                None,
                "zero disables the deadline: {value}"
            );
        }
    }

    #[test]
    fn resolve_timeout_defaults_when_the_flag_is_absent() {
        assert_eq!(
            resolve_timeout(None).unwrap(),
            Some(DEFAULT_BRIEFING_TIMEOUT)
        );
    }

    #[test]
    fn resolve_timeout_prefers_an_explicit_value() {
        assert_eq!(
            resolve_timeout(Some("30s")).unwrap(),
            Some(Duration::from_secs(30))
        );
    }

    #[test]
    fn resolve_timeout_zero_opts_into_no_deadline() {
        assert_eq!(resolve_timeout(Some("0")).unwrap(), None);
        assert_eq!(resolve_timeout(Some("0m")).unwrap(), None);
    }

    #[test]
    fn timeout_rejects_gigantic_but_finite_values_without_panicking() {
        // `Duration::from_secs_f64` panics past its range; a huge finite
        // input must be a regular error, never a crash.
        for value in ["1e308s", "1e308", "1e308m", "1e308ms", "1e20m"] {
            let err = parse_timeout(Some(value)).expect_err(&format!("{value} must err"));
            assert!(
                err.to_string().contains("too large"),
                "the error names the overflow: {err}"
            );
        }
        // Just inside the range still parses.
        assert!(parse_timeout(Some("1e18s")).is_ok());
    }

    // Moved from `super` (the `mod.rs` shell carries a line budget —
    // `tests/cli_mod_line_budget.rs`): the failing-advisor delivery test lives
    // with the headless code it exercises.
    use super::super::{AdvisorLaunch, DeliveryMode, DeliveryPlan, deliver};
    use crate::adapter::harness::Harness;

    #[cfg(unix)]
    fn success_status() -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0)
    }

    #[cfg(unix)]
    struct FailingHeadlessAdvisor {
        log: PathBuf,
    }

    #[cfg(unix)]
    fn failure_status() -> std::process::ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        std::process::ExitStatus::from_raw(0x100)
    }

    #[cfg(unix)]
    impl AdvisorLaunch for FailingHeadlessAdvisor {
        fn direct(
            &mut self,
            _binary: &str,
            _leading: &[&str],
            _prompt: &str,
        ) -> Result<std::process::ExitStatus> {
            Ok(success_status())
        }

        fn headless(
            &mut self,
            _binary: &str,
            _leading: &[&str],
            _prompt: &str,
            _render: Option<RunLineRenderer>,
            _timeout: Option<std::time::Duration>,
        ) -> Result<(std::process::ExitStatus, PathBuf)> {
            Ok((failure_status(), self.log.clone()))
        }
    }

    #[cfg(unix)]
    #[test]
    fn headless_failure_keeps_the_run_log_and_names_it_in_the_error() {
        let log = private_temp_path("saniwalens-headless-test", "log");
        write_private_file(&log, "boom\n").expect("the fake run log is written");
        let plan = DeliveryPlan::launch(
            ("opencode", &["run", "--format", "json"]),
            DeliveryMode::Headless,
            "briefing".to_string(),
            "opencode",
            false,
            Harness::Opencode.run_line_renderer(),
            None,
            |_| "pointer".to_string(),
        );
        let mut advisor = FailingHeadlessAdvisor { log: log.clone() };

        let err = deliver(plan, &mut advisor).expect_err("a non-zero exit fails delivery");

        assert!(
            err.to_string().contains(&log.display().to_string()),
            "the error does not name the run log: {err:#}"
        );
        assert!(log.exists(), "the failed run's log was removed: {log:?}");
        let _ = fs::remove_file(&log);
    }
}
