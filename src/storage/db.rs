use super::migrations;
use crate::models::{ClipboardEntry, EntryMetadata};
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

pub const PAGE_SIZE: usize = 100;

fn now_timestamp() -> i64 {
    Utc::now().timestamp()
}

pub struct Storage {
    conn: Mutex<Connection>,
}

#[derive(Debug)]
pub enum StoragePathError {
    DataDirNotFound,
    CreateDirFailed(std::io::Error),
}

impl std::fmt::Display for StoragePathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoragePathError::DataDirNotFound => {
                write!(f, "Could not find system data directory")
            }
            StoragePathError::CreateDirFailed(e) => {
                write!(f, "Could not create data directory: {}", e)
            }
        }
    }
}

impl std::error::Error for StoragePathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoragePathError::CreateDirFailed(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum StorageError {
    Path(StoragePathError),
    Database(rusqlite::Error),
    EntryNotFound(Uuid),
    /// The storage mutex was poisoned and the database integrity check failed.
    /// This indicates potentially corrupted database state that cannot be safely recovered.
    IntegrityCheckFailed(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Path(e) => write!(f, "{}", e),
            StorageError::Database(e) => write!(f, "{}", e),
            StorageError::EntryNotFound(id) => write!(f, "Entry not found: {}", id),
            StorageError::IntegrityCheckFailed(msg) => {
                write!(
                    f,
                    "Database integrity check failed after mutex poison recovery: {}",
                    msg
                )
            }
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StorageError::Path(e) => Some(e),
            StorageError::Database(e) => Some(e),
            StorageError::EntryNotFound(_) => None,
            StorageError::IntegrityCheckFailed(_) => None,
        }
    }
}

impl From<StoragePathError> for StorageError {
    fn from(err: StoragePathError) -> Self {
        StorageError::Path(err)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(err: rusqlite::Error) -> Self {
        StorageError::Database(err)
    }
}

impl Storage {
    pub fn db_path() -> std::result::Result<PathBuf, StoragePathError> {
        let data_dir = dirs::data_dir()
            .ok_or(StoragePathError::DataDirNotFound)?
            .join("kopi");

        std::fs::create_dir_all(&data_dir).map_err(StoragePathError::CreateDirFailed)?;

        let db_name = if cfg!(debug_assertions) {
            "kopi.dev.db"
        } else {
            "kopi.db"
        };
        let path = data_dir.join(db_name);
        debug!("Database path: {:?}", path);
        Ok(path)
    }

    pub fn open() -> std::result::Result<Self, StorageError> {
        Self::new(Self::db_path()?)
    }

