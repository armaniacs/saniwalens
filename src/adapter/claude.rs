//! Claude Code's analyze routine: discovers transcripts and config under the
//! Claude Code roots, extracts them, and hands the rows to the store. The
//! seam is `Harness::analyze` (`harness.rs`) — this module is Claude's half
//! of it (`docs/specs/harness.md`, `session-format.md`, `config-format.md`).
//!
//! It is the one place that walks the Claude Code input route (the projects
//! directory), the per-transcript fingerprint and slug folding rules, and the
//! global/project config catalog that `analyze` stamps into the store.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::adapter::config::claude_home;
use crate::adapter::harness::{
    AnalyzeStats, Harness, ParsedSession, Site, file_fingerprint, load_agents, load_claude_md,
    load_hooks, load_mcp, load_rules, load_skills,
};
use crate::adapter::transcript::{
    CLAUDE_ANALYZER_META_KEY, CLAUDE_ANALYZER_VERSION, count_permission_denials,
    extract_hook_usages, extract_prompt_pointers, extract_tool_errors, extract_work_events,
    parse_session, parse_subagent_sidecar, session_cwd, subagent_id_from_file_name,
    subagent_prompt_id, subagent_sidecar_path, subagents_dir,
};
use crate::core::span::{DEFAULT_IDLE_GAP_MS, SessionStart, extract_spans, session_start};
use crate::core::subagent::{SubagentRun, link_runs};
use crate::core::usage::{attribute_subagents, extract_usage_events, output_tokens};
use crate::store::SessionMeta;

fn default_projects_dir() -> Result<PathBuf> {
    Ok(claude_home()?.join("projects"))
}

/// Fold a worktree slug onto its parent project so usage is not split across
/// `...--wt-feat-x` directories (see `docs/specs/storage.md`).
fn normalize_project(slug: &str) -> String {
    match slug.split_once("--wt-") {
        Some((parent, _)) => parent.to_string(),
        None => slug.to_string(),
    }
}

fn session_meta(
    transcript: &Path,
    root: String,
    sub_tokens: i64,
    sub_agent_count: i64,
    start: Option<SessionStart>,
) -> SessionMeta {
    let id = transcript
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("unknown")
        .to_string();
    let slug = transcript
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    SessionMeta {
        project: normalize_project(&slug),
        slug,
        id,
        root,
        source_path: transcript.display().to_string(),
        sub_tokens,
        sub_agent_count,
        start,
    }
}

/// Main session transcripts: the `<sessionId>.jsonl` files directly under each
/// project slug directory (subagent transcripts live one level deeper and are
/// not main-thread usage).
fn main_transcripts(projects: &Path) -> Result<Vec<PathBuf>> {
    let mut transcripts = Vec::new();
    let slugs = fs::read_dir(projects)
        .with_context(|| format!("read projects dir {}", projects.display()))?;
    for slug in slugs.flatten() {
        if !slug.path().is_dir() {
            continue;
        }
        for entry in fs::read_dir(slug.path()).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "jsonl") {
                transcripts.push(path);
            }
        }
    }
    Ok(transcripts)
}

/// This harness's global config sites, as data: the shared traversal
/// (`adapter::harness::catalog_surfaces`) loads them under `Scope::Global`.
/// Adding a kind is a loader and a row here — the traversal never changes.
pub(crate) fn global_sites(config_dir: &Path) -> Vec<Site> {
    vec![
        Site::new(config_dir.join("skills"), load_skills),
        Site::new(config_dir.join("rules"), load_rules),
        Site::new(config_dir.join("agents"), load_agents),
        Site::new(config_dir.join("mcp.json"), load_mcp),
        Site::new(config_dir.join("settings.json"), load_hooks),
        Site::with_payload(
            config_dir.to_path_buf(),
            load_claude_md,
            &[("CLAUDE.md", "global")],
        ),
    ]
}

