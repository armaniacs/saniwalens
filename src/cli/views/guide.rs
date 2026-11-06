use std::io::{BufRead, Write};
use std::path::Path;

use crate::adapter::harness::{Harness, home_dir};
use crate::session::{SessionEnv, config_file_path, resolve_cli};

use super::super::window::{PeriodArgs, ReadArgs, WindowedDispatch};
use super::super::{doctor, failures, run_analyze, waste};

/// What `run()` should do with this invocation, decided before clap parses.
pub(crate) enum Entry {
    /// No subcommand (or the explicit `tui`): run the guide. Presets come
    /// from the global flags seen before the decision point.
    Guide {
        harness: Option<String>,
        profile: Option<String>,
    },
    /// Anything else: normal clap parsing (today's behavior, unchanged).
    Normal,
}

/// Pure argv classifier: `--harness` / `--profile` values are presets, `tui`
/// is the explicit entry, a `-` flag or any other token is a real command.
pub(crate) fn guide_entry(argv: &[String]) -> Entry {
    let mut harness: Option<String> = None;
    let mut profile: Option<String> = None;
    let mut args = argv.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--harness" {
            match args.next() {
                Some(value) => harness = Some(value.clone()),
                None => return Entry::Normal,
            }
        } else if arg == "--profile" {
            match args.next() {
                Some(value) => profile = Some(value.clone()),
                None => return Entry::Normal,
            }
        } else if arg == "tui" {
            return Entry::Guide { harness, profile };
        } else {
            // A `-` flag (--help, --version, …) or a real subcommand:
            // clap owns this invocation, not the guide.
            return Entry::Normal;
        }
    }
    Entry::Guide { harness, profile }
}

/// The three reports the guide offers (design doc §2: v0.5.4 scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    Doctor,
    Waste,
    Failures,
}

/// The `--scope` answers the guide offers (design doc Q6: ask once).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopeChoice {
    All,
    Global,
    Project,
}

/// Profile names are TOML bare keys and path-safe segments: ASCII
/// alphanumerics plus `-` and `_` only.
pub(crate) fn profile_name_is_valid(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Canonical harness spelling for the config file. Also used by
/// `equivalent_command` (replace its inline match with this).
pub(crate) fn harness_name(harness: &Harness) -> &'static str {
    match harness {
        Harness::Claude => "claude",
        Harness::Opencode => "opencode",
        Harness::Kilocode => "kilocode",
    }
}

/// The reproducible command line for a guided run: printed before the run
/// and reprinted after it, so a guided run is always re-runnable by hand.
/// This string is the guide's machine contract (no JSON golden: the guide
/// has no machine output); the table test above pins every combination.
pub(crate) fn equivalent_command(
    harness: &Harness,
    purpose: Purpose,
    scope: ScopeChoice,
) -> String {
    let harness_part = match harness_name(harness) {
        "claude" => String::new(),
        other => format!(" --harness {other}"),
    };
    let command = match purpose {
        Purpose::Doctor => "doctor",
        Purpose::Waste => "waste",
        Purpose::Failures => "failures",
    };
    let scope_part = match scope {
        ScopeChoice::All => String::new(),
        ScopeChoice::Global => " --scope global".to_string(),
        ScopeChoice::Project => " --scope project".to_string(),
    };
    format!("saniwalens{harness_part} {command}{scope_part}")
}

/// One answered prompt: a 0-based pick, or quit (`q`, EOF, Ctrl-C surfaces as
/// an Err from read_line and aborts the guide with the OS error).
pub(crate) enum Choice {
    Pick(usize),
    Quit,
}

/// Numbered prompt with a default: empty line takes it, `q` quits, anything
/// else retries with the valid range named. Generic over I/O so tests feed
/// `Cursor` instead of a terminal.
pub(crate) fn ask_choice<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    prompt: &str,
    options: &[&str],
    default: usize,
) -> anyhow::Result<Choice> {
    // Callers guarantee a non-empty menu with a default inside it.
    debug_assert!(!options.is_empty(), "ask_choice needs at least one option");
    debug_assert!(
        default < options.len(),
        "ask_choice default must name an option"
    );
    writeln!(output, "{prompt}")?;
    for (i, option) in options.iter().enumerate() {
        writeln!(output, "[{}] {option}", i + 1)?;
    }
    loop {
        write!(output, "> ")?;
        output.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(Choice::Quit);
        }
        let line = line.trim();
        if line.is_empty() {
            return Ok(Choice::Pick(default));
        }
        if line.eq_ignore_ascii_case("q") {
            return Ok(Choice::Quit);
        }
        match line.parse::<usize>() {
            Ok(n) if (1..=options.len()).contains(&n) => return Ok(Choice::Pick(n - 1)),
            _ => writeln!(output, "1-{} か q で答えてください", options.len())?,
        }
    }
}

