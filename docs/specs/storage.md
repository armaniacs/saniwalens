# Storage Specification

The store is the SQLite database `analyze` writes and every consumer reads
(`architecture.md`). It holds the tables that mirror the catalog×usage model ―
a shared `sessions` dimension, the `surfaces` catalog, the `events` spine, and
`subagent_runs` for work that happened in its own transcript ― plus
`ingested_files` for incremental rebuilds. This spec defines the schema and the
ingest contract; the *meaning* of the columns lives in the specs that own them
(`events.md`, `surfaces.md`, `config-format.md`).

## Schema

```sql
-- Shared dimension. One row per analyzed transcript.
CREATE TABLE sessions (
    id              TEXT PRIMARY KEY,   -- sessionId
    project         TEXT NOT NULL,      -- normalized (worktree folded; see below)
    slug            TEXT NOT NULL,      -- raw cwd-slug
    root            TEXT NOT NULL,      -- real start directory (records' cwd, worktree folded); '' when unknown
    source_path     TEXT NOT NULL,      -- the main transcript file
    started_at      TEXT NOT NULL,      -- RFC3339 UTC
    sub_tokens      INTEGER NOT NULL,   -- the session's exact subagent output total (events.md)
    sub_agent_count INTEGER NOT NULL,   -- subagent runs the session spawned, nested included
    version         TEXT                -- Claude Code version, when present
);

-- Catalog: everything installed, with its static cost. Read from live config.
CREATE TABLE surfaces (
    kind          TEXT NOT NULL,    -- skill | rule | mcp_server | mcp_tool | hook | claude_md | permission | agent
    id            TEXT NOT NULL,    -- stable identity within the kind
    scope         TEXT NOT NULL,    -- global | project
    project       TEXT NOT NULL,    -- owning project's normalized slug; '' for global rows
    config_path   TEXT,
    static_tokens INTEGER,          -- token weight of the whole definition (a skill's body = its invocation cost); NULL if unknown (e.g. mcp_tool)
    startup_tokens INTEGER,         -- weight of the slice loaded into every session (description for a skill/agent); NULL if unknown
    load_mode     TEXT NOT NULL,    -- startup_full | startup_description | path_conditional | on_demand | tool_schema
    attrs_json    TEXT,             -- kind-specific extras (paths glob, hook matcher, …)
    PRIMARY KEY (kind, id, scope, project)
);

-- Usage spine. One row per extracted event; a skill span is one kind.
CREATE TABLE events (
    id                   INTEGER PRIMARY KEY,
    session_id           TEXT NOT NULL REFERENCES sessions(id),
    source_path          TEXT NOT NULL,   -- file this event came from (ingest delete key)
    source_line          INTEGER,         -- 0-based line index in source_path; recovers the raw record (prompt text for goal-3 clustering) without storing it
    kind                 TEXT NOT NULL,   -- skill_invocation | session_start | tool_use | agent_spawn | prompt | tool_error | compaction | permission_prompt | …
    surface_kind         TEXT,            -- join key into surfaces (NULL for surfaceless kinds)
    surface_id           TEXT,            -- for tool_error: the friction category (no surface join, surface_kind NULL); for file_edit: the basename hotspots group on (the full path is in target)
    source               TEXT,            -- kind-specific detail string: slash|tool (skill path); behavior class (prompt); a short error-text excerpt (tool_error); NULL otherwise
    started_at           TEXT NOT NULL,   -- RFC3339 UTC
    started_epoch        INTEGER NOT NULL,-- UTC unix seconds (bucketing)
    duration_sec         REAL NOT NULL,
    is_trailing          INTEGER NOT NULL,-- 1 when closed only by session end (duration is a lower bound)
    out_tokens           INTEGER NOT NULL,
    ctx_growth           INTEGER NOT NULL,
    ctx_start            INTEGER NOT NULL,-- for session_start: the context the session began with (the always-on floor)
    ctx_peak             INTEGER NOT NULL,
    sub_tokens           INTEGER NOT NULL,
    sub_agent_count      INTEGER NOT NULL,
    sub_tokens_estimated INTEGER NOT NULL,
    model                TEXT,            -- representative model (skill_invocation); the originating tool name (tool_error)
    target               TEXT,            -- the failed call's subject: file_path edited / command run (tool_error); the edited file's full path (file_edit); the Agent call's id (agent_spawn), joining subagent_runs
    attrs_json           TEXT
);

CREATE INDEX events_by_epoch ON events(started_epoch);
CREATE INDEX events_by_surface ON events(surface_kind, surface_id);

-- One row per subagent execution, from the subagent's own transcript and its
-- sidecar (events.md). This is where "which agent types consumed those tokens?"
-- is answered; the session total answers only "how many".
CREATE TABLE subagent_runs (
    id               INTEGER PRIMARY KEY,
    session_id       TEXT NOT NULL REFERENCES sessions(id),
    source_path      TEXT NOT NULL,   -- the parent session's transcript (ingest delete key)
    run_path         TEXT NOT NULL,   -- the subagent's own transcript
    agent            TEXT,            -- the agent type; NULL when no sidecar recorded it
    agent_id         TEXT NOT NULL,
    tool_use_id      TEXT,            -- the Agent call that spawned this run; joins events.target
    root_tool_use_id TEXT,            -- the main-thread call its spawn tree started at
    parent_agent_id  TEXT,            -- the run that spawned it, for a nested run
    spawn_depth      INTEGER NOT NULL,-- 1 main-thread, 2+ nested
    model            TEXT,
    prompt_id        TEXT,            -- the spawning turn (the fallback join key)
    out_tokens       INTEGER NOT NULL,
    started_at       TEXT NOT NULL,   -- RFC3339 UTC
    started_epoch    INTEGER NOT NULL
);

CREATE INDEX subagent_runs_by_agent  ON subagent_runs(agent);
CREATE INDEX subagent_runs_by_source ON subagent_runs(source_path);

-- Incremental-ingest fingerprints.
CREATE TABLE ingested_files (
    path  TEXT PRIMARY KEY,
    mtime INTEGER NOT NULL,
    size  INTEGER NOT NULL
);

-- Analyze-run metadata: analyzed_at (RFC3339 UTC), projects_dir, config_dir,
-- analyzer_version. Freshness reporting reads analyzed_at; auto-analyze on read
-- commands re-runs the analysis against the same recorded roots (cli.md);
-- analyzer_version invalidates the incremental skip (below).
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- A clean read view over tool_error events for ad-hoc SQL (the `sql` command).
-- The friction signal is overloaded onto generic event columns; the view names
-- it and joins the project so a query need not know the encoding. The view
-- statement is generated from `EVENT_ENCODING` (`store/mod.rs`) ― the table below
-- is the definition, the view is a projection of it.
CREATE VIEW tool_errors AS
SELECT e.session_id, s.project,
       e.surface_id AS category, e.source AS excerpt, e.model AS tool,
       e.target, e.started_epoch
FROM events e JOIN sessions s ON e.session_id = s.id
WHERE e.kind = 'tool_error';
```

