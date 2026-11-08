//! Failures view: recurring tool failures by category, ranked, each with what it suggests fixing. Landed here under the view-landing rule
//! (`docs/specs/architecture.md`): new views live as one file under
//! `src/cli/views/` instead of growing the `src/cli/mod.rs` shell.

use anyhow::Result;

use crate::adapter::harness::Harness;
use crate::core::findings;
use crate::core::friction::ErrorCategory;
use crate::core::period::TimeRange;
use crate::core::scope::ScopeFilter;
use crate::session::Style;
use crate::store::Store;

use super::super::{Align, Format, TableSpec, ViewCtx, empty_in_window, note, render_view};

/// Where the work stumbles: recurring tool failures by category, ranked, each
/// with what it suggests fixing. This is about the work, not the config —
/// recurring failures are fixable friction that wastes turns and tokens.
/// `--scope` routes by ownership (`core::scope`): `global` shows the failures
/// no project owns, `project` each project's majority-owned failures, and
/// `project:<slug>` everything that happened in that one project.
pub(crate) fn failures(
    store: &Store,
    harness: &Harness,
    filter: &ScopeFilter,
    window: TimeRange,
    format: Format,
) -> Result<()> {
    let cells = store.error_counts_by_project(window)?;
    // The scope-blind totals feed only the `All` scope; other scopes route
    // the per-project cells, so no second query runs for them.
    let all: Vec<(String, i64)> = match filter {
        ScopeFilter::All => store.error_counts(window)?,
        _ => Vec::new(),
    };
    let counts = findings::friction_rows(&cells, &all, filter);
    let total: i64 = counts.iter().map(|row| row.count).sum();

    // Unknown stored labels (typos, schema drift) are still listed — nothing
    // is dropped — but their unknown-ness warns instead of silently joining
    // `Other`'s suggestion. Warn-once shape mirrors the waste view.
    let mut unknown_labels: Vec<String> = counts
        .iter()
        .filter(|row| !ErrorCategory::is_known_label(&row.category))
        .map(|row| row.category.clone())
        .collect();
    unknown_labels.sort();
    unknown_labels.dedup();
    if !unknown_labels.is_empty() {
        eprintln!(
            "{}",
            Style::stderr().warn(&format!(
                "warning: {} stored categor{} unknown ({}); shown with the unclassified suggestion",
                unknown_labels.len(),
                if unknown_labels.len() == 1 {
                    "y is"
                } else {
                    "ies are"
                },
                unknown_labels.join(", ")
            ))
        );
    }

    let failures_json: Vec<serde_json::Value> = counts
        .iter()
        .map(|row| {
            serde_json::json!({
                "project": row.project,
                "category": row.category,
                "count": row.count,
                "spread_projects": row.spread,
                "suggestion": ErrorCategory::from_label(&row.category)
                    .suggestion(harness.instruction_file()),
            })
        })
        .collect();
    let json = serde_json::json!({
        "total": total,
        "failures": failures_json,
        "unknown_labels": {
            "count": unknown_labels.len(),
            "labels": unknown_labels,
        },
    });
    let empty_msg = (total == 0).then(|| {
        empty_in_window(
            "no tool failures found for this scope — run `saniwalens analyze` first?",
            window,
        )
    });
    let preamble_text = match filter {
        ScopeFilter::All => {
            "Tool calls that kept failing the same way, grouped by kind of failure.\n\
             Every row wastes turns and tokens; the suggestion says what usually fixes it."
                .to_string()
        }
        ScopeFilter::Global => {
            format!(
                "Failures spread across several projects — no single project owns them, so\n\
                 they read as habits or global-config problems. Fix these in {}.",
                harness.global_config_ref()
            )
        }
        ScopeFilter::Project(None) => {
            format!(
                "Failures each project owns (it holds the majority of that kind). Fix each\n\
                 in that project's own {}, not globally.",
                harness.project_config_ref()
            )
        }
        ScopeFilter::Project(Some(_)) => {
            format!(
                "Every recurring failure inside this one project, most frequent first.\n\
                 Fix them in this project's own {}.",
                harness.project_config_ref()
            )
        }
    };
    let with_project = matches!(filter, ScopeFilter::Project(None));
    let table = TableSpec {
        headers: if with_project {
            &["project", "error", "count", "suggestion"]
        } else {
            &["error", "count", "suggestion"]
        },
        aligns: if with_project {
            &[Align::Left, Align::Left, Align::Right, Align::Left]
        } else {
            &[Align::Left, Align::Right, Align::Left]
        },
        rows: counts
            .iter()
            .map(|row| {
                let mut cells = Vec::new();
                if with_project {
                    cells.push(row.project.clone().unwrap_or_default());
                }
                let error = match row.spread {
                    Some(spread) => format!("{} (across {spread} projects)", row.category),
                    None => row.category.clone(),
                };
                cells.extend([
                    error,
                    row.count.to_string(),
                    ErrorCategory::from_label(&row.category).suggestion(harness.instruction_file()),
                ]);
                cells
            })
            .collect(),
    };
    render_view(
        ViewCtx::single(format, window),
        json,
        empty_msg.as_deref(),
        &preamble_text,
        |format| {
            table.render(format);
            note(&format!(
                "{total} tool failures (denials from the transcript's own marker where present; \
                 everything else is a lexical heuristic)"
            ));
        },
    )
}
