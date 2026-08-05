//! Coverage verification for obligation extraction.
//!
//! This module provides coverage verification to ensure that extracted
//! obligations are actually present in the compacted summary text.
//! If obligations are missing, they can be carried forward to the next
//! context window.

use serde::{Deserialize, Serialize};

// Re-export for convenience
pub use super::patterns::MAX_CRITICAL_CARRY_FORWARD;

use super::extractor::{CompactionObligation, ObligationKind};

/// Coverage status for obligations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    /// No obligations to check
    Unknown,
    /// All obligations covered
    Pass,
    /// All obligations covered with backfill
    PassWithBackfill,
    /// Missing obligations reported (not blocked)
    FailReported,
    /// Missing critical obligations (blocked)
    FailBlocked,
}

impl std::fmt::Display for CoverageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoverageStatus::Unknown => write!(f, "unknown"),
            CoverageStatus::Pass => write!(f, "pass"),
            CoverageStatus::PassWithBackfill => write!(f, "pass_with_backfill"),
            CoverageStatus::FailReported => write!(f, "fail_reported"),
            CoverageStatus::FailBlocked => write!(f, "fail_blocked"),
        }
    }
}

/// Result of coverage verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageResult {
    /// Overall coverage status
    pub status: CoverageStatus,
    /// Total obligations checked
    pub checked_obligations: usize,
    /// Obligations that were found in summary
    pub covered_obligations: usize,
    /// Obligations that were not found
    pub missing_obligations: Vec<String>,
    /// Critical obligations to carry forward
    pub critical_carry_forward: Vec<String>,
    /// Whether coverage check blocked compaction
    #[serde(default)]
    pub blocked: bool,
}

impl CoverageResult {
    /// Create a new coverage result
    pub fn new(
        status: CoverageStatus,
        checked: usize,
        covered: usize,
        missing: Vec<String>,
        critical_carry_forward: Vec<String>,
        blocked: bool,
    ) -> Self {
        Self {
            status,
            checked_obligations: checked,
            covered_obligations: covered,
            missing_obligations: missing,
            critical_carry_forward,
            blocked,
        }
    }

    /// Create a coverage result for empty obligations
    pub fn unknown() -> Self {
        Self {
            status: CoverageStatus::Unknown,
            checked_obligations: 0,
            covered_obligations: 0,
            missing_obligations: Vec::new(),
            critical_carry_forward: Vec::new(),
            blocked: false,
        }
    }

    /// Check if this result indicates success
    pub fn is_pass(&self) -> bool {
        matches!(
            self.status,
            CoverageStatus::Pass | CoverageStatus::PassWithBackfill
        )
    }

    /// Get the number of missing obligations
    pub fn missing_count(&self) -> usize {
        self.missing_obligations.len()
    }
}

/// Verify that obligations are covered in the summary text.
///
/// This function checks if each obligation's value appears in the summary text.
/// If `backfill_missing` is true, missing obligations are added to the carry-forward list.
/// If `block_missing_critical` is true and any critical obligation is missing, the result is blocked.
///
/// # Arguments
///
/// * `summary_text` - The compacted summary text
/// * `obligations` - The obligations to verify
/// * `backfill_missing` - Whether to add missing obligations to carry-forward list
/// * `block_missing_critical` - Whether to block on missing critical obligations
///
/// # Returns
///
/// A `CoverageResult` with the verification status
pub fn verify_summary_coverage(
    summary_text: &str,
    obligations: &[CompactionObligation],
    backfill_missing: bool,
    block_missing_critical: bool,
) -> CoverageResult {
    let search_text = summary_text.to_lowercase();

    // Find missing obligations
    let missing: Vec<String> = obligations
        .iter()
        .filter(|obligation| !search_text.contains(&obligation.value.to_lowercase()))
        .map(|obligation| obligation.label())
        .collect();

    // Check if any missing obligations are critical
    let has_critical_missing = obligations.iter().any(|obligation| {
        obligation.critical
            && !search_text.contains(&obligation.value.to_lowercase())
    });

    let blocked = block_missing_critical && has_critical_missing;

    // Determine status
    let status = if obligations.is_empty() {
        CoverageStatus::Unknown
    } else if blocked {
        CoverageStatus::FailBlocked
    } else if missing.is_empty() {
        CoverageStatus::Pass
    } else if backfill_missing {
        CoverageStatus::PassWithBackfill
    } else {
        CoverageStatus::FailReported
    };

    // Build carry-forward list
    let critical_carry_forward = if backfill_missing || blocked {
        missing
            .iter()
            .take(MAX_CRITICAL_CARRY_FORWARD)
            .cloned()
            .collect()
    } else {
        Vec::new()
    };

    CoverageResult {
        status,
        checked_obligations: obligations.len(),
        covered_obligations: obligations.len() - missing.len(),
        missing_obligations: missing,
        critical_carry_forward,
        blocked,
    }
}

