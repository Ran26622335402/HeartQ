//! Regex patterns for obligation extraction.
//!
//! These patterns are derived from OpenSquilla's `compaction_state.py` and
//! adapted for Rust. They match various types of high-signal content in
//! conversation entries.

use once_cell::sync::Lazy;
use regex::Regex;

/// Maximum characters for obligation values (except commands)
pub const MAX_OBLIGATION_VALUE_CHARS: usize = 240;

/// Maximum characters for command obligations
pub const MAX_COMMAND_CHARS: usize = 460;

/// Maximum critical carry-forward items
pub const MAX_CRITICAL_CARRY_FORWARD: usize = 32;

// ---------------------------------------------------------------------------
// Pattern prefixes and markers
// ---------------------------------------------------------------------------

/// Markers for goal detection (user messages)
pub const GOAL_PREFIXES: &[&str] = &[
    "goal:",
    "objective:",
    "目标:",
];

/// Markers for constraint detection (user messages)
pub const CONSTRAINT_PREFIXES: &[&str] = &[
    "constraint:",
    "constraints:",
    "限制:",
    "要求:",
];

/// Markers for decision/rationale detection
pub const DECISION_PREFIXES: &[&str] = &[
    "decision:",
    "rationale:",
    "reason:",
    "decided:",
    "决定:",
    "原因:",
];

/// Markers for next action detection (assistant messages)
pub const NEXT_ACTION_MARKERS: &[&str] = &[
    "next i will",
    "next step",
    "下一步",
    "i will ",
    "我会",
];

/// Markers for do-not-repeat detection
pub const DO_NOT_REPEAT_MARKERS: &[&str] = &[
    "do not repeat",
    "don't repeat",
    "不要重复",
    "不要再",
];

/// Markers for artifact detection
pub const ARTIFACT_MARKERS: &[&str] = &[
    "artifact",
    "generated artifact",
    "附件",
    "产物",
];

/// Markers for error/failure detection
pub const ERROR_MARKERS: &[&str] = &[
    "error",
    "failed",
    "failure",
    "traceback",
    "exit code",
    "exception",
];

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

/// Matches file paths and URLs
/// Note: Uses simpler patterns since Rust regex doesn't support lookbehind
/// Derived from Python: `_PATH_RE`
pub static PATH_RE: Lazy<Regex> = Lazy::new(|| {
    // Pattern that matches paths like /usr/local/bin, ./src, ../config, C:\Users, src/lib.rs, package.json
    Regex::new(
        r"(?:[A-Za-z]:[\\/]|\.{1,2}/|/?)[A-Za-z0-9_.@()+-]+(?:/[A-Za-z0-9_.@()+-]+)*(?:\.[A-Za-z0-9][A-Za-z0-9_.-]{0,15})?"
    ).unwrap()
});

/// Matches shell commands and tool invocations
/// Derived from Python: `_COMMAND_RE`
pub static COMMAND_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?:^|\s)(?:(?:uv run )?(?:pytest|ruff|python|mypy|pyright|npm|pnpm|yarn|git|bash|sh|make|cargo|go test))\b[^\n\r]{0,220}"
    ).unwrap()
});

/// Matches UUIDs and long hexadecimal hashes
/// Derived from Python: `_IDENTIFIER_RE`
pub static IDENTIFIER_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?:[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}|[0-9a-fA-F]{12,64})"
    ).unwrap()
});

/// Matches artifact filenames with common extensions
/// Derived from Python: `_ARTIFACT_NAME_RE`
pub static ARTIFACT_NAME_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"[A-Za-z0-9_.@()+-]+\.(?:pdf|png|jpe?g|gif|csv|json|md|txt|xlsx?|pptx?|docx?|html?|zip)"
    ).unwrap()
});

/// Matches question marks (both ASCII and full-width)
pub static QUESTION_MARK_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[?？]").unwrap()
});

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Check if a line starts with any of the given prefixes (case-insensitive)
pub fn starts_with_prefix(line: &str, prefixes: &[&str]) -> bool {
    let lower = line.to_lowercase();
    prefixes.iter().any(|prefix| lower.starts_with(prefix))
}

/// Check if a line contains any of the given markers (case-insensitive)
pub fn contains_marker(line: &str, markers: &[&str]) -> bool {
    let lower = line.to_lowercase();
    markers.iter().any(|marker| lower.contains(marker))
}

