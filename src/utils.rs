const MAX_TITLE_LENGTH: usize = 50;

pub fn generate_title(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }

    let normalized = content.replace("\r\n", "\n");
    let first_line = get_first_meaningful_line(&normalized);
    let trimmed = first_line.trim();

    if trimmed.is_empty() {
        return String::new();
    }

    truncate_with_ellipsis(trimmed, MAX_TITLE_LENGTH)
}

fn get_first_meaningful_line(content: &str) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    String::new()
}

fn truncate_with_ellipsis(s: &str, max_len: usize) -> String {
    let chars: Vec<char> = s.chars().collect();

    if chars.len() <= max_len {
        return s.to_string();
    }

    let truncated: String = chars[..max_len].iter().collect();
    format!("{}...", truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_single_line() {
        let content = "Hello, world!";
        assert_eq!(generate_title(content), "Hello, world!");
    }

    #[test]
    fn test_multiline_takes_first() {
        let content = "First line\nSecond line\nThird line";
        assert_eq!(generate_title(content), "First line");
    }

    #[test]
    fn test_truncation() {
        let content = "This is a very long line that definitely exceeds the fifty character limit we have set";
        let title = generate_title(content);
        assert_eq!(title.len(), 53); // 50 + "..."
        assert!(title.ends_with("..."));
    }

    #[test]
    fn test_skips_blank_first_lines() {
        assert_eq!(generate_title("\n\nActual title"), "Actual title");
        assert_eq!(generate_title("   \n   \nActual title"), "Actual title");
    }

    #[test]
    fn test_empty_content() {
        assert_eq!(generate_title(""), "");
    }

    #[test]
    fn test_whitespace_only() {
        assert_eq!(generate_title("   \n\n  "), "");
    }

    #[test]
    fn test_leading_trailing_whitespace() {
        let content = "  Hello World  \nSecond line";
        assert_eq!(generate_title(content), "Hello World");
    }

    #[test]
    fn test_exactly_50_chars() {
        let content = "A".repeat(50);
        let title = generate_title(&content);
        assert_eq!(title.len(), 50);
        assert!(!title.ends_with("..."));
    }

    #[test]
    fn test_51_chars() {
        let content = "A".repeat(51);
        let title = generate_title(&content);
        assert_eq!(title.len(), 53); // 50 + "..."
        assert!(title.ends_with("..."));
    }
}
