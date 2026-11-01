//! Helpers shared by the integration test binaries.
//!
//! Each test binary includes this module with `mod common;`; a helper one
//! binary does not use is still compiled, so unused items are allowed here.
#![allow(dead_code)]

use std::collections::HashSet;
use std::io::Write;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The environment variable that keeps a scratch dir for post-mortem
/// inspection instead of removing it when its scope ends.
const KEEP_SCRATCH: &str = "SANIWALENS_KEEP_SCRATCH";

/// The path a scratch dir takes: prefix, process id, name. The pid makes the
/// path unique per test run, so a leftover from a crashed run (another pid) is
/// not reused; `new` clears only the same path.
pub fn scratch_path(prefix: &str, name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}-{name}", std::process::id()))
}

fn keep_scratch() -> bool {
    std::env::var_os(KEEP_SCRATCH).as_deref() == Some(std::ffi::OsStr::new("1"))
}

/// The paths this process currently owns. A second live guard at the same
/// path would remove the first guard's dir, so `new` refuses it loudly.
fn live() -> &'static Mutex<HashSet<PathBuf>> {
    static LIVE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

fn claim(path: &Path) {
    let mut live = live()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !live.insert(path.to_path_buf()) {
        panic!(
            "the scratch dir {} is already live in this process",
            path.display()
        );
    }
}

fn unclaim(path: &Path) {
    let mut live = live()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    live.remove(path);
}

/// Report a failed removal on `sink` (the caller passes stderr), ignoring a
/// write error. `Drop` must not panic, so the report is best-effort.
pub fn report_removal_failure(mut sink: impl Write, path: &Path, err: &std::io::Error) {
    let _ = writeln!(
        sink,
        "saniwalens: leaving scratch dir {} behind: {err}",
        path.display()
    );
}

/// A scratch directory owned for the length of a scope: synthetic only, in
/// the system temp dir, removed by `Drop` on normal exit and on unwind.
///
/// This guard is duplicated in `src/scratch.rs` for unit tests — `cfg(test)`
/// items are not visible to integration binaries, so the two copies cannot be
/// one. Keep the contracts in step: same path shape, a live-path registry,
/// `remove_dir_all` (NotFound only) then `create_dir` (never `create_dir_all`),
/// one ignored stderr write on a failed removal, no panic in `Drop`,
/// `SANIWALENS_KEEP_SCRATCH=1` keeps the dir. `.claude/rules/tdd.md`
/// (Scratch dirs) states the rule.
#[derive(Debug)]
pub struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// Create a fresh, empty scratch dir. The path is claimed for this process
    /// first: a second live guard at the same path panics instead of removing
    /// the first guard's dir. A leftover at the path is removed (only
    /// `NotFound` is accepted), then the dir is made with `create_dir`, so a
    /// leftover that survives is an error, not a dir silently accepted.
    pub fn new(prefix: &str, name: &str) -> Self {
        let path = scratch_path(prefix, name);
        claim(&path);
        match std::fs::remove_dir_all(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                unclaim(&path);
                panic!(
                    "cannot clear the stale scratch dir {}: {err}",
                    path.display()
                );
            }
        }
        if let Err(err) = std::fs::create_dir(&path) {
            unclaim(&path);
            panic!("cannot create the scratch dir {}: {err}", path.display());
        }
        Self { path }
    }
}

impl Deref for ScratchDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        // No panic here: this runs during unwinds, where a second panic
        // aborts the process. A failed removal is reported and dropped; a dir
        // that is already gone is not an error.
        unclaim(&self.path);
        if keep_scratch() {
            return;
        }
        if let Err(err) = std::fs::remove_dir_all(&self.path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            report_removal_failure(std::io::stderr().lock(), &self.path, &err);
        }
    }
}
