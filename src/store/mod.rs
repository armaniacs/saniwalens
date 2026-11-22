//! SQLite store: persist extracted spans as events and query them back. This
//! layer knows SQL but nothing about raw Claude Code formats. See
//! `docs/specs/storage.md`.
//!
//! The schema tracks the spec's `events` table; `attrs_json` is the one column
//! deferred until a report needs it.

use anyhow::{Context, Result};
use rusqlite::Connection;
use rusqlite::params_from_iter;

pub use crate::core::optimize::AgentCost;
use crate::core::period::{TimeRange, WindowPair};
use crate::core::span::{SessionStart, Source, Span};
use crate::core::subagent::SubagentRun;
use crate::core::surface::Surface;
use crate::core::thrash::FileEdit;
use crate::core::trend::{KindRollup, SkillUsage, TrendWindowAggregates, TwoWindow};
use crate::core::usage::UsageEvent;

mod briefing;
mod migrate;
mod permission;
mod schema;
#[cfg(test)]
mod tests;

pub(crate) use permission::create_private_dir_all;
#[cfg(unix)]
pub(crate) use permission::unprivate_dir_warning;

use schema::SCHEMA;

/// A kind-specific meaning an event kind carries in a generic `events` column.
/// Its `Display` is the name such a meaning gets when projected (the
/// `tool_errors` view's aliases), so the table reads as the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventRole {
    /// `tool_error`: the friction category (`edit-precondition`, …).
    Category,
    /// `tool_error`: a short excerpt of the error text.
    Excerpt,
    /// `tool_error`: the tool that produced the failure.
    OriginatingTool,
    /// `tool_error`: the failed call's subject (file path / command).
    FailureTarget,
    /// `file_edit`: the basename hotspot rankings group on.
    FileBasename,
    /// `file_edit`: the edited file's full path.
    FilePath,
    /// `bash_cmd`: the command's leading word.
    CommandWord,
    /// `prompt`: the prompt's behavioral class.
    PromptBehavior,
    /// `agent_spawn`: the spawn call's id, joining `subagent_runs`.
    SpawnCallId,
}

impl std::fmt::Display for EventRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EventRole::Category => "category",
            EventRole::Excerpt => "excerpt",
            EventRole::OriginatingTool => "tool",
            EventRole::FailureTarget => "target",
            EventRole::FileBasename => "basename",
            EventRole::FilePath => "path",
            EventRole::CommandWord => "word",
            EventRole::PromptBehavior => "behavior",
            EventRole::SpawnCallId => "call_id",
        })
    }
}

/// A generic `events` column that can carry a kind-specific meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventColumn {
    SurfaceId,
    Source,
    Model,
    Target,
}

impl EventColumn {
    pub fn name(&self) -> &'static str {
        match self {
            EventColumn::SurfaceId => "surface_id",
            EventColumn::Source => "source",
            EventColumn::Model => "model",
            EventColumn::Target => "target",
        }
    }
}

impl std::fmt::Display for EventColumn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// **The single place** a kind's overloaded-column meaning is defined
/// (`docs/specs/storage.md`). The `tool_errors` view is generated from this
/// table and the readers take their column names from it, so a new event kind
/// is one row here — never an edit spread across view SQL and queries.
///
/// Kinds whose columns carry only their primary meaning (a `skill_invocation`'s
/// `surface_id` *is* the joined skill) need no entry. The writers fill the
/// physical columns without consulting the table — a `work_events` statement
/// spans several kinds at once — and their mapping is pinned against the table
/// by the round-trip tests (`tool_errors_view_exposes_named_columns_and_project`,
/// `file_edits_carry_their_project_session_and_full_path`, …).
pub const EVENT_ENCODING: &[(&str, &[(EventRole, EventColumn)])] = &[
    (
        "tool_error",
        &[
            (EventRole::Category, EventColumn::SurfaceId),
            (EventRole::Excerpt, EventColumn::Source),
            (EventRole::OriginatingTool, EventColumn::Model),
            (EventRole::FailureTarget, EventColumn::Target),
        ],
    ),
    (
        "file_edit",
        &[
            (EventRole::FileBasename, EventColumn::SurfaceId),
            (EventRole::FilePath, EventColumn::Target),
        ],
    ),
    (
        "file_read",
        &[
            (EventRole::FileBasename, EventColumn::SurfaceId),
            (EventRole::FilePath, EventColumn::Target),
        ],
    ),
    (
        "bash_cmd",
        &[(EventRole::CommandWord, EventColumn::SurfaceId)],
    ),
    (
        "prompt",
        &[(EventRole::PromptBehavior, EventColumn::Source)],
    ),
    (
        "agent_spawn",
        &[(EventRole::SpawnCallId, EventColumn::Target)],
    ),
];

/// The column a `(kind, role)` meaning rides in. Panics on an entry missing
/// from [`EVENT_ENCODING`] — every caller's pair is pinned by the tests below,
/// so a panic here is a programming bug, not input.
pub fn encoding_column(kind: &str, role: EventRole) -> &'static str {
    let (_, roles) = EVENT_ENCODING
        .iter()
        .find(|(k, _)| *k == kind)
        .unwrap_or_else(|| panic!("event kind {kind:?} is not in EVENT_ENCODING"));
    roles
        .iter()
        .find(|(r, _)| *r == role)
        .unwrap_or_else(|| panic!("{kind} carries no {role} in EVENT_ENCODING"))
        .1
        .name()
}

/// The `tool_errors` view, generated from the encoding table's `tool_error`
/// entry — the view and the table that defines the meaning cannot drift, and a
/// kind added to the table leaves this definition untouched.
fn tool_errors_view_sql(encoding: &[(&str, &[(EventRole, EventColumn)])]) -> String {
    let (_, roles) = encoding
        .iter()
        .find(|(k, _)| *k == "tool_error")
        .expect("the tool_errors view is defined by its tool_error entry");
    // One projection per role, padded so the generated text matches what a
    // store built before this table already holds — no view churn on open.
    let projected: Vec<String> = roles
        .iter()
        .map(|(role, column)| format!("e.{column:<18} AS {role},"))
        .collect();
    format!(
        "-- A clean read view over tool_error events: the friction columns are
-- overloaded onto generic event columns (defined once in EVENT_ENCODING,
-- which this view is generated from), so the view names them and joins
-- the project — an ad-hoc SQL query asks for any slice without knowing
-- the encoding. `project LIKE '%--wt%'` distinguishes a worktree from the main checkout.
CREATE VIEW tool_errors AS
SELECT e.session_id        AS session_id,
       s.project           AS project,
       {}
       e.started_epoch      AS started_epoch
FROM events e JOIN sessions s ON e.session_id = s.id
WHERE e.kind = 'tool_error';",
        projected.join("\n       ")
    )
}

/// Views are pure definitions over the tables, declared apart from them for the
/// same reason as indexes plus one of their own: `CREATE VIEW IF NOT EXISTS`
/// would leave an older store's stale definition in place, and a reader would
/// silently query yesterday's column mapping. The tool_error view is generated
/// from [`EVENT_ENCODING`] so the definition and its meaning stay one thing.
fn view_schema() -> String {
    tool_errors_view_sql(EVENT_ENCODING)
}

/// Identity and provenance of one analyzed session.
pub struct SessionMeta {
    pub id: String,
    pub project: String,
    pub slug: String,
    /// The real directory the session started in (from the transcript's `cwd`,
    /// worktree folded) — empty when no record carried one. This is what
    /// project-config scanning walks; the slug is too lossy to reconstruct it.
    pub root: String,
    pub source_path: String,
    /// Total output tokens across this session's subagent transcripts, and how
    /// many subagents it spawned.
    pub sub_tokens: i64,
    pub sub_agent_count: i64,
    /// When this session started and the context it started with, when the
    /// transcript carried an assistant record to read it from — the only honest
    /// always-on-floor observation (`core::span::session_start`). Stored as a
    /// `session_start` event so `overhead` can distinguish "no observation" from
    /// a fabricated zero.
    pub start: Option<SessionStart>,
}

