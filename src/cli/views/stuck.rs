//! Stuck view: thrash bursts: files the agent kept re-editing in rapid bursts, the signature of getting stuck. Landed here under the view-landing rule
//! (`docs/specs/architecture.md`): new views live as one file under
//! `src/cli/views/` instead of growing the `src/cli/mod.rs` shell.

use anyhow::Result;

use crate::core::findings;
use crate::core::period::TimeRange;
use crate::core::thrash::{THRASH_GAP_SECS, THRASH_MIN_EDITS};
use crate::session::Session;

use super::super::{Align, Format, TableSpec, ViewCtx, empty_in_window, note, render_view};

/// Thrash bursts: a file edited many times in a short window — where the agent
/// got stuck and kept retrying, as opposed to a hotspot's healthy spread-out
/// edits. An episode is one session's edits to one path, so parallel work on
/// a same-named file is never presented as one agent looping (`core::thrash`).
pub(crate) fn stuck(sess: &Session, window: TimeRange, format: Format) -> Result<()> {
    let store = &sess.store;
    let episodes = findings::stuck_episodes(&store.file_edits(window)?);
    let episodes_json: Vec<serde_json::Value> = episodes
        .iter()
        .map(|e| {
            serde_json::json!({
                "project": e.project,
                "session_id": e.session_id,
                "file": e.file(),
                "path": e.path,
                "edits": e.edits,
                "span_secs": e.span_secs(),
            })
        })
        .collect();
    let empty_msg = episodes.is_empty().then(|| {
        empty_in_window(
            &format!(
                "no thrash episodes (>= {THRASH_MIN_EDITS} rapid re-edits) — run analyze first"
            ),
            window,
        )
    });
    let table = TableSpec {
        headers: &["project", "file", "edits", "within"],
        aligns: &[Align::Left, Align::Left, Align::Right, Align::Right],
        rows: episodes
            .iter()
            .take(20)
            .map(|e| {
                let span = e.span_secs();
                vec![
                    e.project.clone(),
                    e.file().to_string(),
                    e.edits.to_string(),
                    format!("{}m{}s", span / 60, span % 60),
                ]
            })
            .collect(),
    };
    render_view(
        ViewCtx::single(format, window),
        serde_json::json!({ "episodes": episodes_json }),
        empty_msg.as_deref(),
        "Files the agent kept re-editing in rapid bursts — the signature of getting stuck\n\
         and retrying. Worth checking what blocked it there (a rule? missing context?).",
        |format| {
            table.render(format);
            note(&format!(
                "bursts of >= {THRASH_MIN_EDITS} edits to one file within {}m by a single session —\n\
                 likely where the agent got stuck. `--format json` carries the session id and\n\
                 full path to investigate with.",
                THRASH_GAP_SECS / 60
            ));
        },
    )
}
