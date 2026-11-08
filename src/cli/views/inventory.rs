//! Inventory view: every installed config surface, what it weighs, and whether it was ever used. Landed here under the view-landing rule
//! (`docs/specs/architecture.md`): new views live as one file under
//! `src/cli/views/` instead of growing the `src/cli/mod.rs` shell.

use anyhow::Result;

use crate::adapter::harness::Harness;
use crate::core::findings;
use crate::core::period::TimeRange;
use crate::core::scope::ScopeFilter;
use crate::store::Store;

use super::super::{
    Align, Format, TableSpec, ViewCtx, in_scope, render_view, scope_cell, unused_label,
    unused_status,
};

/// Join the catalogued surfaces against usage: each installed surface with its
/// static cost and (for usage-measurable kinds) invocation count, unused ones
/// flagged. Usage with no matching surface is shown as orphaned.
pub(crate) fn inventory(
    store: &Store,
    harness: &Harness,
    filter: &ScopeFilter,
    window: TimeRange,
    format: Format,
) -> Result<()> {
    // Plain rows for the pure join: the store-row mapping is the fetch's half,
    // the join and the measurability routing live in `core::findings`.
    let catalog: Vec<findings::CatalogRow> = store
        .effective_catalog(window)?
        .into_iter()
        .filter(|e| in_scope(filter, &e.scope, &e.project))
        .map(|e| findings::CatalogRow {
            kind: e.kind,
            id: e.id,
            scope: e.scope,
            project: e.project,
            startup_tokens: e.startup_tokens,
            static_tokens: e.static_tokens,
            load_mode: e.load_mode,
            uses: e.uses,
        })
        .collect();
    let usage = store.usage_counts(window)?;
    let rows = findings::inventory_rows(&catalog, &usage, filter);

    let items: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            use findings::InventoryStatus as Status;
            match row.status {
                Status::Orphaned => serde_json::json!({
                    "kind": row.kind, "id": row.id,
                    "scope": null, "project": null,
                    "static_tokens": null, "startup_tokens": null,
                    "uses": row.uses, "load_mode": null,
                    "status": "orphaned",
                }),
                _ => serde_json::json!({
                    "kind": row.kind,
                    "id": row.id,
                    "scope": row.scope,
                    "project": if row.project.is_empty() { None } else { Some(&row.project) },
                    "static_tokens": row.static_tokens,
                    "startup_tokens": row.startup_tokens,
                    "uses": row.uses,
                    "load_mode": row.load_mode,
                    "status": match row.status {
                        Status::Unused => unused_status(window),
                        Status::CatalogOnly => "catalog-only",
                        _ => "",
                    },
                }),
            }
        })
        .collect();
    let empty_msg = (catalog.is_empty() && usage.is_empty())
        .then_some("nothing catalogued — run `saniwalens analyze` first".to_string());

    let preamble_text = harness.inventory_preamble().to_string();
    let table = TableSpec {
        headers: &[
            "kind",
            "id",
            "scope",
            "start tok",
            "static tok",
            "uses",
            "load mode",
            "status",
        ],
        aligns: &[
            Align::Left,
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
            Align::Left,
            Align::Left,
        ],
        rows: {
            let mut table: Vec<Vec<String>> = Vec::new();
            for row in &rows {
                use findings::InventoryStatus as Status;
                if matches!(row.status, Status::Orphaned) {
                    table.push(vec![
                        row.kind.clone(),
                        row.id.clone(),
                        "-".to_string(),
                        "-".to_string(),
                        "-".to_string(),
                        row.uses.map_or_else(|| "?".to_string(), |n| n.to_string()),
                        "-".to_string(),
                        "ORPHANED".to_string(),
                    ]);
                    continue;
                }
                // "unused" is only meaningful for usage-measurable kinds; catalog-only
                // kinds (rules, hooks, CLAUDE.md) emit no events, so 0 means nothing.
                // The pure row carries that distinction as uses/status already.
                let (uses_cell, status) = match row.status {
                    Status::Unused => (
                        row.uses.map_or_else(|| "?".to_string(), |n| n.to_string()),
                        unused_label(window),
                    ),
                    Status::InUse => (
                        row.uses.map_or_else(|| "?".to_string(), |n| n.to_string()),
                        "",
                    ),
                    Status::CatalogOnly => ("-".to_string(), "(catalog-only)"),
                    Status::Orphaned => unreachable!("handled above"),
                };
                let tokens =
                    |value: Option<i64>| value.map_or_else(|| "?".to_string(), |t| t.to_string());
                table.push(vec![
                    row.kind.clone(),
                    row.id.clone(),
                    scope_cell(&row.scope, &row.project),
                    tokens(row.startup_tokens),
                    tokens(row.static_tokens),
                    uses_cell,
                    row.load_mode.clone().unwrap_or_default(),
                    status.to_string(),
                ]);
            }
            table
        },
    };
    render_view(
        ViewCtx::single(format, window),
        serde_json::json!({ "surfaces": items }),
        empty_msg.as_deref(),
        &preamble_text,
        |format| table.render(format),
    )
}
