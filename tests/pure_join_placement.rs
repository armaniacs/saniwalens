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
//! core row, the ledger's deferral note, and this tripwire's own move side:
//! the store-side assertion flips to "core holds them and the store does not",
//! and the constant keeps every entry. Emptying the constant, dropping an
//! entry, or satisfying it with a tombstone comment is not a landing; retiring
//! the file instead rewrites the two prose anchors that name it.
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

/// The remedy half of the move tripwire's failure message, pinned here so its
/// wording cannot drift away from the four points it prescribes.
const MOVE_REMEDY: &str = "In the same PR: update the rule's enumeration \
     (.claude/rules/pure-join-placement.md), the architecture core row \
     (docs/specs/architecture.md), and the ledger's deferral note \
     (pbi/2026-10-30-00-backlog.md); then flip this tripwire's move side in \
     tests/pure_join_placement.rs from \"the definitions sit in src/store.rs\" \
     to \"they sit in core and no longer in the store\", keeping every entry. \
     Do not empty the constant, drop an entry, or satisfy it with a tombstone \
     comment: an empty array makes both sides pass vacuously, and `contains` \
     reads comments as text. Retiring the tripwire instead rewrites both prose \
     anchors that name it (the rule and the architecture paragraph) in the same \
     PR.";

/// Read a repo file relative to the manifest root, failing loud: a missing
/// file is a broken tripwire, not a passing one.
fn read(path: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("cannot read {}: {e}", path))
}

/// The full move failure message, assembled in one place so the pin below can
/// assert it without tripping the tripwire itself.
fn move_message(definition: &str) -> String {
    format!("`{definition}` left src/store.rs: the trigger-riding refactor landed. {MOVE_REMEDY}")
}

/// The symbols the rule names as the 1st pattern, parsed from its enumeration
/// bullet rather than from the definition markers: dropping one entry on
/// either side must fail the set comparison below. The bullet is cut at the
/// next bullet or paragraph boundary, so the follow-on contract bullets cannot
/// leak identifiers into the set.
fn rule_named_symbols(rule: &str) -> std::collections::BTreeSet<&str> {
    const MARKER: &str = "- The 1st pattern, named:";
    let start = rule
        .find(MARKER)
        .unwrap_or_else(|| panic!("the rule must carry the 1st pattern's enumeration"))
        + MARKER.len();
    let bullet = rule[start..]
        .split("\n- ")
        .next()
        .expect("the enumeration bullet is never empty")
        .split("\n\n")
        .next()
        .expect("the enumeration bullet is never empty");
    let mut symbols = std::collections::BTreeSet::new();
    let mut rest = bullet;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let close = after
            .find('`')
            .unwrap_or_else(|| panic!("unbalanced backtick in the rule's enumeration"));
        let token = &after[..close];
        if let Some(first) = token.chars().next() {
            let is_identifier = (first.is_ascii_alphabetic() || first == '_')
                && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if is_identifier {
                symbols.insert(token);
            }
        }
        rest = &after[close + 1..];
    }
    symbols
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
    // (the trigger-riding refactor) fails here, with the four-point remedy the
    // same PR carries (PBI 51).
    let store = read("src/store.rs");
    for (_, definition) in FIRST_PATTERN_JOINS {
        assert!(store.contains(definition), "{}", move_message(definition));
    }
}

#[test]
fn the_trigger_constant_is_not_empty() {
    // An emptied constant would make both sides of this tripwire pass without
    // checking anything, so the empty array is the one state the contract
    // forbids outright.
    assert!(
        !FIRST_PATTERN_JOINS.is_empty(),
        "FIRST_PATTERN_JOINS must keep every entry: an empty array makes both \
         sides of this tripwire pass vacuously"
    );
}

#[test]
fn the_enumerated_set_matches_the_rule() {
    // One-way `contains` lets an entry leave the constant while the rule keeps
    // naming it (or the reverse); compare the sets so a partial deletion fails
    // on either side.
    let rule = read(RULE);
    let named_by_rule = rule_named_symbols(&rule);
    let pinned: std::collections::BTreeSet<&str> = FIRST_PATTERN_JOINS
        .iter()
        .map(|(symbol, _)| *symbol)
        .collect();
    assert_eq!(
        named_by_rule, pinned,
        "the rule's enumeration and FIRST_PATTERN_JOINS must name the same \
         symbols: {RULE}"
    );
}

#[test]
fn the_move_remedy_names_all_four_points() {
    // The message is pinned (PBI 51): it must name the three prose updates and
    // the tripwire's own move-side flip, forbid the vacuous-pass escapes, and
    // point at the two prose anchors if the tripwire itself is retired.
    let message = move_message("pub fn skill_deltas");
    assert!(
        message.contains("`pub fn skill_deltas` left src/store.rs"),
        "the message must name the definition that left the store: {message}"
    );
    assert!(
        message.contains("the rule's enumeration"),
        "point 1: the rule's enumeration: {message}"
    );
    assert!(
        message.contains("the architecture core row"),
        "point 2: the architecture core row: {message}"
    );
    assert!(
        message.contains("the ledger's deferral note"),
        "point 3: the ledger's deferral note: {message}"
    );
    assert!(
        message.contains("tests/pure_join_placement.rs"),
        "point 4: the tripwire itself: {message}"
    );
    assert!(
        message.contains("flip this tripwire's move side"),
        "point 4 must say the move side flips rather than disappears: {message}"
    );
    assert!(
        message.contains("Do not empty the constant")
            && message.contains("drop an entry")
            && message.contains("tombstone comment"),
        "point 4 must forbid emptying, dropping, and tombstone comments: {message}"
    );
    assert!(
        message.contains("both prose anchors"),
        "point 4 must name the two prose anchors required for retirement: {message}"
    );
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
