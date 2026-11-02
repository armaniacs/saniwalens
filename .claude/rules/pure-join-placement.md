---
paths:
  - "src/store.rs"
  - "src/core/**/*.rs"
  - "docs/specs/architecture.md"
  - "pbi/**/*.md"
---

# Pure Join Placement ― Row Shaping over Fetched Rows Belongs in Core

A function that only shapes rows **already fetched from the store** ― a
pairing, an ordering, or a delta over in-memory values, with no SQLite in it ―
is analysis logic, not storage. It belongs in **core** (`src/core/`), wherever
it is called from today and however few its callers are. This clause was
missing when the 1st pattern formed: the trend view's two-window joins landed
in `src/store.rs`, their tests were absorbed there, and `cli/trend.rs` ended
up importing analysis types from the store's namespace ― because nothing in
`docs/specs/architecture.md` or the spec-sync map decided the attribution.
The house style is the opposite: `core/findings.rs` takes a store-free
`FindingsInput`, so the analysis logic stays unit-testable without a database.

## Rules

- A new pure join is **born in core**, and its unit tests live with it there.
  For a pure join, "next to the code" (`.claude/rules/tdd.md`) means next to
  the core module it belongs to, not next to the store file whose query
  fetched the rows.
- The store keeps the query and the row hand-off. Queries select and filter;
  they never shape rows beyond handing them to the join.
- The one store-resident exception is the integration test pinning the
  query→join window split
  (`two_window_aggregation_splits_events_between_the_windows`): which window a
  boundary event lands in is storage behavior, not join logic.
- **The 2nd SQLite-free function that appears in `src/store.rs` is the
  trigger**: the PBI that lands it also moves the accumulated joins to core in
  the same PR. The 1st pattern's trigger 1 fired at PBI 46 (`1ecda2a` /
  `3b4349f`) but the move did not ride that landing; PBI 61 (2026-11-02) is the
  follow-up remediation, not a co-landing. That miss is recorded in the ledger,
  and trigger 2 remains armed for the next accumulation. Landing a join without
  the contract updates below, or parking the move behind a separate cleanup,
  both lose the trigger; the tripwire exists so neither can happen quietly.
- The four points that accompany a move: this rule's enumeration below, the
  core row of the layer table (`docs/specs/architecture.md`), the ledger's
  deferral note (`pbi/2026-10-30-00-backlog.md`), and
  `tests/pure_join_placement.rs` itself, whose move-side test now asserts that
  the definitions sit in core and no longer in the store. Emptying the
  constant, dropping an entry, or satisfying it with a tombstone comment is not
  a landing: an empty array makes both sides of the tripwire pass vacuously,
  and `contains` reads comments as text. Retiring the file instead is allowed
  only by rewriting the two prose anchors that name it (this rule and the
  architecture paragraph) in the same PR.

## The trigger, judged: a grep-able contract, not a parser

The trigger must be findable by grep, so the contract enumerates its symbols:

- The 1st pattern, named: `skill_deltas`, `TwoWindow`, `SkillDelta`,
  `TrendWindowAggregates`, `kind_pairs` ― all in `src/core/trend.rs` now,
  reached from `cli/trend.rs`; `kind_pairs` was the fifth join, left in the
  view until the move enumerated it.
- `grep -n "skill_deltas\|TrendWindowAggregates" src/core/trend.rs` finds the
  1st pattern, and this rule's `paths:` frontmatter loads the file when
  `store.rs` or core is edited, so the write side sees the demand too.
- `tests/pure_join_placement.rs` is the light tripwire: it fails when this
  rule stops naming the symbols, or when the symbols' definitions leave core
  or re-enter `src/store.rs` without the contract updates.

Judged over the alternative: a tripwire that classifies a function as
"SQLite-free" by parsing `store.rs` needs a Rust AST to be sound, and a
heuristic (a body that never mentions `sql`) false-fires on query helpers. The
enumeration plus the tripwire covers the review side; the `paths:` frontmatter
covers the write side.
