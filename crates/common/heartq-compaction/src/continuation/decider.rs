//! Compaction continuation decider.
//!
//! Implements the decision logic for post-compaction continuation.

use std::collections::HashMap;

use super::decision::{CompactionContinuationDecision, ContinuationAction};

/// Decide what action to take after compaction.
///
/// This implements the same decision matrix as OpenSquilla's
/// `decide_compaction_continuation()` function.
///
/// # Decision Matrix
///
/// | Condition | Action |
/// |-----------|--------|
/// | `context_unsalvageable` OR `!receipt_safe` | BlockedAfterCompaction |
/// | `prompt_changed` AND `semantic_flush_ok` | ContinueAfterCompaction |
/// | `retry_count < max_retries` | RetryAfterCompaction |
/// | `raw_session_durable` AND `!semantic_flush_ok` | DegradedContinueAfterCompaction |
/// | `finalization_attempted` | FailedAfterCompaction |
/// | otherwise | PartialAfterCompaction |
pub fn decide_compaction_continuation(
    receipt_safe: bool,
    raw_session_durable: bool,
    semantic_flush_ok: bool,
    retry_count: usize,
    max_retries: usize,
    prompt_changed: bool,
    finalization_attempted: bool,
    context_unsalvageable: bool,
) -> CompactionContinuationDecision {
    let mut details = HashMap::new();
    details.insert("receipt_safe".to_string(), serde_json::json!(receipt_safe));
    details.insert(
        "raw_session_durable".to_string(),
        serde_json::json!(raw_session_durable),
    );
    details.insert("semantic_flush_ok".to_string(), serde_json::json!(semantic_flush_ok));
    details.insert("retry_count".to_string(), serde_json::json!(retry_count));
    details.insert("max_retries".to_string(), serde_json::json!(max_retries));
    details.insert("prompt_changed".to_string(), serde_json::json!(prompt_changed));
    details.insert(
        "finalization_attempted".to_string(),
        serde_json::json!(finalization_attempted),
    );
    details.insert(
        "context_unsalvageable".to_string(),
        serde_json::json!(context_unsalvageable),
    );

    // 1. Blocked if context unsalvageable or receipt not safe
    if context_unsalvageable || !receipt_safe {
        return CompactionContinuationDecision::with_details(
            ContinuationAction::BlockedAfterCompaction,
            "context_unsalvageable",
            details,
        );
    }

    // 2. Continue if prompt changed and semantic flush OK
    if prompt_changed && semantic_flush_ok {
        return CompactionContinuationDecision::with_details(
            ContinuationAction::ContinueAfterCompaction,
            "receipt_safe_prompt_changed",
            details,
        );
    }

    // 3. Retry if we haven't exceeded max retries
    if retry_count < max_retries {
        return CompactionContinuationDecision::with_details(
            ContinuationAction::RetryAfterCompaction,
            "prompt_not_reduced",
            details,
        );
    }

    // 4. Degraded continue if raw session durable but semantic not OK
    if raw_session_durable && !semantic_flush_ok {
        return CompactionContinuationDecision::with_details(
            ContinuationAction::DegradedContinueAfterCompaction,
            "semantic_flush_degraded_raw_durable",
            details,
        );
    }

    // 5. Failed if finalization was attempted
    if finalization_attempted {
        return CompactionContinuationDecision::with_details(
            ContinuationAction::FailedAfterCompaction,
            "finalization_failed_after_retries",
            details,
        );
    }

    // 6. Default: partial completion
    CompactionContinuationDecision::with_details(
        ContinuationAction::PartialAfterCompaction,
        "finalization_required_after_retries",
        details,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::ContinuationAction;

    fn make_decision(
        receipt_safe: bool,
        raw_session_durable: bool,
        semantic_flush_ok: bool,
        retry_count: usize,
        max_retries: usize,
        prompt_changed: bool,
        finalization_attempted: bool,
        context_unsalvageable: bool,
    ) -> CompactionContinuationDecision {
        decide_compaction_continuation(
            receipt_safe,
            raw_session_durable,
            semantic_flush_ok,
            retry_count,
            max_retries,
            prompt_changed,
            finalization_attempted,
            context_unsalvageable,
        )
    }

    #[test]
    fn test_blocked_unsalvageable() {
        let decision = make_decision(true, true, true, 0, 3, false, false, true);
        assert_eq!(decision.action, ContinuationAction::BlockedAfterCompaction);
        assert!(decision.reason.contains("unsalvageable"));
    }

    #[test]
    fn test_blocked_receipt_not_safe() {
        let decision = make_decision(false, true, true, 0, 3, false, false, false);
        assert_eq!(decision.action, ContinuationAction::BlockedAfterCompaction);
    }

    #[test]
    fn test_continue() {
        let decision = make_decision(true, true, true, 0, 3, true, false, false);
        assert_eq!(decision.action, ContinuationAction::ContinueAfterCompaction);
    }

    #[test]
    fn test_retry() {
        let decision = make_decision(true, true, true, 1, 3, false, false, false);
        assert_eq!(decision.action, ContinuationAction::RetryAfterCompaction);
    }

    #[test]
    fn test_retry_exceeded() {
        let decision = make_decision(true, true, true, 3, 3, false, false, false);
        // retry_count (3) >= max_retries (3), so won't retry
        assert_ne!(decision.action, ContinuationAction::RetryAfterCompaction);
    }

    #[test]
    fn test_degraded_continue() {
        let decision = make_decision(true, true, false, 3, 3, false, false, false);
        assert_eq!(
            decision.action,
            ContinuationAction::DegradedContinueAfterCompaction
        );
    }

    #[test]
    fn test_failed() {
        // finalization_attempted with non-durable raw session and no semantic flush
        // Note: DegradedContinue takes priority over Failed in the decision matrix
        let decision = make_decision(true, false, false, 3, 3, false, true, false);
        assert_eq!(decision.action, ContinuationAction::FailedAfterCompaction);
    }

    #[test]
    fn test_partial() {
        let decision = make_decision(true, false, false, 3, 3, false, false, false);
        assert_eq!(decision.action, ContinuationAction::PartialAfterCompaction);
    }

    #[test]
    fn test_decision_details() {
        let decision = make_decision(true, true, true, 2, 5, false, false, false);
        assert_eq!(decision.details["retry_count"], 2);
        assert_eq!(decision.details["max_retries"], 5);
    }

    #[test]
    fn test_priority_blocked_over_continue() {
        // Even with prompt_changed, blocked takes priority
        let decision = make_decision(false, true, true, 0, 3, true, false, false);
        assert_eq!(decision.action, ContinuationAction::BlockedAfterCompaction);
    }

    #[test]
    fn test_priority_continue_over_retry() {
        // Even with retry available, continue takes priority when conditions met
        let decision = make_decision(true, true, true, 1, 3, true, false, false);
        assert_eq!(decision.action, ContinuationAction::ContinueAfterCompaction);
    }
}
