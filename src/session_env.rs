//! The resolution layer under `session.rs`: the environment roots every
//! command resolves through, the locale fallback for the advisor language,
//! and the flags/config-file resolution into a harness, a store path, and a
//! language. `session.rs` keeps the `Session` these feed — the opened and
//! guarded store — and its fs side effects.
//!
//! Reads are either pure over the supplied roots or process-env reads kept
//! at named entry points (`env_roots`, `locale_env`, `home_dir`,
//! `xdg_from_env`), so the resolvers stay hermetic for tests.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::adapter::config_file;
use crate::adapter::harness::Harness;
use crate::core::optimize::AdvisorLanguage;

/// The environment `Session` resolution reads. The caller supplies both
/// roots; tests pass temp dirs, production passes the live environment.
#[derive(Debug)]
pub struct SessionEnv<'a> {
    pub state_home: Option<&'a Path>,
    pub home: Option<&'a str>,
    pub config_home: Option<&'a Path>,
}

/// The process environment roots every command resolves through: the XDG
/// state/config overrides. Owned here so `run()` and the guide entry share
/// one read site; borrow into `SessionEnv` at each use site.
pub struct EnvRoots {
    pub state_home: Option<PathBuf>,
    pub config_home: Option<PathBuf>,
}

pub fn env_roots() -> EnvRoots {
    EnvRoots {
        state_home: std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        config_home: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
    }
}

/// `$VAR` as a path, the process-env read the pure resolvers deliberately do
/// not do so they stay hermetic for tests.
pub(crate) fn xdg_from_env(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

/// The user's home directory, resolved once per run.
///
/// `HOME` is preferred, so unix behaves exactly as before; Windows does not set
/// it, hence `USERPROFILE`. The wrinkle is that a Windows shell like Git Bash
/// *does* export `HOME` — as a POSIX path (`/c/Users/…`) that a native binary
/// cannot resolve. Taking it at face value would send every lookup to a
/// directory that does not exist and report "no transcripts" instead of an
/// error, so on Windows a candidate has to name a real directory to win. The
/// check stays off on unix, where a home that does not exist is worth surfacing
/// as itself rather than silently falling through to another variable.
pub(crate) fn home_dir() -> Option<&'static str> {
    static HOME: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        ["HOME", "USERPROFILE"]
            .into_iter()
            .filter_map(|key| std::env::var(key).ok())
            .filter(|value| !value.is_empty())
            .find(|value| !cfg!(windows) || Path::new(value).is_dir())
    })
    .as_deref()
}

/// The locale variables one invocation falls back to when neither
/// `--language` nor the config file names a language. Read once at the call
/// site ([`locale_env`]); resolved purely by [`resolve_locale_language`] —
/// the same read-here/resolve-pure split as [`EnvRoots`] / [`SessionEnv`].
pub struct LocaleEnv {
    pub lc_all: Option<String>,
    pub lc_messages: Option<String>,
    pub lang: Option<String>,
    pub language: Option<String>,
}

/// Read the process locale for advisor-language resolution. Owned here so
/// `run()` and the guide entry share one read site; resolved at each use
/// site with [`resolve_locale_language`], never inside the resolver.
pub fn locale_env() -> LocaleEnv {
    LocaleEnv {
        lc_all: std::env::var("LC_ALL").ok(),
        lc_messages: std::env::var("LC_MESSAGES").ok(),
        lang: std::env::var("LANG").ok(),
        language: std::env::var("LANGUAGE").ok(),
    }
}

/// Read the process locale and resolve the advisor-language fallback in one
/// call, for the two production wiring sites (`run()` and the guide entry).
/// Tests use [`locale_env`] plus [`resolve_locale_language`] directly with
/// synthetic values instead, so the precedence stays unit-testable.
pub fn process_locale_language() -> Option<AdvisorLanguage> {
    let locale = locale_env();
    resolve_locale_language(
        locale.lc_all.as_deref(),
        locale.lc_messages.as_deref(),
        locale.lang.as_deref(),
        locale.language.as_deref(),
    )
}