/// New file text with `default_harness` set. Line-based edit: everything else
/// (comments, profiles, blank lines) is preserved verbatim. No Serialize
/// round-trip ― `FileConfig` is Deserialize-only, and a rewrite would eat
/// the user's comments.
pub(crate) fn render_default_harness(existing: Option<&str>, harness: &str) -> String {
    const HEADER: &str = "# saniwalens defaults (written by the guided TUI).";
    let line = format!("default_harness = \"{harness}\"");
    let Some(text) = existing else {
        return format!("{HEADER}\n{line}\n");
    };
    let mut out: Vec<String> = Vec::new();
    let mut done = false;
    let mut in_table = false;
    for raw in text.lines() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with('[') {
            in_table = true;
        }
        let is_target = !done
            && !in_table
            && trimmed.starts_with("default_harness")
            && trimmed["default_harness".len()..]
                .trim_start()
                .starts_with('=');
        out.push(if is_target {
            line.clone()
        } else {
            raw.to_string()
        });
        done = done || is_target;
    }
    if !done {
        let pos = out
            .iter()
            .position(|l| l.trim_start().starts_with('['))
            .unwrap_or(out.len());
        out.insert(pos, line);
    }
    let mut rendered = out.join("\n");
    rendered.push('\n');
    rendered
}

/// New file text with `[profiles.<name>]` carrying `harness`. Same
/// line-preserving contract as `render_default_harness`.
pub(crate) fn render_profile(existing: Option<&str>, name: &str, harness: &str) -> String {
    const HEADER: &str = "# saniwalens defaults (written by the guided TUI).";
    let header = format!("[profiles.{name}]");
    let line = format!("harness = \"{harness}\"");
    let Some(text) = existing else {
        return format!("{HEADER}\n{header}\n{line}\n");
    };
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut done = false;
    for raw in text.lines() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with('[') {
            // Close the section at the next header; insert the harness line
            // if the section had none.
            if in_section && !done {
                out.push(line.clone());
                done = true;
            }
            in_section = trimmed == header;
        }
        let is_target = in_section
            && !done
            && trimmed.starts_with("harness")
            && trimmed["harness".len()..].trim_start().starts_with('=');
        out.push(if is_target {
            line.clone()
        } else {
            raw.to_string()
        });
        done = done || is_target;
    }
    if in_section && !done {
        out.push(line.clone());
        done = true;
    }
    if !done {
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(header);
        out.push(line);
    }
    let mut rendered = out.join("\n");
    rendered.push('\n');
    rendered
}

/// The post-run save conversation: 3-choice (skip / default / profile),
/// profile-name prompt with validation retry, path + rendered text shown,
/// y/N confirm. Returns the text to write, or `None` for "don't save".
/// No filesystem access here ― the caller reads before and writes after ―
/// so `Cursor`-driven tests cover every branch without touching disk.
pub(crate) fn ask_save<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    existing: Option<&str>,
    harness: &str,
    path: &Path,
) -> anyhow::Result<Option<String>> {
    writeln!(
        output,
        "この設定を保存しますか? (config: {})",
        path.display()
    )?;
    let pick = ask_choice(
        input,
        output,
        "保存先を選んでください",
        &[
            "保存しない",
            "既定のharnessにする (default_harness)",
            "profileに保存する",
        ],
        0,
    )?;
    let rendered = match pick {
        Choice::Quit | Choice::Pick(0) => return Ok(None),
        Choice::Pick(1) => render_default_harness(existing, harness),
        _ => {
            let name = ask_profile_name(input, output)?;
            let Some(name) = name else {
                return Ok(None);
            };
            render_profile(existing, &name, harness)
        }
    };
    writeln!(output, "書き込む内容:\n{rendered}")?;
    writeln!(output, "保存しますか? [y/N]")?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    if line.trim().eq_ignore_ascii_case("y") {
        Ok(Some(rendered))
    } else {
        Ok(None)
    }
}

/// Free-text profile-name prompt with validation retry; `q`/EOF quits the
/// save (not the guide).
fn ask_profile_name<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
) -> anyhow::Result<Option<String>> {
    loop {
        write!(output, "profile名 (英数・-・_)? [qでやめる] ")?;
        output.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let name = line.trim();
        if name.eq_ignore_ascii_case("q") {
            return Ok(None);
        }
        if profile_name_is_valid(name) {
            return Ok(Some(name.to_string()));
        }
        writeln!(output, "使えるのは英数・-・_だけです")?;
    }
}

