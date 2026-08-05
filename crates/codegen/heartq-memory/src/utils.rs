//! Utility functions for memory formatting, token estimation, and context helpers.
//!
//! This module provides standalone utility functions for working with memory blocks,
//! including formatting, token estimation, truncation, and memory context helpers.

use crate::prefetch::{MemoryBlock, MemorySource};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// Format a single memory block for display.
///
/// Returns a formatted string with source label, path, line range, score,
/// and the block content.
pub fn format_memory_block(block: &MemoryBlock) -> String {
    let source_label = match block.source {
        MemorySource::Global => "global memory",
        MemorySource::Workspace => "workspace memory",
        MemorySource::Session => "session memory",
    };

    format!(
        "[{}] {} (lines {}-{}, score: {:.2})\n{}",
        source_label,
        block.path,
        block.start_line,
        block.end_line,
        block.score,
        block.content
    )
}

/// Estimate tokens in a string (rough approximation).
///
/// Uses a simple character-based heuristic: approximately 4 characters per token,
/// plus overhead for role/content markers.
pub fn estimate_tokens(text: &str) -> u64 {
    (text.len() as u64 / 4) + 10
}

/// Estimate tokens in a memory block.
pub fn estimate_block_tokens(block: &MemoryBlock) -> u64 {
    estimate_tokens(&block.content)
}

/// Estimate tokens in multiple memory blocks.
pub fn estimate_blocks_tokens(blocks: &[MemoryBlock]) -> u64 {
    blocks.iter().map(estimate_block_tokens).sum()
}

/// Truncate text to fit within token budget.
///
/// Finds a good break point near the character limit (4 chars per token).
/// Prefers breaking at newlines, then spaces, then any character.
pub fn truncate_to_token_budget(text: &str, max_tokens: u64) -> String {
    let max_chars = (max_tokens * 4) as usize;
    if text.len() <= max_chars {
        return text.to_string();
    }

    // Find a good break point near the limit
    let truncated = &text[..max_chars];
    if let Some(last_newline) = truncated.rfind('\n') {
        truncated[..last_newline].to_string()
    } else if let Some(last_space) = truncated.rfind(' ') {
        truncated[..last_space].to_string()
    } else {
        truncated.to_string()
    }
}

/// Determine memory source from file path.
///
/// Analyzes the path structure to determine the appropriate memory source:
/// - Paths containing "/sessions/" are session memory
/// - Paths ending with "MEMORY.md" are either global or workspace
/// - Other paths are workspace memory
pub fn source_from_path(path: &Path) -> MemorySource {
    let path_str = path.to_string_lossy();

    if path_str.contains("/sessions/") {
        MemorySource::Session
    } else if path_str.ends_with("MEMORY.md") {
        // Check if MEMORY.md is directly under memory/ (Global)
        // or in a workspace subdirectory like memory/abc123/MEMORY.md (Workspace)
        let parent = path.parent();
        if let Some(parent_path) = parent {
            let parent_str = parent_path.to_string_lossy();
            // Check if parent ends with "/memory" and "memory" is a complete path component
            // e.g., /home/user/.heartq/memory/MEMORY.md -> Global (parent ends with "/memory")
            // e.g., /home/user/.heartq/memory/abc123/MEMORY.md -> Workspace (parent ends with "/abc123")
            if parent_str.ends_with("/memory") {
                return MemorySource::Global;
            }
        }
        MemorySource::Workspace
    } else {
        MemorySource::Workspace
    }
}

/// Determine memory source from a string path.
pub fn source_from_path_str(path: &str) -> MemorySource {
    source_from_path(Path::new(path))
}

/// Check if a path should be included in memory.
///
/// Returns false for binary/non-text file types that shouldn't be indexed.
pub fn is_valid_memory_path(path: &Path) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();

    // Exclude certain file types
    const EXCLUDE_EXTENSIONS: &[&str] = &[
        // Images
        ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".ico", ".svg", ".webp",
        // Audio
        ".mp3", ".wav", ".ogg", ".flac", ".aac", ".m4a",
        // Video
        ".mp4", ".avi", ".mov", ".mkv", ".webm", ".flv",
        // Archives
        ".pdf", ".zip", ".tar", ".gz", ".rar", ".7z", ".bz2",
        // Binaries
        ".exe", ".dll", ".so", ".dylib", ".o", ".a", ".lib",
        // Fonts
        ".ttf", ".otf", ".woff", ".woff2", ".eot",
        // Other
        ".db", ".sqlite", ".sqlite3",
    ];

    for ext in EXCLUDE_EXTENSIONS {
        if path_str.ends_with(ext) {
            return false;
        }
    }

    true
}

