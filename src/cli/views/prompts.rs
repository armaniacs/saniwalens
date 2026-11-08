//! Prompts view: how the user steers the session, and which repeated
//! prompts are worth promoting to a skill file. Landed here under the
//! view-landing rule (`docs/specs/architecture.md`): views live as one file
//! under `src/cli/views/`, never as growth in the `src/cli/mod.rs` shell.

use anyhow::Result;

use crate::adapter::harness::Harness;
use crate::adapter::transcript::prompt_text_at;
use crate::core::period::TimeRange;
use crate::core::prompt::{SkillCandidate, cluster_candidates};
use crate::session::Style;
use crate::store::Store;

use super::super::{Align, Format, TableSpec, ViewCtx, note, render_view};

/// Show the mix of how the user steers the session, and what it suggests. Heavy
/// steering points to room for more autonomy; frequent corrections point to
/// clearer upfront specs. The classes are lexical heuristics (`core::prompt`).
pub(crate) fn prompts(
    store: &Store,
    harness: &Harness,
    window: TimeRange,
    format: Format,
) -> Result<()> {
    let counts = store.prompt_behavior_counts(window)?;
    let total: i64 = counts.iter().map(|(_, n)| n).sum();
    // Skill candidates (PBI 12): repeated prompt shapes are skill material.
    // Texts are re-read from transcripts at report time — never stored — and
    // rotated or missing sources count as pruned, never as errors.
    let (candidates, pruned) = skill_candidates(store, window)?;
    let behaviors: Vec<serde_json::Value> = counts
        .iter()
        .map(|(behavior, n)| {
            serde_json::json!({
                "behavior": behavior,
                "count": n,
                "share_pct": if total > 0 {
                    (*n as f64 * 100.0 / total as f64).round() as i64
                } else {
                    0
                },
            })
        })
        .collect();
    let candidates_json: Vec<serde_json::Value> = candidates
        .iter()
        .map(|c| {
            serde_json::json!({
                "template": c.template,
                "count": c.count,
                "example": c.example,
                "sources": c.sources.iter().take(10).collect::<Vec<_>>(),
            })
        })
        .collect();
    let json = serde_json::json!({
        "total": total,
        "behaviors": behaviors,
        "candidates": candidates_json,
        "pruned_prompts": pruned,
    });
    // The empty wording stays the view's data (session counts, the harness
    // harvest note) while the empty branch itself is the spine's: the
    // bespoke `if total == 0` block is gone, routed through `render_view`.
    let empty_msg = if total == 0 {
        Some(prompts_empty_message(store, window)?)
    } else {
        None
    };
    let preamble_text = format!(
        "How you steer {}, classified per prompt: instruct (substantive asks),\n\
         steer (\"go ahead\" / \"yes\"), correct (\"no, instead…\"), question. Lots of\n\
         steering = room for more autonomy; lots of correcting = unclear upfront specs.",
        harness.display_name()
    );
    let style = Style::stdout();
    let pct = |n: i64| (n as f64 * 100.0 / total as f64).round() as i64;
    let get = |name: &str| {
        counts
            .iter()
            .find(|(b, _)| b == name)
            .map_or(0, |(_, n)| *n)
    };

    let behaviors_table = TableSpec {
        headers: &["behavior", "count", "share"],
        aligns: &[Align::Left, Align::Right, Align::Right],
        rows: counts
            .iter()
            .map(|(behavior, n)| vec![behavior.clone(), n.to_string(), format!("{}%", pct(*n))])
            .collect(),
    };
    let steer = pct(get("steer"));
    let correct = pct(get("correct"));
    let candidates_table = TableSpec {
        headers: &["candidate", "times", "e.g.", "from"],
        aligns: &[Align::Left, Align::Right, Align::Left, Align::Left],
        rows: candidates
            .iter()
            .map(|c| {
                let example = if c.example.chars().count() > 60 {
                    format!("{} …", c.example.chars().take(59).collect::<String>())
                } else {
                    c.example.clone()
                };
                let from = if c.sources.len() == 1 {
                    c.sources[0].clone()
                } else {
                    format!("{} (+{} more)", c.sources[0], c.sources.len() - 1)
                };
                vec![c.template.clone(), c.count.to_string(), example, from]
            })
            .collect(),
    };
    render_view(
        ViewCtx::single(format, window),
        json,
        empty_msg.as_deref(),
        &preamble_text,
        |format| {
            behaviors_table.render(format);
            note(&format!(
                "{total} prompts (behavioral classes are lexical heuristics)"
            ));
            let mut flagged = false;
            if steer >= 25 {
                flagged = true;
                println!(
                    "  - {}",
                    style.warn(&format!(
                        "{steer}% steering (\"go ahead\" / \"yes\" / \"next\"): you approve in small \
                         steps — room for more autonomy (clearer upfront scope, /loop, longer leash)."
                    ))
                );
            }
            if correct >= 10 {
                flagged = true;
                println!(
                    "  - {}",
                    style.warn(&format!(
                        "{correct}% corrections (\"no\" / \"instead\" / \"戻して\"): rework after a \
                         wrong turn — tighter initial specs or rules could cut it."
                    ))
                );
            }
            if !flagged {
                println!(
                    "  - {}",
                    style.good(&format!(
                        "healthy mix: mostly substantive instructions, low correction ({correct}%) — \
                         good alignment, no obvious babysitting or rework problem."
                    ))
                );
            }
            if !candidates_table.rows.is_empty() {
                println!();
                println!("Candidate new skills — prompts worth promoting to a skill file:");
                candidates_table.render(format);
            }
            if pruned > 0 {
                note(&format!(
                    "{pruned} prompt(s) referenced rotated or missing transcripts — excluded from candidates, not silently dropped"
                ));
            }
        },
    )
}