/// Whether one locale value votes a language: lowercase it, drop the encoding
/// (`.UTF-8`) and any `@` modifier, then match the first `_`/`-`-separated
/// subtag against the subtag table. `C`, `POSIX`, and the empty value vote
/// nothing (`None`) — a strange value falls back to the default, it never
/// fails the invocation. The table is the single source for locale voting, so
/// a language that gains a locale mapping is one row.
const LOCALE_SUBTAG_LANGUAGE: [(&str, AdvisorLanguage); 1] = [("ja", AdvisorLanguage::Ja)];

/// The advisor language one locale value votes for, if any, resolved through
/// the subtag table. `None` means the value carries no opinion.
pub fn locale_language(value: &str) -> Option<AdvisorLanguage> {
    let lower = value.trim().to_lowercase();
    let base = lower.split(['.', '@']).next().unwrap_or("");
    let subtag = base.split(['_', '-']).next().unwrap_or("");
    LOCALE_SUBTAG_LANGUAGE
        .iter()
        .find(|(subtag_language, _)| *subtag_language == subtag)
        .map(|(_, language)| *language)
}

/// Whether one locale value names Japanese — derived from the subtag table
/// (`locale_language`), never a hardcoded subtag.
pub fn locale_is_japanese(value: &str) -> bool {
    locale_language(value) == Some(AdvisorLanguage::Ja)
}

/// Whether a locale value is the bare `C` / `POSIX` locale that suppresses
/// `LANGUAGE` (gettext's C priority): exactly `C` or `POSIX`,
/// case-insensitive, with no encoding or modifier. `C.UTF-8` is *not* bare —
/// on Ubuntu `LANG=C.UTF-8` pairs with `LANGUAGE=ja`, so only the bare form
/// suppresses and anything else defers to the `LANGUAGE` head.
pub fn locale_is_c(value: &str) -> bool {
    matches!(value.trim().to_lowercase().as_str(), "c" | "posix")
}

/// The voting entry of a `LANGUAGE` priority list (`ja_JP:ja:en` → `ja_JP`):
/// the colon-separated head, trimmed. A blank list votes nothing (`None`).
pub fn language_head(value: &str) -> Option<&str> {
    let head = value.split(':').next().unwrap_or("").trim();
    (!head.is_empty()).then_some(head)
}

/// Resolve the locale fallback with POSIX precedence: `LC_ALL` wins over
/// `LC_MESSAGES`, which wins over `LANG`; an empty value counts as unset.
/// Beneath a quiet POSIX layer — no winner, or one that votes no language
/// and is not bare `C` — the `LANGUAGE` head votes (its own colon-separated
/// first entry, normalized like every other locale value). A bare-`C` /
/// `POSIX` winner suppresses `LANGUAGE` entirely (gettext's C priority), as
/// does a language-voting winner needing no help. Yields `Some(language)`
/// exactly when a layer votes one through the subtag table, `None`
/// otherwise — the caller falls back to the English default, so
/// `C`/`en_US`/unset all render byte-identical to before.
pub fn resolve_locale_language(
    lc_all: Option<&str>,
    lc_messages: Option<&str>,
    lang: Option<&str>,
    language: Option<&str>,
) -> Option<AdvisorLanguage> {
    let winner = [lc_all, lc_messages, lang]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty());
    if let Some(winner) = winner {
        if let Some(vote) = locale_language(winner) {
            return Some(vote);
        }
        if locale_is_c(winner) {
            return None;
        }
    }
    language.and_then(language_head).and_then(locale_language)
}

/// What `--harness` / `--profile` / `--language` plus the config file resolve
/// to, before any store is opened.
#[derive(Debug, PartialEq)]
pub struct ResolvedCli {
    pub harness: Harness,
    pub config_db: Option<PathBuf>,
    pub language: AdvisorLanguage,
    pub language_source: LanguageSource,
}

