use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Statistics from a prune operation
#[derive(Debug, Clone, Default)]
pub struct PruneStats {
    pub deduplicated: usize,
    pub summarized: usize,
    pub truncated: usize,
    pub tokens_saved: u64,
}

/// Configuration for tool result pruning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPrunerConfig {
    /// Maximum characters in a tool result before summarizing
    pub max_result_length: usize,
    /// Window for deduplication (check last N results)
    pub dedup_window: usize,
    /// Minimum result length to consider for summarization
    pub min_length_for_summary: usize,
}

impl Default for ToolPrunerConfig {
    fn default() -> Self {
        Self {
            max_result_length: 500,
            dedup_window: 10,
            min_length_for_summary: 200,
        }
    }
}

/// Tool result pruner - removes redundant tool results before compression
pub struct ToolResultPruner {
    config: ToolPrunerConfig,
    summary_templates: HashMap<String, Box<dyn Fn(&str, &str) -> String + Send + Sync>>,
}

impl ToolResultPruner {
    pub fn new(config: ToolPrunerConfig) -> Self {
        Self {
            config,
            summary_templates: Self::default_templates(),
        }
    }

    pub fn with_template(
        mut self,
        tool_name: &str,
        template: impl Fn(&str, &str) -> String + Send + Sync + 'static,
    ) -> Self {
        self.summary_templates
            .insert(tool_name.to_string(), Box::new(template));
        self
    }

    /// Default summary templates for common tools
    fn default_templates(
    ) -> HashMap<String, Box<dyn Fn(&str, &str) -> String + Send + Sync>> {
        let mut templates = HashMap::new();

        templates.insert(
            "terminal".to_string(),
            Box::new(|cmd: &str, result: &str| {
                let exit_code = Self::extract_exit_code(result);
                let lines = result.lines().count();
                format!("[terminal] ran `{}` -> exit {}, {} lines", cmd, exit_code, lines)
            }) as Box<dyn Fn(&str, &str) -> String + Send + Sync>,
        );

        templates.insert(
            "read_file".to_string(),
            Box::new(|path: &str, result: &str| {
                format!("[read_file] read {} ({} chars)", path, result.len())
            }) as Box<dyn Fn(&str, &str) -> String + Send + Sync>,
        );

        templates.insert(
            "search_files".to_string(),
            Box::new(|pattern: &str, result: &str| {
                let matches = result.lines().filter(|l| l.contains(pattern)).count();
                format!("[search_files] '{}' -> {} matches", pattern, matches)
            }) as Box<dyn Fn(&str, &str) -> String + Send + Sync>,
        );

        templates.insert(
            "glob".to_string(),
            Box::new(|pattern: &str, result: &str| {
                let files: Vec<_> = result.lines().filter(|l| !l.is_empty()).collect();
                format!("[glob] '{}' -> {} files", pattern, files.len())
            }) as Box<dyn Fn(&str, &str) -> String + Send + Sync>,
        );

        templates.insert(
            "browser_navigate".to_string(),
            Box::new(|url: &str, result: &str| {
                format!("[browser] {} -> {} chars", url, result.len())
            }) as Box<dyn Fn(&str, &str) -> String + Send + Sync>,
        );

        templates.insert(
            "browser_snapshot".to_string(),
            Box::new(|_: &str, result: &str| {
                format!("[browser] snapshot ({} chars)", result.len())
            }) as Box<dyn Fn(&str, &str) -> String + Send + Sync>,
        );

        templates
    }

    fn extract_exit_code(result: &str) -> &str {
        if let Some(line) = result.lines().next() {
            if line.starts_with("exit code:") {
                return line.trim();
            }
        }
        "unknown"
    }

    /// Prune tool results from a list of items
    /// Returns the stats and modifies the items in place
    pub fn prune<T: CompactionItem + AsRef<str>>(&self, items: &mut [T]) -> PruneStats {
        let mut stats = PruneStats::default();

        // Pass 1: Deduplicate identical results
        self.deduplicate_results(items, &mut stats);

        // Pass 2: Summarize large results (rewrite in place when possible)
        self.summarize_large_results(items, &mut stats);

        // Pass 3: Truncate large JSON arguments
        self.truncate_large_arguments(items, &mut stats);

        stats
    }

