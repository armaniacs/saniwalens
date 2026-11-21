use super::migrate::{declared_sql, migrate, reconcile_columns, table_exists, table_shape};
use super::permission::{
    narrow_store_or_warn, unprivate_dir_warning, unprivate_store_file_warning,
    unrestrictable_store_file_warning,
};
use super::schema::{SCHEMA, SCHEMA_VERSION};
use super::*;
use crate::core::period::TimeRange;
use crate::core::subagent::SplitCoverage;
use crate::core::surface::{LoadMode, Scope};
use crate::core::trend::skill_deltas;

/// The store is agnostic to which analysis-semantics pair a harness
/// stamps, so the tests use their own — nothing here depends on a real
/// harness's key or version.
const TEST_ANALYZER_KEY: &str = "test_analyzer_version";
const TEST_ANALYZER_VERSION: &str = "1";

/// Permissions are the reason this test and the next one work on a real
/// temp directory instead of `Store::in_memory`: the default store path
/// lives under a user state directory that may not exist yet, and both it
/// and the store must come out owner-only.
#[test]
fn open_creates_a_missing_directory_and_keeps_both_owner_only() {
    let dir = std::env::temp_dir().join(format!("saniwalens-open-test-{}", std::process::id()));
    let path = dir.join("saniwalens").join("saniwalens.db");
    let _ = std::fs::remove_dir_all(&dir);

    drop(Store::open(&path, OpenMode::ReadWrite).unwrap());

    assert!(path.exists(), "the store was not created at {path:?}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&path), 0o600);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The store an older saniwalens left under the umask is the case that actually
/// exists in the wild, so opening it has to narrow it — otherwise the
/// protection never reaches anyone who already ran the tool.
#[cfg(unix)]
#[test]
fn open_narrows_a_store_an_earlier_run_left_readable() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("saniwalens-mode-test-{}", std::process::id()));
    let path = dir.join("saniwalens.db");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    drop(Store::open(&path, OpenMode::ReadWrite).unwrap());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    drop(Store::open(&path, OpenMode::ReadWrite).unwrap());

    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---- PBI 25: existing store anomalies are visible ----

#[cfg(unix)]
#[test]
fn a_store_file_mode_the_owner_only_posture_allows_yields_no_warning() {
    let path = std::path::Path::new("/state/saniwalens.db");
    assert_eq!(unprivate_store_file_warning(path, 0o100600), None);
}

#[cfg(unix)]
#[test]
fn a_group_other_readable_store_file_mode_yields_a_warning() {
    let path = std::path::Path::new("/state/saniwalens.db");
    let line = unprivate_store_file_warning(path, 0o100644).unwrap();
    assert!(line.starts_with("warning: "));
    assert!(line.contains("saniwalens.db"));
    assert!(line.contains("644"));
}

#[cfg(unix)]
#[test]
fn a_group_writable_store_file_mode_yields_a_warning_too() {
    let path = std::path::Path::new("/state/saniwalens.db");
    assert!(unprivate_store_file_warning(path, 0o100660).is_some());
}

#[cfg(unix)]
#[test]
fn a_store_parent_dir_yields_a_warning_only_when_group_other_bits_are_set() {
    let dir = std::path::Path::new("/state");
    assert_eq!(unprivate_dir_warning(dir, 0o040700), None);
    assert!(unprivate_dir_warning(dir, 0o040777).is_some());
    assert!(unprivate_dir_warning(dir, 0o040750).is_some());
}

#[cfg(unix)]
#[test]
fn opening_a_chmodded_group_other_readable_store_warns_and_completes() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("saniwalens-warn-test-{}", std::process::id()));
    let path = dir.join("saniwalens.db");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    drop(Store::open(&path, OpenMode::ReadWrite).unwrap());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

    // the judge sees the exposure from the same metadata the open reads
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert!(unprivate_store_file_warning(&path, mode).is_some());

    // the open completes, the narrowing still happens, and the next open
    // is silent again — the warning is once per exposure, not per run
    drop(Store::open(&path, OpenMode::ReadWrite).unwrap());
    let narrowed = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(narrowed, 0o600);
    let after = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(unprivate_store_file_warning(&path, after), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_refused_narrowing_warns_and_continues_while_real_failures_still_fail() {
    let path = std::path::Path::new("/state/saniwalens.db");

    let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    assert!(narrow_store_or_warn(path, Err(denied)).is_ok());

    let broken = std::io::Error::other("boom");
    let err = narrow_store_or_warn(path, Err(broken)).unwrap_err();
    assert!(err.to_string().contains("restrict store permissions"));

    let line = unrestrictable_store_file_warning(path);
    assert!(line.starts_with("warning: "));
    assert!(line.contains("saniwalens.db"));
    assert!(line.contains("0600"));
}

fn span(skill: &str, out_tokens: u64, ctx_growth: u64, duration_sec: f64) -> Span {
    Span {
        skill: skill.to_string(),
        source: Source::Slash,
        started_epoch_ms: 1_700_000_000_000,
        duration_sec,
        out_tokens,
        ctx_growth,
        ctx_start: 0,
        ctx_peak: ctx_growth,
        model: Some("claude-opus-4-7".to_string()),
        is_trailing: false,
        spawns: Vec::new(),
        sub_tokens: 0,
        sub_agent_count: 0,
        sub_tokens_estimated: false,
    }
}

fn session(id: &str) -> SessionMeta {
    SessionMeta {
        id: id.to_string(),
        project: "demo".to_string(),
        slug: "demo".to_string(),
        root: String::new(),
        source_path: format!("/tmp/{id}.jsonl"),
        sub_tokens: 0,
        sub_agent_count: 0,
        start: None,
    }
}

fn agent_spawn(surface_id: &str, tool_use_id: &str) -> UsageEvent {
    UsageEvent {
        surface_kind: "agent".to_string(),
        surface_id: surface_id.to_string(),
        started_epoch_ms: 1_700_000_000_000,
        tool_use_id: Some(tool_use_id.to_string()),
    }
}

fn subagent_run(agent: &str, agent_id: &str, root: &str, out_tokens: u64) -> SubagentRun {
    SubagentRun {
        agent: Some(agent.to_string()),
        agent_id: agent_id.to_string(),
        run_path: format!("/tmp/example/subagents/agent-{agent_id}.jsonl"),
        tool_use_id: Some(format!("toolu_{agent_id}")),
        parent_agent_id: None,
        spawn_depth: 1,
        model: Some("sonnet".to_string()),
        prompt_id: Some("p1".to_string()),
        out_tokens,
        started_epoch_ms: 1_700_000_000_000,
        root_tool_use_id: Some(root.to_string()),
    }
}

#[test]
fn limited_queries_report_truncation_at_the_boundary() {
    // Exactly-at-limit is complete; one more row flips the flag and the
    // extra probe row is dropped. `None` stays unbounded.
    let mut store = Store::in_memory().unwrap();
    for id in ["s1", "s2", "s3"] {
        store
            .ingest(IngestBatch::new(&session(id), &[], &[]))
            .unwrap();
    }
    let q = "SELECT id FROM sessions ORDER BY id";
    let (_, rows, truncated) = store.query_limited(q, Some(3)).unwrap();
    assert!(!truncated);
    assert_eq!(rows.len(), 3);
    let (_, rows, truncated) = store.query_limited(q, Some(2)).unwrap();
    assert!(truncated);
    assert_eq!(rows.len(), 2);
    let (_, rows, truncated) = store.query_limited(q, None).unwrap();
    assert!(!truncated);
    assert_eq!(rows.len(), 3);
    let (_, rows, truncated) = store.query_json_limited(q, Some(2)).unwrap();
    assert!(truncated);
    assert_eq!(rows.len(), 2);
}

#[test]
fn file_read_targets_saturate_at_the_limit() {
    // The never-fires evidence fetch is bounded like the briefing
    // readers: exactly-at-limit is the complete head of the same ordered
    // stream, one distinct target more is cut, and a duplicate read
    // still counts once.
    let mut store = Store::in_memory().unwrap();
    let s1 = session("s1");
    let paths: Vec<String> = (0..3).map(|i| format!("/repo/f{i}.txt")).collect();
    let mut events: Vec<(i64, &str, &str, Option<&str>)> = paths
        .iter()
        .map(|p| (1_700_000_000_000, "file_read", "f.txt", Some(p.as_str())))
        .collect();
    events.push((
        1_700_000_000_001,
        "file_read",
        "f.txt",
        Some(paths[0].as_str()),
    ));
    store
        .ingest(IngestBatch::new(&s1, &[], &[]).work_events(&events))
        .unwrap();

    assert_eq!(
        store.file_read_targets(TimeRange::ALL, 2).unwrap(),
        vec!["/repo/f0.txt".to_string(), "/repo/f1.txt".to_string(),]
    );
    assert_eq!(store.file_read_targets(TimeRange::ALL, 3).unwrap().len(), 3);
    assert_eq!(
        store.file_read_targets(TimeRange::ALL, 100).unwrap().len(),
        3
    );
}

#[test]
fn subagent_cost_rolls_up_per_agent_type() {
    let mut store = Store::in_memory().unwrap();
    let s1 = session("s1");
    store
        .ingest(IngestBatch::new(&s1, &[], &[]).subagent_runs(&[
            subagent_run("Explore", "a1", "toolu_a1", 400),
            subagent_run("Explore", "a2", "toolu_a2", 100),
            subagent_run("code-writer", "a3", "toolu_a3", 900),
        ]))
        .unwrap();

    assert_eq!(
        store.subagent_by_agent(TimeRange::ALL).unwrap(),
        vec![
            AgentCost {
                agent: Some("code-writer".to_string()),
                runs: 1,
                out_tokens: 900,
            },
            AgentCost {
                agent: Some("Explore".to_string()),
                runs: 2,
                out_tokens: 500,
            },
        ]
    );
}

#[test]
fn a_spawn_event_takes_the_cost_of_the_run_it_started() {
    let mut store = Store::in_memory().unwrap();
    let s1 = session("s1");
    let mut nested = subagent_run("code-writer", "a2", "toolu_a1", 700);
    nested.parent_agent_id = Some("a1".to_string());
    nested.spawn_depth = 2;
    store
        .ingest(
            IngestBatch::new(&s1, &[], &[agent_spawn("Explore", "toolu_a1")])
                .subagent_runs(&[subagent_run("Explore", "a1", "toolu_a1", 400), nested]),
        )
        .unwrap();

    let (out_tokens, sub_tokens, sub_agents) = store
        .query(
            "SELECT out_tokens, sub_tokens, sub_agent_count FROM events \
                 WHERE kind = 'agent_spawn'",
        )
        .map(|(_, rows)| (rows[0][0].clone(), rows[0][1].clone(), rows[0][2].clone()))
        .unwrap();

    // The spawn's own run wrote 400; the run it spawned in turn wrote 700.
    assert_eq!((out_tokens.as_str(), sub_tokens.as_str()), ("400", "700"));
    assert_eq!(sub_agents, "1");
}

#[test]
fn a_spawn_whose_run_is_missing_keeps_its_zeros() {
    let mut store = Store::in_memory().unwrap();
    let s1 = session("s1");
    store
        .ingest(
            IngestBatch::new(&s1, &[], &[agent_spawn("Explore", "toolu_gone")])
                .subagent_runs(&[subagent_run("Explore", "a1", "toolu_a1", 400)]),
        )
        .unwrap();

    let (_, rows) = store
        .query("SELECT out_tokens, sub_tokens FROM events WHERE target = 'toolu_gone'")
        .unwrap();
    assert_eq!(rows, vec![vec!["0".to_string(), "0".to_string()]]);
}

#[test]
fn a_spawn_takes_only_its_own_sessions_run() {
    // Call ids are unique within a session, not across the store — a resumed
    // or copied transcript can carry the same id into a second session.
    let mut store = Store::in_memory().unwrap();
    let (s1, s2) = (session("s1"), session("s2"));
    store
        .ingest(
            IngestBatch::new(&s1, &[], &[agent_spawn("Explore", "toolu_a1")])
                .subagent_runs(&[subagent_run("Explore", "a1", "toolu_a1", 400)]),
        )
        .unwrap();
    store
        .ingest(
            IngestBatch::new(&s2, &[], &[agent_spawn("Explore", "toolu_a1")])
                .subagent_runs(&[subagent_run("Explore", "a1", "toolu_a1", 900)]),
        )
        .unwrap();

    let (_, rows) = store
        .query(
            "SELECT session_id, out_tokens FROM events \
                 WHERE kind = 'agent_spawn' ORDER BY session_id",
        )
        .unwrap();
    assert_eq!(
        rows,
        vec![
            vec!["s1".to_string(), "400".to_string()],
            vec!["s2".to_string(), "900".to_string()],
        ]
    );
}

#[test]
fn re_ingesting_a_session_replaces_its_subagent_runs() {
    let mut store = Store::in_memory().unwrap();
    let s1 = session("s1");
    store.ingest(IngestBatch::new(&s1, &[], &[])).unwrap();
    let runs = [subagent_run("Explore", "a1", "toolu_a1", 400)];
    store
        .ingest(IngestBatch::new(&s1, &[], &[]).subagent_runs(&runs))
        .unwrap();
    store
        .ingest(IngestBatch::new(&s1, &[], &[]).subagent_runs(&runs))
        .unwrap();

    assert_eq!(
        store.subagent_by_agent(TimeRange::ALL).unwrap(),
        vec![AgentCost {
            agent: Some("Explore".to_string()),
            runs: 1,
            out_tokens: 400,
        }]
    );
}

#[test]
fn rolls_up_usage_per_skill_across_sessions() {
    let mut store = Store::in_memory().unwrap();
    store
        .ingest(IngestBatch::new(
            &session("s1"),
            &[
                span("git-commit", 100, 50, 2.0),
                span("git-commit", 200, 30, 1.0),
            ],
            &[],
        ))
        .unwrap();
    store
        .ingest(IngestBatch::new(
            &session("s2"),
            &[span("pr-create", 10, 5, 0.5)],
            &[],
        ))
        .unwrap();

    let usage = store.skill_usage(TimeRange::ALL).unwrap();

    assert_eq!(
        usage,
        vec![
            SkillUsage {
                skill: "git-commit".to_string(),
                invocations: 2,
                out_tokens: 300,
                ctx_growth: 80,
                duration_sec: 3.0,
            },
            SkillUsage {
                skill: "pr-create".to_string(),
                invocations: 1,
                out_tokens: 10,
                ctx_growth: 5,
                duration_sec: 0.5,
            },
        ]
    );
}

#[test]
fn re_ingesting_a_session_replaces_its_events() {
    let mut store = Store::in_memory().unwrap();
    store
        .ingest(IngestBatch::new(
            &session("s1"),
            &[span("git-commit", 100, 50, 2.0)],
            &[],
        ))
        .unwrap();
    // Same source_path, different content — must supersede, not accumulate.
    store
        .ingest(IngestBatch::new(
            &session("s1"),
            &[span("git-commit", 999, 999, 9.0)],
            &[],
        ))
        .unwrap();

    let usage = store.skill_usage(TimeRange::ALL).unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].invocations, 1);
    assert_eq!(usage[0].out_tokens, 999);
}