/// Thin filesystem wrapper: create parents, write atomically-by-rename is
/// overkill for a small config ― a direct write is what the tests pin.
fn persist_config(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}

pub(crate) fn run_guide<R: BufRead, W: Write>(
    harness: &Harness,
    dispatch: &WindowedDispatch,
    input: &mut R,
    output: &mut W,
    config_path: Option<&Path>,
) -> anyhow::Result<()> {
    let harness_names = ["claude", "opencode", "kilocode"];
    let current = match harness {
        Harness::Claude => 0,
        Harness::Opencode => 1,
        Harness::Kilocode => 2,
    };
    // Step 1 (design §2): confirm, not re-ask ― Enter keeps the resolved value.
    writeln!(output, "使っているエージェント: {}", harness_names[current])?;

    let period = PeriodArgs {
        since: None,
        until: None,
        last: None,
    };
    let read = ReadArgs {
        frozen: false,
        db: None,
    };
    loop {
        // Step 2: purpose.
        let purpose = match ask_choice(
            input,
            output,
            "やりたいことを選んでください（q で終了）",
            &[
                "健康診断 (doctor)",
                "無駄を探す (waste)",
                "失敗を調べる (failures)",
            ],
            0,
        )? {
            Choice::Quit => return Ok(()),
            Choice::Pick(0) => Purpose::Doctor,
            Choice::Pick(1) => Purpose::Waste,
            _ => Purpose::Failures,
        };
        // Step 3: scope.
        let scope = match ask_choice(
            input,
            output,
            "範囲を選んでください",
            &["全体", "global", "project"],
            0,
        )? {
            Choice::Quit => return Ok(()),
            Choice::Pick(0) => ScopeChoice::All,
            Choice::Pick(1) => ScopeChoice::Global,
            _ => ScopeChoice::Project,
        };
        // Step 4: show the command, run it, reprint it.
        let command = equivalent_command(harness, purpose, scope);
        writeln!(output, "実行: {command}")?;
        let scope_flag: Option<&str> = match scope {
            ScopeChoice::All => None,
            ScopeChoice::Global => Some("global"),
            ScopeChoice::Project => Some("project"),
        };
        match purpose {
            Purpose::Doctor => {
                let opened = dispatch.open_scoped_table_json(
                    scope_flag,
                    None,
                    &period,
                    &read,
                    run_analyze,
                )?;
                let (windowed, filter) = opened.into_parts();
                doctor(&windowed.sess, &filter, windowed.window, windowed.format)?;
            }
            Purpose::Waste => {
                let opened = dispatch.open_scoped(scope_flag, None, &period, &read, run_analyze)?;
                let (windowed, filter) = opened.into_parts();
                waste(&windowed.sess, &filter, windowed.window, windowed.format)?;
            }
            Purpose::Failures => {
                let opened = dispatch.open_scoped(scope_flag, None, &period, &read, run_analyze)?;
                let (windowed, filter) = opened.into_parts();
                failures(&windowed.sess, &filter, windowed.window, windowed.format)?;
            }
        }
        writeln!(output, "実行したコマンド: {command}")?;
        if let Some(path) = config_path {
            let existing = std::fs::read_to_string(path).ok();
            if let Some(text) = ask_save(
                input,
                output,
                existing.as_deref(),
                harness_name(harness),
                path,
            )? {
                persist_config(path, &text)?;
            }
        }
        writeln!(output, "もう1件見る? [y/N]")?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        if !line.trim().eq_ignore_ascii_case("y") {
            return Ok(());
        }
    }
}

