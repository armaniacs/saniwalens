//! The plans/backups cleanup command, outside-in
//! (`pbi/2026-11-10-02-feat-plans-cleanup.md`).
//!
//! Every `--apply` handoff leaves a plan file behind and every apply session
//! leaves a backup generation behind; without a sweeper the secret-bearing
//! `0600` files accumulate without bound. This file pins the list and sweep
//! through the real binary. Every fixture is synthetic (a scratch HOME the
//! binary resolves its state home from); no harness binary is ever spawned
//! and no store is ever needed.

use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::ScratchDir;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_saniwalens"))
}

/// A synthetic home sandbox: `HOME` and `XDG_STATE_HOME` both point into
/// scratch space, so the binary under test never touches the real home or
/// the real state dir.
fn sandbox(name: &str) -> ScratchDir {
    ScratchDir::new("saniwalens-cleanup-e2e", name)
}

fn run_in(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .args(args)
        .env("HOME", home)
        .env("XDG_STATE_HOME", home.join("state"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .output()
        .expect("the binary runs")
}

fn plans_dir(home: &Path) -> PathBuf {
    home.join("state").join("saniwalens").join("plans")
}

fn backups_dir(home: &Path) -> PathBuf {
    home.join("state").join("saniwalens").join("backups")
}

fn write_plan(home: &Path, name: &str, body: &str) -> PathBuf {
    let dir = plans_dir(home);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

fn write_backup(home: &Path, generation: &str, name: &str, body: &str) -> PathBuf {
    let dir = backups_dir(home).join(generation);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

/// Give `path` a distinct synthetic mtime (seconds past a fixed base), so
/// "newest" is deterministic without sleeping between creates.
#[cfg(unix)]
fn set_mtime(path: &Path, secs: u32) {
    assert!(secs < 60, "the base minute holds the whole sequence");
    let stamp = format!("202601010000.{secs:02}");
    let status = Command::new("touch")
        .args(["-t", &stamp, &path.display().to_string()])
        .status()
        .expect("touch runs");
    assert!(status.success(), "touch sets the mtime");
}

#[test]
fn cleanup_help_advertises_list_and_keep() {
    let out = Command::new(binary())
        .args(["cleanup", "--help"])
        .output()
        .expect("the binary runs");
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(
        help.contains("--list"),
        "the list mode is not advertised:\n{help}"
    );
    assert!(
        help.contains("--keep"),
        "the sweep mode is not advertised:\n{help}"
    );
}

#[test]
fn list_shows_paths_sizes_and_times_and_deletes_nothing() {
    // BDD "一覧できる": path, size, and creation time per entry.
    let home = sandbox("list");
    let first = write_plan(&home, "plan-aaa.md", "12345");
    let second = write_plan(&home, "plan-bbb.md", "1234567");
    let generation = backups_dir(&home).join("2026-01-01T00-00-00");
    let backed = write_backup(&home, "2026-01-01T00-00-00", "CLAUDE.md", "old");

    for args in [vec!["cleanup"], vec!["cleanup", "--list"]] {
        let out = run_in(&home, &args);
        assert!(
            out.status.success(),
            "list failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert!(stdout.contains(&first.display().to_string()), "{stdout}");
        assert!(stdout.contains(&second.display().to_string()), "{stdout}");
        // A backup generation lists as one entry (its own dir, not its
        // children): the sweep deletes generations, never loose files.
        assert!(
            stdout.contains(&generation.display().to_string()),
            "{stdout}"
        );
        assert!(stdout.contains('B'), "no sizes rendered:\n{stdout}");
        assert!(stdout.contains("2026"), "no timestamps rendered:\n{stdout}");
    }

    assert!(first.exists(), "a listing deletes nothing");
    assert!(second.exists(), "a listing deletes nothing");
    assert!(backed.exists(), "a listing deletes nothing");
}

#[test]
fn list_needs_no_store() {
    // The sweep owns handoff files, not analysis: it must work on a fresh
    // machine with no store and create none.
    let home = sandbox("no-store");
    write_plan(&home, "plan-aaa.md", "plan");

    let out = run_in(&home, &["cleanup"]);
    assert!(
        out.status.success(),
        "list failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !home
            .join("state")
            .join("saniwalens")
            .join("saniwalens.db")
            .exists(),
        "listing created a store it never needs"
    );
}

#[test]
fn list_json_reports_entries_and_the_total() {
    let home = sandbox("list-json");
    write_plan(&home, "plan-aaa.md", "12345");
    write_backup(&home, "2026-01-01T00-00-00", "CLAUDE.md", "old!");

    let out = run_in(&home, &["cleanup", "--format", "json"]);
    assert!(
        out.status.success(),
        "json list failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is one JSON document");
    let areas = payload["areas"].as_array().expect("areas array");
    assert_eq!(areas.len(), 2, "{payload}");
    // The plan's own bytes plus the generation's recursive total over its
    // files (links never followed).
    assert!(
        payload["total_bytes"].as_u64().unwrap() >= 5 + 4,
        "{payload}"
    );
    let paths: Vec<&str> = areas
        .iter()
        .flat_map(|area| area["entries"].as_array().unwrap().iter())
        .map(|entry| entry["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths.len(), 2, "{payload}");
    assert!(
        paths.iter().any(|path| path.ends_with("plan-aaa.md")),
        "{payload}"
    );
}

#[cfg(unix)]
#[test]
fn sweep_keeps_the_newest_and_reports_every_removed_path() {
    // BDD "掃除できる": the two newest of four plans survive, the two
    // oldest are deleted and every deleted path is printed.
    let home = sandbox("sweep");
    let oldest = write_plan(&home, "plan-01.md", "oldest");
    set_mtime(&oldest, 1);
    let old = write_plan(&home, "plan-02.md", "old");
    set_mtime(&old, 2);
    let new = write_plan(&home, "plan-03.md", "new");
    set_mtime(&new, 3);
    let newest = write_plan(&home, "plan-04.md", "newest");
    set_mtime(&newest, 4);
    let gen_old = write_backup(&home, "2026-01-01T00-00-00", "CLAUDE.md", "old");
    let gen_mid = write_backup(&home, "2026-01-02T00-00-00", "CLAUDE.md", "mid");
    let gen_new = write_backup(&home, "2026-01-03T00-00-00", "CLAUDE.md", "new");
    // Generations are dirs: the mtime that orders them is the dir's own.
    set_mtime(&backups_dir(&home).join("2026-01-01T00-00-00"), 1);
    set_mtime(&backups_dir(&home).join("2026-01-02T00-00-00"), 2);
    set_mtime(&backups_dir(&home).join("2026-01-03T00-00-00"), 3);

    let out = run_in(&home, &["cleanup", "--keep", "2"]);
    assert!(
        out.status.success(),
        "sweep failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("keeping 2 newest, removing 2"), "{stdout}");
    assert!(
        stdout.contains(&oldest.display().to_string()),
        "a removed path is reported:\n{stdout}"
    );
    assert!(
        stdout.contains(&old.display().to_string()),
        "a removed path is reported:\n{stdout}"
    );
    assert!(
        !stdout.contains(&new.display().to_string()),
        "a kept path stays unreported:\n{stdout}"
    );

    assert!(!oldest.exists(), "the oldest plan is gone");
    assert!(!old.exists(), "the second-oldest plan is gone");
    assert!(new.exists(), "a kept plan stays");
    assert!(newest.exists(), "the newest plan stays");
    assert!(gen_mid.exists(), "a kept backup stays");
    assert!(gen_new.exists(), "the newest backup stays");
    assert!(!gen_old.exists(), "the pruned backup generation is gone");
}

#[cfg(unix)]
#[test]
fn sweep_markdown_renders_the_removed_paths_as_a_table() {
    // M-3: the human sweep goes through the shared table renderer, so
    // `--format markdown` pastes as a real table.
    let home = sandbox("sweep-markdown");
    let oldest = write_plan(&home, "plan-01.md", "oldest");
    set_mtime(&oldest, 1);
    let newest = write_plan(&home, "plan-02.md", "newest");
    set_mtime(&newest, 2);

    let out = run_in(&home, &["cleanup", "--keep", "1", "--format", "markdown"]);
    assert!(
        out.status.success(),
        "sweep failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("| AREA |"), "{stdout}");
    assert!(stdout.contains(&oldest.display().to_string()), "{stdout}");
    assert!(!oldest.exists(), "the oldest plan is gone");
    assert!(newest.exists(), "the kept plan stays");
}

#[cfg(unix)]
#[test]
fn sweep_removes_a_dangling_link_and_reports_it() {
    // M-2: a link whose target is gone still sits inside the area, so a
    // `--keep 0` sweep removes the link itself.
    use std::os::unix::fs::symlink;
    let home = sandbox("dangling");
    let link = plans_dir(&home);
    std::fs::create_dir_all(&link).unwrap();
    let link = link.join("plan-link.md");
    symlink(home.join("no-such-target.md"), &link).unwrap();

    let out = run_in(&home, &["cleanup", "--keep", "0"]);
    assert!(
        out.status.success(),
        "sweep failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(&link.display().to_string()),
        "the removed link is reported:\n{stdout}"
    );
    assert!(
        std::fs::symlink_metadata(&link).is_err(),
        "the dangling link itself is gone"
    );
}

#[cfg(unix)]
#[test]
fn sweep_leaves_outside_files_and_escaping_links_alone() {
    // BDD "対象外は触らない": a same-named file outside the state home and
    // a symlink inside plans/ pointing at a secret outside both survive a
    // `--keep 0` sweep that deletes everything inside.
    use std::os::unix::fs::symlink;
    let home = sandbox("outside");
    let inside = write_plan(&home, "plan-aaa.md", "inside");
    let bystander = home.join("plan-aaa.md");
    std::fs::write(&bystander, "bystander").unwrap();
    let secret = home.join("secret.md");
    std::fs::write(&secret, "secret").unwrap();
    let link = plans_dir(&home).join("plan-link.md");
    symlink(&secret, &link).unwrap();

    let out = run_in(&home, &["cleanup", "--keep", "0"]);
    assert!(
        out.status.success(),
        "sweep failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("skipped"),
        "the escape is not reported:\n{stderr}"
    );

    assert!(!inside.exists(), "the inside plan is gone");
    assert_eq!(std::fs::read_to_string(&bystander).unwrap(), "bystander");
    assert_eq!(std::fs::read_to_string(&secret).unwrap(), "secret");
    assert!(link.exists(), "the escaping link itself stays");
}

#[cfg(unix)]
#[test]
fn bare_keep_prunes_to_the_default_ten() {
    // The documented default: a bare `--keep` keeps the ten newest.
    let home = sandbox("default-keep");
    for n in 1..=11u32 {
        let path = write_plan(&home, &format!("plan-{n:02}.md"), "x");
        set_mtime(&path, n);
    }

    let out = run_in(&home, &["cleanup", "--keep"]);
    assert!(
        out.status.success(),
        "sweep failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("keeping 10 newest, removing 1"), "{stdout}");
    let remaining: Vec<_> = std::fs::read_dir(plans_dir(&home)).unwrap().collect();
    assert_eq!(remaining.len(), 10, "one generation pruned");
    assert!(
        !plans_dir(&home).join("plan-01.md").exists(),
        "the oldest generation is the one pruned"
    );
}

#[test]
fn sweep_on_empty_areas_succeeds_with_zero_counts() {
    // BDD境界 0件: nothing saved yet is not an error.
    let home = sandbox("empty");
    let out = run_in(&home, &["cleanup", "--keep", "5"]);
    assert!(
        out.status.success(),
        "empty sweep failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("removing 0"), "{stdout}");
    assert!(stdout.contains("removed 0"), "{stdout}");
}

#[test]
fn sweep_json_reports_removed_and_kept_per_area() {
    let home = sandbox("sweep-json");
    write_plan(&home, "plan-aaa.md", "12345");

    let out = run_in(&home, &["cleanup", "--keep", "0", "--format", "json"]);
    assert!(
        out.status.success(),
        "json sweep failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is one JSON document");
    assert_eq!(payload["removed_total"], serde_json::json!(1), "{payload}");
    assert_eq!(payload["removed_bytes"], serde_json::json!(5), "{payload}");
    let plans = &payload["areas"][0];
    assert_eq!(plans["area"], serde_json::json!("plans"), "{payload}");
    assert_eq!(plans["removed"].as_array().unwrap().len(), 1, "{payload}");
    assert!(!plans_dir(&home).join("plan-aaa.md").exists());
}

#[test]
fn bad_keep_and_list_with_keep_fail() {
    // PBI単体 "フラグ解釈 (--keep の不正値)": clap refuses a non-number,
    // and --list conflicts with the sweep.
    let home = sandbox("bad-flags");
    for args in [
        vec!["cleanup", "--keep", "abc"],
        vec!["cleanup", "--list", "--keep", "3"],
    ] {
        let out = run_in(&home, &args);
        assert!(!out.status.success(), "bad flags accepted: {args:?}");
    }
}