/// A skill surface whose body weighs `static_tokens` and whose description
/// (the only part loaded at startup) weighs `startup_tokens`.
fn skill_surface(id: &str, scope: Scope, static_tokens: u64, startup_tokens: u64) -> Surface {
    Surface {
        kind: "skill".to_string(),
        id: id.to_string(),
        scope,
        config_path: format!("/cfg/{id}"),
        static_tokens: Some(static_tokens),
        startup_tokens: Some(startup_tokens),
        load_mode: LoadMode::StartupDescription,
        paths: Vec::new(),
        description: None,
    }
}

fn surface(id: &str, scope: Scope, static_tokens: u64) -> Surface {
    skill_surface(id, scope, static_tokens, 0)
}

#[test]
fn a_project_surface_shadows_global_only_for_its_own_sessions() {
    // skill/git-commit is installed globally AND in project alpha; alpha and
    // beta each invoke it once. Alpha's use lands on alpha's project row,
    // beta's on the global row — two uses total, never double-counted, and
    // beta's usage never inflates alpha's copy.
    let mut store = Store::in_memory().unwrap();
    let mut alpha = session("a1");
    alpha.project = "alpha".to_string();
    let mut beta = session("b1");
    beta.project = "beta".to_string();
    store
        .ingest(IngestBatch::new(
            &alpha,
            &[span("git-commit", 1, 1, 1.0)],
            &[],
        ))
        .unwrap();
    store
        .ingest(IngestBatch::new(
            &beta,
            &[span("git-commit", 1, 1, 1.0)],
            &[],
        ))
        .unwrap();
    store
        .replace_surfaces(&[
            surface("git-commit", Scope::Global, 100),
            surface("git-commit", Scope::Project("alpha".to_string()), 250),
        ])
        .unwrap();

    let catalog = store.effective_catalog(TimeRange::ALL).unwrap();

    assert_eq!(catalog.len(), 2);
    let global = catalog.iter().find(|e| e.scope == "global").unwrap();
    let project = catalog.iter().find(|e| e.scope == "project").unwrap();
    assert_eq!(project.project, "alpha");
    assert_eq!(project.static_tokens, Some(250));
    assert_eq!(project.uses, 1); // alpha's invocation only
    assert_eq!(global.uses, 1); // beta's invocation only
}

#[test]
fn an_unshadowed_global_surface_counts_usage_from_every_project() {
    let mut store = Store::in_memory().unwrap();
    let mut alpha = session("a1");
    alpha.project = "alpha".to_string();
    let mut beta = session("b1");
    beta.project = "beta".to_string();
    store
        .ingest(IngestBatch::new(
            &alpha,
            &[span("pr-create", 1, 1, 1.0)],
            &[],
        ))
        .unwrap();
    store
        .ingest(IngestBatch::new(
            &beta,
            &[span("pr-create", 1, 1, 1.0)],
            &[],
        ))
        .unwrap();
    store
        .replace_surfaces(&[surface("pr-create", Scope::Global, 40)])
        .unwrap();

    let catalog = store.effective_catalog(TimeRange::ALL).unwrap();
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].uses, 2);
}

#[test]
fn session_roots_are_distinct_pairs_and_skip_unknown() {
    let mut store = Store::in_memory().unwrap();
    let mut a = session("a1");
    a.root = "/tmp/example/app".to_string();
    a.project = "alpha".to_string();
    let mut b = session("b1");
    b.root = "/tmp/example/app".to_string(); // duplicate root
    b.project = "alpha".to_string();
    let c = session("c1"); // root unknown (empty)
    store.ingest(IngestBatch::new(&a, &[], &[])).unwrap();
    store.ingest(IngestBatch::new(&b, &[], &[])).unwrap();
    store.ingest(IngestBatch::new(&c, &[], &[])).unwrap();

    assert_eq!(
        store.session_roots().unwrap(),
        vec![("/tmp/example/app".to_string(), "alpha".to_string())]
    );
}

/// A store as an older saniwalens left it: `sessions` without the columns later
/// releases added, and `surfaces` still keyed without `project`.
fn legacy_store() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE sessions (
                 id          TEXT PRIMARY KEY,
                 project     TEXT NOT NULL,
                 slug        TEXT NOT NULL,
                 source_path TEXT NOT NULL,
                 started_at  TEXT NOT NULL
             );
             CREATE TABLE surfaces (
                 kind          TEXT NOT NULL,
                 id            TEXT NOT NULL,
                 scope         TEXT NOT NULL,
                 config_path   TEXT,
                 static_tokens INTEGER,
                 load_mode     TEXT NOT NULL,
                 PRIMARY KEY (kind, id, scope)
             );",
    )
    .unwrap();
    conn
}

#[test]
fn a_legacy_store_gains_missing_columns_without_losing_its_history() {
    // The rows are irreplaceable once Claude Code has pruned the source
    // transcripts, so upgrading must add the columns in place.
    let conn = legacy_store();
    conn.execute(
        "INSERT INTO sessions (id, project, slug, source_path, started_at)
             VALUES ('s1', 'demo', 'demo', '/tmp/example/s1.jsonl', '2026-01-01T00:00:00Z')",
        (),
    )
    .unwrap();

    let store = Store::from_connection(conn).expect("must migrate, not refuse");

    let (_, rows) = store
        .query("SELECT id, root, sub_tokens FROM sessions")
        .unwrap();
    assert_eq!(
        rows,
        vec![vec!["s1".to_string(), String::new(), "0".to_string()]],
        "the pre-existing session must survive, with the added columns defaulted"
    );
}

#[test]
fn a_legacy_catalog_table_is_rebuilt_when_its_shape_drifted() {
    // `surfaces` is a snapshot of live config, rebuilt on every analyze, so
    // a shape change is recreated rather than patched column by column.
    let conn = legacy_store();

    let store = Store::from_connection(conn).expect("must migrate, not refuse");

    let names: Vec<String> = table_shape(&store.conn, "surfaces")
        .unwrap()
        .into_iter()
        .map(|column| column.name)
        .collect();
    assert!(names.contains(&"project".to_string()), "{names:?}");
}

