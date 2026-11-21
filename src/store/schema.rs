pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id              TEXT PRIMARY KEY,
    project         TEXT NOT NULL,
    slug            TEXT NOT NULL,
    root            TEXT NOT NULL DEFAULT '',
    source_path     TEXT NOT NULL,
    started_at      TEXT NOT NULL,
    sub_tokens      INTEGER NOT NULL DEFAULT 0,
    sub_agent_count INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS events (
    id            INTEGER PRIMARY KEY,
    session_id    TEXT NOT NULL,
    source_path   TEXT NOT NULL,
    source_line   INTEGER,
    kind          TEXT NOT NULL,
    surface_kind  TEXT,
    surface_id    TEXT,
    source        TEXT,
    started_at    TEXT NOT NULL,
    started_epoch INTEGER NOT NULL,
    duration_sec  REAL NOT NULL,
    out_tokens    INTEGER NOT NULL,
    ctx_growth    INTEGER NOT NULL,
    ctx_start     INTEGER NOT NULL,
    ctx_peak      INTEGER NOT NULL,
    model         TEXT,
    target        TEXT,
    sub_tokens           INTEGER NOT NULL DEFAULT 0,
    sub_agent_count      INTEGER NOT NULL DEFAULT 0,
    sub_tokens_estimated INTEGER NOT NULL DEFAULT 0,
    is_trailing          INTEGER NOT NULL DEFAULT 0
);
-- One row per subagent execution: which agent type ran, and what its own output
-- cost. The session total says how much subagent work cost; this says *whose*.
-- `source_path` is the parent session's transcript — the ingest delete key, as
-- for events — while `run_path` points at the subagent's own transcript.
CREATE TABLE IF NOT EXISTS subagent_runs (
    id                INTEGER PRIMARY KEY,
    session_id        TEXT NOT NULL,
    source_path       TEXT NOT NULL,
    run_path          TEXT NOT NULL,
    agent             TEXT,
    agent_id          TEXT NOT NULL,
    tool_use_id       TEXT,
    root_tool_use_id  TEXT,
    parent_agent_id   TEXT,
    spawn_depth       INTEGER NOT NULL DEFAULT 1,
    model             TEXT,
    prompt_id         TEXT,
    out_tokens        INTEGER NOT NULL DEFAULT 0,
    started_at        TEXT NOT NULL,
    started_epoch     INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS surfaces (
    kind          TEXT NOT NULL,
    id            TEXT NOT NULL,
    scope         TEXT NOT NULL,
    project       TEXT NOT NULL DEFAULT '',
    config_path   TEXT,
    static_tokens INTEGER,
    startup_tokens INTEGER,
    load_mode     TEXT NOT NULL,
    attrs_json    TEXT,
    description   TEXT,
    PRIMARY KEY (kind, id, scope, project)
);
-- Analyze-run metadata (analyzed_at, projects_dir, config_dir) so read
-- commands can report freshness and re-run the analysis with the same roots,
-- plus analyzer_version, which invalidates the ingest fingerprints above.
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Incremental-ingest fingerprints: a transcript whose (mtime, size) matches is
-- skipped on re-analyze (see docs/specs/storage.md). `mtime` is epoch
-- nanoseconds; values stamped before the nanos change never match again, so
-- the first run after the upgrade re-ingests once, idempotently.
CREATE TABLE IF NOT EXISTS ingested_files (
    path  TEXT PRIMARY KEY,
    mtime INTEGER NOT NULL,
    size  INTEGER NOT NULL
);
";

/// Indexes are declared apart from the tables so `migrate` can apply them only
/// after reconciling columns. Declared alongside the table, an index over a
/// column a future release adds would be replayed against a legacy shape, fail on
/// the missing column, and take the whole migration down with it.
pub(super) const INDEX_SCHEMA: &str = "
CREATE INDEX events_by_epoch ON events(started_epoch);
CREATE INDEX events_by_surface ON events(surface_kind, surface_id);
CREATE INDEX subagent_runs_by_agent ON subagent_runs(agent);
CREATE INDEX subagent_runs_by_source ON subagent_runs(source_path);
";

/// Stamped into `PRAGMA user_version`. Bumped only for a change `migrate` cannot
/// reconcile — an existing column removed, retyped, or given a new meaning.
/// Additive drift (a new column, table, or view) needs no bump: `migrate`
/// converges any older store onto the declared schema in place.
pub(super) const SCHEMA_VERSION: i64 = 1;

/// Tables holding analysis history. Claude Code prunes transcripts on its own
/// retention schedule, after which these rows are the only surviving record of
/// those sessions — so a shape change adds columns in place, never rebuilds.
pub(super) const MIGRATED_TABLES: [&str; 5] = [
    "sessions",
    "events",
    "ingested_files",
    "meta",
    "subagent_runs",
];

/// Tables regenerated wholesale from live config on every analyze
/// (`replace_surfaces`), so a shape change costs nothing to drop and recreate.
pub(super) const REBUILT_TABLES: [&str; 1] = ["surfaces"];