/// This harness's per-project config sites: `<root>/.claude` mirrored, plus
/// the in-repo `.mcp.json` and the `CLAUDE.md` / `AGENTS.md` pair (CLAUDE.md
/// first: a repo shipping `AGENTS.md` as a symlink to it reports the first
/// loader that resolves).
pub(crate) fn project_sites(root: &Path) -> Vec<Site> {
    vec![
        Site::new(root.join(".claude").join("skills"), load_skills),
        Site::new(root.join(".claude").join("rules"), load_rules),
        Site::new(root.join(".claude").join("agents"), load_agents),
        Site::new(root.join(".mcp.json"), load_mcp),
        Site::new(root.join(".claude").join("settings.json"), load_hooks),
        Site::new(root.join(".claude").join("settings.local.json"), load_hooks),
        Site::with_payload(
            root.to_path_buf(),
            load_claude_md,
            &[("CLAUDE.md", "CLAUDE.md"), ("AGENTS.md", "AGENTS.md")],
        ),
    ]
}

/// Fold a worktree checkout's path onto its parent repository, mirroring
/// `normalize_project`'s slug rule at path level: `/repo/.wt/feat-x` → `/repo`.
///
/// Either separator delimits the segment, so a cwd recorded on Windows
/// (`C:\repo\.wt\feat-x`) folds like a unix one. The slug rule keys on `--wt-`
/// and is separator-free, so without this the two halves of the same
/// normalization disagreed and a project folded by slug stayed split by root.
fn normalize_root(cwd: &str) -> String {
    const SEGMENT: &str = ".wt";
    for (idx, _) in cwd.match_indices(SEGMENT) {
        let has_parent = cwd[..idx].ends_with(['/', '\\']);
        let has_child = cwd[idx + SEGMENT.len()..].starts_with(['/', '\\']);
        if has_parent && has_child {
            // `idx` is at least 1 here, and the separator before it is one byte.
            let parent = &cwd[..idx - 1];
            // Directly under a filesystem root there is no parent to fold onto:
            // the separator *is* the root, so dropping it would turn `/.wt/x`
            // into `""` and `C:\.wt\x` into a drive-relative `C:`. A drive
            // prefix is exactly two bytes, which keeps an ordinary directory
            // whose name ends in a colon out of this branch.
            if parent.is_empty() || (parent.len() == 2 && parent.ends_with(':')) {
                return cwd[..idx].to_string();
            }
            return parent.to_string();
        }
    }
    cwd.to_string()
}

