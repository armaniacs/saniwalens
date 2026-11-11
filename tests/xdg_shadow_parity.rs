//! PBI C3 parity pins: XDG resolution and the opencode shadow loaders.
//!
//! BDD 1 pins the byte-identical resolution of the three XDG path resolvers
//! over the unset / relative / absolute combination table, so the shared
//! helper refactor cannot drift a single byte. BDD 2 pins that the opencode
//! shadow `SiteLoader`s read exactly what the canonical config readers read
//! (on fixtures without YAML `#` comments, the one known divergence), plus a
//! golden pin of the opencode catalog through the public entry points so the
//! shadow deletion cannot change the catalog. Synthetic temp data only.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use saniwalens::adapter::harness::Harness;
use saniwalens::core::surface::Scope;

mod common;
use common::ScratchDir;

fn opt(path: &str) -> Option<&Path> {
    Some(Path::new(path))
}

// ---- BDD 1: the XDG combination table resolves byte-identically ----

// Absolute fixtures come from the temp dir so the table runs on every OS:
// what the table pins is the absolute-vs-relative-vs-unset combination,
// not any particular root. Expected values are built with the same joins
// the resolvers use.
fn abs(parts: &[&str]) -> PathBuf {
    let mut p = std::env::temp_dir();
    for part in parts {
        p.push(part);
    }
    p
}

#[test]
fn opencode_db_path_resolves_the_xdg_table() {
    let xdg = abs(&["saniwalens-xdg", "data"]);
    let home = abs(&["saniwalens-home", "u"]);
    // Absolute data home wins over everything.
    assert_eq!(
        Harness::opencode_db_path(Some(xdg.as_path()), Some(home.as_path())),
        Some(xdg.join("opencode/opencode.db"))
    );
    // A relative data home is invalid per the XDG spec and ignored.
    assert_eq!(
        Harness::opencode_db_path(opt("relative/data"), Some(home.as_path())),
        Some(home.join(".local/share/opencode/opencode.db"))
    );
    // Unset falls back to the home-anchored share dir.
    assert_eq!(
        Harness::opencode_db_path(None, Some(home.as_path())),
        Some(home.join(".local/share/opencode/opencode.db"))
    );
    // An absolute data home needs no home at all.
    assert_eq!(
        Harness::opencode_db_path(Some(xdg.as_path()), None),
        Some(xdg.join("opencode/opencode.db"))
    );
    // Without an absolute home there is no fallback answer.
    assert_eq!(Harness::opencode_db_path(None, None), None);
    assert_eq!(Harness::opencode_db_path(opt("relative/data"), None), None);
    assert_eq!(Harness::opencode_db_path(None, opt("relative/home")), None);
}

#[test]
fn opencode_config_dir_resolves_the_xdg_table() {
    let xdg = abs(&["saniwalens-xdg", "config"]);
    let home = abs(&["saniwalens-home", "u"]);
    assert_eq!(
        Harness::opencode_config_dir(Some(xdg.as_path()), Some(home.as_path())),
        Some(xdg.join("opencode"))
    );
    assert_eq!(
        Harness::opencode_config_dir(opt("relative/config"), Some(home.as_path())),
        Some(home.join(".config/opencode"))
    );
    assert_eq!(
        Harness::opencode_config_dir(None, Some(home.as_path())),
        Some(home.join(".config/opencode"))
    );
    assert_eq!(
        Harness::opencode_config_dir(Some(xdg.as_path()), None),
        Some(xdg.join("opencode"))
    );
    assert_eq!(Harness::opencode_config_dir(None, None), None);
    assert_eq!(
        Harness::opencode_config_dir(opt("relative/config"), None),
        None
    );
    assert_eq!(
        Harness::opencode_config_dir(None, opt("relative/home")),
        None
    );
}

#[test]
fn kilocode_config_dir_resolves_the_xdg_table() {
    let xdg = abs(&["saniwalens-xdg", "config"]);
    let home = abs(&["saniwalens-home", "u"]);
    assert_eq!(
        Harness::kilocode_config_dir(Some(xdg.as_path()), Some(home.as_path())),
        Some(xdg.join("kilo"))
    );
    assert_eq!(
        Harness::kilocode_config_dir(opt("relative/config"), Some(home.as_path())),
        Some(home.join(".config/kilo"))
    );
    assert_eq!(
        Harness::kilocode_config_dir(None, Some(home.as_path())),
        Some(home.join(".config/kilo"))
    );
    assert_eq!(
        Harness::kilocode_config_dir(Some(xdg.as_path()), None),
        Some(xdg.join("kilo"))
    );
    assert_eq!(Harness::kilocode_config_dir(None, None), None);
    assert_eq!(
        Harness::kilocode_config_dir(opt("relative/config"), None),
        None
    );
    assert_eq!(
        Harness::kilocode_config_dir(None, opt("relative/home")),
        None
    );
}