/// Build a coverage map for structured summaries.
///
/// Returns a map with coverage statistics that can be serialized
/// into the structured summary's `source_coverage` field.
pub fn build_coverage_map(result: &CoverageResult) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    map.insert("status".to_string(), serde_json::json!(result.status.to_string()));
    map.insert(
        "checked_obligations".to_string(),
        serde_json::json!(result.checked_obligations),
    );
    map.insert(
        "covered_obligations".to_string(),
        serde_json::json!(result.covered_obligations),
    );
    if !result.missing_obligations.is_empty() {
        map.insert(
            "missing_count".to_string(),
            serde_json::json!(result.missing_obligations.len()),
        );
    }
    if !result.critical_carry_forward.is_empty() {
        map.insert(
            "critical_carry_forward_count".to_string(),
            serde_json::json!(result.critical_carry_forward.len()),
        );
    }
    map
}

/// Group obligations by kind for structured summary construction.
pub fn group_obligations_by_kind(
    obligations: &[CompactionObligation],
) -> std::collections::HashMap<ObligationKind, Vec<String>> {
    let mut groups: std::collections::HashMap<ObligationKind, Vec<String>> =
        std::collections::HashMap::new();

    for obligation in obligations {
        groups
            .entry(obligation.kind)
            .or_default()
            .push(obligation.value.clone());
    }

    groups
}

