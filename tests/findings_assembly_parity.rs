//! Parity for PBI C7 (findings single assembly): the CLI views (`stuck`,
//! `failures`, `inventory`, `prompts`) must render byte-identical output after
//! their shaping moves to pure `core::findings` assembly.
//!
//! The unit half pins the new pure fns against the legacy view behavior on
//! identical synthetic input (boundary gaps, min-edit cuts, scope routing,
//! unused/catalog-only/orphaned statuses). The placement half pins the
//! pure-join-placement rule: the shaping definitions live in core/adapter only,
//! the views only fetch, call, and render.

use std::path::Path;

use saniwalens::core::findings::{
    self, CatalogRow, FrictionViewRow, InventoryStatus, inventory_rows,
};
use saniwalens::core::scope::ScopeFilter;
use saniwalens::core::thrash::{FileEdit, THRASH_GAP_SECS, THRASH_MIN_EDITS};
use saniwalens::store::FrictionCell;

fn edit(session: &str, project: &str, path: &str, epoch: i64) -> FileEdit {
    FileEdit {
        session_id: session.to_string(),
        project: project.to_string(),
        path: path.to_string(),
        epoch,
    }
}

fn read_src(rel: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(root.join(rel)).expect("repo source is readable")
}

#[test]
fn thrash_consts_have_a_single_definition_shared_by_both_sides() {
    // One definition in core/thrash.rs; core/findings.rs re-exports the name so
    // the `findings::THRASH_*` paths keep resolving without a second definition.
    assert_eq!(THRASH_GAP_SECS, 5 * 60);
    assert_eq!(THRASH_MIN_EDITS, 4);
    assert_eq!(findings::THRASH_GAP_SECS, THRASH_GAP_SECS);
    assert_eq!(findings::THRASH_MIN_EDITS, THRASH_MIN_EDITS);
    let thrash = read_src("src/core/thrash.rs");
    assert!(thrash.contains("pub const THRASH_GAP_SECS"));
    assert!(thrash.contains("pub const THRASH_MIN_EDITS"));
    let assembly = read_src("src/core/findings.rs");
    assert!(
        !assembly.contains("pub const THRASH_GAP_SECS: i64"),
        "no second gap definition in core/findings.rs"
    );
    assert!(
        !assembly.contains("pub const THRASH_MIN_EDITS: u32"),
        "no second min-edits definition in core/findings.rs"
    );
}

#[test]
fn stuck_episodes_keep_the_legacy_gap_and_minimum_boundaries() {
    // A gap of exactly THRASH_GAP_SECS still joins; one second more splits.
    let joined = vec![
        edit("s1", "demo", "/p/a.rs", 0),
        edit("s1", "demo", "/p/a.rs", THRASH_GAP_SECS - 60),
        edit("s1", "demo", "/p/a.rs", 2 * (THRASH_GAP_SECS - 60)),
        edit("s1", "demo", "/p/a.rs", 3 * (THRASH_GAP_SECS - 60)),
    ];
    assert_eq!(findings::stuck_episodes(&joined).len(), 1);

    let split = vec![
        edit("s1", "demo", "/p/a.rs", 0),
        edit("s1", "demo", "/p/a.rs", 10),
        edit("s1", "demo", "/p/a.rs", 20),
        edit("s1", "demo", "/p/a.rs", 20 + THRASH_GAP_SECS + 1),
    ];
    assert!(
        findings::stuck_episodes(&split).is_empty(),
        "a 3+1 run never reaches the minimum on either side of the split"
    );

    // Below the minimum is ignored; exactly at it counts.
    let three = vec![
        edit("s1", "demo", "/p/a.rs", 0),
        edit("s1", "demo", "/p/a.rs", 10),
        edit("s1", "demo", "/p/a.rs", 20),
    ];
    assert!(findings::stuck_episodes(&three).is_empty());
    let mut four = three.clone();
    four.push(edit("s1", "demo", "/p/a.rs", 30));
    let episodes = findings::stuck_episodes(&four);
    assert_eq!(episodes.len(), 1);
    assert_eq!(episodes[0].edits, THRASH_MIN_EDITS);
    assert_eq!(episodes[0].project, "demo");
}