/// Clean markdown content for memory storage.
///
/// Removes empty lines at start/end and skips lines that are likely
/// binary/non-text content.
pub fn clean_markdown(content: &str) -> String {
    let mut result = String::with_capacity(content.len());

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines at start/end
        if trimmed.is_empty() && result.is_empty() {
            continue;
        }

        // Skip very long lines that are likely binary/non-text
        if trimmed.len() >= 500 {
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    // Remove trailing newlines
    while result.ends_with('\n') {
        result.pop();
    }

    result
}

/// Generate a unique memory ID from path and start line.
///
/// Creates a deterministic hash-based ID that combines the file path
/// and line number.
pub fn generate_memory_id(path: &Path, start_line: usize) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    start_line.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Parse a memory ID back to components.
///
/// Currently returns None as full implementation would require
/// additional encoding. This is a placeholder for future enhancement.
pub fn parse_memory_id(_id: &str) -> Option<(String, usize)> {
    None
}

/// Build a memory context header.
///
/// Returns the header markup used to denote the start of memory context
/// in prompts. This signals to the model that the following content
/// is recalled memory, not new user input.
pub fn build_memory_header() -> String {
    r#"<memory-context>
[System note: The following is recalled memory context, NOT new user input.
Treat as authoritative reference data — do not repeat or re-summarize it
unless the user explicitly asks about the memory content itself.]

"#
    .to_string()
}

/// Build a memory context footer.
///
/// Returns the footer markup used to denote the end of memory context.
pub fn build_memory_footer() -> String {
    "\n</memory-context>".to_string()
}

/// Check if content is a memory context block.
///
/// Returns true if the content contains both opening and closing
/// memory context tags.
pub fn is_memory_context(content: &str) -> bool {
    content.contains("<memory-context>") && content.contains("</memory-context>")
}

/// Extract content from a memory context block.
///
/// Returns the content between the memory context tags, including
/// the tags themselves. Returns None if tags are malformed.
pub fn extract_memory_context(content: &str) -> Option<String> {
    if let Some(start) = content.find("<memory-context>") {
        if let Some(end) = content.find("</memory-context>") {
            // Include the closing tag
            let end_inclusive = end + "</memory-context>".len();
            return Some(content[start..end_inclusive].to_string());
        }
    }
    None
}

/// Sanitize memory content for injection into prompts.
///
/// Removes any existing memory context tags from content to prevent
/// nesting or duplication when injecting into prompts.
pub fn sanitize_for_injection(content: &str) -> String {
    // Remove memory-context tags while preserving text content.
    // For nested structures like
    // <memory-context>outer <memory-context>inner</memory-context> content</memory-context>,
    // we preserve all text content: "outer inner content"
    // For non-nested: "Some text <memory-context>old</memory-context> text" -> "Some text text"
    let open_tag = "<memory-context>";
    let close_tag = "</memory-context>";

    let mut result = content.to_string();

    // Remove all memory-context tags (both opening and closing)
    // while preserving all text content
    while let Some(open_pos) = result.find(open_tag) {
        result = format!("{}{}", &result[..open_pos], &result[open_pos + open_tag.len()..]);
    }

    while let Some(close_pos) = result.find(close_tag) {
        result = format!("{}{}", &result[..close_pos], &result[close_pos + close_tag.len()..]);
    }

    result.trim().to_string()
}

/// Build a complete memory context wrapper around content.
pub fn wrap_memory_context(content: &str) -> String {
    if content.is_empty() {
        String::new()
    } else {
        format!(
            "{}{}{}",
            build_memory_header(),
            content,
            build_memory_footer()
        )
    }
}

/// Create a MemoryBlock with default timestamp (0).
///
/// Convenience function for tests and simple usage.
#[cfg(test)]
pub fn test_block(
    content: &str,
    source: MemorySource,
    score: f64,
    path: &str,
    start_line: usize,
    end_line: usize,
) -> MemoryBlock {
    MemoryBlock {
        content: content.to_string(),
        source,
        score,
        path: path.to_string(),
        start_line,
        end_line,
        timestamp: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Token estimation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_estimate_tokens() {
        assert!(estimate_tokens("hello world") > 0);
        assert!(estimate_tokens("") < 20);
    }

    #[test]
    fn test_estimate_tokens_long_text() {
        let text = "a".repeat(1000);
        let tokens = estimate_tokens(&text);
        // ~1000/4 + 10 = 260
        assert!(tokens >= 250 && tokens <= 270);
    }

    #[test]
    fn test_estimate_block_tokens() {
        let block = test_block(
            "hello world",
            MemorySource::Global,
            1.0,
            "/test.md",
            1,
            1,
        );
        assert!(estimate_block_tokens(&block) > 0);
    }

    #[test]
    fn test_estimate_blocks_tokens() {
        let blocks = vec![
            test_block(
                "hello world",
                MemorySource::Global,
                1.0,
                "/test.md",
                1,
                1,
            ),
            test_block(
                "foo bar",
                MemorySource::Workspace,
                0.9,
                "/test2.md",
                2,
                2,
            ),
        ];
        let total = estimate_blocks_tokens(&blocks);
        assert!(total > 0);
    }

    // -----------------------------------------------------------------------
    // Truncation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_truncate_to_token_budget_short() {
        let text = "short text";
        let result = truncate_to_token_budget(text, 100);
        assert_eq!(result, text);
    }

    #[test]
    fn test_truncate_to_token_budget_long() {
        let long_text = "a".repeat(1000);
        let truncated = truncate_to_token_budget(&long_text, 100);
        assert!(truncated.len() < 1000);
        // Should be at most 4 * 100 = 400 chars (plus potential break adjustments)
        assert!(truncated.len() <= 401);
    }

    #[test]
    fn test_truncate_at_newline() {
        let text = "line1\nline2\nline3";
        let truncated = truncate_to_token_budget(text, 1); // 4 chars max
        assert!(!truncated.contains('\n'));
    }

    #[test]
    fn test_truncate_at_space() {
        let text = "word1 word2 word3";
        let truncated = truncate_to_token_budget(text, 1); // 4 chars max
        // Should break at space if no newline nearby
        assert!(!truncated.ends_with(' '));
    }

    // -----------------------------------------------------------------------
    // Path analysis tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_source_from_path_session() {
        let path = Path::new("/home/user/.heartq/memory/abc123/sessions/2024-01-01-test-ab123456.md");
        assert_eq!(source_from_path(path), MemorySource::Session);
    }

    #[test]
    fn test_source_from_path_global_memory() {
        let path = Path::new("/home/user/.heartq/memory/MEMORY.md");
        assert_eq!(source_from_path(path), MemorySource::Global);
    }

    #[test]
    fn test_source_from_path_workspace_memory() {
        let path = Path::new("/home/user/.heartq/memory/abc123/MEMORY.md");
        assert_eq!(source_from_path(path), MemorySource::Workspace);
    }

    #[test]
    fn test_source_from_path_other() {
        let path = Path::new("/project/src/main.rs");
        assert_eq!(source_from_path(path), MemorySource::Workspace);
    }

    #[test]
    fn test_source_from_path_str() {
        assert_eq!(
            source_from_path_str("/home/user/.heartq/memory/sessions/2024-01-01-test.md"),
            MemorySource::Session
        );
        assert_eq!(source_from_path_str("/home/user/.heartq/memory/MEMORY.md"), MemorySource::Global);
        assert_eq!(
            source_from_path_str("/home/user/.heartq/memory/workspace123/MEMORY.md"),
            MemorySource::Workspace
        );
    }

    #[test]
    fn test_is_valid_memory_path_valid() {
        assert!(is_valid_memory_path(Path::new("/test.rs")));
        assert!(is_valid_memory_path(Path::new("/test.md")));
        assert!(is_valid_memory_path(Path::new("/test.txt")));
    }

    #[test]
    fn test_is_valid_memory_path_excluded() {
        assert!(!is_valid_memory_path(Path::new("/test.png")));
        assert!(!is_valid_memory_path(Path::new("/test.jpg")));
        assert!(!is_valid_memory_path(Path::new("/test.mp3")));
        assert!(!is_valid_memory_path(Path::new("/test.mp4")));
        assert!(!is_valid_memory_path(Path::new("/test.pdf")));
        assert!(!is_valid_memory_path(Path::new("/test.zip")));
        assert!(!is_valid_memory_path(Path::new("/test.exe")));
        assert!(!is_valid_memory_path(Path::new("/test.dll")));
        assert!(!is_valid_memory_path(Path::new("/test.ttf")));
    }

    // -----------------------------------------------------------------------
    // Markdown cleaning tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_clean_markdown() {
        let input = "\n\n# Title\n\nSome content\n\n";
        let cleaned = clean_markdown(input);
        assert!(!cleaned.starts_with('\n'));
        // Should contain the content but not leading/trailing empty lines
        assert!(cleaned.contains("# Title"));
        assert!(cleaned.contains("Some content"));
    }

    #[test]
    fn test_clean_markdown_preserves_content() {
        let input = "# Header\n\nParagraph text.\n\n- Item 1\n- Item 2";
        let cleaned = clean_markdown(input);
        assert!(cleaned.contains("# Header"));
        assert!(cleaned.contains("Paragraph text"));
        assert!(cleaned.contains("- Item 1"));
    }

    #[test]
    fn test_clean_markdown_filters_long_lines() {
        let input = format!("# Normal\n{}\n# After", "x".repeat(500));
        let cleaned = clean_markdown(&input);
        assert!(!cleaned.contains(&"x".repeat(500)));
    }

    // -----------------------------------------------------------------------
    // Memory ID tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_generate_memory_id() {
        let id = generate_memory_id(Path::new("/test/path.md"), 10);
        assert_eq!(id.len(), 16);
        // Should be valid hex
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_generate_memory_id_deterministic() {
        let id1 = generate_memory_id(Path::new("/test.md"), 5);
        let id2 = generate_memory_id(Path::new("/test.md"), 5);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_generate_memory_id_different_lines() {
        let id1 = generate_memory_id(Path::new("/test.md"), 5);
        let id2 = generate_memory_id(Path::new("/test.md"), 10);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_parse_memory_id() {
        assert_eq!(parse_memory_id("anything"), None);
    }

    // -----------------------------------------------------------------------
    // Memory context tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_memory_context() {
        assert!(is_memory_context("<memory-context>content</memory-context>"));
        assert!(is_memory_context("prefix <memory-context>x</memory-context> suffix"));
        assert!(!is_memory_context("no tags here"));
        assert!(!is_memory_context("<other-tag></other-tag>"));
    }

    #[test]
    fn test_extract_memory_context() {
        let content = "prefix <memory-context>extracted</memory-context> suffix";
        let extracted = extract_memory_context(content);
        assert!(extracted.is_some());
        let extracted = extracted.unwrap();
        assert!(extracted.contains("<memory-context>"));
        assert!(extracted.contains("</memory-context>"));
        assert!(extracted.contains("extracted"));
    }

    #[test]
    fn test_extract_memory_context_no_tags() {
        assert_eq!(extract_memory_context("no tags"), None);
    }

    #[test]
    fn test_extract_memory_context_unclosed_tag() {
        assert_eq!(
            extract_memory_context("<memory-context>unclosed"),
            None
        );
    }

    #[test]
    fn test_sanitize_for_injection() {
        let input = "Some text <memory-context>old content</memory-context> more text";
        let sanitized = sanitize_for_injection(input);
        assert!(!sanitized.contains("<memory-context>"));
        assert!(!sanitized.contains("</memory-context>"));
        assert!(sanitized.contains("Some text"));
        assert!(sanitized.contains("more text"));
    }

    #[test]
    fn test_sanitize_for_injection_nested() {
        let input = "<memory-context>outer <memory-context>inner</memory-context> content</memory-context>";
        let sanitized = sanitize_for_injection(input);
        assert!(!sanitized.contains("<memory-context>"));
        assert!(sanitized.contains("outer"));
        assert!(sanitized.contains("inner"));
        assert!(sanitized.contains("content"));
    }

    #[test]
    fn test_sanitize_for_injection_no_tags() {
        let input = "clean text with no memory tags";
        let sanitized = sanitize_for_injection(input);
        assert_eq!(sanitized, input);
    }

    // -----------------------------------------------------------------------
    // Format and wrap tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_memory_block() {
        let block = test_block(
            "test content",
            MemorySource::Global,
            0.95,
            "/path/to/memory.md",
            10,
            20,
        );
        let formatted = format_memory_block(&block);
        assert!(formatted.contains("global memory"));
        assert!(formatted.contains("/path/to/memory.md"));
        assert!(formatted.contains("lines 10-20"));
        assert!(formatted.contains("score: 0.95"));
        assert!(formatted.contains("test content"));
    }

    #[test]
    fn test_wrap_memory_context() {
        let content = "memory content here";
        let wrapped = wrap_memory_context(content);
        assert!(wrapped.starts_with("<memory-context>"));
        assert!(wrapped.ends_with("</memory-context>"));
        assert!(wrapped.contains(content));
    }

    #[test]
    fn test_wrap_memory_context_empty() {
        let wrapped = wrap_memory_context("");
        assert!(wrapped.is_empty());
    }

    #[test]
    fn test_build_memory_header() {
        let header = build_memory_header();
        assert!(header.contains("<memory-context>"));
        assert!(header.contains("System note"));
        assert!(header.contains("recalled memory context"));
    }

    #[test]
    fn test_build_memory_footer() {
        let footer = build_memory_footer();
        assert_eq!(footer, "\n</memory-context>");
    }
}
