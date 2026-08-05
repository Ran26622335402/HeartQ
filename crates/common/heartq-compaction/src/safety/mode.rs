//! Safety mode enum and helpers.
//!
//! Defines the safety modes for compaction and provides
//! normalization/parsing utilities.

use serde::{Deserialize, Serialize};

/// Safety mode for compaction operations.
///
/// Derived from OpenSquilla's `FlushCompactionSafetyMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyMode {
    /// Default: Requires safe receipt for destructive compaction
    #[default]
    Protect,
    /// Allows degraded forensic mode
    BestEffort,
    /// Strictly requires safe receipt
    Block,
    /// Compaction completely disabled
    Off,
}

impl SafetyMode {
    /// Parse a safety mode from a string.
    ///
    /// Handles various string formats and normalizes them.
    pub fn from_str(s: &str) -> Self {
        let normalized = s.trim().to_lowercase().replace('-', "_");

        match normalized.as_str() {
            "" | "protect" | "protected" => SafetyMode::Protect,
            "best_effort" | "besteffort" | "legacy" => SafetyMode::BestEffort,
            "block" | "strict" | "require_safe_receipt" => SafetyMode::Block,
            "off" | "disabled" | "none" | "false" | "0" => SafetyMode::Off,
            _ => SafetyMode::Protect,
        }
    }

    /// Check if this mode allows destructive compaction given receipt safety.
    pub fn allows_destructive(&self, receipt_safe: bool) -> bool {
        match self {
            SafetyMode::Off => false,
            SafetyMode::Protect | SafetyMode::BestEffort | SafetyMode::Block => receipt_safe,
        }
    }

    /// Check if this mode requires a safe receipt.
    pub fn requires_safe_receipt(&self) -> bool {
        matches!(self, SafetyMode::Block)
    }

    /// Get the string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            SafetyMode::Protect => "protect",
            SafetyMode::BestEffort => "best_effort",
            SafetyMode::Block => "block",
            SafetyMode::Off => "off",
        }
    }
}

impl std::fmt::Display for SafetyMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let mode = SafetyMode::default();
        assert_eq!(mode, SafetyMode::Protect);
    }

    #[test]
    fn test_from_str() {
        assert_eq!(SafetyMode::from_str("protect"), SafetyMode::Protect);
        assert_eq!(SafetyMode::from_str("best-effort"), SafetyMode::BestEffort);
        assert_eq!(SafetyMode::from_str("block"), SafetyMode::Block);
        assert_eq!(SafetyMode::from_str("off"), SafetyMode::Off);
        assert_eq!(SafetyMode::from_str("disabled"), SafetyMode::Off);
        assert_eq!(SafetyMode::from_str("unknown"), SafetyMode::Protect);
    }

    #[test]
    fn test_allows_destructive() {
        assert!(!SafetyMode::Off.allows_destructive(true));
        assert!(!SafetyMode::Off.allows_destructive(false));

        assert!(SafetyMode::Protect.allows_destructive(true));
        assert!(!SafetyMode::Protect.allows_destructive(false));

        assert!(SafetyMode::BestEffort.allows_destructive(true));
        assert!(!SafetyMode::BestEffort.allows_destructive(false));

        assert!(SafetyMode::Block.allows_destructive(true));
        assert!(!SafetyMode::Block.allows_destructive(false));
    }

    #[test]
    fn test_requires_safe_receipt() {
        assert!(!SafetyMode::Protect.requires_safe_receipt());
        assert!(!SafetyMode::BestEffort.requires_safe_receipt());
        assert!(SafetyMode::Block.requires_safe_receipt());
        assert!(!SafetyMode::Off.requires_safe_receipt());
    }

    #[test]
    fn test_as_str() {
        assert_eq!(SafetyMode::Protect.as_str(), "protect");
        assert_eq!(SafetyMode::BestEffort.as_str(), "best_effort");
        assert_eq!(SafetyMode::Block.as_str(), "block");
        assert_eq!(SafetyMode::Off.as_str(), "off");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", SafetyMode::Protect), "protect");
    }
}
