//! Helpers shared by the integration test binaries.
//!
//! Each test binary includes this module with `mod common;`; a helper one
//! binary does not use is still compiled, so unused items are allowed here.
#![allow(dead_code)]

use std::ops::Deref;
use std::path::{Path, PathBuf};

/// The environment variable that keeps a scratch dir for post-mortem
/// inspection instead of removing it when its scope ends.
const KEEP_SCRATCH: &str = "SANIWALENS_KEEP_SCRATCH";

/// The path a scratch dir takes: prefix, process id, name. Unique per test
/// in a run, stable across runs, so a dir left behind is found again.
pub fn scratch_path(prefix: &str, name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}-{name}", std::process::id()))
}

fn keep_scratch() -> bool {
    std::env::var_os(KEEP_SCRATCH).as_deref() == Some(std::ffi::OsStr::new("1"))
}

/// A scratch directory owned for the length of a scope: synthetic only, in
/// the system temp dir, removed by `Drop` on normal exit and on unwind.
///
/// This guard is duplicated in `src/scratch.rs` for unit tests — `cfg(test)`
/// items are not visible to integration binaries, so the two copies cannot be
/// one. Keep the contracts in step: same path shape, `remove_dir_all`
/// (NotFound only) then `create_dir` (never `create_dir_all`), one stderr line
/// on a failed removal, no panic in `Drop`, `SANIWALENS_KEEP_SCRATCH=1` keeps
/// the dir. `.claude/rules/tdd.md` (Scratch dirs) states the rule.
#[derive(Debug)]
pub struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// Create a fresh, empty scratch dir. An existing dir of the same path is
    /// removed first so a dir left by an earlier run is never reused; only
    /// `NotFound` is accepted from that removal. The dir is then made with
    /// `create_dir`, so a leftover dir that survives is an error, not a dir
    /// silently accepted.
    pub fn new(prefix: &str, name: &str) -> Self {
        let path = scratch_path(prefix, name);
        match std::fs::remove_dir_all(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => panic!(
                "cannot clear the stale scratch dir {}: {err}",
                path.display()
            ),
        }
        std::fs::create_dir(&path).unwrap_or_else(|err| {
            panic!("cannot create the scratch dir {}: {err}", path.display())
        });
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
        // aborts the process. A failed removal is reported and dropped.
        if keep_scratch() {
            return;
        }
        if let Err(err) = std::fs::remove_dir_all(&self.path) {
            eprintln!(
                "saniwalens: leaving scratch dir {} behind: {err}",
                self.path.display()
            );
        }
    }
}
