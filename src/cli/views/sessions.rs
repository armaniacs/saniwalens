//! Sessions view: one row per session — what it spent, how long it ran,
//! and how often it stumbled. Landed here under the view-landing rule
//! (`docs/specs/architecture.md`): views live as one file under
//! `src/cli/views/`, never as growth in the `src/cli/mod.rs` shell.

use anyhow::Result;

use crate::adapter::harness::Harness;
use crate::core::period::TimeRange;
use crate::store::{SessionRow, Store};

use super::super::{Align, Format, TableSpec, ViewCtx, empty_in_window, render_view};

/// Sort key for the sessions table. Tokens first: cost is what the view is
/// for; duration and friction are the alternate cuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionSort {
    Tokens,
    Duration,
    Friction,
}

/// Parse `--sort`. Unknown values fail with the accepted list, like `--by`.
pub(crate) fn parse_sort(value: Option<&str>) -> Result<SessionSort> {
    match value.unwrap_or("tokens") {
        "tokens" => Ok(SessionSort::Tokens),
        "duration" => Ok(SessionSort::Duration),
        "friction" => Ok(SessionSort::Friction),
        other => anyhow::bail!("unknown --sort '{other}' (tokens|duration|friction)"),
    }
}

/// Order rows by the chosen cut, largest first. Ties fall back to the
/// session id so the order never churns between runs.
pub(crate) fn sort_sessions(rows: &mut [SessionRow], by: SessionSort) {
    rows.sort_by(|a, b| {
        let key = |r: &SessionRow| match by {
            SessionSort::Tokens => r.out_tokens,
            SessionSort::Duration => r.duration_sec,
            SessionSort::Friction => r.friction,
        };
        key(b).cmp(&key(a)).then_with(|| a.id.cmp(&b.id))
    });
}

/// The `project/slug` cell: the slug when it names the project, the project
/// otherwise (slugs fold worktrees; the project is what the reader owns).
fn session_cell(row: &SessionRow) -> String {
    if row.slug.is_empty() || row.slug == row.project {
        row.project.clone()
    } else {
        format!("{}/{}", row.project, row.slug)
    }
}

pub(crate) fn sessions(
    store: &Store,
    _harness: &Harness,
    sort: SessionSort,
    window: TimeRange,
    format: Format,
) -> Result<()> {
    let mut rows = store.session_breakdown(window)?;
    sort_sessions(&mut rows, sort);
    let sessions: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "session": row.id,
                "project": row.project,
                "slug": row.slug,
                "started_at": row.started_at,
                "out_tokens": row.out_tokens,
                "duration_sec": row.duration_sec,
                "friction": row.friction,
            })
        })
        .collect();
    let empty_msg = rows.is_empty().then(|| {
        empty_in_window(
            "no sessions in the evaluation window — run `saniwalens analyze` first",
            window,
        )
    });
    let table = TableSpec {
        headers: &[
            "session", "project", "started", "tokens", "secs", "friction",
        ],
        aligns: &[
            Align::Left,
            Align::Left,
            Align::Left,
            Align::Right,
            Align::Right,
            Align::Right,
        ],
        rows: rows
            .iter()
            .map(|row| {
                vec![
                    row.id.clone(),
                    session_cell(row),
                    row.started_at.clone(),
                    row.out_tokens.to_string(),
                    row.duration_sec.to_string(),
                    row.friction.to_string(),
                ]
            })
            .collect(),
    };
    render_view(
        ViewCtx::single(format, window),
        serde_json::json!({ "sessions": sessions }),
        empty_msg.as_deref(),
        "Every session: what it spent, how long it ran, and how often tool\n\
         calls kept failing inside it. Quiet sessions read 0 — they ran, they\n\
         just did not stumble — rather than vanishing from the list.",
        |format| table.render(format),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, tokens: i64, duration: i64, friction: i64) -> SessionRow {
        SessionRow {
            id: id.to_string(),
            project: "demo".to_string(),
            slug: "demo".to_string(),
            started_at: "2023-11-14T22:13:21+00:00".to_string(),
            out_tokens: tokens,
            duration_sec: duration,
            friction,
        }
    }

    #[test]
    fn sessions_sort_by_each_cut_largest_first() {
        let mut rows = vec![row("b", 10, 30, 0), row("a", 20, 5, 9), row("c", 20, 99, 1)];
        sort_sessions(&mut rows, SessionSort::Tokens);
        assert_eq!(rows[0].id, "a");
        assert_eq!(rows[1].id, "c");
        sort_sessions(&mut rows, SessionSort::Duration);
        assert_eq!(rows[0].id, "c");
        sort_sessions(&mut rows, SessionSort::Friction);
        assert_eq!(rows[0].id, "a");
    }

    #[test]
    fn ties_fall_back_to_the_session_id() {
        let mut rows = vec![row("b", 10, 10, 0), row("a", 10, 10, 0)];
        sort_sessions(&mut rows, SessionSort::Tokens);
        assert_eq!(rows[0].id, "a");
    }

    #[test]
    fn unknown_sort_values_fail_with_the_list() {
        assert!(parse_sort(None).is_ok());
        assert!(parse_sort(Some("tokens")).is_ok());
        assert!(parse_sort(Some("bogus")).is_err());
    }
}