// ---- BDD 2: the shadow loaders read what the canonical readers read ----

/// A config dir with one command and one agent, both with plain descriptions
/// (no YAML `#` comments: comment stripping is the one known divergence
/// between the opencode copy and the canonical reader).
fn markdown_fixture(root: &Path) -> (PathBuf, PathBuf) {
    let commands = root.join("command");
    let agents = root.join("agent");
    std::fs::create_dir_all(&commands).unwrap();
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(
        commands.join("review.md"),
        "---\ndescription: review the branch diff\n---\n$ARGUMENTS",
    )
    .unwrap();
    std::fs::write(
        agents.join("scout.md"),
        "---\ndescription: scouts the codebase\n---\nlong body",
    )
    .unwrap();
    (commands, agents)
}

fn sorted(
    mut surfaces: Vec<saniwalens::core::surface::Surface>,
) -> Vec<saniwalens::core::surface::Surface> {
    surfaces.sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
    surfaces
}

#[test]
fn shadow_command_reader_matches_the_canonical_reader() {
    let root = ScratchDir::new("saniwalens-xdg-parity", "command");
    let (commands, _) = markdown_fixture(&root);
    let scope = Scope::Global;

    let shadow = sorted(saniwalens::adapter::opencode::read_command_surfaces(
        &commands, &scope,
    ));
    let canonical = sorted(saniwalens::adapter::config::read_command_surfaces(
        &commands, &scope,
    ));
    assert_eq!(shadow, canonical);
}

#[test]
fn shadow_agent_reader_matches_the_canonical_reader() {
    let root = ScratchDir::new("saniwalens-xdg-parity", "agent");
    let (_, agents) = markdown_fixture(&root);
    let scope = Scope::Global;

    let shadow = sorted(saniwalens::adapter::opencode::read_agent_surfaces(
        &agents, &scope,
    ));
    let canonical = sorted(saniwalens::adapter::config::read_agent_surfaces(
        &agents, &scope,
    ));
    assert_eq!(shadow, canonical);
}

#[test]
fn opencode_catalog_pins_command_agent_and_claude_md() {
    let root = ScratchDir::new("saniwalens-xdg-parity", "catalog");
    let config_dir = root.join("cfg");
    let (commands, agents) = markdown_fixture(&config_dir);
    let _ = (&commands, &agents);

    let mut seen = HashSet::new();
    let global = sorted(saniwalens::adapter::opencode::read_global_surfaces(
        &config_dir,
        None,
        &mut seen,
    ));
    let rows: Vec<(String, String, Scope)> = global
        .iter()
        .map(|s| (s.kind.clone(), s.id.clone(), s.scope.clone()))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("agent".to_string(), "scout".to_string(), Scope::Global),
            ("command".to_string(), "review".to_string(), Scope::Global),
        ]
    );

    let project_root = root.join("proj");
    std::fs::create_dir_all(project_root.join(".opencode/command")).unwrap();
    std::fs::create_dir_all(project_root.join(".opencode/agent")).unwrap();
    std::fs::write(
        project_root.join(".opencode/command/review.md"),
        "---\ndescription: review\n---\nbody",
    )
    .unwrap();
    std::fs::write(
        project_root.join(".opencode/agent/scout.md"),
        "---\ndescription: scouts\n---\nbody",
    )
    .unwrap();
    std::fs::write(project_root.join("AGENTS.md"), "project instructions").unwrap();

    let project = sorted(saniwalens::adapter::opencode::read_project_surfaces(
        &project_root,
        "demo",
        None,
        &mut seen,
    ));
    let rows: Vec<(String, String, Scope)> = project
        .iter()
        .map(|s| (s.kind.clone(), s.id.clone(), s.scope.clone()))
        .collect();
    let project_scope = Scope::Project("demo".to_string());
    assert_eq!(
        rows,
        vec![
            (
                "agent".to_string(),
                "scout".to_string(),
                project_scope.clone()
            ),
            (
                "claude_md".to_string(),
                "AGENTS.md".to_string(),
                project_scope.clone()
            ),
            (
                "command".to_string(),
                "review".to_string(),
                project_scope.clone()
            ),
        ]
    );
}
