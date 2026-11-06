# Guided TUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` (recommended) to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the no-args / `tui` interactive guide (harness → purpose → scope → run) per `docs/exploration/2026-11-06-guided-tui-design.md`, satisfying the repo's view-addition acceptance criteria.

**Architecture:** All guide logic lives in one new file `src/cli/views/guide.rs` (pure, testable core: argv classification, choice prompts on generic I/O, equivalent-command builder) and reuses the existing `WindowedDispatch` opens plus the existing `doctor` / `waste` / `failures` report functions in-process. `src/cli/mod.rs` gains only the `Tui` variant, its dispatch arm, and the no-args branch.

**Tech Stack:** Rust (stable), std only (`std::io::IsTerminal`, `BufRead` / `Write` generics for testability). No new dependencies.

---

## File map

- Create: `src/cli/views/guide.rs` — `Entry`, `guide_entry()`, `Purpose`, `ScopeChoice`, `equivalent_command()`, `Choice`, `ask_choice()`, `run_guide()`, `run_guide_entry()` + unit tests.
- Create: `src/cli/views/mod.rs` — `pub(crate) mod guide; pub(crate) mod trend;` plus re-exports.
- Move: `src/cli/trend.rs` → `src/cli/views/trend.rs` (`git mv`, same commit as the views dir creation).
- Modify: `src/cli/mod.rs` — replace `mod trend` / `use trend::*` with `mod views` / `use views::trend::*`, add `Command::Tui` + arm + no-args branch (`use std::io::IsTerminal;`). Nothing else: no view logic here.
- Modify: `src/cli/window.rs` — tests-only: add `Command::Tui` to the `Analyze | Sql | Optimize` fallthrough arm of `windowed_flags()` (line ~644).
- Modify: `tests/cli_help_golden.rs` — `WHOLE_HISTORY: [&str; 3]` → `[&str; 4]` adding `"tui"` (`tui` carries no `--last`, so it is whole-history, not `PINNED`; `WINDOWED_VIEW_COUNT` stays 9).
- Modify: `tests/cli_mod_line_budget.rs` — raise `MOD_RS_LINE_BUDGET` to the new measured `wc -l` count (exact number set at implementation time; raising is a flagged review point, done here with the spec update).
- Create: `tests/guide_entry.rs` — piped-stdin refusal tests (non-TTY `tui` refuses; non-TTY no-args keeps clap's missing-subcommand error).
- Modify: `docs/specs/cli.md` — guide section under Views; `docs/specs/architecture.md` — layer table row (line ~101) + view-landing rule status (lines ~103–122: "created when the next view lands" is now done); `CHANGELOG.md` — new `## [Unreleased]` Added entry.

---

### Task 1: `src/cli/views/` split groundwork

**Files:**
- Move: `src/cli/trend.rs` → `src/cli/views/trend.rs`
- Create: `src/cli/views/mod.rs`
- Modify: `src/cli/mod.rs` (module declarations only)
- Test: `cargo test` (existing suite must stay green; proves the move is behavior-free)

- [ ] **Step 1: Move trend.rs and create the views module**

```bash
git mv src/cli/trend.rs src/cli/views/trend.rs
```

```rust
// src/cli/views/mod.rs (new file, complete content)
pub(crate) mod guide;
pub(crate) mod trend;
```

- [ ] **Step 2: Repoint mod.rs declarations (2 lines out, 2 lines in)**

```rust
// src/cli/mod.rs: replace
pub(crate) mod trend;
// with
pub(crate) mod views;
```

```rust
// src/cli/mod.rs: replace
pub(crate) use trend::*;
// with
pub(crate) use views::trend::*;
```

- [ ] **Step 3: Run the full suite to prove the move is behavior-free**

Run: `cargo test`
Expected: all green (same counts as before the move; `trend_json_golden`, `cli_help_golden`, `cli_mod_line_budget` all pass — the budget still holds because `mod.rs` line count is unchanged)

- [ ] **Step 4: Commit**

```bash
git add src/cli/views src/cli/mod.rs && git commit -m "refactor(cli): land src/cli/views/ and move trend.rs into it"
```

---

### Task 2: Pure core — entry classifier, purpose/scope, equivalent command

**Files:**
- Modify: `src/cli/views/guide.rs` (new; start with this task's items + `#[cfg(test)] mod tests`)
- Modify: `src/cli/mod.rs` — nothing in this task (guide.rs is not wired yet, so add `pub(crate) mod guide;` to `views/mod.rs` only — already done in Task 1 — and keep `guide` `#[allow(dead_code)]`-free by testing it directly: unit tests in the same file count as use)

- [ ] **Step 1: Write the failing tests**

```rust
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
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test views::guide`
Expected: FAIL — `could not find guide in views` / unresolved items (file does not exist yet)

- [ ] **Step 3: Write the minimal implementation**

```rust
// src/cli/views/guide.rs (complete content for this task)
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
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test views::guide`
Expected: PASS (5 tests)

- [ ] **Step 5: Commit**

```bash
git add src/cli/views/guide.rs && git commit -m "feat(cli): pure guide core (entry classifier and equivalent command)"
```

---

### Task 3: Prompt + loop on injectable I/O

**Files:**
- Modify: `src/cli/views/guide.rs` (append `Choice`, `ask_choice`, `run_guide`, `run_guide_entry` + tests)
- Test: `cargo test views::guide`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn empty_answer_takes_the_default() {
    let mut input = std::io::Cursor::new("
");
    let mut output = Vec::new();
    assert!(matches!(
        ask_choice(&mut input, &mut output, "prompt", &["a", "b"], 1).unwrap(),
        Choice::Pick(1)
    ));
}

#[test]
fn q_quits_and_eof_quits() {
    let mut output = Vec::new();
    for stdin in ["q
", "Q
", ""] {
        let mut input = std::io::Cursor::new(stdin);
        assert!(
            matches!(ask_choice(&mut input, &mut output, "p", &["a"], 0).unwrap(), Choice::Quit),
            "{stdin:?}"
        );
    }
}

#[test]
fn out_of_range_retries_then_accepts() {
    let mut input = std::io::Cursor::new("9
2
");
    let mut output = Vec::new();
    assert!(matches!(
        ask_choice(&mut input, &mut output, "p", &["a", "b"], 0).unwrap(),
        Choice::Pick(1)
    ));
    let shown = String::from_utf8(output).unwrap();
    assert!(shown.contains("1-2"), "retry hint names the valid range: {shown}");
}

#[test]
fn quitting_at_the_first_prompt_runs_no_report() {
    // No store, no harness roots, no dispatch work: `q` exits before any I/O
    // beyond the prompt. The harness value is never read on this path.
    let harness = Harness::Claude;
    let dispatch = fake_dispatch();
    let mut input = std::io::Cursor::new("q
");
    let mut output = Vec::new();
    run_guide(&harness, &dispatch, &mut input, &mut output).unwrap();
    assert!(String::from_utf8(output).unwrap().contains("健康診断"));
}
```

Note: `fake_dispatch()` cannot exist without a real `Session` — a `WindowedDispatch` borrows `&Harness`, `Option<&Path>`, `&SessionEnv`, all constructible without touching disk:
`SessionEnv { state_home: None, home: Path::new("/nonexistent"), config_home: None }`.
Check `SessionEnv`'s exact fields in `src/session.rs` at implementation time and mirror them. (If `SessionEnv` gains a lifetime or private fields, construct it the same way `run()` does — the fields are read in `src/cli/mod.rs` lines 246–252.)

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test views::guide`
Expected: FAIL — `ask_choice`, `Choice`, `run_guide` do not exist

- [ ] **Step 3: Write the minimal implementation**

```rust
use std::io::{BufRead, Write};

use super::super::{run_analyze, window::{PeriodArgs, ReadArgs, WindowedDispatch}};
use super::super::report::{doctor, failures, waste};
use crate::session::{SessionEnv, resolve_cli};

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
```

`run_guide` (same file): harness confirm → loop { purpose → scope → print command → execute with default flags → reprint → continue? }. Execution mirrors the `run()` arms exactly, with defaults:

```rust
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
    // Step 1 (design §2): confirm, not re-ask — Enter keeps the resolved value.
    writeln!(output, "使っているエージェント: {}", harness_names[current])?;

    let period = PeriodArgs { since: None, until: None, last: None };
    let read = ReadArgs { frozen: false, db: None };
    loop {
        // Step 2: purpose.
        let purpose = match ask_choice(
            input, output,
            "やりたいことを選んでください（q で終了）",
            &["健康診断 (doctor)", "無駄を探す (waste)", "失敗を調べる (failures)"],
            0,
        )? {
            Choice::Quit => return Ok(()),
            Choice::Pick(0) => Purpose::Doctor,
            Choice::Pick(1) => Purpose::Waste,
            _ => Purpose::Failures,
        };
        // Step 3: scope.
        let scope = match ask_choice(
            input, output,
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
                let opened =
                    dispatch.open_scoped_table_json(scope_flag, None, &period, &read, run_analyze)?;
                let (windowed, filter) = opened.into_parts();
                doctor(&windowed.sess, &filter, windowed.window, windowed.format)?;
            }
            Purpose::Waste => {
                let opened =
                    dispatch.open_scoped(scope_flag, None, &period, &read, run_analyze)?;
                let (windowed, filter) = opened.into_parts();
                waste(&windowed.sess, &filter, windowed.window, windowed.format)?;
            }
            Purpose::Failures => {
                let opened =
                    dispatch.open_scoped(scope_flag, None, &period, &read, run_analyze)?;
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
```

Stdio entry (same file; mirrors `run()` lines 246–263 for env + resolve):

```rust
pub(crate) fn run_guide_entry(harness: Option<&str>, profile: Option<&str>) -> anyhow::Result<()> {
    use crate::adapter::harness::home_dir;
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
```

(If `resolved`'s field names differ — `harness`, `config_db` per `run()` lines 253–255 — mirror `run()` exactly; the compiler points at any drift.)

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test views::guide`
Expected: PASS (all new tests; `quitting_at_the_first_prompt_runs_no_report` proves no store is touched before the first answer)

- [ ] **Step 5: Commit**

```bash
git add src/cli/views/guide.rs && git commit -m "feat(cli): guided prompt loop reusing the report dispatches"
```

---

### Task 4: Wire `run()` — no-args branch + `Tui` variant

**Files:**
- Modify: `src/cli/mod.rs` (variant + arm + branch + `IsTerminal` import only)
- Modify: `src/cli/window.rs` (tests-only fallthrough arm)
- Test: `cargo test` (tripwire + partition tests will fail first — that's Task 5's input)

- [ ] **Step 1: Add the variant, the arm, and the pre-parse branch**

```rust
// enum Command, after the Optimize variant:
/// Step-by-step guide to the typical patterns (doctor / waste / failures).
/// Needs an interactive terminal; without one it refuses instead of hanging.
Tui,
```

```rust
// match cli.command, after the Optimize arm:
Command::Tui => {
    if !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal()) {
        anyhow::bail!("`tui` needs an interactive terminal (stdin and stdout must both be TTYs)");
    }
    views::guide::run_guide_entry(cli.harness.as_deref(), cli.profile.as_deref())
}
```

```rust
// top of run(), before `let cli = Cli::parse();`:
let argv: Vec<String> = std::env::args().collect();
if let views::guide::Entry::Guide { harness, profile } = views::guide::guide_entry(&argv) {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        return views::guide::run_guide_entry(harness.as_deref(), profile.as_deref());
    }
    // Non-TTY without a subcommand: fall through to `Cli::parse`, whose
    // missing-subcommand error is what scripts see today (behavior unchanged).
}
```

```rust
// imports: add to the existing `use std::...` block
use std::io::IsTerminal;
```

- [ ] **Step 2: Extend the window.rs test fallthrough (line ~644)**

```rust
Command::Analyze { .. } | Command::Sql { .. } | Command::Optimize { .. } | Command::Tui => {
```

- [ ] **Step 3: Run the suite and record the two expected failures**

Run: `cargo test`
Expected: FAIL in `cli_mod_line_budget` (over by the added lines) and `cli_help_golden::every_subcommand_is_either_windowed_or_whole_history` (`tui` uncovered). Everything else green. Note the exact over-budget count for Task 6.

- [ ] **Step 4: Commit**

```bash
git add src/cli/mod.rs src/cli/window.rs && git commit -m "feat(cli): wire the guide (no-args TTY branch and tui subcommand)"
```

---

### Task 5: Tripwire updates + refusal integration tests

**Files:**
- Modify: `tests/cli_help_golden.rs` (`WHOLE_HISTORY` 3 → 4)
- Create: `tests/guide_entry.rs`
- Test: `cargo test --test cli_help_golden --test guide_entry`

- [ ] **Step 1: Claim `tui` as whole-history**

```rust
const WHOLE_HISTORY: [&str; 4] = ["analyze", "sql", "optimize", "tui"];
```

(`WINDOWED_VIEW_COUNT` stays 9; `tui` carries no `--last`, so it is not `PINNED`.)

- [ ] **Step 2: Write the refusal tests**

```rust
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
    assert!(!out.status.success(), "piped `tui` must fail, not wait on stdin");
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
```

- [ ] **Step 3: Run to verify they pass (red first if the Task 4 arm is missing)**

Run: `cargo test --test guide_entry --test cli_help_golden`
Expected: PASS. (If run against a tree without Task 4, both fail — that is the red proof; with Task 4, green.)

- [ ] **Step 4: Commit**

```bash
git add tests/guide_entry.rs tests/cli_help_golden.rs && git commit -m "test(cli): pin the guide's non-TTY contract and claim tui as whole-history"
```

---

### Task 6: Specs, budget, changelog, full green

**Files:**
- Modify: `docs/specs/cli.md` (guide section under Views), `docs/specs/architecture.md` (layer-table report row ~line 101 + view-landing rule ~lines 103–122), `CHANGELOG.md` (Unreleased), `tests/cli_mod_line_budget.rs` (exact count)
- Test: `make check` + `make test`

- [ ] **Step 1: cli.md — guide contract (new subsection under `## Views`)**

Content (English, matching the file): entry conditions (no subcommand + both TTYs; explicit `tui`; `--harness` preset skips step 1), the 4-step flow with defaults, table-only output, command echo before/after as the reproducible artifact, non-TTY behavior (clap error / refusal), v0.5.4 scope (doctor/waste/failures) with pointer to the design doc's post-v0.5.5 expansion note. Reference `views/guide.rs`, not pasted code.

- [ ] **Step 2: architecture.md — the landed split**

Update the report row (~line 101): `cli/views/` now exists, holding `guide.rs` (the interactive entry) and `trend.rs` (moved). Update the view-landing rule (~lines 103–122): the "created when the next view lands" deferral is discharged — this PR created it and moved `trend.rs` in the same commit; future views add one file under `cli/views/`.

- [ ] **Step 3: CHANGELOG — Unreleased entry**

```md
## [Unreleased]

### Added

- 対話ガイド: サブコマンドなし起動（TTY のとき）・`tui`
  サブコマンドで、harness → やりたいこと（doctor / waste / failures）→
  範囲の順に選ぶだけでレポートを表示。実行前後に相当コマンド文を出す。
```

- [ ] **Step 4: Set the exact line budget**

Run: `wc -l src/cli/mod.rs` → set `MOD_RS_LINE_BUDGET` to that exact number (no slack; raising is the flagged review point, justified here by the `Tui` dispatch arm + no-args branch, with view logic kept out per the landing rule).

- [ ] **Step 5: Full green**

Run: `make check` (rustfmt + clippy, the CI gate)
Expected: clean
Run: `make test`
Expected: all suites ok, 0 failed — including `cli_mod_rs_stays_within_its_line_budget`, `every_subcommand_is_either_windowed_or_whole_history`, and the new `guide_entry` tests

- [ ] **Step 6: Commit**

```bash
git add docs/specs/cli.md docs/specs/architecture.md CHANGELOG.md tests/cli_mod_line_budget.rs && git commit -m "docs: guide contract, landed views/ split, and changelog"
```

---

## Self-review

- **Spec coverage:** design §1 (entry) → Tasks 2 (classifier) + 4 (branch/arm); §2 (flow) → Task 3 (`run_guide`); §3 (execution/echo) → Tasks 3 (`equivalent_command` use, echo lines) + 2 (pin table); §4 (tests/non-goals) → Tasks 2/3/5 + Task 6 specs; expansion declaration → Task 6 (pointer in cli.md). No orphan requirements.
- **Placeholder scan:** every step names exact files, exact code, exact commands with expected output. Two intentional runtime measurements (budget count in Task 4→6, `SessionEnv` field mirror in Task 3) are procedures with fallbacks, not TBDs.
- **Type consistency:** `Entry::Guide { harness: Option<String>, profile: Option<String> }`, `Purpose::{Doctor,Waste,Failures}`, `ScopeChoice::{All,Global,Project}`, `equivalent_command(&Harness, Purpose, ScopeChoice) -> String`, `Choice::{Pick(usize),Quit}`, `ask_choice<R: BufRead, W: Write>(...) -> anyhow::Result<Choice>`, `run_guide(&Harness, &WindowedDispatch, &mut R, &mut W)`, `run_guide_entry(Option<&str>, Option<&str>)` — used identically in Tasks 2–5.
- **Deviations from `cli-view-addition.md`, stated:** (1) no JSON golden — the guide has no machine output; the `equivalent_command` table test (Task 2) is the machine contract; (2) budget raised, not avoided — dispatch arms cannot live anywhere but `mod.rs`; view logic stays out.

## Execution handoff

**Plan complete and saved to `docs/exploration/2026-11-06-guided-tui-plan.md`** (repo layout keeps working docs in `docs/exploration/` beside the design doc, instead of the skill-default `docs/superpowers/plans/`).

**Two execution options:**

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration (repo AGENTS.md mandates this for implementation).

**2. Inline execution** — execute tasks here in this session, batch with checkpoints.

**Which approach?**
