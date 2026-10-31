//! The evaluation-window plumbing every report view shares: the period flags
//! resolved ahead of the store, and the window's disclosure on stderr, in the
//! JSON payloads, and in a table's framing block.

use anyhow::Result;

use crate::core::bucket::JST_OFFSET_SECS;
use crate::core::period::{self, TimeRange, WindowPair};
use crate::session::Style;

/// Resolve the period flags before the session is built — the same
/// ahead-of-store evaluation `parse_scope`/`parse_format` have, so an
/// unparseable date fails on its own. The clock is read here: this is the
/// thin shell around the pure transform in `core::period`.
pub(crate) fn parse_period(
    since: Option<&str>,
    until: Option<&str>,
    last: Option<&str>,
) -> Result<TimeRange> {
    let now = chrono::Utc::now().timestamp();
    period::resolve(since, until, last, now, JST_OFFSET_SECS).map_err(anyhow::Error::msg)
}

/// Resolve the trend's period flags into the two comparison windows before
/// the session is built — the same ahead-of-store evaluation `parse_period`
/// gives the other views, so an unparseable date, a `--until` without a
/// `--since`, or an inverted window fails as a period error before any store
/// work or freshness line. The clock is read here: the thin shell around the
/// pure transform in `core::period`.
pub(crate) fn parse_pair(
    since: Option<&str>,
    until: Option<&str>,
    last: Option<&str>,
) -> Result<WindowPair> {
    let now = chrono::Utc::now().timestamp();
    period::resolve_pair(since, until, last, now, JST_OFFSET_SECS).map_err(anyhow::Error::msg)
}

/// The evaluation-window disclosure — stderr (every format) and the framing
/// block's last line (table only) render it. `None` without the period flags,
/// which is what keeps every existing output byte-identical.
pub(crate) fn window_line(window: TimeRange) -> Option<String> {
    (!window.is_all()).then(|| {
        format!(
            "report window: {}",
            period::display(window, JST_OFFSET_SECS)
        )
    })
}

/// The JSON `window` object — added only when the flags drew a window, so the
/// JSON contract without the flags is untouched.
pub(crate) fn window_json(window: TimeRange) -> Option<serde_json::Value> {
    (!window.is_all()).then(|| {
        let (since, until) = period::bounds(window, JST_OFFSET_SECS);
        serde_json::json!({ "since": since, "until": until })
    })
}

/// Attach the evaluation window to a JSON payload — only when the flags drew
/// one, so the JSON contract without the flags is untouched.
pub(crate) fn with_window(mut payload: serde_json::Value, window: TimeRange) -> serde_json::Value {
    if let Some(w) = window_json(window) {
        payload["window"] = w;
    }
    payload
}

/// Print the evaluation window beside the freshness line (stderr, every
/// format) — piped stdout and `--format json` stay clean.
pub(crate) fn eprint_window(window: TimeRange) {
    if let Some(line) = window_line(window) {
        eprintln!("{}", Style::stderr().dim(&line));
    }
}

/// A framing block with the evaluation window appended (table only — markdown
/// is the bare table, json the machine contract).
pub(crate) fn framed(text: &str, window: TimeRange) -> String {
    match window_line(window) {
        Some(line) => format!("{text}\n{line}"),
        None => text.to_string(),
    }
}

/// An empty report's wording when the flags drew a window: the action part
/// ("run analyze") would misread — the store may hold history outside the
/// window, and the observation stands within it. The window itself is on
/// stderr beside the freshness line.
pub(crate) fn empty_in_window(message: &str, window: TimeRange) -> String {
    match window.is_all() {
        true => message.to_string(),
        false => {
            let head = message.split(" — ").next().unwrap_or(message);
            format!("{head} in the evaluation window")
        }
    }
}

/// The catalog row's unused label: a windowed read means "unused *within the
/// window*" — the catalog is current config, the usage half is historical.
pub(crate) fn unused_label(window: TimeRange) -> &'static str {
    match window.is_all() {
        true => "UNUSED",
        false => "UNUSED (window)",
    }
}
