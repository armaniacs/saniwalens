//! Contract newtypes (PBI C8): the store <-> findings contract in named types.
//!
//! - `FrictionCell` / `ToolBreakdown` carry named fields: a swapped tuple no
//!   longer typechecks (verified by hand; no trybuild harness exists).
//! - `Floor::{Unknown, Observed}` replaces the `None -> 0` sentinel at the
//!   `FindingsInput` boundary.
//! - `effective_catalog(events_window)` is windowed: the same catalog reads
//!   different `uses` under different windows.
//! - Output parity stays pinned by the 51 goldens under
//!   `tests/golden/{mod_split_views,render_view}` (run unmodified).

use saniwalens::core::findings::{FindingsInput, Floor, collect_findings};
use saniwalens::core::period::TimeRange;
use saniwalens::core::span::{SessionStart, Source, Span};
use saniwalens::core::surface::{LoadMode, Scope, Surface};
use saniwalens::store::{
    FrictionCell, IngestBatch, ProjectSlug, SessionMeta, SessionRoot, Store, ToolBreakdown,
};

fn session(id: &str, project: &str, root: &str, start_ctx: Option<u64>) -> SessionMeta {
    SessionMeta {
        id: id.to_string(),
        project: project.to_string(),
        slug: project.to_string(),
        root: root.to_string(),
        source_path: format!("/tmp/{id}.jsonl"),
        sub_tokens: 0,
        sub_agent_count: 0,
        start: start_ctx.map(|ctx| SessionStart {
            timestamp_ms: 1_700_000_000_000,
            ctx,
        }),
    }
}

fn span(skill: &str, epoch_ms: i64) -> Span {
    Span {
        skill: skill.to_string(),
        source: Source::Slash,
        started_epoch_ms: epoch_ms,
        duration_sec: 1.0,
        out_tokens: 7,
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

fn skill_surface(id: &str) -> Surface {
    Surface {
        kind: "skill".to_string(),
        id: id.to_string(),
        scope: Scope::Global,
        config_path: format!("/cfg/{id}"),
        static_tokens: Some(100),
        startup_tokens: Some(10),
        load_mode: LoadMode::StartupDescription,
        paths: Vec::new(),
        description: None,
    }
}

fn error_store() -> Store {
    let mut store = Store::in_memory().unwrap();
    let meta = session("s1", "alpha", "/repo/alpha", None);
    let errors: Vec<(i64, &str, &str, &str, &str)> = vec![
        (
            1_700_000_000_000,
            "path-not-found",
            "missing /a",
            "Read",
            "t",
        ),
        (
            1_700_000_001_000,
            "path-not-found",
            "missing /b",
            "Read",
            "t",
        ),
        (
            1_700_000_002_000,
            "edit-precondition",
            "no such file",
            "Edit",
            "t",
        ),
    ];
    let refs: Vec<(i64, &str, &str, &str, &str)> = errors
        .iter()
        .map(|(e, c, x, t, g)| (*e, *c, *x, *t, *g))
        .collect();
    store
        .ingest(IngestBatch::new(&meta, &[], &[]).tool_errors(&refs))
        .unwrap();
    store
}

#[test]
fn friction_cells_carry_named_fields_in_store_order() {
    let store = error_store();

    let cells: Vec<FrictionCell> = store.error_counts_by_project(TimeRange::ALL).unwrap();
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0].project, "alpha");
    assert_eq!(cells[0].category, "path-not-found");
    assert_eq!(cells[0].count, 2);
    assert_eq!(cells[1].category, "edit-precondition");
    assert_eq!(cells[1].count, 1);

    let bounded = store
        .error_counts_by_project_briefing(TimeRange::ALL, 1)
        .unwrap();
    assert_eq!(bounded, cells[..1]);
}

