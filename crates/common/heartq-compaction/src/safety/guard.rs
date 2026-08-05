//! Safety guard for compaction decisions.
//!
//! Provides the SafetyGuard struct which makes compaction decisions
//! based on safety mode and receipt status.

use serde::{Deserialize, Serialize};

use super::mode::SafetyMode;
use super::receipt::CompactionReceipt;

/// Decision about whether compaction is allowed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyDecision {
    /// Whether destructive compaction is allowed.
    pub allows_destructive: bool,
    /// Human-readable reason for the decision.
    pub reason: String,
    /// The safety mode that was applied.
    pub mode: SafetyMode,
}

/// Compaction decision types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionDecision {
    /// Safe destructive compaction allowed.
    SafeDestructive,
    /// Degraded forensic mode (archive only, no destruction).
    DegradedForensic,
    /// Emergency ephemeral (block compaction but don't fail).
    EmergencyEphemeral,
    /// Compaction disabled.
    Disabled,
}

impl std::fmt::Display for CompactionDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompactionDecision::SafeDestructive => write!(f, "safe_destructive"),
            CompactionDecision::DegradedForensic => write!(f, "degraded_forensic"),
            CompactionDecision::EmergencyEphemeral => write!(f, "emergency_ephemeral"),
            CompactionDecision::Disabled => write!(f, "disabled"),
        }
    }
}

/// Safety guard for making compaction decisions.
pub struct SafetyGuard {
    mode: SafetyMode,
}

impl SafetyGuard {
    /// Create a new safety guard with the given mode.
    pub fn new(mode: SafetyMode) -> Self {
        Self { mode }
    }

    /// Check if compaction should proceed.
    ///
    /// Returns a `SafetyDecision` with the result.
    pub fn check(&self, receipt: Option<&CompactionReceipt>) -> SafetyDecision {
        let receipt_safe = receipt
            .map(|r| r.allows_destructive_compaction())
            .unwrap_or(false);

        match self.mode {
            SafetyMode::Off => SafetyDecision {
                allows_destructive: false,
                reason: "compaction disabled".to_string(),
                mode: self.mode,
            },
            SafetyMode::Protect => {
                if receipt_safe {
                    SafetyDecision {
                        allows_destructive: true,
                        reason: "safe receipt verified".to_string(),
                        mode: self.mode,
                    }
                } else {
                    SafetyDecision {
                        allows_destructive: false,
                        reason: "receipt not safe, using degraded forensic".to_string(),
                        mode: self.mode,
                    }
                }
            }
            SafetyMode::BestEffort => {
                if receipt_safe {
                    SafetyDecision {
                        allows_destructive: true,
                        reason: "safe receipt verified".to_string(),
                        mode: self.mode,
                    }
                } else {
                    SafetyDecision {
                        allows_destructive: false,
                        reason: "receipt not safe, using degraded forensic".to_string(),
                        mode: self.mode,
                    }
                }
            }
            SafetyMode::Block => {
                if receipt_safe {
                    SafetyDecision {
                        allows_destructive: true,
                        reason: "safe receipt verified".to_string(),
                        mode: self.mode,
                    }
                } else {
                    SafetyDecision {
                        allows_destructive: false,
                        reason: "strict mode requires safe receipt".to_string(),
                        mode: self.mode,
                    }
                }
            }
        }
    }

    /// Make a compaction decision.
    ///
    /// This returns a `CompactionDecision` which is more granular
    /// than `SafetyDecision`.
    pub fn decide_compaction(&self, receipt: Option<&CompactionReceipt>) -> CompactionDecision {
        let receipt_safe = receipt
            .map(|r| r.allows_destructive_compaction())
            .unwrap_or(false);

        match self.mode {
            SafetyMode::Off => CompactionDecision::Disabled,
            SafetyMode::Protect | SafetyMode::BestEffort => {
                if receipt_safe {
                    CompactionDecision::SafeDestructive
                } else {
                    CompactionDecision::DegradedForensic
                }
            }
            SafetyMode::Block => {
                if receipt_safe {
                    CompactionDecision::SafeDestructive
                } else {
                    CompactionDecision::EmergencyEphemeral
                }
            }
        }
    }

