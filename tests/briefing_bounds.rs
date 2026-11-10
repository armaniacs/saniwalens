//! Briefing bounds (pbi/2026-11-09-01): parity + golden pins.
//!
//! - Store briefing readers are bounded (`LIMIT`): under the limit the bounded
//!   result is a byte-identical prefix of the unbounded one; over the limit
//!   only the first `limit` rows come back.
//! - `skill_out_tokens` aggregates in SQL (`SUM`), never via a full `Vec`.
//! - The rendered briefing never exceeds `BRIEFING_MAX_BYTES`; an over-long
//!   briefing is cut with a truncation marker, never silently dropped.

use saniwalens::core::findings::{
    BRIEFING_EVENTS_LIMIT, BRIEFING_GROUPS_LIMIT, FindingsInput, collect_findings,
};
use saniwalens::core::optimize::{
    BRIEFING_MAX_BYTES, BRIEFING_TRUNCATION_MARKER, Findings, render_briefing,
};
use saniwalens::core::period::TimeRange;
use saniwalens::core::scope::ScopeFilter;
use saniwalens::core::span::{Source, Span};
use saniwalens::core::surface::{LoadMode, Scope, Surface};
use saniwalens::store::{IngestBatch, SessionMeta, Store};

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

fn span(skill: &str, out_tokens: u64, epoch_ms: i64) -> Span {
    Span {
        skill: skill.to_string(),
        source: Source::Slash,
        started_epoch_ms: epoch_ms,
        duration_sec: 1.0,
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

fn claude_copy() -> saniwalens::core::optimize::HarnessCopy {
    saniwalens::core::optimize::HarnessCopy {
        agent: "Claude Code",
        article: "a",
        source: "session transcripts",
        raw_source: "~/.claude/projects transcripts",
        global_ref: "~/.claude",
        project_ref: ".claude / CLAUDE.md",
        instruction_file: "CLAUDE.md",
        config_inventory: "CLAUDE.md, rules, hooks, skills, and settings",
        language: saniwalens::core::optimize::AdvisorLanguage::default(),
    }
}

#[test]
fn skill_out_tokens_sum_matches_the_vec_total() {
    // Scenario 2: the SQL SUM is the same number the old Vec-collect + sum
    // produced — the aggregation moves to the store, the total does not move.
    let mut store = Store::in_memory().unwrap();
    store
        .ingest(IngestBatch::new(
            &session("s1"),
            &[
                span("a", 100, 1_700_000_000_000),
                span("b", 250, 1_700_000_001_000),
            ],
            &[],
        ))
        .unwrap();
    store
        .ingest(IngestBatch::new(
            &session("s2"),
            &[span("a", 50, 1_700_000_002_000)],
            &[],
        ))
        .unwrap();

    let via_vec: i64 = store
        .skill_usage(TimeRange::ALL)
        .unwrap()
        .iter()
        .map(|r| r.out_tokens)
        .sum();
    let via_sum = store.skill_out_tokens_sum(TimeRange::ALL).unwrap();

    assert_eq!(via_sum, 400);
    assert_eq!(via_sum, via_vec);
}

#[test]
fn skill_out_tokens_sum_is_zero_on_an_empty_store() {
    let store = Store::in_memory().unwrap();

    assert_eq!(store.skill_out_tokens_sum(TimeRange::ALL).unwrap(), 0);
}

#[test]
fn bounded_readers_match_unbounded_under_the_limit() {
    // Byte-identical discipline: when the data fits, the bounded reader
    // returns exactly what the unbounded one does.
    let mut store = Store::in_memory().unwrap();
    store
        .ingest(IngestBatch::new(
            &session("s1"),
            &[span("a", 10, 1_700_000_000_000)],
            &[],
        ))
        .unwrap();
    store
        .ingest(IngestBatch::new(&session("s1"), &[], &[]).work_events(&[(
            1_700_000_000_000,
            "bash_cmd",
            "ls",
            None,
        )]))
        .unwrap();

    assert_eq!(
        store
            .file_edits_briefing(TimeRange::ALL, BRIEFING_EVENTS_LIMIT)
            .unwrap(),
        store.file_edits(TimeRange::ALL).unwrap()
    );
    assert_eq!(
        store
            .work_counts_briefing("bash_cmd", TimeRange::ALL, BRIEFING_GROUPS_LIMIT)
            .unwrap(),
        store.work_counts("bash_cmd", TimeRange::ALL).unwrap()
    );
    assert_eq!(
        store
            .error_tool_breakdown_briefing(TimeRange::ALL, BRIEFING_GROUPS_LIMIT)
            .unwrap(),
        store.error_tool_breakdown(TimeRange::ALL).unwrap()
    );
    assert_eq!(
        store
            .skill_usage_briefing(TimeRange::ALL, BRIEFING_GROUPS_LIMIT)
            .unwrap(),
        store.skill_usage(TimeRange::ALL).unwrap()
    );
    assert_eq!(
        store
            .subagent_by_agent_briefing(TimeRange::ALL, BRIEFING_GROUPS_LIMIT)
            .unwrap(),
        store.subagent_by_agent(TimeRange::ALL).unwrap()
    );
    assert_eq!(
        store
            .skill_event_costs_briefing(TimeRange::ALL, BRIEFING_EVENTS_LIMIT)
            .unwrap(),
        store.skill_event_costs(TimeRange::ALL).unwrap()
    );
}

#[test]
fn bounded_readers_cut_at_the_limit_with_unbounded_order() {
    // Over the limit only the first `limit` rows come back, in the same order
    // the unbounded reader reports — a head cut, never a reshuffle.
    let mut store = Store::in_memory().unwrap();
    let spans: Vec<Span> = (0..10)
        .map(|n| span(&format!("skill-{n:02}"), 10, 1_700_000_000_000 + n))
        .collect();
    store
        .ingest(IngestBatch::new(&session("s1"), &spans, &[]))
        .unwrap();

    let full = store.skill_event_costs(TimeRange::ALL).unwrap();
    assert_eq!(full.len(), 10);
    let bounded = store.skill_event_costs_briefing(TimeRange::ALL, 4).unwrap();
    assert_eq!(bounded, full[..4]);

    let full_counts = store.work_counts("bash_cmd", TimeRange::ALL).unwrap();
    let _ = full_counts;
}

#[test]
fn work_counts_briefing_caps_distinct_groups() {
    let mut store = Store::in_memory().unwrap();
    let events: Vec<(i64, &str, &str, Option<&str>)> = (0..8)
        .map(|n| {
            (
                1_700_000_000_000 + n,
                "bash_cmd",
                Box::leak(format!("cmd-{n:02}").into_boxed_str()) as &str,
                None,
            )
        })
        .collect();
    store
        .ingest(IngestBatch::new(&session("s1"), &[], &[]).work_events(&events))
        .unwrap();

    let full = store.work_counts("bash_cmd", TimeRange::ALL).unwrap();
    assert_eq!(full.len(), 8);
    let bounded = store
        .work_counts_briefing("bash_cmd", TimeRange::ALL, 3)
        .unwrap();
    assert_eq!(bounded.len(), 3);
    assert_eq!(bounded, full[..3]);
}

#[test]
fn large_event_store_stays_bounded() {
    // Scenario 1: multi-year scale. 5_000 edits are stored; the briefing read
    // takes only its cap.
    let mut store = Store::in_memory().unwrap();
    let events: Vec<(i64, &str, &str, Option<&str>)> = (0..5_000)
        .map(|n| {
            (
                1_700_000_000_000 + n,
                "file_edit",
                "main.rs",
                Some(Box::leak(format!("/repo/src/main-{n}.rs").into_boxed_str()) as &str),
            )
        })
        .collect();
    store
        .ingest(IngestBatch::new(&session("s1"), &[], &[]).work_events(&events))
        .unwrap();

    let bounded = store.file_edits_briefing(TimeRange::ALL, 1_000).unwrap();
    assert_eq!(bounded.len(), 1_000);
    let full = store.file_edits(TimeRange::ALL).unwrap();
    assert_eq!(full.len(), 5_000);
    assert_eq!(bounded, full[..1_000]);
}

#[test]
fn collect_findings_still_sums_skill_out_tokens() {
    // The Vec path keeps working for already-fetched rows: the SUM reader is
    // the bounded way to fetch, this is the unchanged assembly of it.
    let input = FindingsInput {
        skill_out_tokens: vec![10, 20, 30],
        ..Default::default()
    };

    assert_eq!(collect_findings(&input).main_out, 60);
}

#[test]
fn small_briefing_is_unchanged_with_no_marker() {
    // Byte-identical discipline: under BRIEFING_MAX_BYTES the text carries no
    // marker and keeps its full content.
    let findings = Findings {
        main_out: 1234,
        ..Default::default()
    };
    let brief = render_briefing(&findings, &ScopeFilter::All, &claude_copy());

    assert!(brief.contains("1234"));
    assert!(!brief.contains(BRIEFING_TRUNCATION_MARKER));
    assert!(brief.len() <= BRIEFING_MAX_BYTES);
}

#[test]
fn overlong_briefing_is_cut_with_a_marker_within_budget() {
    // Scenario 3 (golden): over BRIEFING_MAX_BYTES the briefing ends with the
    // truncation marker and fits the budget — data is never silently dropped.
    let findings = Findings {
        unused: (0..20_000)
            .map(|n| saniwalens::core::optimize::SurfaceRef {
                kind: "skill".to_string(),
                id: format!("unused-skill-{n:05}"),
                startup_tokens: Some(9),
                on_demand_tokens: Some(1345),
            })
            .collect(),
        ..Default::default()
    };
    let brief = render_briefing(&findings, &ScopeFilter::All, &claude_copy());

    assert!(
        brief.len() <= BRIEFING_MAX_BYTES,
        "briefing stays within budget: {} > {BRIEFING_MAX_BYTES}",
        brief.len()
    );
    assert!(
        brief.contains(BRIEFING_TRUNCATION_MARKER),
        "the cut is marked, never silent"
    );
    assert!(
        brief.ends_with(BRIEFING_TRUNCATION_MARKER),
        "the marker is the last thing rendered"
    );
}

#[test]
fn error_breakdown_and_catalog_briefing_reads_are_bounded() {
    let mut store = Store::in_memory().unwrap();
    let errors: Vec<(i64, &str, &str, &str, &str)> = (0..6)
        .map(|n| {
            (
                1_700_000_000_000 + n,
                Box::leak(format!("cat-{n:02}").into_boxed_str()) as &str,
                "excerpt",
                "Bash",
                "target",
            )
        })
        .collect();
    let refs: Vec<(i64, &str, &str, &str, &str)> = errors
        .iter()
        .map(|(e, c, x, t, g)| (*e, *c, *x, *t, *g))
        .collect();
    store
        .ingest(IngestBatch::new(&session("s1"), &[], &[]).tool_errors(&refs))
        .unwrap();
    let surfaces: Vec<Surface> = (0..6)
        .map(|n| Surface {
            kind: "skill".to_string(),
            id: format!("skill-{n:02}"),
            scope: Scope::Global,
            config_path: format!("/cfg/skill-{n:02}"),
            static_tokens: Some(100),
            startup_tokens: Some(10),
            load_mode: LoadMode::StartupDescription,
            paths: Vec::new(),
            description: None,
        })
        .collect();
    store.replace_surfaces(&surfaces).unwrap();

    let full_breakdown = store.error_tool_breakdown(TimeRange::ALL).unwrap();
    let bounded_breakdown = store
        .error_tool_breakdown_briefing(TimeRange::ALL, 2)
        .unwrap();
    assert_eq!(bounded_breakdown, full_breakdown[..2]);

    let full_catalog = store.effective_catalog(TimeRange::ALL).unwrap();
    assert_eq!(full_catalog.len(), 6);
    let bounded_catalog = store.effective_catalog_briefing(TimeRange::ALL, 2).unwrap();
    assert_eq!(bounded_catalog.len(), 2);
    assert_eq!(bounded_catalog, full_catalog[..2]);

    let full_cells = store.error_counts_by_project(TimeRange::ALL).unwrap();
    let bounded_cells = store
        .error_counts_by_project_briefing(TimeRange::ALL, 2)
        .unwrap();
    assert_eq!(bounded_cells, full_cells[..2]);

    let full_behaviors = store.prompt_behavior_counts(TimeRange::ALL).unwrap();
    let bounded_behaviors = store
        .prompt_behavior_counts_briefing(TimeRange::ALL, BRIEFING_GROUPS_LIMIT)
        .unwrap();
    assert_eq!(bounded_behaviors, full_behaviors);

    let full_roots = store.session_roots().unwrap();
    // The briefing twin carries the slug typed: project back to tuples for
    // the prefix comparison (PBI C8: the untyped reader stays for the adapter
    // caller, which takes `&[(String, String)]`).
    let bounded_roots: Vec<(String, String)> = store
        .session_roots_briefing(BRIEFING_GROUPS_LIMIT)
        .unwrap()
        .into_iter()
        .map(|root| (root.root, root.project.0))
        .collect();
    assert_eq!(bounded_roots, full_roots);

    let full_agents = store.subagent_by_agent(TimeRange::ALL).unwrap();
    let bounded_agents = store
        .subagent_by_agent_briefing(TimeRange::ALL, BRIEFING_GROUPS_LIMIT)
        .unwrap();
    assert_eq!(bounded_agents, full_agents);
}
