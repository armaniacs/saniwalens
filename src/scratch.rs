//! The unit-test copy of the scratch-dir guard.
//!
//! `cfg(test)` items are not visible to integration binaries, so the guard
//! exists twice: here for tests inside the crate, and in `tests/common/mod.rs`
//! for the integration test binaries. The two copies cannot be one without a
//! dev-dependency, which this crate does not take. Keep them in step: same
//! path shape, `remove_dir_all` (NotFound only) then `create_dir` (never
//! `create_dir_all`), one stderr line on a failed removal, no panic in `Drop`,
//! `SANIWALENS_KEEP_SCRATCH=1` keeps the dir. `.claude/rules/tdd.md`
//! (Scratch dirs) states the rule; `tests/scratch_dir_contract.rs` pins the
//! other copy and the tests below pin this one.

use std::ops::Deref;
use std::path::{Path, PathBuf};

/// The environment variable that keeps a scratch dir for post-mortem
/// inspection instead of removing it when its scope ends.
const KEEP_SCRATCH: &str = "SANIWALENS_KEEP_SCRATCH";

/// The path a scratch dir takes: prefix, process id, name.
pub(crate) fn scratch_path(prefix: &str, name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}-{name}", std::process::id()))
}

fn keep_scratch() -> bool {
    std::env::var_os(KEEP_SCRATCH).as_deref() == Some(std::ffi::OsStr::new("1"))
}

/// A scratch directory owned for the length of a scope: synthetic only, in
/// the system temp dir, removed by `Drop` on normal exit and on unwind.
#[derive(Debug)]
pub(crate) struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// Create a fresh, empty scratch dir, removing a leftover dir of the same
    /// path first (NotFound only). `create_dir`, not `create_dir_all`: an
    /// existing dir is an error, never silently accepted.
    pub(crate) fn new(prefix: &str, name: &str) -> Self {
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

#[cfg(test)]
mod tests {
    use super::{ScratchDir, scratch_path};

    const PREFIX: &str = "saniwalens-unit-scratch";

    #[test]
    fn a_scratch_dir_is_removed_when_its_scope_ends() {
        let path = {
            let dir = ScratchDir::new(PREFIX, "normal-exit");
            assert!(dir.is_dir(), "the dir exists while its guard lives");
            dir.to_path_buf()
        };

        assert!(!path.exists(), "the dir survived its scope: {path:?}");
    }

    #[test]
    fn a_panicking_scope_still_removes_its_scratch_dir() {
        let mut path: Option<std::path::PathBuf> = None;
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let dir = ScratchDir::new(PREFIX, "panic");
            path = Some(dir.to_path_buf());
            panic!("the test body panics after making its scratch dir");
        }));

        assert!(panicked.is_err(), "the closure panicked");
        let path = path.expect("the guard ran before the panic");
        assert!(!path.exists(), "the dir survived the unwind: {path:?}");
    }

    #[test]
    fn a_leftover_scratch_dir_is_replaced_not_reused() {
        let stale = scratch_path(PREFIX, "stale");
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("store.db"), b"STALE ROWS").unwrap();

        let dir = ScratchDir::new(PREFIX, "stale");

        assert!(
            !dir.join("store.db").exists(),
            "the stale store.db was reused: {:?}",
            dir.join("store.db")
        );
        assert_eq!(
            std::fs::read_dir(&*dir).unwrap().count(),
            0,
            "the dir is not empty after creation: {dir:?}"
        );
    }
}
