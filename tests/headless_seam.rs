//! The headless run's seam, end to end through the delivery: the renderer the
//! harness chose reaches the advisor, and the heartbeat's quiet-stretch logic
//! — the beat cadence and the stop handling — is pinned in one place. No
//! process spawns here; the fixtures are synthetic and the streams they stand
//! for never touch real data.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

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

#[test]
fn the_heartbeat_beats_on_the_cadence_and_stops() {
    // Synthetic run state — no process, no log: the quiet-stretch logic alone.
    let stop = Arc::new(AtomicBool::new(false));
    let events = Arc::new(AtomicUsize::new(0));
    let (handle, beats) = heartbeat::spawn(
        Arc::clone(&stop),
        Arc::clone(&events),
        Duration::from_millis(1),
        Duration::from_millis(5),
    );

    // A beat fires on the cadence and carries the streamed-event count, so a
    // quiet stretch reads as waiting, not a hang.
    events.fetch_add(3, Ordering::Relaxed);
    let (_, count) = beats
        .recv_timeout(Duration::from_secs(2))
        .expect("a beat while the run works");
    assert!(count >= 3, "the beat counts the streamed events");

    // Flipping `stop` ends the thread: its sender drops, and at most one beat
    // already in flight precedes the disconnect.
    stop.store(true, Ordering::Relaxed);
    let mut stragglers = 0;
    loop {
        match beats.recv_timeout(Duration::from_secs(2)) {
            Err(RecvTimeoutError::Disconnected) => break,
            Ok(_) => stragglers += 1,
            Err(RecvTimeoutError::Timeout) => {
                panic!("the heartbeat kept running past the stop flag")
            }
        }
        assert!(stragglers <= 1, "the heartbeat kept beating past the stop");
    }
    handle.join().unwrap();
}