/// Clean obligation text by normalizing whitespace and trimming
pub fn clean_text(text: &str, max_chars: usize) -> String {
    let trim_chars: &[char] = &['`', '\t', '\r', '\n', ',', ';', ')', ']'];
    let cleaned = text
        .replace(|c: char| c.is_whitespace(), " ")
        .trim()
        .trim_start_matches(trim_chars)
        .trim_end_matches(trim_chars)
        .to_string();

    if cleaned.len() <= max_chars {
        cleaned
    } else {
        let mut truncated = cleaned[..max_chars - 3].to_string();
        while !truncated.is_char_boundary(truncated.len()) {
            truncated.pop();
        }
        truncated + "..."
    }
}

/// Extract text after a colon label (for prefixed obligations)
pub fn after_label(line: &str) -> &str {
    match line.find(':') {
        Some(idx) => line[idx + 1..].trim(),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_re() {
        // Unix paths
        assert!(PATH_RE.is_match("/usr/local/bin"));
        assert!(PATH_RE.is_match("./src/main.rs"));
        assert!(PATH_RE.is_match("../config.yml"));
        assert!(PATH_RE.is_match("src/lib.rs"));

        // Windows paths
        assert!(PATH_RE.is_match("C:\\Users\\test"));
        assert!(PATH_RE.is_match("D:/projects"));

        // File with extension
        assert!(PATH_RE.is_match("package.json"));
        assert!(PATH_RE.is_match("Cargo.toml"));
        assert!(PATH_RE.is_match("test_file.md"));

        // Paths with special chars
        assert!(PATH_RE.is_match("my-project_v2.0/src/app.js"));
    }

    #[test]
    fn test_command_re() {
        assert!(COMMAND_RE.is_match("pytest tests/"));
        assert!(COMMAND_RE.is_match("python manage.py"));
        assert!(COMMAND_RE.is_match("ruff check src/"));
        assert!(COMMAND_RE.is_match("git status"));
        assert!(COMMAND_RE.is_match("cargo build"));
        assert!(COMMAND_RE.is_match("npm install"));
        assert!(COMMAND_RE.is_match("uv run python script.py"));
    }

    #[test]
    fn test_identifier_re() {
        // UUIDs
        assert!(IDENTIFIER_RE.is_match("550e8400-e29b-41d4-a716-446655440000"));
        assert!(IDENTIFIER_RE.is_match("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));

        // Long hashes (12+ hex chars)
        assert!(IDENTIFIER_RE.is_match("abcdef1234567890abcdef1234567890"));
        assert!(IDENTIFIER_RE.is_match("abc123def456")); // 12 chars - minimum for non-UUID
        // Short strings should not match
        assert!(!IDENTIFIER_RE.is_match("abc123"));
    }

    #[test]
    fn test_artifact_name_re() {
        assert!(ARTIFACT_NAME_RE.is_match("document.pdf"));
        assert!(ARTIFACT_NAME_RE.is_match("image.png"));
        assert!(ARTIFACT_NAME_RE.is_match("data.json"));
        assert!(ARTIFACT_NAME_RE.is_match("report.xlsx"));
        assert!(ARTIFACT_NAME_RE.is_match("presentation.pptx"));
    }

    #[test]
    fn test_clean_text() {
        assert_eq!(clean_text("  hello world  ", 100), "hello world");
        assert_eq!(clean_text("`code`;", 100), "code");
        assert_eq!(
            clean_text("this is a very long text that exceeds the limit", 20),
            "this is a very lo..."
        );
    }

    #[test]
    fn test_starts_with_prefix() {
        assert!(starts_with_prefix("Goal: Build a compiler", GOAL_PREFIXES));
        assert!(starts_with_prefix("constraint: Must be fast", CONSTRAINT_PREFIXES));
        assert!(!starts_with_prefix("Just some text", GOAL_PREFIXES));
    }

    #[test]
    fn test_contains_marker() {
        assert!(contains_marker("Do not repeat this", DO_NOT_REPEAT_MARKERS));
        assert!(contains_marker("ERROR: something failed", ERROR_MARKERS));
        assert!(!contains_marker("Normal text", ERROR_MARKERS));
    }
}
