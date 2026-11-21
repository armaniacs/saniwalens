use anyhow::{Context, Result};

/// `create_dir_all` with owner-only permissions on every directory it creates —
/// the mode the XDG Base Directory spec prescribes for a directory a program
/// creates on the user's behalf. Directories that already exist keep theirs;
/// whether an existing non-owner-only mode warrants a warning is the caller's
/// call ([`unprivate_dir_warning`]), because the rendering lives in the
/// caller's layer.
pub(crate) fn create_private_dir_all(dir: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir)
}

/// The stderr warning for an already-existing directory whose mode still
/// reaches group/other accounts, or `None` when it is owner-only (or the
/// path is not there yet, which the caller checks first). The pure half of
/// the private-dir warning: same-user tests pin it through real `chmod`ed
/// modes, since staging a genuinely foreign-owned directory would need root.
/// `Store::open` and `cli::deliver` both call this and render it in their own
/// layer's style.
#[cfg(unix)]
pub(crate) fn unprivate_dir_warning(dir: &std::path::Path, mode: u32) -> Option<String> {
    (mode & 0o077 != 0).then(|| {
        format!(
            "warning: directory {} already exists with group/other access bits (mode {:o}) — \
             private state may be reachable by other local accounts; continuing",
            dir.display(),
            mode & 0o777
        )
    })
}

/// Make sure the store file is readable only by its owner: create it `0600`
/// when absent, and narrow it when it is already there with group/other bits
/// set. Creating it here rather than letting SQLite create it under the umask
/// is what removes the window — SQLite would otherwise write the schema into a
/// world-readable file and only then have it chmod'ed — and it lets SQLite
/// inherit the mode for the rollback journal it writes beside the database.
///
/// An existing store is narrowed rather than left alone: a store an older
/// saniwalens created under the umask is the realistic case, and nobody
/// deliberately shares this store. Only a regular file is touched, so pointing
/// `--db` at a directory fails at SQLite instead of silently re-moding it, and
/// a store owned by someone else is left as it is — re-moding it is not this
/// process's call, and refusing to run over it would break a store legitimately
/// created under another account. Left as it is, but not silently: a store
/// with group/other bits and a refused narrowing are both warned about on
/// stderr — "whatever mode already sits there" is exactly the premise above
/// broken, and the open proceeds but must not do so invisibly.
pub(super) fn ensure_owner_only(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::fs::{OpenOptions, Permissions};
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(_) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e).with_context(|| format!("create store {}", path.display())),
        }
        let meta = std::fs::metadata(path)
            .with_context(|| format!("read store permissions {}", path.display()))?;
        let mode = meta.permissions().mode();
        if meta.is_file()
            && let Some(line) = unprivate_store_file_warning(path, mode)
        {
            eprintln!("{line}");
        }
        if meta.is_file() && mode & 0o077 != 0 {
            // 0600 outright rather than masking the existing mode: masking would
            // carry over a stray execute bit, and could leave the owner without
            // the read/write the store needs to be opened at all.
            narrow_store_or_warn(
                path,
                std::fs::set_permissions(path, Permissions::from_mode(0o600)),
            )?;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// The stderr warning for an already-existing store file whose mode still
/// reaches group/other accounts, or `None` when it is owner-only. Fires
/// whether or not the narrowing below succeeds — the exposure happened
/// either way. The pure half of the warning, testable same-user through
/// real `chmod`ed modes.
#[cfg(unix)]
pub(super) fn unprivate_store_file_warning(path: &std::path::Path, mode: u32) -> Option<String> {
    (mode & 0o077 != 0).then(|| {
        format!(
            "warning: store file {} has group/other access bits (mode {:o}) — \
             transcript-derived data may be readable by other local accounts",
            path.display(),
            mode & 0o777
        )
    })
}

/// The narrowing half of [`ensure_owner_only`]'s existing-file branch: a
/// refused narrowing — in practice a store owned by another account — warns
/// and continues, while any other failure stays a real error. Kept as a
/// value-passing function so the deny branch is testable same-user with a
/// synthetic denied error; staging a genuinely foreign-owned store file
/// needs root.
#[cfg(unix)]
pub(super) fn narrow_store_or_warn(
    path: &std::path::Path,
    narrowed: std::io::Result<()>,
) -> Result<()> {
    match narrowed {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("{}", unrestrictable_store_file_warning(path));
            Ok(())
        }
        Err(e) => Err(e).with_context(|| format!("restrict store permissions {}", path.display())),
    }
}

/// The stderr warning for a store file this process was not allowed to narrow
/// to `0600` — in practice one owned by another account, whose mode (and
/// contents) stay whatever they are.
#[cfg(unix)]
pub(super) fn unrestrictable_store_file_warning(path: &std::path::Path) -> String {
    format!(
        "warning: store file {} could not be restricted to 0600 (owned by another account?) — \
         continuing with the mode it has",
        path.display()
    )
}