pub fn analyze(projects: Option<PathBuf>, db: &Path) -> Result<AnalyzeStats> {
    let projects = projects.map(Ok).unwrap_or_else(default_projects_dir)?;
    let config_dir = claude_home()?;
    // The transcripts are cheap to list; each parse does the read, so a
    // skipped transcript is never read. The template owns the skip, ingest,
    // stamp, catalog, meta, and stats steps around those parses.
    crate::adapter::harness::analyze_template(
        Harness::Claude,
        db,
        CLAUDE_ANALYZER_META_KEY,
        CLAUDE_ANALYZER_VERSION,
        &projects,
        &config_dir,
        main_transcripts(&projects)?,
        |transcript| {
            (
                transcript.display().to_string(),
                file_fingerprint(transcript),
            )
        },
        |transcript: PathBuf, pre| {
            let text = fs::read_to_string(&transcript)
                .with_context(|| format!("read {}", transcript.display()))?;
            let denials = count_permission_denials(&text);
            let records = parse_session(&text);
            let mut spans = extract_spans(&records, DEFAULT_IDLE_GAP_MS);
            let mut usage = extract_usage_events(&records);
            // Hook firings join the catalog like any other usage: the same
            // `hookName` the settings catalogues under.
            usage.extend(extract_hook_usages(&text));
            let runs = subagent_runs(&transcript);
            attribute_subagents(&mut spans, &runs);
            let sub_tokens: i64 = runs.iter().map(|run| run.out_tokens as i64).sum();
            let root = session_cwd(&text).map(|cwd| normalize_root(&cwd));
            let meta = session_meta(
                &transcript,
                root.unwrap_or_default(),
                sub_tokens,
                runs.len() as i64,
                session_start(&records),
            );
            let prompts: Vec<(usize, i64, &'static str)> = extract_prompt_pointers(&text)
                .into_iter()
                .map(|(line, ts, behavior)| (line, ts, behavior.label()))
                .collect();
            // The template borrows these into its rows.
            let errors = extract_tool_errors(&text);
            let work = extract_work_events(&text);
            Ok(ParsedSession {
                meta,
                spans,
                usage,
                runs,
                prompts,
                errors,
                work,
                denials,
                // Record the size actually consumed, so a tail appended after
                // the read shows a mismatch next run instead of being skipped
                // forever.
                stamp: pre.map(|(mtime, _)| (mtime, text.len() as i64)),
                restat: move || file_fingerprint(&transcript),
            })
        },
    )
}

/// Every subagent run of a session, read from `<sessionId>/subagents/` beside
/// the main transcript: `agent-<agentId>.jsonl` holds the run's cost, and its
/// `.meta.json` sidecar names the agent type and the call that spawned it.
///
/// Runs come back linked (`link_runs`), so a nested run already knows the
/// main-thread spawn its tree started at. A run whose sidecar is missing keeps
/// an unknown agent type and only the turn-level join key — it still counts,
/// with the gap visible, rather than being guessed at or dropped.
fn subagent_runs(transcript: &Path) -> Vec<SubagentRun> {
    let Ok(entries) = fs::read_dir(subagents_dir(transcript)) else {
        return Vec::new();
    };
    let mut runs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "jsonl") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let sidecar = fs::read_to_string(subagent_sidecar_path(&path))
            .ok()
            .and_then(|json| parse_subagent_sidecar(&json))
            .unwrap_or_default();
        let records = parse_session(&text);
        runs.push(SubagentRun {
            agent: sidecar.agent_type,
            agent_id: path
                .file_stem()
                .map(|stem| subagent_id_from_file_name(&stem.to_string_lossy()))
                .unwrap_or_default(),
            run_path: path.display().to_string(),
            tool_use_id: sidecar.tool_use_id,
            parent_agent_id: sidecar.parent_agent_id,
            spawn_depth: sidecar.spawn_depth.unwrap_or(1),
            model: sidecar.model,
            prompt_id: subagent_prompt_id(&text),
            out_tokens: output_tokens(&records),
            started_epoch_ms: records.first().map(|r| r.timestamp_ms).unwrap_or_default(),
            root_tool_use_id: None,
        });
    }
    // Directory order is filesystem-dependent; spawn order is not. Sorting keeps
    // one session's ingest byte-identical across machines and re-runs.
    runs.sort_by(|a, b| {
        a.started_epoch_ms
            .cmp(&b.started_epoch_ms)
            .then_with(|| a.agent_id.cmp(&b.agent_id))
    });
    link_runs(&mut runs);
    runs
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{OpenMode, Store};
    use std::path::Path;

    /// Serializes the tests that point `analyze` at a synthetic config root:
    /// they share the process-wide `CLAUDE_CONFIG_DIR`, so they must not run
    /// concurrently.
    static CONFIG_DIR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Stamp `path` with an exact mtime — the production equivalent is the
    /// filesystem clock moving while a transcript is edited.
    fn set_mtime(path: &Path, mtime: std::time::SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    /// One synthetic transcript and one synthetic skill under a fixture config
    /// root — never a real transcript
    /// (`.claude/rules/session-data-privacy.md`).
    ///
    /// The expected counts are what the **pre-refactor binary** produced for
    /// exactly this fixture (recorded 2026-10-30): `analyzed 1 session(s)
    /// (0 unchanged), 1 skill invocation(s), 1 surface(s) catalogued,
    /// 0 subagent tokens across 0 subagent(s), 0 permission denial(s)`.
    fn fixture(dir: &Path) {
        let projects = dir.join("projects/-tmp-example-proj");
        let skill = dir.join("skills/demo-skill");
        std::fs::create_dir_all(&projects).unwrap();
        std::fs::create_dir_all(&skill).unwrap();
        let jsonl = concat!(
            r#"{"type":"user","cwd":"/tmp/example/proj","timestamp":"2026-01-01T00:00:00.000Z","message":{"content":"<command-name>/git-commit</command-name>"}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01.000Z","message":{"model":"claude-opus-4-7","usage":{"input_tokens":10,"cache_read_input_tokens":90,"cache_creation_input_tokens":0,"output_tokens":40}}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03.000Z","message":{"model":"claude-opus-4-7","usage":{"input_tokens":0,"cache_read_input_tokens":250,"cache_creation_input_tokens":0,"output_tokens":60}}}"#,
            "\n",
            r#"{"type":"user","timestamp":"2026-01-01T00:00:05.000Z","message":{"content":"thanks"}}"#,
        );
        std::fs::write(
            projects.join("aaaaaaaa-0000-0000-0000-000000000001.jsonl"),
            jsonl,
        )
        .unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: demo-skill\ndescription: Demo skill for the analyze fixture\n---\n# demo\n",
        )
        .unwrap();
    }

    #[test]
    fn claude_analyze_keeps_the_recorded_counts_on_a_fixture() {
        let _guard = CONFIG_DIR_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "saniwalens-claude-analyze-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        fixture(&dir);
        // The routine resolves its default input route and config root from
        // the environment, exactly as before the seam existed; the lock above
        // is what keeps this env read free of parallel readers.
        unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", &dir) };

        let db = dir.join("store.db");
        let stats = crate::adapter::harness::Harness::Claude
            .analyze(None, &db)
            .unwrap();

        assert_eq!(
            (
                stats.sessions,
                stats.skipped,
                stats.spans_total,
                stats.surface_count,
                stats.denials
            ),
            (1, 0, 1, 1, 0),
            "the counts the pre-refactor analyze produced for this fixture"
        );

        unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_same_second_same_size_edit_is_reingested_while_unchanged_is_skipped() {
        // The incremental contract (`pbi/2026-11-08-10-fix-fingerprint-stale-skip.md`):
        // an untouched transcript stays skipped, while a same-second,
        // same-size edit is re-ingested and the session row reflects the new
        // content. Synthetic fixture only — never a real transcript
        // (`.claude/rules/session-data-privacy.md`).
        let _guard = CONFIG_DIR_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "saniwalens-claude-incremental-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        fixture(&dir);
        unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", &dir) };

        let transcript =
            dir.join("projects/-tmp-example-proj/aaaaaaaa-0000-0000-0000-000000000001.jsonl");
        // Pin the pre-ingest mtime inside one whole second so the later edit
        // deterministically lands in that same second.
        let base = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::new(1_700_000_100, 100_000_000);
        set_mtime(&transcript, base);

        let db = dir.join("store.db");
        let first = crate::adapter::harness::Harness::Claude
            .analyze(None, &db)
            .unwrap();
        assert_eq!((first.sessions, first.skipped), (1, 0));

        // Untouched second run: the fast path still skips.
        let second = crate::adapter::harness::Harness::Claude
            .analyze(None, &db)
            .unwrap();
        assert_eq!((second.sessions, second.skipped), (0, 1));

        // Same-size rewrite (`60` → `66`, one token count) stamped inside the
        // same whole second: must be re-ingested, not skipped.
        let before = std::fs::read_to_string(&transcript).unwrap();
        let edited = before.replacen("\"output_tokens\":60", "\"output_tokens\":66", 1);
        assert_eq!(edited.len(), before.len(), "the edit must keep the size");
        std::fs::write(&transcript, edited).unwrap();
        set_mtime(&transcript, base + std::time::Duration::from_millis(500));

        let third = crate::adapter::harness::Harness::Claude
            .analyze(None, &db)
            .unwrap();
        assert_eq!(
            (third.sessions, third.skipped),
            (1, 0),
            "a same-second same-size edit must be re-ingested"
        );

        // The session row carries the new content, not the stale first pass:
        // the span's output tokens moved from 40 + 60 to 40 + 66.
        let store = Store::open(&db, OpenMode::ReadOnly).unwrap();
        let (_, rows) = store
            .query("SELECT out_tokens FROM events WHERE kind = 'skill_invocation'")
            .unwrap();
        assert_eq!(rows, vec![vec!["106".to_string()]]);

        unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalize_root_leaves_a_dot_wt_that_is_not_a_segment_alone() {
        // Only a whole `.wt` directory is a worktree parent.
        assert_eq!(
            normalize_root("/tmp/example/repo/x.wt/feat-x"),
            "/tmp/example/repo/x.wt/feat-x"
        );
        // A trailing `.wt` names no checkout to fold onto.
        assert_eq!(
            normalize_root("/tmp/example/repo/.wt"),
            "/tmp/example/repo/.wt"
        );
    }

    #[test]
    fn normalize_root_keeps_the_filesystem_root_separator() {
        // Folding onto the root leaves the separator standing: it *is* the
        // parent. Dropping it would hand back a relative path — `""` on unix,
        // the drive-relative `C:` on Windows — and the root then names a
        // different directory than the one the session ran in.
        assert_eq!(normalize_root("/.wt/feat-x"), "/");
        assert_eq!(normalize_root(r"C:\.wt\feat-x"), r"C:\");
        // A directory whose name merely ends in a colon is not a drive.
        assert_eq!(normalize_root("/tmp/odd:/.wt/feat-x"), "/tmp/odd:");
    }

    #[test]
    fn normalize_root_folds_a_backslash_worktree_path() {
        // A cwd recorded on Windows spells the same layout natively; leaving it
        // unfolded splits a project whose slug already folded.
        assert_eq!(
            normalize_root(r"C:\example\repo\.wt\feat-x"),
            r"C:\example\repo"
        );
        assert_eq!(normalize_root(r"C:\example\repo"), r"C:\example\repo");
    }

    #[test]
    fn normalize_root_folds_a_worktree_path_onto_its_repo() {
        assert_eq!(
            normalize_root("/tmp/example/repo/.wt/feat-x"),
            "/tmp/example/repo"
        );
        // Idempotent: an already-folded root stays put.
        assert_eq!(normalize_root("/tmp/example/repo"), "/tmp/example/repo");
    }

    // ---- PBI C11: a mid-read append breaks the re-stat match ----

    /// One synthetic transcript under a scratch dir — never a real transcript
    /// (`.claude/rules/session-data-privacy.md`).
    fn restat_fixture(name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "saniwalens-claude-restat-test-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("transcript.jsonl");
        std::fs::write(&path, "aaaa").unwrap();
        (dir, path)
    }

    #[test]
    fn a_mid_read_append_breaks_the_restat_match() {
        // The production race in miniature: stat, then the file grows before
        // the post-read re-stat. The gate `analyze` stamps through must
        // refuse, so the grown file is re-ingested next run, never skipped.
        let (dir, path) = restat_fixture("drift");
        let base = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::new(1_700_000_100, 100_000_000);
        set_mtime(&path, base);
        let pre = crate::adapter::harness::file_fingerprint(&path).unwrap();
        std::fs::write(&path, "aaaabbbb").unwrap();
        set_mtime(&path, base + std::time::Duration::from_millis(500));
        let post = crate::adapter::harness::file_fingerprint(&path).unwrap();
        assert_ne!(pre, post, "the drift fixture must move the fingerprint");
        assert!(
            !crate::adapter::harness::restat_matches(Some(pre), Some(post)),
            "a file that grew mid-read must not be stamped"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_untouched_file_passes_the_restat_match() {
        // The gate must not add re-ingests on the quiet path: an unchanged
        // file stats identically twice, so the stamp proceeds as before.
        let (dir, path) = restat_fixture("quiet");
        let pre = crate::adapter::harness::file_fingerprint(&path).unwrap();
        let post = crate::adapter::harness::file_fingerprint(&path).unwrap();
        assert!(crate::adapter::harness::restat_matches(
            Some(pre),
            Some(post)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
