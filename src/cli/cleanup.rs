//! Prune the `--apply` handoff's leftovers: saved plans and backups
//! (`docs/specs/cli.md`).
//!
//! Every `--apply` run leaves a plan file behind, and every apply session
//! leaves a backup generation behind; without a sweeper the secret-bearing
//! `0600` files accumulate without bound. This command lists what is there
//! and deletes all but the newest N generations per area. It reads and
//! deletes only under the handoff's own directories (`plans/` beside
//! `backups/` under the state home) ― never the store, never anything
//! outside them ― so it needs no harness, no store, and no refresh.
//!
//! The generation choice (which entries are victims) and the path scoping
//! (what counts as inside) are pure over their inputs and unit-tested; only
//! the directory walk and the deletes are I/O.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};

use super::handoff;
use super::{Align, Format, emit_json, render};
use crate::session::SessionEnv;

/// One listed entry: a plan file or one backup generation directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The entry's own path (a generation dir itself, not its children).
    pub path: PathBuf,
    /// The entry's size in bytes: a file's own length, a generation dir's
    /// recursive total over the files under it. Symlinks never resolve:
    /// a link counts as a link instead of pulling its target's size in.
    pub bytes: u64,
    /// The entry's own modification time, for newest-first ordering.
    pub modified: SystemTime,
}

/// Split `entries` into (kept, victims): the newest `keep` generations stay,
/// the rest go. Newest-first is `(modified, path)`: a newer mtime wins, and
/// an mtime tie breaks on the path so the choice is deterministic even when
/// two generations land in the same timestamp tick.
pub fn select_victims(entries: Vec<Entry>, keep: usize) -> (Vec<Entry>, Vec<Entry>) {
    let mut ordered = entries;
    ordered.sort_by(|left, right| {
        right
            .modified
            .cmp(&left.modified)
            .then_with(|| left.path.cmp(&right.path))
    });
    let split = keep.min(ordered.len());
    let victims = ordered.split_off(split);
    (ordered, victims)
}

/// A human byte count for the table: `512 B`, `1.5 KB`, `1.0 MB`.
pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let size = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", size / KIB)
    } else {
        format!("{:.1} MB", size / (KIB * KIB))
    }
}

/// True when `candidate` counts as inside `root`. A resolvable path counts
/// when its resolved form sits under the resolved root, symlinks followed,
/// so a link pointing outside the area reads as outside. A dangling link
/// resolves nowhere: it counts by its own seat instead, inside when its
/// parent dir resolves inside the root (the sweep then removes the link
/// itself, never a target). A path that no longer resolves at all and is
/// not a link is not inside; the sweep ignores it as vanished, not skipped.
pub fn is_within(root: &Path, candidate: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    if let Ok(resolved) = candidate.canonicalize() {
        return resolved.starts_with(&root);
    }
    let Ok(meta) = fs::symlink_metadata(candidate) else {
        return false;
    };
    if !meta.file_type().is_symlink() {
        return false;
    }
    let Some(parent) = candidate.parent() else {
        return false;
    };
    let Ok(parent) = parent.canonicalize() else {
        return false;
    };
    parent.starts_with(&root)
}

/// Run the `cleanup` command: list the handoff's plans and backups, or ―
/// with `keep` ― delete all but the newest `keep` generations per area.
pub(crate) fn run_cleanup(
    env: &SessionEnv,
    _list: bool,
    keep: Option<usize>,
    format: Format,
) -> Result<()> {
    let home = env.home.map(Path::new);
    let areas = [
        ("plans", handoff::plans_dir(env.state_home, home)?),
        ("backups", handoff::backups_dir(env.state_home, home)?),
    ];
    match keep {
        Some(keep) => sweep_areas(&areas, keep, format),
        None => list_areas(&areas, format),
    }
}

/// The area's direct children as entries: plan files and backup generation
/// dirs alike, never the area dir itself. A missing area lists as empty ― a
/// fresh machine simply has nothing saved yet. A generation dir's size is
/// the recursive total over the files under it; symlinks never resolve, so
/// a link counts as a link instead of pulling its target's size (or its
/// target at all) into the listing.
pub fn collect_entries(dir: &Path) -> Vec<Entry> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for child in read.flatten() {
        let path = child.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let bytes = match meta.file_type().is_dir() {
            true => dir_size(&path),
            false => meta.len(),
        };
        entries.push(Entry {
            path,
            bytes,
            modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        });
    }
    entries
}