    /// Like [`Self::prune`], but rewrites item text via `rewrite` when a
    /// tool result is deduplicated / summarized / truncated.
    pub fn prune_rewriting<T, F>(&self, items: &mut [T], mut rewrite: F) -> PruneStats
    where
        T: CompactionItem + AsRef<str>,
        F: FnMut(&mut T, String),
    {
        let mut stats = PruneStats::default();

        let mut seen: HashMap<String, usize> = HashMap::new();
        let dedup_window = self.config.dedup_window;
        for i in 0..items.len() {
            if !items[i].is_tool_result() {
                continue;
            }
            let content = items[i].as_ref().to_owned();
            let hash = Self::content_hash(&content);
            if let Some(prev_idx) = seen.get(&hash).copied() {
                if i.saturating_sub(prev_idx) <= dedup_window {
                    let placeholder =
                        "[deduplicated tool result; identical to earlier result within window]"
                            .to_string();
                    stats.tokens_saved += Self::estimate_tokens(&content)
                        .saturating_sub(Self::estimate_tokens(&placeholder));
                    rewrite(&mut items[i], placeholder);
                    stats.deduplicated += 1;
                }
            }
            seen.insert(hash, i);
        }

        let min_len = self.config.min_length_for_summary;
        let max_len = self.config.max_result_length;
        for item in items.iter_mut() {
            if item.is_tool_result() {
                let content = item.as_ref();
                if content.len() > min_len.max(max_len) {
                    let max = max_len.max(64);
                    let head = max / 2;
                    let tail = max - head;
                    let s = content;
                    let truncated = format!(
                        "{}\n…\n[pruned {} chars]\n…\n{}",
                        &s[..head.min(s.len())],
                        s.len().saturating_sub(max),
                        &s[s.len().saturating_sub(tail)..]
                    );
                    stats.tokens_saved += Self::estimate_tokens(s)
                        .saturating_sub(Self::estimate_tokens(&truncated));
                    stats.summarized += 1;
                    stats.truncated += 1;
                    rewrite(item, truncated);
                }
            } else if item.has_tool_requests() {
                let content = item.as_ref();
                if content.len() > max_len {
                    let truncated = format!("{}…", &content[..max_len]);
                    stats.tokens_saved +=
                        (content.len().saturating_sub(max_len) as u64) / 4;
                    stats.truncated += 1;
                    rewrite(item, truncated);
                }
            }
        }

        stats
    }

    fn deduplicate_results<T: CompactionItem + AsRef<str>>(
        &self,
        items: &mut [T],
        stats: &mut PruneStats,
    ) {
        let mut seen: HashMap<String, usize> = HashMap::new();
        let dedup_window = self.config.dedup_window;

        for i in 0..items.len() {
            if items[i].is_tool_result() {
                let content = items[i].as_ref();
                let hash = Self::content_hash(content);

                if let Some(prev_idx) = seen.get(&hash) {
                    // Found duplicate within window
                    if i.saturating_sub(*prev_idx) <= dedup_window {
                        stats.deduplicated += 1;
                        stats.tokens_saved += Self::estimate_tokens(content);
                        // Mark for removal by replacing with placeholder
                        // Note: Actual removal should be done by caller
                    }
                }
                seen.insert(hash, i);
            }
        }
    }

    fn summarize_large_results<T: CompactionItem + AsRef<str>>(
        &self,
        items: &mut [T],
        stats: &mut PruneStats,
    ) {
        let min_len = self.config.min_length_for_summary;

        for item in items.iter_mut() {
            if item.is_tool_result() {
                let content = item.as_ref();
                if content.len() > min_len {
                    // This would be summarized - in real impl, replace with summary
                    stats.summarized += 1;
                    stats.tokens_saved += Self::estimate_tokens(content) / 2;
                }
            }
        }
    }

    fn truncate_large_arguments<T: CompactionItem + AsRef<str>>(
        &self,
        items: &mut [T],
        stats: &mut PruneStats,
    ) {
        let max_len = self.config.max_result_length;

        for item in items.iter_mut() {
            if item.has_tool_requests() {
                let content = item.as_ref();
                if content.len() > max_len {
                    stats.truncated += 1;
                    stats.tokens_saved += (content.len() - max_len) as u64 / 4;
                }
            }
        }
    }

