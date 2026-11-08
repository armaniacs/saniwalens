//! Parity + RAII pins for the briefing delivery (PBI C10).
//!
//! The parity half pins today's successful delivery byte for byte — the argv
//! the advisor sees, the briefing bytes behind the pointer file, the timeout
//! passed through untouched, and the exit-code propagation — so the RAII
//! migration cannot change the normal path. The RAII half pins the leak the
//! migration closes: a panicking advisor must still remove the secret
//! briefing file. No process spawns here; the doubles stand in for the
//! advisor and the fixtures are synthetic.

#![cfg(unix)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use saniwalens::cli::{AdvisorLaunch, DeliveryMode, DeliveryPlan, deliver};

fn success_status() -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(0)
}

fn failure_status() -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(0x100)
}

/// The delivery double: records what would have been launched instead of
/// spawning anything, so the briefing's argv can be asserted.
struct RecordingAdvisor {
    direct: Vec<(String, Vec<String>, String)>,
    headless: Vec<(String, Vec<String>, String, Option<Duration>)>,
    fail_with: Option<std::process::ExitStatus>,
}

impl RecordingAdvisor {
    fn succeeding() -> Self {
        Self {
            direct: Vec::new(),
            headless: Vec::new(),
            fail_with: None,
        }
    }

    fn failing(status: std::process::ExitStatus) -> Self {
        Self {
            direct: Vec::new(),
            headless: Vec::new(),
            fail_with: Some(status),
        }
    }

    fn status(&self) -> std::process::ExitStatus {
        self.fail_with.unwrap_or_else(success_status)
    }
}

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
        Ok(self.status())
    }

    fn headless(
        &mut self,
        binary: &str,
        leading: &[&str],
        prompt: &str,
        _render: Option<saniwalens::adapter::harness::RunLineRenderer>,
        timeout: Option<Duration>,
    ) -> Result<(std::process::ExitStatus, PathBuf)> {
        self.headless.push((
            binary.to_string(),
            leading.iter().map(|s| s.to_string()).collect(),
            prompt.to_string(),
            timeout,
        ));
        Ok((
            self.status(),
            PathBuf::from("/tmp/saniwalens-raii-test-never.log"),
        ))
    }
}

/// The briefing bytes behind the pointer file, captured while the pointer
/// closure runs (the file is gone by the time `deliver` returns).
struct SeenFile {
    path: Arc<Mutex<Option<PathBuf>>>,
    bytes: Arc<Mutex<Option<String>>>,
}

impl SeenFile {
    fn new() -> Self {
        Self {
            path: Arc::new(Mutex::new(None)),
            bytes: Arc::new(Mutex::new(None)),
        }
    }

    fn pointer(&self) -> impl FnOnce(&std::path::Path) -> String + '_ {
        let path = Arc::clone(&self.path);
        let bytes = Arc::clone(&self.bytes);
        move |p: &std::path::Path| {
            *path.lock().unwrap() = Some(p.to_path_buf());
            *bytes.lock().unwrap() = Some(std::fs::read_to_string(p).unwrap());
            "POINTER PROMPT".to_string()
        }
    }

    fn path(&self) -> PathBuf {
        self.path
            .lock()
            .unwrap()
            .clone()
            .expect("the pointer saw the file")
    }

    fn bytes(&self) -> String {
        self.bytes
            .lock()
            .unwrap()
            .clone()
            .expect("the pointer read the file")
    }
}

#[test]
fn the_interactive_delivery_argv_and_briefing_bytes_are_pinned() {
    let seen = SeenFile::new();
    let plan = DeliveryPlan::launch(
        ("claude", &[]),
        DeliveryMode::Interactive,
        "the rendered briefing".to_string(),
        "Claude Code",
        false,
        None,
        None,
        seen.pointer(),
    );
    let mut advisor = RecordingAdvisor::succeeding();

    deliver(plan, &mut advisor).unwrap();

    assert_eq!(advisor.direct.len(), 1);
    assert_eq!(advisor.direct[0].0, "claude");
    assert!(advisor.direct[0].1.is_empty());
    assert_eq!(advisor.direct[0].2, "POINTER PROMPT");
    assert_eq!(seen.bytes(), "the rendered briefing");
    assert!(!seen.path().exists(), "the briefing file outlived its run");
}

