//! The `--apply` handoff, outside-in (`pbi/2026-11-09-01-feat-optimize-apply-handoff.md`).
//!
//! A normal interactive launch (spawning a real TUI) is out of scope for E2E
//! here: a test runner has no TTY to open one on, so the happy-path launch is
//! covered by the `direct` doubles in `cli::handoff` unit tests instead. This
//! file pins the refusal paths through the real binary and the
//! extract-save-prompt chain through the public seam. Every fixture is
//! synthetic; no harness binary is ever spawned.

use std::path::{Path, PathBuf};
use std::process::Command;

use saniwalens::adapter::harness::{Harness, RunLine, extract_plan_text};
use saniwalens::core::optimize as optimize_mod;
use saniwalens::core::scope::ScopeFilter;

mod common;
use common::ScratchDir;

/// A synthetic `--format json` tool-event line: the noise a headless run
/// streams around the agent's text. The real envelope shape, never a
/// harvested line.
const TOOL_LINE: &str = r#"{"type":"tool_use","timestamp":1792904532992,"sessionID":"ses_demo","part":{"partID":"prt_demo","type":"tool","tool":"read","state":{"status":"completed"}}}"#;

/// A synthetic text-event line carrying plan prose.
const TEXT_LINE: &str = r#"{"type":"text","timestamp":1792904882135,"sessionID":"ses_demo","part":{"id":"prt_demo","type":"text","text":"1. Add a file map to AGENTS.md"}}"#;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_saniwalens"))
}

/// A synthetic home sandbox: `HOME` and `XDG_STATE_HOME` both point into
/// scratch space, so the binary under test never touches the real home or
/// the real state dir.
fn sandbox(name: &str) -> ScratchDir {
    ScratchDir::new("saniwalens-apply-e2e", name)
}

fn run_in(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .args(args)
        .env("HOME", home)
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .output()
        .expect("the binary runs")
}

#[test]
fn optimize_help_advertises_apply() {
    let out = Command::new(binary())
        .args(["optimize", "--help"])
        .output()
        .expect("the binary runs");
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(
        help.contains("--apply"),
        "the flag is not advertised:\n{help}"
    );
}

#[test]
fn apply_with_print_fails_before_any_spawn() {
    // Printing ends the run instead of handing its plan to a dialog: the
    // conflict fails fast, before any store work or spawn.
    let home = sandbox("print-conflict");
    let out = run_in(&home, &["optimize", "--apply", "--print"]);
    assert!(!out.status.success(), "the conflict was accepted");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("--apply"), "{stderr}");
    assert!(stderr.contains("--print"), "{stderr}");
}

#[test]
fn apply_under_an_interactive_harness_fails_without_spawning() {
    // claude's session is already interactive: there is no headless output
    // to hand off, so the run fails with the headless gate's message.
    let home = sandbox("interactive-refusal");
    let out = run_in(&home, &["--harness", "claude", "optimize", "--apply"]);
    assert!(!out.status.success(), "the refusal did not fire");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("headless"), "{stderr}");
}