    fn lock_conn(&self) -> std::result::Result<MutexGuard<'_, Connection>, StorageError> {
        match self.conn.lock() {
            Ok(guard) => Ok(guard),
            Err(poisoned) => {
                error!(
                    "Storage mutex was poisoned (likely a thread panicked while holding the lock). \
                     Attempting recovery with integrity check."
                );
                let guard = poisoned.into_inner();

                // Verify database integrity before allowing continued use.
                // If integrity check fails, we must not continue as the database
                // may be in a corrupted state that could lead to data loss.
                match guard.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0)) {
                    Ok(result) if result == "ok" => {
                        warn!(
                            "Database integrity check passed after mutex poison recovery. \
                             Continuing with recovered connection."
                        );
                        Ok(guard)
                    }
                    Ok(result) => {
                        error!(
                            "Database integrity check FAILED after mutex poison recovery: {}",
                            result
                        );
                        Err(StorageError::IntegrityCheckFailed(result))
                    }
                    Err(e) => {
                        error!(
                            "Failed to run database integrity check after mutex poison recovery: {}",
                            e
                        );
                        Err(StorageError::IntegrityCheckFailed(format!(
                            "Could not run integrity check: {}",
                            e
                        )))
                    }
                }
            }
        }
    }

    pub fn new(path: PathBuf) -> std::result::Result<Self, StorageError> {
        info!("Opening database at {:?}", path);
        let mut conn = Connection::open(&path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;

        migrations::run(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    pub fn new_in_memory() -> std::result::Result<Self, StorageError> {
        let mut conn = Connection::open_in_memory()?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // Note: WAL mode has no effect on in-memory databases but we set
        // synchronous for consistency with the file-based configuration
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        migrations::run(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert_entry(&self, entry: &ClipboardEntry) -> std::result::Result<(), StorageError> {
        let mut conn = self.lock_conn()?;
        debug!("Inserting clipboard entry: {}", entry.id);

        // Use a transaction to ensure atomicity between the main table insert
        // and the FTS trigger. If the FTS trigger fails, the transaction rolls back.
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO clipboard_entries (id, title, content, has_custom_title, is_starred, deleted_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                entry.id.to_string(),
                entry.title,
                entry.content,
                entry.has_custom_title,
                entry.is_starred,
                entry.deleted_at.map(|dt| dt.timestamp()),
                entry.created_at.timestamp(),
                entry.updated_at.timestamp(),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_entry(
        &self,
        id: &Uuid,
    ) -> std::result::Result<Option<ClipboardEntry>, StorageError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, content, has_custom_title, is_starred, deleted_at, created_at, updated_at
             FROM clipboard_entries WHERE id = ?1",
        )?;

        let result = stmt.query_row([id.to_string()], Self::row_to_entry);

        match result {
            Ok(entry) => Ok(Some(entry)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    pub fn get_entries_first_page(&self) -> std::result::Result<Vec<ClipboardEntry>, StorageError> {
        self.get_entries_page(PAGE_SIZE, 0)
    }

    pub fn get_entries_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> std::result::Result<Vec<ClipboardEntry>, StorageError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, content, has_custom_title, is_starred, deleted_at, created_at, updated_at
             FROM clipboard_entries
             WHERE deleted_at IS NULL
             ORDER BY created_at DESC
             LIMIT ?1 OFFSET ?2",
        )?;

        let rows = stmt.query_map([limit as i64, offset as i64], Self::row_to_entry)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    pub fn get_all_entries_metadata(
        &self,
    ) -> std::result::Result<Vec<EntryMetadata>, StorageError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, has_custom_title, is_starred, created_at, updated_at
             FROM clipboard_entries
             WHERE deleted_at IS NULL
             ORDER BY created_at DESC",
        )?;

        let rows = stmt.query_map([], Self::row_to_metadata)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    pub fn get_entries_metadata_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> std::result::Result<Vec<EntryMetadata>, StorageError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, has_custom_title, is_starred, created_at, updated_at
             FROM clipboard_entries
             WHERE deleted_at IS NULL
             ORDER BY created_at DESC
             LIMIT ?1 OFFSET ?2",
        )?;

        let rows = stmt.query_map([limit as i64, offset as i64], Self::row_to_metadata)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    pub fn get_entry_content(
        &self,
        id: &Uuid,
    ) -> std::result::Result<Option<String>, StorageError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare("SELECT content FROM clipboard_entries WHERE id = ?1")?;

        let result = stmt.query_row([id.to_string()], |row| row.get::<_, String>(0));

        match result {
            Ok(content) => Ok(Some(content)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(StorageError::Database(e)),
        }
    }

    pub fn count_entries(&self) -> std::result::Result<usize, StorageError> {
        let conn = self.lock_conn()?;
        conn.query_row(
            "SELECT COUNT(*) FROM clipboard_entries WHERE deleted_at IS NULL",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count as usize)
        .map_err(StorageError::Database)
    }

    pub fn get_starred_entries(&self) -> std::result::Result<Vec<ClipboardEntry>, StorageError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, content, has_custom_title, is_starred, deleted_at, created_at, updated_at
             FROM clipboard_entries
             WHERE deleted_at IS NULL AND is_starred = 1
             ORDER BY created_at DESC",
        )?;

        let rows = stmt.query_map([], Self::row_to_entry)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    pub fn get_recent_entries(&self) -> std::result::Result<Vec<ClipboardEntry>, StorageError> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, content, has_custom_title, is_starred, deleted_at, created_at, updated_at
             FROM clipboard_entries
             WHERE deleted_at IS NULL AND is_starred = 0
             ORDER BY created_at DESC",
        )?;

        let rows = stmt.query_map([], Self::row_to_entry)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    pub fn update_entry(&self, entry: &ClipboardEntry) -> std::result::Result<(), StorageError> {
        let conn = self.lock_conn()?;
        debug!("Updating clipboard entry: {}", entry.id);
        let rows_affected = conn.execute(
            "UPDATE clipboard_entries
             SET title = ?2, content = ?3, has_custom_title = ?4, is_starred = ?5, deleted_at = ?6, updated_at = ?7
             WHERE id = ?1",
            params![
                entry.id.to_string(),
                entry.title,
                entry.content,
                entry.has_custom_title,
                entry.is_starred,
                entry.deleted_at.map(|dt| dt.timestamp()),
                entry.updated_at.timestamp(),
            ],
        )?;
        if rows_affected == 0 {
            return Err(StorageError::EntryNotFound(entry.id));
        }
        Ok(())
    }

    pub fn soft_delete_entry(&self, id: &Uuid) -> std::result::Result<(), StorageError> {
        let conn = self.lock_conn()?;
        let now = now_timestamp();
        debug!("Soft deleting clipboard entry: {}", id);
        let rows_affected = conn.execute(
            "UPDATE clipboard_entries
             SET deleted_at = ?2, updated_at = ?2
             WHERE id = ?1",
            params![id.to_string(), now],
        )?;
        if rows_affected == 0 {
            return Err(StorageError::EntryNotFound(*id));
        }
        Ok(())
    }

    pub fn restore_entry(&self, id: &Uuid) -> std::result::Result<(), StorageError> {
        let conn = self.lock_conn()?;
        let now = now_timestamp();
        debug!("Restoring clipboard entry: {}", id);
        let rows_affected = conn.execute(
            "UPDATE clipboard_entries
             SET deleted_at = NULL, updated_at = ?2
             WHERE id = ?1",
            params![id.to_string(), now],
        )?;
        if rows_affected == 0 {
            return Err(StorageError::EntryNotFound(*id));
        }
        Ok(())
    }

    pub fn toggle_starred(&self, id: &Uuid) -> std::result::Result<bool, StorageError> {
        let mut conn = self.lock_conn()?;
        let now = now_timestamp();
        debug!("Toggling starred status for entry: {}", id);
        let tx = conn.transaction()?;

        let rows_affected = tx.execute(
            "UPDATE clipboard_entries SET is_starred = 1 - is_starred, updated_at = ?2 WHERE id = ?1",
            params![id.to_string(), now],
        )?;

        if rows_affected == 0 {
            tx.rollback()?;
            return Err(StorageError::EntryNotFound(*id));
        }

        let new_starred: bool = tx.query_row(
            "SELECT is_starred FROM clipboard_entries WHERE id = ?1",
            [id.to_string()],
            |row| row.get(0),
        )?;

        tx.commit()?;
        Ok(new_starred)
    }

    pub fn cleanup_old_deleted_entries(&self) -> std::result::Result<usize, StorageError> {
        let cutoff = Utc::now() - chrono::Duration::hours(24);
        self.cleanup_deleted_entries_before(cutoff)
    }

    #[cfg(test)]
    pub fn cleanup_deleted_entries_before(
        &self,
        cutoff: DateTime<Utc>,
    ) -> std::result::Result<usize, StorageError> {
        self.cleanup_deleted_entries_before_impl(cutoff)
    }

    #[cfg(not(test))]
    fn cleanup_deleted_entries_before(
        &self,
        cutoff: DateTime<Utc>,
    ) -> std::result::Result<usize, StorageError> {
        self.cleanup_deleted_entries_before_impl(cutoff)
    }

    fn cleanup_deleted_entries_before_impl(
        &self,
        cutoff: DateTime<Utc>,
    ) -> std::result::Result<usize, StorageError> {
        let conn = self.lock_conn()?;
        let cutoff_ts = cutoff.timestamp();
        debug!("Cleaning up entries deleted before: {}", cutoff_ts);

        let count = conn.execute(
            "DELETE FROM clipboard_entries
             WHERE deleted_at IS NOT NULL AND deleted_at < ?1",
            [cutoff_ts],
        )?;

        if count > 0 {
            info!("Permanently deleted {} old entries", count);
        }
        Ok(count)
    }

    pub fn clear_all_deleted_entries(&self) -> std::result::Result<usize, StorageError> {
        let conn = self.lock_conn()?;
        debug!("Clearing all soft-deleted entries");

        let count = conn.execute(
            "DELETE FROM clipboard_entries WHERE deleted_at IS NOT NULL",
            [],
        )?;

        if count > 0 {
            info!("Permanently deleted {} entries", count);
        }
        Ok(count)
    }

    pub fn rename_entry(
        &self,
        id: &Uuid,
        new_title: &str,
    ) -> std::result::Result<(), StorageError> {
        const MAX_TITLE_LENGTH: usize = 255;
        let title = if new_title.len() > MAX_TITLE_LENGTH {
            warn!(
                "Title exceeds {} chars, truncating for entry {}",
                MAX_TITLE_LENGTH, id
            );
            &new_title[..new_title
                .char_indices()
                .take(MAX_TITLE_LENGTH)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0)]
        } else {
            new_title
        };

        let conn = self.lock_conn()?;
        let now = now_timestamp();
        debug!("Renaming entry {} to: {}", id, title);
        let rows_affected = conn.execute(
            "UPDATE clipboard_entries
             SET title = ?2, has_custom_title = 1, updated_at = ?3
             WHERE id = ?1",
            params![id.to_string(), title, now],
        )?;
        if rows_affected == 0 {
            return Err(StorageError::EntryNotFound(*id));
        }
        Ok(())
    }

    pub fn update_content(
        &self,
        id: &Uuid,
        new_content: &str,
        regenerate_title: bool,
    ) -> std::result::Result<(), StorageError> {
        let conn = self.lock_conn()?;
        let now = now_timestamp();
        debug!("Updating content for entry: {}", id);

        let rows_affected = if regenerate_title {
            let new_title = crate::utils::generate_title(new_content);
            conn.execute(
                "UPDATE clipboard_entries
                 SET content = ?2, title = ?3, updated_at = ?4
                 WHERE id = ?1",
                params![id.to_string(), new_content, new_title, now],
            )?
        } else {
            conn.execute(
                "UPDATE clipboard_entries
                 SET content = ?2, updated_at = ?3
                 WHERE id = ?1",
                params![id.to_string(), new_content, now],
            )?
        };
        if rows_affected == 0 {
            return Err(StorageError::EntryNotFound(*id));
        }
        Ok(())
    }

    fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<ClipboardEntry> {
        let id_str: String = row.get("id")?;
        let deleted_at_ts: Option<i64> = row.get("deleted_at")?;
        let created_at_ts: i64 = row.get("created_at")?;
        let updated_at_ts: i64 = row.get("updated_at")?;

        let id = Uuid::parse_str(&id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;

        let deleted_at = deleted_at_ts.and_then(|ts| DateTime::from_timestamp(ts, 0));

        let created_at = DateTime::from_timestamp(created_at_ts, 0).unwrap_or_else(|| {
            warn!(
                "Invalid created_at timestamp {} for entry {}, using current time",
                created_at_ts, id_str
            );
            Utc::now()
        });

        let updated_at = DateTime::from_timestamp(updated_at_ts, 0).unwrap_or_else(|| {
            warn!(
                "Invalid updated_at timestamp {} for entry {}, using current time",
                updated_at_ts, id_str
            );
            Utc::now()
        });

        Ok(ClipboardEntry {
            id,
            title: row.get("title")?,
            content: row.get("content")?,
            has_custom_title: row.get("has_custom_title")?,
            is_starred: row.get("is_starred")?,
            deleted_at,
            created_at,
            updated_at,
        })
    }

    fn row_to_metadata(row: &rusqlite::Row) -> rusqlite::Result<EntryMetadata> {
        let id_str: String = row.get("id")?;
        let created_at_ts: i64 = row.get("created_at")?;
        let updated_at_ts: i64 = row.get("updated_at")?;

        let id = Uuid::parse_str(&id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;

        let created_at = DateTime::from_timestamp(created_at_ts, 0).unwrap_or_else(|| {
            warn!(
                "Invalid created_at timestamp {} for entry {}, using current time",
                created_at_ts, id_str
            );
            Utc::now()
        });

        let updated_at = DateTime::from_timestamp(updated_at_ts, 0).unwrap_or_else(|| {
            warn!(
                "Invalid updated_at timestamp {} for entry {}, using current time",
                updated_at_ts, id_str
            );
            Utc::now()
        });

        Ok(EntryMetadata {
            id,
            title: row.get("title")?,
            has_custom_title: row.get("has_custom_title")?,
            is_starred: row.get("is_starred")?,
            created_at,
            updated_at,
        })
    }

    pub fn search_entries(
        &self,
        query: &str,
    ) -> std::result::Result<Vec<EntryMetadata>, StorageError> {
        let trimmed = query.trim();

        // Empty query returns all non-deleted entries
        if trimmed.is_empty() {
            return self.get_all_entries_metadata();
        }

        let conn = self.lock_conn()?;

        // Sanitize query for FTS5: escape special characters and wrap terms in quotes
        let sanitized = Self::sanitize_fts_query(trimmed);

        debug!("FTS search query: {:?} -> {:?}", query, sanitized);

        let mut stmt = conn.prepare(
            "SELECT e.id, e.title, e.has_custom_title, e.is_starred, e.created_at, e.updated_at
             FROM clipboard_entries e
             INNER JOIN clipboard_entries_fts fts ON e.rowid = fts.rowid
             WHERE clipboard_entries_fts MATCH ?1 AND e.deleted_at IS NULL
             ORDER BY bm25(clipboard_entries_fts, 10.0, 1.0) ASC",
        )?;

        let rows = stmt.query_map([&sanitized], Self::row_to_metadata)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Database)
    }

    fn sanitize_fts_query(query: &str) -> String {
        // Split on whitespace and wrap each non-empty term in quotes
        // This handles most FTS5 special characters (AND, OR, NOT, *, etc.)
        let terms: Vec<String> = query
            .split_whitespace()
            .filter(|term| !term.is_empty())
            .map(|term| {
                // Escape internal double quotes by doubling them
                let escaped = term.replace('"', "\"\"");
                format!("\"{}\"*", escaped)
            })
            .collect();

        terms.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_entry(content: &str) -> ClipboardEntry {
        ClipboardEntry::new(content.to_string())
    }

    #[test]
    fn insert_entry_and_get_entry() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");
        let entry = create_test_entry("Test clipboard content");

        storage
            .insert_entry(&entry)
            .expect("Failed to insert entry");

        let retrieved = storage
            .get_entry(&entry.id)
            .expect("Failed to get entry")
            .expect("Entry not found");

        assert_eq!(retrieved.id, entry.id);
        assert_eq!(retrieved.content, entry.content);
        assert_eq!(retrieved.title, entry.title);
        assert!(!retrieved.is_deleted());
        assert!(!retrieved.is_starred);
    }

    #[test]
    fn update_entry_modifies_fields() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");
        let mut entry = create_test_entry("Original content");

        storage
            .insert_entry(&entry)
            .expect("Failed to insert entry");

        entry.content = "Updated content".to_string();
        entry.title = "Updated title".to_string();
        entry.is_starred = true;

        storage
            .update_entry(&entry)
            .expect("Failed to update entry");

        let retrieved = storage
            .get_entry(&entry.id)
            .expect("Failed to get entry")
            .expect("Entry not found");

        assert_eq!(retrieved.content, "Updated content");
        assert_eq!(retrieved.title, "Updated title");
        assert!(retrieved.is_starred);
    }

    #[test]
    fn soft_delete_and_restore_entry() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");
        let entry = create_test_entry("Content to delete");

        storage
            .insert_entry(&entry)
            .expect("Failed to insert entry");

        storage
            .soft_delete_entry(&entry.id)
            .expect("Failed to soft delete entry");

        let deleted_entry = storage
            .get_entry(&entry.id)
            .expect("Failed to get entry")
            .expect("Entry not found");

        assert!(deleted_entry.is_deleted());
        assert!(deleted_entry.deleted_at.is_some());

        let all_entries = storage
            .get_entries_first_page()
            .expect("Failed to get all entries");
        assert!(all_entries.iter().all(|e| e.id != entry.id));

        storage
            .restore_entry(&entry.id)
            .expect("Failed to restore entry");

        let restored_entry = storage
            .get_entry(&entry.id)
            .expect("Failed to get entry")
            .expect("Entry not found");

        assert!(!restored_entry.is_deleted());
        assert!(restored_entry.deleted_at.is_none());

        let all_entries = storage
            .get_entries_first_page()
            .expect("Failed to get all entries");
        assert!(all_entries.iter().any(|e| e.id == entry.id));
    }

    #[test]
    fn get_entry_returns_none_for_nonexistent_id() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");
        let fake_id = Uuid::new_v4();

        let result = storage.get_entry(&fake_id).expect("Failed to query entry");

        assert!(result.is_none());
    }

    #[test]
    fn toggle_starred_flips_status() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");
        let entry = create_test_entry("Content to star");

        storage
            .insert_entry(&entry)
            .expect("Failed to insert entry");

        let retrieved = storage
            .get_entry(&entry.id)
            .expect("Failed to get entry")
            .expect("Entry not found");
        assert!(!retrieved.is_starred);

        let new_status = storage
            .toggle_starred(&entry.id)
            .expect("Failed to toggle starred");
        assert!(new_status);

        let retrieved = storage
            .get_entry(&entry.id)
            .expect("Failed to get entry")
            .expect("Entry not found");
        assert!(retrieved.is_starred);

        let new_status = storage
            .toggle_starred(&entry.id)
            .expect("Failed to toggle starred");
        assert!(!new_status);

        let retrieved = storage
            .get_entry(&entry.id)
            .expect("Failed to get entry")
            .expect("Entry not found");
        assert!(!retrieved.is_starred);
    }

    #[test]
    fn clear_all_deleted_entries_removes_only_deleted() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");

        let entry1 = create_test_entry("Entry 1 - will be deleted");
        let entry2 = create_test_entry("Entry 2 - will be deleted");
        let entry3 = create_test_entry("Entry 3 - not deleted");

        storage.insert_entry(&entry1).expect("Failed to insert");
        storage.insert_entry(&entry2).expect("Failed to insert");
        storage.insert_entry(&entry3).expect("Failed to insert");

        storage
            .soft_delete_entry(&entry1.id)
            .expect("Failed to soft delete");
        storage
            .soft_delete_entry(&entry2.id)
            .expect("Failed to soft delete");

        assert!(storage.get_entry(&entry1.id).unwrap().is_some());
        assert!(storage.get_entry(&entry2.id).unwrap().is_some());
        assert!(storage.get_entry(&entry3.id).unwrap().is_some());

        let count = storage
            .clear_all_deleted_entries()
            .expect("Failed to clear deleted");
        assert_eq!(count, 2);

        assert!(storage.get_entry(&entry1.id).unwrap().is_none());
        assert!(storage.get_entry(&entry2.id).unwrap().is_none());
        assert!(storage.get_entry(&entry3.id).unwrap().is_some());
    }

    #[test]
    fn clear_all_deleted_entries_returns_zero_when_none_deleted() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");

        let entry = create_test_entry("Not deleted entry");
        storage.insert_entry(&entry).expect("Failed to insert");

        let count = storage
            .clear_all_deleted_entries()
            .expect("Failed to clear deleted");
        assert_eq!(count, 0);

        assert!(storage.get_entry(&entry.id).unwrap().is_some());
    }

    #[test]
    fn cleanup_deleted_entries_respects_24_hour_threshold() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");

        let entry1 = create_test_entry("Entry 1 - recently deleted");
        let entry2 = create_test_entry("Entry 2 - old deletion");
        let entry3 = create_test_entry("Entry 3 - not deleted");

        storage.insert_entry(&entry1).expect("Failed to insert");
        storage.insert_entry(&entry2).expect("Failed to insert");
        storage.insert_entry(&entry3).expect("Failed to insert");

        let before_deletion = Utc::now();

        storage
            .soft_delete_entry(&entry1.id)
            .expect("Failed to soft delete");
        storage
            .soft_delete_entry(&entry2.id)
            .expect("Failed to soft delete");

        let cutoff_past = before_deletion - chrono::Duration::hours(1);
        let count = storage
            .cleanup_deleted_entries_before(cutoff_past)
            .expect("Failed to cleanup");
        assert_eq!(count, 0);

        assert!(storage.get_entry(&entry1.id).unwrap().is_some());
        assert!(storage.get_entry(&entry2.id).unwrap().is_some());

        let cutoff_future = Utc::now() + chrono::Duration::hours(25);
        let count = storage
            .cleanup_deleted_entries_before(cutoff_future)
            .expect("Failed to cleanup");
        assert_eq!(count, 2);

        assert!(storage.get_entry(&entry1.id).unwrap().is_none());
        assert!(storage.get_entry(&entry2.id).unwrap().is_none());
        assert!(storage.get_entry(&entry3.id).unwrap().is_some());
    }

    #[test]
    fn get_entries_page_returns_correct_offset_and_limit() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");

        let entries: Vec<_> = (0..5)
            .map(|i| create_test_entry(&format!("Entry {}", i)))
            .collect();

        for entry in &entries {
            storage.insert_entry(entry).expect("Failed to insert");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let page1 = storage.get_entries_page(2, 0).expect("Failed to get page");
        assert_eq!(page1.len(), 2);

        let page2 = storage.get_entries_page(2, 2).expect("Failed to get page");
        assert_eq!(page2.len(), 2);

        let page3 = storage.get_entries_page(2, 4).expect("Failed to get page");
        assert_eq!(page3.len(), 1);

        let empty = storage.get_entries_page(2, 10).expect("Failed to get page");
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn count_entries_returns_correct_count() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");

        assert_eq!(storage.count_entries().unwrap(), 0);

        let entry1 = create_test_entry("Entry 1");
        let entry2 = create_test_entry("Entry 2");

        storage.insert_entry(&entry1).expect("Failed to insert");
        assert_eq!(storage.count_entries().unwrap(), 1);

        storage.insert_entry(&entry2).expect("Failed to insert");
        assert_eq!(storage.count_entries().unwrap(), 2);

        storage
            .soft_delete_entry(&entry1.id)
            .expect("Failed to soft delete");
        assert_eq!(storage.count_entries().unwrap(), 1);
    }

    #[test]
    fn search_entries_empty_query_returns_all() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");

        let entry1 = create_test_entry("Hello world");
        let entry2 = create_test_entry("Goodbye moon");

        storage.insert_entry(&entry1).expect("Failed to insert");
        storage.insert_entry(&entry2).expect("Failed to insert");

        let results = storage.search_entries("").expect("Failed to search");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_entries_finds_by_content() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");

        let entry1 = create_test_entry("The quick brown fox jumps over the lazy dog");
        let entry2 = create_test_entry("Hello world");
        let entry3 = create_test_entry("Goodbye moon");

        storage.insert_entry(&entry1).expect("Failed to insert");
        storage.insert_entry(&entry2).expect("Failed to insert");
        storage.insert_entry(&entry3).expect("Failed to insert");

        let results = storage.search_entries("fox").expect("Failed to search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, entry1.id);

        let results = storage.search_entries("hello").expect("Failed to search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, entry2.id);
    }

    #[test]
    fn search_entries_case_insensitive() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");

        let entry = create_test_entry("Hello World");
        storage.insert_entry(&entry).expect("Failed to insert");

        let results = storage.search_entries("hello").expect("Failed to search");
        assert_eq!(results.len(), 1);

        let results = storage.search_entries("HELLO").expect("Failed to search");
        assert_eq!(results.len(), 1);

        let results = storage.search_entries("HeLLo").expect("Failed to search");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_entries_excludes_deleted() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");

        let entry1 = create_test_entry("Hello world");
        let entry2 = create_test_entry("Hello universe");

        storage.insert_entry(&entry1).expect("Failed to insert");
        storage.insert_entry(&entry2).expect("Failed to insert");

        let results = storage.search_entries("hello").expect("Failed to search");
        assert_eq!(results.len(), 2);

        storage
            .soft_delete_entry(&entry1.id)
            .expect("Failed to soft delete");

        let results = storage.search_entries("hello").expect("Failed to search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, entry2.id);
    }

    #[test]
    fn search_entries_prefix_matching() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");

        let entry = create_test_entry("Programming in Rust is fun");
        storage.insert_entry(&entry).expect("Failed to insert");

        let results = storage.search_entries("prog").expect("Failed to search");
        assert_eq!(results.len(), 1);

        let results = storage.search_entries("ru").expect("Failed to search");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_entries_multiple_terms() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");

        let entry1 = create_test_entry("The quick brown fox");
        let entry2 = create_test_entry("The slow brown dog");
        let entry3 = create_test_entry("The quick yellow cat");

        storage.insert_entry(&entry1).expect("Failed to insert");
        storage.insert_entry(&entry2).expect("Failed to insert");
        storage.insert_entry(&entry3).expect("Failed to insert");

        let results = storage
            .search_entries("quick brown")
            .expect("Failed to search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, entry1.id);
    }

    #[test]
    fn search_entries_handles_special_characters() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");

        let entry = create_test_entry("Test content with special chars: AND OR NOT");
        storage.insert_entry(&entry).expect("Failed to insert");

        let results = storage.search_entries("AND").expect("Failed to search");
        assert_eq!(results.len(), 1);

        let results = storage.search_entries("OR").expect("Failed to search");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_entries_updates_fts_on_content_change() {
        let storage = Storage::new_in_memory().expect("Failed to create in-memory storage");

        let mut entry = create_test_entry("Original content apple");
        storage.insert_entry(&entry).expect("Failed to insert");

        let results = storage.search_entries("apple").expect("Failed to search");
        assert_eq!(results.len(), 1);

        entry.content = "Updated content banana".to_string();
        entry.title = "Updated content banana".to_string();
        storage.update_entry(&entry).expect("Failed to update");

        let results = storage.search_entries("apple").expect("Failed to search");
        assert_eq!(results.len(), 0);

        let results = storage.search_entries("banana").expect("Failed to search");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn sanitize_fts_query_wraps_terms() {
        assert_eq!(Storage::sanitize_fts_query("hello"), "\"hello\"*");
        assert_eq!(
            Storage::sanitize_fts_query("hello world"),
            "\"hello\"* \"world\"*"
        );
    }

    #[test]
    fn sanitize_fts_query_escapes_quotes() {
        assert_eq!(
            Storage::sanitize_fts_query("say \"hello\""),
            "\"say\"* \"\"\"hello\"\"\"*"
        );
    }

    #[test]
    fn sanitize_fts_query_handles_whitespace() {
        assert_eq!(Storage::sanitize_fts_query("  hello  "), "\"hello\"*");
        assert_eq!(
            Storage::sanitize_fts_query("hello   world"),
            "\"hello\"* \"world\"*"
        );
    }
}
