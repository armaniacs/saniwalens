# CLI Specification

The CLI is the user's surface onto the stages (`architecture.md`): `analyze`
populates the store, the **views** and `sql` read it, and `optimize` hands the
findings to the harness's advisor session. This spec defines the command
contract and the reporting model; exact flag spellings live with the
implementation and stay in sync via `.claude/rules/spec-sync.md`.

## `analyze` ― populate the store

```
saniwalens analyze [--projects <dir>] [--config <dir>] [--db <path>]
```

Reads both Claude Code inputs, session transcripts (default
`~/.claude/projects/`) and live config (default `~/.claude/` plus the relevant
project `.claude/`), and writes the SQLite store (`storage.md`). That
`~/.claude` root is `$CLAUDE_CONFIG_DIR` whenever the variable names a non-empty
path (an empty value carries no location and is treated as unset), so
saniwalens follows a relocated config tree instead of reporting on an empty
default. Incremental and idempotent: unchanged transcripts are skipped, changed
ones replaced, the surface catalog rebuilt from current config. The verb is
`analyze`, not `build`: it analyzes raw input into facts.

`analyze` reads everything **read-only** and never copies input into the repo or
the store beyond the derived facts (`.claude/rules/session-data-privacy.md`).

### `--db` ― one store per user, not per directory

Shared by every command. `analyze` walks *every* project's transcripts under
`~/.claude/projects`, so a cwd-relative default would contradict the data it
holds: it would build a separate copy of the same cross-project store per
directory and leave database files inside unrelated Git working trees. The
default is therefore user-level,
`${XDG_STATE_HOME:-~/.local/state}/saniwalens/saniwalens.db`. `Session::open` in
`session.rs` owns resolution, the legacy-store move, and the guarded open as
one seam; `Store::open` creates the directory on demand, owner-only like the
store itself (`storage.md`). A non-absolute `XDG_STATE_HOME` is invalid per the
XDG spec and ignored, since honoring it would reintroduce the cwd-relative
store.

It is state, not cache (`$XDG_CACHE_HOME`), because the store is not
regenerable: Claude Code prunes transcripts on its own schedule, after which the
extracted rows are the only surviving record of those sessions (`storage.md`).
A cleaner may empty a cache without asking, which this file cannot survive; for
the same reason the store migrates in place on a schema change instead of being
discarded. A project-local store is available on request: `--db ./saniwalens.db`.

### `config.toml` ― defaults and named profiles

The tool's own config lives at
`${XDG_CONFIG_HOME:-~/.config}/saniwalens/config.toml` (`config_file_path` in
`session.rs`; a relative `XDG_CONFIG_HOME` is invalid and ignored, like
`XDG_STATE_HOME` above). It is optional: without it every invocation behaves
exactly as before.

```toml
default_harness = "kilocode"   # claude | opencode | kilocode
language = "ja"                # en (the default) | ja

[profiles.work]
harness = "claude"
db = "work.db"                 # relative to the config file's directory
```

It holds top-level defaults plus named profiles that `--profile` selects.

- **Resolution order**, strongest first: explicit flags, the selected profile,
  the file's top-level defaults, the builtins (`claude`, the per-harness store
  path).