#[test]
fn normal_print_leaves_no_plan_file_behind() {
    // BDD "通常実行では何も残さない": a normal `--print` run on a synthetic
    // store prints the briefing and creates nothing under `plans/`.
    let home = sandbox("normal-print");
    let db = home.join("s.db");
    // An empty store is enough: `--frozen` skips the refresh, `--print`
    // skips every spawn, and the briefing renders its empty sections away.
    let out = run_in(
        &home,
        &[
            "optimize",
            "--print",
            "--frozen",
            "--db",
            &db.display().to_string(),
        ],
    );
    assert!(
        out.status.success(),
        "normal print failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("# saniwalens analysis"), "{stdout}");
    assert!(
        !home.join("state").join("saniwalens").join("plans").exists(),
        "a normal run left a plans dir behind"
    );
}

#[test]
fn plan_extraction_drops_json_event_lines() {
    // The integration half of the extraction contract: raw headless lines in,
    // agent text out, through the harness seam's own renderer.
    let render = Harness::Opencode
        .run_line_renderer()
        .expect("a headless harness ships a renderer");
    let rendered: Vec<RunLine> = [TOOL_LINE, TEXT_LINE, "not json at all"]
        .iter()
        .map(|line| render(line))
        .collect();
    assert_eq!(
        extract_plan_text(&rendered),
        "1. Add a file map to AGENTS.md"
    );
}

#[test]
fn the_apply_prompt_chain_names_the_plan_and_the_store() {
    // The dialog's opening prompt through the public seam: the saved plan
    // path, the apply order, and the store pointer for `saniwalens sql`
    // digs — with no briefing data on argv.
    let copy = Harness::Opencode.copy();
    let prompt = optimize_mod::apply_prompt(
        "/tmp/example/state/saniwalens/plans/plan-x.md",
        "/tmp/example/oc.db",
        &ScopeFilter::All,
        &copy,
    );
    assert!(prompt.contains("opencode optimization advisor"), "{prompt}");
    assert!(
        prompt.contains("/tmp/example/state/saniwalens/plans/plan-x.md"),
        "{prompt}"
    );
    assert!(prompt.contains("Read that file in full"), "{prompt}");
    assert!(prompt.contains("apply"), "{prompt}");
    assert!(prompt.contains("saniwalens sql"), "{prompt}");
    assert!(prompt.contains("--db /tmp/example/oc.db"), "{prompt}");
    assert!(!prompt.contains("# saniwalens analysis"), "{prompt}");
}

#[test]
fn the_apply_prompt_chain_prescribes_the_safety_procedure() {
    // BDD "適用はバックアップ・diff・確認を経る" + "適用結果が分かる" +
    // "承認がなければ何も変わらない", through the public seam: backup
    // (git-tracked and untracked, private), planned diff before the
    // per-item choice, refusal semantics, and the pinned result-summary
    // sections. Updated for the stepwise walkthrough (design 2026-11-10,
    // PBI 2026-11-10-04): the former single approval gate is now per-item
    // approval, so this pins the item walkthrough, the y/n/q choices, and
    // the blanket-instruction refusal instead of "Ask once"/"single gate".
    // A live dialog launch stays out of scope here (see the file
    // header): the `direct` doubles in `cli::handoff` unit tests cover the
    // launch, and no harness binary is ever spawned.
    let copy = Harness::Opencode.copy();
    let prompt = optimize_mod::apply_prompt(
        "/tmp/example/state/saniwalens/plans/plan-x.md",
        "/tmp/example/oc.db",
        &ScopeFilter::All,
        &copy,
    );
    assert!(prompt.contains("Back up"), "{prompt}");
    assert!(prompt.contains("saniwalens/backups"), "{prompt}");
    assert!(
        prompt.contains("$XDG_STATE_HOME/saniwalens/backups"),
        "{prompt}"
    );
    assert!(prompt.contains("0700"), "{prompt}");
    assert!(prompt.contains("0600"), "{prompt}");
    assert!(prompt.contains("git-tracked"), "{prompt}");
    assert!(prompt.contains("untracked"), "{prompt}");
    assert!(prompt.contains("backup path"), "{prompt}");
    let diff = prompt
        .find("Show the planned diff")
        .expect("a diff step: {prompt}");
    let approval = prompt
        .find("Ask this item's choice")
        .expect("a per-item choice step: {prompt}");
    assert!(
        diff < approval,
        "the diff must precede the approval:\n{prompt}"
    );
    assert!(prompt.contains("Show the planned diff"), "{prompt}");
    assert!(prompt.contains("one item at a time"), "{prompt}");
    assert!(prompt.contains("no whole-plan batch approval"), "{prompt}");
    assert!(prompt.contains("`y` applies it"), "{prompt}");
    assert!(prompt.contains("`n` skips to the next item"), "{prompt}");
    assert!(prompt.contains("`q` aborts the whole session"), "{prompt}");
    assert!(prompt.contains("do the rest"), "{prompt}");
    assert!(prompt.contains("change nothing"), "{prompt}");
    assert!(prompt.contains("byte-identical"), "{prompt}");
    assert!(prompt.contains("keep the backups"), "{prompt}");
    assert!(prompt.contains("## Applied changes"), "{prompt}");
    assert!(
        prompt.contains("## Proposal only (not applied)"),
        "{prompt}"
    );
}

#[test]
fn interactive_shapes_match_the_spike_table() {
    // The Task 1 spike's launch-shape table, pinned: claude opens on the
    // bare prompt, kilo and opencode through `--prompt`. The opencode arm is
    // the expected shape (flag acceptance verified; one TTY-eyed
    // confirmation still outstanding).
    assert_eq!(
        Harness::Claude.interactive_launch_command(),
        ("claude", &[] as &[&str])
    );
    assert_eq!(
        Harness::Kilocode.interactive_launch_command(),
        ("kilo", &["--prompt"] as &[&str])
    );
    assert_eq!(
        Harness::Opencode.interactive_launch_command(),
        ("opencode", &["--prompt"] as &[&str])
    );
}
