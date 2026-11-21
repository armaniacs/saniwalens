use std::collections::BTreeMap;

use anyhow::Result;
use rusqlite::Connection;

use super::schema::{INDEX_SCHEMA, MIGRATED_TABLES, REBUILT_TABLES, SCHEMA_VERSION};
use super::view_schema;

/// Bring an existing store onto the current schema **in place**. The store
/// outlives the transcripts it was built from, so an upgrade may not resolve
/// schema drift by asking for the file to be deleted — that would discard
/// history no re-analysis can reconstruct (`docs/specs/storage.md`).
///
/// Drift is reconciled per table by what the table costs to lose, and the
/// declared schema is the single source of truth for the target shape: it is
/// applied to a scratch database and read back, so adding a column to `SCHEMA`
/// is all a future change needs — provided the changed table is classified
/// into [`MIGRATED_TABLES`] or [`REBUILT_TABLES`], since the convergence loop
/// visits only those. An unclassified declaration would drift every existing
/// store silently, so the migration refuses it up front.
///
/// The schema text is a parameter only so tests can replay a declaration this
/// release has not made yet — the classification guard's mistake is by
/// definition absent from a correct `SCHEMA`.
///
/// The version guard and the stamp recording this build's schema share one
/// `BEGIN IMMEDIATE` transaction: deciding outside the lock would let a
/// concurrent saniwalens move `user_version` between the guard's read and
/// this build's write, and this build would then lay its shape over a store
/// a newer binary still reads.
pub(super) fn migrate(conn: &mut Connection, schema: &str) -> Result<()> {
    let desired = Connection::open_in_memory()?;
    desired.execute_batch(schema)?;
    desired.execute_batch(INDEX_SCHEMA)?;
    desired.execute_batch(&view_schema())?;
    ensure_all_classified(&desired)?;

    // One transaction for the whole migration, as every other multi-statement
    // write in this file does: reconciliation may bail partway through, and a
    // store left half-migrated is exactly the outcome in-place upgrading exists
    // to avoid. SQLite rolls DDL back like any other statement.
    //
    // BEGIN IMMEDIATE, not the deferred default: it takes the write lock up
    // front, so the version guard below is serialized with the user_version
    // stamp at the end, and a concurrent writer's lock delays the guard itself
    // (waiting, via the busy timeout set in `from_connection`) instead of the
    // migration deciding first and BUSY-failing mid-DDL. The guard sits before
    // any DDL, so its bail drops an untouched transaction.
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

    let version: i64 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        // Reconciliation only ever adds; it cannot undo a newer saniwalens's
        // changes, and writing this build's shape over them would corrupt a
        // store that binary still reads correctly.
        anyhow::bail!(
            "the store was written by a newer saniwalens (schema v{version}; this build knows \
             v{SCHEMA_VERSION}) — upgrade saniwalens to read it"
        );
    }

    let mut rebuilt = false;
    for table in REBUILT_TABLES {
        if table_exists(&tx, table)? && table_shape(&tx, table)? != table_shape(&desired, table)? {
            tx.execute_batch(&format!("DROP TABLE {table}"))?;
            rebuilt = true;
        }
    }
    tx.execute_batch(schema)?;
    for table in MIGRATED_TABLES {
        reconcile_columns(&tx, &desired, table)?;
    }
    // Indexes and views come last: both reference columns the step above may
    // have just added.
    reconcile_derived(&tx, &desired, "index")?;
    reconcile_derived(&tx, &desired, "view")?;
    if rebuilt {
        // The catalog is empty until the next analyze refills it. Without this a
        // `--frozen` read would print an ordinary freshness line over an empty
        // catalog, reporting "nothing installed" as though it were a finding.
        tx.execute("DELETE FROM meta WHERE key = 'analyzed_at'", ())?;
    }

    tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
    tx.commit()?;
    Ok(())
}

/// Every table the declaration names must be classified. The convergence loop
/// visits only `MIGRATED_TABLES` and `REBUILT_TABLES`, so an unclassified table
/// would keep its stored shape on every store opened before it was declared —
/// the declared promise holding only where the table is created fresh. Internal
/// `sqlite_*` tables are never classifiable and never declared, so they are
/// excluded rather than reported.
fn ensure_all_classified(desired: &Connection) -> Result<()> {
    let mut stmt = desired.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for name in names {
        if !MIGRATED_TABLES.contains(&name.as_str()) && !REBUILT_TABLES.contains(&name.as_str()) {
            anyhow::bail!(
                "`{name}` is declared in the schema but listed in neither MIGRATED_TABLES nor \
                 REBUILT_TABLES, so its drift would never be reconciled on existing stores — \
                 classify it before declaring it"
            );
        }
    }
    Ok(())
}