/// One catalogued surface row with its **effective** usage: the invocations
/// attributed to this row under per-project shadowing (a project row absorbs
/// its own project's events; the global row keeps everyone else's). See
/// `docs/specs/surfaces.md`.
#[derive(Debug, PartialEq, serde::Serialize)]
pub struct CatalogEntry {
    pub kind: String,
    pub id: String,
    /// `global` | `project`.
    pub scope: String,
    /// The owning project's normalized slug; empty for global rows.
    pub project: String,
    /// The whole definition's weight — for a skill or agent, what its body
    /// costs when invoked, not what having it installed costs.
    pub static_tokens: Option<i64>,
    /// What this surface adds to every session's startup context; `None` when
    /// unknown (an MCP server's tool schema).
    pub startup_tokens: Option<i64>,
    pub load_mode: String,
    pub uses: i64,
    /// Rule `paths:` globs, parsed from `attrs_json` (empty for every other
    /// kind, and for rows written before globs were recorded). The never-fires
    /// wedge reads these, never the raw JSON.
    pub paths: Vec<String>,
    /// A skill/agent/command's own description (PBI 15), for the
    /// redundant-purpose wedge. `None` when the kind declares none or the row
    /// predates the column.
    pub description: Option<String>,
}

impl From<CatalogEntry> for crate::core::findings::CatalogRow {
    /// The fetch boundary's projection: the store row keeps `paths` and
    /// `description` for the wedges that need them, while the assembly's row
    /// keeps the eight fields the briefing and the inventory share. One
    /// `From` here replaces the per-caller field-by-field mapping.
    fn from(e: CatalogEntry) -> Self {
        crate::core::findings::CatalogRow {
            kind: e.kind,
            id: e.id,
            scope: e.scope,
            project: e.project,
            startup_tokens: e.startup_tokens,
            static_tokens: e.static_tokens,
            load_mode: e.load_mode,
            uses: e.uses,
        }
    }
}

/// The shared `surfaces`-row → [`CatalogEntry`] mapping: the unbounded query
/// and its bounded briefing twin must not drift on column order or attrs
/// parsing, so both hand their rows to this one function.
fn catalog_entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CatalogEntry> {
    let attrs: Option<String> = row.get(7)?;
    Ok(CatalogEntry {
        kind: row.get(0)?,
        id: row.get(1)?,
        scope: row.get(2)?,
        project: row.get(3)?,
        static_tokens: row.get(4)?,
        startup_tokens: row.get(5)?,
        load_mode: row.get(6)?,
        uses: row.get(9)?,
        paths: attrs
            .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
            .unwrap_or_default(),
        description: row.get(8)?,
    })
}

// The briefing's row types are owned by the pure core (core::findings), which
// consumes them; re-exported here so the `crate::store::...` paths of the
// query methods and their callers stay stable.
pub use crate::core::findings::{FrictionCell, ProjectSlug, SessionRoot, ToolBreakdown};

/// One session's cost shape: what it spent, how long it ran, and how often
/// it stumbled. Zero-event and zero-friction sessions read 0, never vanish.
#[derive(Debug, PartialEq)]
pub struct SessionRow {
    pub id: String,
    pub project: String,
    pub slug: String,
    pub started_at: String,
    pub out_tokens: i64,
    pub duration_sec: i64,
    pub friction: i64,
}

/// One skill event's cost, with its UTC start, for time bucketing.
#[derive(Debug, PartialEq, serde::Serialize)]
pub struct EventCost {
    pub started_epoch: i64,
    pub out_tokens: i64,
    pub ctx_growth: i64,
    pub duration_sec: f64,
}

/// What opening a store is allowed to do — the guarantees that used to be
/// spread across `open` / `open_readonly` names are now one open path with a
/// typed mode (`docs/specs/storage.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// Create the file if needed, narrow it to owner-only, and migrate the
    /// schema in place. Every command but `sql`.
    ReadWrite,
    /// Open an existing file only: never create, never chmod, never migrate.
    /// The `sql` command's ad-hoc query must not mutate the derived store, so
    /// an old store's missing column surfaces as SQLite's own error.
    ReadOnly,
}

pub struct Store {
    conn: Connection,
}

/// Everything one transcript yields, submitted as a single unit
/// (`Store::ingest`). The store applies it inside one transaction in its own
/// order, so the sequence the old five functions demanded of callers — the
/// session row first, always — is unreachable rather than remembered
/// (`docs/specs/storage.md`).
pub struct IngestBatch<'a> {
    pub session: &'a SessionMeta,
    pub spans: &'a [Span],
    pub usage: &'a [UsageEvent],
    pub prompts: &'a [(usize, i64, &'a str)],
    pub subagent_runs: &'a [SubagentRun],
    pub tool_errors: &'a [(i64, &'a str, &'a str, &'a str, &'a str)],
    pub work_events: &'a [(i64, &'a str, &'a str, Option<&'a str>)],
}

impl<'a> IngestBatch<'a> {
    /// The batch's floor: the session row plus its spans and usage events.
    /// Every child kind starts empty and is added by its builder method.
    pub fn new(session: &'a SessionMeta, spans: &'a [Span], usage: &'a [UsageEvent]) -> Self {
        Self {
            session,
            spans,
            usage,
            prompts: &[],
            subagent_runs: &[],
            tool_errors: &[],
            work_events: &[],
        }
    }

    pub fn prompts(mut self, prompts: &'a [(usize, i64, &'a str)]) -> Self {
        self.prompts = prompts;
        self
    }

    pub fn subagent_runs(mut self, runs: &'a [SubagentRun]) -> Self {
        self.subagent_runs = runs;
        self
    }

    pub fn tool_errors(mut self, errors: &'a [(i64, &'a str, &'a str, &'a str, &'a str)]) -> Self {
        self.tool_errors = errors;
        self
    }

    pub fn work_events(mut self, events: &'a [(i64, &'a str, &'a str, Option<&'a str>)]) -> Self {
        self.work_events = events;
        self
    }
}

