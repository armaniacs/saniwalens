//! One invocation's opened and guarded store: the `Session` the resolution
//! builds and guards. The resolution itself — environment roots, locale, and
//! flags/config into a harness and a store path — lives in `session_env`;
//! the presentation primitives the views share live in `cli::report`.
//!
//! `run()` arms used to repeat this wiring eleven times over: every command
//! re-derived the same triple through free functions scattered across
//! `cli/mod.rs`. The last three real defects lived in that wiring, not in parsing
//! or rendering, so it gets one module with one seam. See
//! `docs/specs/cli.md`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::adapter::harness::{AnalyzeStats, Harness};
use crate::cli::report::{Style, eprint_freshness, freshness_line};
use crate::core::optimize::AdvisorLanguage;
use crate::session_env::{LanguageSource, SessionEnv, state_base};
use crate::store::{OpenMode, Store};

/// One invocation's resolved world: which harness, which store file, and the
/// opened, harness-guarded store. Every command builds exactly this and
/// nothing else before doing its own work.
///
/// `language` is the advisor's output language (resolved flags > file >
/// builtin, English by default). It lives on the session so the `optimize`
/// family reads it without another parameter — including the guided TUI's
/// inline next step, which builds its session the same way and therefore
/// renders the same language without naming the flag. `language_source`
/// names the layer that decided it, for the `--verbose`-only decision line
/// (`language_decision_line`); sessions built without a resolution (the
/// guide's inline run always passes `verbose = false`, so the line never
/// renders there) keep the default.
pub struct Session {
    pub harness: Harness,
    pub db_path: PathBuf,
    pub store: Store,
    pub language: AdvisorLanguage,
    pub language_source: LanguageSource,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The store holds an open SQLite connection, which is deliberately
        // left out: Debug is for the tests that unwrap through Session.
        f.debug_struct("Session")
            .field("harness", &self.harness)
            .field("db_path", &self.db_path)
            .finish()
    }
}

impl Session {
    /// Resolve the path, move a legacy store into place on a pure default,
    /// open, and guard. The analyze arms run analysis themselves afterwards
    /// (which guards again); the read arms go through [`Session::open_read`].
    pub fn open(
        harness: &Harness,
        db: Option<PathBuf>,
        config_db: Option<&Path>,
        env: &SessionEnv,
    ) -> Result<Self> {
        let path = Self::resolve_path(harness, db, config_db, env)?;
        Self::open_resolved(harness, path)
    }

    /// Open and guard an already-resolved path. Split from [`Session::open`]
    /// so a caller can resolve the path, parse its own flags, and only then
    /// open — the exact evaluation order `run()`'s optimize arm had before the
    /// Session seam (`docs/specs/cli.md`).
    pub(crate) fn open_resolved(harness: &Harness, path: PathBuf) -> Result<Self> {
        let store = Store::open(&path, OpenMode::ReadWrite).context("open store")?;
        guard_harness(&store, harness, &path)?;
        Ok(Session {
            harness: *harness,
            db_path: path,
            store,
            language: AdvisorLanguage::default(),
            language_source: LanguageSource::Default,
        })
    }

    /// The read path for the views: open, then analyze unless frozen — exactly
    /// what `open_for_read` did, messages included. `analyze` runs the refresh
    /// analysis; tests pass a stub.
    pub fn open_read(
        harness: &Harness,
        db: Option<PathBuf>,
        config_db: Option<&Path>,
        frozen: bool,
        env: &SessionEnv,
        analyze: impl FnOnce(&Harness, Option<PathBuf>, &Path) -> Result<AnalyzeStats>,
    ) -> Result<Self> {
        if frozen {
            // Frozen means no mutation at all: resolve without the legacy
            // move, open read-only (no create, no chmod, no migrate), guard,
            // and print the same freshness line as the analyzing path.
            let path = Self::resolve_frozen_path(harness, db, config_db, env)?;
            let sess = Self::open_frozen(harness, path)?;
            eprint_freshness(&freshness_line(sess.store.meta("analyzed_at")?.as_deref()));
            return Ok(sess);
        }
        let mut sess = Self::open(harness, db, config_db, env)?;
        sess.refresh(analyze)?;
        Ok(sess)
    }

