//! Phase 2 tests for compaction components.
//!
//! This module contains unit tests for Phase 2 components:
//! - [`ToolPrunerConfig`](precompaction::ToolPrunerConfig) and [`ToolResultPruner`](precompaction::ToolResultPruner)
//! - [`EnhancedSummaryTemplate`](inter_compaction::EnhancedSummaryTemplate) and [`SummaryBuilder`](inter_compaction::EnhancedSummaryBuilder)
//! - [`CompressionState`](state::CompressionState) and related utilities
//! - [`CompressionLock`](lock::CompressionLock) and [`init_lock_table`](lock::init_lock_table)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::precompaction::{CompactionItem, PruneStats, ToolPrunerConfig, ToolResultPruner};
    use crate::inter_compaction::enhanced_summary::{
        EnhancedSummaryTemplate, SummaryBuilder, SummarySection,
    };
    use crate::lock::{init_lock_table, CompressionLock};

    // ========== Mock Structures for Testing ==========
    // Note: CompressionState is referenced in lib.rs but not yet implemented.
    // These mocks allow testing the integration points.

    mod compression_state_mocks {
        use super::*;

        /// Mock compression state for testing anti-jitter behavior.
        /// 
        /// Tracks ineffective compression attempts and blocks further
        /// compression after exceeding a threshold to prevent thrashing.
        #[derive(Debug, Clone, Default)]
        pub struct CompressionState {
            /// Number of consecutive ineffective compressions
            pub ineffective_compression_count: usize,
            /// Maximum ineffective compressions before blocking
            pub max_ineffective: usize,
            /// Whether compression is currently blocked
            is_blocked: bool,
        }

        impl CompressionState {
            pub fn new(max_ineffective: usize) -> Self {
                Self {
                    max_ineffective,
                    ..Default::default()
                }
            }

            /// Check if compression should be blocked due to ineffective attempts
            pub fn is_blocked(&self) -> bool {
                self.ineffective_compression_count >= self.max_ineffective || self.is_blocked
            }

            /// Record a compression attempt that didn't reduce context
            pub fn record_ineffective(&mut self) {
                self.ineffective_compression_count += 1;
            }

            /// Record successful compression
            pub fn record_successful(&mut self) {
                self.ineffective_compression_count = 0;
                self.is_blocked = false;
            }

            /// Reset state (e.g., on new conversation turn)
            pub fn reset(&mut self) {
                self.ineffective_compression_count = 0;
                self.is_blocked = false;
            }

            /// Force block compression (e.g., after fallback was used)
            pub fn force_block(&mut self) {
                self.is_blocked = true;
            }
        }
    }

    // ========== ToolPruner Tests ==========
    mod tool_pruner_tests {
        use super::*;

        #[test]
        fn test_pruner_config_default() {
            let config = ToolPrunerConfig::default();
            assert_eq!(config.max_result_length, 500);
            assert_eq!(config.dedup_window, 10);
            assert_eq!(config.min_length_for_summary, 200);
        }

        #[test]
        fn test_pruner_config_custom() {
            let config = ToolPrunerConfig {
                max_result_length: 1000,
                dedup_window: 5,
                min_length_for_summary: 500,
            };
            assert_eq!(config.max_result_length, 1000);
            assert_eq!(config.dedup_window, 5);
        }

        #[test]
        fn test_prune_stats_default() {
            let stats = PruneStats::default();
            assert_eq!(stats.deduplicated, 0);
            assert_eq!(stats.summarized, 0);
            assert_eq!(stats.truncated, 0);
            assert_eq!(stats.tokens_saved, 0);
        }

        #[test]
        fn test_prune_stats_with_values() {
            let stats = PruneStats {
                deduplicated: 5,
                summarized: 3,
                truncated: 2,
                tokens_saved: 1000,
            };
            assert_eq!(stats.deduplicated, 5);
            assert_eq!(stats.summarized, 3);
            assert_eq!(stats.truncated, 2);
            assert_eq!(stats.tokens_saved, 1000);
        }

        #[test]
        fn test_tool_result_pruner_new() {
            let config = ToolPrunerConfig::default();
            let pruner = ToolResultPruner::new(config);
            
            // Check default templates are registered
            assert!(pruner.summary_templates.contains_key("terminal"));
            assert!(pruner.summary_templates.contains_key("read_file"));
            assert!(pruner.summary_templates.contains_key("search_files"));
            assert!(pruner.summary_templates.contains_key("glob"));
            assert!(pruner.summary_templates.contains_key("browser_navigate"));
            assert!(pruner.summary_templates.contains_key("browser_snapshot"));
        }

        #[test]
        fn test_tool_result_pruner_with_custom_template() {
            let config = ToolPrunerConfig::default();
            let custom_template = |_input: &str, _output: &str| {
                "custom summary".to_string()
            };
            
            let pruner = ToolResultPruner::new(config)
                .with_template("custom_tool", custom_template);
            
            assert!(pruner.summary_templates.contains_key("custom_tool"));
        }

        #[test]
        fn test_compaction_item_trait_default() {
            // Test the blanket implementation for types implementing AsRef<str>
            let text = "Hello, world!";
            assert!(!text.is_tool_result());
            assert!(!text.has_tool_requests());
        }

        #[test]
        fn test_compaction_item_with_tool_calls() {
            let json_with_tools = r#"{"tool_calls": [{"name": "test", "args": {}}]}"#;
            assert!(json_with_tools.has_tool_requests());
        }

        #[test]
        fn test_compaction_item_without_tool_calls() {
            let plain_text = "Just some regular text";
            assert!(!plain_text.has_tool_requests());
        }
    }

    // ========== EnhancedSummaryTemplate Tests ==========
    mod summary_template_tests {
        use super::*;

        #[test]
        fn test_template_new() {
            let template = EnhancedSummaryTemplate::new();
            assert!(template.is_empty());
        }

        #[test]
        fn test_template_default() {
            let template = EnhancedSummaryTemplate::default();
            assert!(template.is_empty());
        }

        #[test]
        fn test_template_is_empty() {
            let empty = EnhancedSummaryTemplate::new();
            assert!(empty.is_empty());
            
            let with_task = EnhancedSummaryTemplate {
                historical_task: "task".to_string(),
                ..Default::default()
            };
            assert!(!with_task.is_empty());
            
            let with_state = EnhancedSummaryTemplate {
                in_progress_state: "state".to_string(),
                ..Default::default()
            };
            assert!(!with_state.is_empty());
            
            let with_pending = EnhancedSummaryTemplate {
                pending_asks: vec!["item".to_string()],
                ..Default::default()
            };
            assert!(!with_pending.is_empty());
            
            let with_remaining = EnhancedSummaryTemplate {
                remaining_work: vec!["item".to_string()],
                ..Default::default()
            };
            assert!(!with_remaining.is_empty());
        }

        #[test]
        fn test_template_format_empty() {
            let template = EnhancedSummaryTemplate::new();
            let formatted = template.format();
            
            // Empty template should still have prefix and end marker
            assert!(formatted.contains("[CONTEXT COMPACTION"));
            assert!(formatted.contains("--- END OF CONTEXT SUMMARY"));
        }

        #[test]
        fn test_template_format_with_task() {
            let template = EnhancedSummaryTemplate {
                historical_task: "Test task".to_string(),
                ..Default::default()
            };
            
            let formatted = template.format();
            assert!(formatted.contains("## Historical Task Snapshot"));
            assert!(formatted.contains("Test task"));
        }

        #[test]
        fn test_template_format_with_in_progress() {
            let template = EnhancedSummaryTemplate {
                in_progress_state: "Working on it".to_string(),
                ..Default::default()
            };
            
            let formatted = template.format();
            assert!(formatted.contains("## Historical In-Progress State"));
            assert!(formatted.contains("Working on it"));
        }

        #[test]
        fn test_template_format_with_pending() {
            let template = EnhancedSummaryTemplate {
                pending_asks: vec![
                    "Question 1".to_string(),
                    "Question 2".to_string(),
                ],
                ..Default::default()
            };
            
            let formatted = template.format();
            assert!(formatted.contains("## Historical Pending User Asks"));
            assert!(formatted.contains("- Question 1"));
            assert!(formatted.contains("- Question 2"));
        }

        #[test]
        fn test_template_format_with_remaining() {
            let template = EnhancedSummaryTemplate {
                remaining_work: vec![
                    "Item 1".to_string(),
                    "Item 2".to_string(),
                ],
                ..Default::default()
            };
            
            let formatted = template.format();
            assert!(formatted.contains("## Historical Remaining Work"));
            assert!(formatted.contains("- Item 1"));
            assert!(formatted.contains("- Item 2"));
        }

        #[test]
        fn test_template_format_full() {
            let template = EnhancedSummaryTemplate {
                historical_task: "Main task".to_string(),
                in_progress_state: "Current progress".to_string(),
                pending_asks: vec!["Q1".to_string()],
                remaining_work: vec!["Work item".to_string()],
            };
            
            let formatted = template.format();
            assert!(formatted.contains("## Historical Task Snapshot"));
            assert!(formatted.contains("Main task"));
            assert!(formatted.contains("## Historical In-Progress State"));
            assert!(formatted.contains("Current progress"));
            assert!(formatted.contains("## Historical Pending User Asks"));
            assert!(formatted.contains("Q1"));
            assert!(formatted.contains("## Historical Remaining Work"));
            assert!(formatted.contains("Work item"));
            assert!(formatted.contains("--- END OF CONTEXT SUMMARY"));
        }

        #[test]
        fn test_template_parse_empty() {
            let template = EnhancedSummaryTemplate::parse("");
            assert!(template.is_empty());
        }

        #[test]
        fn test_template_parse_task_section() {
            let input = r#"## Historical Task Snapshot

This is the historical task content
spanning multiple lines

## Historical In-Progress State

Current state"#;
            
            let template = EnhancedSummaryTemplate::parse(input);
            assert!(template.historical_task.contains("This is the historical task"));
        }

        #[test]
        fn test_template_parse_in_progress_section() {
            let input = r#"## Historical In-Progress State

Currently working on feature X
With some details"#;
            
            let template = EnhancedSummaryTemplate::parse(input);
            assert!(template.in_progress_state.contains("Currently working"));
        }

        #[test]
        fn test_template_parse_pending_list() {
            let input = r#"## Historical Pending User Asks

- First question
- Second question
- Third question"#;
            
            let template = EnhancedSummaryTemplate::parse(input);
            assert_eq!(template.pending_asks.len(), 3);
        }

        #[test]
        fn test_template_parse_remaining_list() {
            let input = r#"## Historical Remaining Work

- Task 1
- Task 2"#;
            
            let template = EnhancedSummaryTemplate::parse(input);
            assert_eq!(template.remaining_work.len(), 2);
        }

        #[test]
        fn test_template_constants() {
            assert!(!EnhancedSummaryTemplate::SUMMARY_PREFIX.is_empty());
            assert!(!EnhancedSummaryTemplate::TASK_HEADING.is_empty());
            assert!(!EnhancedSummaryTemplate::IN_PROGRESS_HEADING.is_empty());
            assert!(!EnhancedSummaryTemplate::PENDING_HEADING.is_empty());
            assert!(!EnhancedSummaryTemplate::REMAINING_HEADING.is_empty());
            assert!(!EnhancedSummaryTemplate::END_MARKER.is_empty());
        }

        #[test]
        fn test_template_update_with_task() {
            let mut template = EnhancedSummaryTemplate {
                historical_task: "Old task".to_string(),
                in_progress_state: "Old state".to_string(),
                ..Default::default()
            };
            
            template.update_with("New task content", SummarySection::Task);
            
            assert_eq!(template.historical_task, "New task content");
            // Old task should move to in_progress
            assert_eq!(template.in_progress_state, "Old task");
        }

        #[test]
        fn test_template_update_with_in_progress() {
            let mut template = EnhancedSummaryTemplate::default();
            template.update_with("New in progress state", SummarySection::InProgress);
            
            assert_eq!(template.in_progress_state, "New in progress state");
        }

        #[test]
        fn test_template_update_with_pending() {
            let mut template = EnhancedSummaryTemplate::default();
            template.update_with("Item 1\nItem 2\nItem 3", SummarySection::Pending);
            
            assert_eq!(template.pending_asks.len(), 3);
            assert!(template.pending_asks.contains(&"Item 1".to_string()));
            assert!(template.pending_asks.contains(&"Item 2".to_string()));
            assert!(template.pending_asks.contains(&"Item 3".to_string()));
        }

        #[test]
        fn test_template_update_with_remaining() {
            let mut template = EnhancedSummaryTemplate::default();
            template.update_with("Work item A\nWork item B", SummarySection::Remaining);
            
            assert_eq!(template.remaining_work.len(), 2);
        }
    }

    // ========== SummaryBuilder Tests ==========
    mod summary_builder_tests {
        use super::*;

        #[test]
        fn test_builder_new() {
            let builder = SummaryBuilder::new();
            let result = builder.build();
            assert!(result.contains("[CONTEXT COMPACTION"));
        }

        #[test]
        fn test_builder_task() {
            let result = SummaryBuilder::new()
                .task("My task")
                .build();
            
            assert!(result.contains("My task"));
        }

        #[test]
        fn test_builder_in_progress() {
            let result = SummaryBuilder::new()
                .in_progress("Current work")
                .build();
            
            assert!(result.contains("Current work"));
        }

        #[test]
        fn test_builder_add_pending() {
            let result = SummaryBuilder::new()
                .add_pending("Pending 1")
                .add_pending("Pending 2")
                .build();
            
            assert!(result.contains("- Pending 1"));
            assert!(result.contains("- Pending 2"));
        }

        #[test]
        fn test_builder_add_remaining() {
            let result = SummaryBuilder::new()
                .add_remaining("Work 1")
                .add_remaining("Work 2")
                .build();
            
            assert!(result.contains("- Work 1"));
            assert!(result.contains("- Work 2"));
        }

        #[test]
        fn test_builder_full() {
            let result = SummaryBuilder::new()
                .task("Main task")
                .in_progress("Doing work")
                .add_pending("Question?")
                .add_remaining("Future work")
                .build();
            
            assert!(result.contains("Main task"));
            assert!(result.contains("Doing work"));
            assert!(result.contains("Question?"));
            assert!(result.contains("Future work"));
            assert!(result.contains("--- END OF CONTEXT SUMMARY"));
        }

        #[test]
        fn test_builder_method_chaining() {
            // Test that builder methods return self for chaining
            let builder = SummaryBuilder::new();
            let _ = builder.task("task");
            let _ = builder.in_progress("state");
            let _ = builder.add_pending("pending");
            let _ = builder.add_remaining("remaining");
        }
    }

    // ========== CompressionState Tests (Mock Implementation) ==========
    mod compression_state_tests {
        use super::compression_state_mocks::*;
        use super::*;

        #[test]
        fn test_state_default_not_blocked() {
            let state = CompressionState::new(3);
            assert!(!state.is_blocked());
        }

        #[test]
        fn test_state_blocked_by_threshold() {
            let mut state = CompressionState::new(3);
            
            state.record_ineffective();
            assert!(!state.is_blocked());
            
            state.record_ineffective();
            assert!(!state.is_blocked());
            
            state.record_ineffective();
            assert!(state.is_blocked());
        }

        #[test]
        fn test_state_successful_resets_count() {
            let mut state = CompressionState::new(2);
            
            state.record_ineffective();
            state.record_ineffective();
            assert!(state.is_blocked());
            
            state.record_successful();
            assert!(!state.is_blocked());
            
            // Should be able to have 2 more ineffective compressions
            state.record_ineffective();
            assert!(!state.is_blocked());
        }

        #[test]
        fn test_state_reset() {
            let mut state = CompressionState::new(1);
            state.record_ineffective();
            assert!(state.is_blocked());
            
            state.reset();
            assert!(!state.is_blocked());
        }

        #[test]
        fn test_state_force_block() {
            let mut state = CompressionState::new(10);
            assert!(!state.is_blocked());
            
            state.force_block();
            assert!(state.is_blocked());
        }

        #[test]
        fn test_state_record_fallback_pattern() {
            let mut state = CompressionState::new(2);
            
            // Simulate fallback usage
            state.record_ineffective();
            state.record_ineffective();
            assert!(state.is_blocked());
            
            // Reset for next session
            state.reset();
            state.record_successful();
            assert!(!state.is_blocked());
        }

        #[test]
        fn test_state_anti_jitter_concept() {
            // Test the anti-jitter pattern: block compression after
            // consecutive ineffective attempts
            let mut state = CompressionState::new(2);
            
            // First compression attempt
            state.record_ineffective();
            assert!(!state.is_blocked());
            
            // Second compression attempt still ineffective
            state.record_ineffective();
            assert!(state.is_blocked());
            
            // Block prevents further attempts
            // (In real implementation, this would prevent compression calls)
        }
    }

    // ========== CompressionLock Tests ==========
    mod compression_lock_tests {
        use super::*;
        use tempfile::tempdir;

        #[test]
        fn test_generate_holder_id() {
            let id1 = CompressionLock::generate_holder_id();
            let id2 = CompressionLock::generate_holder_id();
            
            assert!(id1.starts_with("pid="));
            assert_ne!(id1, id2); // Should be unique per call
        }

        #[test]
        fn test_generate_holder_id_format() {
            let id = CompressionLock::generate_holder_id();
            
            // Format: pid={pid}:{hash}
            assert!(id.starts_with("pid="));
            assert!(id.contains(":"));
        }

        #[test]
        fn test_init_lock_table() {
            let temp_dir = tempdir().unwrap();
            let db_path = temp_dir.path().join("test.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            
            init_lock_table(&conn).unwrap();
            
            // Verify table exists
            let count: i32 = conn.query_row(
                "SELECT COUNT(*) FROM compression_locks",
                [],
                |row| row.get(0),
            ).unwrap();
            assert_eq!(count, 0);
        }

        #[test]
        fn test_lock_acquire_success() {
            let temp_dir = tempdir().unwrap();
            let db_path = temp_dir.path().join("test.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            init_lock_table(&conn).unwrap();
            
            let lock = CompressionLock::try_acquire(&conn, "session-1", 60).unwrap();
            
            assert!(lock.is_some());
            let lock = lock.unwrap();
            assert_eq!(lock.session_id(), "session-1");
            assert!(lock.is_valid());
            assert!(lock.holder_id().starts_with("pid="));
        }

        #[test]
        fn test_lock_acquire_and_release() {
            let temp_dir = tempdir().unwrap();
            let db_path = temp_dir.path().join("test.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            init_lock_table(&conn).unwrap();
            
            // Acquire lock
            let lock = CompressionLock::try_acquire(&conn, "session-1", 60).unwrap().unwrap();
            assert_eq!(lock.session_id(), "session-1");
            assert!(lock.is_valid());
            
            // Release lock
            lock.release().unwrap();
            
            // Can acquire again after release
            let lock2 = CompressionLock::try_acquire(&conn, "session-1", 60).unwrap();
            assert!(lock2.is_some());
        }

        #[test]
        fn test_lock_prevents_concurrent_acquire() {
            let temp_dir = tempdir().unwrap();
            let db_path = temp_dir.path().join("test.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            init_lock_table(&conn).unwrap();
            
            // First acquire succeeds
            let lock1 = CompressionLock::try_acquire(&conn, "session-1", 60).unwrap();
            assert!(lock1.is_some());
            
            // Second acquire for same session fails
            let lock2 = CompressionLock::try_acquire(&conn, "session-1", 60).unwrap();
            assert!(lock2.is_none());
        }

        #[test]
        fn test_lock_allows_different_sessions() {
            let temp_dir = tempdir().unwrap();
            let db_path = temp_dir.path().join("test.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            init_lock_table(&conn).unwrap();
            
            // Acquire lock for session-1
            let lock1 = CompressionLock::try_acquire(&conn, "session-1", 60).unwrap();
            assert!(lock1.is_some());
            
            // Can still acquire for different session
            let lock2 = CompressionLock::try_acquire(&conn, "session-2", 60).unwrap();
            assert!(lock2.is_some());
            
            // And another
            let lock3 = CompressionLock::try_acquire(&conn, "session-3", 60).unwrap();
            assert!(lock3.is_some());
        }

        #[test]
        fn test_lock_refresh() {
            let temp_dir = tempdir().unwrap();
            let db_path = temp_dir.path().join("test.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            init_lock_table(&conn).unwrap();
            
            let lock = CompressionLock::try_acquire(&conn, "session-1", 60).unwrap().unwrap();
            
            // Refresh should succeed
            let refreshed = lock.refresh().unwrap();
            assert!(refreshed);
        }

        #[test]
        fn test_lock_is_valid() {
            let temp_dir = tempdir().unwrap();
            let db_path = temp_dir.path().join("test.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            init_lock_table(&conn).unwrap();
            
            let lock = CompressionLock::try_acquire(&conn, "session-1", 1).unwrap().unwrap();
            
            // Immediately after acquire, should be valid
            assert!(lock.is_valid());
        }

        #[test]
        fn test_default_ttl() {
            assert_eq!(
                CompressionLock::DEFAULT_TTL.as_secs(),
                300
            );
        }

        #[test]
        fn test_default_refresh_interval() {
            let interval = CompressionLock::default_refresh_interval();
            assert_eq!(interval, CompressionLock::DEFAULT_TTL / 2);
        }
    }

    // ========== Integration Tests ==========
    mod integration_tests {
        use super::*;

        #[test]
        fn test_tool_pruner_with_mock_items() {
            // Mock item type for testing
            struct MockItem {
                content: String,
                is_tool: bool,
                has_calls: bool,
            }
            
            impl AsRef<str> for MockItem {
                fn as_ref(&self) -> &str {
                    &self.content
                }
            }
            
            impl CompactionItem for MockItem {
                fn is_tool_result(&self) -> bool {
                    self.is_tool
                }
                
                fn has_tool_requests(&self) -> bool {
                    self.has_calls
                }
            }
            
            let config = ToolPrunerConfig::default();
            let pruner = ToolResultPruner::new(config);
            
            let mut items = vec![
                MockItem {
                    content: "Some tool result".to_string(),
                    is_tool: true,
                    has_calls: false,
                },
                MockItem {
                    content: r#"{"tool_calls": [{"name": "test"}]}"#.to_string(),
                    is_tool: false,
                    has_calls: true,
                },
            ];
            
            let stats = pruner.prune(&mut items);
            
            // Just verify no panics and stats are returned
            assert_eq!(stats.deduplicated + stats.summarized + stats.truncated, 0);
        }

        #[test]
        fn test_summary_roundtrip() {
            let original = EnhancedSummaryTemplate {
                historical_task: "Original task".to_string(),
                in_progress_state: "Original state".to_string(),
                pending_asks: vec!["Ask 1".to_string()],
                remaining_work: vec!["Work 1".to_string()],
            };
            
            // Format to string
            let formatted = original.format();
            
            // Parse back
            let parsed = EnhancedSummaryTemplate::parse(&formatted);
            
            assert!(parsed.historical_task.contains("Original task"));
        }

        #[test]
        fn test_lock_table_creation() {
            let temp_dir = tempdir().unwrap();
            let db_path = temp_dir.path().join("test.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            
            // Create table
            init_lock_table(&conn).unwrap();
            
            // Insert a lock
            conn.execute(
                "INSERT INTO compression_locks (session_id, holder, ttl_until) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    "test-session",
                    "test-holder",
                    "2025-01-01T00:00:00Z"
                ],
            ).unwrap();
            
            // Verify
            let count: i32 = conn.query_row(
                "SELECT COUNT(*) FROM compression_locks",
                [],
                |row| row.get(0),
            ).unwrap();
            
            assert_eq!(count, 1);
        }
    }
}
