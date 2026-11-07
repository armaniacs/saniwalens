# CLI Specification

The CLI is the user's surface onto the stages (`architecture.md`): `analyze`
populates the store, the **views** and `sql` read it, and `optimize` hands the
findings to an interactive `claude` session. This spec defines the command
contract and the reporting model; exact flag spellings live with the
implementation and stay in sync via `.claude/rules/spec-sync.md`.

## `analyze` ― populate the store

```
saniwalens analyze [--projects <dir>] [--config <dir>] [--db <path>]
```

Reads both Claude Code inputs ― session transcripts (default
`~/.claude/projects/`) and live config (default `~/.claude/` plus the relevant
project `.claude/`) ― and writes the SQLite store (`storage.md`). That
`~/.claude` root is `$CLAUDE_CONFIG_DIR` whenever that variable names a
non-empty path ― an empty value carries no location and is treated as unset ―
so saniwalens follows a config tree the user has relocated instead of reporting on
an empty default. Incremental and idempotent: unchanged transcripts are skipped,
changed ones replaced, the surface catalog rebuilt from current config
(`storage.md`). The verb is `analyze`, not `build` ― it analyzes raw input into
facts.

`analyze` reads everything **read-only** and never copies input into the repo or
the store beyond the derived facts (`.claude/rules/session-data-privacy.md`).

### `--db` ― one store per user, not per directory

Shared by every command. The input is not scoped to the current directory ―
`analyze` walks *every* project's transcripts under `~/.claude/projects` ― so a
cwd-relative default would contradict the data it holds: it would build a
separate copy of the same cross-project store per directory saniwalens is invoked
from, and leave a database file inside unrelated Git working trees. The default
is therefore user-level, `${XDG_STATE_HOME:-~/.local/state}/saniwalens/saniwalens.db`
(`Session::open` in `session.rs`, which owns resolution, the legacy-store move,
and the guarded open as one seam), with the directory created on demand by
`Store::open` ― owner-only, like the store itself (`storage.md`). A non-absolute
`XDG_STATE_HOME` is invalid per the XDG spec and ignored ― honoring one would
reintroduce exactly the cwd-relative store this default exists to avoid.

State and not cache (`$XDG_CACHE_HOME`) because the store is not regenerable.
It is built incrementally from the transcripts, but Claude Code prunes those on
its own retention schedule, and the rows already extracted are then the only
surviving record of those sessions ― nothing on disk can rebuild them
(`storage.md`). A cache directory is by definition somewhere a cleaner may empty
without asking, which is the one thing this file cannot survive; that the store
migrates in place rather than being discarded on a schema change is the same
premise applied to upgrades. A project-local store stays available by asking for
it ― `--db ./saniwalens.db`.

### `config.toml` ― defaults and named profiles

The tool's own config file lives at
`${XDG_CONFIG_HOME:-~/.config}/saniwalens/config.toml` (`config_file_path` in
`session.rs`; a relative `XDG_CONFIG_HOME` is invalid and ignored, like its
`XDG_STATE_HOME` sibling above). The file is optional ― without it every
invocation behaves exactly as before.

```toml
default_harness = "kilocode"   # claude | opencode | kilocode

[profiles.work]
harness = "claude"
db = "work.db"                 # relative to the config file's directory
```

