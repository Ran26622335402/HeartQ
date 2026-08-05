//! Continuation action and decision types.
//!
//! Defines the possible actions to take after compaction
//! and the decision struct.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Continuation action after compaction.
///
/// Derived from OpenSquilla's `CompactionContinuationAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationAction {
    /// Continue after successful compaction
    ContinueAfterCompaction,
    /// Retry compaction
    RetryAfterCompaction,
    /// Continue in degraded mode
    DegradedContinueAfterCompaction,
    /// Partial completion
    PartialAfterCompaction,
    /// Blocked due to safety
    BlockedAfterCompaction,
    /// Failed after retries
    FailedAfterCompaction,
}

impl std::fmt::Display for ContinuationAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContinuationAction::ContinueAfterCompaction => write!(f, "continue_after_compaction"),
            ContinuationAction::RetryAfterCompaction => write!(f, "retry_after_compaction"),
            ContinuationAction::DegradedContinueAfterCompaction => {
                write!(f, "degraded_continue_after_compaction")
            }
            ContinuationAction::PartialAfterCompaction => write!(f, "partial_after_compaction"),
            ContinuationAction::BlockedAfterCompaction => write!(f, "blocked_after_compaction"),
            ContinuationAction::FailedAfterCompaction => write!(f, "failed_after_compaction"),
        }
    }
}

/// A compaction continuation decision with reason and details.
///
/// Derived from OpenSquilla's `CompactionContinuationDecision`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionContinuationDecision {
    /// The action to take
    pub action: ContinuationAction,
    /// Human-readable reason for the decision
    pub reason: String,
    /// Additional details about the decision
    #[serde(default)]
    pub details: HashMap<String, serde_json::Value>,
}

impl CompactionContinuationDecision {
    /// Create a new decision.
    pub fn new(action: ContinuationAction, reason: impl Into<String>) -> Self {
        Self {
            action,
            reason: reason.into(),
            details: HashMap::new(),
        }
    }

    /// Create a new decision with details.
    pub fn with_details(
        action: ContinuationAction,
        reason: impl Into<String>,
        details: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            action,
            reason: reason.into(),
            details,
        }
    }

    /// Convert to a dictionary representation.
    pub fn to_dict(&self) -> HashMap<String, serde_json::Value> {
        let mut dict = HashMap::new();
        dict.insert("action".to_string(), serde_json::json!(self.action.to_string()));
        dict.insert("reason".to_string(), serde_json::json!(self.reason));
        if !self.details.is_empty() {
            dict.insert("details".to_string(), serde_json::json!(&self.details));
        }
        dict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        assert_eq!(
            format!("{}", ContinuationAction::ContinueAfterCompaction),
            "continue_after_compaction"
        );
        assert_eq!(
            format!("{}", ContinuationAction::BlockedAfterCompaction),
            "blocked_after_compaction"
        );
    }

    #[test]
    fn test_new_decision() {
        let decision = CompactionContinuationDecision::new(
            ContinuationAction::ContinueAfterCompaction,
            "receipt safe",
        );

        assert_eq!(decision.action, ContinuationAction::ContinueAfterCompaction);
        assert_eq!(decision.reason, "receipt safe");
        assert!(decision.details.is_empty());
    }

    #[test]
    fn test_to_dict() {
        let decision = CompactionContinuationDecision::new(
            ContinuationAction::RetryAfterCompaction,
            "prompt not reduced",
        );

        let dict = decision.to_dict();
        assert_eq!(dict["action"], "retry_after_compaction");
        assert_eq!(dict["reason"], "prompt not reduced");
    }
}
