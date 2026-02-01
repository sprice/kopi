CREATE TABLE IF NOT EXISTS clipboard_entries (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    has_custom_title INTEGER NOT NULL DEFAULT 0,
    is_starred INTEGER NOT NULL DEFAULT 0,
    deleted_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_deleted_starred_created ON clipboard_entries(is_starred, created_at DESC) WHERE deleted_at IS NULL;
CREATE INDEX idx_deleted_at_partial ON clipboard_entries(deleted_at) WHERE deleted_at IS NOT NULL;

-- FTS5 virtual table for full-text search on title and content
-- Uses unicode61 tokenizer for proper Unicode handling
CREATE VIRTUAL TABLE IF NOT EXISTS clipboard_entries_fts USING fts5(
    title,
    content,
    content='clipboard_entries',
    content_rowid='rowid',
    tokenize='unicode61'
);

-- Triggers to keep FTS index in sync with main table
CREATE TRIGGER IF NOT EXISTS clipboard_entries_ai AFTER INSERT ON clipboard_entries BEGIN
    INSERT INTO clipboard_entries_fts(rowid, title, content)
    VALUES (NEW.rowid, NEW.title, NEW.content);
END;

CREATE TRIGGER IF NOT EXISTS clipboard_entries_ad AFTER DELETE ON clipboard_entries BEGIN
    INSERT INTO clipboard_entries_fts(clipboard_entries_fts, rowid, title, content)
    VALUES ('delete', OLD.rowid, OLD.title, OLD.content);
END;

CREATE TRIGGER IF NOT EXISTS clipboard_entries_au AFTER UPDATE ON clipboard_entries BEGIN
    INSERT INTO clipboard_entries_fts(clipboard_entries_fts, rowid, title, content)
    VALUES ('delete', OLD.rowid, OLD.title, OLD.content);
    INSERT INTO clipboard_entries_fts(rowid, title, content)
    VALUES (NEW.rowid, NEW.title, NEW.content);
END;
