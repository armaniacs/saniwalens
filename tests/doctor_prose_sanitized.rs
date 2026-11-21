//! Doctor's prose reaches the terminal through raw `println!` — not through
//! `render()`, the choke point that sanitizes every table and markdown cell
//! (`src/cli/report.rs`). These tests pin the invariant end to end: control
//! bytes riding a transcript-derived `file_path` must never reach doctor's
//! stdout, whether they arrive through a fresh analyze or already sit inside
//! the store from an ingest made before the boundary normalization existed
//! (ingested rows are never rewritten). The binary's own stdio is real and
//! every path it touches is synthetic (privacy rule).

use std::path::{Path, PathBuf};

use saniwalens::core::span::SessionStart;
use saniwalens::core::subagent::SubagentRun;
use saniwalens::store::{IngestBatch, OpenMode, SessionMeta, Store};

mod common;
use common::ScratchDir;

/// One Edit target as a hostile (or hallucinating) harness would record it:
/// CSI, OSC with a BEL terminator, a bare ESC-follower pair, stray BELs, C1
/// (U+009B — the 8-bit CSI a terminal would act on), a bidirectional
/// override, a bidi isolate (U+2069 — the pairing partner of the overrides
/// that `is_control()` also misses), zero-width format chars, and a carriage
/// return. The file never
/// has to exist for any of this to reach prose — the value comes straight
/// from the transcript's `tool_use.input`.
fn hostile_path() -> String {
    "/tmp/example/proj/evil\u{1b}[2J\u{1b}]0;pwn\u{7}\u{9b}\u{202e}\u{2069}weird\u{200b}\u{feff}name\u{1b}M\r with\u{7}  double  spaces.md"
        .to_string()
}

/// The `WHAT TO FIX FIRST` lines out of doctor's text report: the section runs
/// from its heading to the next section heading — today `COST`, printed at
/// column 0 while every in-section line (items, sub-lines, the dropped-items
/// note) is indented. Blank lines inside the section are layout, not a
/// terminator: the old helper cut at the first empty line, so a blank line
/// gaining between items silently dropped everything after it from the
/// extraction while the tests stayed green.
fn doctor_routed_lines(stdout: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(stdout);
    text.lines()
        .skip_while(|line| !line.contains("WHAT TO FIX FIRST"))
        .skip(1) // the heading itself
        .take_while(|line| line.trim().is_empty() || line.starts_with(' '))
        .map(|line| line.trim().to_string())
        .collect()
}

/// The section's structural floor: non-empty, and every numbered item doctor
/// printed for the fixture is inside the extraction. A terminator or layout
/// change that quietly shortens the section fails here, not in coverage.
fn assert_section_reaches_both_items(lines: &[String]) {
    let non_empty: Vec<&String> = lines.iter().filter(|line| !line.is_empty()).collect();
    assert!(
        !non_empty.is_empty(),
        "the WHAT TO FIX FIRST extraction is empty: {lines:?}"
    );
    for number in ["1.", "2."] {
        assert!(
            non_empty.iter().any(|line| line.starts_with(number)),
            "item {number} fell out of the WHAT TO FIX FIRST extraction: {non_empty:?}"
        );
    }
}

/// The `stuck re-editing` prose line — the one interpolating the thrash
/// episode's file basename.
fn stuck_line(stdout: &[u8]) -> Option<String> {
    doctor_routed_lines(stdout)
        .into_iter()
        .find(|line| line.contains("stuck re-editing"))
}

/// No control byte beyond line structure may reach stdout: ESC (and with it
/// every CSI/OSC sequence), BEL, C1 (which `is_control` covers), and CR all
/// read as hostile — a terminal would act on them instead of showing them,
/// and CR alone can rewrite the line head. Only `\n` and `\t` are prose
/// layout and stay whitelisted.
fn assert_no_control_bytes(stdout: &[u8]) {
    let text = String::from_utf8_lossy(stdout);
    let hostile: Vec<String> = text
        .chars()
        .filter(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        .map(|c| format!("U+{:04X}", c as u32))
        .collect();
    assert!(
        hostile.is_empty(),
        "control bytes reached stdout: {hostile:?} in\n{text}"
    );
}

/// Bidi overrides / isolates and zero-width format chars are invisible to
/// `is_control()` (they are Cf, not Cc), so their absence needs its own
/// assert: an override reorders what the user reads, an isolate scopes the
/// reordering, and a zero-width char spoofs text that is copied out of the
/// report.
fn assert_no_bidi_or_zero_width_chars(stdout: &[u8]) {
    let text = String::from_utf8_lossy(stdout);
    let hostile: Vec<String> = text
        .chars()
        .filter(|c| {
            matches!(
                *c as u32,
                0x202A..=0x202E | 0x2066..=0x2069 | 0x200B..=0x200F | 0xFEFF
            )
        })
        .map(|c| format!("U+{:04X}", c as u32))
        .collect();
    assert!(
        hostile.is_empty(),
        "bidi/zero-width chars reached stdout: {hostile:?} in\n{text}"
    );
}

/// Run the binary on the synthetic home and store, and return its raw output.
/// The XDG config/data roots are removed rather than redirected: the ambient
/// ones would let the binary read the user's real config, and this file's
/// contract is that every path it touches is synthetic.
fn run(dir: &Path, db: &Path, args: &[&str]) -> std::process::Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_saniwalens"));
    command.args(args).arg("--db").arg(db);
    if !args.contains(&"--frozen") {
        // Non-frozen doctor re-analyzes from the synthetic home; without the
        // config root it would walk (an empty slice of) the user's real one.
        command.env("CLAUDE_CONFIG_DIR", dir.join(".claude"));
    }
    command
        .env("HOME", dir)
        .env("XDG_STATE_HOME", dir.join("state"))
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .output()
        .expect("the binary runs")
}