### Indexes are the planner's choices

A declared index is an option handed to SQLite's planner, not behavior: it never
changes what a query returns, only how rows are reached. Indexes are declared in
`INDEX_SCHEMA` (`store/mod.rs`; the DDL above mirrors it) and reconciled into every
existing store on the next ReadWrite open, so adding one is non-migrating:
`SCHEMA_VERSION` does not move. ReadOnly opens (`--frozen`, `sql`) cannot create
it, so a store not opened read-write since the declaration keeps scanning.

`events_by_epoch` backs the windowed readers that scan `events` on
`started_epoch` (the twelve windowed methods, including the rollup and
skill-usage queries in `trend_aggregates`' two windows). On a synthetic 1M-row
store with a 7-day window it cut a query from 106-220 ms to 36-42 ms (SQLite
3.54.0), the plan changing from `SCAN events` to `SEARCH USING INDEX`. The claim
stops at the `events` axis: the sessions-axis readers bind the TEXT
`sessions.started_at` over a small table that needs no index, and the per-agent
breakdown binds `subagent_runs.started_epoch`, which this index does not cover.

The gain is not immediate. On a young store the planner keeps choosing
`events_by_surface`, and the index pays off only as statistics grow, from roughly
500k-1M rows (at 1M rows one `trend` run costs about 500 ms and `findings` about
1 s, past the ~100 ms felt threshold). Check the count with
`saniwalens sql "SELECT COUNT(*) FROM events"`.

Two scoping notes are the contract, not defects. The ingest delete
(`DELETE FROM events WHERE source_path = ?`) gains nothing from this index;
serving `source_path` is a separate change of the same shape. And the
user-facing consequence of the ReadOnly asymmetry is stated where users read it
(`cli.md`'s Period section, README's Notes).

### Column encoding is defined once

The generic columns carry kind-specific meanings: `tool_error`'s friction
category rides in `surface_id`, its excerpt in `source`, the originating tool in
`model`; `file_edit` keeps the basename in `surface_id` and the full path in
`target`; a prompt's behavioral class rides in `source`. That mapping is defined
**once**, in `EVENT_ENCODING` (`store/mod.rs`), and everything derives from it: the
`tool_errors` view is generated from the table, readers take column names from
it (`encoding_column`), and the writer's `INSERT` columns for `tool_error` and
`prompt` are spelled from it. The work-event writer is the one place that fills
physical columns without consulting the table, because one statement spans
several kinds; what a row's value means depends on its `kind`, read back
through the table.

A new event kind is therefore a row in the table, not an edit scattered across
view SQL and queries. Round-trip tests
(`tool_errors_view_exposes_named_columns_and_project`,
`file_edits_carry_their_project_session_and_full_path`, …) pin writers against
the same table, so writer and reader cannot disagree about which column holds
what.

The `REFERENCES` annotations above record which column joins which table; they
are **not declared constraints**. SQLite enforces foreign keys only with
`PRAGMA foreign_keys` on, and enabling it would constrain the order a
delete-then-insert re-ingest may touch tables in, for no integrity this
single-writer store lacks. The schema states the relationship; ingest maintains
it (per `source_path`, below).

The store is also a **read surface for arbitrary queries** (`cli.md`: `sql`).
Being plain SQLite over already-extracted facts, any session-analysis slice (the
`optimize` agent chasing a root cause, say) is a `SELECT` away, cheaper and less
error-prone than re-parsing raw transcripts. `sql` opens the db read-only
(`Store::open(path, OpenMode::ReadOnly)`), and views like `tool_errors` keep
queries clean despite the column overloading.

`attrs_json` is the additive escape hatch: a new event kind or surface attribute
lands there without a migration, and graduates to a column only if reports query
it often. That is what keeps "add a new surface" non-migrating.

### Prompt text is referenced, never copied

A `prompt` event stores `(source_path, source_line)`, not the prompt string. The
future skill-extraction layer (clustering recurring prompts into candidate
skills ― `architecture.md`) recovers the text by re-reading that line on demand.
This keeps personal data out of the store (`.claude/rules/session-data-privacy.md`)
while reserving the capability now, so it survives transcript rotation: the
pointer is cheap to keep and the alternative ― discovering later that the text is
gone ― is unrecoverable. (If the source file is rotated away, the pointer simply
resolves to nothing; the event's counts remain.)

### The store file is owner-only

What survives ingestion is still sensitive: file paths, tool-error excerpts,
project names, and pointers back into raw transcripts. The default store is one
well-known user-level path shared by every project (`cli.md`), and the umask
default (commonly a `0755` directory and `0644` file) would expose that
aggregate to other local accounts. `Store::open` therefore creates the directory
`0700` and the store `0600`, matching how the `optimize` briefing (the same
class of data) is written.

The file is created by `Store::open` itself rather than chmod'ed after SQLite
creates it, which keeps the guarantee airtight: the schema write would otherwise
land in a file still carrying the umask mode, and SQLite copies the database's
mode onto its rollback journal, so the journal inherits `0600` for free. An
**existing** store is narrowed too, since a store created under the umask by an
older version is the case that actually exists. Only a regular file is
re-moded: a `--db` pointing at a directory still fails at SQLite, and a store
owned by another account is left alone, since re-moding it is not this
process's call and refusing to run over it would break a store legitimately
created under another user.

Being unable to narrow the store is not swallowed: the PermissionDenied case
(where a foreign-owned, group/world-readable store is in place) warns on
stderr and continues, and a directory or store file that already carries
group/other bits warns the same way. The posture is "warn, never silently
inherit": the tool does not refuse to run (that would break the legitimate
cross-user case above), but the privacy degradation is always visible.

## Surface identity, scope, and the effective join

`surfaces` is keyed `(kind, id, scope, project)` because the same logical
surface can be installed globally and in **any number of projects** (e.g. a
`git-commit` skill in `~/.claude/skills/` and in two projects'
`.claude/skills/`). Events carry only `(surface_kind, surface_id)` ― the
transcript does not reveal which copy was loaded ― but they *do* join a session
whose `project` is known, and Claude Code's own resolution makes a project's
copy shadow the global one **inside that project only**.

So the catalog×usage join is defined per session project: an event from project
P joins P's project row for that `(kind, id)` when one exists, else the global
row. The join stays strictly 1:N-safe ― one event never matches two surface
rows, and one project's usage never inflates another project's copy
(`Store::effective_catalog`). Scope and project are retained on every catalog
row so reports can route a finding to the config layer that owns the fix
(`cli.md` `--scope`). `surfaces.md` and `config-format.md` describe the same
contract.

## Timestamps are stored in UTC

`started_at` / `started_epoch` are UTC; the transcript gives UTC and the store
keeps it. Timezone is a **presentation** concern: the report converts to the
target zone (default JST) when bucketing and displaying (`cli.md`). Storing local
time would corrupt sorting and break portability across machines.

## Project normalization

A worktree directory has its own `cwd-slug` (a `...--wt-feat-x` suffix), so the
same logical project is otherwise scattered across slugs. `sessions.project` is
the **normalized** project (worktree suffix folded to the parent); `sessions.slug`
keeps the raw slug. Reports group by `project` so a project's usage is not split,
while the raw slug stays available for drill-down. The exact folding rule is an
adapter concern documented where it is implemented, but it must preserve a
testable invariant: **one logical project maps to exactly one `project` value,
and folding is idempotent** (folding an already-folded value is a no-op). Getting
this wrong splits or merges a project's usage, which is a correctness bug for
every per-project wedge, not a presentation detail.

`sessions.root` folds by the same rule one level down, on the real path rather
than the slug. The two must agree: the slug form keys on a `--wt-` infix and is
separator-free, so the path form accepts **either separator** ― otherwise a
worktree recorded on Windows folds by `project` while `root` stays split, and
per-root config scanning (`config-format.md`) sees two roots where there is one
checkout.

## Schema evolution is non-destructive

The store outlives its inputs. Claude Code prunes transcripts on its own
schedule, and once a transcript is gone the rows extracted from it are the only
record of that session. An upgrade may therefore never resolve schema drift by
asking for the file to be deleted; `Store::open` migrates in place (`migrate` in
`store/mod.rs`).

The declared schema is the single source of truth for the target shape: it is
applied to a scratch in-memory database, read back through
`PRAGMA table_info`, and the live store is reconciled against that. Adding a
column to the declaration is the whole of a routine migration; there is no
parallel list of ALTER statements to keep in step. Every table the declaration
names must be classified in the migration's convergence lists (`MIGRATED` /
`REBUILT` in `store/mod.rs`); a declared table missing from both fails the migration
naming it, because a silently unclassified table would never be reconciled and
its next added column would break every existing store at `INSERT` time.