    /// Resolve without writing anything: no legacy-store move — frozen must
    /// not touch the store before the open either. Pure over its inputs; the
    /// default resolution with the move is [`Session::resolve_path`].
    pub(crate) fn resolve_frozen_path(
        harness: &Harness,
        db: Option<PathBuf>,
        config_db: Option<&Path>,
        env: &SessionEnv,
    ) -> Result<PathBuf> {
        store_path(
            db.or_else(|| config_db.map(Path::to_path_buf)),
            harness,
            env.state_home,
            env.home.map(Path::new),
        )
    }

    /// Open an already-resolved path without writing anything: read-only (no
    /// create, no chmod, no migrate) and guarded, so an absent store is an
    /// error — frozen never creates one. The frozen counterpart of
    /// [`Session::open_resolved`].
    pub(crate) fn open_frozen(harness: &Harness, path: PathBuf) -> Result<Self> {
        let store = Store::open(&path, OpenMode::ReadOnly)
            .with_context(|| format!("open store {} (run `analyze` first?)", path.display()))?;
        guard_harness(&store, harness, &path)?;
        Ok(Session {
            harness: *harness,
            db_path: path,
            store,
            language: AdvisorLanguage::default(),
            language_source: LanguageSource::Default,
        })
    }

    /// The optimize family's resolution (`optimize`, `--apply`, the guide's
    /// inline next step): frozen skips the legacy-store move — nothing may be
    /// touched before the open either — the default keeps it. Split from the
    /// open so a caller parses its own flags in between, keeping the failure
    /// order the pre-Session dispatch had.
    pub(crate) fn optimize_path(
        harness: &Harness,
        db: Option<PathBuf>,
        config_db: Option<&Path>,
        frozen: bool,
        env: &SessionEnv,
    ) -> Result<PathBuf> {
        if frozen {
            Self::resolve_frozen_path(harness, db, config_db, env)
        } else {
            Self::resolve_path(harness, db, config_db, env)
        }
    }

    /// The optimize family's open on an already-resolved path: frozen opens
    /// read-only (no create, no chmod, no migrate; an absent store is an
    /// error), the default opens read-write exactly as before.
    pub(crate) fn open_optimize(harness: &Harness, path: PathBuf, frozen: bool) -> Result<Self> {
        if frozen {
            Self::open_frozen(harness, path)
        } else {
            Self::open_resolved(harness, path)
        }
    }

    /// The read-only path for `sql`: resolve and migrate like everything
    /// else, but open read-only and never refresh. The freshness line stays
    /// with the caller, which prints it only when the store carries one.
    pub fn open_readonly(
        harness: &Harness,
        db: Option<PathBuf>,
        config_db: Option<&Path>,
        env: &SessionEnv,
    ) -> Result<Self> {
        let pure_default = db.is_none() && config_db.is_none();
        let explicit = db.or_else(|| config_db.map(Path::to_path_buf));
        let path = store_path(explicit, harness, env.state_home, env.home.map(Path::new))?;
        if pure_default {
            migrate_legacy_store(&path);
        }
        let store = Store::open(&path, OpenMode::ReadOnly)
            .with_context(|| format!("open store {} (run `analyze` first?)", path.display()))?;
        guard_harness(&store, harness, &path)?;
        Ok(Session {
            harness: *harness,
            db_path: path,
            store,
            language: AdvisorLanguage::default(),
            language_source: LanguageSource::Default,
        })
    }

    /// Re-analyze against the roots the store was built from (recorded in
    /// meta); a fresh db falls back to the defaults. The projects lookup
    /// reads the live connection — the analyze step writes through its own
    /// connection, so only the re-open after it stays: reopening to read one
    /// meta key was the second of the two re-opens (PBI C8).
    fn refresh(
        &mut self,
        analyze: impl FnOnce(&Harness, Option<PathBuf>, &Path) -> Result<AnalyzeStats>,
    ) -> Result<()> {
        let projects = match self.db_path.exists() {
            true => self.store.meta("projects_dir")?.map(PathBuf::from),
            false => None,
        };
        let stats = analyze(&self.harness, projects, &self.db_path)?;
        eprintln!(
            "{}",
            Style::stderr().dim(&refreshed_just_now_line(stats, &self.harness))
        );
        self.store = Store::open(&self.db_path, OpenMode::ReadWrite).context("open store")?;
        Ok(())
    }