#[test]
fn an_up_to_date_catalog_survives_reopening() {
    // Reopening must not drop `surfaces` merely because the store predates
    // version stamping — a `--frozen` read would then report an empty catalog.
    let mut store = Store::in_memory().unwrap();
    store
        .replace_surfaces(&[surface("git-commit", Scope::Global, 1)])
        .unwrap();
    store
        .conn
        .execute_batch("PRAGMA user_version = 0;")
        .unwrap();

    let store = Store::from_connection(store.conn).unwrap();

    assert_eq!(store.effective_catalog(TimeRange::ALL).unwrap().len(), 1);
}

#[test]
fn a_stale_view_definition_is_replaced_on_open() {
    // `CREATE VIEW IF NOT EXISTS` leaves an older definition in place, so a
    // reader would silently query yesterday's column mapping.
    let conn = legacy_store();
    conn.execute_batch("CREATE VIEW tool_errors AS SELECT id AS session_id FROM sessions;")
        .unwrap();

    let store = Store::from_connection(conn).expect("must migrate, not refuse");

    let sql: String = store
        .conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'view' AND name = 'tool_errors'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(sql.contains("kind = 'tool_error'"), "stale view: {sql}");
}

#[test]
fn opening_stamps_the_current_schema_version() {
    let store = Store::from_connection(legacy_store()).unwrap();

    let version: i64 = store
        .conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
}

#[test]
fn a_store_written_by_a_newer_saniwalens_is_refused_without_advising_deletion() {
    // Writing to it with this binary's schema would corrupt history the
    // newer saniwalens can still read; deleting it would throw that history away.
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!("PRAGMA user_version = {};", SCHEMA_VERSION + 1))
        .unwrap();

    let err = Store::from_connection(conn).err().expect("must be refused");

    let msg = err.to_string();
    assert!(msg.contains("upgrade"), "{msg}");
    assert!(!msg.contains("delete"), "must not advise deletion: {msg}");
}

#[test]
fn the_version_guard_waits_for_a_concurrent_writer_before_refusing() {
    use std::sync::mpsc;
    use std::time::Duration;

    // The guard reads user_version inside the migration's BEGIN IMMEDIATE,
    // so while another writer holds the store the guard cannot even run: a
    // refusal that raced past the lock would decide on a version a
    // concurrent saniwalens may be moving (the TOCTOU `migrate` closes).
    let dir = std::env::temp_dir().join(format!(
        "saniwalens-guard-refuse-test-{}",
        std::process::id()
    ));
    let path = dir.join("saniwalens.db");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let fixture = Connection::open(&path).unwrap();
    fixture
        .execute_batch(&format!("PRAGMA user_version = {};", SCHEMA_VERSION + 1))
        .unwrap();
    drop(fixture);

    let holder = Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE;").unwrap();

    let (done, rx) = mpsc::channel();
    let store_path = path.clone();
    let handle = std::thread::spawn(move || {
        let conn = Connection::open(&store_path).unwrap();
        let _ = done.send(Store::from_connection(conn).map(|_| ()));
    });

    let held = rx.recv_timeout(Duration::from_millis(500));
    assert!(
        held.is_err(),
        "the guard must wait for the concurrent writer, not refuse past it: {held:?}"
    );

    holder.execute_batch("ROLLBACK;").unwrap();

    let outcome = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the guard must run once the writer releases the store");
    handle.join().unwrap();

    let msg = outcome
        .expect_err("a store one version ahead must still be refused")
        .to_string();
    assert!(msg.contains("upgrade"), "{msg}");
    assert!(!msg.contains("delete"), "must not advise deletion: {msg}");

    let untouched = Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    let version: i64 = untouched
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION + 1, "the refusal must not stamp");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn migrate_waits_out_a_concurrent_writer_then_migrates_on_release() {
    use std::sync::mpsc;
    use std::time::Duration;

    // Same-version concurrency: waiting must end in a migration, not in the
    // old "database is locked" hard failure — the busy timeout set in
    // `from_connection`, before migrate's transaction, is what turns the
    // guard's BEGIN IMMEDIATE into a wait.
    let dir =
        std::env::temp_dir().join(format!("saniwalens-guard-wait-test-{}", std::process::id()));
    let path = dir.join("saniwalens.db");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut seeded = Store::open(&path, OpenMode::ReadWrite).unwrap();
    seeded
        .replace_surfaces(&[surface("git-commit", Scope::Global, 1)])
        .unwrap();
    drop(seeded);

    let holder = Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE;").unwrap();

    let (done, rx) = mpsc::channel();
    let store_path = path.clone();
    let handle = std::thread::spawn(move || {
        let _ = done.send(Store::open(&store_path, OpenMode::ReadWrite).map(|_| ()));
    });

    let held = rx.recv_timeout(Duration::from_millis(500));
    assert!(
        held.is_err(),
        "migrate must not run its transaction past a concurrent writer's lock: {held:?}"
    );

    holder.execute_batch("ROLLBACK;").unwrap();

    let outcome = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("migrate must complete once the writer releases the store");
    handle.join().unwrap();
    outcome.expect("the waiting open must succeed");

    let check = Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap();
    let version: i64 = check
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    let count: i64 = check
        .query_row("SELECT count(*) FROM surfaces", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1, "the seeded surface must survive the wait");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_refused_migration_leaves_the_store_untouched() {
    // The guard sits before any DDL inside the transaction, so its bail
    // drops an untouched transaction — the same connection stays usable and
    // user_version keeps exactly the value the newer binary wrote.
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!("PRAGMA user_version = {};", SCHEMA_VERSION + 1))
        .unwrap();

    migrate(&mut conn, SCHEMA).expect_err("must be refused");

    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION + 1);
}

#[test]
fn a_column_that_cannot_be_added_in_place_is_reported() {
    // SQLite cannot add a NOT NULL column without a default; failing loudly
    // beats leaving the store half-migrated.
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE t (id TEXT PRIMARY KEY);")
        .unwrap();
    let desired = Connection::open_in_memory().unwrap();
    desired
        .execute_batch("CREATE TABLE t (id TEXT PRIMARY KEY, tag TEXT NOT NULL);")
        .unwrap();

    let err = reconcile_columns(&conn, &desired, "t").expect_err("must be reported");

    assert!(err.to_string().contains("tag"), "{err}");
}

#[test]
fn a_column_whose_declaration_drifted_is_reported() {
    // Only additive drift is reconcilable: SQLite cannot retype or re-key a
    // column in place, so a declaration change that reconciliation would
    // silently skip has to fail while it is still a development mistake.
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE t (id TEXT PRIMARY KEY, n TEXT NOT NULL DEFAULT '');")
        .unwrap();
    let desired = Connection::open_in_memory().unwrap();
    desired
        .execute_batch("CREATE TABLE t (id TEXT PRIMARY KEY, n INTEGER NOT NULL DEFAULT 0);")
        .unwrap();

    let err = reconcile_columns(&conn, &desired, "t").expect_err("must be reported");

    assert!(err.to_string().contains('n'), "{err}");
}

#[test]
fn a_failed_migration_leaves_the_store_untouched() {
    // The whole point of migrating in place is that the file is never left
    // worse than it was found, so a bail partway through must roll back the
    // columns already added.
    let mut conn = legacy_store();
    conn.execute_batch(
        "CREATE TABLE events (
                 id            INTEGER PRIMARY KEY,
                 session_id    TEXT NOT NULL,
                 source_path   TEXT NOT NULL,
                 kind          TEXT NOT NULL,
                 started_at    TEXT NOT NULL,
                 started_epoch INTEGER NOT NULL,
                 duration_sec  REAL NOT NULL,
                 out_tokens    TEXT NOT NULL,
                 ctx_growth    INTEGER NOT NULL,
                 ctx_start     INTEGER NOT NULL,
                 ctx_peak      INTEGER NOT NULL
             );",
    )
    .unwrap();

    migrate(&mut conn, SCHEMA).expect_err("the drifted events.out_tokens must be reported");

    let names: Vec<String> = table_shape(&conn, "sessions")
        .unwrap()
        .into_iter()
        .map(|column| column.name)
        .collect();
    assert!(
        !names.contains(&"root".to_string()),
        "sessions was altered before the bail and not rolled back: {names:?}"
    );
}

#[test]
fn a_drifted_index_is_recreated_from_the_declaration() {
    // `CREATE INDEX IF NOT EXISTS` leaves an older definition in place, so
    // the store would keep an index the declaration no longer describes.
    let store = Store::in_memory().unwrap();
    store
        .conn
        .execute_batch(
            "DROP INDEX events_by_surface;
                 CREATE INDEX events_by_surface ON events(surface_id);",
        )
        .unwrap();

    let store = Store::from_connection(store.conn).unwrap();

    let sql: String = store
        .conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'events_by_surface'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(sql.contains("surface_kind"), "stale index: {sql}");
}

#[test]
fn rebuilding_the_catalog_clears_the_analyze_timestamp() {
    // The rebuilt catalog is empty until the next analyze repopulates it, and
    // a `--frozen` read would otherwise report that emptiness as a finished
    // analysis. Dropping the timestamp routes it to the freshness warning.
    let mut store = Store::in_memory().unwrap();
    store
        .set_meta("analyzed_at", "2026-01-01T00:00:00Z")
        .unwrap();
    store
        .conn
        .execute_batch(
            "DROP TABLE surfaces;
                 CREATE TABLE surfaces (
                     kind TEXT NOT NULL, id TEXT NOT NULL, scope TEXT NOT NULL,
                     config_path TEXT, static_tokens INTEGER, load_mode TEXT NOT NULL,
                     PRIMARY KEY (kind, id, scope)
                 );",
        )
        .unwrap();

    let store = Store::from_connection(store.conn).unwrap();

    assert_eq!(store.meta("analyzed_at").unwrap(), None);
}

#[test]
fn a_view_the_declaration_does_not_name_is_left_alone() {
    // The store is meant to be explored with ad-hoc SQL, so a helper view
    // someone saved into the file is theirs — reconciliation replaces what the
    // declaration names and touches nothing else.
    let store = Store::from_connection(Connection::open_in_memory().unwrap()).unwrap();
    store
        .conn
        .execute_batch("CREATE VIEW stray AS SELECT id FROM sessions;")
        .unwrap();

    let store = Store::from_connection(store.conn).expect("a second open must not collide");

    let views: Vec<String> = declared_sql(&store.conn, "view")
        .unwrap()
        .into_keys()
        .collect();
    assert_eq!(views, vec!["stray".to_string(), "tool_errors".to_string()]);
}

