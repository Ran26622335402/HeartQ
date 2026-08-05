//! Comprehensive unit tests for the heartq-memory crate.
//!
//! This module organizes tests for the memory system into logical groups:
//! - Prefetch tests: MemoryBlock, MemorySource, and PrefetchResult
//! - Sync tests: Insight, MemoryScope, and TurnSync implementations
//! - Utility tests: Token estimation, truncation, and context helpers
//! - Error tests: MemoryError variants and utilities

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Prefetch Tests ==========
    mod prefetch_tests {
        use super::*;
        use crate::prefetch::{MemoryBlock, MemorySource, PrefetchResult};

        #[test]
        fn test_memory_block_estimate_tokens() {
            let block = MemoryBlock {
                content: "Hello world this is a test".to_string(),
                source: MemorySource::Workspace,
                score: 0.8,
                path: "test.md".to_string(),
                start_line: 1,
                end_line: 2,
                timestamp: 1234567890,
            };

            let tokens = block.estimate_tokens();
            assert!(tokens > 0);
            assert!(tokens < 30);
        }

        #[test]
        fn test_memory_block_estimate_tokens_long_content() {
            let block = MemoryBlock {
                content: "a".repeat(1000),
                source: MemorySource::Global,
                score: 0.5,
                path: "long.md".to_string(),
                start_line: 1,
                end_line: 100,
                timestamp: 1234567890,
            };

            let tokens = block.estimate_tokens();
            // 1000 chars / 4 + 20 overhead = 270
            assert!(tokens >= 250 && tokens <= 280);
        }

        #[test]
        fn test_prefetch_result_new_not_exceeded() {
            let blocks = vec![
                MemoryBlock {
                    content: "Short content".to_string(),
                    source: MemorySource::Global,
                    score: 0.9,
                    path: "test.md".to_string(),
                    start_line: 1,
                    end_line: 2,
                    timestamp: 1000,
                },
            ];

            let result = PrefetchResult::new(blocks, 1000);
            assert!(!result.exceeded);
            assert_eq!(result.blocks.len(), 1);
        }

        #[test]
        fn test_prefetch_result_exceeded() {
            let blocks = vec![
                MemoryBlock {
                    content: "x".repeat(10000),
                    source: MemorySource::Global,
                    score: 0.5,
                    path: "test.md".to_string(),
                    start_line: 1,
                    end_line: 100,
                    timestamp: None,
                },
            ];

            let result = PrefetchResult::new(blocks, 1000);
            assert!(result.exceeded);
        }

        #[test]
        fn test_prefetch_result_empty() {
            let result = PrefetchResult::empty();
            assert!(result.is_empty());
            assert_eq!(result.len(), 0);
            assert!(!result.exceeded);
        }

        #[test]
        fn test_prefetch_result_default() {
            let result = PrefetchResult::default();
            assert!(result.is_empty());
        }

        #[test]
        fn test_memory_source_from_str() {
            assert_eq!(MemorySource::from_str("global"), Some(MemorySource::Global));
            assert_eq!(MemorySource::from_str("workspace"), Some(MemorySource::Workspace));
            assert_eq!(MemorySource::from_str("session"), Some(MemorySource::Session));
            assert_eq!(MemorySource::from_str("unknown"), None);
        }

        #[test]
        fn test_memory_source_as_str() {
            assert_eq!(MemorySource::Global.as_str(), "global");
            assert_eq!(MemorySource::Workspace.as_str(), "workspace");
            assert_eq!(MemorySource::Session.as_str(), "session");
        }

        #[test]
        fn test_memory_source_debug() {
            let source = MemorySource::Global;
            let debug_str = format!("{:?}", source);
            assert_eq!(debug_str, "Global");
        }

        #[test]
        fn test_memory_block_debug() {
            let block = MemoryBlock {
                content: "test".to_string(),
                source: MemorySource::Workspace,
                score: 0.5,
                path: "/path".to_string(),
                start_line: 1,
                end_line: 2,
                timestamp: 123,
            };
            let debug_str = format!("{:?}", block);
            assert!(debug_str.contains("MemoryBlock"));
        }

        #[test]
        fn test_prefetch_result_debug() {
            let result = PrefetchResult::empty();
            let debug_str = format!("{:?}", result);
            assert!(debug_str.contains("PrefetchResult"));
        }
    }

    // ========== Sync Tests ==========
    mod sync_tests {
        use super::*;
        use crate::sync::{Insight, InsightSource, MemoryScope, RuleBasedInsightExtractor};

        #[test]
        fn test_insight_creation() {
            let insight = Insight::new(
                "Remember to check config".to_string(),
                InsightSource::UserMessage,
                0.8,
            );
            assert_eq!(insight.source, InsightSource::UserMessage);
            assert_eq!(insight.importance, 0.8);
            assert!(insight.tags.is_empty());
        }

        #[test]
        fn test_insight_with_tags() {
            let insight = Insight::new("Config".to_string(), InsightSource::UserMessage, 0.6)
                .with_tag("config");
            assert!(insight.tags.contains(&"config".to_string()));
        }

        #[test]
        fn test_insight_with_multiple_tags() {
            let insight = Insight::new("Important note".to_string(), InsightSource::UserMessage, 0.9)
                .with_tag("important")
                .with_tag("note");
            assert_eq!(insight.tags.len(), 2);
            assert!(insight.tags.contains(&"important".to_string()));
            assert!(insight.tags.contains(&"note".to_string()));
        }

        #[test]
        fn test_insight_debug() {
            let insight = Insight::new(
                "Test content".to_string(),
                InsightSource::AssistantResponse,
                0.7,
            );
            let debug_str = format!("{:?}", insight);
            assert!(debug_str.contains("Insight"));
            assert!(debug_str.contains("Test content"));
        }

        #[test]
        fn test_memory_scope_directory() {
            assert_eq!(MemoryScope::Global.directory_name(), "MEMORY.md");
            assert_eq!(MemoryScope::Workspace.directory_name(), "MEMORY.md");
            assert_eq!(MemoryScope::Session.directory_name(), "sessions");
        }

        #[test]
        fn test_memory_scope_default() {
            let scope = MemoryScope::default();
            assert_eq!(scope, MemoryScope::Workspace);
        }

        #[test]
        fn test_memory_scope_debug() {
            let scope = MemoryScope::Global;
            let debug_str = format!("{:?}", scope);
            assert_eq!(debug_str, "Global");
        }

        #[test]
        fn test_rule_based_extractor_creation() {
            let extractor = RuleBasedInsightExtractor::new(0.5);
            assert!(extractor.min_importance >= 0.0);
        }

        #[test]
        fn test_rule_based_extractor_extract_important() {
            let extractor = RuleBasedInsightExtractor::new(0.3);
            let insights = extractor.extract_from_user("Remember to check the config file");
            assert!(!insights.is_empty());
        }

        #[test]
        fn test_rule_based_extractor_extract_normal() {
            let extractor = RuleBasedInsightExtractor::new(0.3);
            let insights = extractor.extract_from_user("hello world");
            // "hello world" has base score of 0.3 which meets min_importance of 0.3
            assert!(!insights.is_empty());
        }

        #[test]
        fn test_rule_based_extractor_empty_input() {
            let extractor = RuleBasedInsightExtractor::new(0.3);
            let insights = extractor.extract_from_user("");
            assert!(insights.is_empty());
        }

        #[test]
        fn test_rule_based_extractor_multiline() {
            let extractor = RuleBasedInsightExtractor::new(0.5);
            let content = "This is line one\nRemember to do something important\nThis is line three";
            let insights = extractor.extract_from_user(content);
            assert!(!insights.is_empty());
        }

        #[test]
        fn test_rule_based_extractor_assess_importance() {
            let extractor = RuleBasedInsightExtractor::new(0.0);
            // Normal text
            let normal_score = extractor.assess_importance("normal text");
            assert!(normal_score >= 0.3);

            // Text with keywords
            let important_score = extractor.assess_importance("remember to check config");
            assert!(important_score > normal_score);

            // Text with strong keywords
            let strong_score = extractor.assess_importance("always use this pattern");
            assert!(strong_score > normal_score);

            // Text with list marker
            let list_score = extractor.assess_importance("- important item");
            assert!(list_score > normal_score);
        }

        #[test]
        fn test_insight_source_variants() {
            assert!(matches!(
                InsightSource::UserMessage,
                InsightSource::UserMessage
            ));
            assert!(matches!(
                InsightSource::AssistantResponse,
                InsightSource::AssistantResponse
            ));
            assert!(matches!(
                InsightSource::ToolResult,
                InsightSource::ToolResult
            ));
            assert!(matches!(
                InsightSource::SessionSummary,
                InsightSource::SessionSummary
            ));
        }
    }

    // ========== Utility Tests ==========
    mod utils_tests {
        use super::*;
        use crate::utils::*;
        use crate::prefetch::MemorySource;

        #[test]
        fn test_estimate_tokens() {
            let tokens = estimate_tokens("Hello world");
            assert!(tokens >= 2);
        }

        #[test]
        fn test_estimate_tokens_empty() {
            let tokens = estimate_tokens("");
            assert!(tokens < 20);
        }

        #[test]
        fn test_estimate_tokens_long_text() {
            let text = "a".repeat(1000);
            let tokens = estimate_tokens(&text);
            // 1000/4 + 10 = 260
            assert!(tokens >= 250 && tokens <= 270);
        }

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
        }

        #[test]
        fn test_truncate_to_token_budget_at_newline() {
            let text = "line1\nline2\nline3";
            let truncated = truncate_to_token_budget(text, 1); // 4 chars max
            assert!(!truncated.contains('\n'));
        }

        #[test]
        fn test_truncate_to_token_budget_at_space() {
            let text = "word1 word2 word3";
            let truncated = truncate_to_token_budget(text, 1); // 4 chars max
            assert!(!truncated.ends_with(' '));
        }

        #[test]
        fn test_is_memory_context_true() {
            assert!(is_memory_context("<memory-context>test</memory-context>"));
            assert!(is_memory_context("prefix <memory-context>content</memory-context> suffix"));
        }

        #[test]
        fn test_is_memory_context_false() {
            assert!(!is_memory_context("normal text"));
            assert!(!is_memory_context("<other-tag></other-tag>"));
            assert!(!is_memory_context("<memory-context>unclosed"));
        }

        #[test]
        fn test_extract_memory_context() {
            let content = "prefix <memory-context>extracted content</memory-context> suffix";
            let extracted = extract_memory_context(content);
            assert!(extracted.is_some());
            let extracted = extracted.unwrap();
            assert!(extracted.contains("<memory-context>"));
            assert!(extracted.contains("</memory-context>"));
            assert!(extracted.contains("extracted content"));
        }

        #[test]
        fn test_extract_memory_context_none() {
            assert_eq!(extract_memory_context("no tags"), None);
            assert_eq!(extract_memory_context("<memory-context>unclosed"), None);
        }

        #[test]
        fn test_sanitize_for_injection() {
            let input = "text <memory-context>old</memory-context> more";
            let sanitized = sanitize_for_injection(input);
            assert!(!sanitized.contains("<memory-context>"));
            assert!(sanitized.contains("text"));
            assert!(sanitized.contains("more"));
        }

        #[test]
        fn test_sanitize_for_injection_nested() {
            let input = "<memory-context>outer <memory-context>inner</memory-context> content</memory-context>";
            let sanitized = sanitize_for_injection(input);
            assert!(!sanitized.contains("<memory-context>"));
        }

        #[test]
        fn test_build_memory_header() {
            let header = build_memory_header();
            assert!(header.contains("<memory-context>"));
            assert!(header.contains("recalled memory"));
        }

        #[test]
        fn test_build_memory_footer() {
            let footer = build_memory_footer();
            assert_eq!(footer, "\n</memory-context>");
        }

        #[test]
        fn test_wrap_memory_context() {
            let content = "test content";
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
        fn test_source_from_path_session() {
            let path = std::path::Path::new("/home/.heartq/memory/abc/sessions/2024-01.md");
            assert_eq!(source_from_path(path), MemorySource::Session);
        }

        #[test]
        fn test_source_from_path_global() {
            let path = std::path::Path::new("/home/.heartq/memory/MEMORY.md");
            assert_eq!(source_from_path(path), MemorySource::Global);
        }

        #[test]
        fn test_source_from_path_workspace() {
            let path = std::path::Path::new("/home/.heartq/memory/abc/MEMORY.md");
            assert_eq!(source_from_path(path), MemorySource::Workspace);
        }

        #[test]
        fn test_is_valid_memory_path_valid() {
            assert!(is_valid_memory_path(std::path::Path::new("/test.rs")));
            assert!(is_valid_memory_path(std::path::Path::new("/test.md")));
        }

        #[test]
        fn test_is_valid_memory_path_invalid() {
            assert!(!is_valid_memory_path(std::path::Path::new("/test.png")));
            assert!(!is_valid_memory_path(std::path::Path::new("/test.jpg")));
            assert!(!is_valid_memory_path(std::path::Path::new("/test.pdf")));
            assert!(!is_valid_memory_path(std::path::Path::new("/test.mp3")));
            assert!(!is_valid_memory_path(std::path::Path::new("/test.mp4")));
            assert!(!is_valid_memory_path(std::path::Path::new("/test.zip")));
        }

        #[test]
        fn test_generate_memory_id() {
            let id = generate_memory_id(std::path::Path::new("/test.md"), 10);
            assert_eq!(id.len(), 16);
            assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        }

        #[test]
        fn test_generate_memory_id_deterministic() {
            let id1 = generate_memory_id(std::path::Path::new("/test.md"), 5);
            let id2 = generate_memory_id(std::path::Path::new("/test.md"), 5);
            assert_eq!(id1, id2);
        }

        #[test]
        fn test_parse_memory_id() {
            assert_eq!(parse_memory_id("anything"), None);
        }

        #[test]
        fn test_clean_markdown() {
            let input = "\n\n# Title\n\nContent\n\n";
            let cleaned = clean_markdown(input);
            assert!(!cleaned.starts_with('\n'));
            assert!(cleaned.contains("# Title"));
            assert!(cleaned.contains("Content"));
        }

        #[test]
        fn test_format_memory_block() {
            let block = crate::utils::test_block(
                "test content",
                MemorySource::Global,
                0.95,
                "/path.md",
                10,
                20,
            );
            let formatted = format_memory_block(&block);
            assert!(formatted.contains("global memory"));
            assert!(formatted.contains("score: 0.95"));
        }
    }

    // ========== Error Tests ==========
    mod error_tests {
        use super::*;
        use crate::error::MemoryError;

        #[test]
        fn test_error_is_transient_timeout() {
            let timeout_err = MemoryError::Timeout(30);
            assert!(timeout_err.is_transient());
        }

        #[test]
        fn test_error_is_transient_read() {
            let read_err = MemoryError::ReadError(std::io::Error::new(
                std::io::ErrorKind::Other,
                "read failed",
            ));
            assert!(read_err.is_transient());
        }

        #[test]
        fn test_error_is_transient_write() {
            let write_err = MemoryError::WriteError(std::io::Error::new(
                std::io::ErrorKind::Other,
                "write failed",
            ));
            assert!(write_err.is_transient());
        }

        #[test]
        fn test_error_is_not_transient() {
            let config_err = MemoryError::Config("invalid".to_string());
            assert!(!config_err.is_transient());
        }

        #[test]
        fn test_error_user_message_backend_not_available() {
            let err = MemoryError::BackendNotAvailable;
            assert_eq!(err.user_message(), "Memory system not configured");
        }

        #[test]
        fn test_error_user_message_timeout() {
            let err = MemoryError::Timeout(30);
            assert!(err.user_message().contains("30"));
        }

        #[test]
        fn test_error_user_message_path_not_found() {
            let err = MemoryError::PathNotFound("/test/path".to_string());
            assert!(err.user_message().contains("/test/path"));
        }

        #[test]
        fn test_error_user_message_permission_denied() {
            let err = MemoryError::PermissionDenied("/test".to_string());
            assert!(err.user_message().contains("Permission denied"));
        }

        #[test]
        fn test_error_debug() {
            let err = MemoryError::Backend("test error".to_string());
            let debug_str = format!("{:?}", err);
            assert!(debug_str.contains("Backend"));
        }

        #[test]
        fn test_error_display() {
            let err = MemoryError::SearchError("query failed".to_string());
            let display_str = err.to_string();
            assert!(display_str.contains("Search failed"));
            assert!(display_str.contains("query failed"));
        }

        #[test]
        fn test_error_from_io() {
            let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
            let mem_err: MemoryError = MemoryError::ReadError(io_err);
            assert!(mem_err.is_transient());
        }

        #[test]
        fn test_error_config() {
            let err = MemoryError::Config("missing field".to_string());
            assert!(err.to_string().contains("missing field"));
        }

        #[test]
        fn test_error_invalid_config() {
            let err = MemoryError::InvalidConfig {
                field: "max_tokens".to_string(),
                message: "must be positive".to_string(),
            };
            assert!(err.to_string().contains("max_tokens"));
            assert!(err.to_string().contains("must be positive"));
        }

        #[test]
        fn test_error_other() {
            let err = MemoryError::Other("unknown error".to_string());
            assert!(err.to_string().contains("unknown error"));
        }
    }
}
