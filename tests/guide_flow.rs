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
//!
//! NOTE: no E2E drives the inline next-step execution (`y` at the
//! next-step question): answering yes launches a real advisor subprocess --
//! the plain inline optimize on an interactive harness, the `--apply`
//! handoff (plan save plus the harness's own dialog) on a headless one --
//! which a scripted test must never spawn. The headless `y` display line
//! (`saniwalens optimize --apply`, scope-matched) is therefore pinned by the
//! `next_step_line` unit tests, not by a run below. The branch adds no new
//! question,
//! so every script below keeps its input positions: the execute answer is the
//! 4th slot (9th on a looped second iteration), always No (empty line) here,
//! and only the question's appearance is asserted.

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
    // Harness / purpose / scope / execute? / save / continue all take the
    // default: doctor over all history, no inline optimize, no save, no
    // second report.
    let (dir, out) = run_guided("defaults", &[], "\n\n\n\n\n\n");
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
        shown.contains("次の一手を実行"),
        "the guide offers to run the next step inline: {shown}"
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
    // Harness / purpose / scope default, No to the inline optimize, save as
    // default_harness, confirm, EOF ends it.
    let (dir, out) = run_guided("default-save", &[], "\n\n\n\n2\ny\n");
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
    // Harness / purpose / scope default, No to the inline optimize, then
    // the profile save answers.
    let script = "\n\n\n\n3\nwork\ny\n";
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
fn quit_at_harness_runs_no_report_and_touches_no_store() {
    // `q` at the harness prompt quits before any report or save: the guide
    // never opens the store, so no database file may appear. The trailing
    // newline is never read (no report runs, so the execute question never
    // appears); it keeps every scripted input carrying the new answer slot.
    let (dir, out) = run_guided("quit-harness", &[], "q\n\n");
    assert!(
        out.status.success(),
        "quitting must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let shown = stdout(&out);
    assert!(
        shown.contains("使っているエージェントを選んで"),
        "the harness prompt is shown: {shown}"
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

#[test]
fn pick_opencode_echoes_the_harness_command() {
    // Choosing 2 (opencode) at the harness prompt flows the selected
    // harness into the echoed command. The report itself cannot run here:
    // the synthetic home holds no opencode database, so the binary exits
    // non-zero after the echo — the echo line is the assertion. The trailing
    // newline is never read (the report fails before the execute question).
    let (_dir, out) = run_guided("pick-opencode", &[], "2\n\n\n\n\n");
    let shown = stdout(&out);
    assert!(
        shown.contains("実行: saniwalens --harness opencode doctor"),
        "the selected harness names the command: {shown}"
    );
}

#[test]
fn continue_y_loops_back_with_the_same_harness_and_asks_it_once() {
    // Default harness, doctor over all history, No to the inline optimize,
    // skip save, `y` continues: the second iteration runs doctor again with
    // the same (default) harness, and the harness question appears exactly
    // once. One extra No per iteration for the execute question.
    let (_dir, out) = run_guided("loop-keeps-harness", &[], "\n\n\n\n\ny\n\n\n\n\nn\n");
    assert!(
        out.status.success(),
        "looped flow must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let shown = stdout(&out);
    assert_eq!(
        shown.matches("実行: saniwalens doctor").count(),
        2,
        "both iterations echo the kept harness command: {shown}"
    );
    assert_eq!(
        shown.matches("使っているエージェントを選んで").count(),
        1,
        "the harness is asked once, then kept: {shown}"
    );
}

#[test]
fn quit_first_runs_no_report_and_touches_no_store() {
    // Harness default, then `q` at the purpose prompt quits before any
    // report or save: the guide never opens the store, so no database file
    // may appear. The trailing newline is never read (no report runs).
    let (dir, out) = run_guided("quit-first", &[], "\nq\n\n");
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
