use std::io::{BufRead, Write};

use crate::adapter::harness::{Harness, home_dir};
use crate::session::{SessionEnv, resolve_cli};

use super::super::window::{PeriodArgs, ReadArgs, WindowedDispatch};
use super::super::{doctor, failures, run_analyze, waste};

/// What `run()` should do with this invocation, decided before clap parses.
pub(crate) enum Entry {
    /// No subcommand (or the explicit `tui`): run the guide. Presets come
    /// from the global flags seen before the decision point.
    Guide { harness: Option<String>, profile: Option<String> },
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

/// The reproducible command line for a guided run: printed before the run
/// and reprinted after it, so a guided run is always re-runnable by hand.
/// This string is the guide's machine contract (no JSON golden: the guide
/// has no machine output); the table test above pins every combination.
pub(crate) fn equivalent_command(
    harness: &Harness,
    purpose: Purpose,
    scope: ScopeChoice,
) -> String {
    let harness_part = match harness {
        Harness::Claude => String::new(),
        Harness::Opencode => " --harness opencode".to_string(),
        Harness::Kilocode => " --harness kilocode".to_string(),
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

pub(crate) fn run_guide<R: BufRead, W: Write>(
    harness: &Harness,
    dispatch: &WindowedDispatch,
    input: &mut R,
    output: &mut W,
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
    run_guide(
        &resolved.harness,
        &dispatch,
        &mut stdin.lock(),
        &mut stdout.lock(),
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
            Entry::Guide { harness: None, profile: None }
        ));
    }

    #[test]
    fn harness_flag_becomes_the_preset_not_a_subcommand() {
        let argv = vec!["saniwalens".to_string(), "--harness".to_string(), "kilocode".to_string()];
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
        for (harness, h) in &harnesses {
            for (command, purpose) in &purposes {
                for (s, scope) in &scopes {
                    assert_eq!(
                        equivalent_command(harness, *purpose, *scope),
                        format!("saniwalens{h} {command}{s}"),
                    );
                }
            }
        }
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
        run_guide(&harness, &dispatch, &mut input, &mut output).unwrap();
        assert!(String::from_utf8(output).unwrap().contains("健康診断"));
    }
}