- **`language`** follows the same order without the profile layer (profiles
  never carry it, so selecting one never blocks the file default):
  `--language`, the file's `language`, the locale fallback, English. The locale
  fallback reads `LC_ALL`, then `LC_MESSAGES`, then `LANG` (POSIX order; empty
  counts as unset), then the head of the `LANGUAGE` list (`ja_JP:ja` votes
  `ja_JP`). It yields Japanese only when the winning value's leading subtag is
  `ja` (`ja`, `ja_JP`, `ja_JP.UTF-8`). A bare `C` or `POSIX` winner suppresses
  `LANGUAGE` entirely (gettext's C priority; `C.UTF-8` is not bare). Anything
  else, including empty, keeps English, byte-identical to before.
- **Paths.** A relative `db` joins the directory holding `config.toml`, never
  the working directory. An explicit `--db` is taken as is, even when relative;
  `~` is not expanded.
- **Errors.** Unknown keys are ignored so a newer file never breaks an older
  binary. A missing file is silence. Anything else wrong (unparsable TOML,
  unknown harness, missing profile, unknown language) fails the invocation with
  the file named; an unknown `--profile` lists the available ones and an
  unknown `language` lists `en | ja`.
- **Flags before store.** Flag values (`--scope`, `--format`) are parsed before
  the session is built, so an invalid flag fails on its own, with no store
  opened and no freshness line ahead of the error. The windowed dispatch in
  `cli/window.rs` fixes the order per view family: format, period, then store
  for views without `--scope`; scope first for `inventory`, `waste`,
  `failures`, and `doctor` (whose `--format` is `table | json`, see Output
  formats); format, pair, then store for `trend`. Views that never filter get a
  read with no filter to ignore; only the scoped entries return the filter
  `--scope` selected. `optimize` is the exception: it resolves the store path
  first.

## Views ― read the store

```
saniwalens <view> [--scope global|project[:<slug>]] [--by <bucket>] [--frozen]
              [--format table|markdown|json] [--db <path>]
```

Each view is its own top-level command; their queries only read the store, never
raw input. The set is deliberately curated ― a view earns a command by carrying
logic a one-line query cannot (a classification, an algorithm, a suggestion). Any
*other* slice is a `sql` query (below), so thin "just count a column" views are
not commands.

### Freshness ― reads auto-refresh

A stale store the user cannot notice was a real failure mode, so a read command
**runs the incremental `analyze` stage first by default** ― the same composition
`optimize` always had (`architecture.md`). With `ingested_files` skipping
unchanged transcripts (`storage.md`), an up-to-date store costs one stat per
transcript. The refresh reuses the roots recorded in `meta` (`projects_dir`),
and a missing db is simply created ― "absent db" is an error only for `sql`.
`--frozen` suppresses the refresh and reads the store as-is.

Every read prints a **one-line freshness header on stderr** (stdout stays clean
for pipes): what was refreshed, or ― under `--frozen` / `sql` ― how old the
store is, with a re-analyze hint once it is older than a day.

### Scope ― route each finding to the config layer that owns the fix

"Optimize my global setup" and "optimize this project" are different tasks, and
mixing them lets one busy project drown the global picture. Every finding is
routed (`core::scope`):

- **Config wedges** route by the surface's own scope column (`storage.md`).
- **Friction categories** route by concentration: a strict-majority project owns
  the category (fix in that project's config); a spread category is global (a
  cross-project habit).
- **Thrash** routes to the project it happened in ― carried on the episode
  itself, since an episode is scoped to one session and therefore to one project
  (`events.md`); **cd% / prompting / token totals** are behavioral, hence global.

`--scope` narrows a report to one layer: `global`, `project` (all projects), or
`project:<slug>` (one). `doctor`, the optimize briefing, and (by default) the
scoped views show **both layers split into sections** rather than a merged
ranking.

| View | Answers |
| --- | --- |
| `doctor` | The entry point: a one-screen health check **written for someone who has never seen saniwalens's vocabulary** ― every item says what happened, why it costs, and what to do. Sections: `WHAT TO FIX FIRST` (recurring problems as a prioritized to-do list, each routed to the owning config layer with the follow-up command), `COST` (the session-start context and output totals in plain words, humanized units, naming the agent types behind the subagent figure ― and, when the named rows cover only part of that figure, saying so first; see *Reporting honesty*), `CONFIG WORTH PRUNING` (dead or heavy config per owner), `LOOKS HEALTHY` (what explicitly needs no action). `--scope` narrows to one layer; the text form leads with actions, never a stats dump. |
| `inventory` | The catalog×usage join per surface **row** (scope and owning project shown; usage attributed under per-project shadowing ― `surfaces.md`): startup cost, static cost, load mode, usage. `--scope` filters to one layer; orphaned usage (no scope to filter on) appears in the unfiltered view only. |
| `usage` | Skill event rollups: per skill, or per time bucket (`--by`): frequency, tokens, `ctx_growth`, duration. Leads with a token-destination line (main-thread skill output vs subagent total), then splits the subagent half **per agent type** (runs and output tokens, costliest first), because the aggregate is usually the larger number and the split is the actionable part. Split coverage is reported per *Reporting honesty*. A section with no rows is omitted rather than rendered as bare headers. |
| `models` | Where the tokens went, by model: main-thread usage events and subagent runs grouped per model, costliest first. `tool_error` rows are excluded (their `model` is the originating tool, not a model); missing models bundle as `unknown` (bundled, never dropped). A `note` calls out a model holding four fifths or more of the window. The curated logic beyond one `sql` line: the unknown bundling, the tool exclusion, the deterministic order, and the concentration note. |
| `trend` | How usage moved: each event kind (count / output tokens / duration), each skill (invocations, largest movement first), each used surface (`kind`/`id`, largest movement first), and the session / subagent totals, for this window against the equal-length window right before it (default 7d vs the 7d before; `--last` or `--since` [+ `--until`] fixes the current one, see Period). Every row carries both windows' values, the delta, and the direction; unavailable comparisons and the metrics a kind does not carry follow *Reporting honesty*. Both windows' ranges are disclosed (`window.current` / `window.previous` in JSON). A section with no rows is omitted rather than rendered as bare headers. |
| `waste` | Just the flagged opportunities (unused, costly+rare, always-on heavy, …) with their evidence and owning scope; `--scope` filters to one layer. Rows with an unknown `load_mode` label are excluded from routing and reported as a stderr warning plus JSON `unknown_load_modes {count, labels}` (see `surfaces.md`). |
| `overhead` | Reconcile the empirical always-on floor (the leanest observed **session start**, not the context a skill happened to run at ― `surfaces.md`) against the readable always-on config; the residual is the system prompt + built-in tools + MCP schemas the catalog cannot weigh. The per-project table carries the session count behind each floor, because the floor is a minimum over observations and a project with few sessions may never have caught a fresh one. Reports the metric as unavailable when no session start was observed at all. |
| `prompts` | How the user steers the session: the mix of steer / correct / question / instruct prompts (`core::prompt`, lexical heuristics), with a verdict ― heavy steering suggests more autonomy, frequent correction suggests clearer upfront specs. This is a behavioral signal, not a config metric; embeddings showed prompt *topics* do not map to reusable skills, so the value is in *how* you prompt, not *what about*. Below the mix, repeated prompt shapes (masked templates, three or more sightings) list as candidate new skills with a count, an exemplar, and their sources; rotated or missing transcripts count as pruned. |
| `failures` | Where the work stumbles: recurring tool failures by category (`core::friction` ― edit-precondition, path-not-found, blocked-by-hook, …), ranked, each with what it suggests fixing and the originating-tool split. This analyses the *work*, not the config ― where the real cost is. The classifier separates fixable friction from non-actionable noise (cancelled, transient). `--scope` follows the routing above: `global` = spread categories, `project` = each project's majority-owned categories, `project:<slug>` = everything in that one project (this subsumes the old `--project` flag). Denials are categorised from the transcript's own denial marker; everything else by lexical heuristics. A stored category the classifier does not know (typo, schema drift) is still listed, but warns on stderr and is counted in JSON `unknown_labels {count, labels}` instead of silently joining `Other`. |
| `stuck` | Bursts of rapid re-edits to one file by one session (`core::thrash` ― N+ edits within a few minutes), ranked. This isolates *where Claude got stuck and kept retrying* from healthy spread-out editing ― an algorithmic signal a flat edit count cannot give. Observed: a file edited 25× in under 8 minutes. The table carries the project; `--format json` adds the session id and full path, which is what makes an episode investigable. |
| `sessions` | Every session in the window: output tokens (events plus the session row's subagent total), wall-clock span covered, and tool-error count ― quiet sessions read 0, never vanish. `--sort tokens|duration|friction` (default tokens) orders largest first with the session id breaking ties, so the order never churns. |

### Output formats ― human vs machine

Every command has a human form and a machine form, strictly separated.

- **Human** (default `table`; `--format markdown` for pasting into notes or PRs;
  `doctor` and `analyze` have their own text layouts). Output must explain
  itself: every table opens with a dimmed framing block saying what the view
  shows and how to read it (table only; markdown stays a bare table), headers
  are words rather than abbreviations, and interpretation notes are dimmed
  footnotes. ANSI styling turns on only when stdout (stderr for the freshness
  line) is a terminal, `NO_COLOR` is unset, and `TERM` is not `dumb`; pipes and
  captures get plain bytes. Styling is applied after column padding so
  alignment is never skewed.
- **Machine** (`--format json`, every command): a typed JSON document alone on
  stdout. SQLite integers, reals, and NULLs stay JSON numbers and null
  (`Store::query_json`); `doctor` emits the routed findings
  (`core::optimize::Findings`, `projects` filtered by `--scope`); each view
  emits its rows as named objects. All freshness and refresh chatter goes to
  stderr, so `saniwalens <cmd> --format json | jq` needs no filtering.
  Human-only footnotes are folded into the JSON (`total`, …) or dropped, never
  mixed in.
- **`trend`** rows and JSON are two renderings of one typed payload
  (`trend_payload` in `cli/views/trend.rs`), so a metric's delta, direction, and
  reason are judged once. Float metrics compare at the display's 0.1 precision,
  so drift below it reads as `same` in both forms. `tests/trend_json_golden.rs`
  pins the JSON. Each metric carries `reason` (see *Reporting honesty*), the
  serialized `UnavailableReason` enum, and `direction == "unavailable"` exactly
  when `reason` is not null.

`doctor`'s `--format` vocabulary is `table | json` only: `markdown` (and the
`md` shorthand the other views accept) is refused at the flag stage
(`unknown --format 'markdown' (table|json)`) before the refresh, the freshness
line, or the store's creation. The other windowed views, `trend` included, take
all three.

Slices not covered here (the Bash command mix, the most-edited files, any
error-category breakdown) are a `sql` one-liner; the thin `commands` /
`hotspots` views were dropped once `sql` existed.

### Guided entry ― pick instead of typing

Bare `saniwalens` on an interactive terminal, or the explicit `tui`
subcommand, opens the guided entry instead of a report. It asks which harness,
what to look at, and which scope, then runs the equivalent report command.
Implementation: `cli/views/guide.rs`; this section states the contract.

**Entry.** No subcommand with stdin and stdout both TTYs, or explicit `tui`.
`--harness` or `--profile` is a preset that skips step 1. `--language` is also
a preset and rides into the inline next step, but selects no harness, so step 1
is still asked. Any other token (a real subcommand, `--help`, `--version`)
belongs to clap; the guide never intercepts it.

**Steps.** Each has a default (empty input keeps it; `q` quits):

1. Harness: `claude`, `opencode`, or `kilocode`, defaulting to the resolved value.
2. Purpose: `doctor`, `waste`, or `failures`.
3. Scope: all history (no `--scope`), `global`, or `project`.
4. After the report: offer to run the printed `optimize` command inline
   (default no; `q` counts as no), then the config save question below.

The harness is kept for the whole invocation: after each report the guide asks
whether to look at one more (default no) and loops back to the purpose.

Output is always the table form; the guide takes no `--format` or period flags.
It echoes the equivalent command line before running (e.g. `saniwalens
--harness kilocode waste --scope global`) and again after the report. The
reproducible artifact is that command line, not the session.

**Next step.** `optimize` is not a menu item, but after each report the guide
prints a next-step line with the matching scope (`saniwalens optimize --scope
global`). The line is harness-aware: on a headless harness it carries
`--apply`. The inline run branches on run mode. A headless harness has no
dialog partner in the seeded session, so the guide hands off (`--apply`: the
plan is saved and the harness's own interactive session opens on it); an
interactive harness already sits in a dialog, so plain `optimize` applies its
own plan. The inline run carries the resolved advisor language in the same
order as `optimize`; with no language set anywhere the prompt is byte-identical
to before. The other reports, period flags, and `sql` stay out of the guide;
candidates for expansion and their order are in the design doc's note
(`docs/exploration/2026-11-06-guided-tui-design.md`).

**Config save.** After the report the guide asks whether to save the used
harness to `config.toml`: skip, set it as `default_harness`, or store it under a
named `[profiles.<name>]` profile. The edit is line-preserving (only that line
or section changes; comments and the rest stay untouched), and the guide shows
the path and changed excerpt and asks for confirmation before writing. The write
re-reads the file first and aborts with a conflict error if it changed since the
dialog read it (last-writer-wins would silently drop the other edit). The
replacement is an atomic rename through a pid-unique sibling temp file, so a
mid-write kill leaves the old or the new config, never a truncation.

**Non-TTY.** Explicit `tui` without both TTYs is refused with an error instead
of blocking on unreadable prompts. A bare invocation without both TTYs falls
through to clap, whose missing-subcommand error is what scripts see.
`tests/guide_flow.rs` drives the real binary through piped stdio with
`SANIWALENS_TEST_FORCE_TTY=1`, which counts as a terminal for the check above;
the variable is test-only, unsupported, and must never be set by users.

### Period ― narrow a report to a window of the history

The windowed report views take `--since` / `--until` / `--last`, so "what did
the agent cost last week" is a flag, not a `sql` query. `analyze`, `sql`, and
`optimize` take none; they are whole-history by contract. Which commands are
windowed is derived from the subcommand tree (a command carrying `--last`),
never a hand-maintained list; `tests/cli_help_golden.rs` pins that partition
beside the `--help` captures.

`trend` resolves the same flags as a **pair**: the current window plus the
equal-length window right before it (the previous ends one second before the
current starts, so no event is counted twice). `--since` alone runs to now, a
bounded window unlike the single-window views' open end; `--until` alone is an
error, since the comparison window is undefined without a current start.

- **`--since DATE` / `--until DATE`**: a named-day window. Each date is a whole
  day (`YYYY-MM-DD`) in the presentation zone, **JST** (the bucketing zone);
  comparison is against the UTC epoch the store keeps. `--until` is
  **inclusive**.
- **`--last SPAN`**: a rolling window ending now (`<N>d`, `<N>h`, `<N>w`).
  Exclusive with the explicit dates (clap conflict), so "last 7 days" is never
  silently reinterpreted by a stray `--since`. The conflict is declared once on
  the shared period-flags group (`cli/window.rs`); trend's pair keeps its own
  declaration so its pair-specific help survives.
- **Calendar bound.** Every resolved instant must stay inside the calendar
  chrono can express (`NaiveDate`, roughly ±262,000 years), checked once at the
  resolve entry rather than trusted to the zone's sign. A `--last` span that
  leaves it (e.g. `--last 100000000d`) gets the invalid-span error. A named day
  whose resolved midnight or last second leaves it gets the empty-window error
  naming its flag (the day end adds 86,399 s after the zone conversion, so a
  west offset can overshoot the ceiling). In `trend` the bound also covers the
  **derived previous window**: `--last` reaches back twice, so a span whose
  current window resolves can still push the previous one out (e.g.
  `--last 50000000d`, invalid-span), and a `--since` whose derived previous
  leaves the calendar gets the empty-window error naming `--since`, because the
  input is the date and the `--last` wording would blame the wrong flag.
- **Errors are parse-time**, before any report renders or store work runs:
  non-zero exit, empty stdout. An unparseable date fails; `--since` after
  `--until` is an empty-window error rather than an empty-looking report.
  Refusing is deliberate: an unresolvable window would otherwise disclose an
  empty `since` and a store bound (`started_at >= ''`) that passes every
  session, a successful-looking report that measures nothing.
- **The window is disclosed, never silent.** A dimmed `report window:` line
  rides with the freshness line on stderr (every format); the framing block
  carries it as its last line (table only); `doctor` prints it in-band under its
  heading; `--format json` gains a `window` object (`since` / `until` as
  RFC 3339 with offset; trend's `window.current` / `window.previous`). Bounds
  never render as an empty string. Two presentation limits are unreachable in a
  real run: at the calendar's ends the rendered wall clock can sit up to one
  zone width outside it (the ceiling in JST renders as
  `+262143-01-01T08:59:59+09:00`) while the instant stays exact, and RFC 3339
  names years 0000..=9999 only, so edge-year renders do not parse back. A test
  in `src/core/period.rs` pins both byte for byte.
- Without the flags, output is byte-identical to the behavior before periods
  existed.
- **Inventory.** The catalog half is current config and is not windowed; its
  **usage half** is. An unused row under a window says `UNUSED (window)` /
  `"unused-in-window"`.
- **Unstamped sessions.** Sessions the writer could not stamp (no observable
  start, empty `started_at`) are excluded from windowed `session_stats` /
  `subagent_totals` (`store.rs`'s `<> ''` guard) rather than guessed into them;
  without the flags they still count.
- **Index.** Windowed readers that scan `events` go through `started_epoch`.
  The store declares the index that keeps this fast (`storage.md`) and creates
  it on the next read-write open; `--frozen` and `sql` (read-only) never create
  it, so a store not opened read-write since keeps scanning. Sessions-axis
  figures read `sessions.started_at`, a small table needing no index. Check the
  size with `saniwalens sql "SELECT COUNT(*) FROM events"`.

## Time bucketing

`--by` groups by `year | month | week | day | hour`. Because the store keeps UTC
(`storage.md`), bucketing converts to `--tz` first (**default JST**) so day/hour
buckets mean the user's local day/hour ― bucketing UTC would smear a local day
across two buckets.

`--tz` takes an offset, not a zone name, and its domain is minute-granular:
-86,340..=86,340 seconds in 60-second steps (`±23:59`), 2,879 offsets.
`FixedOffset::east_opt` also accepts a seconds remainder, but a bound renders
the offset with `%:z`, which rounds to the nearest minute ― a remainder would
be silently rounded in the report, and the ends of the unbounded seconds
domain (±86,399) would render `+24:00`, which is not a valid offset. The period
path does not read `--tz` yet (Period above); the domain is declared here so
wiring it cannot widen the rendering silently.

A span is assigned **whole to its start-time bucket**; its cost is not split
across boundaries. This is correct for coarse buckets (year/month) and for
counting, but **distorts fine buckets** for long spans: a `loop` span lasting
hours (observed ~10,000s) lands entirely in one hour even though its work spanned
many. So at `--by hour` / `--by day`, the report **flags long and trailing spans**
(`events.is_trailing`, and a duration threshold) rather than letting them build a
silent spike. Duration-weighted distribution is a later option; v1 assigns to the
start bucket and is honest about the limitation.

## Reporting honesty

The report never hides an approximation; it labels it (mirroring the caveats in
`events.md` / `surfaces.md`).

**Estimates and unknowns**

- `sub_tokens_estimated` rows are marked, so estimated subagent cost is not
  read as exact.
- Trailing spans (closed only by session end) are marked; their duration is a
  lower bound.
- Surfaces with unknown `static_tokens` (e.g. `mcp_tool` without an available
  schema) are marked unknown, never zero.
- A surface's startup cost and on-demand body cost are two figures, never one
  (`surfaces.md`): printing a skill's body cost alone under "what to trim"
  reads as a startup saving the deletion never delivers.
- "Unused" always carries its window, so it is not mistaken for "never
  installed": all history without period flags, `UNUSED (window)` under them
  (inventory; other views' empty states read "in the evaluation window").
- A window the input makes unresolvable is an **error**, never a silent report
  (see Period).

**Subagent split** (per-agent rows against the subagent total printed above them)

- A run whose agent type was never recorded shows as `(unknown)`, never merged
  into a named type and never dropped, so the rows keep summing to the total
  for every extractable run.
- They *cannot* sum to it when a store predating the per-agent split is read
  with `--frozen`. The report says so on stderr instead of letting an absent
  breakdown pass for a zero one (`storage.md`'s report-the-gap contract). The
  message rides with the freshness line, so piped stdout and JSON stay clean.
  Under a drawn window an empty split is usually the two time axes at work (the
  total reads the session's stamped start, the rows each run's own epoch), so
  the message names the window instead of pointing at a refresh that cannot
  fill a straddle.
- They sum to only *part* of it when sessions analyzed before run extraction
  keep their totals (`storage.md`: the store outlives its inputs) but pruned
  transcripts can never yield their rows. This is a property of the data, not a
  staleness a refresh could cure, so the rows are labeled a **partial split**
  in-band: `usage` prints covered and uncovered figures between total and
  table, `doctor` states the gap before naming any agent, the briefing does the
  same under its subagent line, and JSON carries `agent_split`
  (`core::subagent::SplitCoverage`). Under a window the gap is the straddle, so
  labels say the uncovered tokens fall outside the window. `doctor` calls its
  top rows "most of" the subagent output only when they exceed half the total.

**`trend` comparisons**

- A comparison is `n/a` / `"unavailable"` only when *that row's* previous side
  holds no baseline: no event of that kind, no invocation of that skill, or no
  session on the totals axis. It never means the baseline measured zero. A
  0-token span is a measurement, so is a session that ran no subagent, and a
  baseline that merely rounds to the displayed zero still compares.
- Count axes tie their baseline to the aggregate row's presence on purpose: a
  window with no event of a kind stays `no_baseline` rather than reading
  0 → N as a measured move.
- A kind renders only the metrics it carries (point kinds: the count alone,
  since ingest writes their tokens and duration as constant zeros;
  `events.md`). The table omits the rest, so the unavailable label never
  attaches to a metric it does not show. JSON keeps all three metrics per kind
  with a null delta and a `reason`: `null` (judged), `"no_baseline"` (previous
  side absent), or `"not_applicable"` (the kind does not carry the metric).
- The reason sentence names the row scope ("that row's previous side") so one
  sentence covers the kind, skill, and totals sections, and prints only when a
  report holds such a comparison. The empty-previous-window footnote prints when
  that window holds no event of any kind; a session alone does not suppress it,
  since totals rows may compare while kind rows are `n/a`.

Meta-skills are not nested (`events.md`): a driver and the skills it ran are
each their own span, so a child's cost lands on the child and no figure is
silently double-counted.

When **several caveats apply to one row** ― e.g. a long `loop` span that is
trailing, holds equally-split subagent tokens, and is bucketed whole into one
hour ― the row is marked **low-confidence overall**, not merely tagged three
times. Stacked approximations compound into exactly the kind of time-concentrated
spike a reader over-trusts; one combined flag says "read this number loosely".

## `sql` ― query the store directly

```
saniwalens sql [<query>] [--format table|markdown|json] [--limit <N>] [--db <path>]
```

`sql` is the one read that never auto-refreshes: it opens the store strictly
read-only, so an ad-hoc query is reproducible and can never mutate anything,
and an absent db is an error (run `analyze` first) rather than a silently
created empty one. It prints the freshness header (store age) on stderr so a
stale read is at least visible.

The query is the argument or, when omitted, **stdin**
(`echo "SELECT …" | saniwalens sql`; stdin sidesteps shell quoting). It exists
because this is a *session-analysis* tool: anyone wanting a slice the fixed
views do not cover, notably the `optimize` session chasing a root cause, should
query the analyzed store, not re-parse raw transcripts.

A `tool_errors` view (`storage.md`) names the friction columns that are
otherwise overloaded onto generic event columns (`category`, `excerpt`, `tool`,
`target`, `project`), so a friction query reads cleanly without knowing the
encoding ― e.g. `SELECT target, COUNT(*) FROM tool_errors WHERE
category='edit-precondition' GROUP BY target` answers "which files keep failing
to edit", which the error text alone cannot. `SELECT sql FROM sqlite_master`
lists the full schema.

Result rows are bounded: at most `--limit` rows materialize (default 10000,
`0` disables the bound). A truncated result warns on stderr with the opt-out,
so the "any slice" contract stays reachable without ever passing a partial
slice off as complete; the JSON payload keeps its bare-array shape. The SQL
text itself (argument or stdin) is unbounded — it is user-supplied input, not
an amplification: only fetched rows balloon memory, and those are capped.

## `optimize` ― act on the findings with an advisor session

```
saniwalens optimize [--projects <dir>] [--db <path>] [--scope global|project[:<slug>]]
                [--frozen] [--print] [--verbose] [--timeout <DURATION>] [--apply]
                [--language en|ja]
```

The AI-proposal consumer (`architecture.md`). Rather than emit static
recommendations the tool cannot safely act on, `optimize` analyzes (unless
`--frozen`), composes the findings with a prescribed advisor prompt, and
launches the harness's agent seeded with it:

- **Interactive** (`claude`): a session that applies edits on agreement.
- **Headless** (`opencode run --format json`): streams only the agent's text,
  logs raw tool output to a private file (`--verbose` restores the raw stream),
  and prints the plan without editing. The output opens with a proposal-only
  status line stating no files were changed, so a truncated read still shows it.

The private log lives for the whole run. A failure keeps it and names it in the
error; a success removes it (the raw stream may echo secrets) unless
`SANIWALENS_KEEP_RUN_LOG=1`. The child and heartbeat thread are torn down on
every exit path (`cli/headless.rs`). `--timeout <DURATION>` (e.g. `30s`, `2m`,
`500ms`) bounds a headless run: on expiry the child is killed and reaped and
the error names the raw log. It is off by default so a legitimately long run is
not cut short.

**Prompt shape.** The session **investigates autonomously to a conclusion**: it
pins down each root cause and proposes a concrete fix instead of handing the
analysis back with "which area do you want to start with?". It prioritises work
friction over config size, verifies before recommending removal (an "unused"
skill may be invoked by subagents) by inspecting rather than asking, and,
interactively, pauses for exactly one thing: approval of the concrete fix-plan
before any file is edited. This "drive to a plan, gate only on applying it"
shape is the design; if it changes, update `instructions` in `core::optimize`
and this paragraph together. `instructions` renders from the harness copy
(`HarnessCopy`), not a constant, so the advisor identity, config locations, and
raw-input pointers name the harness whose rows the briefing renders.

**Language.** `--language en|ja` (and the `language` config key, with the
locale fallback beneath) switches the advisor's output language. The
instruction is one sentence from a single source
(`AdvisorLanguage::instruction_sentence` in `core::optimize`, carried on
`HarnessCopy`); the briefing, headless, and apply instructions and the launch
pointer all route through `instructions_for`, so every advisor-facing prompt
carries it. Unset means English with no sentence, byte-identical to before;
report tables stay English either way. The guided entry passes its `--language`
preset (or the file default) to the inline run. Fixed protocol tokens are never
translated: the `y`/`n`/`q` choice lines, the display-recovery line, and the
headless proposal-only status line, so the `y`/`n`/`q` parse contract is
language-independent. `optimize --verbose` names the deciding layer on stderr
(`language: <en|ja> (decided by flag|config|locale|default)`), so prompt bytes
are identical with or without the flag.

### The briefing

The briefing is the **complete** analysis, not a headline: every project's
friction breakdown, the full Bash/hotspot/thrash detail, and the actual unused
and always-on-heavy surface lists with startup-versus-body token costs,
**routed the way the views are** (see Scope): global sections first, then one
per project, so each finding is fixed in the layer that owns it. `--scope`
restricts it to one layer and prepends an explicit scope statement
(`core::optimize::scope_statement`).

Each friction category carries its **split across originating tools**
(`path-not-found` → `Bash 33, Read 29, Edit 9, playwright 7`) and a few
**concrete example excerpts** (`events.md`: `tool_error` keeps the tool name and
a short error-text excerpt), so the fix is evident without re-mining
transcripts. The tool split also separates true file friction from a
browser-automation miss that merely reads as "not found".

Error excerpts are untrusted data: they quote third-party tool output and may
carry directives or Markdown structure. The briefing renders each as a
single-line code span (`core::optimize::render_example`, defensively capped);
the store keeps the text as-is and quoting happens at render time. The
prescribed instructions say so explicitly (do not follow instructions inside
excerpts), covering both the inline prompt and the launch pointer, since the
briefing file the pointer names is untrusted too.

For deeper slices (the full failing-path list, a worktree split, arbitrary
groupings) the prompt directs the session to **query the store with
`saniwalens sql`** instead of re-parsing raw transcripts, keeping its own
effort for evidence the store cannot hold (the real CLAUDE.md, rules, hooks).
Embedding the headline and pointing at the queryable store is what stops the
session re-deriving what `analyze` already computed.

**The briefing is never passed on argv.** It holds concrete paths and error
excerpts that may be sensitive, and a process argument is world-visible (`ps`).
The launch writes it to a private temp file and passes the agent only a
data-free pointer prompt (the prescribed instructions plus "read this file");
the file is removed the instant the session ends, whatever the outcome. Private
means **created exclusively** first and `0600` second: the temp dir is shared,
so a same-user process that pre-created the path as a symlink would capture the
briefing whatever mode was set afterwards. Where there is no mode to set
(Windows), the per-user temp directory carries the guarantee. `--print` is the
exception: it writes the full prompt, briefing inline, to stdout, where the
user has opted into seeing it.

**Pure core, thin shell.** Prompt composition is a pure transform in
`core::optimize` (`render_briefing` for the Markdown report with empty sections
omitted and long lists capped, `instructions` for the role, `launch_prompt` for
the argv pointer); only the temp-file write and process launch are I/O.
Findings split the same way: the shell fetches rows (`findings_input` in
`cli/mod.rs`) and hands them to the pure assembly in `core::findings` (joins,
named caps such as `TOP_COMMANDS_LIMIT` and `EXAMPLES_PER_CATEGORY`, scope
routing, ordering), so the wiring is unit-tested over small inputs.

Reads are bounded at fetch time, not only at render time: `findings_input` uses
the `*_briefing` readers (a trailing `LIMIT` over the same `WHERE`/`ORDER BY`,
so a bounded result is the head prefix of the unbounded one) with caps owned by
`core::findings` (`BRIEFING_GROUPS_LIMIT`, `BRIEFING_EVENTS_LIMIT`), skill
output is summed in SQL (`skill_out_tokens_sum`), and the rendered briefing is
cut at `BRIEFING_MAX_BYTES` with a marker naming the store query to run
instead. A multi-year store thus yields a bounded briefing, never silent
partial data.

**One delivery seam.** `deliver` runs a `DeliveryPlan` whose mode
(`DeliveryMode::Print | Interactive | Headless`, chosen by `--print` over the
harness's launch shape) picks the payload: `Print` writes the prompt to stdout;
launch modes write the briefing to the private pointer file, hand the spawn to
the `AdvisorLaunch` adapter, and remove the file when the run ends. Production
wires `ProcessAdvisor` (inherited stdio, or `run_headless`'s logged pipe unless
`--verbose`); delivery tests wire a recording double and assert argv instead of
spawning.

Per-harness differences are data on the harness seam, so a new harness's
delivery lands there and this wiring stays put: `Harness::copy` assembles the
prompt data, `Harness::launch_command` (in `adapter::harness`) gives the launch
shape, and `Harness::run_line_renderer` gives the headless line renderer the
logged pipe applies line by line. `deliver` returns `Result<()>`, not a typed
outcome: its only caller needs success or failure, and a non-zero exit already
becomes an error naming the raw log. A `DeliveryOutcome` is deferred until a
consumer needs the status or log.

### `--apply` ― hand a plan to an interactive session

`--apply` (headless harnesses only) hands a headless run's plan to the
harness's own interactive session in one command. The advisor runs headless as
usual, but its raw log is kept and the agent-text lines (`Text` only, never the
JSON event lines) are extracted into the plan, saved as
`$XDG_STATE_HOME/saniwalens/plans/plan-<time>-<pid>-<seq>.md` with private
permissions (`0600`, exclusive creation, like the briefing temp files). The
path is always printed, with a note that pruning is a separate command's job: a
handoff run always leaves its plan behind, a normal `optimize` leaves nothing.

The harness's TUI then opens on an apply-ordering prompt
(`core::optimize::apply_prompt`) naming the plan file and the store, so the
dialog digs out each step's evidence with `saniwalens sql` instead of
re-parsing raw input. The launch is `opencode --prompt` for opencode (flag
acceptance verified; TUI prompt injection still awaiting one confirmation on a
real TTY), and `kilo --prompt` or bare `claude` as before
(`Harness::interactive_launch_command` in `adapter::harness`).

**Refusals.** `--apply` refuses `--print` (printing ends the run instead of
handing it off), `--verbose` (the handoff reads the logged stream verbose mode
does not keep), and interactive harnesses (their session already applies its
own plan). These checks run before any store work. If the dialog launch fails,
the error names the kept plan file. If the headless run yields no readable
text, nothing is saved and the error says so. The raw log is removed either way
once its text is read (it may echo secrets).

**Apply safety contract** (`core::optimize::apply_prompt`). The session walks
the plan one item at a time and never moves on without an explicit choice on
the current one: approval is per item, with no whole-plan batch approval. For
each item it:

1. Copies every target file's pre-edit bytes into
   `$XDG_STATE_HOME/saniwalens/backups/<timestamp>/` (dir 0700, files 0600;
   kept like plans, path always reported), for git-tracked and untracked files
   alike, and records `git status --short` for tracked ones.
2. Shows the planned diff (`diff -u` old to new; the file stays unchanged until
   the choice).
3. Explains the item briefly: why, what it fixes, its risk.
4. Asks for the choice on three fixed lines of at most 24 characters each:
   `y`: Apply this item / `n`: Skip to next / `q`: Quit applying. File names,
   line numbers, and gist stay in the diff and explanation, never in the
   choice lines. If the display looks wrapped or broken, one line tells the
   user to resize the terminal and ask for the same item again.

Only `y` edits the file. On refusal, files stay byte-identical, backups are
kept, and the session says so with the backup path. `n` skips to the next item;
`q` aborts and lists every unprocessed item as unapplied. A blanket instruction
such as "do the rest" never counts as a choice: each item still needs its own
`y`/`n`/`q`.

The session closes with a result summary of up to two sections,
`## Applied changes` (omitted when nothing was applied) and
`## Proposal only (not applied)`. Items skipped via `n`, aborted via `q`, never
reached, or refusing the actual config are listed with why, so applied stays
distinguishable from proposal-only without re-reading the plan.

### `cleanup` ― list and prune saved plans and backups

Saved plans and backups accumulate without bound, so `saniwalens cleanup` lists
and prunes them (`cli/cleanup.rs`, beside the handoff seam in `cli/handoff.rs`).
It needs no store and opens none, so it works on a fresh machine. `--format` is
the shared `table | markdown | json`; the JSON machine contract is
`removed_total` plus per-area `removed` / `kept` / `skipped`.

- **List** (bare run, or `--list`): every plan file and backup generation with
  its size (a generation dir lists its recursive total; links never resolve, so
  a link counts as a link) and modification time. Unreadable entries are
  skipped and their count goes to stderr as a warning, so stdout and JSON stay
  byte-identical.
- **Prune** (`--keep N`; a bare `--keep` keeps 10): deletes all but the newest N
  generations in each area. It snapshots every area and prints the count and
  total size before deleting anything, then reports every removed path through
  the shared table renderer, so the sweep shows its evidence before it deletes.
- **Containment.** Only `$XDG_STATE_HOME/saniwalens/plans/` and `.../backups/`
  are ever touched. Every victim is resolved (symlinks followed) and checked
  against those roots right before deletion, so an entry pointing outside is
  reported skipped and never followed out. A dangling link counts by its own
  parent dir and its link is removed. A victim that vanished between listing
  and deletion is ignored: neither an error nor reported skipped.

The default of 10 generations per area is a starting proposal: a handoff run is
manual and infrequent, so 10 covers weeks of iterations while bounding the
secret-bearing files. Tightening it is a review call.
