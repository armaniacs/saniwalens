---
paths:
  - "src/cli/**/*.rs"
  - "pbi/**/*.md"
---

# CLI View Addition ― The Acceptance Criteria Every View PBI Must Carry

`src/cli/mod.rs` is the shell: dispatch and the public CLI API, not views. A
new view lands as one file under `src/cli/views/`, created when the first view
needs it (`docs/specs/architecture.md`, the view-landing rule). That split was
deferred (PBI 32, "wait for the next view"), and the deferral is enforced by a
line-budget tripwire (`tests/cli_mod_line_budget.rs`) rather than by memory ―
so a view-addition PBI must carry the acceptance criteria below, or the next
view quietly re-grows the shell instead.

## View-addition PBI acceptance criteria template

Every PBI that adds a `Command` variant (a view) states, in its acceptance
criteria:

- **`src/cli/views/` is created in the same PR** (or the view file is added
  under it, once the directory exists) ― the view never lands inside
  `cli/mod.rs`, and it lands as one file there.
- **`src/cli/trend.rs` moves into `src/cli/views/` in the same commit** as the
  first view landing there. Its move cost does not depend on the timing, so
  the move is not a second, forgettable step.
- **No view functions are appended to `cli/mod.rs`**: a helper a view needs is
  born in the view's file; the shell keeps dispatch. Growing `cli/mod.rs` past
  its budget fails the tripwire, and raising that budget is a review point in
  the same PR as the spec update it forces.
- **The specs are updated in the same PR** ― both places the split touches in
  `docs/specs/architecture.md` (the report row of the layer table and the
  view-landing rule), plus `docs/specs/cli.md` for the view's command
  contract.
- **A golden test pins the view's JSON machine contract** (`docs/specs/cli.md`
  calls the JSON output "the machine contract"). The first one exists (PBI 39
  pins trend's emitted JSON in `tests/trend_json_golden.rs`), and every later
  view PBI adds its own ― a move without a golden is validated by capture
  alone, which is not the contract.
