//! Tripwire for the pure-join placement rule (PBI 41; the move landed in PBI
//! 61).
//!
//! A function that only shapes rows already fetched from the store belongs in
//! core (`.claude/rules/pure-join-placement.md`; `docs/specs/architecture.md`,
//! the pure-join placement rule). The 1st pattern ― the trend view's two-window
//! joins ― predated the rule and sat in `src/store.rs` until PBI 61 moved it to
//! `src/core/trend.rs`. This tripwire keeps both halves visible to
//! `cargo test`: the rule must keep naming the trigger symbols (the grep-able
//! contract), and the definitions it names must sit in core and no longer in
//! the store. Two external anchors pin the dependency direction the move
//! bought: core never reaches into the store, and the trend view no longer
//! imports analysis types from the store's namespace.
//!
//! The test lives in `tests/`, not in `store.rs`'s own test module, so the
//! tripwire does not grow the very file it guards (the same reason
//! `tests/cli_mod_line_budget.rs` stays out of the shell).

use std::path::Path;

/// The rule file the trigger contract lives in.
const RULE: &str = ".claude/rules/pure-join-placement.md";
/// Where the 1st pattern sits now.
const CORE_TREND: &str = "src/core/trend.rs";
/// The file the placement rule keeps free of row shaping.
const STORE: &str = "src/store.rs";
/// The view that used to import its analysis types from the store.
const CLI_TREND: &str = "src/cli/trend.rs";

/// The 1st pattern the rule enumerates: the trend view's two-window joins, as
/// (symbol, definition marker) pairs. It is five entries, not four: moving the
/// store-resident joins exposed `kind_pairs` as the fifth, previously
/// unenumerated one. Pinning the definition marker, not the bare symbol, keeps
/// the tripwire honest when a caller keeps mentioning a moved join. The
/// function markers carry their opening paren because a bare
/// `pub fn kind_pairs` also matches `pub fn kind_pairs_rows`, which would let a
/// rename satisfy the tripwire while the rule still claimed the old name.
const FIRST_PATTERN_JOINS: [(&str, &str); 5] = [
    ("skill_deltas", "pub fn skill_deltas("),
    ("TwoWindow", "pub struct TwoWindow"),
    ("SkillDelta", "pub struct SkillDelta"),
    ("TrendWindowAggregates", "pub struct TrendWindowAggregates"),
    ("kind_pairs", "pub fn kind_pairs("),
];

/// The remedy half of a failure message, pinned here so its wording cannot
/// drift away from the four points it prescribes.
const MOVE_REMEDY: &str = "In the same PR: update the rule's enumeration \
     (.claude/rules/pure-join-placement.md), the architecture core row \
     (docs/specs/architecture.md), and the ledger's deferral note \
     (pbi/2026-10-30-00-backlog.md); then keep this tripwire's move side in \
     tests/pure_join_placement.rs asserting \"the definitions sit in \
     src/core/trend.rs and no longer in the store\", keeping every entry. Do \
     not empty the constant, drop an entry, or satisfy it with a tombstone \
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

