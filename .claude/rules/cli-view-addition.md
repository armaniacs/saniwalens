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
- **The trend view has moved into `src/cli/views/`** (landed as groundwork
  ahead of the first view, so the move did not wait for it).
- **No view functions are appended to `cli/mod.rs`**: a helper a view needs is
  born in the view's file; the shell keeps dispatch. Growing `cli/mod.rs` past
  its budget fails the tripwire, and raising that budget is a review point in
  the same PR as the spec update it forces.
- **The view's `--help` capture is added in the same PR** ― one row in the
  `PINNED` table in `tests/cli_help_golden.rs`. No windowed-view list is
  maintained anywhere: the set is derived from clap's subcommand tree (a
  subcommand carrying the shared `--last`), and that test pins the derived set
  against `PINNED`. A view with no capture fails with `missing from the hand
  list`; a rename's leftover row fails with `stale`. The conflict and period
  pins derive the same set, so a view is covered the day it lands ― and a
  command that carries no `--last` and is not whole-history fails the
  partition pin the same test holds.
- **The specs are updated in the same PR** ― both places the split touches in
  `docs/specs/architecture.md` (the report row of the layer table and the
  view-landing rule), plus `docs/specs/cli.md` for the view's command
  contract.
- **A golden test pins the view's JSON machine contract** (`docs/specs/cli.md`
  calls the JSON output "the machine contract"). The first one exists (PBI 39
  pins trend's emitted JSON in `tests/trend_json_golden.rs`), and every later
  view PBI adds its own ― a move without a golden is validated by capture
  alone, which is not the contract.