/// Which layer named the advisor language, strongest first: the explicit
/// `--language` flag, the config file's top-level `language`, the locale
/// fallback, or the English default. Carried on the resolution so
/// `--verbose` can print the decision (PBI 2026-11-14-02) without
/// re-deriving it; the value itself stays the only thing prompts render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageSource {
    Flag,
    Config,
    Locale,
    Default,
}

impl LanguageSource {
    pub fn label(self) -> &'static str {
        match self {
            LanguageSource::Flag => "flag",
            LanguageSource::Config => "config",
            LanguageSource::Locale => "locale",
            LanguageSource::Default => "default",
        }
    }
}

/// Resolve one invocation's harness and default store from flags plus the
/// tool's own config file. Pure over the supplied environment: tests pass
/// temp dirs, production passes the live roots. The language keeps its
/// default here — the flag-carrying entry is [`resolve_cli_with_language`],
/// which additionally applies the locale fallback;
/// the guided TUI resolves through that one and carries the language into
/// its inline next-step session.
pub fn resolve_cli(
    cli_harness: Option<&str>,
    cli_profile: Option<&str>,
    env: &SessionEnv,
) -> Result<ResolvedCli> {
    resolve_cli_with_language(cli_harness, cli_profile, None, None, env)
}

/// [`resolve_cli`] with an explicit `--language` flag. Resolution order,
/// strongest first: the flag, the file's top-level `language`, the locale
/// fallback ([`resolve_locale_language`]), then English. The locale is
/// already resolved by the caller — this function never reads the process
/// environment, so tests pass synthetic values.
pub fn resolve_cli_with_language(
    cli_harness: Option<&str>,
    cli_profile: Option<&str>,
    cli_language: Option<&str>,
    locale: Option<AdvisorLanguage>,
    env: &SessionEnv,
) -> Result<ResolvedCli> {
    let config_path = config_file_path(env.config_home, env.home);
    let config_dir = config_path.as_deref().and_then(|p| p.parent());
    let file = config_path
        .as_deref()
        .map(config_file::load)
        .transpose()?
        .flatten();
    let resolved = config_file::resolve(cli_harness, cli_profile, None, file.as_ref(), config_dir)?;
    // The locale votes at most one language through the subtag table: Some
    // names it, None carries no opinion — so the vote feeds the resolution
    // directly instead of round-tripping through a string and the parser.
    let (language, language_source) =
        match config_file::resolve_language(cli_language, file.as_ref()) {
            // A present flag always wins the merge above, so its presence names
            // the source; otherwise the file's own default did.
            Some(code) => {
                let source = if cli_language.is_some() {
                    LanguageSource::Flag
                } else {
                    LanguageSource::Config
                };
                (parse_language(Some(&code))?, source)
            }
            None if let Some(vote) = locale => (vote, LanguageSource::Locale),
            None => (AdvisorLanguage::default(), LanguageSource::Default),
        };
    Ok(ResolvedCli {
        harness: parse_harness(resolved.harness.as_deref())?,
        config_db: resolved.db,
        language,
        language_source,
    })
}

/// Parse `language` after resolution. Absent means English — every
/// pre-language invocation keeps its behavior. An unknown value fails naming
/// the valid list, whether it came from `--language` or the config file.
pub fn parse_language(value: Option<&str>) -> Result<AdvisorLanguage> {
    match value {
        None => Ok(AdvisorLanguage::default()),
        Some(v) => AdvisorLanguage::parse(v)
            .with_context(|| format!("unknown language '{v}' ({})", AdvisorLanguage::label_list())),
    }
}

/// Parse `--harness`. Absent means Claude Code — every pre-flag invocation
/// keeps its behavior, which is what keeps the flag upstream-mergeable. The
/// hint vocabulary comes from the `Harness` single source, never re-spelled.
pub fn parse_harness(value: Option<&str>) -> Result<Harness> {
    match value {
        None => Ok(Harness::Claude),
        Some(v) => Harness::parse(v)
            .with_context(|| format!("unknown --harness '{v}' ({})", Harness::label_list())),
    }
}