    pub(crate) fn resolve_path(
        harness: &Harness,
        db: Option<PathBuf>,
        config_db: Option<&Path>,
        env: &SessionEnv,
    ) -> Result<PathBuf> {
        let pure_default = db.is_none() && config_db.is_none();
        let path = store_path(
            db.or_else(|| config_db.map(Path::to_path_buf)),
            harness,
            env.state_home,
            env.home.map(Path::new),
        )?;
        // The legacy-store move only applies to the pure default resolution:
        // an explicit `--db` or a configured one is the user's own choice.
        if pure_default {
            migrate_legacy_store(&path);
        }
        Ok(path)
    }
}

/// Refuse to open a store another harness built. A store with no `harness`
/// key predates the flag and is adopted (the Claude default), never refused.
pub(crate) fn guard_harness(store: &Store, harness: &Harness, db: &Path) -> Result<()> {
    if let Some(existing) = store.meta("harness")?
        && existing != harness.label()
    {
        anyhow::bail!(
            "the store at {} holds {existing} sessions — rerun with --harness {existing} (or pick another --db)",
            db.display()
        );
    }
    Ok(())
}

/// Resolve `--db`, defaulting to a **user-level** store rather than a
/// cwd-relative file. What gets analyzed is every transcript under
/// `~/.claude/projects` — one cross-project dataset — so a per-directory
/// default would build a separate copy of the same store per directory saniwalens
/// happens to run from, and leave a database inside unrelated Git working
/// trees.
///
/// State, not cache. The store accumulates rows whose source transcripts Claude
/// Code later prunes, at which point it is the only surviving record of those
/// sessions and nothing on disk can rebuild it (`storage.md`). A cache
/// directory is by definition somewhere a cleaner may empty without asking —
/// exactly the one thing this file cannot survive.
///
/// Pure over its inputs (the environment is read by the caller) so the
/// resolution is testable. Each harness resolves to its own file name, so a
/// store built by one harness is never reinterpreted as another's rows
/// (`docs/specs/harness.md`).
fn store_path(
    db: Option<PathBuf>,
    harness: &Harness,
    state_home: Option<&Path>,
    home: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(db) = db {
        return Ok(db);
    }
    Ok(state_base(state_home, home)?
        .join("saniwalens")
        .join(harness.db_file_name()))
}

/// Move a pre-rename store into place. The store outlives the transcripts it
/// was parsed from (`storage.md`), so the rename must never orphan it: the
/// binary has been renamed twice, so the first open under the current name
/// moves whichever legacy directory exists — the intermediate `agentlens` or
/// the original `cclens` — renamed under the new name with its per-harness
/// files, never deleted, never copied over an existing store.
fn migrate_legacy_store(resolved: &Path) {
    const LEGACY_DIRS: [&str; 2] = ["agentlens", "cclens"];
    let Some(new_dir) = resolved.parent() else {
        return;
    };
    let Some(state_base) = new_dir.parent() else {
        return;
    };
    if new_dir.exists() {
        return;
    }
    let Some(legacy_dir) = LEGACY_DIRS
        .iter()
        .map(|name| state_base.join(name))
        .find(|dir| dir.is_dir())
    else {
        return; // nothing to move
    };
    if std::fs::rename(&legacy_dir, new_dir).is_err() {
        return; // a move we cannot make is not ours to force
    }
    // The per-harness files carry an old name too.
    let Ok(entries) = std::fs::read_dir(new_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(stem) = name.to_str() else {
            continue;
        };
        let Some(rest) = LEGACY_DIRS
            .iter()
            .find_map(|prefix| stem.strip_prefix(prefix))
        else {
            continue;
        };
        let _ = std::fs::rename(entry.path(), new_dir.join(format!("saniwalens{rest}")));
    }
}

/// The single wording site for the just-refreshed line: `Session::refresh`
/// (the report views' auto-refresh) and the `optimize` / `--apply` prologue
/// both print through it, so the three routes cannot drift apart.
pub(crate) fn refreshed_just_now_line(stats: AnalyzeStats, harness: &Harness) -> String {
    format!(
        "store: refreshed just now ({} {} re-analyzed, {} unchanged)",
        stats.sessions,
        harness.input_unit(),
        stats.skipped
    )
}

