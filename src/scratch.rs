//! The unit-test copy of the scratch-dir guard.
//!
//! `cfg(test)` items are not visible to integration binaries, so the guard
//! exists twice: here for tests inside the crate, and in `tests/common/mod.rs`
//! for the integration test binaries. The two copies cannot be one without a
//! dev-dependency, which this crate does not take. Keep them in step: same
//! path shape, a live-path registry, `remove_dir_all` (NotFound only) then
//! `create_dir` (never `create_dir_all`), one ignored stderr write on a failed
//! removal, no panic in `Drop`, `SANIWALENS_KEEP_SCRATCH=1` keeps the dir.
//! `.claude/rules/tdd.md` (Scratch dirs) states the rule;
//! `tests/scratch_dir_contract.rs` pins the other copy and the tests below pin
//! this one.

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
pub(crate) fn scratch_path(prefix: &str, name: &str) -> PathBuf {
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
fn report_removal_failure(mut sink: impl Write, path: &Path, err: &std::io::Error) {
    let _ = writeln!(
        sink,
        "saniwalens: leaving scratch dir {} behind: {err}",
        path.display()
    );
}

/// A scratch directory owned for the length of a scope: synthetic only, in
/// the system temp dir, removed by `Drop` on normal exit and on unwind.
#[derive(Debug)]
pub(crate) struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// Create a fresh, empty scratch dir. The path is claimed for this process
    /// first: a second live guard at the same path panics instead of removing
    /// the first guard's dir. A leftover at the path is removed (only
    /// `NotFound` is accepted), then the dir is made with `create_dir`, so a
    /// leftover that survives is an error, not a dir silently accepted.
    pub(crate) fn new(prefix: &str, name: &str) -> Self {
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
        if keep_scratch() {
            unclaim(&self.path);
            return;
        }
        if let Err(err) = std::fs::remove_dir_all(&self.path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            report_removal_failure(std::io::stderr().lock(), &self.path, &err);
        }
        // Release the claim only after the removal: a same-path `new` racing
        // this Drop must still see the path as live and panic, instead of
        // claiming it and having this removal delete the new guard's dir.
        unclaim(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::{ScratchDir, report_removal_failure, scratch_path};
    use std::io::Write;

    const PREFIX: &str = "saniwalens-unit-scratch";

    /// A writer whose every write fails, standing in for an unwritable stderr.
    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
        }
    }

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

    #[test]
    fn a_duplicate_live_scratch_dir_is_rejected() {
        let first = ScratchDir::new(PREFIX, "duplicate");
        let first_path = first.to_path_buf();

        let panicked = std::panic::catch_unwind(|| {
            let _second = ScratchDir::new(PREFIX, "duplicate");
        });

        assert!(panicked.is_err(), "a duplicate live guard must panic");
        assert!(
            first.is_dir(),
            "the live guard's dir was removed by the rejected duplicate"
        );
        drop(first);
        assert!(!first_path.exists());
    }

    #[test]
    fn a_dropped_name_can_be_reused() {
        let first = ScratchDir::new(PREFIX, "reused");
        let path = first.to_path_buf();
        drop(first);

        let second = ScratchDir::new(PREFIX, "reused");

        assert!(second.is_dir());
        assert_eq!(second.to_path_buf(), path);
    }

    #[cfg(unix)]
    #[test]
    fn a_stale_dir_that_cannot_be_removed_panics_instead_of_reusing_its_rows() {
        use std::os::unix::fs::PermissionsExt;

        let stale = scratch_path(PREFIX, "locked-stale");
        let inner = stale.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("store.db"), b"STALE ROWS").unwrap();
        let mut locked = std::fs::metadata(&inner).unwrap().permissions();
        locked.set_mode(0o555);
        std::fs::set_permissions(&inner, locked).unwrap();

        let panicked = std::panic::catch_unwind(|| {
            let _dir = ScratchDir::new(PREFIX, "locked-stale");
        });

        // Restore before asserting so a failure cannot leave the dir locked.
        let mut open = std::fs::metadata(&inner).unwrap().permissions();
        open.set_mode(0o755);
        std::fs::set_permissions(&inner, open).unwrap();

        assert!(panicked.is_err(), "a stale dir that survives must panic");
        assert_eq!(
            std::fs::read(inner.join("store.db")).unwrap(),
            b"STALE ROWS",
            "the stale rows were reused"
        );

        // The failed attempt released its claim: the same name works once the
        // stale dir can be cleared.
        let dir = ScratchDir::new(PREFIX, "locked-stale");
        assert_eq!(std::fs::read_dir(&*dir).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_a_file_is_rejected() {
        let path = scratch_path(PREFIX, "file-path");
        std::fs::write(&path, b"not a dir").unwrap();

        let panicked = std::panic::catch_unwind(|| {
            let _dir = ScratchDir::new(PREFIX, "file-path");
        });

        assert!(panicked.is_err(), "a file at the path must panic");
        assert_eq!(std::fs::read(&path).unwrap(), b"not a dir");
        std::fs::remove_file(&path).unwrap();

        let dir = ScratchDir::new(PREFIX, "file-path");
        assert!(dir.is_dir());
    }

    #[test]
    fn a_guard_whose_dir_vanished_drops_without_panic() {
        let dir = ScratchDir::new(PREFIX, "vanished");
        let path = dir.to_path_buf();
        std::fs::remove_dir_all(&path).unwrap();

        // NotFound is not an error to report: the Drop must stay silent and
        // still release its claim, so the same name is usable again.
        drop(dir);

        let again = ScratchDir::new(PREFIX, "vanished");
        assert!(again.is_dir());
    }

    #[test]
    fn a_failed_removal_report_does_not_panic_when_stderr_is_unwritable() {
        let path = scratch_path(PREFIX, "report");
        let err = std::io::Error::from(std::io::ErrorKind::PermissionDenied);

        // A panicking report would abort a Drop during unwind; this pins the
        // write error being dropped instead.
        report_removal_failure(FailingWriter, &path, &err);
    }
}