#[test]
fn friction_rows_route_each_scope_like_the_legacy_view() {
    // alpha owns path-not-found outright; edit-precondition ties (global);
    // beta-only typo stays project-owned.
    let cell = |project: &str, category: &str, count: i64| FrictionCell {
        project: project.to_string(),
        category: category.to_string(),
        count,
    };
    let cells = vec![
        cell("alpha", "path-not-found", 6),
        cell("beta", "path-not-found", 1),
        cell("alpha", "edit-precondition", 3),
        cell("beta", "edit-precondition", 3),
        cell("beta", "typo", 2),
    ];
    let all = vec![
        ("path-not-found".to_string(), 7),
        ("edit-precondition".to_string(), 6),
        ("typo".to_string(), 2),
    ];

    let rows = findings::friction_rows(&cells, &all, &ScopeFilter::All);
    assert_eq!(
        rows.iter()
            .map(|r| (r.category.clone(), r.count, r.spread))
            .collect::<Vec<_>>(),
        vec![
            ("path-not-found".to_string(), 7, None),
            ("edit-precondition".to_string(), 6, None),
            ("typo".to_string(), 2, None),
        ]
    );
    assert!(rows.iter().all(|r| r.project.is_none()));

    let rows = findings::friction_rows(&cells, &all, &ScopeFilter::Global);
    assert_eq!(rows.len(), 1, "only the tied category is global");
    assert_eq!(rows[0].category, "edit-precondition");
    assert_eq!(rows[0].count, 6);
    assert_eq!(rows[0].spread, Some(2));

    let rows = findings::friction_rows(&cells, &all, &ScopeFilter::Project(None));
    let by_project: Vec<(Option<String>, String, i64)> = rows
        .iter()
        .map(|r| (r.project.clone(), r.category.clone(), r.count))
        .collect();
    assert_eq!(
        by_project,
        vec![
            (Some("alpha".to_string()), "path-not-found".to_string(), 6),
            (Some("beta".to_string()), "typo".to_string(), 2),
        ]
    );

    let rows = findings::friction_rows(
        &cells,
        &all,
        &ScopeFilter::Project(Some("beta".to_string())),
    );
    let beta: Vec<(String, i64)> = rows.iter().map(|r| (r.category.clone(), r.count)).collect();
    assert_eq!(
        beta,
        vec![
            ("path-not-found".to_string(), 1),
            ("edit-precondition".to_string(), 3),
            ("typo".to_string(), 2),
        ],
        "one-project scope keeps cell order, like the legacy filter"
    );
    assert!(
        rows.iter()
            .all(|r| r.project == Some("beta".to_string()) && r.spread.is_none())
    );
}

fn catalog_row(id: &str, kind: &str, uses: i64) -> CatalogRow {
    CatalogRow {
        kind: kind.to_string(),
        id: id.to_string(),
        scope: "global".to_string(),
        project: String::new(),
        startup_tokens: Some(100),
        static_tokens: Some(1500),
        load_mode: "startup_full".to_string(),
        uses,
    }
}