#[test]
fn an_index_is_created_after_the_columns_it_covers() {
    // Indexes are declared apart from the tables so they are applied only
    // after column reconciliation. Declaring one alongside the table would
    // run it against a legacy shape and fail the whole migration — the exact
    // destructive refusal in-place upgrading exists to remove.
    let conn = legacy_store();
    conn.execute_batch(
        "CREATE TABLE events (
                 id            INTEGER PRIMARY KEY,
                 session_id    TEXT NOT NULL,
                 source_path   TEXT NOT NULL,
                 kind          TEXT NOT NULL,
                 surface_id    TEXT,
                 started_at    TEXT NOT NULL,
                 started_epoch INTEGER NOT NULL,
                 duration_sec  REAL NOT NULL,
                 out_tokens    INTEGER NOT NULL,
                 ctx_growth    INTEGER NOT NULL,
                 ctx_start     INTEGER NOT NULL,
                 ctx_peak      INTEGER NOT NULL
             );",
    )
    .unwrap();

    let store = Store::from_connection(conn).expect("must migrate, not refuse");

    let sql = declared_sql(&store.conn, "index").unwrap();
    assert!(sql["events_by_surface"].contains("surface_kind"), "{sql:?}");
}

#[test]
fn a_catalog_without_the_startup_cost_column_is_rebuilt() {
    // A catalog weighed before startup cost was split out would silently
    // answer every "what does removing this save" question with nothing, so
    // it must not survive the upgrade as it is. `surfaces` is regenerated
    // from live config on every analyze, so reconciling it costs nothing to
    // drop — and clearing `analyzed_at` is what routes a `--frozen` read to
    // the freshness warning instead of reporting the empty catalog as a
    // finding.
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE surfaces (kind TEXT NOT NULL, id TEXT NOT NULL,
                                    scope TEXT NOT NULL, project TEXT NOT NULL,
                                    static_tokens INTEGER, load_mode TEXT NOT NULL);
             CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO meta (key, value) VALUES ('analyzed_at', '2026-01-01T00:00:00Z');",
    )
    .unwrap();

    let store = Store::from_connection(conn).expect("must migrate, not refuse");

    let names: Vec<String> = table_shape(&store.conn, "surfaces")
        .unwrap()
        .into_iter()
        .map(|column| column.name)
        .collect();
    assert!(names.contains(&"startup_tokens".to_string()), "{names:?}");
    assert_eq!(store.meta("analyzed_at").unwrap(), None);
}

#[test]
fn a_store_without_subagent_runs_gains_the_table_and_is_re_extracted() {
    // An existing store holds the events but not the runs, and its
    // transcripts have stopped changing — so nothing would ever populate the
    // new table, silently, since every query still succeeds and returns
    // nothing. Two mechanisms cover it, and neither alone would: `migrate`
    // supplies the table, and the analyzer version is what gets the rows
    // extracted into it.
    let mut store = Store::in_memory().unwrap();
    store
        .record_ingested_file("/tmp/example/s1.jsonl", 1, 2)
        .unwrap();
    store
        .conn
        .execute_batch("DROP TABLE subagent_runs")
        .unwrap();
    // Stamped by a build that predates subagent extraction.
    store.set_meta(TEST_ANALYZER_KEY, "0").unwrap();

    let store = Store::from_connection(store.conn).unwrap();

    assert!(table_exists(&store.conn, "subagent_runs").unwrap());
    assert!(
        !store
            .is_analyzer_current(TEST_ANALYZER_KEY, TEST_ANALYZER_VERSION)
            .unwrap()
    );
}

#[test]
fn subagent_runs_column_drift_is_reconciled_and_its_rows_survive() {
    // The red/green line for the classification guard: a store whose
    // `subagent_runs` predates a declared column is exactly the drift a
    // future column addition produces, and the one case the "table absent
    // → created" test cannot see. The table holds run history the
    // transcripts may no longer contain, so the drift must converge in
    // place — silently keeping the old shape would make every later
    // INSERT fail on the missing column.
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
            "CREATE TABLE subagent_runs (
                 id            INTEGER PRIMARY KEY,
                 session_id    TEXT NOT NULL,
                 source_path   TEXT NOT NULL,
                 run_path      TEXT NOT NULL,
                 agent         TEXT,
                 agent_id      TEXT NOT NULL,
                 started_at    TEXT NOT NULL,
                 started_epoch INTEGER NOT NULL
             );
             INSERT INTO subagent_runs (session_id, source_path, run_path, agent_id, started_at, started_epoch)
             VALUES ('s1', '/tmp/example/s1.jsonl', '/tmp/example/s1-child.jsonl',
                     'agent-1', '2026-01-01T00:00:00Z', 1767225600);",
        )
        .unwrap();

    let store = Store::from_connection(conn).expect("must migrate, not refuse");

    let declared = Connection::open_in_memory().unwrap();
    declared.execute_batch(SCHEMA).unwrap();
    // Sorted by name: ALTER TABLE appends, so physical order legitimately
    // differs after an in-place reconcile; what must match is the set of
    // columns and each one's declared type/default.
    let mut shape = table_shape(&store.conn, "subagent_runs").unwrap();
    shape.sort_by(|a, b| a.name.cmp(&b.name));
    let mut target = table_shape(&declared, "subagent_runs").unwrap();
    target.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(
        shape, target,
        "subagent_runs must converge onto the declared shape"
    );
    let (_, rows) = store
        .query("SELECT session_id, agent_id, started_at FROM subagent_runs")
        .unwrap();
    assert_eq!(
        rows,
        vec![vec![
            "s1".to_string(),
            "agent-1".to_string(),
            "2026-01-01T00:00:00Z".to_string()
        ]],
        "the pre-existing run is irreplaceable once the transcript is pruned"
    );
}

#[test]
fn migrate_refuses_a_declared_table_no_convergence_list_classifies() {
    // The convergence loop visits only the classified lists, so a table
    // declared without classification would keep its stored shape on every
    // existing store — the guard exists so the next `subagent_runs` cannot
    // happen. The mistake cannot exist in a correct `SCHEMA`, so the test
    // replays it through the same seam a future release would trip on.
    let mut conn = Connection::open_in_memory().unwrap();
    let mut future_schema = SCHEMA.to_string();
    future_schema.push_str(
        "CREATE TABLE IF NOT EXISTS future_table (
                 id    INTEGER PRIMARY KEY,
                 value TEXT NOT NULL
             );",
    );

    let err = migrate(&mut conn, &future_schema)
        .expect_err("a declared table the lists classify nowhere must be refused");

    assert!(err.to_string().contains("future_table"), "{err}");
}

#[test]
fn reopening_a_current_store_keeps_its_fingerprints() {
    // Migration reconciles shape only. Re-running analyze against a store
    // the current analyzer already wrote must stay cheap.
    let mut store = Store::in_memory().unwrap();
    store
        .record_ingested_file("/tmp/example/s1.jsonl", 1, 2)
        .unwrap();
    store
        .record_analyzer_version(TEST_ANALYZER_KEY, TEST_ANALYZER_VERSION)
        .unwrap();
    let store = Store::from_connection(store.conn).unwrap();

    assert!(
        store
            .is_analyzer_current(TEST_ANALYZER_KEY, TEST_ANALYZER_VERSION)
            .unwrap()
    );
    assert!(store.is_ingested("/tmp/example/s1.jsonl", 1, 2).unwrap());
}

fn span_at_ctx(skill: &str, ctx_start: u64) -> Span {
    Span {
        skill: skill.to_string(),
        source: Source::Slash,
        started_epoch_ms: 1_700_000_000_000,
        duration_sec: 1.0,
        out_tokens: 10,
        ctx_growth: 5,
        ctx_start,
        ctx_peak: ctx_start,
        model: None,
        is_trailing: false,
        spawns: Vec::new(),
        sub_tokens: 0,
        sub_agent_count: 0,
        sub_tokens_estimated: false,
    }
}

#[test]
fn baseline_floor_is_the_leanest_session_start_per_project() {
    let mut store = Store::in_memory().unwrap();
    // Project "alpha" started two sessions at 30000 and 12000; "beta" one
    // at 40000.
    let mut alpha1 = session("a1");
    alpha1.project = "alpha".to_string();
    alpha1.start = Some(SessionStart {
        timestamp_ms: 1_700_000_000_000,
        ctx: 30000,
    });
    store.ingest(IngestBatch::new(&alpha1, &[], &[])).unwrap();
    let mut alpha2 = session("a2");
    alpha2.project = "alpha".to_string();
    alpha2.start = Some(SessionStart {
        timestamp_ms: 1_700_000_000_000,
        ctx: 12000,
    });
    store.ingest(IngestBatch::new(&alpha2, &[], &[])).unwrap();
    let mut beta = session("b1");
    beta.project = "beta".to_string();
    beta.start = Some(SessionStart {
        timestamp_ms: 1_700_000_000_000,
        ctx: 40000,
    });
    store.ingest(IngestBatch::new(&beta, &[], &[])).unwrap();

    assert_eq!(store.baseline_floor(TimeRange::ALL).unwrap(), Some(12000));
    assert_eq!(
        store.baseline_floor_per_project(TimeRange::ALL).unwrap(),
        vec![
            ("beta".to_string(), 40000, 1),
            ("alpha".to_string(), 12000, 2)
        ]
    );
}

#[test]
fn baseline_floor_ignores_the_context_a_skill_happened_to_run_at() {
    let mut store = Store::in_memory().unwrap();
    // A session that started lean but only invoked a skill deep into a long
    // conversation. The span's ctx_start is not a floor observation.
    let mut meta = session("s1");
    meta.start = Some(SessionStart {
        timestamp_ms: 1_700_000_000_000,
        ctx: 50000,
    });
    store
        .ingest(IngestBatch::new(
            &meta,
            &[span_at_ctx("git-commit", 430000)],
            &[],
        ))
        .unwrap();

    assert_eq!(store.baseline_floor(TimeRange::ALL).unwrap(), Some(50000));
}

#[test]
fn a_session_start_event_is_stamped_with_when_the_session_began() {
    // `sql` exposes raw event rows, so an unstamped row would place every
    // session start in 1970.
    let mut store = Store::in_memory().unwrap();
    let mut meta = session("s1");
    meta.start = Some(SessionStart {
        timestamp_ms: 1_700_000_000_000,
        ctx: 50000,
    });
    store.ingest(IngestBatch::new(&meta, &[], &[])).unwrap();

    let (_, rows) = store
        .query("SELECT started_epoch, started_at FROM events WHERE kind = 'session_start'")
        .unwrap();
    assert_eq!(rows[0][0], "1700000000");
    assert!(!rows[0][1].is_empty());
}

