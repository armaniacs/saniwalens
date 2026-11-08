//! Overhead view: what every session pays in context before any work starts, and how much is readable config. Landed here under the view-landing rule
//! (`docs/specs/architecture.md`): new views live as one file under
//! `src/cli/views/` instead of growing the `src/cli/mod.rs` shell.

use anyhow::Result;

use crate::adapter::harness::Harness;
use crate::core::period::TimeRange;
use crate::session::Style;
use crate::store::Store;

use super::super::{Align, Format, TableSpec, ViewCtx, empty_in_window, note, render_view};

/// Reconcile the empirical always-on floor against readable config. The floor is
/// what every session actually starts with; subtracting the config we can read
/// leaves the residual — system prompt, built-in tools, and MCP tool schemas the
/// catalog cannot weigh. The per-project floors let you infer an MCP server's
/// real cost by comparing a project that enables it against one that does not.
pub(crate) fn overhead(
    store: &Store,
    harness: &Harness,
    window: TimeRange,
    format: Format,
) -> Result<()> {
    let floor = store.baseline_floor(window)?;
    let config = store.always_on_config_tokens()?;
    let residual = floor.map(|floor| (floor - config).max(0));
    let per_project = store.baseline_floor_per_project(window)?;

    let per_project_json: Vec<serde_json::Value> = per_project
        .iter()
        .map(|(project, floor, sessions)| {
            serde_json::json!({
                "project": project,
                "floor": floor,
                "sessions": sessions,
            })
        })
        .collect();
    let json = serde_json::json!({
        "floor": floor,
        "config_tokens": config,
        "residual": residual,
        "per_project": per_project_json,
    });
    // No session start observed — say so rather than substituting a mid-session
    // context that would read as a startup cost. The session count is only
    // read on the empty path, as before.
    let empty_msg: Option<String> = match floor {
        Some(_) => None,
        None => {
            let (sessions, _) = store.session_stats(window)?;
            Some(if sessions == 0 {
                empty_in_window("no data — run `saniwalens analyze` first", window)
            } else {
                format!(
                    "always-on floor unavailable — none of the {sessions} analyzed session(s)\n\
                     carries an observable session start. A store written before session-start\n\
                     tracking keeps its old rows (analyze never re-reads a closed session), so\n\
                     the floor appears as newer sessions are analyzed. Discarding the store\n\
                     would backfill it from the {} still on disk, at the cost of every\n\
                     session whose {} {} has already pruned.",
                    harness.raw_source(),
                    harness.source_noun(),
                    harness.display_name()
                )
            })
        }
    };
    let style = Style::stdout();
    let floor_text = floor.map(|floor| {
        let residual = (floor - config).max(0);
        (
            floor,
            residual,
            format!(
                "Observed always-on floor (leanest session-start context): {}",
                style.bold(&format!("{floor} tokens"))
            ),
        )
    });
    let table = TableSpec {
        headers: &["project", "floor_tokens", "sessions"],
        aligns: &[Align::Left, Align::Right, Align::Right],
        rows: per_project
            .into_iter()
            .map(|(project, floor, sessions)| {
                vec![project, floor.to_string(), sessions.to_string()]
            })
            .collect(),
    };
    render_view(
        ViewCtx::single(format, window),
        json,
        empty_msg.as_deref(),
        "What every session pays in context before any work starts, and how much of\n\
         it your own config files account for.",
        |format| {
            let Some((_, residual, floor_line)) = floor_text else {
                return;
            };
            println!("{floor_line}");
            println!(
                "  readable config ({}): {config} tokens",
                harness.readable_config()
            );
            println!(
                "  residual (system prompt + built-in tools + MCP schemas):                {residual} tokens"
            );
            println!(
                "{}",
                style.dim(
                    "  -> the residual is what you cannot read from files; compare projects below\n\
                     \x20    to see an MCP server's marginal cost (a project that enables it starts higher)."
                )
            );
            println!();
            table.render(format);
            note(
                "each floor is the leanest start across that project's sessions — a project with\n\
                 few sessions may not have caught a fresh one, so compare like counts.",
            );
        },
    )
}