/// Collapse every whitespace run to one space, so a prose pin reads the same
/// across the spec's line wrapping.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The full move failure message, assembled in one place so the pin below can
/// assert it without tripping the tripwire itself.
fn move_message(definition: &str) -> String {
    format!(
        "`{definition}` is not in {CORE_TREND}, or is back in {STORE}: the pure join moved to \
         core in PBI 61. {MOVE_REMEDY}"
    )
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
    // judged trigger): the rule must name where the 1st pattern sits now,
    // enumerate its symbols, and state the same-PR move obligation. Lose any
    // of those and the trigger is no longer findable by grep. The old
    // "src/store.rs" assertion would stay true after the move only because the
    // rule's trigger paragraph still names the store, so it now pins the
    // destination instead.
    let rule = read(RULE);
    assert!(
        rule.contains("src/core/trend.rs") || rule.contains("`core`"),
        "the rule must name where the 1st pattern sits now (src/core/trend.rs or core): {RULE}"
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
fn the_enumerated_joins_sit_in_core_and_not_in_the_store() {
    // The move side of the tripwire, flipped by PBI 61: the definitions the
    // rule names must sit in core and must not come back into the store. A
    // definition that leaves core or reappears in `src/store.rs` fails here,
    // with the four-point remedy the same PR carries.
    let core = read(CORE_TREND);
    let store = read(STORE);
    for (_, definition) in FIRST_PATTERN_JOINS {
        assert!(core.contains(definition), "{}", move_message(definition));
        assert!(!store.contains(definition), "{}", move_message(definition));
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
fn core_trend_does_not_reach_into_the_store() {
    // The dependency direction the move bought: a pure join takes the rows it
    // shapes as arguments, never through the store's namespace. `store -> core`
    // is the only direction the layers allow.
    let core = read(CORE_TREND);
    assert!(
        !core.contains("crate::store"),
        "{CORE_TREND} must not import the store: a pure join takes its rows as arguments"
    );
}

#[test]
fn the_trend_view_does_not_import_analysis_types_from_the_store() {
    // The second external anchor: the view's analysis types now come from core.
    // A line that reaches into the store may not name a moved analysis type,
    // and the view must still import them from core, so a silent re-export
    // cannot satisfy this by deletion.
    let cli = read(CLI_TREND);
    for symbol in [
        "SkillDelta",
        "TwoWindow",
        "TrendWindowAggregates",
        "skill_deltas",
    ] {
        for line in cli.lines().filter(|line| line.contains("crate::store")) {
            assert!(
                !line.contains(symbol),
                "the trend view must import `{symbol}` from core, not the store: {line}"
            );
        }
    }
    assert!(
        cli.contains("crate::core::trend"),
        "the trend view must import its analysis types from core: {CLI_TREND}"
    );
}

#[test]
fn the_move_remedy_names_all_four_points() {
    // The message is pinned (PBI 51, re-pinned after the PBI 61 move): it must
    // name the three prose updates and the tripwire's own move side, forbid the
    // vacuous-pass escapes, and point at the two prose anchors if the tripwire
    // itself is retired.
    let message = move_message("pub fn skill_deltas");
    assert!(
        message.contains("`pub fn skill_deltas` is not in src/core/trend.rs"),
        "the message must name the definition that left core: {message}"
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
        message.contains("keep this tripwire's move side"),
        "point 4 must say the move side stays flipped rather than disappears: {message}"
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

#[test]
fn the_spec_no_longer_places_the_joins_in_the_store() {
    // The prose pin for the move: the architecture paragraph must stop
    // deferring the joins to the store and name where they landed. Restoring
    // the deferred wording turns this red in the same PR that would be quietly
    // undoing the move.
    let architecture = normalize(&read("docs/specs/architecture.md"));
    assert!(
        !architecture.contains("still sit in `store.rs`"),
        "docs/specs/architecture.md still defers the joins to the store: the move landed in PBI 61"
    );
    assert!(
        architecture.contains("src/core/trend.rs"),
        "the architecture's pure-join paragraph must name where the joins landed"
    );
}

#[test]
fn the_spec_sync_map_does_not_pin_the_join_location() {
    // The spec-sync table names the owning spec, not the current file
    // location. The location belongs to the placement rule and architecture;
    // a row that says "still in src/store.rs" goes stale the moment the move
    // lands and is a fifth, unowned copy of the same fact.
    let spec_sync = read(".claude/rules/spec-sync.md");
    let row = spec_sync
        .lines()
        .find(|line| line.contains("Pure shaping of fetched rows"))
        .expect("spec-sync must keep the pure-shaping row");
    assert!(
        row.contains("docs/specs/cli.md"),
        "the pure-shaping row must name its owning spec: {row}"
    );
    assert!(
        row.contains("pure-join-placement"),
        "the pure-shaping row must point at the placement rule: {row}"
    );
    assert!(
        !row.contains("src/store.rs") && !row.contains("still sit"),
        "the pure-shaping row must not pin the current join location: {row}"
    );
}
