//! Unresolvable `--last` spans, end to end through the real binary: a span
//! whose resolved window leaves the calendar fails as a period error —
//! non-zero exit, empty stdout, the invalid-span message on stderr — before
//! any store work or report renders. A unit test cannot assert the exit
//! status or the streams: here the binary's own stdio is real, and the
//! paths it would touch are synthetic.

mod common;

use common::ScratchDir;

/// Run the binary on a synthetic home and a scratch db, and return the raw
/// output. The period parse fails before the session opens, so no store is
/// needed — the paths it would touch are synthetic either way.
fn run(name: &str, args: &[&str]) -> std::process::Output {
    // Synthetic only. The `.claude/projects` dir is created empty so the read
    // paths' auto-analyze walks a synthetic tree instead of erroring on a
    // missing one.
    let dir = ScratchDir::new("saniwalens-period-errors", name);
    std::fs::create_dir_all(dir.join(".claude").join("projects")).unwrap();
    std::process::Command::new(env!("CARGO_BIN_EXE_saniwalens"))
        .args(args)
        .arg("--db")
        .arg(dir.join("store.db"))
        .env("HOME", &*dir)
        .env("CLAUDE_CONFIG_DIR", dir.join(".claude"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .output()
        .expect("the binary runs")
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
fn trend_rejects_a_previous_window_that_leaves_the_calendar() {
    // The current window alone resolves (its start is inside the calendar);
    // the derived previous window doubles the reach and would start below
    // the calendar's floor — the disclosure would draw an empty previous
    // since. The pair must refuse it as the same invalid-span error, before
    // any store work or report renders.
    let out = run("trend-previous", &["trend", "--last", "50000000d"]);
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
        stderr.contains("invalid --last '50000000d'"),
        "the invalid-span message is missing: {stderr}"
    );
    assert!(
        !stderr.contains("trend windows:"),
        "the window disclosure rode ahead of the error: {stderr}"
    );
}

#[test]
fn trend_rejects_an_ancient_since_whose_derived_previous_leaves_the_calendar() {
    // clap equals form: the space form is refused by clap as a stray flag,
    // which would make this a different bug. The date parses and the current
    // window resolves, but the equal-length window derived before it reaches
    // past the calendar's floor — refused with the empty-window error that
    // names `--since`, never the invalid-`--last` wording a date did not
    // earn.
    let out = run("trend-since", &["trend", "--since=-200000-01-01"]);
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
        stderr.contains("empty window: --since -200000-01-01"),
        "the error must name --since: {stderr}"
    );
    assert!(
        !stderr.contains("invalid --last"),
        "a date-derived input must not be blamed on --last: {stderr}"
    );
    assert!(
        !stderr.contains("trend windows:"),
        "the window disclosure rode ahead of the error: {stderr}"
    );
}

#[test]
fn the_ordinary_trend_still_runs_and_discloses_both_windows() {
    // The flags-absent default pair — 7d against the 7d before it — resolves
    // and runs: the calendar refusal changes only what it cannot name. Both
    // windows' disclosures carry a since and an until; no empty bound
    // anywhere.
    let out = run("trend-ordinary", &["trend"]);
    assert!(
        out.status.success(),
        "trend must run: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("trend windows: current since "),
        "the two-window disclosure is missing: {stderr}"
    );
    assert!(
        !stderr.contains("since ,"),
        "an empty bound leaked into the disclosure: {stderr}"
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

#[test]
fn usage_rejects_a_date_whose_local_midnight_leaves_the_calendar() {
    // The date path's floor, end to end on the rebuilt binary: NaiveDate::MIN's
    // local midnight in JST sits below the calendar's floor, so the zone
    // conversion refuses it as a period error — non-zero, empty stdout, no
    // window disclosure — instead of a window whose since no date can name.
    // clap equals form: the space form is refused by clap as a stray flag,
    // which would make this a different bug.
    let out = run("date-floor", &["usage", "--since=-262143-01-01"]);
    assert!(
        !out.status.success(),
        "the floor date must fail, not render: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "stdout must stay empty on the error path: {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid --since '-262143-01-01'"),
        "the invalid-date message is missing: {stderr}"
    );
    assert!(
        !stderr.contains("report window:"),
        "the disclosure line rode ahead of the error: {stderr}"
    );
}

#[test]
fn usage_resolves_a_date_at_the_calendars_ceiling() {
    // The date path's ceiling, end to end: the whole day NaiveDate::MAX names
    // ends one zone-width below the calendar's ceiling in JST, so it resolves
    // and the disclosure carries a real date — the widest named day the
    // bounds renderer can draw, with no empty-string fallback behind it.
    let out = run("date-ceiling", &["usage", "--until=+262142-12-31"]);
    assert!(
        out.status.success(),
        "the ceiling date must resolve: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("report window: until +262142-12-31T23:59:59+09:00"),
        "the ceiling instant must disclose as a date that exists: {stderr}"
    );
    assert!(
        !stderr.contains("since ,"),
        "an empty bound leaked into the disclosure: {stderr}"
    );
}