/// A briefing fetch cap as a SQLite `LIMIT` bind: saturates instead of
/// wrapping on platforms where `usize` is wider than `i64`.
fn saturating_limit(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

/// The SQL fragment (a leading ` AND `) and epoch binds that narrow `column`
/// to `range`. Both ends are inclusive — `--until` counts the whole day it
/// names — and an unbounded range renders nothing, so a query's text and its
/// results are unchanged without the period flags. **The one place** the
/// report views' window becomes SQL: queries assemble it into their WHERE
/// rather than growing per-query branches.
fn range_clause(range: TimeRange, column: &str) -> (String, Vec<i64>) {
    let mut clauses = Vec::new();
    let mut binds = Vec::new();
    if let Some(start) = range.start {
        clauses.push(format!("{column} >= ?"));
        binds.push(start);
    }
    if let Some(end) = range.end {
        clauses.push(format!("{column} <= ?"));
        binds.push(end);
    }
    if clauses.is_empty() {
        (String::new(), binds)
    } else {
        (format!(" AND {}", clauses.join(" AND ")), binds)
    }
}

/// The bind vector a range+LIMIT reader passes to `params_from_iter`: the
/// reader's own leading cells (`extra`, e.g. `work_counts_briefing`'s kind
/// cell) first, then the range binds as `Value`s, then the LIMIT bind. The
/// [`saturating_limit`] boundary contract lives here so the briefing
/// readers assemble their parameters without re-deriving it.
fn range_params(
    binds: &[i64],
    extra: impl IntoIterator<Item = rusqlite::types::Value>,
    limit: usize,
) -> Vec<rusqlite::types::Value> {
    let mut params: Vec<rusqlite::types::Value> = extra.into_iter().collect();
    params.extend(binds.iter().copied().map(rusqlite::types::Value::from));
    params.push(rusqlite::types::Value::from(saturating_limit(limit)));
    params
}

/// The same window for a uniform RFC 3339 UTC timestamp column
/// (`sessions.started_at`): lexicographic order is chronological order for
/// the one format the writer stamps (`epoch_ms_to_rfc3339`), and the upper
/// bound is the *exclusive* next second, so a millisecond fraction inside the
/// final second of the named day stays in. A session the writer could not
/// stamp (empty timestamp, no observable start) is **excluded** from any
/// windowed figure rather than guessed into it.
fn range_clause_utc_text(range: TimeRange, column: &str) -> (String, Vec<String>) {
    let mut clauses = Vec::new();
    let mut binds = Vec::new();
    if let Some(start) = range.start {
        clauses.push(format!("{column} >= ?"));
        binds.push(epoch_to_rfc3339_utc(start));
    }
    if let Some(end) = range.end {
        clauses.push(format!("{column} < ?"));
        binds.push(epoch_to_rfc3339_utc(end + 1));
    }
    if clauses.is_empty() {
        (String::new(), binds)
    } else {
        clauses.push(format!("{column} <> ''"));
        (format!(" AND {}", clauses.join(" AND ")), binds)
    }
}

/// The RFC 3339 UTC text of an epoch second, in the one format
/// `sessions.started_at` is stamped with.
fn epoch_to_rfc3339_utc(epoch: i64) -> String {
    crate::time::epoch_to_rfc3339_utc(epoch)
}

impl Store {
    /// Open the store at `path` under a typed guarantee ([`OpenMode`]): the one
    /// open path, with what it may do — create, chmod, migrate — spelled by the
    /// mode argument rather than by which of several constructors was called.
    ///
    /// In `ReadWrite` mode the directory and file are **owner-only**: the store
    /// aggregates transcript-derived data — file paths, error excerpts, project
    /// names — for every project at one well-known user-level path, so the umask
    /// default (commonly a group/world-readable `0755` directory and `0644`
    /// file) would expose it to other local accounts.
    pub fn open(path: impl AsRef<std::path::Path>, mode: OpenMode) -> Result<Self> {
        match mode {
            OpenMode::ReadWrite => {
                let path = path.as_ref();
                if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::MetadataExt;

                        if let Ok(meta) = std::fs::metadata(parent)
                            && meta.is_dir()
                            && let Some(line) =
                                permission::unprivate_dir_warning(parent, meta.mode())
                        {
                            eprintln!("{line}");
                        }
                    }
                    permission::create_private_dir_all(parent)
                        .with_context(|| format!("create store directory {}", parent.display()))?;
                }
                permission::ensure_owner_only(path)?;
                Self::from_connection(Connection::open(path)?)
            }
            OpenMode::ReadOnly => {
                // No create, no chmod, no migration: an ad-hoc query must never
                // mutate the derived store (and an absent db is an error, not a
                // fresh empty one).
                let conn = Connection::open_with_flags(
                    path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )?;
                Ok(Self { conn })
            }
        }
    }

    /// An ephemeral in-memory store, for tests.
    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    /// Run an arbitrary read query, returning `(column names, rows)` with every
    /// cell stringified. Powers the `sql` command — the store's own query
    /// surface, so the analyzed data is reachable for any slice without
    /// re-parsing transcripts. Opened read-only by the caller; a non-read
    /// statement simply errors at SQLite.
    pub fn query(&self, sql: &str) -> Result<(Vec<String>, Vec<Vec<String>>)> {
        let mut stmt = self.conn.prepare(sql)?;
        let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
        let ncol = columns.len();
        let rows = stmt
            .query_map([], |row| {
                (0..ncol)
                    .map(|i| Ok(value_to_string(row.get_ref(i)?)))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((columns, rows))
    }

    /// Like `query`, but bounded: at most `limit` rows are materialized (plus
    /// one probe row to detect the cut). Returns the truncation flag alongside
    /// so the caller can warn instead of silently passing a partial slice off
    /// as complete. `None` means no bound — only the `sql` command's opt-out
    /// uses that, since every other caller asks for aggregated (small) rows.
    pub fn query_limited(
        &self,
        sql: &str,
        limit: Option<usize>,
    ) -> Result<(Vec<String>, Vec<Vec<String>>, bool)> {
        let mut stmt = self.conn.prepare(sql)?;
        let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
        let ncol = columns.len();
        let take = limit.map_or(usize::MAX, |n| n.saturating_add(1));
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut mapped = stmt.query_map([], |row| {
            (0..ncol)
                .map(|i| Ok(value_to_string(row.get_ref(i)?)))
                .collect::<rusqlite::Result<Vec<_>>>()
        })?;
        for row in mapped.by_ref().take(take) {
            rows.push(row?);
        }
        let truncated = limit.is_some_and(|n| rows.len() > n);
        if truncated {
            rows.pop();
        }
        Ok((columns, rows, truncated))
    }

    /// Like `query`, but with SQLite's types preserved as JSON values — the
    /// machine half of the output contract (`--format json`): integers and
    /// reals stay numbers, NULL stays null, so a consumer never re-parses
    /// strings.
    pub fn query_json(&self, sql: &str) -> Result<(Vec<String>, Vec<Vec<serde_json::Value>>)> {
        let mut stmt = self.conn.prepare(sql)?;
        let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
        let ncol = columns.len();
        let rows = stmt
            .query_map([], |row| {
                (0..ncol)
                    .map(|i| Ok(value_to_json(row.get_ref(i)?)))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((columns, rows))
    }

    /// Like [`Store::query_limited`], but with SQLite's types preserved as
    /// JSON values. The machine contract keeps its bare-array shape; the
    /// truncation flag rides stderr beside it, like the table path.
    pub fn query_json_limited(
        &self,
        sql: &str,
        limit: Option<usize>,
    ) -> Result<(Vec<String>, Vec<Vec<serde_json::Value>>, bool)> {
        let mut stmt = self.conn.prepare(sql)?;
        let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
        let ncol = columns.len();
        let take = limit.map_or(usize::MAX, |n| n.saturating_add(1));
        let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
        let mut mapped = stmt.query_map([], |row| {
            (0..ncol)
                .map(|i| Ok(value_to_json(row.get_ref(i)?)))
                .collect::<rusqlite::Result<Vec<_>>>()
        })?;
        for row in mapped.by_ref().take(take) {
            rows.push(row?);
        }
        let truncated = limit.is_some_and(|n| rows.len() > n);
        if truncated {
            rows.pop();
        }
        Ok((columns, rows, truncated))
    }

    fn from_connection(mut conn: Connection) -> Result<Self> {
        // Must be set before `migrate` begins its transaction: BEGIN IMMEDIATE
        // only waits for a concurrent writer's lock while a busy timeout is
        // installed — without one, contended opens die with "database is
        // locked" at the guard instead of waiting out the other connection.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate::migrate(&mut conn, SCHEMA)?;
        Ok(Self { conn })
    }

    /// Ingest one transcript's extraction as a single unit: one transaction,
    /// applied in the store's own order (session row → replace the file's rows
    /// → child rows → spawn settle). The old five-function API left that order
    /// to the caller's convention — `ingest_session` had to run first or its
    /// delete silently dropped the children — so the order now lives inside
    /// this one entry point (`docs/specs/storage.md`).
    pub fn ingest(&mut self, batch: IngestBatch<'_>) -> Result<()> {
        let tx = self.conn.transaction()?;
        Self::write_session(&tx, batch.session, batch.spans, batch.usage)?;
        Self::write_prompts(&tx, batch.session, batch.prompts)?;
        Self::write_subagent_runs(&tx, batch.session, batch.subagent_runs)?;
        Self::write_tool_errors(&tx, batch.session, batch.tool_errors)?;
        Self::write_work_events(&tx, batch.session, batch.work_events)?;
        tx.commit()?;
        Ok(())
    }

    /// The session row plus the replace of everything this file put into
    /// `events` — the delete-then-insert that makes re-ingest "replace, not
    /// append" (`docs/specs/storage.md`), owned by the batch so no caller can
    /// run it in the wrong order. `spans` are skill executions with cost;
    /// `usage` are point events (agent spawns, MCP tool calls) counted for the
    /// catalog join.
    fn write_session(
        tx: &rusqlite::Transaction<'_>,
        session: &SessionMeta,
        spans: &[Span],
        usage: &[UsageEvent],
    ) -> Result<()> {
        // The session's first assistant record precedes any span, so it is the
        // better start when present; the earliest span is the fallback for a
        // transcript that carried no assistant record.
        let started_epoch_ms = session
            .start
            .map(|start| start.timestamp_ms)
            .into_iter()
            .chain(spans.iter().map(|span| span.started_epoch_ms))
            .min();
        let started_at = started_epoch_ms
            .map(epoch_ms_to_rfc3339)
            .unwrap_or_default();

        tx.execute(
            "INSERT OR REPLACE INTO sessions
               (id, project, slug, root, source_path, started_at, sub_tokens, sub_agent_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (
                &session.id,
                &session.project,
                &session.slug,
                &session.root,
                &session.source_path,
                &started_at,
                session.sub_tokens,
                session.sub_agent_count,
            ),
        )?;
        tx.execute(
            "DELETE FROM events WHERE source_path = ?1",
            (&session.source_path,),
        )?;
        // One `session_start` event per session that carried an observable
        // start context — the always-on floor `overhead` reports. It is stamped
        // with when the session began, not left at the epoch: `sql` exposes raw
        // event rows, and a 1970 timestamp there is worse than no row at all.
        if let Some(start) = session.start {
            tx.execute(
                "INSERT INTO events
                   (session_id, source_path, kind, started_at, started_epoch,
                    duration_sec, out_tokens, ctx_growth, ctx_start, ctx_peak)
                 VALUES (?1, ?2, 'session_start', ?3, ?4, 0, 0, 0, ?5, ?5)",
                (
                    &session.id,
                    &session.source_path,
                    epoch_ms_to_rfc3339(start.timestamp_ms),
                    start.timestamp_ms / 1000,
                    start.ctx,
                ),
            )?;
        }
        for span in spans {
            tx.execute(
                "INSERT INTO events
                   (session_id, source_path, kind, surface_kind, surface_id, source,
                    started_at, started_epoch, duration_sec, out_tokens, ctx_growth,
                    ctx_start, ctx_peak, model,
                    sub_tokens, sub_agent_count, sub_tokens_estimated, is_trailing)
                 VALUES (?1, ?2, 'skill_invocation', 'skill', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                (
                    &session.id,
                    &session.source_path,
                    &span.skill,
                    source_label(span.source),
                    epoch_ms_to_rfc3339(span.started_epoch_ms),
                    span.started_epoch_ms / 1000,
                    span.duration_sec,
                    span.out_tokens,
                    span.ctx_growth,
                    span.ctx_start,
                    span.ctx_peak,
                    &span.model,
                    span.sub_tokens,
                    span.sub_agent_count,
                    span.sub_tokens_estimated as i64,
                    span.is_trailing as i64,
                ),
            )?;
        }
        for event in usage {
            let kind = if event.surface_kind == "agent" {
                "agent_spawn"
            } else if event.surface_kind == "hook" {
                "hook_use"
            } else {
                "tool_use"
            };
            // An agent spawn keeps its call id in `target` — the join key to the
            // subagent run it started (`write_subagent_runs`).
            tx.execute(
                "INSERT INTO events
                   (session_id, source_path, kind, surface_kind, surface_id, source,
                    started_at, started_epoch, duration_sec, out_tokens, ctx_growth,
                    ctx_start, ctx_peak, model, target)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, 0, 0, 0, 0, 0, NULL, ?8)",
                (
                    &session.id,
                    &session.source_path,
                    kind,
                    &event.surface_kind,
                    &event.surface_id,
                    epoch_ms_to_rfc3339(event.started_epoch_ms),
                    event.started_epoch_ms / 1000,
                    &event.tool_use_id,
                ),
            )?;
        }
        Ok(())
    }

    /// Prompt-pointer events for a session: `(source_line, epoch_ms)` per user
    /// prompt. The text is not stored — only the pointer (`source_path` with
    /// `source_line`) so it can be re-read later (`docs/specs/storage.md`).
    /// Runs inside the batch transaction, after [`Self::write_session`] has
    /// replaced the file's prior rows.
    fn write_prompts(
        tx: &rusqlite::Transaction<'_>,
        session: &SessionMeta,
        prompts: &[(usize, i64, &str)],
    ) -> Result<()> {
        // The prompt's behavioral class rides in the column EVENT_ENCODING
        // names for `prompt`.
        let behavior_column = encoding_column("prompt", EventRole::PromptBehavior);
        let insert = format!(
            "INSERT INTO events
               (session_id, source_path, source_line, kind, {behavior_column},
                started_at, started_epoch, duration_sec, out_tokens, ctx_growth,
                ctx_start, ctx_peak)
             VALUES (?1, ?2, ?3, 'prompt', ?4, '', ?5, 0, 0, 0, 0, 0)"
        );
        for (line_no, epoch_ms, behavior) in prompts {
            tx.execute(
                &insert,
                (
                    &session.id,
                    &session.source_path,
                    *line_no as i64,
                    *behavior,
                    epoch_ms / 1000,
                ),
            )?;
        }
        Ok(())
    }

    /// A session's subagent runs, and the settle of the cost of the spawns that
    /// started them — inside the batch transaction, after [`Self::write_session`]
    /// inserted those spawn events (the order the old API left to its callers).
    ///
    /// Two rows describe one subagent from opposite sides: the `agent_spawn`
    /// event is what the main thread *did*, the run is what it *cost*. They are
    /// joined by the spawn call's id — kept in the event's `target` — so the
    /// spawn's own `out_tokens` becomes the run's output and its `sub_tokens`
    /// the output of everything that run spawned in turn. A spawn whose run is
    /// missing keeps its zeros rather than borrowing another's figures.
    fn write_subagent_runs(
        tx: &rusqlite::Transaction<'_>,
        session: &SessionMeta,
        runs: &[SubagentRun],
    ) -> Result<()> {
        tx.execute(
            "DELETE FROM subagent_runs WHERE source_path = ?1",
            (&session.source_path,),
        )?;
        for run in runs {
            tx.execute(
                "INSERT INTO subagent_runs
                   (session_id, source_path, run_path, agent, agent_id, tool_use_id,
                    root_tool_use_id, parent_agent_id, spawn_depth, model, prompt_id,
                    out_tokens, started_at, started_epoch)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                (
                    &session.id,
                    &session.source_path,
                    &run.run_path,
                    &run.agent,
                    &run.agent_id,
                    &run.tool_use_id,
                    &run.root_tool_use_id,
                    &run.parent_agent_id,
                    run.spawn_depth,
                    &run.model,
                    &run.prompt_id,
                    run.out_tokens,
                    epoch_ms_to_rfc3339(run.started_epoch_ms),
                    run.started_epoch_ms / 1000,
                ),
            )?;
        }
        tx.execute(
            // Every subquery is scoped to this transcript. A call id is unique
            // within a session but nothing guarantees it across sessions, and an
            // unscoped scalar subquery would silently pick an arbitrary matching
            // row while the sums added another session's runs on top.
            "UPDATE events
                SET out_tokens = COALESCE((SELECT r.out_tokens FROM subagent_runs r
                                            WHERE r.source_path = events.source_path
                                              AND r.tool_use_id = events.target), 0),
                    sub_tokens = COALESCE((SELECT SUM(r.out_tokens) FROM subagent_runs r
                                            WHERE r.source_path = events.source_path
                                              AND r.root_tool_use_id = events.target
                                              AND r.tool_use_id IS NOT events.target), 0),
                    sub_agent_count = (SELECT COUNT(*) FROM subagent_runs r
                                        WHERE r.source_path = events.source_path
                                          AND r.root_tool_use_id = events.target
                                          AND r.tool_use_id IS NOT events.target)
              WHERE kind = 'agent_spawn' AND source_path = ?1 AND target IS NOT NULL",
            (&session.source_path,),
        )?;
        Ok(())
    }

    /// Tool-failure events for a session: `(epoch_ms, category, excerpt, tool,
    /// target)` — inside the batch transaction, after [`Self::write_session`]'s
    /// replace. For a `tool_error` row the otherwise-unused detail columns carry
    /// the friction signal, per `EVENT_ENCODING`: category, excerpt, originating
    /// tool, call subject. `surface_kind` stays NULL so these never enter the
    /// surface-catalog join. The excerpt, tool, and target let a report show
    /// concrete instances, which tool produced them, and which file or command
    /// they hit — without re-reading the transcript. The columns come from the
    /// encoding table, so the writer, the view, and the readers cannot disagree.
    fn write_tool_errors(
        tx: &rusqlite::Transaction<'_>,
        session: &SessionMeta,
        errors: &[(i64, &str, &str, &str, &str)],
    ) -> Result<()> {
        let category = encoding_column("tool_error", EventRole::Category);
        let excerpt = encoding_column("tool_error", EventRole::Excerpt);
        let tool = encoding_column("tool_error", EventRole::OriginatingTool);
        let target = encoding_column("tool_error", EventRole::FailureTarget);
        let insert = format!(
            "INSERT INTO events
               (session_id, source_path, kind, {category}, {excerpt}, {tool}, {target},
                started_at, started_epoch, duration_sec, out_tokens, ctx_growth,
                ctx_start, ctx_peak)
             VALUES (?1, ?2, 'tool_error', ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, 0, 0, 0)"
        );
        for (epoch_ms, category, excerpt, tool, target) in errors {
            tx.execute(
                &insert,
                (
                    &session.id,
                    &session.source_path,
                    *category,
                    *excerpt,
                    *tool,
                    *target,
                    epoch_ms_to_rfc3339(*epoch_ms),
                    epoch_ms / 1000,
                ),
            )?;
        }
        Ok(())
    }

    /// Work events `(epoch_ms, kind, id, path)` — Bash leading words and edited
    /// file basenames — kept out of the catalog join (`surface_kind` NULL), for
    /// a session whose rows [`Self::write_session`] just replaced. A file edit's
    /// full path goes to `target` while `surface_id` keeps the basename that
    /// hotspot rankings group on; which meaning a row's value carries depends on
    /// its `kind` and is read back per `EVENT_ENCODING` (one statement spans
    /// several kinds, so the writer fills the physical columns as they are).
    fn write_work_events(
        tx: &rusqlite::Transaction<'_>,
        session: &SessionMeta,
        events: &[(i64, &str, &str, Option<&str>)],
    ) -> Result<()> {
        for (epoch_ms, kind, id, path) in events {
            tx.execute(
                "INSERT INTO events
                   (session_id, source_path, kind, surface_id, target,
                    started_at, started_epoch, duration_sec, out_tokens, ctx_growth,
                    ctx_start, ctx_peak)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, 0, 0, 0)",
                (
                    &session.id,
                    &session.source_path,
                    kind,
                    id,
                    path,
                    epoch_ms_to_rfc3339(*epoch_ms),
                    epoch_ms / 1000,
                ),
            )?;
        }
        Ok(())
    }

    /// Prompt pointers in the window: `(transcript path, 0-based line)` per
    /// user prompt. The view re-reads the text through the adapter; a missing
    /// file or line resolves to `None` there and counts as pruned, never as
    /// an error — so the store never needs the prompt text itself.
    pub fn prompt_refs(&self, range: TimeRange) -> Result<Vec<(String, i64)>> {
        let (filter, binds) = range_clause(range, "e.started_epoch");
        let sql = format!(
            "SELECT DISTINCT e.source_path, e.source_line
             FROM events e
             WHERE e.kind = 'prompt' AND e.source_line IS NOT NULL{filter}
             ORDER BY e.source_path, e.source_line"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Every file edit as a `FileEdit`, time-ordered — the input to thrash
    /// detection, which needs the session and full path to tell one agent's
    /// retries apart from parallel work on a same-named file (`core::thrash`).
    ///
    /// Rows without a recorded path are **excluded**, not fallen back to the
    /// basename in `surface_id`: grouping same-named files together is the bug
    /// this query exists to avoid, and a silent fallback would reproduce it for
    /// every store written before paths were recorded — invisibly, since the
    /// output would look normal. An empty result surfaces as "no thrash
    /// episodes", which tells the user to re-analyze.
    pub fn file_edits(&self, range: TimeRange) -> Result<Vec<FileEdit>> {
        // Which column holds the full path is EVENT_ENCODING's definition for
        // `file_edit`; this reader takes it from there instead of repeating it.
        let path = encoding_column("file_edit", EventRole::FilePath);
        let (filter, binds) = range_clause(range, "e.started_epoch");
        let sql = format!(
            "SELECT s.project, e.session_id, e.{path}, e.started_epoch
             FROM events e JOIN sessions s ON e.session_id = s.id
             WHERE e.kind = 'file_edit' AND e.{path} IS NOT NULL{filter}
             ORDER BY e.started_epoch"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok(FileEdit {
                    project: row.get(0)?,
                    session_id: row.get(1)?,
                    path: row.get(2)?,
                    epoch: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Every distinct read file target in the window — the evidence the
    /// never-fires wedge matches rule `paths:` globs against. Read targets
    /// are observed, never attributed: they share the `file_edit` encoding
    /// (basename id, full-path target) but ride their own kind, so thrash
    /// (which reads `file_edit` only) never sees them.
    ///
    /// Bounded like the briefing readers: at most `limit` targets come back —
    /// the head prefix of the unbounded result, same `ORDER BY`, with the
    /// `LIMIT ?` bind via [`range_params`] / [`saturating_limit`]. Over the
    /// cap the evidence shrinks, and since never-fires flags a rule on "no
    /// glob matched", a dropped target can only flip a would-be match into a
    /// false flag — so callers pass a cap well above any realistic window
    /// (the briefing's fetch-cap convention) and treat saturation as
    /// degraded evidence, not a narrower window.
    pub fn file_read_targets(&self, range: TimeRange, limit: usize) -> Result<Vec<String>> {
        let path = encoding_column("file_read", EventRole::FilePath);
        let (filter, binds) = range_clause(range, "e.started_epoch");
        let sql = format!(
            "SELECT DISTINCT e.{path}
             FROM events e JOIN sessions s ON e.session_id = s.id
             WHERE e.kind = 'file_read' AND e.{path} IS NOT NULL{filter}
             ORDER BY e.{path} LIMIT ?"
        );
        let params = range_params(&binds, [], limit);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Counts of a work-event kind by id (e.g. Bash leading word, edited file),
    /// most frequent first.
    pub fn work_counts(&self, kind: &str, range: TimeRange) -> Result<Vec<(String, i64)>> {
        // Which meaning a work row's id carries depends on its kind, and
        // EVENT_ENCODING is where that is defined; a kind with no entry is
        // rejected rather than read from an arbitrary column.
        let id = match kind {
            "file_edit" => encoding_column("file_edit", EventRole::FileBasename),
            "bash_cmd" => encoding_column("bash_cmd", EventRole::CommandWord),
            other => anyhow::bail!("no encoding defined for work-event kind {other:?}"),
        };
        let (filter, binds) = range_clause(range, "started_epoch");
        let sql = format!(
            "SELECT {id}, COUNT(*) FROM events WHERE kind = ?1{filter}
             GROUP BY {id} ORDER BY COUNT(*) DESC"
        );
        let kind_cell = rusqlite::types::Value::Text(kind.to_string());
        let mut params: Vec<rusqlite::types::Value> = vec![kind_cell];
        params.extend(binds.iter().copied().map(rusqlite::types::Value::from));
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Counts of tool failures by category, most frequent first.
    pub fn error_counts(&self, range: TimeRange) -> Result<Vec<(String, i64)>> {
        let category = encoding_column("tool_error", EventRole::Category);
        let (filter, binds) = range_clause(range, "started_epoch");
        let sql = format!(
            "SELECT {category}, COUNT(*) FROM events WHERE kind = 'tool_error'{filter}
             GROUP BY {category} ORDER BY COUNT(*) DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Tool-failure counts split by project and category, densest pair first.
    /// Joins each error event back to its session's project so a friction
    /// category can be attributed to the project whose config should carry the
    /// fix — backing a `--project` filter and the dominant-project line in the
    /// summary.
    pub fn error_counts_by_project(&self, range: TimeRange) -> Result<Vec<FrictionCell>> {
        let category = encoding_column("tool_error", EventRole::Category);
        let (filter, binds) = range_clause(range, "e.started_epoch");
        let sql = format!(
            "SELECT s.project, e.{category}, COUNT(*)
             FROM events e JOIN sessions s ON e.session_id = s.id
             WHERE e.kind = 'tool_error'{filter}
             GROUP BY s.project, e.{category}
             ORDER BY COUNT(*) DESC, s.project, e.{category}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok(FrictionCell {
                    project: row.get(0)?,
                    category: row.get(1)?,
                    count: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Up to `per_group` example error excerpts for each `(project, category)`,
    /// as `(project, category, excerpt)`. Gives a report the concrete instances
    /// behind a friction count — the actual failing paths/files — so the reader
    /// (or a seeded agent) need not re-mine the transcripts. Earliest examples
    /// first within a group; empty excerpts are skipped.
    pub fn error_examples(
        &self,
        per_group: u32,
        range: TimeRange,
    ) -> Result<Vec<(String, String, String)>> {
        let category = encoding_column("tool_error", EventRole::Category);
        let excerpt = encoding_column("tool_error", EventRole::Excerpt);
        let (filter, binds) = range_clause(range, "e.started_epoch");
        let sql = format!(
            "SELECT project, category, excerpt FROM (
                 SELECT s.project AS project, e.{category} AS category, e.{excerpt} AS excerpt,
                        ROW_NUMBER() OVER (
                            PARTITION BY s.project, e.{category} ORDER BY e.id
                        ) AS rn
                 FROM events e JOIN sessions s ON e.session_id = s.id
                 WHERE e.kind = 'tool_error' AND e.{excerpt} IS NOT NULL AND e.{excerpt} <> ''{filter}
             )
             WHERE rn <= ?
             ORDER BY project, category, rn"
        );
        // Anonymous placeholders throughout: the range bounds bind first (the
        // filter sits before the LIMIT), the per-group cap last.
        let mut params: Vec<rusqlite::types::Value> = binds
            .iter()
            .copied()
            .map(rusqlite::types::Value::from)
            .collect();
        params.push(rusqlite::types::Value::Integer(i64::from(per_group)));
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Tool-failure counts split by project, category, and originating tool,
    /// densest first. Answers "which tool
    /// produced these failures" — e.g. path-not-found split across Read / Bash /
    /// Edit / a Playwright locator — so a report separates file friction from a
    /// browser miss that merely reads as "not found", and the seeded agent need
    /// not re-derive the attribution from the transcripts.
    pub fn error_tool_breakdown(&self, range: TimeRange) -> Result<Vec<ToolBreakdown>> {
        let category = encoding_column("tool_error", EventRole::Category);
        let tool = encoding_column("tool_error", EventRole::OriginatingTool);
        let (filter, binds) = range_clause(range, "e.started_epoch");
        let sql = format!(
            "SELECT s.project, e.{category}, COALESCE(e.{tool}, 'unknown'), COUNT(*)
             FROM events e JOIN sessions s ON e.session_id = s.id
             WHERE e.kind = 'tool_error'{filter}
             GROUP BY s.project, e.{category}, e.{tool}
             ORDER BY COUNT(*) DESC, s.project, e.{category}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok(ToolBreakdown {
                    project: row.get(0)?,
                    category: row.get(1)?,
                    tool: row.get(2)?,
                    count: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Counts of prompts by behavioral class (`source` column on prompt events),
    /// most frequent first.
    pub fn prompt_behavior_counts(&self, range: TimeRange) -> Result<Vec<(String, i64)>> {
        let behavior = encoding_column("prompt", EventRole::PromptBehavior);
        let (filter, binds) = range_clause(range, "started_epoch");
        let sql = format!(
            "SELECT {behavior}, COUNT(*) FROM events WHERE kind = 'prompt' AND {behavior} IS NOT NULL{filter}
             GROUP BY {behavior} ORDER BY COUNT(*) DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Per-skill usage rollup, most-invoked first.
    pub fn skill_usage(&self, range: TimeRange) -> Result<Vec<SkillUsage>> {
        let (filter, binds) = range_clause(range, "started_epoch");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT surface_id,
                    COUNT(*),
                    SUM(out_tokens),
                    SUM(ctx_growth),
                    SUM(duration_sec)
             FROM events
             WHERE surface_kind = 'skill'{filter}
             GROUP BY surface_id
             ORDER BY COUNT(*) DESC, surface_id"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok(SkillUsage {
                    skill: row.get(0)?,
                    invocations: row.get(1)?,
                    out_tokens: row.get(2)?,
                    ctx_growth: row.get(3)?,
                    duration_sec: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Per-model cost attribution over model-carrying usage events
    /// (`skill_invocation`, `agent_spawn`): count and output tokens per model.
    /// `tool_error` rows are excluded — their `model` column carries the
    /// originating tool, not a model (`docs/specs/events.md`). Ordering is
    /// left to `core::usage::bundle_models` (deterministic, unit-tested).
    /// No migration: `events.model` already exists.
    pub fn model_usage_events(&self, range: TimeRange) -> Result<Vec<(Option<String>, i64, u64)>> {
        let (filter, binds) = range_clause(range, "started_epoch");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT model, COUNT(*), COALESCE(SUM(out_tokens), 0)
             FROM events
             WHERE kind IN ('skill_invocation', 'agent_spawn'){filter}
             GROUP BY model"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Per-model subagent cost: runs and output tokens per `model` over
    /// `subagent_runs`. Bundled with the same `bundle_models` as the event
    /// side, but kept separate — main-thread and subagent costs answer
    /// different questions (`docs/specs/cli.md`).
    pub fn model_usage_subagents(
        &self,
        range: TimeRange,
    ) -> Result<Vec<(Option<String>, i64, u64)>> {
        let (filter, binds) = range_clause(range, "started_epoch");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT model, COUNT(*), COALESCE(SUM(out_tokens), 0)
             FROM subagent_runs
             WHERE 1=1{filter}
             GROUP BY model"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Per-kind event rollup for the trend view: every event kind's count,
    /// output tokens, and wall-clock in the window, ordered by kind so the two
    /// windows' rows line up for a diff. Kinds with no events in the window
    /// are absent rather than zero, so a caller can tell "unused" from "unseen".
    pub fn event_rollup_by_kind(&self, range: TimeRange) -> Result<Vec<KindRollup>> {
        let (filter, binds) = range_clause(range, "started_epoch");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT kind,
                    COUNT(*),
                    COALESCE(SUM(out_tokens), 0),
                    COALESCE(SUM(duration_sec), 0)
             FROM events
             WHERE 1=1{filter}
             GROUP BY kind
             ORDER BY kind"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok(KindRollup {
                    kind: row.get(0)?,
                    events: row.get(1)?,
                    out_tokens: row.get(2)?,
                    duration_sec: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The trend view's aggregates for both windows of `pair`: per-kind event
    /// rollups, per-skill usage, session count, and subagent totals. Each is
    /// computed per window by the same windowed queries the single-window
    /// views read — the trend compares the numbers those views report, never a
    /// second implementation of them.
    pub fn trend_aggregates(&self, pair: WindowPair) -> Result<TwoWindow<TrendWindowAggregates>> {
        Ok(TwoWindow {
            current: self.trend_window_aggregates(pair.current)?,
            previous: self.trend_window_aggregates(pair.previous)?,
        })
    }

    fn trend_window_aggregates(&self, range: TimeRange) -> Result<TrendWindowAggregates> {
        let (sessions, _) = self.session_stats(range)?;
        let (sub_tokens, subagent_runs) = self.subagent_totals(range)?;
        Ok(TrendWindowAggregates {
            by_kind: self.event_rollup_by_kind(range)?,
            skills: self.skill_usage(range)?,
            surfaces: self.usage_counts(range)?,
            sessions,
            subagent_runs,
            sub_tokens,
        })
    }

    /// Total startup tokens of **global** config — what every session pays
    /// unconditionally from `~/.claude` regardless of project: always-on files
    /// in full, plus the descriptions skills and agents contribute to the
    /// startup listing. Bodies that wait to be invoked weigh nothing here, and a
    /// surface whose startup weight is unknown (an MCP schema) is left out of
    /// the sum rather than counted as zero. Project config is always-on only for
    /// its own sessions (`always_on_config_tokens_for`).
    pub fn always_on_config_tokens(&self) -> Result<i64> {
        let total = self.conn.query_row(
            "SELECT COALESCE(SUM(startup_tokens), 0) FROM surfaces
             WHERE scope = 'global'",
            [],
            |row| row.get(0),
        )?;
        Ok(total)
    }

    /// Empirical always-on context floor per project as `(project, floor,
    /// sessions)`: the leanest context any of that project's sessions *started*
    /// with, and how many starts back the figure.
    ///
    /// Only `session_start` events qualify. A skill span's `ctx_start` is the
    /// prompt size wherever that skill ran, which in a long or resumed session
    /// sits far above the session's own start — reading floors from spans
    /// reported startup costs that were really mid-session sizes. The session
    /// count is part of the answer because the floor is a minimum over
    /// observations: one resumed session alone still overstates it, and the
    /// count is what makes that visible.
    pub fn baseline_floor_per_project(&self, range: TimeRange) -> Result<Vec<(String, i64, i64)>> {
        let (filter, binds) = range_clause_utc_text(range, "s.started_at");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT s.project, MIN(e.ctx_start), COUNT(*)
             FROM events e JOIN sessions s ON e.session_id = s.id
             WHERE e.kind = 'session_start' AND e.ctx_start > 0{filter}
             GROUP BY s.project
             ORDER BY MIN(e.ctx_start) DESC"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// The global empirical always-on floor — the leanest session start observed
    /// anywhere. `None` when no session start was observed at all, so callers
    /// report the metric as unavailable instead of inventing a zero.
    pub fn baseline_floor(&self, range: TimeRange) -> Result<Option<i64>> {
        let (filter, binds) = range_clause_utc_text(range, "s.started_at");
        let floor = self.conn.query_row(
            &format!(
                "SELECT MIN(e.ctx_start) FROM events e
                 JOIN sessions s ON e.session_id = s.id
                 WHERE e.kind = 'session_start' AND e.ctx_start > 0{filter}"
            ),
            params_from_iter(binds),
            |row| row.get(0),
        )?;
        Ok(floor)
    }

    /// How much data the store holds: `(sessions, distinct projects)` — the
    /// summary's "what was analyzed" context line.
    pub fn session_stats(&self, range: TimeRange) -> Result<(i64, i64)> {
        let (filter, binds) = range_clause_utc_text(range, "started_at");
        let row = self.conn.query_row(
            &format!("SELECT COUNT(*), COUNT(DISTINCT project) FROM sessions WHERE 1=1{filter}"),
            params_from_iter(binds),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(row)
    }

    /// One row per session in the window: its identity, output tokens
    /// (events plus the session's subagent total, which lives on the session
    /// row), wall-clock span covered, and tool-error count. Sessions without
    /// events or friction read 0 — never vanish — since "quiet" and "missing"
    /// are different answers. Events are windowed; sessions are too (by
    /// `started_at`), so a stale session does not dilute a fresh window.
    pub fn session_breakdown(&self, range: TimeRange) -> Result<Vec<SessionRow>> {
        let (sfilter, sbinds) = range_clause_utc_text(range, "s.started_at");
        let (efilter, ebinds) = range_clause(range, "e.started_epoch");
        // Two bind types (text bounds, epoch bounds): box them into values.
        // Order follows the SQL text: the JOIN's epoch filter precedes the
        // WHERE's text filter, so epoch binds come first.
        let binds: Vec<rusqlite::types::Value> = ebinds
            .into_iter()
            .map(rusqlite::types::Value::from)
            .chain(sbinds.into_iter().map(rusqlite::types::Value::from))
            .collect();
        let mut stmt = self.conn.prepare(&format!(
            "SELECT s.id, s.project, s.slug, s.started_at,
                    COALESCE(SUM(e.out_tokens), 0) + s.sub_tokens,
                    COALESCE(MAX(e.started_epoch), 0) - COALESCE(MIN(e.started_epoch), 0),
                    COALESCE(SUM(CASE WHEN e.kind = 'tool_error' THEN 1 ELSE 0 END), 0)
             FROM sessions s LEFT JOIN events e
               ON e.session_id = s.id{efilter}
             WHERE 1=1{sfilter}
             GROUP BY s.id
             ORDER BY s.id"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok(SessionRow {
                    id: row.get(0)?,
                    project: row.get(1)?,
                    slug: row.get(2)?,
                    started_at: row.get(3)?,
                    out_tokens: row.get(4)?,
                    duration_sec: row.get(5)?,
                    friction: row.get(6)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Total subagent output tokens and subagent count across all sessions.
    pub fn subagent_totals(&self, range: TimeRange) -> Result<(i64, i64)> {
        let (filter, binds) = range_clause_utc_text(range, "started_at");
        let row = self.conn.query_row(
            &format!(
                "SELECT COALESCE(SUM(sub_tokens), 0), COALESCE(SUM(sub_agent_count), 0)
                 FROM sessions WHERE 1=1{filter}"
            ),
            params_from_iter(binds),
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(row)
    }

    /// Subagent output tokens and run count per agent type, costliest first —
    /// the breakdown behind the session-level subagent total.
    ///
    /// Every run counts once, at its own cost, whatever its depth: a nested run
    /// is the agent that produced those tokens, and rolling it into its parent's
    /// type would credit the cost to an agent that only delegated. Runs whose
    /// type is unknown (no sidecar beside the transcript) group under `None`
    /// rather than being dropped, so the rows sum to the total of every run
    /// that was extracted. Sessions whose transcripts were pruned before their
    /// runs could be extracted keep their totals but have no rows here; a
    /// report reconciles the two with `core::subagent::SplitCoverage` rather
    /// than presenting these rows as the whole breakdown.
    pub fn subagent_by_agent(&self, range: TimeRange) -> Result<Vec<AgentCost>> {
        let (filter, binds) = range_clause(range, "started_epoch");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT agent, COUNT(*), COALESCE(SUM(out_tokens), 0)
             FROM subagent_runs
             WHERE 1=1{filter}
             GROUP BY agent
             ORDER BY COALESCE(SUM(out_tokens), 0) DESC, COUNT(*) DESC"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok(AgentCost {
                    agent: row.get(0)?,
                    runs: row.get(1)?,
                    out_tokens: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Invocation counts per surface `(kind, id)` across all event kinds — the
    /// usage side of the catalog join for every surface, not just skills.
    pub fn usage_counts(&self, range: TimeRange) -> Result<Vec<(String, String, i64)>> {
        let (filter, binds) = range_clause(range, "started_epoch");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT surface_kind, surface_id, COUNT(*)
             FROM events
             WHERE surface_kind IS NOT NULL{filter}
             GROUP BY surface_kind, surface_id"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Per-event costs for skill invocations, for time bucketing in the report.
    pub fn skill_event_costs(&self, range: TimeRange) -> Result<Vec<EventCost>> {
        let (filter, binds) = range_clause(range, "started_epoch");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT started_epoch, out_tokens, ctx_growth, duration_sec
             FROM events
             WHERE surface_kind = 'skill'{filter}"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(binds), |row| {
                Ok(EventCost {
                    started_epoch: row.get(0)?,
                    out_tokens: row.get(1)?,
                    ctx_growth: row.get(2)?,
                    duration_sec: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// An analyze-run metadata value, if recorded.
    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query_map([key], |row| row.get(0))?;
        Ok(rows.next().transpose()?)
    }

    /// Record an analyze-run metadata value, superseding any previous one.
    pub fn set_meta(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
            (key, value),
        )?;
        Ok(())
    }

    /// Whether the store's rows were produced by the current analysis semantics.
    /// The `(mtime, size)` skip below never expires, so an improved extractor or
    /// classifier would otherwise leave a transcript that never changes stamped
    /// with its old results forever; a mismatch here tells `analyze` to re-ingest
    /// everything once (`docs/specs/storage.md`). "Everything" means every
    /// transcript still on disk — rows whose source file is gone can never be
    /// re-derived, so they stay as last extracted.
    ///
    /// Which `key`/`version` pair a harness uses is the harness's own business
    /// (`adapter::transcript` for Claude Code, `adapter::opencode`,
    /// `adapter::kilocode`); the store only compares what it was handed, so it
    /// stays harness-agnostic.
    pub fn is_analyzer_current(&self, key: &str, version: &str) -> Result<bool> {
        Ok(self.meta(key)?.as_deref() == Some(version))
    }

    /// Stamp the store with the analysis semantics its rows were produced by.
    /// Call once a full analyze run has completed.
    pub fn record_analyzer_version(&mut self, key: &str, version: &str) -> Result<()> {
        self.set_meta(key, version)
    }

    /// Whether a source file's `(mtime, size)` matches its recorded ingest
    /// fingerprint — if so, re-analyzing may skip it (`docs/specs/storage.md`).
    /// `mtime` is epoch nanoseconds (`adapter::harness::file_fingerprint`).
    pub fn is_ingested(&self, path: &str, mtime: i64, size: i64) -> Result<bool> {
        let matched = self.conn.query_row(
            "SELECT COUNT(*) FROM ingested_files WHERE path = ?1 AND mtime = ?2 AND size = ?3",
            (path, mtime, size),
            |row| row.get::<_, i64>(0),
        )?;
        Ok(matched > 0)
    }

    /// Record a source file's ingest fingerprint. Call **after** the read
    /// completes, with the pre-read stat — a file that grew mid-read then shows
    /// a changed fingerprint next run and is re-ingested (`docs/specs/storage.md`).
    /// `mtime` is epoch nanoseconds (`adapter::harness::file_fingerprint`).
    pub fn record_ingested_file(&mut self, path: &str, mtime: i64, size: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO ingested_files (path, mtime, size) VALUES (?1, ?2, ?3)",
            (path, mtime, size),
        )?;
        Ok(())
    }

    /// Rebuild the surface catalog wholesale — it is a snapshot of current
    /// config, not an accumulation (see `docs/specs/storage.md`).
    pub fn replace_surfaces(&mut self, surfaces: &[Surface]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM surfaces", ())?;
        for surface in surfaces {
            let attrs = if surface.paths.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&surface.paths).context("catalog globs are strings")?)
            };
            tx.execute(
                "INSERT OR REPLACE INTO surfaces
                   (kind, id, scope, project, config_path, static_tokens,
                    startup_tokens, load_mode, attrs_json, description)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                (
                    &surface.kind,
                    &surface.id,
                    surface.scope.label(),
                    surface.scope.project(),
                    &surface.config_path,
                    surface.static_tokens,
                    surface.startup_tokens,
                    surface.load_mode.label(),
                    attrs,
                    &surface.description,
                ),
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The whole catalog — every `(kind, id, scope, project)` row — with each
    /// row's **effective** usage. Shadowing is per project: an event from
    /// project P joins P's project row when one exists for that `(kind, id)`,
    /// else the global row. One event therefore counts on exactly one row
    /// (`docs/specs/surfaces.md`).
    ///
    /// The window covers the **events** half only: the catalog is current
    /// config, the usage joined against it is historical. Callers that read no
    /// window ([`Store::session_roots`], [`Store::always_on_config_tokens`])
    /// take no window at all, so a windowed argument here always means
    /// "usage within this window".
    pub fn effective_catalog(&self, events_window: TimeRange) -> Result<Vec<CatalogEntry>> {
        let (filter, binds) = range_clause(events_window, "e.started_epoch");
        let mut stmt = self.conn.prepare(&format!(
            "SELECT f.kind, f.id, f.scope, f.project, f.static_tokens,
                    f.startup_tokens, f.load_mode, f.attrs_json, f.description, COUNT(u.event_id)
             FROM surfaces f
             LEFT JOIN (SELECT e.id AS event_id, e.surface_kind, e.surface_id,
                               s.project AS session_project
                        FROM events e JOIN sessions s ON e.session_id = s.id
                        WHERE e.surface_kind IS NOT NULL{filter}) u
               ON u.surface_kind = f.kind AND u.surface_id = f.id
              AND ((f.scope = 'project' AND u.session_project = f.project)
                OR (f.scope = 'global' AND u.session_project NOT IN (
                      SELECT p.project FROM surfaces p
                      WHERE p.kind = f.kind AND p.id = f.id AND p.scope = 'project')))
             GROUP BY f.kind, f.id, f.scope, f.project
             ORDER BY f.kind, f.id, f.scope, f.project"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(binds), catalog_entry_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Distinct known `(root, project)` pairs — the directories project-config
    /// scanning walks, each with the normalized project slug its surfaces get
    /// scoped to. Sessions whose transcript carried no cwd are skipped.
    pub fn session_roots(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT root, project FROM sessions WHERE root <> '' ORDER BY root",
        )?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Total startup config tokens for a session in `project`: the global
    /// figure plus that project's own startup cost.
    ///
    /// A surface installed at both scopes under the same `(kind, id)` is counted
    /// twice here. Resolving that would mean asserting which copy Claude Code
    /// actually loads, and the shadowing model this store implements is defined
    /// for the **usage join** (`effective_catalog`), not for load order — a
    /// project rule and its global namesake genuinely both reach the context, so
    /// excluding one would understate the figure instead.
    pub fn always_on_config_tokens_for(&self, project: &str) -> Result<i64> {
        let total = self.conn.query_row(
            "SELECT COALESCE(SUM(startup_tokens), 0) FROM surfaces
             WHERE scope = 'global' OR (scope = 'project' AND project = ?1)",
            [project],
            |row| row.get(0),
        )?;
        Ok(total)
    }

    /// Bounded briefing readers (`pbi/2026-11-09-01-fix-briefing-bounds.md`).
    ///
    /// Every method here mirrors its unbounded twin above — same `WHERE`, same
    /// `ORDER BY`, plus a trailing `LIMIT ?` — so a bounded result is the
    /// head prefix of the unbounded one: identical output under the limit, a
    /// head cut over it. The briefing passes fetch caps owned by
    /// `core::findings` (`BRIEFING_GROUPS_LIMIT`, `BRIEFING_EVENTS_LIMIT`);
    /// the `take(N)` / `truncate(N)` cuts in `core::findings` stay what they
    /// are — display-head cuts applied after the fetch — and must never be
    /// mistaken for these fetch bounds.
    ///
    /// Total skill-invocation output tokens in the window, summed in SQL — the
    /// briefing's `main_out` figure. Fetching every per-skill total into a
    /// `Vec` just to add them up materializes rows for a number SQL keeps in
    /// one pass. Matches
    /// `skill_usage(range).iter().map(|r| r.out_tokens).sum()`.
    pub fn skill_out_tokens_sum(&self, range: TimeRange) -> Result<i64> {
        let (filter, binds) = range_clause(range, "started_epoch");
        let total = self.conn.query_row(
            &format!(
                "SELECT COALESCE(SUM(out_tokens), 0) FROM events \
                 WHERE surface_kind = 'skill'{filter}"
            ),
            params_from_iter(binds),
            |row| row.get(0),
        )?;
        Ok(total)
    }
}

fn source_label(source: Source) -> &'static str {
    match source {
        Source::Slash => "slash",
        Source::Tool => "tool",
    }
}

fn epoch_ms_to_rfc3339(epoch_ms: i64) -> String {
    crate::time::epoch_ms_to_rfc3339(epoch_ms)
}

/// A SQLite cell as a typed JSON value, for the `--format json` surface.
fn value_to_json(v: rusqlite::types::ValueRef<'_>) -> serde_json::Value {
    use rusqlite::types::ValueRef;
    match v {
        ValueRef::Null => serde_json::Value::Null,
        ValueRef::Integer(i) => serde_json::json!(i),
        ValueRef::Real(f) => serde_json::json!(f),
        ValueRef::Text(t) => serde_json::json!(String::from_utf8_lossy(t)),
        ValueRef::Blob(_) => serde_json::json!("<blob>"),
    }
}

/// Stringify a SQLite cell for the generic `query` surface.
fn value_to_string(v: rusqlite::types::ValueRef<'_>) -> String {
    use rusqlite::types::ValueRef;
    match v {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(_) => "<blob>".to_string(),
    }
}
