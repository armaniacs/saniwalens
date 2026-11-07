//! Scripted end-to-end runs through the real binary's guided TUI: prompt
//! answers in, report render plus the echoed command out, the config file
//! written, and the loop exited, all with no terminal anywhere. Piped stdio is
//! never a TTY, so `SANIWALENS_TEST_FORCE_TTY=1` (test-only, unsupported) is
//! what lets the guide start here; without it every run below refuses.
//!
//! NOTE: the opencode-preset scenario from the plan is intentionally absent:
//! the opencode adapter refuses an empty synthetic home (no opencode
//! database there, exit 1), so a scripted preset run cannot reach exit 0
//! without seeding a harness database in the test. Reported as NEEDS_CONTEXT;
//! not worked around here.

mod common;

use std::io::Write;
use std::process::{Command, Stdio};

use common::ScratchDir;

/// Run the binary with a scripted stdin transcript on a synthetic home and
/// return the scratch dir (kept alive by the caller) plus the raw output.
/// The stdin handle is dropped right after the script is written, so the
/// guide's next read sees EOF exactly as a closed pipe would.
fn run_guided(name: &str, args: &[&str], script: &str) -> (ScratchDir, std::process::Output) {
    // Synthetic only: an empty `.claude/projects` tree for HOME isolation,
    // the same shape `tests/guide_entry.rs` uses. `XDG_CONFIG_HOME` is set so
    // the save flow writes under the scratch dir, never the real home.
    let dir = ScratchDir::new("saniwalens-guide-flow", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_saniwalens"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOME", &*dir)
        .env("CLAUDE_CONFIG_DIR", dir.join(".claude"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CONFIG_HOME", dir.join("cfg"))
        .env("SANIWALENS_TEST_FORCE_TTY", "1")
        .spawn()
        .expect("the binary runs");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(script.as_bytes())
        .expect("the script is written");
    let out = child.wait_with_output().expect("the binary exits");
    (dir, out)
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn defaults_flow_renders_report_echoes_command_and_writes_nothing() {
    // Purpose / scope / save / continue all take the default: doctor over
    // all history, no save, no second report.
    let (dir, out) = run_guided("defaults", &[], "\n\n\n\n");
    assert!(
        out.status.success(),
        "defaults flow must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let shown = stdout(&out);
    assert!(
        shown.contains("実行: saniwalens doctor"),
        "the command is echoed before the run: {shown}"
    );
    assert!(
        shown.contains("実行したコマンド: saniwalens doctor"),
        "the command is reprinted after the run: {shown}"
    );
    assert!(
        shown.contains("次の一手: saniwalens optimize"),
        "the next step points at optimize: {shown}"
    );
    assert!(
        !dir.join("cfg")
            .join("saniwalens")
            .join("config.toml")
            .exists(),
        "skipping the save must not create a config file"
    );
}

#[test]
fn default_save_writes_exactly_the_default_harness_line() {
    // Purpose / scope default, save as default_harness, confirm, EOF ends it.
    let (dir, out) = run_guided("default-save", &[], "\n\n2\ny\n");
    assert!(
        out.status.success(),
        "default save must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let shown = stdout(&out);
    assert!(
        shown.contains("書き込む内容:"),
        "the guide shows the text before confirming: {shown}"
    );
    let path = dir.join("cfg").join("saniwalens").join("config.toml");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the save writes the config"),
        "# saniwalens defaults (written by the guided TUI).\ndefault_harness = \"claude\"\n",
        "byte-exact new file, resolved default harness"
    );
}

#[test]
fn profile_save_is_idempotent_across_two_runs() {
    // One scratch dir, two identical runs: the second must update the
    // `[profiles.work]` section in place, never duplicate it.
    let script = "\n\n3\nwork\ny\n";
    let (dir, first) = run_guided("profile-save", &[], script);
    assert!(
        first.status.success(),
        "first run must exit 0: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let path = dir.join("cfg").join("saniwalens").join("config.toml");
    let after_first = std::fs::read_to_string(&path).expect("first run writes the config");
    assert!(
        after_first.contains("[profiles.work]\nharness = \"claude\""),
        "first run stores the profile: {after_first}"
    );
    // Second run reuses the same dir (one guard owns it for the whole test).
    let mut child = Command::new(env!("CARGO_BIN_EXE_saniwalens"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOME", &*dir)
        .env("CLAUDE_CONFIG_DIR", dir.join(".claude"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CONFIG_HOME", dir.join("cfg"))
        .env("SANIWALENS_TEST_FORCE_TTY", "1")
        .spawn()
        .expect("the binary runs");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(script.as_bytes())
        .expect("the script is written");
    let second = child.wait_with_output().expect("the binary exits");
    assert!(
        second.status.success(),
        "second run must exit 0: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let after_second = std::fs::read_to_string(&path).expect("second run keeps the config");
    assert_eq!(
        after_second.matches("[profiles.work]").count(),
        1,
        "no duplicated section: {after_second}"
    );
    assert_eq!(
        after_second.matches("harness = \"claude\"").count(),
        1,
        "no duplicated harness line: {after_second}"
    );
}

#[test]
fn quit_first_runs_no_report_and_touches_no_store() {
    // `q` at the purpose prompt quits before any report or save: the guide
    // never opens the store, so no database file may appear.
    let (dir, out) = run_guided("quit-first", &[], "q\n");
    assert!(
        out.status.success(),
        "quitting must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let shown = stdout(&out);
    assert!(
        shown.contains("やりたいこと"),
        "the purpose prompt is shown: {shown}"
    );
    assert!(
        !shown.contains("実行:"),
        "no report runs, so no command is echoed: {shown}"
    );
    assert!(
        !dir.join("state")
            .join("saniwalens")
            .join("saniwalens.db")
            .exists(),
        "analyze never ran, so no store file may exist"
    );
}