Drift is reconciled per object by what that object costs to lose:

- `sessions`, `events`, `ingested_files`, `meta` hold history, so a missing
  column is appended with `ALTER TABLE ADD COLUMN`; existing rows take the
  column's default, which the "report the gap, never substitute" contract below
  covers. A column already present is **compared, not skipped**: matching on
  name alone would pass silently over a retype or changed default and leave the
  store permanently diverged from its declaration.
- `surfaces` is a snapshot of live config rebuilt on every analyze, so a shape
  change drops and recreates it. The shape is compared first, because an
  unchanged catalog must survive reopening or a `--frozen` read would report an
  empty one. When it *is* rebuilt, `analyzed_at` is cleared, so a `--frozen`
  read gets the freshness warning instead of an ordinary header over an empty
  catalog.
- Indexes and views carry no data, so a drifted one is dropped and recreated.
  Only a *drifted* one: `CREATE … IF NOT EXISTS` would keep a stale definition
  (a reader would silently query yesterday's column mapping), but reindexing
  `events` on every open is not free, so the stored `CREATE` statement is
  compared against the declared one and matches are left untouched. Objects the
  declaration does not name are also left alone: the store is meant to be
  explored with ad-hoc SQL, so a helper view someone saved into the file is
  theirs to keep.

Indexes and views are **declared apart from the tables**, in their own
constants, and applied only after column reconciliation. Declared alongside a
table, they would be replayed by the same `execute_batch` that creates missing
tables, before any column was added, so an index over a column a future release
introduces would hit `no such column` on a legacy store and take the whole
migration down. Deriving the names to reconcile from the declaration also
removes a hand-kept list, which would drift: a declared object missing from it
would collide with its own `CREATE` on the second open.

The whole migration runs in one transaction, so a bail partway never leaves a
half-migrated store.

The version check is part of that atomicity: the guard reads `user_version`
inside the same `BEGIN IMMEDIATE` transaction that stamps it, so a newer build
cannot slip its DDL past the guard in the window between check and stamp. The
read-write open sets a busy timeout (5 s) before migrating, so a concurrent
opener waits briefly and surfaces a clear lock error instead of failing with a
bare `database is locked`; the read-only path sets none.

`PRAGMA user_version` carries `SCHEMA_VERSION`. Reconciliation only ever adds,
so routine additive changes do not bump it; it exists for what reconciliation
cannot express. A store stamped **newer** than this build is refused rather than
written to, since this build's shape would overwrite changes it does not
understand. It is also the seam for a change reconciliation cannot cover (a
column removed, retyped, or given a new meaning): reconciliation *fails* on such
drift rather than ignoring it, so the seam cannot be skipped by accident, and the
migration written to fill it must state what it discards.

`OpenMode` states which guarantee applies: `ReadWrite` creates the file, narrows
it to owner-only, and migrates in place; `ReadOnly` (the `sql` command) does
none of the three. An ad-hoc query against a store older than this build
surfaces SQLite's own missing-column error; any read command opens `ReadWrite`
and migrates first.

## Incremental ingest

Transcripts are append-only and **active sessions keep growing**, so re-running
`analyze` must be cheap and idempotent.

### One transcript, one batch

`Store::ingest(IngestBatch)` is the only way rows enter the store. A caller
submits everything one transcript yielded (session meta, spans, usage events,
prompts, subagent runs, tool errors, work events) and the store applies it in
**one transaction, in its own order**: the session row (insert-or-replace), the
delete of everything this file previously put in `events`, each child set, then
the spawn-cost settle that reads the spawn events just written. No entry point
expresses an order callers must remember. Re-ingesting the same file is the same
batch twice, which the replace contract makes idempotent, and atomicity means a
mid-ingest failure leaves the previous rows intact rather than a half-replaced
file.

- **Skip.** Before ingesting a file, compare `(mtime, size)` against
  `ingested_files`: unchanged skips, changed or new re-ingests. `mtime` is
  **epoch nanoseconds** (`adapter::harness::file_fingerprint`), because
  whole-second truncation made a same-second, same-size edit invisible. Nanos
  fit an `INTEGER` until 2262. It is a cheap change detector, not a content
  hash: a touch without a byte change triggers a harmless idempotent replace,
  and any byte change moves size or mtime.
- **Replace, not append.** Delete all `events` whose `source_path` equals this
  file, re-extract from the whole file, and insert. This is why
  `events.source_path` exists, and it avoids duplicates when a still-open
  session is analyzed twice.
- **Subagent runs** replace on the same key, the *parent* transcript's path, so
  one session's runs are rebuilt as a set and a run whose own transcript
  vanished does not survive as an orphan. Subagent files are deliberately **not**
  fingerprinted: a subagent's completion appends its `tool_result` to the main
  transcript, so the parent's fingerprint already moves when a child finishes.
  Stat-ing every file under `subagents/` on every run would only buy the case of
  a session killed between the child finishing and that write, where the
  session's data is incomplete anyway. Filling an `agent_spawn` row's cost from
  those runs is scoped to the same key: a call id is unique **within** a session
  but not across sessions (a resumed or copied transcript can carry one over),
  so an unscoped join would pick an arbitrary row and sum another session's runs
  on top.
- **Catalog.** `surfaces` is rebuilt wholesale each run from current config: the
  catalog is a snapshot of *now*, not an accumulation (`surfaces.md`).

### Invalidating the skip

The fingerprint is keyed on the *file*, not on what saniwalens knew how to
extract from it. Alone, the skip never expires: a closed session never changes,
so its rows would stay pinned to whatever the first saniwalens understood (an
event kind added later stays missing, a new field stays NULL, a classifier's new
pattern never reaches it).

`meta.analyzer_version` is the invalidation. It names the **analysis semantics**
the stored rows were produced by; a run whose version differs from the store's
ignores the skip, re-ingests every transcript once (replace, so it is safe to
repeat), and stamps the new value. Bump it whenever extraction or classification
changes what an *unchanged* transcript yields. It is deliberately not the crate
version, so a release that changes no analysis costs nobody a full re-analyze.
The Claude Code pair lives with its mapping in `adapter::transcript`
(`CLAUDE_ANALYZER_META_KEY` / `CLAUDE_ANALYZER_VERSION`); each other harness
carries its own beside its mapping (`harness.md`), and
`Store::is_analyzer_current(key, version)` compares whatever it is handed, so
the store owns no harness's key.

### Report the gap, never substitute

Invalidation only recovers what the transcript holds. Where a signal was never
recorded, a reader **reports the gap or drops the row** and never substitutes
something plausible:

- `overhead` reports the always-on floor as unavailable when no `session_start`
  was observed, rather than falling back to a skill span's `ctx_start`
  (`surfaces.md`).
- Thrash detection excludes `file_edit` rows with no `target`, rather than
  falling back to the basename in `surface_id` (`events.md`).

Both fallbacks would look like working output while silently restoring the bug
the field exists to prevent, which is worse than an empty report. Such a result
is a fact about the input: re-analyzing cannot conjure a signal the transcript
never recorded, and that transcript may be gone.

### Reading files that grow mid-run

Active sessions are appended to while `analyze` runs, so the reader tolerates a
**torn read**: a partial final line is skipped (JSONL append is not atomic at
line granularity), never parsed as a broken record. The fingerprint is captured
**after** the read, recording the size actually consumed, so a file that grew
during the read shows a changed `(mtime, size)` next run and is re-ingested with
its tail. Stamping before the read would pair the post-growth size with
pre-growth events and permanently skip the tail. The stamp is further gated on a
post-read re-stat (`restat_matches` in `adapter::harness`): it is written only
when the re-read fingerprint equals the pre-read one, so a mid-run append fails
closed to "ingest without stamping". Row-backed fingerprints (`opencode`'s
`time_updated_ms` plus message count, kilocode's task fold) re-query after
ingest under the same gate.

## Why a dimension + catalog + spine, not one span table

An earlier design had a single skill-centric `spans` table. It could not hold
rules, hooks, MCP, or prompts without contortion, and it conflated the catalog
(what is installed) with usage (what ran). Splitting into `sessions` (dimension)
+ `surfaces` (catalog) + `events` (usage) lets every configuration surface share
one usage spine and one catalog shape, and makes the optimization analysis a
join rather than a special case per surface ― see `surfaces.md`.

`subagent_runs` sits beside the spine rather than in it because a run is not an
event *of the session* ― it happened in another transcript, on another thread,
and its cost was measured there. Putting it in `events` with `surface_kind =
'agent'` would double-count every spawn in the catalog×usage join, which counts
rows per surface. The spine keeps the main thread's spawn; the run keeps the
cost; `events.target` joins them.
