//! The headless run's seam, end to end through the delivery: the renderer the
//! harness chose reaches the advisor, and the heartbeat's quiet-stretch logic
//! is observed through a sample sequence. A beat samples the run state when it
//! fires; the number of beats and their timing are outside the test's control,
//! so the test waits for the samples it needs instead of assuming an order.
//! No process spawns here; the fixtures are synthetic and the streams they
//! stand for never touch real data.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use anyhow::Result;
use saniwalens::adapter::harness::{Harness, RunLineRenderer};
use saniwalens::adapter::opencode::RunEvent;
use saniwalens::cli::heartbeat;
use saniwalens::cli::{AdvisorLaunch, DeliveryMode, DeliveryPlan, deliver};

/// A synthetic `--format json` line: the tool event a headless run streams.
/// The real envelope shape (an event naming the kind, wrapping the part),
/// never a harvested line.
const TOOL_LINE: &str = r#"{"type":"tool_use","timestamp":1792904532992,"sessionID":"ses_demo","part":{"partID":"prt_demo","type":"tool","tool":"read","state":{"status":"completed"}}}"#;

#[cfg(unix)]
fn success_status() -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(0)
}

/// The delivery double: records the headless hand-off — the argv and the
/// renderer the plan carried — instead of spawning.
#[cfg(unix)]
struct HeadlessRecorder {
    headless: Option<(String, Vec<String>, String, Option<RunLineRenderer>)>,
    direct: usize,
}

#[cfg(unix)]
impl AdvisorLaunch for HeadlessRecorder {
    fn direct(
        &mut self,
        _binary: &str,
        _leading: &[&str],
        _prompt: &str,
    ) -> Result<std::process::ExitStatus> {
        self.direct += 1;
        Ok(success_status())
    }

    fn headless(
        &mut self,
        binary: &str,
        leading: &[&str],
        prompt: &str,
        render: Option<RunLineRenderer>,
        _timeout: Option<Duration>,
    ) -> Result<(std::process::ExitStatus, PathBuf)> {
        self.headless = Some((
            binary.to_string(),
            leading.iter().map(|s| s.to_string()).collect(),
            prompt.to_string(),
            render,
        ));
        Ok((
            success_status(),
            PathBuf::from("/tmp/saniwalens-seam-test-never.log"),
        ))
    }
}

#[cfg(unix)]
#[test]
fn the_quiet_headless_delivery_hands_the_advisor_the_seam_renderer() {
    let plan = DeliveryPlan::launch(
        ("opencode", &["run", "--format", "json"]),
        DeliveryMode::Headless,
        "the rendered briefing".to_string(),
        "opencode",
        false,
        Harness::Opencode.run_line_renderer(),
        None,
        |_| "pointer".to_string(),
    );
    let mut advisor = HeadlessRecorder {
        headless: None,
        direct: 0,
    };

    deliver(plan, &mut advisor).unwrap();

    assert_eq!(advisor.direct, 0, "quiet headless logs its stream");
    let (_, leading, _, render) = advisor
        .headless
        .expect("the advisor received the headless run");
    assert_eq!(leading, vec!["run", "--format", "json"]);
    // The renderer is the seam's choice, still working when it arrives: the
    // adapter's opencode stream renderer, not a shell-local copy.
    let render = render.expect("a headless harness ships a renderer");
    assert_eq!(render(TOOL_LINE), RunEvent::Progress("read".to_string()));
}

/// The budget for one phase of the observation, far above one poll (5ms), so a
/// descheduled test thread still reaches the sample it is waiting for. It is a
/// total budget, not a per-receive timeout: a loop that keeps receiving the
/// wrong sample must still fail instead of spinning.
const OBSERVE: Duration = Duration::from_secs(2);

/// Stop the heartbeat thread on the way out, panic or not. Without it a failed
/// assertion leaves the thread beating to stderr for the rest of the binary.
/// The guard owns the handle: the test's normal path and a panic path both go
/// through this Drop, so the explicit join cannot be skipped.
struct StopOnDrop {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl StopOnDrop {
    fn new(stop: Arc<AtomicBool>, handle: std::thread::JoinHandle<()>) -> Self {
        Self {
            stop,
            handle: Some(handle),
        }
    }

    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        use std::io::Write;

        self.stop.store(true, Ordering::Relaxed);
        let Some(handle) = self.handle.take() else {
            return;
        };
        let deadline = Instant::now() + OBSERVE;
        while !handle.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        if handle.is_finished() {
            let _ = handle.join();
        } else {
            // A stuck thread cannot be killed; detaching is the only
            // non-panicking choice. One line, no panic in Drop.
            let _ = writeln!(
                std::io::stderr().lock(),
                "heartbeat did not stop within {OBSERVE:?}; detaching"
            );
        }
    }
}

#[test]
fn the_heartbeat_samples_events_and_stops_cleanly() {
    // Synthetic run state — no process, no log: the quiet-stretch logic alone.
    let stop = Arc::new(AtomicBool::new(false));
    let events = Arc::new(AtomicUsize::new(0));
    // Shrunk to keep the test quick and degenerate on purpose: cadence (1ms)
    // is below poll (5ms), so the beat follows the poll. The due decision and
    // the dropped-slot contract are pinned by the pure decision function and
    // its beat model in `cli::heartbeat` tests; this test only observes the
    // samples it needs.
    let (handle, beats) = heartbeat::spawn(
        Arc::clone(&stop),
        Arc::clone(&events),
        heartbeat::Beat::new(Duration::from_millis(1), Duration::from_millis(5))
            .expect("the test beat is non-zero"),
    );
    let stop_on_drop = StopOnDrop::new(Arc::clone(&stop), handle);

    // The first beat samples the run before any event has streamed, so it
    // carries 0. Waiting for it here makes the order deterministic: the add
    // below can only happen after a beat has already read the empty count.
    let (_, first) = beats
        .recv_timeout(OBSERVE)
        .expect("a beat while the run works");
    assert_eq!(first, 0, "the first beat samples the run before any event");

    // Only now add events. A beat carrying the new count proves the loop
    // re-reads the counter at every beat instead of caching it at thread start.
    events.fetch_add(3, Ordering::Relaxed);
    let deadline = Instant::now() + OBSERVE;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            panic!("no beat carried the new count within {OBSERVE:?}");
        }
        match beats.recv_timeout(remaining) {
            Ok((_, count)) => {
                if count == 3 {
                    break;
                }
            }
            Err(e) => panic!("no beat carried the new count: {e:?}"),
        }
    }

    // Flipping `stop` ends the thread: its sender drops and the receiver
    // disconnects. A beat produced before the stop may still arrive, and at
    // most one beat is sent between the flag being set and the second stop
    // check observing it — a structural invariant of the two checks, not a
    // quantity this test can count.
    // First prove the loop is still alive after the target count: a beat that
    // arrives now cannot be the count-3 beat that was already consumed. Without
    // this, an early self-destruction right after the target beat would look
    // exactly like a stop-honoring exit.
    let (_, after) = beats
        .recv_timeout(OBSERVE)
        .expect("a beat after the target count");
    assert_eq!(after, 3, "the loop is still sampling after the target beat");
    stop_on_drop.stop();
    let deadline = Instant::now() + OBSERVE;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            panic!("the heartbeat kept running past the stop flag");
        }
        match beats.recv_timeout(remaining) {
            Err(RecvTimeoutError::Disconnected) => break,
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {
                panic!("the heartbeat kept running past the stop flag")
            }
        }
    }
    // The guard owns the handle: its Drop stops the beat and joins within the
    // observation budget. No second explicit join here.
    drop(stop_on_drop);
}
