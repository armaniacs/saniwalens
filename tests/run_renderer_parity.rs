//! Parity pins for PBI 2026-11-09-05 (RunLineRenderer generalization).
//!
//! These goldens capture the CURRENT headless rendering behavior byte for
//! byte, through the public harness seam. The refactor must keep every
//! expectation identical; only the event type path changes (from
//! `opencode::RunEvent` to the harness-local `RunLine`). Fixtures are
//! synthetic `--format json` shapes, never harvested lines.

use saniwalens::adapter::harness::{Harness, RunLine};

/// (raw line, expected rendering) pairs covering every renderer branch: text,
/// nested text, envelope tool use, unrecognized envelope, bare tool, untyped
/// JSON, non-JSON, and blank text.
const GOLDENS: &[(&str, &str)] = &[
    (
        r#"{"type":"text","text":"Plan: batch wiki edits"}"#,
        "Text:Plan: batch wiki edits",
    ),
    (
        r#"{"part":{"type":"text","text":"narration"}}"#,
        "Text:narration",
    ),
    (
        r#"{"type":"tool_use","timestamp":1792904532992,"sessionID":"ses_demo","part":{"partID":"prt_demo","type":"tool","tool":"read","state":{"status":"completed"}}}"#,
        "Progress:read",
    ),
    (
        r#"{"type":"text","timestamp":1792904882135,"sessionID":"ses_demo","part":{"id":"prt_demo","type":"text","text":"The plan covers three projects"}}"#,
        "Text:The plan covers three projects",
    ),
    (
        r#"{"type":"step_start","timestamp":1,"part":{"type":"step-start","snapshot":"abc"}}"#,
        "Progress:step-start",
    ),
    (
        r#"{"type":"tool","state":{"status":"pending"}}"#,
        "Progress:tool",
    ),
    (
        r#"{"type":"tool","tool":"bash","state":{"status":"running"}}"#,
        "Progress:bash",
    ),
    (r#"{"timestamp":1,"sessionID":"ses_demo"}"#, "Silent"),
    ("not json at all", "Silent"),
    ("", "Silent"),
    (r#"{"type":"text","text":"  "}"#, "Silent"),
];

fn shape(event: &RunLine) -> String {
    match event {
        RunLine::Text(text) => format!("Text:{text}"),
        RunLine::Progress(tool) => format!("Progress:{tool}"),
        RunLine::Silent => "Silent".to_string(),
    }
}

#[test]
fn opencode_headless_rendering_matches_the_goldens() {
    let render = Harness::Opencode
        .run_line_renderer()
        .expect("opencode is a headless harness");
    for (line, expected) in GOLDENS {
        assert_eq!(&shape(&render(line)), expected, "line: {line}");
    }
}

#[test]
fn kilocode_shares_the_opencode_stream_byte_for_byte() {
    let kilo = Harness::Kilocode
        .run_line_renderer()
        .expect("kilocode is a headless harness");
    let opencode = Harness::Opencode
        .run_line_renderer()
        .expect("opencode is a headless harness");
    for (line, expected) in GOLDENS {
        assert_eq!(&shape(&kilo(line)), expected, "kilo line: {line}");
        assert_eq!(kilo(line), opencode(line), "kilo shares the stream: {line}");
    }
}

#[test]
fn claude_has_no_headless_stream() {
    assert!(Harness::Claude.run_line_renderer().is_none());
}
