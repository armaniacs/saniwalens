//! The guide's non-TTY contract, end to end through the real binary: without
//! an interactive terminal the guide never starts. Piped stdio is never a
//! TTY, so both cases below run non-interactive by construction.

mod common;

use common::ScratchDir;

fn run_piped(name: &str, args: &[&str]) -> std::process::Output {
    // Synthetic only: an empty `.claude/projects` tree for HOME isolation,
    // the same shape `tests/period_errors.rs` uses. No `--db` flag: `tui`
    // takes no flags (clap would reject it before the arm runs) and the
    // no-args case must reach the missing-subcommand error exactly as today.
    let dir = ScratchDir::new("saniwalens-guide-entry", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    std::process::Command::new(env!("CARGO_BIN_EXE_saniwalens"))
        .args(args)
        .env("HOME", &*dir)
        .env("CLAUDE_CONFIG_DIR", dir.join(".claude"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .output()
        .expect("the binary runs")
}

#[test]
fn tui_without_a_terminal_refuses_instead_of_hanging() {
    let out = run_piped("tui-piped", &["tui"]);
    assert!(
        !out.status.success(),
        "piped `tui` must fail, not wait on stdin"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("interactive terminal"),
        "refusal names the missing terminal: {stderr}"
    );
}

#[test]
fn no_subcommand_without_a_terminal_keeps_todays_clap_error() {
    let out = run_piped("no-args-piped", &[]);
    assert!(!out.status.success(), "piped no-args must fail as today");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Usage:"),
        "clap's missing-subcommand error is preserved byte-for-byte: {stderr}"
    );
}

#[test]
fn harness_with_flag_value_falls_through_to_clap_not_the_guide() {
    // `--harness --help` is a flag in the value position: guide_entry must
    // return Normal, so clap — not the guide — owns the invocation. clap
    // reports the missing value (exit 2); the guide never starts.
    let out = run_piped("harness-flag-value", &["--harness", "--help"]);
    assert!(!out.status.success(), "a flag value must fail as today");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("a value is required"),
        "clap's missing-value error owns the invocation: {stderr}"
    );
    assert!(
        !stderr.contains("interactive terminal"),
        "the guide must not start: {stderr}"
    );
}
