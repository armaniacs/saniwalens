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
  the same PR. The move also rides the next trend metric/view landing PR ―
  whichever fires first (the deferral note in
  `pbi/2026-10-30-00-backlog.md`). Landing the move without the contract
  updates below, or parking the move behind a separate cleanup, both lose the
  trigger; the tripwire exists so neither can happen quietly.
- When the move lands, the same PR updates this rule's enumeration below, the
  core row of the layer table (`docs/specs/architecture.md`), and the ledger's
  deferral note.

## The trigger, judged: a grep-able contract, not a parser

The trigger must be findable by grep, so the contract enumerates its symbols:

- The 1st pattern, named: `skill_deltas`, `TwoWindow`, `SkillDelta`,
  `TrendWindowAggregates` ― all in `src/store.rs` today, reached from
  `cli/trend.rs`.
- `grep -n "skill_deltas\|TrendWindowAggregates" src/store.rs` finds the 1st
  pattern, and this rule's `paths:` frontmatter loads the file when `store.rs`
  or core is edited, so the write side sees the demand too.
- `tests/pure_join_placement.rs` is the light tripwire: it fails when this
  rule stops naming the symbols, or when the symbols' definitions leave
  `src/store.rs` before the move lands with its contract updates.

Judged over the alternative: a tripwire that classifies a function as
"SQLite-free" by parsing `store.rs` needs a Rust AST to be sound, and a
heuristic (a body that never mentions `sql`) false-fires on query helpers. The
enumeration plus the tripwire covers the review side; the `paths:` frontmatter
covers the write side.