/// The freshness line for a store this run never refreshed: the same line the
/// frozen read paths print, tolerating a store whose schema predates `meta`
/// the way `sql` does — a display must not fail the command it decorates.
pub(crate) fn eprint_frozen_freshness(store: &Store) {
    if let Ok(analyzed_at) = store.meta("analyzed_at") {
        eprint_freshness(&freshness_line(analyzed_at.as_deref()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::harness::AnalyzeStats;

    use crate::cli::report::Style;
    use crate::session_env::SessionEnv;
    use crate::store::OpenMode;

    fn example_dir(sub: &str) -> PathBuf {
        std::env::temp_dir().join(format!("saniwalens-session-{sub}-{}", std::process::id()))
    }

    fn store_with_harness(dir: &Path, file: &str, label: Option<&str>) -> PathBuf {
        let path = dir.join(file);
        let mut store = Store::open(&path, OpenMode::ReadWrite).unwrap();
        if let Some(label) = label {
            store.set_meta("harness", label).unwrap();
        }
        path
    }

    fn panic_analyzer() -> impl FnOnce(&Harness, Option<PathBuf>, &Path) -> Result<AnalyzeStats> {
        |_, _, _| panic!("analyzer must not run on this path")
    }

    #[test]
    fn open_resolves_config_db_and_migrates_legacy() {
        // The default resolution moves the pre-rename directory into place,
        // exactly like the old resolve_db did.
        let dir = example_dir("open-migrate");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("cclens")).unwrap();
        std::fs::write(dir.join("cclens/cclens.db"), b"rows").unwrap();
        std::fs::write(dir.join("file.db"), b"").unwrap();
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let from_config = dir.join("file.db");
        let sess = Session::open(&Harness::Claude, None, Some(&from_config), &env).unwrap();
        assert_eq!(sess.db_path, from_config);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_prefers_explicit_db_and_skips_migration() {
        let dir = example_dir("open-explicit");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("cclens")).unwrap();
        std::fs::write(dir.join("cclens/cclens.db"), b"legacy").unwrap();
        let chosen = dir.join("chosen.db");
        std::fs::write(&chosen, b"").unwrap();
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let sess = Session::open(&Harness::Claude, Some(chosen.clone()), None, &env).unwrap();
        assert_eq!(sess.db_path, chosen);
        assert!(dir.join("cclens/cclens.db").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_guards_a_wrong_harness_store() {
        let dir = example_dir("open-guard");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = store_with_harness(&dir, "s.db", Some(Harness::Opencode.label()));
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let err = Session::open(&Harness::Claude, Some(path), None, &env).unwrap_err();
        assert!(format!("{err:#}").contains("--harness opencode"), "{err:#}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_adopts_a_store_without_a_harness_key() {
        let dir = example_dir("open-adopt");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = store_with_harness(&dir, "s.db", None);
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let sess = Session::open(&Harness::Claude, Some(path.clone()), None, &env).unwrap();
        assert_eq!(sess.db_path, path);
        assert_eq!(sess.harness, Harness::Claude);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_read_frozen_never_runs_the_analyzer() {
        let dir = example_dir("open-frozen");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = store_with_harness(&dir, "s.db", Some(Harness::Claude.label()));
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let sess = Session::open_read(
            &Harness::Claude,
            Some(path),
            None,
            true,
            &env,
            panic_analyzer(),
        )
        .unwrap();
        assert_eq!(sess.harness, Harness::Claude);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_read_frozen_leaves_the_store_untouched() {
        // Frozen reads promise no mutation: the store's bytes and mtime must
        // survive a frozen open exactly. A ReadWrite open (chmod, migrate)
        // changes them, so this pins the read-only path.
        let dir = example_dir("open-frozen-untouched");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = store_with_harness(&dir, "s.db", Some(Harness::Claude.label()));
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let before_bytes = std::fs::read(&path).unwrap();
        let before_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let _sess = Session::open_read(
            &Harness::Claude,
            Some(path.clone()),
            None,
            true,
            &env,
            panic_analyzer(),
        )
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before_bytes);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before_mtime
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn open_read_frozen_succeeds_on_a_readonly_store() {
        // A frozen open never creates, chmods, or migrates, so it works where
        // nothing may be written — a read-only filesystem equivalent.
        use std::os::unix::fs::PermissionsExt;
        let dir = example_dir("open-frozen-readonly");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = store_with_harness(&dir, "s.db", Some(Harness::Claude.label()));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let sess = Session::open_read(
            &Harness::Claude,
            Some(path.clone()),
            None,
            true,
            &env,
            panic_analyzer(),
        )
        .unwrap();
        assert_eq!(sess.db_path, path);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_read_refresh_runs_the_analyzer_with_meta_projects() {
        use std::cell::RefCell;
        let dir = example_dir("open-refresh");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = store_with_harness(&dir, "s.db", Some(Harness::Claude.label()));
        {
            let mut store = Store::open(&path, OpenMode::ReadWrite).unwrap();
            store
                .set_meta("projects_dir", "/tmp/example/projects")
                .unwrap();
        }
        let seen: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let stats = AnalyzeStats {
            sessions: 3,
            skipped: 1,
            spans_total: 0,
            surface_count: 0,
            denials: 0,
        };
        let sess = Session::open_read(
            &Harness::Claude,
            Some(path.clone()),
            None,
            false,
            &env,
            |h, projects, db| {
                seen.borrow_mut().push(format!(
                    "{}|{}|{}",
                    h.label(),
                    projects
                        .map(|p| p.display().to_string())
                        .as_deref()
                        .unwrap_or("-"),
                    db.display()
                ));
                Ok(AnalyzeStats { ..stats })
            },
        )
        .unwrap();
        assert_eq!(sess.db_path, path);
        assert_eq!(
            seen.borrow().as_slice(),
            &[format!("claude|/tmp/example/projects|{}", path.display())]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_read_refresh_without_projects_passes_none() {
        use std::cell::RefCell;
        let dir = example_dir("open-refresh-none");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = store_with_harness(&dir, "s.db", Some(Harness::Claude.label()));
        let seen: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let stats = AnalyzeStats {
            sessions: 0,
            skipped: 0,
            spans_total: 0,
            surface_count: 0,
            denials: 0,
        };
        Session::open_read(
            &Harness::Claude,
            Some(path),
            None,
            false,
            &env,
            |h, projects, _| {
                seen.borrow_mut()
                    .push(format!("{}|{}", h.label(), projects.is_some()));
                Ok(stats)
            },
        )
        .unwrap();
        assert_eq!(seen.borrow().as_slice(), &["claude|false".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_read_frozen_wrong_harness_is_refused() {
        let dir = example_dir("open-frozen-wrong");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = store_with_harness(&dir, "s.db", Some(Harness::Opencode.label()));
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let err = Session::open_read(
            &Harness::Claude,
            Some(path),
            None,
            true,
            &env,
            panic_analyzer(),
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("--harness opencode"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_readonly_missing_file_errors_like_before() {
        let dir = example_dir("open-readonly-missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let err = Session::open_readonly(&Harness::Claude, Some(dir.join("nope.db")), None, &env)
            .unwrap_err();
        assert!(format!("{err:#}").contains("run `analyze` first?"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- the optimize family's frozen seam (PBI 2026-11-21-19) ----
    //
    // `--frozen` means no mutation on every path that takes it, so the
    // optimize family's resolve+open seam carries the same tripwires the
    // frozen read views got in PBI 2026-11-08-12: no create, no chmod, no
    // migrate — and an absent store is an error, never a fresh empty one.

    #[test]
    fn optimize_frozen_leaves_the_store_untouched() {
        let dir = example_dir("optimize-frozen-untouched");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = store_with_harness(&dir, "s.db", Some(Harness::Claude.label()));
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let before_bytes = std::fs::read(&path).unwrap();
        let before_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let resolved =
            Session::optimize_path(&Harness::Claude, Some(path.clone()), None, true, &env).unwrap();
        let sess = Session::open_optimize(&Harness::Claude, resolved, true).unwrap();
        assert_eq!(sess.db_path, path);
        assert_eq!(std::fs::read(&path).unwrap(), before_bytes);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            before_mtime
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn optimize_frozen_does_not_chmod_the_store() {
        // A ReadWrite open enforces owner-only on the file; a frozen open
        // must leave the mode exactly as it found it.
        use std::os::unix::fs::PermissionsExt;
        let dir = example_dir("optimize-frozen-mode");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = store_with_harness(&dir, "s.db", Some(Harness::Claude.label()));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let sess = Session::open_optimize(&Harness::Claude, path.clone(), true).unwrap();
        assert_eq!(sess.db_path, path);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn optimize_frozen_on_an_absent_store_creates_nothing() {
        // `optimize --frozen` on a missing store must stop with the
        // absent-store error and create no directory and no file — never a
        // fresh empty store for the advisor to brief from.
        let dir = example_dir("optimize-frozen-absent");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let resolved = Session::optimize_path(&Harness::Claude, None, None, true, &env).unwrap();
        assert_eq!(resolved, dir.join("saniwalens").join("saniwalens.db"));
        let err = Session::open_optimize(&Harness::Claude, resolved, true).unwrap_err();
        assert!(
            format!("{err:#}").contains("run `analyze` first?"),
            "{err:#}"
        );
        assert!(
            !dir.join("saniwalens").exists(),
            "the frozen open created the store directory"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn optimize_frozen_skips_the_legacy_move() {
        // The frozen resolution must avoid the migrate at the resolve stage
        // too: a legacy store stays put, and the open fails on the absent
        // new-name store instead of conjuring one.
        let dir = example_dir("optimize-frozen-legacy");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("cclens")).unwrap();
        std::fs::write(dir.join("cclens/cclens.db"), b"rows").unwrap();
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: None,
        };
        let resolved = Session::optimize_path(&Harness::Claude, None, None, true, &env).unwrap();
        assert_eq!(resolved, dir.join("saniwalens").join("saniwalens.db"));
        assert!(Session::open_optimize(&Harness::Claude, resolved, true).is_err());
        assert_eq!(
            std::fs::read(dir.join("cclens/cclens.db")).unwrap(),
            b"rows"
        );
        assert!(!dir.join("saniwalens").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn optimize_frozen_succeeds_on_a_readonly_store() {
        // No create, no chmod, no migrate, so the frozen open works where
        // nothing may be written — a read-only filesystem equivalent.
        use std::os::unix::fs::PermissionsExt;
        let dir = example_dir("optimize-frozen-readonly");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = store_with_harness(&dir, "s.db", Some(Harness::Claude.label()));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        let sess = Session::open_optimize(&Harness::Claude, path.clone(), true).unwrap();
        assert_eq!(sess.db_path, path);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn optimize_frozen_wrong_harness_is_refused() {
        let dir = example_dir("optimize-frozen-wrong");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = store_with_harness(&dir, "s.db", Some(Harness::Opencode.label()));
        let err = Session::open_optimize(&Harness::Claude, path, true).unwrap_err();
        assert!(format!("{err:#}").contains("--harness opencode"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn frozen_freshness_display_tolerates_a_pre_meta_store() {
        // The optimize family's frozen freshness line chose `sql`'s
        // tolerance: a store whose schema predates `meta` skips the display
        // instead of failing the command after a successful open.
        let dir = example_dir("optimize-frozen-premeta");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE t (x)").unwrap();
        drop(conn);
        let store = Store::open(&path, OpenMode::ReadOnly).unwrap();
        assert!(
            store.meta("analyzed_at").is_err(),
            "the fixture holds no meta table"
        );
        eprint_frozen_freshness(&store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_path_defaults_under_the_user_state_dir() {
        // Temp-dir roots: absolute on every OS, unlike `/tmp/...`.
        let state = std::env::temp_dir().join("saniwalens-fixture-state");
        let home = std::env::temp_dir().join("saniwalens-fixture-home");
        assert_eq!(
            store_path(None, &Harness::Claude, Some(&state), None).unwrap(),
            state.join("saniwalens").join("saniwalens.db")
        );
        assert_eq!(
            store_path(None, &Harness::Claude, None, Some(&home)).unwrap(),
            home.join(".local")
                .join("state")
                .join("saniwalens")
                .join("saniwalens.db")
        );
    }

    #[test]
    fn store_path_gives_each_harness_its_own_file() {
        // One store per harness: an opencode run never lands in the Claude
        // file, so an old store is never reinterpreted as another harness.
        let state = std::env::temp_dir().join("saniwalens-fixture-state");
        assert_eq!(
            store_path(None, &Harness::Opencode, Some(&state), None).unwrap(),
            state.join("saniwalens").join("saniwalens-opencode.db")
        );
    }

    #[test]
    fn store_path_ignores_a_relative_xdg_state_home() {
        // XDG: a non-absolute XDG_STATE_HOME is invalid and must be ignored —
        // honoring it would put the store back under the current directory.
        let home = std::env::temp_dir().join("saniwalens-fixture-home");
        assert_eq!(
            store_path(
                None,
                &Harness::Claude,
                Some(Path::new("relative/state")),
                Some(&home)
            )
            .unwrap(),
            home.join(".local")
                .join("state")
                .join("saniwalens")
                .join("saniwalens.db")
        );
    }

    #[test]
    fn store_path_without_a_home_is_an_error() {
        assert!(store_path(None, &Harness::Claude, None, None).is_err());
    }

    #[test]
    fn store_path_refuses_a_relative_home() {
        // The fallback root is held to the same bar as XDG_STATE_HOME: a
        // relative one would put the store back under the current directory.
        assert!(
            store_path(
                None,
                &Harness::Claude,
                None,
                Some(Path::new("relative/home"))
            )
            .is_err()
        );
    }

    #[test]
    fn store_path_takes_an_explicit_db_as_is() {
        // The escape hatch for a deliberately project-local store.
        assert_eq!(
            store_path(
                Some(PathBuf::from("./saniwalens.db")),
                &Harness::Opencode,
                Some(Path::new("/tmp/example/state")),
                Some(Path::new("/tmp/example/home"))
            )
            .unwrap(),
            PathBuf::from("./saniwalens.db")
        );
    }

    #[test]
    fn first_open_moves_a_legacy_store_and_renames_its_files() {
        // The store outlives the transcripts it was parsed from, so the
        // rename must never orphan it: the pre-rename directory moves under
        // the new name, its per-harness files included — never deleted.
        let dir =
            std::env::temp_dir().join(format!("saniwalens-migrate-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("cclens")).unwrap();
        std::fs::write(dir.join("cclens/cclens.db"), b"rows").unwrap();
        std::fs::write(dir.join("cclens/cclens-opencode.db"), b"rows").unwrap();

        migrate_legacy_store(&dir.join("saniwalens/saniwalens.db"));

        assert_eq!(
            std::fs::read(dir.join("saniwalens/saniwalens.db")).unwrap(),
            b"rows"
        );
        assert_eq!(
            std::fs::read(dir.join("saniwalens/saniwalens-opencode.db")).unwrap(),
            b"rows"
        );
        assert!(!dir.join("cclens").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_open_moves_an_intermediate_legacy_store_too() {
        // The binary has been renamed twice: a store living under the
        // intermediate name moves as well, whichever legacy name it uses.
        let dir = std::env::temp_dir().join(format!(
            "saniwalens-migrate-mid-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("agentlens")).unwrap();
        std::fs::write(dir.join("agentlens/agentlens.db"), b"rows").unwrap();
        std::fs::write(dir.join("agentlens/agentlens-kilocode.db"), b"rows").unwrap();

        migrate_legacy_store(&dir.join("saniwalens/saniwalens.db"));

        assert_eq!(
            std::fs::read(dir.join("saniwalens/saniwalens.db")).unwrap(),
            b"rows"
        );
        assert_eq!(
            std::fs::read(dir.join("saniwalens/saniwalens-kilocode.db")).unwrap(),
            b"rows"
        );
        assert!(!dir.join("agentlens").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_migration_never_touches_an_existing_store() {
        // A move never overwrites: a store already living under the new name
        // stays, and the legacy one survives beside it rather than being
        // deleted to make room.
        let dir = std::env::temp_dir().join(format!(
            "saniwalens-migrate-keep-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("cclens")).unwrap();
        std::fs::write(dir.join("cclens/cclens.db"), b"legacy").unwrap();
        std::fs::create_dir_all(dir.join("saniwalens")).unwrap();
        std::fs::write(dir.join("saniwalens/saniwalens.db"), b"current").unwrap();

        migrate_legacy_store(&dir.join("saniwalens/saniwalens.db"));

        assert_eq!(
            std::fs::read(dir.join("saniwalens/saniwalens.db")).unwrap(),
            b"current"
        );
        assert_eq!(
            std::fs::read(dir.join("cclens/cclens.db")).unwrap(),
            b"legacy"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_migration_is_silent_when_nothing_is_to_move() {
        let dir = std::env::temp_dir().join(format!(
            "saniwalens-migrate-none-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        migrate_legacy_store(&dir.join("saniwalens/saniwalens.db"));

        assert!(!dir.join("saniwalens").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
