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

/// Like `run_in`, but the locale variables start cleared (hermetic against
/// the runner's own `LANG`) and then take exactly the given pairs, so each
/// locale case names its full `LC_ALL` / `LC_MESSAGES` / `LANG` triple.
fn run_in_with_locale(home: &Path, args: &[&str], locale: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(binary());
    cmd.args(args)
        .env("HOME", home)
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANG");
    for (key, value) in locale {
        cmd.env(key, value);
    }
    cmd.output().expect("the binary runs")
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
    print_prompt_with_locale(home, extra, &[])
}

/// [`print_prompt`] with an explicit locale triple on top of cleared
/// variables (see [`run_in_with_locale`]).
fn print_prompt_with_locale(
    home: &Path,
    extra: &[&str],
    locale: &[(&str, &str)],
) -> std::process::Output {
    let db = home.join("s.db");
    let mut args = vec![
        "optimize",
        "--print",
        "--frozen",
        "--db",
        db.to_str().expect("the sandbox path is UTF-8"),
    ];
    args.extend(extra);
    run_in_with_locale(home, &args, locale)
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

// PBI `2026-11-11-01-feat-locale-language-fallback.md`, outside-in: the
// locale triple is the only language source, so each case names its full
// `LC_ALL` / `LC_MESSAGES` / `LANG` triple on top of cleared variables.

#[test]
fn locale_lang_ja_carries_the_language_sentence_in_print() {
    // BDD "ロケールだけで日本語になる": with no flag and no config,
    // `LANG=ja_JP.UTF-8` carries the single sentence.
    let home = sandbox("locale-lang-ja");
    let out = print_prompt_with_locale(&home, &[], &[("LANG", "ja_JP.UTF-8")]);
    assert!(
        out.status.success(),
        "print failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(language_sentence()),
        "LANG=ja lost its sentence:\n{stdout}"
    );
}

#[test]
fn locale_lc_all_c_beats_lang_ja() {
    // BDD "LC_ALL が LANG に勝つ": POSIX order silences a `ja` LANG.
    let home = sandbox("locale-lc-all-wins");
    let out = print_prompt_with_locale(&home, &[], &[("LANG", "ja_JP.UTF-8"), ("LC_ALL", "C")]);
    assert!(
        out.status.success(),
        "print failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        !stdout.contains(language_sentence()),
        "LC_ALL=C lost to LANG=ja:\n{stdout}"
    );
}

#[test]
fn explicit_flag_beats_the_locale() {
    // BDD "明示指定はロケールに勝つ" (flag half): `--language en` silences
    // a `ja` locale.
    let home = sandbox("locale-flag-wins");
    let out = print_prompt_with_locale(&home, &["--language", "en"], &[("LANG", "ja_JP.UTF-8")]);
    assert!(
        out.status.success(),
        "print failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        !stdout.contains(language_sentence()),
        "the flag lost to the locale:\n{stdout}"
    );
}

#[test]
fn config_ja_beats_a_c_locale() {
    // BDD "明示指定はロケールに勝つ" (config half): `language = "ja"`
    // survives a `C` locale.
    let home = sandbox("locale-config-wins");
    write_config(&home, "language = \"ja\"\n");
    let out = print_prompt_with_locale(&home, &[], &[("LANG", "C")]);
    assert!(
        out.status.success(),
        "print failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(language_sentence()),
        "the config lost to the locale:\n{stdout}"
    );
}

#[test]
fn locale_c_prints_byte_identical_to_unset() {
    // BDD "変な値は既定に落ちる": a `C` locale renders exactly like no
    // locale at all — the default stays English with no sentence. Both
    // runs share one sandbox so the store path inside the prompt is equal.
    let home = sandbox("locale-c-byte-identical");
    let unset = print_prompt_with_locale(&home, &[], &[]);
    assert!(
        unset.status.success(),
        "print failed: {}",
        String::from_utf8_lossy(&unset.stderr)
    );
    let c = print_prompt_with_locale(&home, &[], &[("LANG", "C")]);
    assert!(
        c.status.success(),
        "print failed: {}",
        String::from_utf8_lossy(&c.stderr)
    );
    assert_eq!(
        c.stdout, unset.stdout,
        "a C locale changed the prompt bytes"
    );
    for (name, value) in [("empty", ""), ("en_US", "en_US.UTF-8")] {
        let out = print_prompt_with_locale(&home, &[], &[("LANG", value)]);
        assert!(
            out.status.success(),
            "print failed for {name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            out.stdout, unset.stdout,
            "a {name} locale changed the prompt bytes"
        );
    }
}
