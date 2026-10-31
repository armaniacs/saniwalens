//! Unresolvable `--last` spans, end to end through the real binary: a span
//! whose resolved window leaves the calendar fails as a period error —
//! non-zero exit, empty stdout, the invalid-span message on stderr — before
//! any store work or report renders. A unit test cannot assert the exit
//! status or the streams: here the binary's own stdio is real, and the
//! paths it would touch are synthetic.

use std::path::PathBuf;

/// A per-test scratch dir, cleaned up afterwards — synthetic only. The
/// `.claude/projects` dir is created empty so the read paths' auto-analyze
/// walks a synthetic tree instead of erroring on a missing one.
fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "saniwalens-period-errors-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    dir
}

/// Run the binary on a synthetic home and a scratch db, and return the raw
/// output. The period parse fails before the session opens, so no store is
/// needed — the paths it would touch are synthetic either way.
fn run(name: &str, args: &[&str]) -> std::process::Output {
    let dir = scratch_dir(name);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_saniwalens"))
        .args(args)
        .arg("--db")
        .arg(dir.join("store.db"))
        .env("HOME", &dir)
        .env("CLAUDE_CONFIG_DIR", dir.join(".claude"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .output()
        .expect("the binary runs");
    let _ = std::fs::remove_dir_all(&dir);
    out
}

#[test]
fn usage_rejects_a_span_whose_window_leaves_the_calendar() {
    // The spans parse (both fit i64) but their windows name dates that do
    // not exist: the command must fail as a period error — non-zero, nothing
    // on stdout, no window disclosure — instead of rendering a report whose
    // freshness line carries an empty since.
    for span in ["92233720368547d", "100000000d"] {
        let out = run(span, &["usage", "--last", span]);
        assert!(
            !out.status.success(),
            "{span} must fail, not render: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.stdout.is_empty(),
            "stdout must stay empty on the error path: {:?}",
            String::from_utf8_lossy(&out.stdout)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!("invalid --last '{span}'")),
            "the invalid-span message is missing: {stderr}"
        );
        assert!(
            !stderr.contains("report window:"),
            "the disclosure line rode ahead of the error: {stderr}"
        );
    }
}

#[test]
fn trend_rejects_a_span_whose_current_window_leaves_the_calendar() {
    // The doubled window still fits i64, so the pair's overflow guard passes
    // it — the calendar check at the shared resolve entry is what refuses it.
    let out = run("trend", &["trend", "--last", "50000000000000d"]);
    assert!(
        !out.status.success(),
        "trend must fail, not render: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "stdout must stay empty on the error path: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid --last '50000000000000d'"),
        "the invalid-span message is missing: {stderr}"
    );
}

#[test]
fn the_ordinary_last_still_runs() {
    // The ordinary span resolves and runs in the same synthetic setup: the
    // calendar check refuses only what it cannot name.
    let out = run("ordinary", &["usage", "--last", "7d"]);
    assert!(
        out.status.success(),
        "usage --last 7d must run: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
