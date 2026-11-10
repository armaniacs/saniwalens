//! The advisor language setting, outside-in
//! (PBI `2026-11-10-07-feat-advisor-language-setting.md`).
//!
//! `optimize --print` never launches an advisor, so the full prompt (briefing
//! inline) is asserted through the real binary on synthetic homes: a `ja`
//! setting must carry the single language sentence, an unset language must
//! not, and an unknown value must fail naming the valid ones. Every fixture
//! is synthetic; no advisor binary is ever spawned.

use std::path::{Path, PathBuf};
use std::process::Command;

use saniwalens::core::optimize::AdvisorLanguage;

mod common;
use common::ScratchDir;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_saniwalens"))
}

/// A synthetic home sandbox: `HOME`, `XDG_STATE_HOME`, and `XDG_CONFIG_HOME`
/// all point into scratch space, so the binary under test never touches the
/// real home, state, or config.
fn sandbox(name: &str) -> ScratchDir {
    ScratchDir::new("saniwalens-advisor-language", name)
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

/// Write the tool's own config file under the sandbox's `XDG_CONFIG_HOME`.
fn write_config(home: &Path, content: &str) {
    let dir = home.join("config").join("saniwalens");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), content).unwrap();
}

/// `optimize --print --frozen` on an empty synthetic store: `--frozen` skips
/// the refresh, `--print` skips every spawn, and the briefing renders its
/// empty sections away — so the prompt shape is asserted, not the analysis.
fn print_prompt(home: &Path, extra: &[&str]) -> std::process::Output {
    let db = home.join("s.db");
    let mut args = vec![
        "optimize",
        "--print",
        "--frozen",
        "--db",
        db.to_str().expect("the sandbox path is UTF-8"),
    ];
    args.extend(extra);
    run_in(home, &args)
}

fn language_sentence() -> &'static str {
    AdvisorLanguage::Ja
        .instruction_sentence()
        .expect("ja carries the single wording source")
}

#[test]
fn ja_config_carries_the_language_sentence_in_print() {
    // BDD "日本語設定で指示文に日本語応答指示が入る": with
    // `language = "ja"` the printed prompt carries the single sentence.
    let home = sandbox("ja-config");
    write_config(&home, "language = \"ja\"\n");
    let out = print_prompt(&home, &[]);
    assert!(
        out.status.success(),
        "print failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(language_sentence()),
        "the ja sentence is missing:\n{stdout}"
    );
}

#[test]
fn language_flag_beats_the_file() {
    // The flag wins over the file: `--language en` silences a `ja` file.
    let home = sandbox("flag-beats-file");
    write_config(&home, "language = \"ja\"\n");
    let out = print_prompt(&home, &["--language", "en"]);
    assert!(
        out.status.success(),
        "print failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        !stdout.contains(language_sentence()),
        "the flag lost to the file:\n{stdout}"
    );
}

#[test]
fn unset_language_prints_no_sentence() {
    // BDD "未設定では何も変わらない" at the process seam: without any
    // language setting the printed prompt carries no language sentence.
    let home = sandbox("unset");
    let out = print_prompt(&home, &[]);
    assert!(
        out.status.success(),
        "print failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        !stdout.contains(language_sentence()),
        "unset output gained a sentence:\n{stdout}"
    );
    assert!(stdout.contains("# saniwalens analysis"), "{stdout}");
}

#[test]
fn unknown_language_fails_with_the_valid_list() {
    // BDD "不正値はエラーになる": an unknown value fails, naming the value
    // and the valid list — before any store work.
    let home = sandbox("unknown");
    write_config(&home, "language = \"jp\"\n");
    let out = print_prompt(&home, &[]);
    assert!(!out.status.success(), "an unknown language was accepted");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("jp"), "{stderr}");
    assert!(stderr.contains("en"), "{stderr}");
    assert!(stderr.contains("ja"), "{stderr}");
    assert!(
        !home.join("s.db").exists(),
        "the store was touched ahead of the refusal"
    );
}