/// Reconcile `table` against the columns `desired` declares for it. Only
/// additive drift is reconcilable in place — SQLite can append a column but not
/// retype or re-key one, and a NOT NULL column needs a default to fill the rows
/// already stored — so anything else is reported rather than half-applied.
///
/// A column already present is compared, not skipped: reconciliation would
/// otherwise pass silently over a retype or a changed default, leaving the store
/// permanently diverged from the declaration it is supposed to converge on.
pub(super) fn reconcile_columns(
    conn: &Connection,
    desired: &Connection,
    table: &str,
) -> Result<()> {
    let present = table_shape(conn, table)?;
    for column in table_shape(desired, table)? {
        if let Some(existing) = present.iter().find(|c| c.name == column.name) {
            if *existing != column {
                anyhow::bail!(
                    "`{table}.{}` is declared as {column:?} but the store holds {existing:?}: \
                     SQLite cannot change a column in place, so this needs an explicit \
                     migration that states what it discards",
                    column.name
                );
            }
            continue;
        }
        let mut ddl = format!(
            "ALTER TABLE {table} ADD COLUMN {} {}",
            column.name, column.decl_type
        );
        match &column.default {
            Some(default) => {
                if column.notnull {
                    ddl.push_str(" NOT NULL");
                }
                ddl.push_str(&format!(" DEFAULT {default}"));
            }
            None if column.notnull => anyhow::bail!(
                "cannot add `{table}.{}` to an existing store: a NOT NULL column needs a \
                 DEFAULT to fill the rows already stored",
                column.name
            ),
            None => {}
        }
        conn.execute_batch(&ddl)?;
    }
    Ok(())
}

/// Recreate every index or view of `kind` whose stored definition differs from
/// the declared one. Both carry no data, so replacing one costs nothing but the
/// rebuild — which is why only a *drifted* object is touched: reindexing a large
/// `events` table on every open would not be free. Objects the declaration does
/// not name are left alone; the store is meant to be explored with ad-hoc SQL, so
/// a helper view someone saved into the file is theirs to keep.
///
/// Deriving the names from the declaration is also what removes the hand-kept
/// list this replaced: such a list drifts, and a declared object missing from it
/// would collide with its own `CREATE` on the store's second open.
fn reconcile_derived(conn: &Connection, desired: &Connection, kind: &str) -> Result<()> {
    let stored = declared_sql(conn, kind)?;
    for (name, sql) in declared_sql(desired, kind)? {
        if stored.get(&name).is_some_and(|existing| *existing == sql) {
            continue;
        }
        conn.execute_batch(&format!("DROP {kind} IF EXISTS {name}"))?;
        conn.execute_batch(&sql)?;
    }
    Ok(())
}

/// `name -> CREATE statement` for every object of `kind` that carries one (an
/// implicit index has none). SQLite stores the statement as written minus
/// `IF NOT EXISTS`, so definitions built from the same declaration compare equal.
pub(super) fn declared_sql(conn: &Connection, kind: &str) -> Result<BTreeMap<String, String>> {
    let mut stmt =
        conn.prepare("SELECT name, sql FROM sqlite_master WHERE type = ?1 AND sql IS NOT NULL")?;
    let rows = stmt
        .query_map([kind], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<BTreeMap<String, String>>>()?;
    Ok(rows)
}

/// One column as SQLite reports it (`PRAGMA table_info`).
#[derive(Debug, PartialEq)]
pub(super) struct ColumnDef {
    pub(super) name: String,
    pub(super) decl_type: String,
    pub(super) notnull: bool,
    pub(super) default: Option<String>,
    pub(super) pk: i64,
}

/// A table's columns in declaration order; empty when the table does not exist.
pub(super) fn table_shape(conn: &Connection, table: &str) -> Result<Vec<ColumnDef>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt
        .query_map([], |row| {
            Ok(ColumnDef {
                name: row.get(1)?,
                decl_type: row.get(2)?,
                notnull: row.get::<_, i64>(3)? != 0,
                default: row.get(4)?,
                pk: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(columns)
}

pub(super) fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get(0),
    )?;
    Ok(n > 0)
}