#[test]
fn a_session_without_spans_still_records_when_it_started() {
    // started_at was derived only from spans, so a session that invoked no
    // skill was stored with an empty start.
    let mut store = Store::in_memory().unwrap();
    let mut meta = session("s1");
    meta.start = Some(SessionStart {
        timestamp_ms: 1_700_000_000_000,
        ctx: 50000,
    });
    store.ingest(IngestBatch::new(&meta, &[], &[])).unwrap();

    let (_, rows) = store
        .query("SELECT started_at FROM sessions WHERE id = 's1'")
        .unwrap();
    assert!(!rows[0][0].is_empty());
}

#[test]
fn baseline_floor_is_absent_when_no_session_start_was_observed() {
    let mut store = Store::in_memory().unwrap();
    let meta = session("s1");
    store
        .ingest(IngestBatch::new(
            &meta,
            &[span_at_ctx("git-commit", 430000)],
            &[],
        ))
        .unwrap();

    assert_eq!(store.baseline_floor(TimeRange::ALL).unwrap(), None);
    assert_eq!(
        store.baseline_floor_per_project(TimeRange::ALL).unwrap(),
        vec![]
    );
}

#[test]
fn always_on_config_sums_every_surface_startup_cost() {
    let mut store = Store::in_memory().unwrap();
    store
        .replace_surfaces(&[
            Surface {
                kind: "claude_md".to_string(),
                id: "global".to_string(),
                scope: Scope::Global,
                config_path: "/c/CLAUDE.md".to_string(),
                static_tokens: Some(600),
                startup_tokens: Some(600),
                load_mode: LoadMode::StartupFull,
                paths: Vec::new(),
                description: None,
            },
            Surface {
                kind: "rule".to_string(),
                id: "git/safety".to_string(),
                scope: Scope::Global,
                config_path: "/c/safety.md".to_string(),
                static_tokens: Some(900),
                startup_tokens: Some(900),
                load_mode: LoadMode::StartupFull,
                paths: Vec::new(),
                description: None,
            },
            // A skill contributes its description, never its 1000-token body.
            skill_surface("git-commit", Scope::Global, 1000, 8),
            // An MCP server's schema is real but unweighable — it must not
            // land in the sum as a false zero-cost row either way.
            Surface {
                kind: "mcp_server".to_string(),
                id: "playwright".to_string(),
                scope: Scope::Global,
                config_path: "/c/.mcp.json".to_string(),
                static_tokens: None,
                startup_tokens: None,
                load_mode: LoadMode::ToolSchema,
                paths: Vec::new(),
                description: None,
            },
            // Another project's CLAUDE.md is always-on *there*, not globally.
            Surface {
                kind: "claude_md".to_string(),
                id: "project".to_string(),
                scope: Scope::Project("alpha".to_string()),
                config_path: "/tmp/example/CLAUDE.md".to_string(),
                static_tokens: Some(400),
                startup_tokens: Some(400),
                load_mode: LoadMode::StartupFull,
                paths: Vec::new(),
                description: None,
            },
        ])
        .unwrap();

    // The global figure excludes project config; a session in alpha pays
    // the global floor plus alpha's own always-on config.
    assert_eq!(store.always_on_config_tokens().unwrap(), 1508);
    assert_eq!(store.always_on_config_tokens_for("alpha").unwrap(), 1908);
}

#[test]
fn the_catalog_carries_startup_and_body_cost_apart() {
    let mut store = Store::in_memory().unwrap();
    store
        .replace_surfaces(&[skill_surface("repro", Scope::Global, 1015, 7)])
        .unwrap();

    let catalog = store.effective_catalog(TimeRange::ALL).unwrap();
    assert_eq!(catalog[0].static_tokens, Some(1015));
    assert_eq!(catalog[0].startup_tokens, Some(7));
}

#[test]
fn replace_surfaces_rebuilds_wholesale() {
    let mut store = Store::in_memory().unwrap();
    store
        .replace_surfaces(&[surface("old", Scope::Global, 1)])
        .unwrap();
    store
        .replace_surfaces(&[surface("new", Scope::Global, 1)])
        .unwrap();

    let catalog = store.effective_catalog(TimeRange::ALL).unwrap();
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].id, "new");
}

#[test]
fn meta_round_trips_and_overwrites() {
    let mut store = Store::in_memory().unwrap();
    assert_eq!(store.meta("analyzed_at").unwrap(), None);
    store
        .set_meta("analyzed_at", "2026-01-01T00:00:00Z")
        .unwrap();
    store
        .set_meta("analyzed_at", "2026-01-02T00:00:00Z")
        .unwrap();
    assert_eq!(
        store.meta("analyzed_at").unwrap().as_deref(),
        Some("2026-01-02T00:00:00Z")
    );
}

#[test]
fn an_unchanged_fingerprint_marks_a_file_ingested() {
    let mut store = Store::in_memory().unwrap();
    // Never seen: needs ingest.
    assert!(!store.is_ingested("/tmp/a.jsonl", 100, 5).unwrap());
    store.record_ingested_file("/tmp/a.jsonl", 100, 5).unwrap();
    // Same (mtime, size): skip. Any change: re-ingest.
    assert!(store.is_ingested("/tmp/a.jsonl", 100, 5).unwrap());
    assert!(!store.is_ingested("/tmp/a.jsonl", 101, 5).unwrap());
    assert!(!store.is_ingested("/tmp/a.jsonl", 100, 6).unwrap());
    // A new fingerprint supersedes the old one.
    store.record_ingested_file("/tmp/a.jsonl", 101, 6).unwrap();
    assert!(store.is_ingested("/tmp/a.jsonl", 101, 6).unwrap());
}

#[test]
fn a_store_is_stale_until_stamped_with_the_current_analyzer() {
    let mut store = Store::in_memory().unwrap();
    // A store written by an older analyzer (or none at all) is stale, so
    // `analyze` re-ingests even the transcripts whose fingerprint matches.
    assert!(
        !store
            .is_analyzer_current(TEST_ANALYZER_KEY, TEST_ANALYZER_VERSION)
            .unwrap()
    );
    store.set_meta(TEST_ANALYZER_KEY, "0").unwrap();
    assert!(
        !store
            .is_analyzer_current(TEST_ANALYZER_KEY, TEST_ANALYZER_VERSION)
            .unwrap()
    );
    store
        .record_analyzer_version(TEST_ANALYZER_KEY, TEST_ANALYZER_VERSION)
        .unwrap();
    assert!(
        store
            .is_analyzer_current(TEST_ANALYZER_KEY, TEST_ANALYZER_VERSION)
            .unwrap()
    );
}

#[test]
fn error_counts_break_down_by_project_and_category() {
    let mut store = Store::in_memory().unwrap();
    // Two projects, the same category concentrated in one of them.
    let mut alpha = session("a1");
    alpha.project = "alpha".to_string();
    let mut beta = session("b1");
    beta.project = "beta".to_string();
    store
        .ingest(IngestBatch::new(&alpha, &[], &[]).tool_errors(&[
            (100, "edit-precondition", "x", "Edit", "f"),
            (200, "edit-precondition", "y", "Edit", "f"),
        ]))
        .unwrap();
    store
        .ingest(IngestBatch::new(&beta, &[], &[]).tool_errors(&[(
            300,
            "edit-precondition",
            "z",
            "Write",
            "f",
        )]))
        .unwrap();

    let rows = store.error_counts_by_project(TimeRange::ALL).unwrap();

    // Densest (project, category) pair first — alpha owns the friction.
    assert_eq!(
        rows,
        vec![
            FrictionCell {
                project: "alpha".to_string(),
                category: "edit-precondition".to_string(),
                count: 2,
            },
            FrictionCell {
                project: "beta".to_string(),
                category: "edit-precondition".to_string(),
                count: 1,
            },
        ]
    );
}

#[test]
fn error_examples_are_capped_per_project_and_category() {
    let mut store = Store::in_memory().unwrap();
    let mut alpha = session("a1");
    alpha.project = "alpha".to_string();
    store
        .ingest(IngestBatch::new(&alpha, &[], &[]).tool_errors(&[
            (100, "path-not-found", "missing /a", "Read", "f"),
            (200, "path-not-found", "missing /b", "Read", "f"),
            (300, "path-not-found", "missing /c", "Bash", "f"),
        ]))
        .unwrap();

    // Two examples per group, earliest first — the third is dropped.
    let examples = store.error_examples(2, TimeRange::ALL).unwrap();
    assert_eq!(
        examples,
        vec![
            (
                "alpha".to_string(),
                "path-not-found".to_string(),
                "missing /a".to_string()
            ),
            (
                "alpha".to_string(),
                "path-not-found".to_string(),
                "missing /b".to_string()
            ),
        ]
    );
}

#[test]
fn session_stats_count_sessions_and_distinct_projects() {
    let mut store = Store::in_memory().unwrap();
    let mut a = session("a1");
    a.project = "alpha".to_string();
    let mut b = session("b1");
    b.project = "alpha".to_string();
    let mut c = session("c1");
    c.project = "beta".to_string();
    store.ingest(IngestBatch::new(&a, &[], &[])).unwrap();
    store.ingest(IngestBatch::new(&b, &[], &[])).unwrap();
    store.ingest(IngestBatch::new(&c, &[], &[])).unwrap();

    assert_eq!(store.session_stats(TimeRange::ALL).unwrap(), (3, 2));
}

#[test]
fn session_breakdown_reports_tokens_duration_and_friction() {
    // One session with a costed span plus a failed tool call; one quiet
    // session with nothing but its start. Friction-empty rows read 0,
    // never vanish — absence of friction is data, not a missing row.
    let mut store = Store::in_memory().unwrap();
    let meta = session("a1");
    store
        .ingest(
            IngestBatch::new(&meta, &[span("git-commit", 100, 0, 60.0)], &[]).tool_errors(&[(
                1_700_000_100_000,
                "timeout",
                "t",
                "Bash",
                "sleep 9",
            )]),
        )
        .unwrap();
    let meta = session("b1");
    store.ingest(IngestBatch::new(&meta, &[], &[])).unwrap();

    let mut rows = store.session_breakdown(TimeRange::ALL).unwrap();
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "a1");
    assert_eq!(rows[0].out_tokens, 100);
    assert_eq!(rows[0].friction, 1);
    assert_eq!(rows[1].id, "b1");
    assert_eq!(rows[1].out_tokens, 0);
    assert_eq!(rows[1].friction, 0);
}

