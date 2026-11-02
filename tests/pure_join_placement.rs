//! Tripwire for the pure-join placement rule (PBI 41).
//!
//! A function that only shapes rows already fetched from the store belongs in
//! core (`.claude/rules/pure-join-placement.md`; `docs/specs/architecture.md`,
//! the pure-join placement rule). The 1st pattern ― the trend view's
//! two-window joins ― predates the rule and still lives in `src/store.rs`; its
//! move rides the next trend metric/view landing PR. This tripwire keeps both
//! halves visible to `cargo test`: the rule must keep naming the trigger
//! symbols (the grep-able contract), and the definitions it names must still
//! sit in `src/store.rs` until the move lands. When the move happens, this
//! file fails and the same PR updates the rule's enumeration, the architecture
//! core row, and the ledger's deferral note.
//!
//! The test lives in `tests/`, not in `store.rs`'s own test module, so the
//! tripwire does not grow the very file it guards (the same reason
//! `tests/cli_mod_line_budget.rs` stays out of the shell).

use std::path::Path;

/// The rule file the trigger contract lives in.
const RULE: &str = ".claude/rules/pure-join-placement.md";

/// The 1st pattern the rule enumerates: the trend view's two-window joins,
/// as (symbol, definition marker) pairs. The store holds the definitions
/// today; core holds them after the move. Pinning the definition marker, not
/// the bare symbol, keeps the tripwire honest when the store's own
/// integration test keeps touching a moved join.
const FIRST_PATTERN_JOINS: [(&str, &str); 4] = [
    ("skill_deltas", "pub fn skill_deltas"),
    ("TwoWindow", "pub struct TwoWindow"),
    ("SkillDelta", "pub struct SkillDelta"),
    ("TrendWindowAggregates", "pub struct TrendWindowAggregates"),
];

/// Read a repo file relative to the manifest root, failing loud: a missing
/// file is a broken tripwire, not a passing one.
fn read(path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("cannot read {}: {e}", path))
}

#[test]
fn the_rule_names_the_trigger_symbols() {
    // The grep-able contract (`.claude/rules/pure-join-placement.md`, the
    // judged trigger): the rule must name the file the 1st pattern sits in,
    // enumerate its symbols, and state the same-PR move obligation. Lose any
    // of those and the trigger is no longer findable by grep.
    let rule = read(RULE);
    assert!(
        rule.contains("src/store.rs"),
        "the rule must name src/store.rs, where the 1st pattern sits: {RULE}"
    );
    assert!(
        rule.contains("same PR"),
        "the rule must state the 2nd-symbol move obligation in the same PR: {RULE}"
    );
    for (symbol, _) in FIRST_PATTERN_JOINS {
        assert!(
            rule.contains(symbol),
            "the rule must enumerate `{symbol}` or the trigger is not grep-able: {RULE}"
        );
    }
}

#[test]
fn the_enumerated_joins_still_sit_in_store_rs() {
    // The move side of the tripwire: while the deferral holds, the definitions
    // the rule names must still be in src/store.rs. The first one that leaves
    // (the trigger-riding refactor) fails here, and the same PR updates the
    // rule's enumeration, the architecture core row, and the ledger.
    let store = read("src/store.rs");
    for (_, definition) in FIRST_PATTERN_JOINS {
        assert!(
            store.contains(definition),
            "`{definition}` left src/store.rs: the trigger-riding refactor landed ― \
             update the rule's enumeration ({RULE}), the architecture core row, \
             and the ledger's deferral note in the same PR"
        );
    }
}

#[test]
fn the_spec_carries_the_placement_rule() {
    // The spec half of the contract: architecture.md's core row owns the
    // placement rule and the trend joins, so the rule and the spec cannot
    // drift apart without a test failing.
    let architecture = read("docs/specs/architecture.md");
    assert!(
        architecture.contains("pure-join-placement"),
        "docs/specs/architecture.md must reference the placement rule"
    );
    assert!(
        architecture.contains("two-window joins"),
        "the architecture core row must carry the trend view's two-window joins"
    );
}