#[test]
fn tool_breakdown_carries_named_fields_in_store_order() {
    let store = error_store();

    let rows: Vec<ToolBreakdown> = store.error_tool_breakdown(TimeRange::ALL).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].project, "alpha");
    assert_eq!(rows[0].category, "path-not-found");
    assert_eq!(rows[0].tool, "Read");
    assert_eq!(rows[0].count, 2);

    let bounded = store
        .error_tool_breakdown_briefing(TimeRange::ALL, 1)
        .unwrap();
    assert_eq!(bounded, rows[..1]);
}

#[test]
fn session_roots_briefing_carries_typed_slugs() {
    let mut store = Store::in_memory().unwrap();
    store
        .ingest(IngestBatch::new(
            &session("s1", "alpha", "/repo/alpha", None),
            &[],
            &[],
        ))
        .unwrap();

    let roots: Vec<SessionRoot> = store.session_roots_briefing(5_000).unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].root, "/repo/alpha");
    assert_eq!(roots[0].project, ProjectSlug("alpha".to_string()));
    // The untyped reader keeps its shape for the adapter caller.
    assert_eq!(
        store.session_roots().unwrap(),
        vec![("/repo/alpha".to_string(), "alpha".to_string())]
    );
}

#[test]
fn an_unknown_floor_omits_the_always_on_section() {
    let mut input = FindingsInput::default();
    assert_eq!(input.floor, Floor::Unknown);
    // Even a known config figure is not claimed when no start was observed.
    input.always_on_config_tokens = 999;

    let findings = collect_findings(&input);

    assert_eq!(findings.floor, 0);
    assert_eq!(findings.config_tokens, 0);
}

#[test]
fn an_observed_floor_applies_the_config_figure() {
    let input = FindingsInput {
        floor: Floor::Observed(1000),
        always_on_config_tokens: 700,
        ..Default::default()
    };

    let findings = collect_findings(&input);

    assert_eq!(findings.floor, 1000);
    assert_eq!(findings.config_tokens, 700);
}

#[test]
fn a_missing_baseline_maps_to_an_unknown_floor() {
    let store = error_store();

    let floor: Floor = store.baseline_floor(TimeRange::ALL).unwrap().into();
    assert_eq!(floor, Floor::Unknown);
}

#[test]
fn a_present_baseline_maps_to_an_observed_floor() {
    let mut store = Store::in_memory().unwrap();
    store
        .ingest(IngestBatch::new(
            &session("s1", "alpha", "/repo/alpha", Some(12_000)),
            &[],
            &[],
        ))
        .unwrap();

    let floor: Floor = store.baseline_floor(TimeRange::ALL).unwrap().into();
    assert_eq!(floor, Floor::Observed(12_000));
}

#[test]
fn the_catalog_usage_follows_the_events_window() {
    // One skill invoked once at T: a window holding T reads uses=1, a window
    // before T reads uses=0, while the catalog identity is the same row.
    const T_MS: i64 = 1_700_000_000_000;
    let mut store = Store::in_memory().unwrap();
    store
        .ingest(IngestBatch::new(
            &session("s1", "alpha", "/repo/alpha", None),
            &[span("demo-skill", T_MS)],
            &[],
        ))
        .unwrap();
    store
        .replace_surfaces(&[skill_surface("demo-skill")])
        .unwrap();

    let holding = TimeRange {
        start: Some(T_MS / 1000 - 10),
        end: Some(T_MS / 1000 + 10),
    };
    let missing = TimeRange {
        start: Some(T_MS / 1000 - 3_600),
        end: Some(T_MS / 1000 - 10),
    };
    let in_window = store.effective_catalog(holding).unwrap();
    let out_window = store.effective_catalog(missing).unwrap();

    assert_eq!(in_window.len(), 1);
    assert_eq!(out_window.len(), 1);
    assert_eq!(in_window[0].id, "demo-skill");
    assert_eq!(in_window[0].uses, 1, "the event falls in the window");
    assert_eq!(out_window[0].uses, 0, "the event falls outside the window");
}