/// The recursive total over the files under `dir`, links never followed:
/// a real subdir recurses, anything else (file, link) counts its own
/// length. Unreadable children are skipped, so a listing never fails over
/// one bad entry.
fn dir_size(dir: &Path) -> u64 {
    let Ok(read) = fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0u64;
    for child in read.flatten() {
        let path = child.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        total = total.saturating_add(match meta.file_type().is_dir() {
            true => dir_size(&path),
            false => meta.len(),
        });
    }
    total
}

/// What one area's sweep did: victims deleted, outsiders skipped. `removed`
/// carries the entries (with their listed bytes) so the report totals what
/// the sweep actually deleted.
#[derive(Debug, PartialEq, Eq)]
pub struct SweepReport {
    pub kept: usize,
    pub removed: Vec<Entry>,
    pub skipped: Vec<PathBuf>,
}

/// Delete all but the newest `keep` entries under `root`. Every victim is
/// re-checked with [`is_within`] right before the delete, so a symlink that
/// escapes the area is skipped instead of followed out; a dangling link
/// counts by its own parent and its link is removed; a victim that vanished
/// since the listing is ignored entirely (the PBI's no-atomicity pitfall
/// only asks that a double delete never falls over).
pub fn sweep_area(root: &Path, entries: Vec<Entry>, keep: usize) -> Result<SweepReport> {
    let (kept, victims) = select_victims(entries, keep);
    let mut removed = Vec::new();
    let mut skipped = Vec::new();
    for victim in victims {
        if !is_within(root, &victim.path) {
            // Not inside and not even a link with a seat inside: a victim
            // that vanished since the listing is not an escape, so ignore
            // it instead of reporting it skipped.
            if fs::symlink_metadata(&victim.path)
                .is_err_and(|err| err.kind() == std::io::ErrorKind::NotFound)
            {
                continue;
            }
            skipped.push(victim.path);
            continue;
        }
        if remove_victim(&victim.path)? {
            removed.push(victim);
        }
    }
    Ok(SweepReport {
        kept: kept.len(),
        removed,
        skipped,
    })
}

/// Delete one in-scope victim: a symlink or file goes with `remove_file`
/// (the link, never its target), a real directory with `remove_dir_all`.
/// True when something was actually deleted; a victim that vanished since
/// the listing reports false instead of failing.
fn remove_victim(path: &Path) -> Result<bool> {
    let file_type = match fs::symlink_metadata(path) {
        Ok(meta) => meta.file_type(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err).with_context(|| format!("stat {}", path.display())),
    };
    let outcome = if file_type.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    match outcome {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("remove {}", path.display())),
    }
}

/// Newest-first for display: the same order the sweep keeps in.
fn newest_first(entries: &mut [Entry]) {
    entries.sort_by(|left, right| {
        right
            .modified
            .cmp(&left.modified)
            .then_with(|| left.path.cmp(&right.path))
    });
}

fn format_time(modified: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(modified)
        .format("%Y-%m-%d %H:%M:%S%:z")
        .to_string()
}