#[test]
fn session_breakdown_window_keeps_matching_sessions() {
    // Bind-order regression: the JOIN's epoch binds precede the WHERE's
    // text binds in the SQL text, so they must bind first. Swapped order
    // silently returned zero rows under any window (found during PBI C4).
    let mut store = Store::in_memory().unwrap();
    let meta = session("a1");
    store
        .ingest(
            IngestBatch::new(&meta, &[span("git-commit", 100, 0, 60.0)], &[]).tool_errors(&[(
                1_700_000_100_000,
                "timeout",
                "t",
                "Bash",
                "sleep 9",
            )]),
        )
        .unwrap();
    let meta = session("b1");
    store.ingest(IngestBatch::new(&meta, &[], &[])).unwrap();

    // The span's epoch (1_700_000_000s) inside the window: the costed
    // session matches with its tokens and friction intact.
    let window = TimeRange {
        start: Some(1_699_999_000),
        end: Some(1_700_001_000),
    };
    let rows = store.session_breakdown(window).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "a1");
    assert_eq!(rows[0].out_tokens, 100);
    assert_eq!(rows[0].friction, 1);

    // A window with nothing in it reports nothing — never a silent zero.
    let outside = TimeRange {
        start: Some(1_600_000_000),
        end: Some(1_600_001_000),
    };
    assert!(store.session_breakdown(outside).unwrap().is_empty());
}

#[test]
fn work_events_and_tool_errors_carry_a_readable_timestamp() {
    // The epoch is not enough for a sql read: an advisor drilling into a
    // session wants the wall-clock time without re-deriving it, and the
    // headless walkthrough stalled on the empty column.
    let mut store = Store::in_memory().unwrap();
    let a1 = session("a1");
    store
        .ingest(
            IngestBatch::new(&a1, &[], &[])
                .tool_errors(&[(
                    2_000,
                    "path-not-found",
                    "no such file",
                    "read_file",
                    "/repo/src/y.rs",
                )])
                .work_events(&[(1_000, "file_edit", "x.rs", Some("/repo/src/x.rs"))]),
        )
        .unwrap();

    let rows = store
        .query("SELECT kind, started_at FROM events ORDER BY started_epoch")
        .unwrap()
        .1;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][1], "1970-01-01T00:00:01+00:00");
    assert_eq!(rows[1][1], "1970-01-01T00:00:02+00:00");
}

#[test]
fn file_edits_carry_their_project_session_and_full_path() {
    let mut store = Store::in_memory().unwrap();
    let mut alpha = session("a1");
    alpha.project = "alpha".to_string();
    store
        .ingest(IngestBatch::new(&alpha, &[], &[]).work_events(&[(
            1_000,
            "file_edit",
            "x.rs",
            Some("/repo/src/x.rs"),
        )]))
        .unwrap();

    assert_eq!(
        store.file_edits(TimeRange::ALL).unwrap(),
        vec![FileEdit {
            project: "alpha".to_string(),
            session_id: "a1".to_string(),
            path: "/repo/src/x.rs".to_string(),
            epoch: 1,
        }]
    );
}

#[test]
fn a_file_edit_without_a_recorded_path_is_excluded() {
    // Falling back to the basename would silently regroup same-named files —
    // the very merge this query exists to prevent — for every store written
    // before paths were recorded.
    let mut store = Store::in_memory().unwrap();
    let alpha = session("a1");
    store
        .ingest(IngestBatch::new(&alpha, &[], &[]).work_events(&[(
            1_000,
            "file_edit",
            "x.rs",
            None,
        )]))
        .unwrap();

    assert_eq!(store.file_edits(TimeRange::ALL).unwrap(), vec![]);
}

#[test]
fn prompt_refs_return_distinct_transcript_lines() {
    let mut store = Store::in_memory().unwrap();
    let alpha = session("a1");
    store
        .ingest(
            IngestBatch::new(&alpha, &[], &[])
                .prompts(&[(0, 1_700_000_000_000, "instruct")])
                .tool_errors(&[(1_000_000, "timeout", "t", "Bash", "sleep 9")]),
        )
        .unwrap();

    assert_eq!(
        store.prompt_refs(TimeRange::ALL).unwrap(),
        vec![("/tmp/a1.jsonl".to_string(), 0)]
    );
}

#[test]
fn query_json_preserves_sqlite_types() {
    let mut store = Store::in_memory().unwrap();
    let mut alpha = session("a1");
    alpha.project = "alpha".to_string();
    store.ingest(IngestBatch::new(&alpha, &[], &[])).unwrap();

    let (cols, rows) = store
        .query_json("SELECT project, COUNT(*) AS n, NULL AS absent, 1.5 AS ratio FROM sessions")
        .unwrap();

    assert_eq!(cols, vec!["project", "n", "absent", "ratio"]);
    // Numbers stay numbers, NULL stays null — a `--format json` consumer
    // must not need to re-parse strings.
    assert_eq!(
        rows,
        vec![vec![
            serde_json::json!("alpha"),
            serde_json::json!(1),
            serde_json::Value::Null,
            serde_json::json!(1.5),
        ]]
    );
}

#[test]
fn query_returns_columns_and_stringified_rows() {
    let mut store = Store::in_memory().unwrap();
    let mut alpha = session("a1");
    alpha.project = "alpha".to_string();
    store
        .ingest(IngestBatch::new(&alpha, &[], &[]).tool_errors(&[(
            100,
            "path-not-found",
            "missing /a",
            "Read",
            "f",
        )]))
        .unwrap();

    let (cols, rows) = store
        .query("SELECT category, tool, COUNT(*) AS n FROM tool_errors GROUP BY category, tool")
        .unwrap();
    assert_eq!(cols, vec!["category", "tool", "n"]);
    assert_eq!(
        rows,
        vec![vec![
            "path-not-found".to_string(),
            "Read".to_string(),
            "1".to_string()
        ]]
    );
}

#[test]
fn tool_errors_view_exposes_named_columns_and_project() {
    let mut store = Store::in_memory().unwrap();
    let mut alpha = session("a1");
    alpha.project = "demo--wt-feature".to_string();
    store
        .ingest(IngestBatch::new(&alpha, &[], &[]).tool_errors(&[(
            100,
            "path-not-found",
            "missing /a",
            "Read",
            "f",
        )]))
        .unwrap();

    // An ad-hoc query against the view sees clean names, not the overloaded
    // event columns, and the worktree filter works off `project`.
    let row: (String, String, String, String) = store
        .conn
        .query_row(
            "SELECT project, category, excerpt, tool FROM tool_errors \
                 WHERE project LIKE '%--wt%'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        row,
        (
            "demo--wt-feature".to_string(),
            "path-not-found".to_string(),
            "missing /a".to_string(),
            "Read".to_string(),
        )
    );
}

#[test]
fn error_tool_breakdown_splits_a_category_across_tools() {
    let mut store = Store::in_memory().unwrap();
    let mut alpha = session("a1");
    alpha.project = "alpha".to_string();
    store
        .ingest(IngestBatch::new(&alpha, &[], &[]).tool_errors(&[
            (100, "path-not-found", "missing /a", "Read", "f"),
            (200, "path-not-found", "missing /b", "Read", "f"),
            (300, "path-not-found", "missing /c", "Bash", "f"),
        ]))
        .unwrap();

    // path-not-found splits Read 2 / Bash 1, densest tool first.
    let rows = store.error_tool_breakdown(TimeRange::ALL).unwrap();
    assert_eq!(
        rows,
        vec![
            ToolBreakdown {
                project: "alpha".to_string(),
                category: "path-not-found".to_string(),
                tool: "Read".to_string(),
                count: 2,
            },
            ToolBreakdown {
                project: "alpha".to_string(),
                category: "path-not-found".to_string(),
                tool: "Bash".to_string(),
                count: 1,
            },
        ]
    );
}

// ---- PBI 02: batch ingest order, typed open guarantees, encoding table ----

fn dump(store: &Store, table: &str) -> Vec<Vec<String>> {
    store
        .query(&format!("SELECT * FROM {table} ORDER BY id"))
        .unwrap()
        .1
}

/// Ingest the batch a caller would submit for a fresh read of one file.
fn ingest_fresh(store: &mut Store, meta: &SessionMeta) {
    store
        .ingest(
            IngestBatch::new(
                meta,
                &[span("new-skill", 7, 7, 2.0)],
                &[agent_spawn("agent", "toolu_new")],
            )
            .prompts(&[(9, 1_700_000_100_000, "instruction")])
            .subagent_runs(&[subagent_run("Explore", "new", "toolu_new", 222)])
            .tool_errors(&[(2_000, "edit-precondition", "new", "Edit", "f")])
            .work_events(&[(2_000, "file_edit", "new.rs", Some("/repo/new.rs"))]),
        )
        .unwrap();
}

#[test]
fn a_batch_replaces_every_prior_row_for_the_file_wholesale() {
    // Order-independence the old five-function API could not express: a
    // store that previously held rows of every kind for this file must end
    // up byte-identical to a store that only ever saw the fresh batch.
    let mut seen = Store::in_memory().unwrap();
    let meta = session("s1");
    seen.ingest(
        IngestBatch::new(
            &meta,
            &[span("old-skill", 5, 5, 1.0)],
            &[agent_spawn("agent", "toolu_old")],
        )
        .prompts(&[(3, 1_700_000_000_000, "instruction")])
        .subagent_runs(&[subagent_run("Explore", "old", "toolu_old", 111)])
        .tool_errors(&[(1_000, "path-not-found", "old", "Read", "f")])
        .work_events(&[(1_000, "file_edit", "old.rs", Some("/repo/old.rs"))]),
    )
    .unwrap();
    ingest_fresh(&mut seen, &meta);

    let mut fresh = Store::in_memory().unwrap();
    ingest_fresh(&mut fresh, &meta);

    for table in ["sessions", "events", "subagent_runs"] {
        assert_eq!(
            dump(&seen, table),
            dump(&fresh, table),
            "{table} must not depend on what was ingested before"
        );
    }
}

#[test]
fn one_batch_settles_the_spawn_costs_of_the_runs_it_carries() {
    // The spawn-cost settle used to run in a separate call after
    // `ingest_session`; inside a batch it must settle in the same commit.
    let mut store = Store::in_memory().unwrap();
    let s1 = session("s1");
    store
        .ingest(
            IngestBatch::new(&s1, &[], &[agent_spawn("Explore", "toolu_a1")])
                .subagent_runs(&[subagent_run("Explore", "a1", "toolu_a1", 400)]),
        )
        .unwrap();

    let rows = store
        .query("SELECT out_tokens FROM events WHERE kind = 'agent_spawn'")
        .unwrap()
        .1;
    assert_eq!(rows, vec![vec!["400".to_string()]]);
}

