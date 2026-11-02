//! The headless advisor run: a logged pipe around the harness's launch, with
//! one guard owning the child and the heartbeat for the run's whole lifetime.
//!
//! `run_headless` used to keep the stop/join on the normal path's last lines,
//! so any of its five `?` returns left the heartbeat running and the child
//! unreaped (`Child`'s drop neither kills nor waits). The guard here makes the
//! lifetime a scope fact: every exit, including an error return, stops the beat
//! and reaps the child. The raw log path is carried into the error so a failed
//! run still names its full output (`docs/specs/harness.md`).

use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::adapter::harness::RunLineRenderer;
use crate::adapter::opencode::RunEvent;
use crate::session::Style;

use super::heartbeat;
use super::{private_temp_path, write_private_file};

/// The beat cadence and stop-poll interval a headless run uses. Defaults to
/// the production constants; tests shrink both so the beat is observable.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Beat {
    pub(crate) cadence: Duration,
    pub(crate) poll: Duration,
}

impl Default for Beat {
    fn default() -> Self {
        Self {
            cadence: heartbeat::CADENCE,
            poll: heartbeat::POLL,
        }
    }
}

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
        let (heartbeat, beats) = heartbeat::spawn(
            Arc::clone(&stop),
            Arc::clone(&events),
            beat.cadence,
            beat.poll,
        );

        Ok(Self {
            stop,
            events,
            _beats: beats,
            log_path,
            log,
            child_err: Some(child_err),
            child: None,
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
    fn wait_for_status(&mut self) -> Result<ExitStatus> {
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
        status
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
        if let Some(handle) = self.heartbeat.take() {
            let _ = handle.join();
        }
    }
}

/// Run the headless advisor with its raw output logged: spawn with stdout
/// piped, write every raw line (and the child's stderr) to a private log, and
/// surface only what the agent says — text parts on stdout (so a pipe like
/// `| pbcopy` receives the narration and the plan), one-line progress markers
/// on stderr. The `render_line` is the harness seam's choice
/// (`Harness::run_line_renderer`), so this loop names no harness. A heartbeat
/// covers quiet stretches (a long tool call, model thinking) so the run never
/// looks hung. Returns the log path so the caller can point at the full
/// detail. The raw event shapes are unversioned upstream, so rendering is
/// lenient: a line with no recognizable text stays in the log, and a run that
/// renders nothing readable says so instead of leaving silence.
pub(crate) fn run_headless(
    binary: &str,
    leading: &[&str],
    prompt: &str,
    render_line: Option<RunLineRenderer>,
    beat: Beat,
) -> Result<(ExitStatus, PathBuf)> {
    let mut run = HeadlessRun::start(beat)?;
    let log_path = run.log_path().to_path_buf();
    let (status, readable) = match drive(&mut run, binary, leading, prompt, render_line) {
        Ok(driven) => driven,
        Err(err) => {
            return Err(err).with_context(|| format!("raw output: {}", log_path.display()));
        }
    };
    drop(run);
    if status.success() && !readable {
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

    let mut readable = false;
    for line in std::io::BufReader::new(stdout).lines() {
        let line = line.context("read the advisor's output")?;
        run.count_event();
        writeln!(run.log(), "{line}").context("write the run log")?;
        match render_line.map_or(RunEvent::Silent, |render| render(&line)) {
            RunEvent::Text(text) => {
                readable = true;
                println!("{text}");
            }
            RunEvent::Progress(tool) => {
                eprintln!("{}", Style::stderr().dim(&format!("  → {tool}")));
            }
            RunEvent::Silent => {}
        }
    }
    let status = run.wait_for_status()?;
    Ok((status, readable))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Instant;

    fn fast_beat() -> Beat {
        Beat {
            cadence: Duration::from_millis(1),
            poll: Duration::from_millis(5),
        }
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
        )
        .expect_err("the invalid UTF-8 line fails the read");

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
    fn a_spawn_failure_leaves_no_child_and_no_beat() {
        let err = run_headless(
            "/nonexistent-saniwalens-headless-binary",
            &[],
            "ignored",
            None,
            fast_beat(),
        )
        .expect_err("a missing binary fails the spawn");
        assert!(
            err.to_string().contains("raw output:"),
            "the spawn error does not name the raw log: {err}"
        );
    }

    #[test]
    fn the_success_path_returns_the_status_and_the_log() {
        let (status, log_path) = run_headless(
            "/bin/sh",
            &["-c", "printf 'hello\\n'"],
            "ignored",
            None,
            fast_beat(),
        )
        .expect("the child succeeds");
        assert!(status.success(), "the child exit status: {status}");
        let log = std::fs::read_to_string(&log_path).expect("the log is readable");
        assert_eq!(log, "hello\n", "the raw line reached the log");
        let _ = std::fs::remove_file(&log_path);
    }

    #[test]
    fn dropping_the_guard_stops_the_beat() {
        let run = HeadlessRun::start(fast_beat()).expect("the log and beat start");
        let started = Instant::now();
        drop(run);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "the heartbeat did not stop within a poll: {:?}",
            started.elapsed()
        );
    }
}
