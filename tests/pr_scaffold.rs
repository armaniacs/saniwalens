//! PR doc scaffold, end to end: `scripts/pr_scaffold.py` writes the `.en.md`
//! / `.ja.md` pair once, refuses to overwrite, and rejects bad slugs before
//! touching the tree. Runs the script in a scratch repo root (a temp dir
//! with an empty `docs/pr/`), so the real `docs/pr/` is never at risk.

use std::path::PathBuf;
use std::process::Command;

mod common;
use common::ScratchDir;

fn scratch_root(name: &str) -> ScratchDir {
    let dir = ScratchDir::new("saniwalens-pr-scaffold", name);
    std::fs::create_dir_all(dir.join("docs").join("pr")).unwrap();
    dir
}

fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/pr_scaffold.py")
}

fn run(root: &ScratchDir, args: &[&str]) -> std::process::Output {
    Command::new("python3")
        .arg(script())
        .args(args)
        .current_dir(&**root)
        .output()
        .expect("the scaffold runs")
}

#[test]
fn a_new_slug_writes_both_files() {
    let root = scratch_root("new-slug");
    let out = run(&root, &["my-feature"]);
    assert!(out.status.success(), "scaffold exits 0: {out:?}");
    for name in ["my-feature.en.md", "my-feature.ja.md"] {
        let text = std::fs::read_to_string(root.join("docs/pr").join(name)).unwrap();
        assert!(text.contains("# my-feature"), "{name} names the slug");
    }
}

#[test]
fn an_existing_slug_writes_nothing_and_fails() {
    let root = scratch_root("existing-slug");
    std::fs::write(root.join("docs/pr/exists.en.md"), "keep me\n").unwrap();
    let before: Vec<PathBuf> = std::fs::read_dir(root.join("docs/pr"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    let out = run(&root, &["exists"]);
    assert!(!out.status.success(), "an existing slug must fail");
    // Only the pre-existing file remains, untouched; no `.ja.md` was added.
    let after: Vec<PathBuf> = std::fs::read_dir(root.join("docs/pr"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(before, after, "nothing was written");
    assert_eq!(
        std::fs::read_to_string(root.join("docs/pr/exists.en.md")).unwrap(),
        "keep me\n"
    );
}

#[test]
fn bad_slugs_are_rejected_before_any_write() {
    let root = scratch_root("bad-slugs");
    for slug in ["", "   ", "../escape", "a/b", ".hidden", "-lead"] {
        let out = run(&root, &[slug]);
        assert!(!out.status.success(), "{slug:?} must fail");
    }
    // Spaces normalize to dashes instead of failing.
    let out = run(&root, &["my feature"]);
    assert!(out.status.success(), "spaces normalize: {out:?}");
    assert!(root.join("docs/pr/my-feature.en.md").exists());
    // Only the normalized pair was written for the good slug.
    let mut names: Vec<String> = std::fs::read_dir(root.join("docs/pr"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["my-feature.en.md", "my-feature.ja.md"]);
}

#[test]
fn generated_pair_is_charset_clean() {
    // The scaffold must never inject out-of-range characters itself: every
    // generated file passes the docs-charset gate.
    let root = scratch_root("charset");
    let out = run(&root, &["charset-check", "Some Title"]);
    assert!(out.status.success(), "scaffold exits 0: {out:?}");
    for name in ["charset-check.en.md", "charset-check.ja.md"] {
        let bytes = std::fs::read(root.join("docs/pr").join(name)).unwrap();
        let text = String::from_utf8(bytes).expect("generated files are UTF-8");
        assert!(!text.contains('\u{2014}'), "{name} must not contain U+2014");
    }
}
