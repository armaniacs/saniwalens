//! The scratch-dir guard's lifetime contract: a test's temp dir is gone when
//! its scope ends — normally or by unwind — and a dir left by an earlier run
//! is replaced, never reused. The guard exists twice (here for integration
//! binaries, `src/scratch.rs` for unit tests); this file pins the shared copy
//! and `src/scratch.rs` pins its own.

mod common;

use common::{ScratchDir, scratch_path};

const PREFIX: &str = "saniwalens-scratch-contract";

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
    // An earlier run died before its guard could run: the dir still exists
    // and carries the store it wrote. The next run must start from an empty
    // dir, not read those rows back.
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
    // The shape of the trend_markdown shadow: two dirs, the second bound to
    // the same name. Both guards own their dirs, so both are removed.
    let (first, second) = {
        let two_sided = ScratchDir::new(PREFIX, "shadow-two-sided");
        let one_sided = ScratchDir::new(PREFIX, "shadow-one-sided");
        (two_sided.to_path_buf(), one_sided.to_path_buf())
    };

    assert!(!first.exists(), "the first dir leaked: {first:?}");
    assert!(!second.exists(), "the second dir leaked: {second:?}");
}