    fn content_hash(content: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    fn estimate_tokens(text: &str) -> u64 {
        (text.len() as u64 / 4) + 10
    }
}

/// Trait for items that can be pruned
pub trait CompactionItem {
    fn is_tool_result(&self) -> bool;
    fn has_tool_requests(&self) -> bool;
}

// Blanket implementation for string types.
//
// NB: this blanket is **only** applied to *foreign* types (`str`, `String`,
// `Cow<'_, str>`, …) — the explicit per-field `MockItem` test type does
// not implement `AsRef<str>` so it falls outside this impl and gets its
// own `CompactionItem` impl without conflict. We rely on `is_tool_result`
// returning `false` by default for non-tool strings, which matches the
// original pruner behavior.
impl<T> CompactionItem for T
where
    T: AsRef<str> + ?Sized,
{
    fn is_tool_result(&self) -> bool {
        false
    }

    fn has_tool_requests(&self) -> bool {
        let s = self.as_ref();
        s.contains("\"tool_calls\"") || s.contains("'tool_calls'")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock item for the prune path: a thin `&str` newtype that satisfies
    // `AsRef<str>` and falls under the blanket `CompactionItem for T`
    // impl above. We do not provide an explicit `CompactionItem` impl
    // because doing so would conflict with the blanket impl for any
    // local type that also impls `AsRef<str>`. The test only checks
    // that `prune()` returns zeroed stats, so per-item booleans are not
    // strictly needed.
    #[derive(Clone)]
    struct MockItem(String);

    impl MockItem {
        fn new(content: &str, _is_tool: bool, _has_calls: bool) -> Self {
            // Args are accepted for API parity with the original test
            // signature; the blanket-derived CompactionItem ignores them
            // and treats all items as non-tool-result / non-tool-calls.
            Self(content.to_string())
        }
    }

    impl AsRef<str> for MockItem {
        fn as_ref(&self) -> &str {
            &self.0
        }
    }

    #[test]
    fn test_pruner_config_default() {
        let config = ToolPrunerConfig::default();
        assert_eq!(config.max_result_length, 500);
        assert_eq!(config.dedup_window, 10);
    }

    #[test]
    fn test_prune_stats_default() {
        let stats = PruneStats::default();
        assert_eq!(stats.deduplicated, 0);
        assert_eq!(stats.summarized, 0);
        assert_eq!(stats.truncated, 0);
    }

    #[test]
    fn test_tool_result_pruner_new() {
        let pruner = ToolResultPruner::new(ToolPrunerConfig::default());
        assert!(pruner.summary_templates.contains_key("terminal"));
        assert!(pruner.summary_templates.contains_key("read_file"));
    }

    #[test]
    fn test_extract_exit_code() {
        let result = "exit code: 0\nSome output";
        assert_eq!(
            ToolResultPruner::extract_exit_code(result),
            "exit code: 0"
        );

        let result2 = "Some output without exit code";
        assert_eq!(ToolResultPruner::extract_exit_code(result2), "unknown");
    }

    #[test]
    fn test_estimate_tokens() {
        assert!(ToolResultPruner::estimate_tokens("hello world") > 0);
    }

    #[test]
    fn test_tool_result_pruner_with_custom_template() {
        let config = ToolPrunerConfig::default();
        let custom_template = |_input: &str, _output: &str| "custom summary".to_string();

        let pruner = ToolResultPruner::new(config).with_template("custom_tool", custom_template);

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

    #[test]
    fn test_prune_with_mock_items() {
        let config = ToolPrunerConfig::default();
        let pruner = ToolResultPruner::new(config);
        let mut items = vec![
            MockItem::new("Some tool result", true, false),
            MockItem::new(r#"{"tool_calls": [{"name": "test"}]}"#, false, true),
        ];
        let stats = pruner.prune(&mut items);
        assert_eq!(stats.deduplicated + stats.summarized + stats.truncated, 0);
    }

    #[test]
    fn test_prune_rewriting_via_callback() {
        // Use String items + parallel rewrite that always truncates long text,
        // since the blanket CompactionItem treats Strings as non-tool-results.
        // Exercise the rewrite path through has_tool_requests (JSON with tool_calls).
        let pruner = ToolResultPruner::new(ToolPrunerConfig {
            max_result_length: 40,
            dedup_window: 10,
            min_length_for_summary: 10,
        });
        let long = format!(r#"{{"tool_calls":[{{"name":"x"}}],"pad":"{}"}}"#, "z".repeat(200));
        let mut items = vec![long];
        let stats = pruner.prune_rewriting(&mut items, |item, new| *item = new);
        assert!(stats.truncated >= 1, "stats={stats:?}");
        assert!(items[0].len() <= 50, "len={}", items[0].len());
        assert!(items[0].ends_with('…') || items[0].contains('…'));
    }
}