#[test]
fn readonly_open_neither_creates_nor_migrates() {
    // The two guarantees `open`/`open_readonly` used to bury in their names
    // are now the `OpenMode` argument's contract.
    let dir = std::env::temp_dir().join(format!("saniwalens-readonly-test-{}", std::process::id()));
    let path = dir.join("store.db");
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        Store::open(&path, OpenMode::ReadOnly).is_err(),
        "a missing store is an error, never a fresh empty one"
    );
    assert!(!path.exists(), "readonly must not create the file");

    // An old-shape store opens readonly as-is: no migration runs, so the
    // missing column surfaces as SQLite's own error.
    std::fs::create_dir_all(&dir).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE sessions (
                 id          TEXT PRIMARY KEY,
                 project     TEXT NOT NULL,
                 slug        TEXT NOT NULL,
                 source_path TEXT NOT NULL,
                 started_at  TEXT NOT NULL
             );",
    )
    .unwrap();
    drop(conn);
    {
        let store = Store::open(&path, OpenMode::ReadOnly).unwrap();
        assert!(
            store.query("SELECT root FROM sessions").is_err(),
            "readonly must not migrate a legacy store"
        );
    }
    {
        let store = Store::open(&path, OpenMode::ReadWrite).unwrap();
        assert!(
            store.query("SELECT root FROM sessions").is_ok(),
            "ReadWrite is the migrating mode"
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_tool_errors_view_is_derived_from_the_encoding_table() {
    // Every meaning the view names must come from EVENT_ENCODING, not from
    // hand-written SQL that can drift away from it.
    let sql = tool_errors_view_sql(EVENT_ENCODING);
    let (_, roles) = EVENT_ENCODING
        .iter()
        .find(|(kind, _)| *kind == "tool_error")
        .expect("tool_error is in the encoding table");
    // Collapse padding so the assertion checks which column feeds which
    // alias, not how the generated text is laid out.
    let flat = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    for (role, column) in *roles {
        assert!(
            flat.contains(&format!("e.{column} AS {role}")),
            "view misses {role} → {column}: {sql}"
        );
    }
    assert!(flat.contains("WHERE e.kind = 'tool_error'"));
}

#[test]
fn adding_a_kind_to_the_encoding_table_leaves_the_view_untouched() {
    // A new event kind is a row in the table — existing view definitions
    // and readers must not need to change for it.
    let mut extended: Vec<(&str, &[(EventRole, EventColumn)])> = EVENT_ENCODING.to_vec();
    extended.push((
        "brand_new_kind",
        &[(EventRole::Category, EventColumn::Target)],
    ));

    assert_eq!(
        tool_errors_view_sql(EVENT_ENCODING),
        tool_errors_view_sql(&extended),
        "the view is defined only by the tool_error entry"
    );
}

// ---- PBI 2026-10-30-01: the report views' evaluation window ----

fn span_at(skill: &str, epoch_secs: i64, out_tokens: u64) -> Span {
    Span {
        skill: skill.to_string(),
        source: Source::Slash,
        started_epoch_ms: epoch_secs * 1000,
        duration_sec: 2.0,
        out_tokens,
        ctx_growth: 5,
        ctx_start: 0,
        ctx_peak: 5,
        model: None,
        is_trailing: false,
        spawns: Vec::new(),
        sub_tokens: 0,
        sub_agent_count: 0,
        sub_tokens_estimated: false,
    }
}

#[test]
fn a_range_narrows_event_aggregates_at_the_inclusive_bounds() {
    let mut store = Store::in_memory().unwrap();
    let s = session("s1");
    let spans = [
        span_at("commit", 1_000, 100),
        span_at("commit", 2_000, 200),
        span_at("commit", 3_000, 400),
    ];
    store.ingest(IngestBatch::new(&s, &spans, &[])).unwrap();

    // Both ends are inclusive: a one-second window catches the span that
    // starts at it, and nothing else.
    let rollup = store
        .skill_usage(TimeRange {
            start: Some(2_000),
            end: Some(2_000),
        })
        .unwrap();
    assert_eq!(rollup.len(), 1);
    assert_eq!(rollup[0].invocations, 1);
    assert_eq!(rollup[0].out_tokens, 200);

    // Without the period flags the whole spine comes back, unchanged.
    assert_eq!(store.skill_usage(TimeRange::ALL).unwrap()[0].invocations, 3);
}

#[test]
fn a_range_narrows_session_aggregates_by_the_stamped_utc_text() {
    let mut store = Store::in_memory().unwrap();
    // Last second of JST 2026-10-07: local midnight of the 8th is
    // 2026-10-07T15:00:00Z.
    let end = 1_791_385_200 - 1;
    // A session that started at 23:59:59.5 JST on the 7th — the millisecond
    // fraction sits inside the final second, so it stays in the window.
    let mut inside = session("inside");
    inside.sub_tokens = 400;
    inside.sub_agent_count = 1;
    inside.start = Some(SessionStart {
        timestamp_ms: end * 1000 + 500,
        ctx: 10_000,
    });
    store.ingest(IngestBatch::new(&inside, &[], &[])).unwrap();
    // A session that started at local midnight of the 8th — outside.
    let mut outside = session("outside");
    outside.sub_tokens = 500;
    outside.sub_agent_count = 1;
    outside.start = Some(SessionStart {
        timestamp_ms: (end + 1) * 1000,
        ctx: 11_000,
    });
    store.ingest(IngestBatch::new(&outside, &[], &[])).unwrap();

    let range = TimeRange {
        start: None,
        end: Some(end),
    };
    assert_eq!(store.subagent_totals(range).unwrap(), (400, 1));
    assert_eq!(store.session_stats(range).unwrap().0, 1);
    // Without the flags both sessions count.
    assert_eq!(store.subagent_totals(TimeRange::ALL).unwrap(), (900, 2));
}

#[test]
fn a_session_that_straddles_until_keeps_its_total_but_drops_the_late_runs() {
    // The two time axes: the total reads the session's stamped start, the
    // per-agent rows read each run's own epoch. A session that started
    // inside the window whose runs landed past `--until` keeps its total
    // while its late runs drop out of the rows — and the gap sentence
    // names the window, not the pruned transcripts.
    let mut store = Store::in_memory().unwrap();
    let end = 1_791_385_200 - 1; // the last second of JST 2026-10-07
    let mut s = session("s1");
    s.sub_tokens = 400;
    s.sub_agent_count = 2;
    s.start = Some(SessionStart {
        timestamp_ms: (end - 60) * 1000, // the session starts inside the window
        ctx: 10_000,
    });
    let mut early = subagent_run("Explore", "a1", "toolu_a1", 100);
    early.started_epoch_ms = (end - 60) * 1000; // inside the window
    let mut late = subagent_run("Explore", "a2", "toolu_a2", 300);
    late.started_epoch_ms = (end + 60) * 1000; // the run lands past --until
    store
        .ingest(IngestBatch::new(&s, &[], &[]).subagent_runs(&[early, late]))
        .unwrap();

    let range = TimeRange {
        start: None,
        end: Some(end),
    };
    // The total stays intact: the session started inside the window.
    assert_eq!(store.subagent_totals(range).unwrap(), (400, 2));
    // The rows keep only the run inside the window.
    let agents = store.subagent_by_agent(range).unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].out_tokens, 100);
    // Without the flags both runs come back unchanged.
    assert_eq!(store.subagent_totals(TimeRange::ALL).unwrap(), (400, 2));
    assert_eq!(store.subagent_by_agent(TimeRange::ALL).unwrap().len(), 1);

    // The straddle's gap sentence names the window, not the transcripts.
    let coverage = SplitCoverage::new(400, 2, agents.iter().map(|a| (a.runs, a.out_tokens)));
    let note = coverage.partial_note_in(range).unwrap();
    assert!(note.contains("fall outside the report window"), "{}", note);
    assert!(
        !note.contains("analyzed before runs were extracted"),
        "{}",
        note
    );
}

#[test]
fn inventory_narrows_the_usage_half_and_keeps_the_catalog_rows() {
    let mut store = Store::in_memory().unwrap();
    let s = session("s1");
    let spans = [span_at("commit", 1_000, 100)];
    let calls = [UsageEvent {
        surface_kind: "agent".to_string(),
        surface_id: "Task".to_string(),
        started_epoch_ms: 2_000_000,
        tool_use_id: None,
    }];
    store.ingest(IngestBatch::new(&s, &spans, &calls)).unwrap();
    store
        .replace_surfaces(&[
            skill_surface("commit", Scope::Global, 10, 5),
            surface("Task", Scope::Global, 20),
        ])
        .unwrap();

    // The span starts at 1_000, the agent call at 2_000: a window ending
    // before the call narrows the usage half only — the catalog (current
    // config) keeps both rows.
    let catalog = store
        .effective_catalog(TimeRange {
            start: None,
            end: Some(1_500),
        })
        .unwrap();
    assert_eq!(catalog.len(), 2, "catalog rows never disappear");
    let skill = catalog
        .iter()
        .find(|e| e.kind == "skill" && e.id == "commit")
        .unwrap();
    assert_eq!(skill.uses, 1, "the span is inside the window");
    let task = catalog
        .iter()
        .find(|e| e.kind == "skill" && e.id == "Task")
        .unwrap();
    assert_eq!(task.uses, 0, "the call is outside the window");
}

#[test]
fn a_range_narrows_failure_counts_and_work_events() {
    let mut store = Store::in_memory().unwrap();
    let mut s1 = session("s1");
    s1.start = Some(SessionStart {
        timestamp_ms: 500_000,
        ctx: 1_000,
    });
    let errors = [(1_000_000, "path-not-found", "no /x", "Read", "/x")];
    let edits = [
        (2_000_000, "file_edit", "main.rs", Some("/repo/main.rs")),
        (3_000_000, "file_edit", "lib.rs", Some("/repo/lib.rs")),
    ];
    store
        .ingest(
            IngestBatch::new(&s1, &[], &[])
                .tool_errors(&errors)
                .work_events(&edits),
        )
        .unwrap();

    let range = TimeRange {
        start: Some(2_500),
        end: None,
    };
    assert!(store.error_counts(range).unwrap().is_empty());
    let edits = store.file_edits(range).unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].path, "/repo/lib.rs");
    // Unbounded, everything comes back.
    assert_eq!(store.error_counts(TimeRange::ALL).unwrap().len(), 1);
    assert_eq!(store.file_edits(TimeRange::ALL).unwrap().len(), 2);
}

