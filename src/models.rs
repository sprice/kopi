use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct EntryMetadata {
    pub id: Uuid,
    pub title: String,
    pub has_custom_title: bool,
    pub is_starred: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub const MAX_CONTENT_SIZE: usize = 1_024 * 1_024;
pub const LARGE_CONTENT_WARNING_THRESHOLD: usize = 100 * 1024;
fn truncate_content(content: String) -> String {
    if content.len() > LARGE_CONTENT_WARNING_THRESHOLD {
        log::debug!("Large clipboard content detected: {} bytes", content.len());
    }

    if content.len() > MAX_CONTENT_SIZE {
        log::warn!(
            "Truncating clipboard content from {} to {} bytes",
            content.len(),
            MAX_CONTENT_SIZE
        );
        let mut truncated = String::with_capacity(MAX_CONTENT_SIZE);
        for ch in content.chars() {
            if truncated.len() + ch.len_utf8() > MAX_CONTENT_SIZE {
                break;
            }
            truncated.push(ch);
        }
        truncated
    } else {
        content
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntry {
    pub id: Uuid,
    pub title: String,
    pub content: String,
    pub has_custom_title: bool,
    pub is_starred: bool,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ClipboardEntry {
    pub fn new(content: String) -> Self {
        let content = truncate_content(content);
        let now = Utc::now();
        let title = Self::generate_title(&content);

        Self {
            id: Uuid::new_v4(),
            title,
            content,
            has_custom_title: false,
            is_starred: false,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn generate_title(content: &str) -> String {
        crate::utils::generate_title(content)
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    pub fn soft_delete(&mut self) {
        let now = Utc::now();
        self.deleted_at = Some(now);
        self.updated_at = now;
    }

    pub fn restore(&mut self) {
        self.deleted_at = None;
        self.updated_at = Utc::now();
    }

    pub fn update_content(&mut self, content: String) {
        self.content = truncate_content(content);
        self.updated_at = Utc::now();
    }

    pub fn update_title(&mut self, title: String) {
        self.title = title;
        self.has_custom_title = true;
        self.updated_at = Utc::now();
    }

    pub fn reset_title_from_content(&mut self) {
        self.title = Self::generate_title(&self.content);
        self.has_custom_title = false;
        self.updated_at = Utc::now();
    }

    pub fn toggle_starred(&mut self) {
        self.is_starred = !self.is_starred;
        self.updated_at = Utc::now();
    }

    pub fn to_metadata(&self) -> EntryMetadata {
        EntryMetadata {
            id: self.id,
            title: self.title.clone(),
            has_custom_title: self.has_custom_title,
            is_starred: self.is_starred,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_title_delegates_to_utils() {
        let content = "Hello World";
        assert_eq!(
            ClipboardEntry::generate_title(content),
            crate::utils::generate_title(content)
        );
    }

    #[test]
    fn test_new_entry_generates_title() {
        let entry = ClipboardEntry::new("Test content".to_string());
        assert_eq!(entry.title, "Test content");
        assert!(!entry.has_custom_title);
    }

    #[test]
    fn test_reset_title_from_content() {
        let mut entry = ClipboardEntry::new("Original content".to_string());
        entry.update_title("Custom title".to_string());
        assert!(entry.has_custom_title);

        entry.reset_title_from_content();
        assert_eq!(entry.title, "Original content");
        assert!(!entry.has_custom_title);
    }
}
