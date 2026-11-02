//! Line-budget tripwire for `src/cli/mod.rs`.
//!
//! The shell must not re-grow a shared file: a new view lands as one file under
//! `src/cli/views/` instead (`docs/specs/architecture.md`, the view-landing
//! rule). Growing `mod.rs` past its measured size fails here with a message
//! pointing at `cli/views/`, so the "wait for the next view" decision is
//! enforced by structure, not prose.
//!
//! The test lives in `tests/`, not in `mod.rs`'s own test module, so the
//! tripwire does not grow the very file it guards.

/// The measured size of `src/cli/mod.rs` at the latest budget change (the
/// PBI 42 measurement of 2026-11-01: 3,100 lines, after the headless
/// heartbeat's quiet-stretch logic moved into `cli/heartbeat.rs`; the PBI 40
/// measurement was 3,110, the PBI 38 measurement 3,322 and the PBI 34
/// landing measured 3,383), with no slack — raising this constant is a
/// review point, done in the same PR as the spec update it forces
/// (`docs/specs/architecture.md`).
const MOD_RS_LINE_BUDGET: usize = 3100;

/// The remedy half of the tripwire's failure message, pinned here so its
/// wording cannot drift away from what it prescribes.
const OVER_BUDGET_REMEDY: &str = "Do not grow the shell: add the view as one \
     file under `src/cli/views/` (docs/specs/architecture.md, the view-landing \
     rule; move `cli/trend.rs` with it in the same commit).";

/// The full failure message, assembled in one place so the pin below can
/// assert it without tripping the tripwire itself.
fn over_budget_message(lines: usize, budget: usize) -> String {
    format!("src/cli/mod.rs is {lines} lines, over the {budget}-line budget. {OVER_BUDGET_REMEDY}")
}

#[test]
fn cli_mod_rs_line_count_matches_wc_l() {
    let source = include_str!("../src/cli/mod.rs");
    // The budget is a newline count — the unit `wc -l src/cli/mod.rs` reports.
    // The file ends with a newline, so the string's line count agrees with it;
    // this pins the two counting methods to the same number.
    assert_eq!(source.matches('\n').count(), source.lines().count());
}

#[test]
fn cli_mod_rs_stays_within_its_line_budget() {
    let source = include_str!("../src/cli/mod.rs");
    let lines = source.lines().count();
    assert!(
        lines <= MOD_RS_LINE_BUDGET,
        "{}",
        over_budget_message(lines, MOD_RS_LINE_BUDGET)
    );
}

#[test]
fn the_over_budget_message_points_at_cli_views() {
    // The message is pinned (PBI 34: the wording is unit-tested, not left to
    // the failure path) — it must name the guarded file, the constraint, and
    // the remedy, which is `src/cli/views/` and never an addition to the shell.
    let message = over_budget_message(3400, MOD_RS_LINE_BUDGET);
    assert!(
        message.contains("src/cli/mod.rs"),
        "the message must name the file it guards: {message}"
    );
    assert!(
        message.contains("line budget"),
        "the message must name the constraint it enforces: {message}"
    );
    assert!(
        message.contains("`src/cli/views/`"),
        "the message must point at src/cli/views/ as the remedy: {message}"
    );
    assert!(
        message.contains("`cli/trend.rs`"),
        "the message must name the same-commit trend move: {message}"
    );
}