fn format_time_rfc3339(modified: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(modified)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// List both areas: path, size, and modification time per entry, with the
/// per-area counts and the total underneath. BDD "一覧できる".
fn list_areas(areas: &[(&str, PathBuf)], format: Format) -> Result<()> {
    let mut listed: Vec<(&str, Vec<Entry>)> = Vec::new();
    let mut total = 0u64;
    for (area, dir) in areas {
        let mut entries = collect_entries(dir);
        newest_first(&mut entries);
        total += entries.iter().map(|entry| entry.bytes).sum::<u64>();
        listed.push((area, entries));
    }
    if format == Format::Json {
        let areas_json: Vec<serde_json::Value> = listed
            .iter()
            .map(|(area, entries)| {
                serde_json::json!({
                    "area": area,
                    "entries": entries.iter().map(|entry| serde_json::json!({
                        "path": entry.path.display().to_string(),
                        "bytes": entry.bytes,
                        "modified": format_time_rfc3339(entry.modified),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        return emit_json(&serde_json::json!({
            "areas": areas_json,
            "total_bytes": total,
        }));
    }
    let mut rows = Vec::new();
    let mut counts = Vec::new();
    for (area, entries) in &listed {
        counts.push(format!("{} {area}(s)", entries.len()));
        for entry in entries {
            rows.push(vec![
                area.to_string(),
                entry.path.display().to_string(),
                format_bytes(entry.bytes),
                format_time(entry.modified),
            ]);
        }
    }
    render(
        &["AREA", "PATH", "SIZE", "MODIFIED"],
        &[Align::Left; 4],
        &rows,
        format,
    );
    println!("\n{} total", counts.join(", "));
    println!("prune with `saniwalens cleanup --keep [N]` (keeps the newest N per area)");
    Ok(())
}

/// Sweep both areas down to the newest `keep` generations each: snapshot
/// every area, print the count and total size before deleting (the PBI's
/// mis-deletion guard shows its evidence before anything goes), then
/// delete, then report every removed path through the shared table
/// renderer. BDD "掃除できる".
fn sweep_areas(areas: &[(&str, PathBuf)], keep: usize, format: Format) -> Result<()> {
    // Snapshot first so the intent printout precedes every delete.
    let mut snapshots: Vec<(&str, PathBuf, Vec<Entry>)> = Vec::new();
    for (area, dir) in areas {
        snapshots.push((area, dir.clone(), collect_entries(dir)));
    }
    for (area, _dir, entries) in &snapshots {
        let (kept, victims) = select_victims(entries.clone(), keep);
        let bytes: u64 = victims.iter().map(|entry| entry.bytes).sum();
        if format == Format::Json {
            continue;
        }
        println!(
            "{area}: keeping {} newest, removing {} ({})",
            kept.len(),
            victims.len(),
            format_bytes(bytes)
        );
    }
    let mut reports: Vec<(&str, SweepReport)> = Vec::new();
    for (area, dir, entries) in snapshots {
        reports.push((area, sweep_area(&dir, entries, keep)?));
    }
    let removed_total: usize = reports.iter().map(|(_, report)| report.removed.len()).sum();
    let removed_bytes: u64 = reports
        .iter()
        .flat_map(|(_, report)| report.removed.iter().map(|entry| entry.bytes))
        .sum();
    let skipped_total: usize = reports.iter().map(|(_, report)| report.skipped.len()).sum();
    if format == Format::Json {
        let areas_json: Vec<serde_json::Value> = reports
            .iter()
            .map(|(area, report)| {
                serde_json::json!({
                    "area": area,
                    "kept": report.kept,
                    "removed": report.removed.iter().map(|entry| serde_json::json!({
                        "path": entry.path.display().to_string(),
                        "bytes": entry.bytes,
                    })).collect::<Vec<_>>(),
                    "removed_bytes": report.removed.iter().map(|entry| entry.bytes).sum::<u64>(),
                    "skipped": report.skipped.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
                })
            })
            .collect();
        return emit_json(&serde_json::json!({
            "keep": keep,
            "areas": areas_json,
            "removed_total": removed_total,
            "removed_bytes": removed_bytes,
        }));
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    for (area, report) in &reports {
        for entry in &report.removed {
            rows.push(vec![
                area.to_string(),
                entry.path.display().to_string(),
                format_bytes(entry.bytes),
            ]);
        }
    }
    if !rows.is_empty() {
        render(
            &["AREA", "PATH", "SIZE"],
            &[Align::Left, Align::Left, Align::Left],
            &rows,
            format,
        );
    }
    for report in reports.iter().map(|(_, report)| report) {
        for skipped in &report.skipped {
            eprintln!("skipped (outside the area): {}", skipped.display());
        }
    }
    println!(
        "removed {removed_total} entrie(s) ({}), skipped {skipped_total}",
        format_bytes(removed_bytes)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, secs: u64) -> Entry {
        Entry {
            path: PathBuf::from(name),
            bytes: name.len() as u64,
            modified: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs),
        }
    }

    fn names(entries: &[Entry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| entry.path.display().to_string())
            .collect()
    }

    #[test]
    fn an_empty_area_has_no_victims() {
        let (kept, victims) = select_victims(Vec::new(), 10);
        assert!(kept.is_empty());
        assert!(victims.is_empty());
    }

    #[test]
    fn fewer_entries_than_keep_keeps_everything() {
        let (kept, victims) = select_victims(vec![entry("b", 2), entry("a", 1)], 10);
        assert_eq!(names(&kept), vec!["b".to_string(), "a".to_string()]);
        assert!(victims.is_empty());
    }

    #[test]
    fn exactly_keep_entries_keeps_everything() {
        let (kept, victims) = select_victims(vec![entry("b", 2), entry("a", 1)], 2);
        assert_eq!(names(&kept), vec!["b".to_string(), "a".to_string()]);
        assert!(victims.is_empty());
    }

    #[test]
    fn one_over_keep_removes_only_the_oldest() {
        // BDD "新しい N 世代だけが残り古いものが消える": the boundary.
        let (kept, victims) = select_victims(
            vec![entry("oldest", 1), entry("mid", 2), entry("newest", 3)],
            2,
        );
        assert_eq!(names(&kept), vec!["newest".to_string(), "mid".to_string()]);
        assert_eq!(names(&victims), vec!["oldest".to_string()]);
    }

    #[test]
    fn keep_zero_removes_everything() {
        // An explicit `--keep 0` means no generation survives: the uniform
        // rule, not a special case.
        let (kept, victims) = select_victims(vec![entry("b", 2), entry("a", 1)], 0);
        assert!(kept.is_empty());
        assert_eq!(names(&victims), vec!["b".to_string(), "a".to_string()]);
    }

    #[test]
    fn an_unsorted_listing_still_keeps_the_newest() {
        let (kept, victims) = select_victims(
            vec![entry("oldest", 1), entry("newest", 3), entry("mid", 2)],
            1,
        );
        assert_eq!(names(&kept), vec!["newest".to_string()]);
        assert_eq!(
            names(&victims),
            vec!["mid".to_string(), "oldest".to_string()]
        );
    }

    #[test]
    fn an_mtime_tie_breaks_on_the_path() {
        // Two generations in one timestamp tick: the choice stays
        // deterministic instead of depending on read_dir order.
        let (kept, victims) = select_victims(vec![entry("b", 7), entry("a", 7)], 1);
        assert_eq!(names(&kept), vec!["a".to_string()]);
        assert_eq!(names(&victims), vec!["b".to_string()]);
    }

    #[test]
    fn byte_counts_render_as_below_a_kibibyte() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn byte_counts_render_kibibytes_with_one_decimal() {
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
    }

    #[test]
    fn byte_counts_render_mebibytes_with_one_decimal() {
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn a_child_of_the_root_is_within() {
        let dir = std::env::temp_dir().join(format!(
            "saniwalens-cleanup-within-{}-{}",
            std::process::id(),
            "child"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("plans")).unwrap();
        let child = dir.join("plans").join("plan-x.md");
        std::fs::write(&child, "plan").unwrap();
        assert!(is_within(&dir.join("plans"), &child));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sibling_of_the_root_is_not_within() {
        let dir = std::env::temp_dir().join(format!(
            "saniwalens-cleanup-within-{}-{}",
            std::process::id(),
            "sibling"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("plans")).unwrap();
        std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
        let outside = dir.join("elsewhere").join("plan-x.md");
        std::fs::write(&outside, "plan").unwrap();
        assert!(!is_within(&dir.join("plans"), &outside));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_vanished_candidate_is_not_within() {
        let dir = std::env::temp_dir().join(format!(
            "saniwalens-cleanup-within-{}-{}",
            std::process::id(),
            "vanished"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("plans")).unwrap();
        assert!(!is_within(
            &dir.join("plans"),
            &dir.join("plans").join("gone.md")
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_escaping_the_root_is_not_within() {
        // BDD "対象外は触らない": a symlink inside plans/ pointing outside
        // must never read as inside, or the sweep would follow it out.
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!(
            "saniwalens-cleanup-within-{}-{}",
            std::process::id(),
            "escape"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("plans")).unwrap();
        let target = dir.join("secret.md");
        std::fs::write(&target, "secret").unwrap();
        let link = dir.join("plans").join("plan-link.md");
        symlink(&target, &link).unwrap();
        assert!(!is_within(&dir.join("plans"), &link));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cleanup_parses_bare_list_and_keep_forms() {
        // The sweep needs no store, no harness, and no refresh: its flags
        // are only the mode plus the shared `--format`.
        use crate::cli::Cli;
        use clap::Parser;
        assert!(Cli::try_parse_from(["saniwalens", "cleanup"]).is_ok());
        assert!(Cli::try_parse_from(["saniwalens", "cleanup", "--list"]).is_ok());
        assert!(Cli::try_parse_from(["saniwalens", "cleanup", "--keep", "3"]).is_ok());
        assert!(Cli::try_parse_from(["saniwalens", "cleanup", "--keep"]).is_ok());
    }

    #[test]
    fn cleanup_rejects_a_bad_keep_and_list_with_keep() {
        use crate::cli::Cli;
        use clap::Parser;
        assert!(Cli::try_parse_from(["saniwalens", "cleanup", "--keep", "abc"]).is_err());
        assert!(Cli::try_parse_from(["saniwalens", "cleanup", "--keep", "-1"]).is_err());
        assert!(Cli::try_parse_from(["saniwalens", "cleanup", "--list", "--keep", "3"]).is_err());
    }

    /// A scratch area with real files, removed on drop so a failure does not
    /// leak plan files into the runner's temp dir.
    struct Area(PathBuf);

    impl Area {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "saniwalens-cleanup-area-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn file(&self, name: &str, body: &str, secs: u64) -> Entry {
            let path = self.0.join(name);
            std::fs::write(&path, body).unwrap();
            Entry {
                path,
                bytes: body.len() as u64,
                modified: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs),
            }
        }
    }

    impl Drop for Area {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn sweep_deletes_only_the_oldest_generations() {
        // BDD "新しい N 世代だけが残り古いものが消える", through real
        // deletes: the listed entries carry explicit times, so no sleeps.
        let area = Area::new("sweep");
        let oldest = area.file("plan-oldest.md", "old", 100);
        let mid = area.file("plan-mid.md", "mid!", 200);
        let newest = area.file("plan-newest.md", "newest!", 300);
        let entries = vec![oldest.clone(), mid.clone(), newest.clone()];

        let report = sweep_area(area.path(), entries, 2).unwrap();

        assert_eq!(report.kept, 2);
        assert!(report.skipped.is_empty());
        assert!(!oldest.path.exists(), "the oldest generation is gone");
        assert!(mid.path.exists(), "a kept generation stays");
        assert!(newest.path.exists(), "the newest generation stays");
        assert_eq!(report.removed, vec![oldest]);
    }

    #[test]
    fn sweep_with_room_to_spare_deletes_nothing() {
        let area = Area::new("spare");
        let first = area.file("plan-a.md", "a", 100);
        let second = area.file("plan-b.md", "b", 200);

        let report = sweep_area(area.path(), vec![first.clone(), second.clone()], 5).unwrap();

        assert_eq!(report.kept, 2);
        assert!(report.removed.is_empty());
        assert!(first.path.exists() && second.path.exists());
    }

    #[test]
    fn sweep_tolerates_a_victim_that_vanished_first() {
        // BDD落とし穴 "列挙してから消すだけ": a file deleted between the
        // listing and the sweep is not an error and is not reported removed.
        let area = Area::new("vanished");
        let gone = area.file("plan-gone.md", "gone", 100);
        let stale = Entry {
            bytes: 99,
            ..gone.clone()
        };
        std::fs::remove_file(&gone.path).unwrap();
        let kept = area.file("plan-kept.md", "kept", 200);

        let report = sweep_area(area.path(), vec![stale], 0).unwrap();

        assert!(
            report.removed.is_empty(),
            "a vanished file is not a removal"
        );
        assert!(
            report.skipped.is_empty(),
            "a vanished file is ignored, not skipped"
        );
        assert!(kept.path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn sweep_skips_a_symlink_that_escapes_and_leaves_its_target() {
        // BDD "対象外は触らない": the link resolves outside, so the sweep
        // skips the link itself and never touches the target.
        use std::os::unix::fs::symlink;
        let area = Area::new("escape-sweep");
        let outside = std::env::temp_dir().join(format!(
            "saniwalens-cleanup-outside-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        let target = outside.join("secret.md");
        std::fs::write(&target, "secret").unwrap();
        let link = area.path().join("plan-link.md");
        symlink(&target, &link).unwrap();
        let victim_old = area.file("plan-old.md", "old", 100);
        let victim_new = area.file("plan-new.md", "new", 300);
        let link_entry = Entry {
            path: link.clone(),
            bytes: 6,
            modified: SystemTime::UNIX_EPOCH,
        };

        let report = sweep_area(
            area.path(),
            vec![link_entry, victim_old.clone(), victim_new.clone()],
            1,
        )
        .unwrap();

        assert_eq!(report.kept, 1);
        assert_eq!(report.removed, vec![victim_old]);
        assert_eq!(report.skipped, vec![link.clone()]);
        assert!(link.exists(), "the escaping link itself stays");
        assert!(victim_new.path.exists(), "the kept generation stays");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "secret");
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn collect_lists_children_with_sizes_and_skips_a_missing_dir() {
        let area = Area::new("collect");
        area.file("plan-a.md", "12345", 100);
        assert!(collect_entries(&area.path().join("no-such-dir")).is_empty());

        let mut listed = collect_entries(area.path());
        listed.sort_by(|left, right| left.path.cmp(&right.path));
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].bytes, 5);
        assert!(listed[0].path.ends_with("plan-a.md"));
    }

    #[test]
    fn collect_sums_a_generation_dir_recursively() {
        // I-1: a backup generation lists its contents' total, not the dir
        // entry's own bytes.
        let area = Area::new("collect-recursive");
        let generation = area.path().join("2026-01-01T00-00-00");
        std::fs::create_dir_all(&generation).unwrap();
        std::fs::write(generation.join("a.md"), "12345").unwrap();
        std::fs::create_dir_all(generation.join("nested")).unwrap();
        std::fs::write(generation.join("nested").join("b.md"), "1234567").unwrap();

        let mut listed = collect_entries(area.path());
        assert_eq!(listed.len(), 1);
        assert_eq!(listed.pop().unwrap().bytes, 5 + 7);
    }

    #[cfg(unix)]
    #[test]
    fn collect_never_follows_a_link_for_sizes() {
        // A link inside a generation counts as a link, never its target.
        use std::os::unix::fs::symlink;
        let area = Area::new("collect-link");
        let generation = area.path().join("2026-01-01T00-00-00");
        std::fs::create_dir_all(&generation).unwrap();
        std::fs::write(generation.join("real.md"), "12345").unwrap();
        let target = area.path().join("big.md");
        std::fs::write(&target, "x".repeat(4096)).unwrap();
        symlink(&target, generation.join("link.md")).unwrap();

        let listed = collect_entries(area.path());
        let generation_entry = listed
            .iter()
            .find(|entry| entry.path == generation)
            .expect("the generation lists");
        assert!(
            generation_entry.bytes < 4096,
            "the link pulled its target in: {}",
            generation_entry.bytes
        );
    }

    #[cfg(unix)]
    #[test]
    fn sweep_removes_a_dangling_link_by_its_own_seat() {
        // M-2: a link whose target is gone resolves nowhere but still sits
        // inside the area, so the sweep removes the link itself.
        use std::os::unix::fs::symlink;
        let area = Area::new("dangling-sweep");
        let target = area.path().join("target-gone.md");
        let link = area.path().join("plan-link.md");
        symlink(&target, &link).unwrap();
        assert!(!target.exists(), "the fixture is dangling");
        assert!(is_within(area.path(), &link));

        let victim = Entry {
            path: link.clone(),
            bytes: 1,
            modified: SystemTime::UNIX_EPOCH,
        };
        let report = sweep_area(area.path(), vec![victim.clone()], 0).unwrap();

        assert_eq!(report.removed, vec![victim]);
        assert!(report.skipped.is_empty());
        assert!(
            fs::symlink_metadata(&link).is_err(),
            "the dangling link itself is gone"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_link_counts_as_within_by_its_parent() {
        use std::os::unix::fs::symlink;
        let area = Area::new("dangling-within");
        let link = area.path().join("plan-link.md");
        symlink(area.path().join("no-such-target.md"), &link).unwrap();
        assert!(is_within(area.path(), &link));
    }
}