    /// Get the safety mode.
    pub fn mode(&self) -> SafetyMode {
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::safety::receipt::CompactionReceipt;

    fn safe_receipt() -> CompactionReceipt {
        CompactionReceipt::builder()
            .mode("llm")
            .indexed_chunk_count(5)
            .integrity_status("ok")
            .output_coverage_status("ok")
            .obligation_count(3)
            .obligation_status("ok")
            .build()
    }

    fn unsafe_receipt() -> CompactionReceipt {
        CompactionReceipt::builder()
            .mode("system")
            .indexed_chunk_count(0)
            .build()
    }

    #[test]
    fn test_off_mode() {
        let guard = SafetyGuard::new(SafetyMode::Off);
        let decision = guard.check(Some(&safe_receipt()));
        assert!(!decision.allows_destructive);
        assert!(decision.reason.contains("disabled"));

        let compaction = guard.decide_compaction(Some(&safe_receipt()));
        assert_eq!(compaction, CompactionDecision::Disabled);
    }

    #[test]
    fn test_protect_mode_with_safe_receipt() {
        let guard = SafetyGuard::new(SafetyMode::Protect);
        let decision = guard.check(Some(&safe_receipt()));
        assert!(decision.allows_destructive);
        assert!(decision.reason.contains("safe receipt"));
    }

    #[test]
    fn test_protect_mode_with_unsafe_receipt() {
        let guard = SafetyGuard::new(SafetyMode::Protect);
        let decision = guard.check(Some(&unsafe_receipt()));
        assert!(!decision.allows_destructive);
        assert!(decision.reason.contains("degraded forensic"));
    }

    #[test]
    fn test_best_effort_mode_with_safe_receipt() {
        let guard = SafetyGuard::new(SafetyMode::BestEffort);
        let decision = guard.check(Some(&safe_receipt()));
        assert!(decision.allows_destructive);
    }

    #[test]
    fn test_best_effort_mode_with_unsafe_receipt() {
        let guard = SafetyGuard::new(SafetyMode::BestEffort);
        let decision = guard.check(Some(&unsafe_receipt()));
        assert!(!decision.allows_destructive);
    }

    #[test]
    fn test_block_mode_with_safe_receipt() {
        let guard = SafetyGuard::new(SafetyMode::Block);
        let decision = guard.check(Some(&safe_receipt()));
        assert!(decision.allows_destructive);
    }

    #[test]
    fn test_block_mode_with_unsafe_receipt() {
        let guard = SafetyGuard::new(SafetyMode::Block);
        let decision = guard.check(Some(&unsafe_receipt()));
        assert!(!decision.allows_destructive);
        assert!(decision.reason.contains("strict mode"));
    }

    #[test]
    fn test_no_receipt() {
        let guard = SafetyGuard::new(SafetyMode::Protect);
        let decision = guard.check(None);
        assert!(!decision.allows_destructive);

        let guard = SafetyGuard::new(SafetyMode::Off);
        let decision = guard.check(None);
        assert!(!decision.allows_destructive);
    }

    #[test]
    fn test_decide_compaction() {
        let guard = SafetyGuard::new(SafetyMode::Protect);

        let decision = guard.decide_compaction(Some(&safe_receipt()));
        assert_eq!(decision, CompactionDecision::SafeDestructive);

        let decision = guard.decide_compaction(Some(&unsafe_receipt()));
        assert_eq!(decision, CompactionDecision::DegradedForensic);

        let guard = SafetyGuard::new(SafetyMode::Block);
        let decision = guard.decide_compaction(Some(&unsafe_receipt()));
        assert_eq!(decision, CompactionDecision::EmergencyEphemeral);
    }

    #[test]
    fn test_mode_getter() {
        let guard = SafetyGuard::new(SafetyMode::BestEffort);
        assert_eq!(guard.mode(), SafetyMode::BestEffort);
    }
}
