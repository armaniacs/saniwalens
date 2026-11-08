//! Waste view: the flagged config-cleanup opportunities (unused, costly+rare,
//! always-on heavy) with their evidence and owning scope. Landed here under the
//! view-landing rule (`docs/specs/architecture.md`): new views live as one file
//! under `src/cli/views/` instead of growing the `src/cli/mod.rs` shell.

use anyhow::Result;

use crate::core::period::TimeRange;
use crate::core::scope::ScopeFilter;
use crate::core::surface::{
    DuplicateCandidate, HEAVY_TOKENS, LoadMode, RuleGlobs, StartupSavings, Wedge, classify_wedge,
    is_usage_measurable, never_fires, on_demand_tokens, redundant_pairs, startup_savings,
};
use crate::session::{Session, Style};

use super::super::{
    Align, Format, emit_json, empty_in_window, eprint_window, framed, in_scope, preamble, render,
    savings_cell, savings_suggestion, scope_cell, with_window,
};

pub(crate) fn waste(
    sess: &Session,
    filter: &ScopeFilter,
    window: TimeRange,
    format: Format,
) -> Result<()> {
    let store = &sess.store;
    eprint_window(window);
    let catalog: Vec<_> = store
        .effective_catalog(window)?
        .into_iter()
        .filter(|e| in_scope(filter, &e.scope, &e.project))
        .collect();

    struct Row<'a> {
        wedge: Wedge,
        kind: &'a str,
        id: &'a str,
        scope: &'a str,
        project: &'a str,
        uses: i64,
        savings: StartupSavings,
        /// What every session pays for having it installed.
        startup_tokens: Option<u64>,
        /// What its body costs when loaded — an invocation cost, never a saving.
        on_demand_tokens: Option<u64>,
        /// Never-fires evidence: the unmatched globs (and body cost) replace
        /// the generic suggestion. `None` for every other wedge.
        evidence: Option<String>,
    }

    let mut found: Vec<Row> = Vec::new();
    // PBI 07: no silent coercion — an unparsable label is warned + counted,
    // never a known mode. Unknown rows are excluded from wedge routing:
    // filing them under any default mode would misstate what removal saves.
    let mut unknown_load_modes: Vec<String> = Vec::new();
    for entry in &catalog {
        let measurable = is_usage_measurable(&entry.kind);
        let load_mode = match LoadMode::from_label(&entry.load_mode) {
            Some(load_mode) => load_mode,
            None => {
                unknown_load_modes.push(entry.load_mode.clone());
                continue;
            }
        };
        let static_tokens = entry.static_tokens.map(|tokens| tokens as u64);
        let startup_tokens = entry.startup_tokens.map(|tokens| tokens as u64);
        if let Some(wedge) = classify_wedge(
            measurable,
            load_mode,
            static_tokens,
            entry.uses,
            HEAVY_TOKENS,
        ) {
            found.push(Row {
                wedge,
                kind: &entry.kind,
                id: &entry.id,
                scope: &entry.scope,
                project: &entry.project,
                uses: entry.uses,
                savings: startup_savings(load_mode, startup_tokens),
                startup_tokens,
                on_demand_tokens: on_demand_tokens(load_mode, static_tokens),
                evidence: None,
            });
        }
    }

    // Never-fires (PBI 04): path-conditional rules whose globs matched
    // nothing read in-window. Rules without declared globs are skipped (the
    // core matcher), and a window with no read targets at all yields no
    // flags — no evidence, never an accusation.
    let read_targets = store.file_read_targets(window)?;
    let rules: Vec<RuleGlobs> = catalog
        .iter()
        .filter(|e| e.kind == "rule")
        .filter_map(|e| match LoadMode::from_label(&e.load_mode) {
            Some(LoadMode::PathConditional) if !e.paths.is_empty() => Some(RuleGlobs {
                id: e.id.clone(),
                scope: e.scope.clone(),
                project: e.project.clone(),
                globs: e.paths.clone(),
                static_tokens: e.static_tokens.map(|tokens| tokens as u64),
            }),
            _ => None,
        })
        .collect();
    let hits = never_fires(&rules, &read_targets);
    let hit_ids: std::collections::HashSet<&str> = hits.iter().map(|hit| hit.id.as_str()).collect();
    for rule in &rules {
        if !hit_ids.contains(rule.id.as_str()) {
            continue;
        }
        let body = match rule.static_tokens {
            Some(n) => format!("{n}-token body"),
            None => "body".to_string(),
        };
        found.push(Row {
            wedge: Wedge::NeverFires,
            kind: "rule",
            id: &rule.id,
            scope: &rule.scope,
            project: &rule.project,
            uses: 0,
            savings: StartupSavings::Declutter,
            startup_tokens: None,
            on_demand_tokens: rule.static_tokens,
            evidence: Some(format!(
                "never fired in window [{}] — narrow its paths or delete its {body}",
                rule.globs.join(", ")
            )),
        });
    }

    // Redundant (PBI 15): skills/agents/commands whose descriptions overlap
    // enough to be the same purpose. Each paired surface gets one row naming
    // its first counterpart — mutual, deterministic, no pair listed twice.
    // Descriptions come from the catalog (`description` column); surfaces
    // without one can never match, so MCP schemas and rules stay out.
    let dupes: Vec<DuplicateCandidate> = catalog
        .iter()
        .filter(|e| matches!(e.kind.as_str(), "skill" | "agent" | "command"))
        .filter_map(|e| {
            e.description.as_deref().map(|d| DuplicateCandidate {
                id: e.id.as_str(),
                kind: e.kind.as_str(),
                scope: e.scope.as_str(),
                project: e.project.as_str(),
                description: d,
            })
        })
        .collect();
    let mut paired: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (i, j) in redundant_pairs(&dupes) {
        paired.entry(i).or_insert(j);
        paired.entry(j).or_insert(i);
    }
    let mut paired_ids: Vec<usize> = paired.keys().copied().collect();
    paired_ids.sort();
    for i in paired_ids {
        let other = &dupes[paired[&i]];
        let me = &dupes[i];
        let entry = &catalog
            .iter()
            .find(|e| e.kind == me.kind && e.id == me.id && e.scope == me.scope)
            .expect("duplicate candidate comes from the catalog");
        let load_mode = LoadMode::from_label(&entry.load_mode).unwrap_or(LoadMode::OnDemand);
        found.push(Row {
            wedge: Wedge::Redundant,
            kind: &entry.kind,
            id: &entry.id,
            scope: &entry.scope,
            project: &entry.project,
            uses: entry.uses,
            savings: startup_savings(load_mode, entry.startup_tokens.map(|t| t as u64)),
            startup_tokens: entry.startup_tokens.map(|t| t as u64),
            on_demand_tokens: on_demand_tokens(load_mode, entry.static_tokens.map(|t| t as u64)),
            evidence: Some(format!(
                "redundant with {}/{} ({}) — merge it with its double or delete one",
                other.kind, other.id, other.scope
            )),
        });
    }

    found.sort_by_key(|row| savings_rank(row.savings));

    // Warn + count: the unknown rows above never reach a wedge, so the count
    // is the visibility — stderr keeps it out of piped output (`usage` view
    // warns the same way; see `missing_split_warning`).
    if !unknown_load_modes.is_empty() {
        let mut labels = unknown_load_modes.clone();
        labels.sort();
        labels.dedup();
        eprintln!(
            "{}",
            Style::stderr().warn(&format!(
                "warning: {} catalog row(s) with unknown load_mode ({}) excluded from wedge routing",
                unknown_load_modes.len(),
                labels.join(", ")
            ))
        );
    }

    if format == Format::Json {
        let wedges: Vec<serde_json::Value> = found
            .iter()
            .map(|row| {
                let (savings, savings_tokens) = match row.savings {
                    StartupSavings::Tokens(n) => ("tokens", Some(n)),
                    StartupSavings::UnknownSchema => ("unknown_schema", None),
                    StartupSavings::Declutter => ("declutter", None),
                };
                serde_json::json!({
                    "wedge": row.wedge.label(),
                    "kind": row.kind,
                    "id": row.id,
                    "scope": row.scope,
                    "project": if row.project.is_empty() { None } else { Some(row.project) },
                    "savings": savings,
                    "savings_tokens": savings_tokens,
                    "startup_tokens": row.startup_tokens,
                    "on_demand_tokens": row.on_demand_tokens,
                    "uses": if is_usage_measurable(row.kind) { Some(row.uses) } else { None },
                    "suggestion": row.evidence.clone().unwrap_or_else(|| savings_suggestion(row.wedge, row.savings, row.on_demand_tokens)),
                })
            })
            .collect();
        return emit_json(&with_window(
            serde_json::json!({
                "wedges": wedges,
                "unknown_load_modes": {
                    "count": unknown_load_modes.len(),
                    "labels": unknown_load_modes,
                },
            }),
            window,
        ));
    }
    if found.is_empty() {
        println!(
            "{}",
            empty_in_window(
                "nothing wasteful found — run `saniwalens analyze` first",
                window
            )
        );
        return Ok(());
    }

    preamble(
        format,
        &framed(
            "Concrete cleanup opportunities in your config, ranked by what removing each\n\
             one actually saves at session start. A \"~\" figure is declutter-only — just\n\
             the description is loaded at startup; the body costs nothing until invoked.",
            window,
        ),
    );
    let rows: Vec<Vec<String>> = found
        .iter()
        .map(|row| {
            let uses_cell = if is_usage_measurable(row.kind) {
                row.uses.to_string()
            } else {
                "-".to_string()
            };
            vec![
                row.wedge.label().to_string(),
                format!("{}/{}", row.kind, row.id),
                scope_cell(row.scope, row.project),
                savings_cell(row.savings, row.startup_tokens),
                uses_cell,
                row.evidence.clone().unwrap_or_else(|| {
                    savings_suggestion(row.wedge, row.savings, row.on_demand_tokens)
                }),
            ]
        })
        .collect();
    render(
        &[
            "problem",
            "surface",
            "scope",
            "saves@start",
            "uses",
            "suggestion",
        ],
        &[
            Align::Left,
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Left,
        ],
        &rows,
        format,
    );
    Ok(())
}

/// View-only (waste view): sort key — real measured savings first (largest
/// first), then unmeasured MCP schemas, then declutter-only.
fn savings_rank(savings: StartupSavings) -> (u8, std::cmp::Reverse<u64>) {
    match savings {
        StartupSavings::Tokens(n) => (0, std::cmp::Reverse(n)),
        StartupSavings::UnknownSchema => (1, std::cmp::Reverse(0)),
        StartupSavings::Declutter => (2, std::cmp::Reverse(0)),
    }
}