#[test]
fn inventory_rows_keep_the_legacy_status_and_orphan_shape() {
    let catalog = vec![
        catalog_row("used-skill", "skill", 3),
        catalog_row("idle-skill", "skill", 0),
        {
            let mut row = catalog_row("project-rule", "rule", 0);
            row.kind = "rule".to_string();
            row
        },
    ];
    let usage = vec![("skill".to_string(), "ghost-skill".to_string(), 5)];

    let rows = inventory_rows(&catalog, &usage, &ScopeFilter::All);
    assert_eq!(rows.len(), 4, "three catalog rows plus one orphan");
    assert!(matches!(rows[0].status, InventoryStatus::InUse));
    assert_eq!(rows[0].uses, Some(3));
    assert!(matches!(rows[1].status, InventoryStatus::Unused));
    assert_eq!(rows[1].uses, Some(0));
    assert!(
        matches!(rows[2].status, InventoryStatus::CatalogOnly),
        "rules emit no events, so 0 means nothing"
    );
    assert_eq!(rows[2].uses, None);
    let orphan = &rows[3];
    assert!(matches!(orphan.status, InventoryStatus::Orphaned));
    assert_eq!(orphan.kind, "skill");
    assert_eq!(orphan.id, "ghost-skill");
    assert_eq!(orphan.uses, Some(5));
    assert_eq!(orphan.load_mode, None);

    // A scoped view never carries orphans: they have no scope to filter on.
    let rows = inventory_rows(&catalog, &usage, &ScopeFilter::Global);
    assert_eq!(rows.len(), 3);
    assert!(
        rows.iter()
            .all(|r| !matches!(r.status, InventoryStatus::Orphaned))
    );

    // The unused/window distinction stays a render-time label: the pure row
    // only says Unused either way (the view picks UNUSED vs UNUSED (window)).
    let rows = inventory_rows(&catalog, &usage, &ScopeFilter::All);
    assert!(matches!(rows[1].status, InventoryStatus::Unused));
}

#[test]
fn friction_view_row_shape_stays_render_ready() {
    // The view renders project/category/count/spread without further logic.
    let row = FrictionViewRow {
        project: None,
        category: "edit-precondition".to_string(),
        count: 6,
        spread: Some(2),
    };
    assert_eq!(row.spread, Some(2));
}

#[test]
fn views_hold_no_shaping_definitions_only_calls() {
    // stuck/failures/inventory: the fetch + pure + render split. The shaping
    // (thrash params, friction routing, measurability) lives in core; the views
    // only fetch rows, call the pure fns, and render. (PBI C6: the views moved
    // from the `cli/mod.rs` shell to one file each under `cli/views/`, so the
    // call sites below are read from the landed files, not the shell.)
    let shell = read_src("src/cli/mod.rs");
    let stuck = read_src("src/cli/views/stuck.rs");
    let failures = read_src("src/cli/views/failures.rs");
    let inventory = read_src("src/cli/views/inventory.rs");
    for source in [&shell, &stuck, &failures, &inventory] {
        assert!(
            !source.contains("detect_thrash(&"),
            "stuck must call findings::stuck_episodes, not detect_thrash directly"
        );
        assert!(
            !source.contains("split_friction(&"),
            "failures must call findings::friction_rows, not split_friction directly"
        );
        assert!(
            !source.contains("is_usage_measurable(&"),
            "inventory must use findings::inventory_rows, not is_usage_measurable directly"
        );
        assert!(
            !source.contains("const GAP_SECS: i64"),
            "no local thrash params in the shell"
        );
        assert!(
            !source.contains("const MIN_EDITS: u32"),
            "no local thrash params in the shell"
        );
    }
    assert!(
        stuck.contains("stuck_episodes("),
        "the stuck view reaches the pure stuck assembly"
    );
    assert!(
        failures.contains("friction_rows("),
        "the failures view reaches the pure friction assembly"
    );
    assert!(
        inventory.contains("inventory_rows("),
        "the inventory view reaches the pure inventory assembly"
    );
}

#[test]
fn prompts_view_reads_transcripts_only_through_the_adapter() {
    // skill_candidates: refs come from the store, text through the adapter's
    // prompt_text_at, clustering through core::prompt. No direct FS read, no
    // inline transcript parse in the view.
    let view = read_src("src/cli/views/prompts.rs");
    assert!(
        !view.contains("std::fs"),
        "the prompts view must not touch the filesystem directly"
    );
    assert!(
        !view.contains("prompt_text_in"),
        "parsing stays in the adapter; the view calls prompt_text_at"
    );
    assert!(
        view.contains("prompt_text_at"),
        "the view resolves prompt texts through the adapter"
    );
    assert!(
        view.contains("cluster_candidates"),
        "clustering stays a pure core call"
    );
}