/// Get the first obligation value for a given kind.
pub fn first_by_kind(
    obligations: &[CompactionObligation],
    kind: ObligationKind,
) -> Option<String> {
    obligations
        .iter()
        .find(|o| o.kind == kind)
        .map(|o| o.value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obligations::extractor::ObligationExtractor;
    use crate::obligations::extractor::SimpleEntry;

    fn create_obligation(kind: ObligationKind, value: &str) -> CompactionObligation {
        CompactionObligation::new(kind, value.to_string(), None, None)
    }

    #[test]
    fn test_verify_coverage_all_present() {
        let summary = "Goal: Build a compiler. Decision: Use Rust.";
        let obligations = vec![
            create_obligation(ObligationKind::UserGoal, "Build a compiler"),
            create_obligation(ObligationKind::Decision, "Use Rust"),
        ];

        let result = verify_summary_coverage(summary, &obligations, true, false);

        assert!(result.is_pass());
        assert_eq!(result.checked_obligations, 2);
        assert_eq!(result.covered_obligations, 2);
        assert!(result.missing_obligations.is_empty());
    }

    #[test]
    fn test_verify_coverage_some_missing() {
        let summary = "Goal: Build a compiler.";
        let obligations = vec![
            create_obligation(ObligationKind::UserGoal, "Build a compiler"),
            create_obligation(ObligationKind::Decision, "Use Rust"),
        ];

        let result = verify_summary_coverage(summary, &obligations, true, false);

        assert_eq!(result.status, CoverageStatus::PassWithBackfill);
        assert_eq!(result.missing_count(), 1);
        assert!(!result.critical_carry_forward.is_empty());
    }

    #[test]
    fn test_verify_coverage_blocked() {
        let summary = "Goal: Build a compiler.";
        let mut obligations = vec![
            create_obligation(ObligationKind::UserGoal, "Build a compiler"),
            create_obligation(ObligationKind::Decision, "Use Rust"),
        ];
        obligations[1].critical = true;

        let result = verify_summary_coverage(summary, &obligations, false, true);

        assert_eq!(result.status, CoverageStatus::FailBlocked);
        assert!(result.blocked);
    }

    #[test]
    fn test_verify_coverage_empty_obligations() {
        let summary = "Some summary text";
        let obligations: Vec<CompactionObligation> = vec![];

        let result = verify_summary_coverage(summary, &obligations, true, false);

        assert_eq!(result.status, CoverageStatus::Unknown);
        assert_eq!(result.checked_obligations, 0);
    }

    #[test]
    fn test_verify_coverage_no_backfill() {
        let summary = "Goal: Build a compiler.";
        let obligations = vec![
            create_obligation(ObligationKind::UserGoal, "Build a compiler"),
            create_obligation(ObligationKind::Decision, "Use Rust"),
        ];

        let result = verify_summary_coverage(summary, &obligations, false, false);

        assert_eq!(result.status, CoverageStatus::FailReported);
        assert!(result.critical_carry_forward.is_empty());
    }

    #[test]
    fn test_coverage_result_unknown() {
        let result = CoverageResult::unknown();
        assert_eq!(result.status, CoverageStatus::Unknown);
        assert!(!result.is_pass());
    }

    #[test]
    fn test_build_coverage_map() {
        let result = CoverageResult::new(
            CoverageStatus::Pass,
            5,
            4,
            vec!["missing1".to_string()],
            vec!["carry1".to_string()],
            false,
        );

        let map = build_coverage_map(&result);

        assert_eq!(map.get("status").unwrap(), "pass");
        assert_eq!(map.get("checked_obligations").unwrap(), 5);
        assert_eq!(map.get("covered_obligations").unwrap(), 4);
    }

    #[test]
    fn test_group_obligations_by_kind() {
        let obligations = vec![
            create_obligation(ObligationKind::UserGoal, "Goal 1"),
            create_obligation(ObligationKind::UserGoal, "Goal 2"),
            create_obligation(ObligationKind::Decision, "Decision 1"),
        ];

        let groups = group_obligations_by_kind(&obligations);

        assert_eq!(groups.get(&ObligationKind::UserGoal).unwrap().len(), 2);
        assert_eq!(groups.get(&ObligationKind::Decision).unwrap().len(), 1);
    }

    #[test]
    fn test_first_by_kind() {
        let obligations = vec![
            create_obligation(ObligationKind::UserGoal, "First Goal"),
            create_obligation(ObligationKind::UserGoal, "Second Goal"),
        ];

        let first = first_by_kind(&obligations, ObligationKind::UserGoal);
        assert_eq!(first, Some("First Goal".to_string()));

        let none = first_by_kind(&obligations, ObligationKind::Decision);
        assert_eq!(none, None);
    }

    #[test]
    fn test_case_insensitive_search() {
        let summary = "GOAL: BUILD A COMPILER";
        let obligations = vec![create_obligation(
            ObligationKind::UserGoal,
            "Build a compiler",
        )];

        let result = verify_summary_coverage(summary, &obligations, true, false);

        assert!(result.is_pass());
    }

    #[test]
    fn test_integration_with_extractor() {
        let mut extractor = ObligationExtractor::new();
        let entries = vec![
            SimpleEntry::user("Goal: Build a Rust web service"),
            SimpleEntry::assistant("Decision: Using Actix-web framework"),
        ];
        let obligations = extractor.extract(&entries);

        // Create a summary that covers some but not all
        let summary = "Goal: Build a Rust web service";
        let result = verify_summary_coverage(summary, &obligations, true, false);

        assert_eq!(result.checked_obligations, obligations.len());
        assert!(result.covered_obligations >= 1);
    }
}