/// Where the tool's own config file lives, if anywhere: under
/// `$XDG_CONFIG_HOME/saniwalens/`, else `~/.config/saniwalens/`. Pure over
/// its inputs like `store_path`, for the same reason. `None` means there is
/// no file to read, which means "behave exactly as before".
pub fn config_file_path(config_home: Option<&Path>, home: Option<&str>) -> Option<PathBuf> {
    if let Some(dir) = config_home.filter(|dir| dir.is_absolute()) {
        return Some(dir.join("saniwalens/config.toml"));
    }
    home.filter(|dir| Path::new(dir).is_absolute())
        .map(|dir| Path::new(dir).join(".config/saniwalens/config.toml"))
}

/// The absolute base every user-level state dir resolves under: an absolute
/// `$XDG_STATE_HOME` wins, a relative one is ignored (the XDG spec says a
/// non-absolute value is invalid), and without an absolute home there is no
/// answer. Both roots must be absolute: honoring a relative one would
/// reintroduce a cwd-relative location this default exists to avoid. Shared by
/// every resolver that owns a directory under it (`store_path`, the handoff
/// plans/backups dirs), so the rule is written once.
pub(crate) fn state_base(state_home: Option<&Path>, home: Option<&Path>) -> Result<PathBuf> {
    Ok(match state_home.filter(|dir| dir.is_absolute()) {
        Some(dir) => dir.to_path_buf(),
        None => home
            .filter(|dir| dir.is_absolute())
            .context("HOME is not set to an absolute path")?
            .join(".local")
            .join("state"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example_dir(sub: &str) -> PathBuf {
        std::env::temp_dir().join(format!("saniwalens-session-{sub}-{}", std::process::id()))
    }

    #[test]
    fn parse_harness_unknown_error_lists_every_harness() {
        // The hint vocabulary is pinned byte for byte: the refactor builds it
        // from the `Harness` single source, and this literal must not move.
        let err = parse_harness(Some("auto")).unwrap_err();
        assert_eq!(
            format!("{err:#}"),
            "unknown --harness 'auto' (claude | opencode | kilocode)"
        );
    }

    #[test]
    fn parse_harness_defaults_to_claude_and_rejects_the_rest() {
        assert_eq!(parse_harness(None).unwrap(), Harness::Claude);
        assert_eq!(parse_harness(Some("opencode")).unwrap(), Harness::Opencode);
        assert!(parse_harness(Some("auto")).is_err());
    }

    #[test]
    fn config_file_path_defaults_under_the_user_config_dir() {
        // Absolute on every OS: a spelled-out `/tmp/...` is not absolute on
        // Windows, and these resolvers ignore non-absolute inputs.
        let config = std::env::temp_dir().join("saniwalens-fixture-config");
        let home = std::env::temp_dir().join("saniwalens-fixture-home");
        assert_eq!(
            config_file_path(Some(&config), None),
            Some(config.join("saniwalens/config.toml"))
        );
        assert_eq!(
            config_file_path(None, Some(home.to_str().unwrap())),
            Some(home.join(".config/saniwalens/config.toml"))
        );
    }

    #[test]
    fn config_file_path_ignores_a_relative_xdg_config_home() {
        assert_eq!(
            config_file_path(Some(Path::new("relative/config")), None),
            None
        );
    }

    #[test]
    fn config_file_path_without_a_home_is_none() {
        assert_eq!(config_file_path(None, None), None);
    }

    #[test]
    fn resolve_cli_without_a_file_means_builtins() {
        let dir = example_dir("no-file");
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: Some(&dir),
        };
        let resolved = resolve_cli(None, None, &env).unwrap();
        assert_eq!(
            resolved,
            ResolvedCli {
                harness: Harness::Claude,
                config_db: None,
                language: AdvisorLanguage::En,
                language_source: LanguageSource::Default,
            }
        );
    }

    #[test]
    fn resolve_cli_reads_default_harness_and_config_relative_db() {
        let dir = example_dir("file-defaults");
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = dir.join("saniwalens");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join("config.toml"),
            "default_harness = \"kilocode\"\ndb = \"k.db\"\n",
        )
        .unwrap();
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: Some(&dir),
        };
        let resolved = resolve_cli(None, None, &env).unwrap();
        assert_eq!(resolved.harness, Harness::Kilocode);
        assert_eq!(resolved.config_db, Some(cfg.join("k.db")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_cli_flags_beat_the_file() {
        let dir = example_dir("file-override");
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = dir.join("saniwalens");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join("config.toml"),
            "default_harness = \"kilocode\"\n[profiles.work]\nharness = \"claude\"\n",
        )
        .unwrap();
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: Some(&dir),
        };
        let resolved = resolve_cli(Some("opencode"), Some("work"), &env).unwrap();
        assert_eq!(resolved.harness, Harness::Opencode);
        assert_eq!(resolved.config_db, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_cli_unknown_profile_lists_available() {
        let dir = example_dir("file-unknown-profile");
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = dir.join("saniwalens");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join("config.toml"),
            "[profiles.work]\nharness = \"claude\"\n",
        )
        .unwrap();
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: Some(&dir),
        };
        let err = resolve_cli(None, Some("play"), &env).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("play"), "{msg}");
        assert!(msg.contains("work"), "{msg}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_cli_unknown_harness_keeps_the_flag_error() {
        let dir = example_dir("file-bad-harness");
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = dir.join("saniwalens");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("config.toml"), "default_harness = \"auto\"\n").unwrap();
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: Some(&dir),
        };
        let err = resolve_cli(None, None, &env).unwrap_err();
        assert!(format!("{err:#}").contains("unknown --harness 'auto'"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn write_config(dir: &Path, content: &str) {
        let cfg = dir.join("saniwalens");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("config.toml"), content).unwrap();
    }

    #[test]
    fn parse_language_defaults_to_en_and_accepts_ja() {
        assert_eq!(parse_language(None).unwrap(), AdvisorLanguage::En);
        assert_eq!(parse_language(Some("en")).unwrap(), AdvisorLanguage::En);
        assert_eq!(parse_language(Some("ja")).unwrap(), AdvisorLanguage::Ja);
    }

    #[test]
    fn parse_language_rejects_unknown_values_with_the_valid_list() {
        // PBI 2026-11-10-07: an unknown value fails naming the valid ones.
        for bad in ["jp", "EN", ""] {
            let err = parse_language(Some(bad)).unwrap_err();
            let msg = format!("{err:#}");
            assert!(msg.contains(bad), "{msg}");
            assert!(msg.contains("en"), "{msg}");
            assert!(msg.contains("ja"), "{msg}");
        }
    }

    #[test]
    fn parse_language_unknown_hint_pins_the_candidate_bytes() {
        // PBI 2026-11-21-04: the hint vocabulary moves to the AdvisorLanguage
        // single source; this literal pins the bytes across the refactor, the
        // same shape as parse_harness's pin above.
        let err = parse_language(Some("fr")).unwrap_err();
        assert_eq!(format!("{err:#}"), "unknown language 'fr' (en | ja)");
    }

    #[test]
    fn resolve_cli_language_flag_beats_the_file() {
        let dir = example_dir("file-language-override");
        let _ = std::fs::remove_dir_all(&dir);
        write_config(&dir, "language = \"ja\"\n");
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: Some(&dir),
        };
        let resolved = resolve_cli_with_language(None, None, Some("en"), None, &env).unwrap();
        assert_eq!(resolved.language, AdvisorLanguage::En);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_cli_language_falls_back_to_the_file_then_english() {
        let dir = example_dir("file-language-default");
        let _ = std::fs::remove_dir_all(&dir);
        write_config(&dir, "language = \"ja\"\n");
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: Some(&dir),
        };
        let resolved = resolve_cli_with_language(None, None, None, None, &env).unwrap();
        assert_eq!(resolved.language, AdvisorLanguage::Ja);
        let _ = std::fs::remove_dir_all(&dir);

        let bare = example_dir("file-language-unset");
        let env = SessionEnv {
            state_home: Some(&bare),
            home: None,
            config_home: Some(&bare),
        };
        let resolved = resolve_cli_with_language(None, None, None, None, &env).unwrap();
        assert_eq!(resolved.language, AdvisorLanguage::En);
    }

    #[test]
    fn resolve_cli_language_rejects_an_unknown_file_value() {
        let dir = example_dir("file-language-bad");
        let _ = std::fs::remove_dir_all(&dir);
        write_config(&dir, "language = \"jp\"\n");
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: Some(&dir),
        };
        let err = resolve_cli_with_language(None, None, None, None, &env).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("jp"), "{msg}");
        assert!(msg.contains("en") && msg.contains("ja"), "{msg}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locale_is_japanese_matches_the_leading_subtag_only() {
        // PBI 2026-11-11-01: normalize by lowercasing, dropping the
        // encoding (`.UTF-8`) and `@` modifier, then reading the first
        // `_`/`-`-separated subtag — `ja_JP.UTF-8` is Japanese, `C` is not.
        for ja in [
            "ja",
            "ja_JP",
            "ja_JP.UTF-8",
            "JA-jp",
            "ja@tokyo",
            "ja_JP.UTF-8@modifier",
        ] {
            assert!(locale_is_japanese(ja), "{ja}");
        }
        for not_ja in [
            "",
            "C",
            "POSIX",
            "en",
            "en_US",
            "en_US.UTF-8",
            "japanese",
            "de_JA",
        ] {
            assert!(!locale_is_japanese(not_ja), "{not_ja}");
        }
    }

    #[test]
    fn locale_votes_resolve_through_the_subtag_table() {
        // PBI 2026-11-21-04: the subtag→language table is the single source
        // for locale voting; `locale_is_japanese` stays a derived view of it,
        // so a language that gains a locale mapping is one table row.
        assert_eq!(locale_language("ja_JP.UTF-8"), Some(AdvisorLanguage::Ja));
        assert_eq!(locale_language("ja"), Some(AdvisorLanguage::Ja));
        assert_eq!(locale_language("JA-jp"), Some(AdvisorLanguage::Ja));
        for silent in ["en_US.UTF-8", "C", "C.UTF-8", "", "japanese", "de_JA"] {
            assert_eq!(locale_language(silent), None, "{silent}");
        }
        assert!(locale_is_japanese("ja_JP.UTF-8"));
        assert!(!locale_is_japanese("en_US.UTF-8"));
    }

    #[test]
    fn resolve_locale_language_follows_posix_precedence() {
        // PBI 2026-11-11-01: `LC_ALL` > `LC_MESSAGES` > `LANG`; an empty
        // value counts as unset. A non-Japanese winner carries no opinion
        // (`None`), so the caller falls back to the English default.
        use AdvisorLanguage::Ja;
        assert_eq!(
            resolve_locale_language(Some("ja_JP.UTF-8"), None, None, None),
            Some(Ja)
        );
        assert_eq!(
            resolve_locale_language(None, Some("ja"), None, None),
            Some(Ja)
        );
        assert_eq!(
            resolve_locale_language(None, None, Some("ja_JP.UTF-8"), None),
            Some(Ja)
        );
        assert_eq!(resolve_locale_language(None, None, None, None), None);
        assert_eq!(
            resolve_locale_language(Some(""), Some(""), Some(""), None),
            None
        );
        // Boundary: `LC_ALL=C` silences `LANG=ja`; `LC_MESSAGES` wins over
        // `LANG` the same way; a `ja` `LC_ALL` wins over everything below.
        assert_eq!(
            resolve_locale_language(Some("C"), None, Some("ja_JP.UTF-8"), None),
            None
        );
        assert_eq!(
            resolve_locale_language(None, Some("C"), Some("ja"), None),
            None
        );
        assert_eq!(
            resolve_locale_language(Some("ja"), Some("C"), Some("C"), None),
            Some(Ja)
        );
        assert_eq!(
            resolve_locale_language(None, None, Some("en_US.UTF-8"), None),
            None
        );
    }

    #[test]
    fn resolve_cli_language_prefers_flag_over_file_over_locale_over_english() {
        // PBI 2026-11-11-01: the full order — `--language` > config >
        // locale > En — as one table over the file-backed and bare dirs.
        use AdvisorLanguage::{En, Ja};
        let dir = example_dir("file-language-locale-order");
        let _ = std::fs::remove_dir_all(&dir);
        write_config(&dir, "language = \"ja\"\n");
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: Some(&dir),
        };
        // The flag beats the file even with a Japanese locale behind it.
        let resolved = resolve_cli_with_language(None, None, Some("en"), Some(Ja), &env).unwrap();
        assert_eq!(resolved.language, En);
        // The file beats a non-Japanese (`C`) locale: `config ja` +
        // `LANG=C` stays Japanese.
        let resolved = resolve_cli_with_language(None, None, None, None, &env).unwrap();
        assert_eq!(resolved.language, Ja);
        let _ = std::fs::remove_dir_all(&dir);

        let bare = example_dir("file-language-locale-bare");
        let bare_env = SessionEnv {
            state_home: Some(&bare),
            home: None,
            config_home: Some(&bare),
        };
        // The locale fills the gap the flag and the file leave.
        let resolved = resolve_cli_with_language(None, None, None, Some(Ja), &bare_env).unwrap();
        assert_eq!(resolved.language, Ja);
        // ... and the default is still English with no locale behind it.
        let resolved = resolve_cli_with_language(None, None, None, None, &bare_env).unwrap();
        assert_eq!(resolved.language, En);
        // The flag beats the locale with no file in play either.
        let resolved =
            resolve_cli_with_language(None, None, Some("en"), Some(Ja), &bare_env).unwrap();
        assert_eq!(resolved.language, En);
    }

    #[test]
    fn locale_is_c_matches_only_the_bare_suppressing_form() {
        // PBI 2026-11-14-01: only a bare `C` / `POSIX` winner suppresses
        // `LANGUAGE` (gettext's C priority). `C.UTF-8` is not bare — on
        // Ubuntu `LANG=C.UTF-8` pairs with `LANGUAGE=ja`, so it must defer.
        for c in ["C", "c", "POSIX", "posix", "  C  "] {
            assert!(locale_is_c(c), "{c}");
        }
        for not_c in [
            "",
            "C.UTF-8",
            "c.utf8",
            "en",
            "en_US.UTF-8",
            "ja",
            "ja_JP.UTF-8",
        ] {
            assert!(!locale_is_c(not_c), "{not_c}");
        }
    }

    #[test]
    fn language_head_takes_the_first_colon_entry_only() {
        // PBI 2026-11-14-01: `LANGUAGE` is a priority list — only the head
        // votes, and a blank list votes nothing.
        assert_eq!(language_head("ja_JP:ja"), Some("ja_JP"));
        assert_eq!(language_head("fr:en"), Some("fr"));
        assert_eq!(language_head("ja"), Some("ja"));
        assert_eq!(language_head(""), None);
        assert_eq!(language_head("   "), None);
        assert_eq!(language_head("  ja_JP : en"), Some("ja_JP"));
    }

    #[test]
    fn resolve_locale_language_reads_language_beneath_a_quiet_posix() {
        // PBI 2026-11-14-01: the `LANGUAGE` column of the resolution table —
        // a quiet POSIX layer (unset, or non-Japanese and non-C) defers to
        // the `LANGUAGE` head; a bare-`C` winner suppresses it.
        use AdvisorLanguage::Ja;
        // BDD "LANGUAGEだけで日本語になる": `LANG=C.UTF-8` is not bare C,
        // so `LANGUAGE=ja_JP:ja` votes Japanese.
        assert_eq!(
            resolve_locale_language(None, None, Some("C.UTF-8"), Some("ja_JP:ja")),
            Some(Ja)
        );
        // No POSIX at all: the head alone decides.
        assert_eq!(
            resolve_locale_language(None, None, None, Some("ja_JP:en_US:fr")),
            Some(Ja)
        );
        assert_eq!(
            resolve_locale_language(None, None, None, Some("fr:en")),
            None
        );
        assert_eq!(resolve_locale_language(None, None, None, Some("")), None);
        // BDD "先頭サブタグのみを見る": a non-Japanese head stays silent
        // even with `ja` behind it.
        assert_eq!(
            resolve_locale_language(None, None, None, Some("fr:ja")),
            None
        );
        // BDD "CロケールはLANGUAGEに勝つ": a bare-`C` winner ignores
        // `LANGUAGE`, at every POSIX layer.
        assert_eq!(
            resolve_locale_language(Some("C"), None, Some("ja"), Some("ja")),
            None
        );
        assert_eq!(
            resolve_locale_language(None, Some("POSIX"), None, Some("ja")),
            None
        );
        assert_eq!(
            resolve_locale_language(None, None, Some("C"), Some("ja")),
            None
        );
        // A Japanese POSIX winner needs no help from `LANGUAGE`.
        assert_eq!(
            resolve_locale_language(None, None, Some("ja_JP.UTF-8"), Some("fr")),
            Some(Ja)
        );
        // A non-Japanese, non-C POSIX layer still defers to `LANGUAGE`
        // (gettext: `LANG` names the codeset, `LANGUAGE` the messages).
        assert_eq!(
            resolve_locale_language(None, None, Some("en_US.UTF-8"), Some("ja")),
            Some(Ja)
        );
    }

    #[test]
    fn resolve_cli_language_reports_its_source() {
        // PBI 2026-11-14-02: the resolution names its winning layer —
        // flag, then the file, then the locale, then the default — so
        // `--verbose` can print the decision without re-deriving it.
        use AdvisorLanguage::{En, Ja};
        use LanguageSource::{Config, Default, Flag, Locale};
        let dir = example_dir("file-language-source");
        let _ = std::fs::remove_dir_all(&dir);
        write_config(&dir, "language = \"ja\"\n");
        let env = SessionEnv {
            state_home: Some(&dir),
            home: None,
            config_home: Some(&dir),
        };
        // The flag beats the file.
        let resolved = resolve_cli_with_language(None, None, Some("en"), Some(Ja), &env).unwrap();
        assert_eq!((resolved.language, resolved.language_source), (En, Flag));
        // The file beats a `C` locale.
        let resolved = resolve_cli_with_language(None, None, None, None, &env).unwrap();
        assert_eq!((resolved.language, resolved.language_source), (Ja, Config));
        let _ = std::fs::remove_dir_all(&dir);

        let bare = example_dir("file-language-source-bare");
        let bare_env = SessionEnv {
            state_home: Some(&bare),
            home: None,
            config_home: Some(&bare),
        };
        // The locale fills the gap the flag and the file leave ...
        let resolved = resolve_cli_with_language(None, None, None, Some(Ja), &bare_env).unwrap();
        assert_eq!((resolved.language, resolved.language_source), (Ja, Locale));
        // ... and the default decides when nothing votes.
        let resolved = resolve_cli_with_language(None, None, None, None, &bare_env).unwrap();
        assert_eq!((resolved.language, resolved.language_source), (En, Default));
    }
}
