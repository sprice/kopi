use crate::models::{ClipboardEntry, EntryMetadata};
use crate::search::{self, OwnedSearchResult, SearchResult, SubstringMatcher};
use crate::storage::{PAGE_SIZE, Storage};
use crate::ui::theme::{SIDEBAR_DEFAULT_WIDTH_F32, SIDEBAR_MAX_WIDTH_F32, SIDEBAR_MIN_WIDTH_F32};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub const UNDO_DURATION_SECS: u64 = 5;

pub struct AppState {
    pub entries: IndexMap<Uuid, EntryMetadata>,
    pub selected_entry_id: Option<Uuid>,
    pub selected_content: Option<String>,
    pub sidebar_visible: bool,
    pub sidebar_width: f32,
    pub pending_deletes: HashMap<Uuid, DateTime<Utc>>,
    pub has_more_entries: bool,
    pub loading_more: bool,
    pub search_query: String,
    fts_results: Option<Vec<OwnedSearchResult>>,
    storage: Arc<Storage>,
    matcher: SubstringMatcher,
}

impl AppState {
    pub fn new(storage: Arc<Storage>) -> Self {
        let entries = match storage.get_all_entries_metadata() {
            Ok(metadata_vec) => {
                info!("Loaded {} entry metadata from database", metadata_vec.len());
                metadata_vec.into_iter().map(|m| (m.id, m)).collect()
            }
            Err(e) => {
                error!("Failed to load entries: {}", e);
                IndexMap::new()
            }
        };

        let selected_entry_id = entries.first().map(|(id, _)| *id);

        let selected_content =
            selected_entry_id.and_then(|id| match storage.get_entry_content(&id) {
                Ok(content) => content,
                Err(e) => {
                    error!("Failed to load content for entry {}: {}", id, e);
                    None
                }
            });

        Self {
            entries,
            selected_entry_id,
            selected_content,
            sidebar_visible: true,
            sidebar_width: SIDEBAR_DEFAULT_WIDTH_F32,
            pending_deletes: HashMap::new(),
            has_more_entries: false,
            loading_more: false,
            search_query: String::new(),
            fts_results: None,
            storage,
            matcher: SubstringMatcher,
        }
    }

    pub fn load_more_entries(&mut self) {
        if self.loading_more || !self.has_more_entries {
            return;
        }

        self.loading_more = true;
        let offset = self.entries.len();

        match self.storage.get_entries_metadata_page(PAGE_SIZE, offset) {
            Ok(new_entries) => {
                info!(
                    "Loaded {} more entry metadata (offset {})",
                    new_entries.len(),
                    offset
                );
                self.has_more_entries = new_entries.len() >= PAGE_SIZE;
                for metadata in new_entries {
                    self.entries.insert(metadata.id, metadata);
                }
            }
            Err(e) => {
                error!("Failed to load more entries: {}", e);
            }
        }

        self.loading_more = false;
    }

    pub fn add_entry(&mut self, entry: ClipboardEntry) {
        debug!("Adding new entry: {} - {}", entry.id, entry.title);
        let id = entry.id;
        let content = entry.content.clone();
        let metadata = entry.to_metadata();
        self.entries.shift_insert(0, id, metadata);

        if self.selected_entry_id == Some(id) {
            self.selected_content = Some(content);
        }
    }

    pub fn selected_entry(&self) -> Option<&EntryMetadata> {
        self.selected_entry_id.and_then(|id| self.entries.get(&id))
    }

    pub fn selected_entry_mut(&mut self) -> Option<&mut EntryMetadata> {
        self.selected_entry_id
            .and_then(|id| self.entries.get_mut(&id))
    }

    pub fn selected_entry_content(&self) -> Option<&str> {
        self.selected_content.as_deref()
    }

    pub fn select_entry(&mut self, id: Uuid) {
        if self.entries.contains_key(&id) && self.selected_entry_id != Some(id) {
            self.selected_entry_id = Some(id);
            match self.storage.get_entry_content(&id) {
                Ok(content) => {
                    self.selected_content = content;
                }
                Err(e) => {
                    error!("Failed to load content for entry {}: {}", id, e);
                    self.selected_content = None;
                }
            }
            debug!("Selected entry: {}", id);
        }
    }

    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query.clone();
        debug!("Search query updated: {:?}", self.search_query);

