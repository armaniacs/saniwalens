use crate::adapter::harness::Harness;

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
            harness = args.next().cloned();
        } else if arg == "--profile" {
            profile = args.next().cloned();
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

#[cfg(test)]
mod tests {
    // appended inside `#[cfg(test)] mod tests` in src/cli/views/guide.rs
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
    fn equivalent_command_covers_every_combination() {
        let cases = [
            (Harness::Claude, Purpose::Doctor, ScopeChoice::All, "saniwalens doctor"),
            (Harness::Claude, Purpose::Waste, ScopeChoice::Global, "saniwalens waste --scope global"),
            (Harness::Opencode, Purpose::Failures, ScopeChoice::Project, "saniwalens --harness opencode failures --scope project"),
            (Harness::Kilocode, Purpose::Doctor, ScopeChoice::All, "saniwalens --harness kilocode doctor"),
        ];
        for (harness, purpose, scope, expected) in cases {
            assert_eq!(equivalent_command(&harness, purpose, scope), expected);
        }
    }
}