/// A store assembled directly — the shape an ingest from before the boundary
/// normalization left behind: hostile bytes ride the work event's path, the
/// project name, and the session root, and the adapter never sees them again.
/// What must clean these up is the display side.
fn hostile_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let start_ms = 100_000_000;
        let path = hostile_path();
        let id = path.rsplit_once('/').unwrap().1.to_string();
        // Four edits inside the thrash window make the hostile path a stuck
        // episode; two timeouts give the project friction, so doctor prints
        // its WHAT TO FIX FIRST section at all. The Bash mix (3 of 5 `cd`)
        // adds the cd-convention item as a second one, so the section's
        // structural pin can hold both items against a silent shrink.
        let edits: Vec<(i64, &str, &str, Option<&str>)> = (0..4)
            .map(|i| {
                (
                    start_ms + i * 60_000,
                    "file_edit",
                    id.as_str(),
                    Some(path.as_str()),
                )
            })
            .chain([
                (start_ms + 400_000, "bash_cmd", "cd", None),
                (start_ms + 410_000, "bash_cmd", "cd", None),
                (start_ms + 420_000, "bash_cmd", "cd", None),
                (start_ms + 430_000, "bash_cmd", "cargo", None),
                (start_ms + 440_000, "bash_cmd", "git", None),
            ])
            .collect();
        let meta = SessionMeta {
            id: "hostile".to_string(),
            project: "demo\u{1b}[1m\u{202e}\u{9b}proj".to_string(),
            slug: "hostile".to_string(),
            root: "/tmp/example/\u{1b}]0;root\u{7}\rproj".to_string(),
            source_path: "hostile.jsonl".to_string(),
            sub_tokens: 0,
            sub_agent_count: 0,
            start: Some(SessionStart {
                timestamp_ms: start_ms,
                ctx: 9_000,
            }),
        };
        store
            .ingest(
                IngestBatch::new(&meta, &[], &[])
                    .tool_errors(&[
                        (start_ms + 100_000, "timeout", "t", "Bash", "sleep 9"),
                        (start_ms + 200_000, "timeout", "t", "Bash", "sleep 9"),
                    ])
                    .work_events(&edits),
            )
            .unwrap();
    }
    db
}

/// The subagent split's prose line — the one interpolating the raw agent
/// type name out of `subagent_runs`.
fn agent_split_line(stdout: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .find(|line| line.contains("Full split"))
        .map(|line| line.trim().to_string())
}

/// A store whose one subagent run's agent type carries the hostile
/// vocabulary — the same bytes a transcript's sidecar `agentType` can hold.
/// The agent name is the one transcript-derived value doctor interpolates
/// into prose; nothing between the store row and the `println!` may pass it
/// through unsanitized (the usage view renders the same value through
/// `sanitize_cell` — doctor must not be the weaker sibling).
fn hostile_agent_store(dir: &Path) -> PathBuf {
    let db = dir.join("store.db");
    {
        let mut store = Store::open(&db, OpenMode::ReadWrite).unwrap();
        let start_ms = 100_000_000;
        let meta = SessionMeta {
            id: "hostile-agent".to_string(),
            project: "agent-demo".to_string(),
            slug: "hostile-agent".to_string(),
            root: "/tmp/example/agent-proj".to_string(),
            source_path: "hostile-agent.jsonl".to_string(),
            sub_tokens: 40_000,
            sub_agent_count: 1,
            start: Some(SessionStart {
                timestamp_ms: start_ms,
                ctx: 9_000,
            }),
        };
        let run = SubagentRun {
            agent: Some("ev\u{1b}[31m\u{202e}il\u{200b}\u{9b}agent\u{7}".to_string()),
            agent_id: "run-1".to_string(),
            run_path: "run.jsonl".to_string(),
            tool_use_id: None,
            parent_agent_id: None,
            spawn_depth: 1,
            model: None,
            prompt_id: None,
            out_tokens: 40_000,
            started_epoch_ms: start_ms,
            root_tool_use_id: None,
        };
        store
            .ingest(IngestBatch::new(&meta, &[], &[]).subagent_runs(&[run]))
            .unwrap();
    }
    db
}