It holds two kinds of defaults plus named profiles `--profile` selects.
Resolution order, strongest first: explicit flags, the selected profile, the
file's top-level defaults, then the builtins (`claude`, the per-harness store
path). Flag *values* (`--scope`, `--format`) are parsed before the session is
built, so an invalid flag fails on its own ― no store open, no freshness line
ahead of the error. The report views resolve those flags through the
windowed dispatch in `cli/window.rs`: `WindowedDispatch::open` for the four
views without `--scope` (format, then period, then `Session::open_read`),
`open_scoped` for the three with it (`inventory`, `waste`, `failures`: scope,
then format, then period, then the store), `open_scoped_table_json` for
`doctor` (the same scope-first order, with the `table | json` vocabulary of the
section below), and `open_pair` for trend (format, then pair, then the store).
The narrowing is carried by what each entry returns ― `open` gives a
`WindowedRead` (session, format, window), so the views that never filter have
no filter to ignore, and only the scoped entries (`open_scoped` /
`open_scoped_table_json`) return a `ScopedRead` (that read plus the filter
`--scope` selected); `optimize` is the exception that resolves the store path
first, exactly as the pre-`Session` dispatch did. A relative `db` joins the directory holding `config.toml` ― never the
current working directory ― while an explicit `--db` is taken as is, even
when relative; `~` is not expanded. Unknown keys are ignored so a newer file
never breaks an older binary. A missing file is silence, not an error; anything
else wrong with the file (unparsable TOML, unknown harness, missing profile)
fails the invocation with the file named, and an unknown `--profile` lists the
available ones.

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
| `usage` | Skill event rollups: per skill, or per time bucket (`--by`) ― frequency, tokens, `ctx_growth`, duration. Leads with a token-destination line (main-thread skill output vs subagent total) so the reader sees where tokens actually go before the table, then splits the subagent half **per agent type** (runs and output tokens, costliest first) ― the aggregate is usually the larger number and the split is the part that can be acted on. JSON carries the split's coverage as `agent_split` in every state. In text, a partial split prints its covered and uncovered figures between the total and the table, an unavailable split (no rows at all) is warned on stderr, and rows that cover the total need no note (see *Reporting honesty*). A section with no rows is omitted rather than rendered as bare headers. |
| `trend` | How usage moved: each kind of event (count / output tokens / duration), each skill (invocations, largest movement first), and the session / subagent totals ― this window against the equal-length window right before it (default 7d vs the 7d before; `--last` or `--since` [+ `--until`] fixes the current one, the pair resolved by the Period section's rules). A kind renders the metrics it carries: point kinds render the count alone, because ingest writes their output tokens and duration as constant zeros (`events.md`: a span is currently `skill_invocation` only) ― their rows for those metrics are omitted rather than rendered as `0.0` / `n/a`, and `--format json` keeps every kind's three metrics (the machine contract; only the table omits what cannot apply). Every rendered row carries both windows' values, the delta, and the direction; a comparison is `n/a` / `"unavailable"` with no delta claimed only when *that row's* previous side holds no baseline, never a drop to zero, and the n/a reason text rides only with a report that holds that comparison (see *Reporting honesty*). Both windows' ranges ― the current one and the equal-length one **derived** one second before it ― are disclosed like every view's window is, and a pair whose either window leaves the calendar the Period section names is refused as a period error rather than disclosed with an empty bound; `--format json` carries them as `window.current` / `window.previous`. A section with no rows (e.g. no skill usage in either window) is omitted rather than rendered as bare headers. |
| `waste` | Just the flagged opportunities (unused, costly+rare, always-on heavy, …) with their evidence and owning scope; `--scope` filters to one layer. |
| `overhead` | Reconcile the empirical always-on floor (the leanest observed **session start**, not the context a skill happened to run at ― `surfaces.md`) against the readable always-on config; the residual is the system prompt + built-in tools + MCP schemas the catalog cannot weigh. The per-project table carries the session count behind each floor, because the floor is a minimum over observations and a project with few sessions may never have caught a fresh one. Reports the metric as unavailable when no session start was observed at all. |
| `prompts` | How the user steers the session: the mix of steer / correct / question / instruct prompts (`core::prompt`, lexical heuristics), with a verdict ― heavy steering suggests more autonomy, frequent correction suggests clearer upfront specs. This is a behavioral signal, not a config metric; embeddings showed prompt *topics* do not map to reusable skills, so the value is in *how* you prompt, not *what about*. |
| `failures` | Where the work stumbles: recurring tool failures by category (`core::friction` ― edit-precondition, path-not-found, blocked-by-hook, …), ranked, each with what it suggests fixing and the originating-tool split. This analyses the *work*, not the config ― where the real cost is. The classifier separates fixable friction from non-actionable noise (cancelled, transient). `--scope` follows the routing above: `global` = spread categories, `project` = each project's majority-owned categories, `project:<slug>` = everything in that one project (this subsumes the old `--project` flag). Denials are categorised from the transcript's own denial marker; everything else by lexical heuristics. |
| `stuck` | Bursts of rapid re-edits to one file by one session (`core::thrash` ― N+ edits within a few minutes), ranked. This isolates *where Claude got stuck and kept retrying* from healthy spread-out editing ― an algorithmic signal a flat edit count cannot give. Observed: a file edited 25× in under 8 minutes. The table carries the project; `--format json` adds the session id and full path, which is what makes an episode investigable. |

Trend's per-metric reason. A comparison is `n/a` / `"unavailable"` only when that
row's previous side holds no baseline ― its kind, its skill, or a session on the
totals axis absent from the previous window ― and never because the baseline
measured zero: a row whose aggregate exists in the previous window compares at
its measured value, zero included (a 0-token span is a measurement, and so is a
session that ran no subagent). Every metric's JSON object carries `reason`
beside its direction: `null` when the comparison was judged,
`"no_baseline"` when the previous side is absent, and `"not_applicable"`
when the kind does not carry the metric (see *Reporting honesty* and the
machine contract below).

### Output formats ― human vs machine

Every command has both a human form and a machine form, strictly separated:

- **Human** (default `table`, plus `--format markdown` for pasting into
  notes/PRs; `doctor` and `analyze` have their own text layouts instead of
  markdown). The output must explain itself: every table opens with a dimmed
  **framing block** saying what the view shows and how to read it (table
  format only ― markdown stays a bare table), headers are words rather than
  abbreviations, and interpretation notes are dimmed footnotes. ANSI styling ―
  headings, dimmed context, highlighted counts and staleness warnings ― turns
  on only when stdout (stderr for the freshness line) is a terminal,
  `NO_COLOR` is unset, and `TERM` is not `dumb`; pipes and captures always get
  plain bytes. Styling is applied after column padding so it never skews table
  alignment.
- **Machine** (`--format json`, on every command): a typed JSON document alone
  on stdout ― SQLite integers/reals/NULLs stay JSON numbers/null
  (`Store::query_json`), `doctor` emits the routed findings structure
  (`core::optimize::Findings`, `projects` filtered by `--scope`), and each view
  emits its rows as named objects. `trend`'s rows and its JSON are two forms
  of one typed payload (`trend_payload` in `cli/views/trend.rs`): both derive from
  it, so a metric's delta, direction, and reason are judged once ― float
  metrics at the display's 0.1 precision, so a drift smaller than the shown
  0.1 (an ms-derived duration's addition error) reads as `same` in both
  forms; a golden test (`tests/trend_json_golden.rs`) pins the emitted JSON.
  Every trend metric carries `reason` beside its direction: `null` when the
  comparison was judged, `"no_baseline"` when that row's previous side holds
  no aggregate, and `"not_applicable"` when the kind does not carry the
  metric. The values are the payload's `UnavailableReason` enum serialized
  (machine-readable only, never prose), and `direction == "unavailable"`
  exactly when `reason` is not null, so a consumer distinguishes "nothing to
  compare against" from "this metric does not apply" without inferring it
  from the numbers. All
  freshness/refresh chatter goes to
  **stderr** (see above), so `saniwalens <cmd> --format json | jq` needs no
  filtering. Human-only footnotes (row counts, heuristic caveats) are either
  folded into the JSON (`total`, …) or dropped, never mixed into it.

`doctor`'s human form is its own text layout rather than a table pasted as
markdown, so its `--format` vocabulary is `table | json` and nothing else:
`markdown` (and the `md` shorthand that resolves to markdown in the other
views) is refused at the flag stage ― `unknown --format 'markdown'
(table|json)` ― before the refresh, the freshness line, or the store's
creation, as the flags-before-store contract above requires. The other
windowed views (`trend` included) take all three values.

Slices not covered here ― the Bash command mix, the most-edited files, an error
category broken down any way ― are a `sql` one-liner; those once had thin
`commands`/`hotspots` views, dropped once `sql` existed.

### Guided entry ― pick instead of typing

Running `saniwalens` with no subcommand on an interactive terminal, or with
the explicit `tui` subcommand, opens the guided entry instead of a report:
it asks which agent harness is in use, what to look at, and which scope, then
runs the equivalent report command. The implementation lives in
`cli/views/guide.rs`; this section states the contract, not the code.

Entry conditions: no subcommand *and* stdin and stdout both TTYs, or the
explicit `tui` subcommand. A `--harness` (or `--profile`) flag given with
either entry is a preset: step 1 is skipped and the named value is used. Any
other token ― a real subcommand, `--help`, `--version` ― belongs to clap,
and the guide never intercepts it.

The four steps, each defaulting to its first choice (empty input keeps it,
`q` quits the guide): (1) confirm the harness (the resolved default is
shown); (2) pick the purpose ― `doctor` (health check), `waste` (prune
candidates), or `failures` (recurring friction); (3) pick the scope ― all
history (no `--scope` flag), `global`, or `project`; (4) after the report,
answer the config save question below. After each report the guide asks
whether to look at one more (default no) and loops on `y`.

Output is always the table form: the guide takes no `--format` and no period
flags. Before running, it echoes the equivalent command line (e.g.
`saniwalens --harness kilocode waste --scope global`); after the report it
echoes the executed command again. The reproducible artifact is that command
line, not the session ― the report itself is the same table the command
would print.

After the report, the guide asks whether to save the used harness to
`config.toml`: skip, set it as `default_harness`, or store it under a named
`[profiles.<name>]` profile. The edit is line-preserving ― only the one line
(or section) changes, comments and everything else stay untouched ― and the
guide shows the path plus the changed excerpt and asks for confirmation
before writing.

Non-TTY behavior: the explicit `tui` subcommand without both TTYs is refused
with an error instead of blocking on unreadable prompts; a bare no-args
invocation without both TTYs falls through to clap, whose
missing-subcommand error is what scripts see today. The scripted E2E suite
(`tests/guide_flow.rs`) drives the real binary through piped stdio by setting
`SANIWALENS_TEST_FORCE_TTY=1`, which counts as a terminal for the check above;
the variable is test-only and unsupported, and must never be set by users.

v0.5.4 scope is `doctor` / `waste` / `failures` plus the config save above.
`optimize` itself stays out of the guide, but after each report the guide
prints a next-step line pointing at it with the matching scope
(e.g. `saniwalens optimize --scope global`); the remaining reports, period
flags, and `sql` stay out. The candidates and their order are declared in
the design doc's post-v0.5.5 expansion note
(`docs/exploration/2026-11-06-guided-tui-design.md`).

### Period ― narrow a report to a window of the history

The windowed report views above take `--since` / `--until` / `--last` so "what did
the agent cost last week" is a view flag, not a `sql` query. The other commands
(`analyze`, `sql`, `optimize`) take none ― they are whole-history by contract.
Which commands are windowed is derived from the subcommand tree (a command
carrying `--last`), never a hand-maintained list: every subcommand
must resolve to windowed or whole-history, and `tests/cli_help_golden.rs` pins
that partition beside the `--help` captures.

`trend` resolves the same flags as a **pair** ― the current window plus the
equal-length window immediately before it (the previous ends one second before
the current starts, so no event is counted in both). `--since` alone runs the
current window to now ― a bounded window, unlike the single-window views' open
end ― and `--until` alone is an error: with no current start, the window to
compare against is undefined.

- `--since DATE` / `--until DATE`: a named-day window over the history, each
  date a whole day (`YYYY-MM-DD`, read in the presentation zone ― **JST**, the
  same zone the bucketing table uses; the comparison is against the UTC epoch
  the store keeps). `--until` is **inclusive**: the whole named day counts.
  The resolved instants carry the same calendar bound the rolling span has,
  checked at the same resolve entry: the day end adds its +86,399 after the
  zone conversion, so a west offset (the `--tz` zone Time bucketing declares,
  not wired into the period path) can resolve `NaiveDate::MAX`'s last second
  past the calendar's ceiling, and a date whose resolved midnight or last
  second leaves the calendar is refused with the empty-window error naming
  its flag. Under **JST** the named-day path stays inside the calendar on its
  own ― `NaiveDate::MIN`'s midnight sits below the floor, where the zone
  conversion refuses it as a local time ― which is why the bound is
  established at the entry instead of trusted to the zone's sign.
- `--last SPAN`: a rolling window ending now ― `<N>d`, `<N>h`, `<N>w`.
  Exclusive with the explicit dates (clap conflict), so "last 7 days" can never
  be silently reinterpreted by a stray `--since`. The conflict is declared once
  on the shared period-flags group the single-window views flatten
  (`cli/window.rs`); trend's pair keeps its own declaration, so its
  pair-specific help wording survives (the help capture pins hold every view's
  help byte-identical). The span's resolved window
  must stay inside the calendar chrono can express (`NaiveDate`'s range,
  roughly ±262,000 years, `NaiveDateTime::MIN`/`MAX`'s timestamps): a span
  whose resolved window leaves it (e.g. `--last 100000000d`) has no date to
  name, so it is refused at the resolve entry with the same invalid-span error
  as an unparseable one. `trend`'s pair resolves through the same entry, so a
  current window outside the calendar is refused there too ― and so is the
  **derived previous window**, the equal-length window the pair builds one
  second before the current one: `--last` reaches back twice, so a span whose
  current window still resolves can start the derived previous below the
  calendar (e.g. `--last 50000000d`), refused with the same invalid-span
  error. The date-derived pair has the same bound: a `--since` whose
  equal-length derived previous leaves the calendar (e.g.
  `--since=-200000-01-01`) is refused with the empty-window error naming
  `--since` ― the input is the date, so the invalid-`--last` wording would
  blame the wrong flag.
- Errors are parse-time, before any report renders: an unparseable date fails
  on stderr with a non-zero exit and an empty stdout; `--since` after `--until`
  is an empty-window error rather than an empty-looking report. A `--last`
  whose resolved window leaves the calendar is that same invalid-span error,
  and a named day whose resolved midnight or last second leaves it is the
  empty-window error naming its flag ― non-zero, empty stdout, before any
  report renders or store work runs.
- The window is **disclosed, never silent**: a dimmed `report window:` line
  rides with the freshness line on stderr (every format), the framing block
  carries it as its last line (table only ― markdown stays a bare table),
  `doctor` prints it in-band under its heading, and `--format json` gains a
  `window` object (`since` / `until` as RFC 3339 with their offset). Every
  accepted window renders a bound that names its instant, never an empty
  string: an in-calendar instant renders in any valid zone, so the bounds
  renderer has no empty-string fallback to absorb an out-of-calendar bound
  with. Two limits belong to the presentation, not to the window. At the
  calendar's own ends the rendered wall clock sits at most one zone width (less
  than a day) outside the calendar while the instant it names stays exact and
  in-calendar, and the spelled-out **local date** is at most one day outside ―
  the ceiling rendered in JST is `+262143-01-01T08:59:59+09:00`, a date
  `NaiveDate` cannot name. Only a clock within a day of ±262,000 years reaches
  either, so a real run cannot. And RFC 3339 names years 0000..=9999 only, so a
  render at the calendar's edge years has the RFC 3339 shape but does not parse
  back as RFC 3339. The tests pin both ends byte for byte
  (`the_presentation_spill_at_a_calendar_end_is_pinned_byte_for_byte` in
  `src/core/period.rs`).
  Without the flags every output is byte-identical to the pre-period behavior.
- The catalog half of `inventory` is current config, so it is not windowed by
  time ― its **usage half** is. An unused row under a window says
  `UNUSED (window)` / `"unused-in-window"`, never plain "unused".
- Sessions the writer could not stamp (no observable start, empty
  `started_at`) are **excluded** from a windowed `session_stats` /
  `subagent_totals` figure (`store.rs`'s `<> ''` guard) rather than guessed
  into it; without the flags they still count.
- The windowed readers that scan `events` read the store through
  `started_epoch`, and the store grows with every analyze. The store declares
  the index that keeps those reads fast (`storage.md`) and creates it on the
  next read-write open; `--frozen` / `sql` (read-only) never create it, so a
  store not opened read-write since it was declared keeps scanning. The
  sessions-axis figures read `sessions.started_at` instead ― a small table no
  index needs to serve. Watch the size with
  `saniwalens sql "SELECT COUNT(*) FROM events"`.

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

The report never hides an approximation; it labels it (these mirror the metric
caveats in `events.md` / `surfaces.md`):

- `sub_tokens_estimated` rows are marked so estimated subagent cost is not read
  as exact.
- Trailing spans (closed only by session end) are marked; their duration is a
  lower bound.
- Surfaces with unknown `static_tokens` (e.g. `mcp_tool` without an available
  schema) are marked unknown, never reported as zero cost.
- A surface's startup cost and its on-demand body cost are shown as two figures,
  never one (`surfaces.md`): a skill's body is what invoking it costs, so
  printing it alone under a "what to trim" heading reads as a startup saving the
  deletion never delivers.
- "Unused" always carries the window it was evaluated over, so it is not mistaken
  for "never installed": without the period flags the window is all history, and
  under `--since` / `--until` / `--last` the row itself says `UNUSED (window)`
  (inventory; the other views' empty states read "in the evaluation window").
- A window the input makes unresolvable is an **error**, never a silent
  report: a `--last` whose resolved window ― or, in `trend`, the equal-length
  window **derived** one second before the current one ― leaves the calendar
  names dates that do not exist, and fails with the invalid-span error; a
  `--since` whose derived previous leaves it fails with the empty-window
  error naming `--since`; and a named day whose resolved midnight or last
  second leaves it fails with the empty-window error naming its own flag.
  The alternative would be a disclosure line with an empty `since` and a
  store bound (`started_at >= ''`) that silently passes every session ― a
  successful-looking report that measures nothing. The bounds renderer keeps
  no empty-string fallback to absorb an unrenderable instant with: an
  accepted window's instants are in-calendar by the resolve entry's check, so
  a bound always renders, the disclosure line and the JSON `window` cannot
  carry an empty string, and the one-zone-width presentation spill at the
  calendar's own ends (qualified in the Period section) is pinned by a test
  instead of being passed over as if it named a valid date.
- A subagent run whose agent type was never recorded is shown as `(unknown)`,
  never merged into a named type and never dropped ― the per-agent rows must
  keep summing to the subagent total the same report prints above them, for
  every run that could be extracted (the two cases below cover the runs that
  could not).
- When they *cannot* sum to it ― a store predating the per-agent split, read
  with `--frozen` so the refresh that would fill it never ran ― the report says
  so on stderr instead of letting an absent breakdown pass for a zero one
  (`storage.md`'s report-the-gap contract). It rides with the freshness line
  rather than in the table, so piped stdout and `--format json` stay clean.
  Under a drawn window an empty split is usually the two time axes at work ―
  the total reads the session's stamped start, the rows read each run's own
  epoch ― so the message names the window instead of pointing at a refresh
  that cannot fill a straddle; the refresh hint stays without the flags.
- When they sum to only *part* of it ― sessions analyzed before runs were
  extracted keep their totals (`storage.md`: the store outlives its inputs), but
  once Claude Code has pruned their transcripts no refresh can ever produce their
  rows ― the rows are labeled a **partial split** in-band, beside the numbers
  they qualify, because this is a property of the data and not a staleness a
  refresh could cure: `usage` prints the covered and uncovered figures between
  the total and the table, `doctor` states the gap before naming any agent, the
  briefing does the same under its subagent line, and JSON carries the
  reconciliation as `agent_split` (`core::subagent::SplitCoverage`). Under a
  drawn window the same gap is the straddle, so the in-band labels say the
  uncovered tokens fall outside the report window instead of the
  pruned-transcript reason; without the flags the wording is unchanged.
  `doctor` introduces its top rows as "most of" the subagent output only when
  they exceed half of the total ― a long tail, or a partial split, gets a
  wording that claims no more than the rows show.
- An `n/a` in `trend` states its real reason, and the reason is a fact about
  that row: its previous side holds no baseline ― no event of that kind, no
  invocation of that skill, or no session on the totals axis. It never means
  the baseline measured zero, and never a drop to zero. The table's reason
  text names that row scope ("that row's previous side") because one sentence
  covers the kind, skill, and totals sections, and it is printed only when a
  report actually holds such a comparison; the footnote about an empty
  previous window is printed when that window holds no event of any kind. A
  session alone does not suppress it: the note speaks about events, and the
  totals rows may still compare while the kind rows are n/a. A metric a kind does not carry renders no row at all ― the table
  omits it, so the unavailable label is never attached to a metric the table
  does not show. `--format json` keeps every kind's three metrics for its
  consumers, holding the same shape with a null delta and
  `"reason": "not_applicable"` where the kind carries nothing,
  `"reason": "no_baseline"` where the previous side is absent, and
  `reason: null` where the comparison was judged. A baseline that measured
  zero still compares: a 0-token span is a measurement, a session that ran no
  subagent measured 0 runs, and a baseline that only rounds to the displayed
  zero compares too. Count axes keep their baseline tied to the aggregate
  row's presence, deliberately: a window with no event of a kind stays
  `no_baseline` rather than reading a 0 -> N change as a measured move. A
  byte-identity criterion stated for this view by an earlier refactor is
  withdrawn, and two deliberate output changes replace it: a report holding a
  measured-zero baseline now compares its cells instead of printing `n/a`, and
  a report that holds any n/a comparison prints a one-line reason sentence
  whose wording names the row scope (`that row's previous side`) so it covers
  the kind, skill, and totals sections. Pinning output that the spec forbids
  is what the criterion did; the invariants above, each pinned by a test, are
  what stands in its place.

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
saniwalens sql [<query>] [--format table|markdown|json] [--db <path>]
```

`sql` is the one read that never auto-refreshes ― it opens the store strictly
read-only, so an ad-hoc query is reproducible and can never mutate anything.
It prints the freshness header (store age) on stderr instead, so a stale read
is at least visible.

The store's own query surface: run an arbitrary read query and print the result.
The query is the argument, or ― when omitted ― read from **stdin**, so both
`saniwalens sql "SELECT …"` and `echo "SELECT …" | saniwalens sql` work (stdin
sidesteps shell quoting for complex queries). This exists because the tool is a
*session-analysis* tool: anyone wanting a slice the fixed views do not cover ―
notably the `optimize` session chasing a root cause ― should query the analyzed
store, not re-parse the raw transcripts. The store is opened **read-only** so an
ad-hoc query can never mutate the derived data, and an absent db is an error
(run `analyze` first), not a silently-created empty one.

A `tool_errors` view (`storage.md`) names the friction columns that are
otherwise overloaded onto generic event columns (`category`, `excerpt`, `tool`,
`target`, `project`), so a friction query reads cleanly without knowing the
encoding ― e.g. `SELECT target, COUNT(*) FROM tool_errors WHERE
category='edit-precondition' GROUP BY target` answers "which files keep failing
to edit", which the error text alone cannot. `SELECT sql FROM sqlite_master`
lists the full schema.

## `optimize` ― act on the findings with an advisor session

```
saniwalens optimize [--projects <dir>] [--db <path>] [--scope global|project[:<slug>]]
                [--frozen] [--print] [--verbose] [--timeout <DURATION>]
```

The AI-proposal consumer (`architecture.md`): rather than emit static
"recommendations" the tool cannot safely act on, `optimize` analyzes (unless
`--frozen`, the same flag the views use), composes the findings with a prescribed advisor prompt, and
launches the harness's agent seeded with it ― `claude` for an interactive
session that applies edits on agreement, `opencode run --format json` for a
headless run that streams only the agent's text while the raw tool output
logs to a private file (`--verbose` restores the raw stream) and prints the
plan without editing. The private log lives for the whole run and is named in
any failure, so a failed advisor still leaves its full output reachable; the
run's child and heartbeat thread are torn down on every exit path, not only on
success (`cli/headless.rs`). `--timeout <DURATION>` (e.g. `30s`, `2m`, `500ms`)
bounds a headless run; when it expires the child is killed and reaped and the
error names the raw log. It is off by default, so a legitimately long run is not
cut short. The prompt makes
the session **investigate autonomously to a conclusion**: it pins down each root
cause and proposes a concrete fix, rather than handing the analysis back with a
"which area do you want to start with?". It prioritises work friction over config
size, verifies before recommending removal (an "unused" skill may be invoked by
subagents) by inspecting rather than asking, and ― in interactive runs ― pauses for exactly one thing ―
approval of the concrete fix-plan before any file is edited. This "drive to a
plan, gate only on applying it" shape is the design; if it changes, update
`instructions` in `core::optimize` and this paragraph together.
`instructions` renders from the harness copy (`HarnessCopy`), not a constant,
so the advisor identity, config locations, and raw-input pointers name the
harness whose rows the briefing renders.

Crucially the briefing is the **complete** analysis, not a headline ― every
project's friction breakdown, the full Bash/hotspot/thrash detail, and the actual
unused / always-on-heavy surface lists with startup-vs-body token costs ― **routed the same way
the views are** (see "Scope"): global sections first, then one section per
project, so the session fixes each finding in the layer that owns it. `--scope`
restricts the briefing to one layer and prepends an explicit scope statement
(`core::optimize::scope_statement`), turning the session into "optimize my
global setup" or "optimize this project" specifically. Each friction category
also carries its **split across the originating tools** (`path-not-found` →
`Bash 33, Read 29, Edit 9, playwright 7`) and a few **concrete example excerpts**
― the actual failing paths/files behind the count (`events.md`: `tool_error`
keeps the tool name and a short error-text excerpt) ― so the fix is obvious from
the briefing and the session need not re-mine the transcripts to attribute or
locate the failures. The tool split also separates true file friction from a
browser-automation miss that merely reads as "not found". The briefing is the
*headline*; for any deeper slice (the full failing-path list, a worktree split,
arbitrary groupings) the prompt directs the session to **query the store with
`saniwalens sql`** rather than re-parse the raw transcripts in Python ― the
store is exactly the analysis surface saniwalens exists to provide. It still
spends its effort on the evidence the store cannot hold (the real CLAUDE.md,
rules, hooks). Embedding the headline plus pointing at the queryable store is
what stops the seeded session from re-deriving what `analyze` already computed.

**The briefing is never passed on argv.** It holds concrete paths and error
excerpts that may be sensitive, and a process argument is world-visible (`ps`).
So the launch writes the briefing to a private temp file and passes `claude`
only a data-free pointer prompt (the prescribed instructions plus "read this
file"); the file is removed the instant the session ends, whatever the outcome.
Private means **created exclusively** first and `0600` second: the temp dir is
shared, so a same-user process that pre-creates the path as a symlink would
capture the briefing no matter what mode was set afterwards. Where there is no
mode to set (Windows), the per-user temp directory carries the guarantee
instead. `--print` is the exception ― it writes the full prompt (briefing inline)
to stdout for piping / inspection, where the user has opted into seeing it.

The split mirrors the rest of the tool: prompt composition is a pure transform
(`core::optimize` ― `render_briefing` for the Markdown report with empty sections
omitted and long lists capped, `instructions` for the role, `launch_prompt` for
the argv pointer), and only the temp-file write and process launch are I/O.
The findings themselves split the same way: the shell fetches the store's rows
(`findings_input` in `cli/mod.rs`) and hands them to the pure assembly in
`core::findings` ― the joins, the named caps (`TOP_COMMANDS_LIMIT`,
`EXAMPLES_PER_CATEGORY`, `THRASH_LINES_PER_PROJECT`, …), the scope routing, and
the ordering ― so the briefing's wiring is unit-tested over small inputs and
only the queries and the print are I/O.

The send is one seam: `deliver` runs a `DeliveryPlan` whose mode
(`DeliveryMode::Print | Interactive | Headless`, chosen by `--print` over the
harness's launch shape) picks the payload ― `Print` writes the prompt to
stdout, launch modes write the briefing to the private pointer file, hand the
spawn to the `AdvisorLaunch` adapter, and remove the file the moment the run
ends. Production wires `ProcessAdvisor` (inherited stdio, or `run_headless`'s
logged pipe unless `--verbose`); the delivery tests wire a recording double
and assert the argv instead of spawning. `Harness::copy` assembles the harness
data those prompts render from, so the shell never spells `HarnessCopy` field
by field. The launch shape the mode defers to is per-harness **data on the
seam** too (`Harness::launch_command` in `adapter::harness`, beside the
harness's other arms), so a new harness's delivery lands there and this
section's wiring stays put. The headless run's raw lines render through
per-harness data as well: the plan carries the renderer the harness seam
chose (`Harness::run_line_renderer`), and the logged pipe applies it line by
line, so the loop never names a harness and a new harness's stream needs no
shell edit. `deliver` returns `Result<()>`, not a typed outcome: its only caller
(`optimize`) needs success/failure, and a non-zero exit is already turned into
an error naming the raw log; a `DeliveryOutcome` is deferred until a consumer
needs the status or the log. PBI 57 records this decision.