#[test]
fn a_range_narrows_error_examples_and_keeps_the_cap_binds_last() {
    // The anonymous-placeholder trick is textual-order-sensitive: the
    // range binds first (the filter sits before the per-group cap), the
    // cap last. A windowed test pins that a refactor moving the filter
    // cannot silently truncate the examples.
    let mut store = Store::in_memory().unwrap();
    let mut s1 = session("s1");
    s1.start = Some(SessionStart {
        timestamp_ms: 500_000,
        ctx: 1_000,
    });
    let errors = [
        (1_000_000, "path-not-found", "no /a", "Read", "/a"),
        (1_100_000, "path-not-found", "no /b", "Read", "/b"),
        (1_200_000, "path-not-found", "no /c", "Read", "/c"),
        (2_000_000, "timeout", "timed out", "Bash", "cmd"),
        (9_000_000, "path-not-found", "no /late", "Read", "/late"),
    ];
    store
        .ingest(IngestBatch::new(&s1, &[], &[]).tool_errors(&errors))
        .unwrap();

    // A window that keeps only the timeout: the past-`--until` example
    // and the pre-`--since` ones drop out.
    let range = TimeRange {
        start: Some(1_500),
        end: Some(2_500),
    };
    assert_eq!(
        store.error_examples(2, range).unwrap(),
        vec![(
            "demo".to_string(),
            "timeout".to_string(),
            "timed out".to_string()
        )]
    );

    // The cap binds last: three in-group examples, per_group 2, keeps the
    // two earliest — the past-`--until` one never sneaks in.
    let range = TimeRange {
        start: Some(900),
        end: Some(1_500),
    };
    let examples = store.error_examples(2, range).unwrap();
    assert_eq!(examples.len(), 2);
    assert_eq!(examples[0].2, "no /a");
    assert_eq!(examples[1].2, "no /b");

    // Unbounded, every example comes back — the cap still applied per
    // group.
    assert_eq!(store.error_examples(2, TimeRange::ALL).unwrap().len(), 3);
    assert_eq!(store.error_examples(10, TimeRange::ALL).unwrap().len(), 5);
}

// ---- PBI 2026-10-30-02: the trend view's two windows ----
//
// The pure joins (`skill_deltas`, `kind_pairs`) and their unit tests moved
// to `core::trend` (PBI 61). What stays here is the one test that is
// storage behavior: which window a boundary event lands in.

#[test]
fn two_window_aggregation_splits_events_between_the_windows() {
    // Adjacent same-length windows: previous [1_000, 2_000], current
    // [2_001, 3_000] — no second is shared, so a boundary event lands in
    // exactly one window.
    let previous = TimeRange {
        start: Some(1_000),
        end: Some(2_000),
    };
    let current = TimeRange {
        start: Some(2_001),
        end: Some(3_000),
    };
    let mut store = Store::in_memory().unwrap();

    // A session stamped into each window, so the session count and the
    // subagent totals split too (the sessions table's own axis). The
    // stamped start is the earliest of the declared start and the
    // session's spans, so each window's spans ride its own session.
    let mut past = session("past");
    past.start = Some(SessionStart {
        timestamp_ms: 1_500 * 1000,
        ctx: 9_000,
    });
    past.sub_tokens = 400;
    past.sub_agent_count = 1;
    let past_spans = [
        span_at("commit", 1_500, 100), // previous only
        span_at("review", 1_800, 50),  // previous only
        span_at("edge", 2_000, 10),    // the previous window's last second
    ];
    store
        .ingest(IngestBatch::new(&past, &past_spans, &[]))
        .unwrap();

    let mut present = session("present");
    present.start = Some(SessionStart {
        timestamp_ms: 2_500 * 1000,
        ctx: 10_000,
    });
    present.sub_tokens = 500;
    present.sub_agent_count = 2;
    let present_spans = [
        span_at("commit", 2_500, 300), // current only
        span_at("edge", 2_001, 10),    // the current window's first second
    ];
    store
        .ingest(
            IngestBatch::new(&present, &present_spans, &[]).work_events(&[(
                2_600_000,
                "file_edit",
                "main.rs",
                Some("/repo/main.rs"),
            )]),
        )
        .unwrap();

    let agg = store
        .trend_aggregates(WindowPair { current, previous })
        .unwrap();

    // Per-kind rollups: the current window has a file_edit the previous
    // one lacks; the skill invocations split 2 vs 3 and never leak across
    // the seam — each boundary event stays in its own window.
    let kinds: Vec<&str> = agg
        .current
        .by_kind
        .iter()
        .map(|k| k.kind.as_str())
        .collect();
    assert_eq!(kinds, ["file_edit", "session_start", "skill_invocation"]);
    let cur_skill = agg
        .current
        .by_kind
        .iter()
        .find(|k| k.kind == "skill_invocation")
        .unwrap();
    assert_eq!(cur_skill.events, 2, "commit@2_500 + edge@2_001 only");
    assert_eq!(cur_skill.out_tokens, 310);
    assert_eq!(cur_skill.duration_sec, 4.0);
    let prev_skill = agg
        .previous
        .by_kind
        .iter()
        .find(|k| k.kind == "skill_invocation")
        .unwrap();
    assert_eq!(
        prev_skill.events, 3,
        "commit@1_500 + review@1_800 + edge@2_000"
    );
    assert_eq!(prev_skill.out_tokens, 160);
    let kinds: Vec<&str> = agg
        .previous
        .by_kind
        .iter()
        .map(|k| k.kind.as_str())
        .collect();
    assert_eq!(kinds, ["session_start", "skill_invocation"]);

    // Per-skill usage per window, and the delta ordering across them.
    assert_eq!(
        agg.current
            .skills
            .iter()
            .map(|s| (s.skill.as_str(), s.invocations))
            .collect::<Vec<_>>(),
        [("commit", 1), ("edge", 1)]
    );
    assert_eq!(
        agg.previous
            .skills
            .iter()
            .map(|s| (s.skill.as_str(), s.invocations))
            .collect::<Vec<_>>(),
        [("commit", 1), ("edge", 1), ("review", 1)]
    );
    let deltas = skill_deltas(&agg.current.skills, &agg.previous.skills);
    assert_eq!(
        deltas
            .iter()
            .map(|d| (d.skill.as_str(), d.delta))
            .collect::<Vec<_>>(),
        [("review", -1), ("commit", 0), ("edge", 0)]
    );

    // Session count and subagent totals follow the sessions axis.
    assert_eq!(agg.current.sessions, 1);
    assert_eq!(agg.previous.sessions, 1);
    assert_eq!(
        (agg.current.sub_tokens, agg.current.subagent_runs),
        (500, 2)
    );
    assert_eq!(
        (agg.previous.sub_tokens, agg.previous.subagent_runs),
        (400, 1)
    );
}

// ---- PBI 2026-10-31-31: the events_by_epoch index ----

#[test]
fn a_new_store_declares_the_epoch_index() {
    // `started_epoch` is the column every windowed reader filters on; the
    // index is the planner's choice that keeps those reads from scanning
    // the whole spine (`docs/specs/storage.md`).
    let store = Store::in_memory().unwrap();
    let sql = declared_sql(&store.conn, "index").unwrap();
    let ddl = sql
        .get("events_by_epoch")
        .expect("events_by_epoch must be declared on a fresh store");
    assert!(ddl.contains("started_epoch"), "wrong column: {ddl}");
}

#[test]
fn an_existing_store_gains_the_epoch_index_on_the_next_open() {
    // A store created before the index was declared must converge on the
    // next ReadWrite open. Reconciliation only ever adds, so
    // SCHEMA_VERSION does not move for it (`docs/specs/storage.md`).
    let store = Store::in_memory().unwrap();
    store
        .conn
        .execute_batch("DROP INDEX IF EXISTS events_by_epoch;")
        .unwrap();
    let version_before: i64 = store
        .conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();

    let store = Store::from_connection(store.conn).unwrap();

    let indexes = declared_sql(&store.conn, "index").unwrap();
    assert!(
        indexes.contains_key("events_by_epoch"),
        "the index must be recreated on open: {indexes:?}"
    );
    let version_after: i64 = store
        .conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        version_after, version_before,
        "adding the index must not bump SCHEMA_VERSION"
    );
}

#[test]
fn the_epoch_index_leaves_windowed_output_byte_identical() {
    // The index is a planner's option, never a result change: the same
    // windowed reader and the raw windowed SQL it rests on must answer
    // byte-identically with and without it (PBI 2026-10-31-31's
    // output-invariance scenario).
    let spans = [
        span_at("commit", 1_000, 100),
        span_at("commit", 2_000, 200),
        span_at("commit", 9_000, 400),
    ];
    let mut indexed = Store::in_memory().unwrap();
    indexed
        .ingest(IngestBatch::new(&session("s1"), &spans, &[]))
        .unwrap();
    let mut plain = Store::in_memory().unwrap();
    plain
        .ingest(IngestBatch::new(&session("s1"), &spans, &[]))
        .unwrap();
    plain
        .conn
        .execute_batch("DROP INDEX IF EXISTS events_by_epoch;")
        .unwrap();
    // The two stores must differ in nothing but the index itself.
    assert!(
        declared_sql(&indexed.conn, "index")
            .unwrap()
            .contains_key("events_by_epoch"),
        "the indexed store lost its index"
    );
    assert!(
        !declared_sql(&plain.conn, "index")
            .unwrap()
            .contains_key("events_by_epoch"),
        "the comparison store kept the index"
    );

    let window = TimeRange {
        start: Some(1_500),
        end: Some(8_000),
    };
    assert_eq!(
        indexed.skill_usage(window).unwrap(),
        plain.skill_usage(window).unwrap(),
        "index presence changed the windowed reader's result"
    );
    let sql = "SELECT kind, COUNT(*), SUM(out_tokens) FROM events \
                   WHERE started_epoch >= 1500 AND started_epoch < 8001 \
                   GROUP BY kind";
    let (cols_i, rows_i) = indexed.query(sql).unwrap();
    let (cols_p, rows_p) = plain.query(sql).unwrap();
    assert_eq!(cols_i, cols_p);
    assert_eq!(
        rows_i, rows_p,
        "index presence changed the windowed query's output"
    );
    assert_eq!(
        rows_i.len(),
        1,
        "the window must catch data for this check to be non-vacuous"
    );
}
