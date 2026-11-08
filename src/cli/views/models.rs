//! Model breakdown view: which models the tokens went to, split the way the
//! costs actually divide — main-thread usage events vs subagent runs. Landed
//! here under the view-landing rule (`docs/specs/architecture.md`): a new
//! view is one file under `src/cli/views/`, never growth in the shell.

use anyhow::Result;

use crate::core::period::TimeRange;
use crate::core::usage::bundle_models;
use crate::session::Session;

use super::super::{Align, Format, TableSpec, ViewCtx, empty_in_window, render_view};

/// Per-model cost split for one window: main-thread rows (model-attributed
/// usage events) and subagent rows (runs grouped by their own model).
/// `tool_error` rows are excluded upstream — their `model` column carries the
/// originating tool, not a model — and missing models bundle as `unknown`
/// (a dropped row would read as a smaller total).
pub(crate) fn models(sess: &Session, window: TimeRange, format: Format) -> Result<()> {
    let store = &sess.store;
    let main = bundle_models(store.model_usage_events(window)?);
    let subs = bundle_models(store.model_usage_subagents(window)?);

    let json = serde_json::json!({
        "main": main.iter().map(|row| serde_json::json!({
            "model": row.model,
            "events": row.count,
            "out_tokens": row.out_tokens,
        })).collect::<Vec<_>>(),
        "subagents": subs.iter().map(|row| serde_json::json!({
            "model": row.model,
            "runs": row.count,
            "out_tokens": row.out_tokens,
        })).collect::<Vec<_>>(),
    });
    let empty_msg = (main.is_empty() && subs.is_empty()).then(|| {
        empty_in_window(
            "no model usage found — run `saniwalens analyze` first",
            window,
        )
    });
    let total_main: u64 = main.iter().map(|row| row.out_tokens).sum();
    let total_sub: u64 = subs.iter().map(|row| row.out_tokens).sum();
    let mut combined: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    for row in main.iter().chain(subs.iter()) {
        *combined.entry(row.model.as_str()).or_default() += row.out_tokens;
    }
    let (top_model, top_tokens) = combined
        .iter()
        .max_by_key(|(_, tokens)| *tokens)
        .map(|(model, tokens)| (*model, *tokens))
        .unwrap_or(("", 0));
    let summary = format!(
        "tokens: main-thread {total_main}, subagents {total_sub} — by model, costliest first"
    );
    let concentration = concentration_note(total_main + total_sub, top_model, top_tokens);
    let main_table = TableSpec {
        headers: &["model", "events", "out tokens"],
        aligns: &[Align::Left, Align::Right, Align::Right],
        rows: main
            .iter()
            .map(|row| {
                vec![
                    row.model.clone(),
                    row.count.to_string(),
                    row.out_tokens.to_string(),
                ]
            })
            .collect(),
    };
    let subs_table = TableSpec {
        headers: &["model", "runs", "out tokens"],
        aligns: &[Align::Left, Align::Right, Align::Right],
        rows: subs
            .iter()
            .map(|row| {
                vec![
                    row.model.clone(),
                    row.count.to_string(),
                    row.out_tokens.to_string(),
                ]
            })
            .collect(),
    };
    render_view(
        ViewCtx::single(format, window),
        json,
        empty_msg.as_deref(),
        "Where the tokens went, by model: main-thread usage first, then the\n\
         subagent runs grouped by their own model. An `unknown` row is usage\n\
         with no recorded model — bundled, never dropped.",
        |format| {
            println!("{summary}");
            if let Some(note) = concentration {
                println!("{note}");
            }
            println!();
            if !main_table.rows.is_empty() {
                main_table.render(format);
                println!();
            }
            if subs_table.rows.is_empty() {
                return;
            }
            subs_table.render(format);
        },
    )
}

/// Warn when the model mix is concentrated enough to act on: one model holds
/// at least four fifths of the combined output tokens. The note names the
/// model and its share, not the decision — the reader decides whether to
/// shift work.
pub(crate) fn concentration_note(total: u64, top_model: &str, top_tokens: u64) -> Option<String> {
    if total > 0 && top_tokens.saturating_mul(5) >= total.saturating_mul(4) {
        Some(format!(
            "note: {top_model} holds {}% of this window's output tokens",
            top_tokens * 100 / total
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concentration_fires_at_four_fifths() {
        assert_eq!(
            concentration_note(100, "opus", 80),
            Some("note: opus holds 80% of this window's output tokens".to_string())
        );
        assert_eq!(concentration_note(100, "opus", 79), None);
        assert_eq!(concentration_note(0, "opus", 0), None);
    }
}
