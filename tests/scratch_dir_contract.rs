//! The scratch-dir guard's lifetime contract: a test's temp dir is gone when
//! its scope ends — normally or by unwind — a leftover at the same path is
//! replaced rather than reused, and a second live guard for that path is
//! refused. The guard exists twice (here for integration binaries,
//! `src/scratch.rs` for unit tests); this file pins the shared copy and
//! `src/scratch.rs` pins its own.

mod common;

use common::{ScratchDir, report_removal_failure, scratch_path};
use std::io::Write;

const PREFIX: &str = "saniwalens-scratch-contract";

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
        std::fs::write(dir.join("store.db"), b"rows").unwrap();
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
    // A guard never ran, so the path still exists and carries the store it
    // wrote: a panic before Drop, or a crashed run whose pid was handed back.
    // The next guard at that path must start from an empty dir, not read those
    // rows back.
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
fn two_scratch_dirs_in_one_scope_are_both_removed() {
    // Two guards with distinct names in one scope, the shape trend_markdown
    // uses for its two fixtures: each guard owns its dir, so both are removed.
    let (first, second) = {
        let two_sided = ScratchDir::new(PREFIX, "shadow-two-sided");
        let one_sided = ScratchDir::new(PREFIX, "shadow-one-sided");
        (two_sided.to_path_buf(), one_sided.to_path_buf())
    };

    assert!(!first.exists(), "the first dir leaked: {first:?}");
    assert!(!second.exists(), "the second dir leaked: {second:?}");
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
    assert!(!first_path.exists(), "the first dir leaked: {first_path:?}");
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

    // A plain `remove_dir_all` that ignores its error plus `create_dir_all`
    // would accept this dir and reuse the rows. The read-only inner dir makes
    // the removal fail, so `new` must panic and leave the rows untouched.
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

    // Restore before asserting so a failed assertion cannot leave a locked dir.
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
    // `remove_dir_all` on a file is an error that is not NotFound: `new` must
    // panic rather than call `create_dir_all` over it.
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

    // NotFound is not an error to report: the Drop must stay silent and still
    // release its claim, so the same name is usable again.
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

const UNIT_GUARD: &str = "src/scratch.rs";
const INTEGRATION_GUARD: &str = "tests/common/mod.rs";

/// Extract the `impl Drop for ScratchDir { ... }` block from a source file.
/// A missing or duplicated block is a broken fixture, not an empty contract.
fn drop_body(path: &str) -> String {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    let marker = "impl Drop for ScratchDir {";
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("{path} has no `{marker}`"));
    let after = &source[start..];
    assert_eq!(
        after.matches(marker).count(),
        1,
        "{path} has more than one Drop impl"
    );

    let mut depth = 0usize;
    for (index, ch) in after.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return after[..=index].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("{path}'s Drop impl has unbalanced braces");
}

/// Compare the two copies without comments, blank lines, or indentation: the
/// bodies must be the same statements, not merely look similar.
fn normalized(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The order contract: `keep_scratch` returns before any removal, and the final
/// `unclaim` comes only after `remove_dir_all`.
fn assert_drop_order(body: &str) {
    let remove = body
        .find("remove_dir_all")
        .expect("the Drop removes the dir");
    let last_unclaim = body.rfind("unclaim(").expect("the Drop releases the claim");
    assert!(
        remove < last_unclaim,
        "the Drop unclaims before removing: {body}"
    );
    let keep = body
        .find("keep_scratch()")
        .expect("the Drop checks keep_scratch");
    assert!(
        keep < remove,
        "keep_scratch must return before the removal: {body}"
    );
}

#[test]
fn the_two_copies_share_the_drop_contract() {
    let unit = drop_body(UNIT_GUARD);
    let integration = drop_body(INTEGRATION_GUARD);
    assert_eq!(
        normalized(&unit),
        normalized(&integration),
        "the two ScratchDir Drop copies drifted; keep them in step"
    );
    assert_drop_order(&unit);
    assert_drop_order(&integration);
}

const RIGHT_ORDER: &str = "impl Drop for ScratchDir {
    fn drop(&mut self) {
        if keep_scratch() {
            return;
        }
        let _ = std::fs::remove_dir_all(&self.path);
        unclaim(&self.path);
    }
}";

const WRONG_ORDER: &str = "impl Drop for ScratchDir {
    fn drop(&mut self) {
        if keep_scratch() {
            return;
        }
        unclaim(&self.path);
        let _ = std::fs::remove_dir_all(&self.path);
    }
}";

#[test]
#[should_panic(expected = "the two ScratchDir Drop copies drifted")]
fn a_single_changed_copy_is_rejected() {
    let dir = ScratchDir::new(PREFIX, "drop-contract-mismatch");
    let left = dir.join("left.rs");
    let right = dir.join("right.rs");
    std::fs::write(&left, RIGHT_ORDER).unwrap();
    std::fs::write(&right, WRONG_ORDER).unwrap();

    let left = drop_body(left.to_str().unwrap());
    let right = drop_body(right.to_str().unwrap());
    assert_eq!(
        normalized(&left),
        normalized(&right),
        "the two ScratchDir Drop copies drifted; keep them in step"
    );
}

#[test]
#[should_panic(expected = "the Drop unclaims before removing")]
fn both_copies_with_the_wrong_order_are_rejected() {
    let dir = ScratchDir::new(PREFIX, "drop-contract-wrong-order");
    let path = dir.join("guard.rs");
    std::fs::write(&path, WRONG_ORDER).unwrap();
    let body = drop_body(path.to_str().unwrap());
    assert_drop_order(&body);
}