        if query.trim().is_empty() {
            self.fts_results = None;
        } else {
            match search::search_entries_fts(&self.storage, &query) {
                Ok(results) => {
                    debug!("FTS search returned {} results", results.len());
                    self.fts_results = Some(results);
                }
                Err(e) => {
                    warn!("FTS search failed, falling back to in-memory: {}", e);
                    self.fts_results = None;
                }
            }
        }
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.fts_results = None;
    }

    pub fn is_searching(&self) -> bool {
        !self.search_query.is_empty()
    }

    pub fn search_results(&self) -> Vec<SearchResult<'_>> {
        search::filter_entries(self.entries.values(), &self.search_query, &self.matcher)
    }

    pub fn partitioned_entries(&self) -> (Vec<&EntryMetadata>, Vec<&EntryMetadata>) {
        if self.is_searching() {
            if let Some(ref fts_results) = self.fts_results {
                let (starred, recent): (Vec<_>, Vec<_>) =
                    fts_results.iter().partition(|r| r.entry.is_starred);
                return (
                    starred.into_iter().map(|r| &r.entry).collect(),
                    recent.into_iter().map(|r| &r.entry).collect(),
                );
            }

            let results = self.search_results();
            let (starred, recent): (Vec<_>, Vec<_>) =
                results.into_iter().partition(|r| r.entry.is_starred);
            (
                starred.into_iter().map(|r| r.entry).collect(),
                recent.into_iter().map(|r| r.entry).collect(),
            )
        } else {
            let (starred, recent): (Vec<_>, Vec<_>) =
                self.entries.values().partition(|e| e.is_starred);
            (starred, recent)
        }
    }

    pub fn toggle_sidebar(&mut self) {
        self.sidebar_visible = !self.sidebar_visible;
        debug!("Sidebar visible: {}", self.sidebar_visible);
    }

    pub fn set_sidebar_width(&mut self, width: f32) {
        self.sidebar_width = width.clamp(SIDEBAR_MIN_WIDTH_F32, SIDEBAR_MAX_WIDTH_F32);
    }

    pub fn update_entry_content(&mut self, id: Uuid, content: String) {
        if let Some(metadata) = self.entries.get_mut(&id) {
            let regenerate_title = !metadata.has_custom_title;

            let old_title = metadata.title.clone();
            let old_updated_at = metadata.updated_at;

            if regenerate_title {
                metadata.title = ClipboardEntry::generate_title(&content);
            }
            metadata.updated_at = chrono::Utc::now();

            if let Err(e) = self.storage.update_content(&id, &content, regenerate_title) {
                error!("Failed to update entry content: {}", e);
                metadata.title = old_title;
                metadata.updated_at = old_updated_at;
                return;
            }

            if self.selected_entry_id == Some(id) {
                self.selected_content = Some(content);
            }
        }
    }

    pub fn update_entry_title(&mut self, id: Uuid, title: String) {
        if let Some(metadata) = self.entries.get_mut(&id) {
            let old_title = metadata.title.clone();
            let old_has_custom_title = metadata.has_custom_title;
            let old_updated_at = metadata.updated_at;

            metadata.title = title.clone();
            metadata.has_custom_title = true;
            metadata.updated_at = chrono::Utc::now();

            if let Err(e) = self.storage.rename_entry(&id, &title) {
                error!("Failed to update entry title: {}", e);
                metadata.title = old_title;
                metadata.has_custom_title = old_has_custom_title;
                metadata.updated_at = old_updated_at;
            }
        }
    }

    pub fn reset_entry_title(&mut self, id: Uuid) {
        if !self.entries.contains_key(&id) {
            debug!("Cannot reset title: entry {} not found in memory", id);
            return;
        }

        let content = if self.selected_entry_id == Some(id) {
            self.selected_content.clone()
        } else {
            match self.storage.get_entry_content(&id) {
                Ok(content) => content,
                Err(e) => {
                    error!("Failed to get content for title reset: {}", e);
                    return;
                }
            }
        };

        let Some(content) = content else {
            debug!("Cannot reset title: no content found for entry {}", id);
            return;
        };

        let new_title = ClipboardEntry::generate_title(&content);

        let Some(metadata) = self.entries.get_mut(&id) else {
            debug!(
                "Cannot reset title: entry {} was removed during content fetch",
                id
            );
            return;
        };

        let old_title = metadata.title.clone();
        let old_has_custom_title = metadata.has_custom_title;
        let old_updated_at = metadata.updated_at;

        metadata.title = new_title.clone();
        metadata.has_custom_title = false;
        metadata.updated_at = chrono::Utc::now();

        if let Err(e) = self.storage.update_content(&id, &content, true) {
            error!("Failed to reset entry title: {}", e);
            metadata.title = old_title;
            metadata.has_custom_title = old_has_custom_title;
            metadata.updated_at = old_updated_at;
        }
    }

    pub fn toggle_starred(&mut self, id: Uuid) {
        if let Some(metadata) = self.entries.get_mut(&id) {
            let old_updated_at = metadata.updated_at;

            metadata.is_starred = !metadata.is_starred;
            metadata.updated_at = chrono::Utc::now();

            if let Err(e) = self.storage.toggle_starred(&id) {
                error!("Failed to toggle starred status: {}", e);
                metadata.is_starred = !metadata.is_starred;
                metadata.updated_at = old_updated_at;
            }

            debug!("Toggled starred for {}: {}", id, metadata.is_starred);
        }
    }

    pub fn soft_delete(&mut self, id: Uuid) {
        if self.entries.contains_key(&id) {
            if let Err(e) = self.storage.soft_delete_entry(&id) {
                error!("Failed to soft delete entry: {}", e);
                return;
            }

            self.pending_deletes.insert(id, Utc::now());
            info!("Soft deleted entry: {}", id);

            if self.selected_entry_id == Some(id) {
                let next_entry = self
                    .entries
                    .iter()
                    .find(|(entry_id, _)| **entry_id != id)
                    .map(|(entry_id, _)| *entry_id);
                self.selected_entry_id = next_entry;
                if let Some(new_id) = self.selected_entry_id {
                    match self.storage.get_entry_content(&new_id) {
                        Ok(content) => self.selected_content = content,
                        Err(e) => {
                            error!("Failed to load content for entry {}: {}", new_id, e);
                            self.selected_content = None;
                        }
                    }
                } else {
                    self.selected_content = None;
                }
            }
        }
    }

    pub fn undo_delete(&mut self, entry_id: Uuid) -> bool {
        if let Some(delete_time) = self.pending_deletes.remove(&entry_id)
            && (Utc::now() - delete_time).num_seconds() < UNDO_DURATION_SECS as i64
        {
            if let Err(e) = self.storage.restore_entry(&entry_id) {
                error!("Failed to restore entry: {}", e);
                self.pending_deletes.insert(entry_id, delete_time);
                return false;
            }

            if let Ok(Some(entry)) = self.storage.get_entry(&entry_id) {
                self.selected_entry_id = Some(entry_id);
                self.selected_content = Some(entry.content);
                info!("Restored entry: {}", entry_id);
                return true;
            }
        }

        false
    }

    pub fn is_pending_delete(&self, entry_id: Uuid) -> bool {
        self.pending_deletes.contains_key(&entry_id)
    }

    pub fn clear_pending_delete(&mut self, entry_id: Uuid) {
        if self.pending_deletes.remove(&entry_id).is_some() {
            self.entries.shift_remove(&entry_id);
        }
    }

    pub fn cleanup_old_entries(&self) {
        match self.storage.cleanup_old_deleted_entries() {
            Ok(count) if count > 0 => {
                info!("Cleaned up {} old deleted entries", count);
            }
            Ok(_) => {
                debug!("No old entries to clean up");
            }
            Err(e) => {
                error!("Failed to cleanup old entries: {}", e);
            }
        }
    }

    pub fn reload_entries(&mut self) {
        match self.storage.get_all_entries_metadata() {
            Ok(metadata_vec) => {
                self.has_more_entries = metadata_vec.len() >= PAGE_SIZE;
                self.entries = metadata_vec.into_iter().map(|m| (m.id, m)).collect();
                info!(
                    "Reloaded {} entry metadata from database",
                    self.entries.len()
                );

                if let Some(id) = self.selected_entry_id
                    && !self.entries.contains_key(&id)
                {
                    self.selected_entry_id = self.entries.first().map(|(entry_id, _)| *entry_id);
                }

                if let Some(id) = self.selected_entry_id {
                    match self.storage.get_entry_content(&id) {
                        Ok(content) => self.selected_content = content,
                        Err(e) => {
                            error!("Failed to load content for entry {}: {}", id, e);
                            self.selected_content = None;
                        }
                    }
                } else {
                    self.selected_content = None;
                }
            }
            Err(e) => {
                error!("Failed to reload entries: {}", e);
            }
        }
    }

    pub fn storage(&self) -> &Arc<Storage> {
        &self.storage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ClipboardEntry;

    fn create_test_storage() -> Arc<Storage> {
        Arc::new(Storage::new_in_memory().expect("Failed to create in-memory storage"))
    }

    fn create_test_entry(content: &str) -> ClipboardEntry {
        ClipboardEntry::new(content.to_string())
    }

    fn create_starred_entry(content: &str) -> ClipboardEntry {
        let mut entry = ClipboardEntry::new(content.to_string());
        entry.toggle_starred();
        entry
    }

    #[test]
    fn new_app_state_initializes_with_empty_entries() {
        let storage = create_test_storage();
        let state = AppState::new(storage);

        assert!(state.entries.is_empty());
        assert!(state.selected_entry_id.is_none());
        assert!(state.selected_content.is_none());
        assert!(state.sidebar_visible);
        assert_eq!(state.sidebar_width, SIDEBAR_DEFAULT_WIDTH_F32);
        assert!(state.pending_deletes.is_empty());
        assert!(!state.has_more_entries);
        assert!(!state.loading_more);
        assert!(state.search_query.is_empty());
    }

    #[test]
    fn new_app_state_loads_existing_entries() {
        let storage = create_test_storage();
        let entry = create_test_entry("Existing entry");
        storage.insert_entry(&entry).expect("Failed to insert");

        let state = AppState::new(Arc::clone(&storage));

        assert_eq!(state.entries.len(), 1);
        assert!(state.entries.contains_key(&entry.id));
        assert_eq!(state.selected_entry_id, Some(entry.id));
        assert_eq!(state.selected_content, Some("Existing entry".to_string()));
    }

    #[test]
    fn new_app_state_selects_first_entry() {
        let storage = create_test_storage();

        let entry1 = create_test_entry("Entry 1");
        storage.insert_entry(&entry1).expect("Failed to insert");

        let state = AppState::new(Arc::clone(&storage));

        assert_eq!(state.selected_entry_id, Some(entry1.id));
        assert_eq!(state.selected_content, Some("Entry 1".to_string()));
    }

    #[test]
    fn add_entry_inserts_at_front() {
        let storage = create_test_storage();
        let mut state = AppState::new(Arc::clone(&storage));

        let entry1 = create_test_entry("First entry");
        let entry2 = create_test_entry("Second entry");

        state.add_entry(entry1.clone());
        state.add_entry(entry2.clone());

        let first_id = state.entries.first().map(|(id, _)| *id);
        assert_eq!(first_id, Some(entry2.id));
    }

    #[test]
    fn add_entry_updates_selected_content_if_selected() {
        let storage = create_test_storage();
        let mut state = AppState::new(Arc::clone(&storage));

        let entry = create_test_entry("New entry content");
        state.selected_entry_id = Some(entry.id);

        state.add_entry(entry);

        assert_eq!(
            state.selected_content,
            Some("New entry content".to_string())
        );
    }

    #[test]
    fn select_entry_updates_selection() {
        let storage = create_test_storage();
        let entry = create_test_entry("Test content");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.selected_entry_id = None;
        state.selected_content = None;

        state.select_entry(entry.id);

        assert_eq!(state.selected_entry_id, Some(entry.id));
        assert_eq!(state.selected_content, Some("Test content".to_string()));
    }

    #[test]
    fn select_entry_ignores_nonexistent_id() {
        let storage = create_test_storage();
        let entry = create_test_entry("Test content");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        let original_selection = state.selected_entry_id;

        let fake_id = Uuid::new_v4();
        state.select_entry(fake_id);

        assert_eq!(state.selected_entry_id, original_selection);
    }

    #[test]
    fn select_entry_skips_if_already_selected() {
        let storage = create_test_storage();
        let entry = create_test_entry("Test content");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.selected_entry_id = Some(entry.id);
        state.selected_content = Some("Modified content".to_string());

        state.select_entry(entry.id);

        assert_eq!(state.selected_content, Some("Modified content".to_string()));
    }

    #[test]
    fn selected_entry_returns_metadata_when_selected() {
        let storage = create_test_storage();
        let entry = create_test_entry("Test content");
        storage.insert_entry(&entry).expect("Failed to insert");

        let state = AppState::new(Arc::clone(&storage));

        let selected = state.selected_entry();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().id, entry.id);
    }

    #[test]
    fn selected_entry_returns_none_when_nothing_selected() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);
        state.selected_entry_id = None;

        assert!(state.selected_entry().is_none());
    }

    #[test]
    fn soft_delete_keeps_entry_pending_until_cleared() {
        let storage = create_test_storage();
        let entry = create_test_entry("To be deleted");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        assert!(state.entries.contains_key(&entry.id));

        state.soft_delete(entry.id);

        assert!(state.entries.contains_key(&entry.id));
        assert!(!state.pending_deletes.is_empty());

        state.clear_pending_delete(entry.id);

        assert!(!state.entries.contains_key(&entry.id));
    }

    #[test]
    fn soft_delete_sets_pending_delete() {
        let storage = create_test_storage();
        let entry = create_test_entry("To be deleted");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.soft_delete(entry.id);

        assert!(state.is_pending_delete(entry.id));
    }

    #[test]
    fn soft_delete_updates_selection_to_first_remaining() {
        let storage = create_test_storage();

        let entry1 = create_test_entry("Entry 1");
        let entry2 = create_test_entry("Entry 2");
        storage.insert_entry(&entry1).expect("Failed to insert");
        storage.insert_entry(&entry2).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        // Select the first entry in the map (could be either one)
        let first_id = *state.entries.first().unwrap().0;
        let second_id = *state.entries.last().unwrap().0;
        state.select_entry(first_id);

        state.soft_delete(first_id);

        assert_eq!(state.selected_entry_id, Some(second_id));
    }

    #[test]
    fn soft_delete_clears_selection_when_no_entries_remain() {
        let storage = create_test_storage();
        let entry = create_test_entry("Only entry");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.soft_delete(entry.id);

        assert!(state.selected_entry_id.is_none());
        assert!(state.selected_content.is_none());
    }

    #[test]
    fn soft_delete_ignores_nonexistent_entry() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);

        let fake_id = Uuid::new_v4();
        state.soft_delete(fake_id);

        assert!(state.pending_deletes.is_empty());
    }

    #[test]
    fn undo_delete_restores_entry_within_window() {
        let storage = create_test_storage();
        let entry = create_test_entry("To be restored");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.soft_delete(entry.id);

        assert!(state.entries.contains_key(&entry.id));
        assert!(state.is_pending_delete(entry.id));

        let restored = state.undo_delete(entry.id);

        assert!(restored);
        assert!(state.entries.contains_key(&entry.id));
        assert!(!state.is_pending_delete(entry.id));
        assert_eq!(state.selected_entry_id, Some(entry.id));
    }

    #[test]
    fn undo_delete_returns_false_when_no_pending_delete() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);
        let fake_id = Uuid::new_v4();

        assert!(!state.undo_delete(fake_id));
    }

    #[test]
    fn undo_delete_clears_pending_delete_on_success() {
        let storage = create_test_storage();
        let entry = create_test_entry("To be restored");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.soft_delete(entry.id);
        state.undo_delete(entry.id);

        assert!(!state.is_pending_delete(entry.id));
    }

    #[test]
    fn is_pending_delete_returns_true_for_deleted_entry() {
        let storage = create_test_storage();
        let entry = create_test_entry("Deleted entry");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.soft_delete(entry.id);

        assert!(state.is_pending_delete(entry.id));
    }

    #[test]
    fn is_pending_delete_returns_false_when_not_deleted() {
        let storage = create_test_storage();
        let entry = create_test_entry("Normal entry");
        storage.insert_entry(&entry).expect("Failed to insert");

        let state = AppState::new(Arc::clone(&storage));

        assert!(!state.is_pending_delete(entry.id));
    }

    #[test]
    fn clear_pending_delete_clears_matching_entry() {
        let storage = create_test_storage();
        let entry = create_test_entry("Deleted entry");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.soft_delete(entry.id);

        state.clear_pending_delete(entry.id);

        assert!(state.pending_deletes.is_empty());
    }

    #[test]
    fn clear_pending_delete_ignores_non_matching_entry() {
        let storage = create_test_storage();
        let entry = create_test_entry("Deleted entry");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.soft_delete(entry.id);

        let other_id = Uuid::new_v4();
        state.clear_pending_delete(other_id);

        assert!(!state.pending_deletes.is_empty());
    }

    #[test]
    fn toggle_starred_flips_status() {
        let storage = create_test_storage();
        let entry = create_test_entry("Test entry");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        assert!(!state.entries.get(&entry.id).unwrap().is_starred);

        state.toggle_starred(entry.id);
        assert!(state.entries.get(&entry.id).unwrap().is_starred);

        state.toggle_starred(entry.id);
        assert!(!state.entries.get(&entry.id).unwrap().is_starred);
    }

    #[test]
    fn toggle_starred_ignores_nonexistent_entry() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);

        let fake_id = Uuid::new_v4();
        state.toggle_starred(fake_id);
    }

    #[test]
    fn toggle_starred_persists_to_storage() {
        let storage = create_test_storage();
        let entry = create_test_entry("Test entry");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.toggle_starred(entry.id);

        let stored = storage.get_entry(&entry.id).unwrap().unwrap();
        assert!(stored.is_starred);
    }

    #[test]
    fn update_entry_title_changes_title() {
        let storage = create_test_storage();
        let entry = create_test_entry("Original content");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.update_entry_title(entry.id, "Custom Title".to_string());

        let metadata = state.entries.get(&entry.id).unwrap();
        assert_eq!(metadata.title, "Custom Title");
        assert!(metadata.has_custom_title);
    }

    #[test]
    fn update_entry_title_persists_to_storage() {
        let storage = create_test_storage();
        let entry = create_test_entry("Original content");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.update_entry_title(entry.id, "Custom Title".to_string());

        let stored = storage.get_entry(&entry.id).unwrap().unwrap();
        assert_eq!(stored.title, "Custom Title");
        assert!(stored.has_custom_title);
    }

    #[test]
    fn update_entry_title_ignores_nonexistent_entry() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);

        let fake_id = Uuid::new_v4();
        state.update_entry_title(fake_id, "Title".to_string());
    }

    #[test]
    fn reset_entry_title_regenerates_from_content() {
        let storage = create_test_storage();
        let entry = create_test_entry("Original content");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.update_entry_title(entry.id, "Custom Title".to_string());

        state.reset_entry_title(entry.id);

        let metadata = state.entries.get(&entry.id).unwrap();
        assert_eq!(metadata.title, "Original content");
        assert!(!metadata.has_custom_title);
    }

    #[test]
    fn reset_entry_title_uses_cached_content_when_selected() {
        let storage = create_test_storage();
        let entry = create_test_entry("Cached content");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.selected_entry_id = Some(entry.id);
        state.selected_content = Some("Cached content".to_string());

        state.update_entry_title(entry.id, "Custom".to_string());
        state.reset_entry_title(entry.id);

        let metadata = state.entries.get(&entry.id).unwrap();
        assert!(!metadata.has_custom_title);
    }

    #[test]
    fn reset_entry_title_ignores_nonexistent_entry() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);

        let fake_id = Uuid::new_v4();
        state.reset_entry_title(fake_id);
    }

    #[test]
    fn update_entry_content_changes_content() {
        let storage = create_test_storage();
        let entry = create_test_entry("Original content");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.select_entry(entry.id);

        state.update_entry_content(entry.id, "Updated content".to_string());

        assert_eq!(state.selected_content, Some("Updated content".to_string()));
    }

    #[test]
    fn update_entry_content_regenerates_title_if_not_custom() {
        let storage = create_test_storage();
        let entry = create_test_entry("Original content");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.update_entry_content(entry.id, "New content for title".to_string());

        let metadata = state.entries.get(&entry.id).unwrap();
        assert_eq!(metadata.title, "New content for title");
    }

    #[test]
    fn update_entry_content_preserves_custom_title() {
        let storage = create_test_storage();
        let entry = create_test_entry("Original content");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.update_entry_title(entry.id, "Custom Title".to_string());
        state.update_entry_content(entry.id, "New content".to_string());

        let metadata = state.entries.get(&entry.id).unwrap();
        assert_eq!(metadata.title, "Custom Title");
        assert!(metadata.has_custom_title);
    }

    #[test]
    fn update_entry_content_ignores_nonexistent_entry() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);

        let fake_id = Uuid::new_v4();
        state.update_entry_content(fake_id, "Content".to_string());
    }

    #[test]
    fn set_search_query_updates_query() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);

        state.set_search_query("test query".to_string());

        assert_eq!(state.search_query, "test query");
        assert!(state.is_searching());
    }

    #[test]
    fn set_search_query_clears_fts_on_empty() {
        let storage = create_test_storage();
        let entry = create_test_entry("Test content");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.set_search_query("test".to_string());
        state.set_search_query("".to_string());

        assert!(state.fts_results.is_none());
        assert!(!state.is_searching());
    }

    #[test]
    fn clear_search_resets_query_and_results() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);

        state.set_search_query("test".to_string());
        state.clear_search();

        assert!(state.search_query.is_empty());
        assert!(state.fts_results.is_none());
        assert!(!state.is_searching());
    }

    #[test]
    fn is_searching_returns_false_for_empty_query() {
        let storage = create_test_storage();
        let state = AppState::new(storage);

        assert!(!state.is_searching());
    }

    #[test]
    fn partitioned_entries_separates_starred_and_recent() {
        let storage = create_test_storage();

        let entry1 = create_test_entry("Regular entry");
        let entry2 = create_starred_entry("Starred entry");
        storage.insert_entry(&entry1).expect("Failed to insert");
        storage.insert_entry(&entry2).expect("Failed to insert");

        let state = AppState::new(Arc::clone(&storage));
        let (starred, recent) = state.partitioned_entries();

        assert_eq!(starred.len(), 1);
        assert_eq!(recent.len(), 1);
        assert!(starred[0].is_starred);
        assert!(!recent[0].is_starred);
    }

    #[test]
    fn partitioned_entries_all_recent_when_none_starred() {
        let storage = create_test_storage();

        let entry1 = create_test_entry("Entry 1");
        let entry2 = create_test_entry("Entry 2");
        storage.insert_entry(&entry1).expect("Failed to insert");
        storage.insert_entry(&entry2).expect("Failed to insert");

        let state = AppState::new(Arc::clone(&storage));
        let (starred, recent) = state.partitioned_entries();

        assert!(starred.is_empty());
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn partitioned_entries_all_starred() {
        let storage = create_test_storage();

        let entry1 = create_starred_entry("Starred 1");
        let entry2 = create_starred_entry("Starred 2");
        storage.insert_entry(&entry1).expect("Failed to insert");
        storage.insert_entry(&entry2).expect("Failed to insert");

        let state = AppState::new(Arc::clone(&storage));
        let (starred, recent) = state.partitioned_entries();

        assert_eq!(starred.len(), 2);
        assert!(recent.is_empty());
    }

    #[test]
    fn partitioned_entries_respects_search_filter() {
        let storage = create_test_storage();

        let entry1 = create_test_entry("Apple pie recipe");
        let entry2 = create_test_entry("Banana bread recipe");
        storage.insert_entry(&entry1).expect("Failed to insert");
        storage.insert_entry(&entry2).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.set_search_query("apple".to_string());

        let (starred, recent) = state.partitioned_entries();

        assert_eq!(starred.len() + recent.len(), 1);
    }

    #[test]
    fn toggle_sidebar_flips_visibility() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);

        assert!(state.sidebar_visible);

        state.toggle_sidebar();
        assert!(!state.sidebar_visible);

        state.toggle_sidebar();
        assert!(state.sidebar_visible);
    }

    #[test]
    fn set_sidebar_width_clamps_to_min() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);

        state.set_sidebar_width(0.0);
        assert_eq!(state.sidebar_width, SIDEBAR_MIN_WIDTH_F32);
    }

    #[test]
    fn set_sidebar_width_clamps_to_max() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);

        state.set_sidebar_width(1000.0);
        assert_eq!(state.sidebar_width, SIDEBAR_MAX_WIDTH_F32);
    }

    #[test]
    fn set_sidebar_width_accepts_valid_width() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);

        let valid_width = (SIDEBAR_MIN_WIDTH_F32 + SIDEBAR_MAX_WIDTH_F32) / 2.0;
        state.set_sidebar_width(valid_width);
        assert_eq!(state.sidebar_width, valid_width);
    }

    #[test]
    fn load_more_entries_does_nothing_when_loading() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);
        state.loading_more = true;
        state.has_more_entries = true;

        let initial_count = state.entries.len();
        state.load_more_entries();

        assert_eq!(state.entries.len(), initial_count);
    }

    #[test]
    fn load_more_entries_does_nothing_when_no_more() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);
        state.has_more_entries = false;

        let initial_count = state.entries.len();
        state.load_more_entries();

        assert_eq!(state.entries.len(), initial_count);
    }

    #[test]
    fn has_more_returns_stored_value() {
        let storage = create_test_storage();
        let mut state = AppState::new(storage);

        state.has_more_entries = true;
        assert!(state.has_more_entries);

        state.has_more_entries = false;
        assert!(!state.has_more_entries);
    }

    #[test]
    fn reload_entries_refreshes_from_storage() {
        let storage = create_test_storage();
        let entry = create_test_entry("Entry 1");
        storage.insert_entry(&entry).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        assert_eq!(state.entries.len(), 1);

        let entry2 = create_test_entry("Entry 2");
        storage.insert_entry(&entry2).expect("Failed to insert");
        assert_eq!(state.entries.len(), 1);

        state.reload_entries();

        assert_eq!(state.entries.len(), 2);
    }

    #[test]
    fn reload_entries_updates_selection_if_deleted() {
        let storage = create_test_storage();
        let entry1 = create_test_entry("Entry 1");
        let entry2 = create_test_entry("Entry 2");
        storage.insert_entry(&entry1).expect("Failed to insert");
        storage.insert_entry(&entry2).expect("Failed to insert");

        let mut state = AppState::new(Arc::clone(&storage));
        state.select_entry(entry1.id);
        assert_eq!(state.selected_entry_id, Some(entry1.id));

        // Delete entry1 directly in storage
        storage
            .soft_delete_entry(&entry1.id)
            .expect("Failed to delete");

        state.reload_entries();

        assert_eq!(state.selected_entry_id, Some(entry2.id));
    }

    #[test]
    fn cleanup_old_entries_does_not_panic() {
        let storage = create_test_storage();
        let state = AppState::new(storage);
        state.cleanup_old_entries();
    }
}