/// The prompts empty message: what an empty prompt half means depends on the
/// store (sessions analyzed but no prompt signal = this harness harvests
/// none, so `analyze` would send the user in a circle) and on the window.
/// Data for the spine's empty slot — the wording is byte-pinned, only the
/// branch moves into [`render_view`].
fn prompts_empty_message(store: &crate::store::Store, window: TimeRange) -> Result<String> {
    // An analyzed store with sessions but no prompts means this harness
    // harvests no prompt signal (opencode v1 does not) — telling the user
    // to run `analyze` again would send them in a circle.
    let (sessions, _) = store.session_stats(window)?;
    Ok(if !window.is_all() {
        if sessions > 0 {
            format!(
                "no prompts in the evaluation window — the store holds {sessions} analyzed session(s) within it"
            )
        } else {
            "no prompts in the evaluation window".to_string()
        }
    } else if sessions > 0 {
        format!(
            "no prompts harvested for this harness — the store holds {sessions} analyzed session(s)"
        )
    } else {
        "no prompts found — run `saniwalens analyze` first".to_string()
    })
}

/// Skill-candidate extraction for the prompts view (PBI 12): group the
/// window's prompt texts by reusable shape, keep repeats, count the rest as
/// pruned. Texts resolve through the adapter one pointer at a time — never
/// stored — and rotated or missing sources count as pruned, never as errors.
/// The refs arrive ordered by transcript then line, so candidates keep the
/// same first-seen order as the grouped read this replaces (PBI C7).
pub(crate) fn skill_candidates(
    store: &Store,
    window: TimeRange,
) -> Result<(Vec<SkillCandidate>, i64)> {
    use std::path::Path;
    let refs = store.prompt_refs(window)?;
    let mut texts: Vec<(String, String)> = Vec::new();
    let mut pruned: i64 = 0;
    for (path, line) in &refs {
        match prompt_text_at(Path::new(path), *line) {
            Some(text) => texts.push((text, format!("{path}:{line}"))),
            None => pruned += 1,
        }
    }
    let candidates = cluster_candidates(
        texts
            .iter()
            .map(|(text, source)| (text.as_str(), source.as_str())),
    );
    Ok((candidates, pruned))
}