#[test]
fn the_quiet_headless_delivery_passes_argv_and_timeout_through() {
    let seen = SeenFile::new();
    let plan = DeliveryPlan::launch(
        ("opencode", &["run", "--format", "json"]),
        DeliveryMode::Headless,
        "the rendered briefing".to_string(),
        "opencode",
        false,
        None,
        Some(Duration::from_secs(60)),
        seen.pointer(),
    );
    let mut advisor = RecordingAdvisor::succeeding();

    deliver(plan, &mut advisor).unwrap();

    assert_eq!(advisor.headless.len(), 1);
    assert_eq!(advisor.headless[0].0, "opencode");
    assert_eq!(advisor.headless[0].1, vec!["run", "--format", "json"]);
    assert_eq!(advisor.headless[0].2, "POINTER PROMPT");
    assert_eq!(advisor.headless[0].3, Some(Duration::from_secs(60)));
    assert!(advisor.direct.is_empty());
    assert!(!seen.path().exists(), "the briefing file outlived its run");
}

#[test]
fn a_failing_advisor_propagates_its_exit_status() {
    let plan = DeliveryPlan::launch(
        ("claude", &[]),
        DeliveryMode::Interactive,
        "briefing".to_string(),
        "Claude Code",
        false,
        None,
        None,
        |_| "pointer".to_string(),
    );
    let mut advisor = RecordingAdvisor::failing(failure_status());

    let err = deliver(plan, &mut advisor).expect_err("a non-zero exit fails delivery");

    assert!(
        err.to_string().contains("exited with"),
        "the error names the exit status: {err:#}"
    );
}

/// An advisor that panics instead of returning, standing in for a crash
/// between the briefing write and the normal-path removal.
struct PanickingAdvisor;

impl AdvisorLaunch for PanickingAdvisor {
    fn direct(
        &mut self,
        _binary: &str,
        _leading: &[&str],
        _prompt: &str,
    ) -> Result<std::process::ExitStatus> {
        panic!("the advisor crashed mid-run");
    }

    fn headless(
        &mut self,
        _binary: &str,
        _leading: &[&str],
        _prompt: &str,
        _render: Option<saniwalens::adapter::harness::RunLineRenderer>,
        _timeout: Option<Duration>,
    ) -> Result<(std::process::ExitStatus, PathBuf)> {
        panic!("the advisor crashed mid-run");
    }
}

#[test]
fn a_panicking_advisor_still_removes_the_briefing_file() {
    let seen_path = Arc::new(Mutex::new(None::<PathBuf>));
    let plan = DeliveryPlan::launch(
        ("claude", &[]),
        DeliveryMode::Interactive,
        "secret briefing".to_string(),
        "Claude Code",
        false,
        None,
        None,
        |p: &std::path::Path| {
            *seen_path.lock().unwrap() = Some(p.to_path_buf());
            "pointer".to_string()
        },
    );
    let mut advisor = PanickingAdvisor;

    let outcome =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| deliver(plan, &mut advisor)));
    assert!(outcome.is_err(), "the advisor panic propagates");

    let path = seen_path
        .lock()
        .unwrap()
        .clone()
        .expect("the pointer saw the file");
    // Best-effort tidy: a failure here must not litter the runner's temp dir.
    let leaked = path.exists();
    let _ = std::fs::remove_file(&path);
    assert!(!leaked, "the briefing file survived the advisor panic");
}

/// An advisor that fails with an error (not a status), standing in for a
/// spawn failure between the briefing write and the normal-path removal.
struct ErroringAdvisor;

impl AdvisorLaunch for ErroringAdvisor {
    fn direct(
        &mut self,
        _binary: &str,
        _leading: &[&str],
        _prompt: &str,
    ) -> Result<std::process::ExitStatus> {
        anyhow::bail!("spawn failed");
    }

    fn headless(
        &mut self,
        _binary: &str,
        _leading: &[&str],
        _prompt: &str,
        _render: Option<saniwalens::adapter::harness::RunLineRenderer>,
        _timeout: Option<Duration>,
    ) -> Result<(std::process::ExitStatus, PathBuf)> {
        anyhow::bail!("spawn failed");
    }
}

#[test]
fn an_erroring_advisor_still_removes_the_briefing_file() {
    let seen = SeenFile::new();
    let plan = DeliveryPlan::launch(
        ("claude", &[]),
        DeliveryMode::Interactive,
        "secret briefing".to_string(),
        "Claude Code",
        false,
        None,
        None,
        seen.pointer(),
    );
    let mut advisor = ErroringAdvisor;

    deliver(plan, &mut advisor).expect_err("the spawn failure fails delivery");
    assert!(
        !seen.path().exists(),
        "the briefing file survived the error"
    );
}