pub(crate) fn run_guide_entry(harness: Option<&str>, profile: Option<&str>) -> anyhow::Result<()> {
    let state_home = std::env::var_os("XDG_STATE_HOME").map(std::path::PathBuf::from);
    let config_home = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from);
    let env = SessionEnv {
        state_home: state_home.as_deref(),
        home: home_dir(),
        config_home: config_home.as_deref(),
    };
    let resolved = resolve_cli(harness, profile, &env)?;
    let dispatch = WindowedDispatch {
        harness: &resolved.harness,
        config_db: resolved.config_db.as_deref(),
        env: &env,
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let config_path = config_file_path(config_home.as_deref(), home_dir());
    run_guide(
        &resolved.harness,
        &dispatch,
        &mut stdin.lock(),
        &mut stdout.lock(),
        config_path.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_invocation_is_a_guide_with_no_preset() {
        let argv = vec!["saniwalens".to_string()];
        assert!(matches!(
            guide_entry(&argv),
            Entry::Guide {
                harness: None,
                profile: None
            }
        ));
    }

    #[test]
    fn harness_flag_becomes_the_preset_not_a_subcommand() {
        let argv = vec![
            "saniwalens".to_string(),
            "--harness".to_string(),
            "kilocode".to_string(),
        ];
        assert!(matches!(
            guide_entry(&argv),
            Entry::Guide { harness: Some(h), profile: None } if h == "kilocode"
        ));
    }

    #[test]
    fn tui_is_an_explicit_guide_entry() {
        let argv = vec!["saniwalens".to_string(), "tui".to_string()];
        assert!(matches!(guide_entry(&argv), Entry::Guide { .. }));
    }

    #[test]
    fn a_real_subcommand_is_not_a_guide() {
        for argv in [
            vec!["saniwalens", "doctor"],
            vec!["saniwalens", "--help"],
            vec!["saniwalens", "doctor", "--scope", "global"],
        ] {
            let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
            assert!(matches!(guide_entry(&owned), Entry::Normal), "{argv:?}");
        }
    }

    #[test]
    fn trailing_harness_without_a_value_is_normal() {
        let argv = vec!["saniwalens".to_string(), "--harness".to_string()];
        assert!(matches!(guide_entry(&argv), Entry::Normal));
    }

    #[test]
    fn trailing_profile_without_a_value_is_normal() {
        let argv = vec!["saniwalens".to_string(), "--profile".to_string()];
        assert!(matches!(guide_entry(&argv), Entry::Normal));
    }

    #[test]
    fn equivalent_command_covers_the_full_matrix() {
        let harnesses = [
            (Harness::Claude, ""),
            (Harness::Opencode, " --harness opencode"),
            (Harness::Kilocode, " --harness kilocode"),
        ];
        let purposes = [
            ("doctor", Purpose::Doctor),
            ("waste", Purpose::Waste),
            ("failures", Purpose::Failures),
        ];
        let scopes = [
            ("", ScopeChoice::All),
            (" --scope global", ScopeChoice::Global),
            (" --scope project", ScopeChoice::Project),
        ];
        let mut n = 0;
        for (harness, h) in &harnesses {
            for (command, purpose) in &purposes {
                for (s, scope) in &scopes {
                    n += 1;
                    assert_eq!(
                        equivalent_command(harness, *purpose, *scope),
                        format!("saniwalens{h} {command}{s}"),
                    );
                }
            }
        }
        assert_eq!(n, 27, "3 harnesses x 3 purposes x 3 scopes");
    }

    #[test]
    fn empty_answer_takes_the_default() {
        let mut input = std::io::Cursor::new("\n");
        let mut output = Vec::new();
        assert!(matches!(
            ask_choice(&mut input, &mut output, "prompt", &["a", "b"], 1).unwrap(),
            Choice::Pick(1)
        ));
    }

    #[test]
    fn q_quits_and_eof_quits() {
        let mut output = Vec::new();
        for stdin in ["q\n", "Q\n", ""] {
            let mut input = std::io::Cursor::new(stdin);
            assert!(
                matches!(
                    ask_choice(&mut input, &mut output, "p", &["a"], 0).unwrap(),
                    Choice::Quit
                ),
                "{stdin:?}"
            );
        }
    }

    #[test]
    fn out_of_range_retries_then_accepts() {
        let mut input = std::io::Cursor::new("9\n2\n");
        let mut output = Vec::new();
        assert!(matches!(
            ask_choice(&mut input, &mut output, "p", &["a", "b"], 0).unwrap(),
            Choice::Pick(1)
        ));
        let shown = String::from_utf8(output).unwrap();
        assert!(
            shown.contains("1-2"),
            "retry hint names the valid range: {shown}"
        );
    }

    #[test]
    fn quitting_at_the_first_prompt_runs_no_report() {
        // No store, no harness roots, no dispatch work: `q` exits before any I/O
        // beyond the prompt. The harness value is never read on this path.
        let harness = Harness::Claude;
        let env = SessionEnv {
            state_home: None,
            home: Some("/nonexistent"),
            config_home: None,
        };
        let dispatch = WindowedDispatch {
            harness: &harness,
            config_db: None,
            env: &env,
        };
        let mut input = std::io::Cursor::new("q\n");
        let mut output = Vec::new();
        run_guide(&harness, &dispatch, &mut input, &mut output, None).unwrap();
        let shown = String::from_utf8(output).unwrap();
        assert!(shown.contains("健康診断"));
        assert!(!shown.contains("実行:"));
    }

    #[test]
    fn default_harness_save_builds_a_new_file_from_nothing() {
        assert_eq!(
            render_default_harness(None, "kilocode"),
            "# saniwalens defaults (written by the guided TUI).\ndefault_harness = \"kilocode\"\n",
        );
    }

    #[test]
    fn default_harness_save_replaces_only_its_own_line() {
        let existing =
            "# keep me\ndefault_harness = \"claude\"\n[profiles.work]\nharness = \"opencode\"\n";
        let out = render_default_harness(Some(existing), "kilocode");
        assert!(out.contains("# keep me\n"), "comments survive: {out}");
        assert!(
            out.contains("default_harness = \"kilocode\"\n"),
            "value replaced: {out}"
        );
        assert!(
            out.contains("[profiles.work]\nharness = \"opencode\"\n"),
            "other sections untouched: {out}"
        );
        assert_eq!(
            out.matches("default_harness").count(),
            1,
            "no duplication: {out}"
        );
    }

    #[test]
    fn default_harness_save_inserts_before_the_first_table() {
        let existing = "[profiles.work]\nharness = \"opencode\"\n";
        let out = render_default_harness(Some(existing), "claude");
        assert!(
            out.starts_with("default_harness = \"claude\"\n"),
            "top-level, before [profiles]: {out}"
        );
    }

    #[test]
    fn profile_save_appends_a_new_section() {
        let out = render_profile(None, "work", "kilocode");
        assert!(
            out.contains("[profiles.work]\nharness = \"kilocode\"\n"),
            "{out}"
        );
    }

    #[test]
    fn profile_save_updates_the_harness_line_inside_its_section() {
        let existing = "default_harness = \"claude\"\n[profiles.work]\nharness = \"opencode\"\n[profiles.home]\nharness = \"claude\"\n";
        let out = render_profile(Some(existing), "work", "kilocode");
        assert!(
            out.contains("[profiles.work]\nharness = \"kilocode\"\n"),
            "{out}"
        );
        assert!(
            out.contains("[profiles.home]\nharness = \"claude\"\n"),
            "other profiles untouched: {out}"
        );
    }

    #[test]
    fn profile_names_allow_only_ascii_word_chars_and_dashes() {
        assert!(profile_name_is_valid("work"));
        assert!(profile_name_is_valid("my-profile_2"));
        assert!(!profile_name_is_valid(""));
        assert!(!profile_name_is_valid("my profile"));
        assert!(!profile_name_is_valid("../../etc"));
    }

    #[test]
    fn ask_save_skip_returns_none() {
        let mut input = std::io::Cursor::new("1\n");
        let mut output = Vec::new();
        let path = std::path::Path::new("/tmp/saniwalens-test-config.toml");
        let out = ask_save(&mut input, &mut output, None, "kilocode", path).unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn ask_save_default_choice_with_y_writes_default_harness() {
        let mut input = std::io::Cursor::new("2\ny\n");
        let mut output = Vec::new();
        let path = std::path::Path::new("/tmp/saniwalens-test-config.toml");
        let out = ask_save(&mut input, &mut output, None, "kilocode", path)
            .unwrap()
            .expect("y confirms the save");
        assert!(out.contains("default_harness = \"kilocode\"\n"), "{out}");
    }

    #[test]
    fn ask_save_profile_choice_retries_an_invalid_name() {
        let mut input = std::io::Cursor::new("3\nbad name\nwork\ny\n");
        let mut output = Vec::new();
        let path = std::path::Path::new("/tmp/saniwalens-test-config.toml");
        let out = ask_save(&mut input, &mut output, None, "kilocode", path)
            .unwrap()
            .expect("y confirms the save");
        assert!(
            out.contains("[profiles.work]\nharness = \"kilocode\"\n"),
            "{out}"
        );
        let shown = String::from_utf8(output).unwrap();
        assert!(shown.contains("使えるのは英数"), "{shown}");
    }

    #[test]
    fn ask_save_q_at_choice_returns_none() {
        let mut input = std::io::Cursor::new("q\n");
        let mut output = Vec::new();
        let path = std::path::Path::new("/tmp/saniwalens-test-config.toml");
        let out = ask_save(&mut input, &mut output, None, "kilocode", path).unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn ask_save_n_at_confirm_returns_none() {
        let mut input = std::io::Cursor::new("2\nn\n");
        let mut output = Vec::new();
        let path = std::path::Path::new("/tmp/saniwalens-test-config.toml");
        let out = ask_save(&mut input, &mut output, None, "kilocode", path).unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn persist_config_round_trip_through_a_pid_unique_dir() {
        let dir =
            std::env::temp_dir().join(format!("saniwalens-guide-test-{}", std::process::id()));
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);
        persist_config(&path, "default_harness = \"kilocode\"\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "default_harness = \"kilocode\"\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
