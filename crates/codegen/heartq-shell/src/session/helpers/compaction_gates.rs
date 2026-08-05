//! Post-summary OpenSquilla + Hermes compaction gates for the full-replace path.
//!
//! Production compaction still uses `sample_full_replace_summary`; this module
//! runs obligation extraction, coverage verification, safety receipt checks,
//! anti-thrash bookkeeping, and continuation decisions around that summary.

use heartq_compaction::{
    decide_compaction_continuation, verify_summary_coverage, CompactionContinuationDecision,
    CompactionDecision, CompactionReceipt, CompressionState, ContinuationAction, CoverageStatus,
    ObligationExtractor, SafetyGuard, SafetyMode, SimpleEntry,
};
use heartq_sampling_types::ConversationItem;
use std::sync::Mutex;

/// Session-scoped anti-thrash state (Hermes CompressionState).
static COMPRESSION_STATE: Mutex<Option<CompressionState>> = Mutex::new(None);

fn compression_state() -> std::sync::MutexGuard<'static, Option<CompressionState>> {
    COMPRESSION_STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Return true when anti-thrash blocks another compaction attempt.
pub fn anti_thrash_blocked() -> bool {
    let mut guard = compression_state();
    let state = guard.get_or_insert_with(|| CompressionState::new(3));
    state.is_blocked()
}

pub fn record_compaction_success() {
    if let Some(state) = compression_state().as_mut() {
        state.record_successful();
    }
}

pub fn record_compaction_ineffective() {
    if let Some(state) = compression_state().as_mut() {
        state.record_ineffective();
    }
}

/// Outcome of post-summary gates.
#[derive(Debug, Clone)]
pub struct GateOutcome {
    /// Potentially augmented summary (coverage backfill appended).
    pub summary: String,
    pub decision: CompactionDecision,
    pub continuation: CompactionContinuationDecision,
    pub receipt_safe: bool,
    pub obligation_count: usize,
    pub coverage_status: String,
}

fn to_simple_entries(items: &[ConversationItem]) -> Vec<SimpleEntry> {
    items
        .iter()
        .filter_map(|item| {
            let text = item.text_content();
            if text.trim().is_empty() {
                return None;
            }
            let role = match item {
                ConversationItem::System(_) => "system",
                ConversationItem::User(_) => "user",
                ConversationItem::Assistant(_) => "assistant",
                ConversationItem::ToolResult(_) => "tool",
                ConversationItem::BackendToolCall(_) => "tool",
                ConversationItem::Reasoning(_) => "assistant",
            };
            Some(SimpleEntry {
                role: Some(role.to_string()),
                id: None,
                content: text,
                tool_calls: Vec::new(),
            })
        })
        .collect()
}

/// Apply OpenSquilla obligation/coverage/safety gates to a compaction summary.
///
/// Uses [`SafetyMode::BestEffort`] by default so HeartQ's full-replace path
/// can proceed in degraded forensic mode when the receipt is incomplete.
pub fn apply_post_summary_gates(
    conversation: &[ConversationItem],
    summary: &str,
    safety_mode: SafetyMode,
) -> GateOutcome {
    let entries = to_simple_entries(conversation);
    let mut extractor = ObligationExtractor::new();
    let obligations = extractor.extract(&entries);
    let obligation_count = obligations.len();

    let coverage = verify_summary_coverage(summary, &obligations, true, false);
    let mut final_summary = summary.to_string();
    if !coverage.critical_carry_forward.is_empty() {
        final_summary.push_str("\n\n## Carry-forward obligations\n");
        for label in &coverage.critical_carry_forward {
            final_summary.push_str("- ");
            final_summary.push_str(label);
            final_summary.push('\n');
        }
    }

    let coverage_ok = matches!(
        coverage.status,
        CoverageStatus::Pass | CoverageStatus::PassWithBackfill | CoverageStatus::Unknown
    );
    let coverage_status = coverage.status.to_string();
    let obligation_status = match coverage.status {
        CoverageStatus::Pass | CoverageStatus::Unknown => "ok",
        CoverageStatus::PassWithBackfill => "backfilled",
        CoverageStatus::FailReported | CoverageStatus::FailBlocked => "incomplete",
    };

    let mut builder = CompactionReceipt::builder()
        .mode("llm")
        .indexed_chunk_count(1)
        .integrity_status("ok")
        .output_coverage_status(if coverage_ok { "ok" } else { "incomplete" })
        .invalid_candidate_count(0)
        .obligation_count(obligation_count)
        .obligation_status(obligation_status);
    for id in &coverage.missing_obligations {
        builder = builder.add_missing_obligation(id.clone());
    }
    let receipt = builder.build();

    let receipt_safe = receipt.allows_destructive_compaction();
    let guard = SafetyGuard::new(safety_mode);
    let decision = guard.decide_compaction(Some(&receipt));

    let continuation = decide_compaction_continuation(
        receipt_safe || matches!(decision, CompactionDecision::DegradedForensic),
        true,
        true,
        0,
        2,
        true,
        false,
        matches!(decision, CompactionDecision::Disabled),
    );

    tracing::info!(
        target: "heartq_compaction_gates",
        obligation_count,
        coverage_status = %coverage_status,
        receipt_safe,
        decision = %decision,
        continuation = ?continuation.action,
        "COMPACTION_GATES: post-summary OpenSquilla/Hermes gates applied"
    );

    if matches!(
        continuation.action,
        ContinuationAction::BlockedAfterCompaction
    ) {
        record_compaction_ineffective();
    } else {
        record_compaction_success();
    }

    GateOutcome {
        summary: final_summary,
        decision,
        continuation,
        receipt_safe,
        obligation_count,
        coverage_status,
    }
}