#[test]
fn doctor_never_prints_a_hostile_path_already_inside_the_store() {
    let dir = ScratchDir::new("saniwalens-doctor-prose", "hostile-store");
    let db = hostile_store(&dir);
    let out = run(&dir, &db, &["doctor", "--frozen"]);
    assert!(
        out.status.success(),
        "doctor must run: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The trigger reproduced: the stuck line names the hostile file.
    let stuck = stuck_line(&out.stdout).expect("doctor prints the stuck line");
    assert_section_reaches_both_items(&doctor_routed_lines(&out.stdout));
    // The readable data survives sanitization; the bytes a terminal would
    // act on do not.
    assert!(
        stuck.contains("evil"),
        "the basename's data is kept: {stuck}"
    );
    assert!(
        stuck.contains("spaces.md"),
        "the basename's tail is kept: {stuck}"
    );
    assert_no_control_bytes(&out.stdout);
    assert_no_bidi_or_zero_width_chars(&out.stdout);
}

#[test]
fn doctor_never_prints_a_hostile_agent_name_from_the_store() {
    let dir = ScratchDir::new("saniwalens-doctor-prose", "hostile-agent");
    let db = hostile_agent_store(&dir);
    let out = run(&dir, &db, &["doctor", "--frozen"]);
    assert!(
        out.status.success(),
        "doctor must run: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The trigger reproduced: the split line names the hostile agent type.
    let split = agent_split_line(&out.stdout).expect("doctor prints the agent split line");
    assert!(
        split.contains("evilagent"),
        "the agent name's readable data is kept: {split}"
    );
    assert_no_control_bytes(&out.stdout);
    assert_no_bidi_or_zero_width_chars(&out.stdout);
}

#[test]
fn analyze_then_doctor_carries_no_control_bytes_to_stdout() {
    let dir = ScratchDir::new("saniwalens-doctor-prose", "hostile-transcript");
    let projects = dir
        .join(".claude")
        .join("projects")
        .join("-tmp-example-proj");
    std::fs::create_dir_all(&projects).unwrap();
    // The PBI's trigger: one session whose Edit file_path carries ESC / CSI /
    // OSC / BEL sequences — four edits inside the thrash window, plus one
    // failed tool result so the project owns friction and doctor prints its
    // section. The path also rides C1, a bidi override, and zero-width chars
    // (a raw CR cannot ride a JSON string unescaped and is collapsed by the
    // ingest whitespace fold — the store-root route covers CR instead).
    let hostile_file_path = "/tmp/example/proj/evil\\u001b[2J\\u001b]0;pwn\\u0007\\u009b\\u202eweird\\u200b\\ufeff name\\u0007  double  spaces.md";
    let edit = |timestamp: &str| {
        format!(
            r#"{{"type":"assistant","timestamp":"{timestamp}","message":{{"model":"synthetic-model","usage":{{"input_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":40}},"content":[{{"type":"tool_use","name":"Edit","input":{{"file_path":"{hostile_file_path}"}}}}]}}}}"#
        )
    };
    std::fs::write(
        projects.join("aaaaaaaa-0000-0000-0000-000000000001.jsonl"),
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            r#"{"type":"user","cwd":"/tmp/example/proj","timestamp":"2026-01-01T00:00:00.000Z","message":{"content":"do it"}}"#,
            edit("2026-01-01T00:01:00.000Z"),
            edit("2026-01-01T00:02:00.000Z"),
            edit("2026-01-01T00:03:00.000Z"),
            edit("2026-01-01T00:04:00.000Z"),
            r#"{"type":"user","timestamp":"2026-01-01T00:04:30.000Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1","is_error":true,"content":"command not found: cargo"}]}}"#,
            r#"{"type":"assistant","timestamp":"2026-01-01T00:05:00.000Z","message":{"model":"synthetic-model","usage":{"input_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":10}}}"#,
        ),
    )
    .unwrap();
    let db = dir.join("store.db");
    let out = run(&dir, &db, &["doctor"]);
    assert!(
        out.status.success(),
        "doctor must run: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stuck = stuck_line(&out.stdout).expect("doctor prints the stuck line");
    assert!(
        stuck.contains("spaces.md"),
        "the basename's tail survives: {stuck}"
    );
    assert_no_control_bytes(&out.stdout);
    assert_no_bidi_or_zero_width_chars(&out.stdout);
}
