use crate::models::EntryMetadata;
use crate::storage::{Storage, StorageError};

#[derive(Debug, Clone)]
pub struct SearchResult<'a> {
    pub entry: &'a EntryMetadata,
    pub score: i64,
}

#[derive(Debug, Clone)]
pub struct OwnedSearchResult {
    pub entry: EntryMetadata,
}

pub fn search_entries_fts(
    storage: &Storage,
    query: &str,
) -> Result<Vec<OwnedSearchResult>, StorageError> {
    storage.search_entries(query).map(|entries| {
        entries
            .into_iter()
            .map(|entry| OwnedSearchResult { entry })
            .collect()
    })
}

pub trait SearchMatcher: Send + Sync {
    fn matches(&self, query: &str, text: &str) -> Option<i64>;
}

#[derive(Debug, Default, Clone)]
pub struct SubstringMatcher;

impl SearchMatcher for SubstringMatcher {
    fn matches(&self, query: &str, text: &str) -> Option<i64> {
        let query_lower = query.to_lowercase();
        let text_lower = text.to_lowercase();

        text_lower
            .find(&query_lower)
            .map(|position| 1000_i64.saturating_sub(position as i64))
    }
}

pub fn filter_entries<'a>(
    entries: impl Iterator<Item = &'a EntryMetadata>,
    query: &str,
    matcher: &impl SearchMatcher,
) -> Vec<SearchResult<'a>> {
    if query.is_empty() {
        return entries
            .map(|entry| SearchResult { entry, score: 0 })
            .collect();
    }

    let mut results: Vec<SearchResult<'a>> = entries
        .filter_map(|entry| {
            matcher
                .matches(query, &entry.title)
                .map(|score| SearchResult { entry, score })
        })
        .collect();

    results.sort_by(|a, b| b.score.cmp(&a.score));
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ClipboardEntry;

    fn make_metadata(title: &str) -> EntryMetadata {
        let entry = ClipboardEntry::new(title.to_string());
        entry.to_metadata()
    }

    fn make_metadata_with_title(title: &str, content: &str) -> EntryMetadata {
        let mut entry = ClipboardEntry::new(content.to_string());
        entry.update_title(title.to_string());
        entry.to_metadata()
    }

    #[test]
    fn substring_matcher_finds_exact_match() {
        let matcher = SubstringMatcher;
        assert!(matcher.matches("hello", "hello world").is_some());
    }

    #[test]
    fn substring_matcher_case_insensitive() {
        let matcher = SubstringMatcher;
        assert!(matcher.matches("HELLO", "hello world").is_some());
        assert!(matcher.matches("hello", "HELLO WORLD").is_some());
    }

    #[test]
    fn substring_matcher_no_match() {
        let matcher = SubstringMatcher;
        assert!(matcher.matches("xyz", "hello world").is_none());
    }

    #[test]
    fn filter_entries_empty_query_returns_all() {
        let entries = [make_metadata("First"), make_metadata("Second")];

        let results = filter_entries(entries.iter(), "", &SubstringMatcher);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn filter_entries_matches_title() {
        let entries = [
            make_metadata_with_title("Hello World", "Some content"),
            make_metadata_with_title("Goodbye", "Other content"),
        ];

        let results = filter_entries(entries.iter(), "hello", &SubstringMatcher);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.title, "Hello World");
    }

    #[test]
    fn filter_entries_title_only_search() {
        // With lazy loading, search only works on titles
        let entries = [
            make_metadata_with_title("Title 1", "Hello World"),
            make_metadata_with_title("Title 2", "Goodbye"),
        ];

        // Searching for "hello" won't find content since we only search titles
        let results = filter_entries(entries.iter(), "hello", &SubstringMatcher);
        assert_eq!(results.len(), 0);

        // But searching for title text works
        let results = filter_entries(entries.iter(), "Title 1", &SubstringMatcher);
        assert_eq!(results.len(), 1);
    }
}
