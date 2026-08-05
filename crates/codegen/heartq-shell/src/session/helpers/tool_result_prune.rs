//! Hermes-style tool-result pruning for full-replace compaction input.
//!
//! Applies [`heartq_compaction::ToolPrunerConfig`] to
//! [`ConversationItem`] tool results: deduplicate identical results within a
//! window, and soft-trim oversized payloads before the summarizer runs.

use std::collections::HashMap;
use std::sync::Arc;

use heartq_compaction::{PruneStats, ToolPrunerConfig};
use heartq_sampling_types::ConversationItem;

/// Mutate `items` in place; returns prune statistics.
pub fn prune_conversation_tool_results(
    items: &mut [ConversationItem],
    config: &ToolPrunerConfig,
) -> PruneStats {
    let mut stats = PruneStats::default();

    // Pass 1: deduplicate identical tool results within the window.
    let mut seen: HashMap<u64, usize> = HashMap::new();
    for i in 0..items.len() {
        let content = match &items[i] {
            ConversationItem::ToolResult(tr) => tr.content.as_ref().to_owned(),
            _ => continue,
        };
        let hash = content_hash(&content);
        if let Some(&prev) = seen.get(&hash) {
            if i.saturating_sub(prev) <= config.dedup_window {
                let placeholder =
                    "[deduplicated tool result; identical to an earlier result within window]";
                stats.tokens_saved += estimate_tokens(&content)
                    .saturating_sub(estimate_tokens(placeholder));
                if let ConversationItem::ToolResult(tr) = &mut items[i] {
                    tr.content = Arc::<str>::from(placeholder);
                }
                stats.deduplicated += 1;
            }
        }
        seen.insert(hash, i);
    }

    // Pass 2: soft-trim oversized tool results (keep head + tail).
    for item in items.iter_mut() {
        let ConversationItem::ToolResult(tr) = item else {
            continue;
        };
        let len = tr.content.len();
        if len <= config.max_result_length {
            continue;
        }
        if len < config.min_length_for_summary {
            continue;
        }
        let max = config.max_result_length.max(64);
        let head = max / 2;
        let tail = max - head;
        let s = tr.content.as_ref();
        let (head_s, tail_s) = utf8_head_tail(s, head, tail);
        let truncated = format!(
            "{}\n…\n[pruned {} chars]\n…\n{}",
            head_s,
            len - max,
            tail_s
        );
        stats.tokens_saved += estimate_tokens(s).saturating_sub(estimate_tokens(&truncated));
        stats.truncated += 1;
        stats.summarized += 1;
        tr.content = Arc::<str>::from(truncated);
    }

    stats
}

/// Split `s` into a UTF-8-safe head (≤ `head_bytes`) and tail (≤ `tail_bytes`).
///
/// Byte budgets are floored to the previous char boundary so multi-byte
/// characters (e.g. CJK) never panic on `&s[i..]`.
fn utf8_head_tail(s: &str, head_bytes: usize, tail_bytes: usize) -> (&str, &str) {
    let len = s.len();
    let head_end = floor_char_boundary(s, head_bytes.min(len));
    let tail_start = ceil_char_boundary(s, len.saturating_sub(tail_bytes));
    // Avoid overlapping head/tail when the string is only moderately long.
    let tail_start = tail_start.max(head_end);
    (&s[..head_end], &s[tail_start..])
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn content_hash(content: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

fn estimate_tokens(text: &str) -> u64 {
    (text.len() as u64 / 4) + 10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_long_tool_result() {
        let mut items = vec![ConversationItem::tool_result(
            "c1",
            "x".repeat(2_000),
        )];
        let cfg = ToolPrunerConfig {
            max_result_length: 200,
            dedup_window: 10,
            min_length_for_summary: 100,
        };
        let stats = prune_conversation_tool_results(&mut items, &cfg);
        assert!(stats.truncated >= 1);
        if let ConversationItem::ToolResult(tr) = &items[0] {
            assert!(tr.content.len() < 2_000);
            assert!(tr.content.contains("[pruned"));
        } else {
            panic!("expected tool result");
        }
    }

    #[test]
    fn dedups_identical_results() {
        let body = "same payload ".repeat(20);
        let mut items = vec![
            ConversationItem::tool_result("a", body.clone()),
            ConversationItem::user("ok"),
            ConversationItem::tool_result("b", body),
        ];
        let cfg = ToolPrunerConfig::default();
        let stats = prune_conversation_tool_results(&mut items, &cfg);
        assert_eq!(stats.deduplicated, 1);
        if let ConversationItem::ToolResult(tr) = &items[2] {
            assert!(tr.content.contains("deduplicated"));
        }
    }

    #[test]
    fn truncates_multibyte_utf8_without_panic() {
        // Reproduces the production panic: byte index landing inside a CJK
        // char (e.g. '明' spans 3 bytes).
        let body = "说明".repeat(200);
        assert!(body.len() > 250);
        let mut items = vec![ConversationItem::tool_result("cjk", body)];
        let cfg = ToolPrunerConfig {
            max_result_length: 250,
            dedup_window: 10,
            min_length_for_summary: 100,
        };
        let stats = prune_conversation_tool_results(&mut items, &cfg);
        assert!(stats.truncated >= 1);
        if let ConversationItem::ToolResult(tr) = &items[0] {
            assert!(tr.content.contains("[pruned"));
            // Round-trip through str to prove the slice is valid UTF-8.
            let _: &str = tr.content.as_ref();
        } else {
            panic!("expected tool result");
        }
    }
}
