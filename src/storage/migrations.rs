use include_dir::{Dir, include_dir};
use rusqlite::{Connection, Result};

static MIGRATIONS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/storage/migrations");

/// Runs all pending database migrations.
///
/// # Limitations
///
/// This migration system is forward-only and does not support rollback/downgrade operations.
/// If a migration needs to be reverted, you must create a new migration that undoes the changes.
/// For development, you can delete the database file to start fresh.
pub fn run(conn: &mut Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            name TEXT PRIMARY KEY,
            applied_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        )",
        [],
    )?;

    let mut migrations: Vec<_> = MIGRATIONS_DIR
        .files()
        .filter(|f| f.path().extension().is_some_and(|ext| ext == "sql"))
        .collect();

    migrations.sort_by_key(|f| f.path());

    for file in migrations {
        let name = file
            .path()
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| rusqlite::Error::InvalidPath(file.path().to_path_buf()))?;

        let already_applied: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE name = ?1)",
            [name],
            |row| row.get(0),
        )?;

        if !already_applied {
            let sql = std::str::from_utf8(file.contents()).map_err(rusqlite::Error::Utf8Error)?;

            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute("INSERT INTO schema_migrations (name) VALUES (?1)", [name])?;
            tx.commit()?;

            log::info!("Applied migration: {}", name);
        }
    }

    Ok(())
}
