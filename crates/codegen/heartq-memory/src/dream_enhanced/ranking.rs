//! Promotion ranking for Dream.
//!
//! Provides deterministic ranking of dream candidates for promotion.
//! Derived from OpenSquilla's ranking module.

use super::evidence::PromotionEvidenceEntry;
use super::evidence::PromotionEvidenceStore;

/// Signal type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalType {
    /// Positive signal (prefers, accepted, successful, works, use)
    Positive,
    /// Correction signal (do not, don't, rejected, wrong, instead)
    Correction,
    /// Failure signal (failed, error, exception, traceback, rollback)
    Failure,
    /// Manual signal (memory:, remember that)
    Manual,
    /// Neutral (default)
    Neutral,
}

impl SignalType {
    /// Classify text to determine signal type.
    pub fn from_text(text: &str) -> Self {
        let lower = text.to_lowercase();

        if lower.contains("memory:") || lower.contains("remember that") {
            SignalType::Manual
        } else if lower.contains("do not")
            || lower.contains("don't")
            || lower.contains("rejected")
            || lower.contains("wrong")
            || lower.contains("instead")
        {
            SignalType::Correction
        } else if lower.contains("failed")
            || lower.contains("error")
            || lower.contains("exception")
            || lower.contains("traceback")
            || lower.contains("rollback")
        {
            SignalType::Failure
        } else if lower.contains("prefers")
            || lower.contains("accepted")
            || lower.contains("successful")
            || lower.contains("works")
            || lower.contains("use ")
        {
            SignalType::Positive
        } else {
            SignalType::Neutral
        }
    }
}

/// Signal counts for a candidate.
#[derive(Debug, Clone, Default)]
pub struct SignalCounts {
    pub positive: usize,
    pub correction: usize,
    pub failure: usize,
    pub manual: usize,
}

impl SignalCounts {
    /// Create from evidence entry.
    pub fn from_entry(entry: &PromotionEvidenceEntry) -> Self {
        Self {
            positive: entry.positive_signal_count,
            correction: entry.correction_signal_count,
            failure: entry.failure_signal_count,
            manual: entry.manual_signal_count,
        }
    }

    /// Total signals.
    pub fn total(&self) -> usize {
        self.positive + self.correction + self.failure + self.manual
    }
}

/// A promotion candidate with ranking score.
#[derive(Debug, Clone)]
pub struct PromotionCandidate {
    /// Candidate ID
    pub candidate_id: String,
    /// Source path
    pub source_path: String,
    /// The snippet text
    pub snippet: String,
    /// SHA256 of snippet
    pub snippet_sha256: String,
    /// SHA256 of normalized snippet
    pub claim_sha256: String,
    /// Promotion score (0.0 to 1.0)
    pub score: f64,
    /// Reasons for the score
    pub reasons: Vec<String>,
    /// Signal counts
    pub signal_counts: SignalCounts,
}

impl PromotionCandidate {
    /// Get the score.
    pub fn score(&self) -> f64 {
        self.score
    }
}

/// Calculate promotion score for an entry.
///
/// Score formula:
/// - frequency: log1p(seen_count) / log1p(6) * 0.35
/// - signal_balance: base 0.55 + adjustments * 0.30
/// - source_confidence: 0.75 (memory_file) or 0.50 * 0.20
/// - consolidation: min(len(source_days) / 3, 1.0) * 0.15
fn calculate_score(entry: &PromotionEvidenceEntry) -> f64 {
    // Frequency component (35% weight)
    let frequency: f64 = {
        let seen = entry.seen_count.max(0) as f64;
        let log_seen = (seen + 1.0).ln();
        let log_6 = 6.0_f64.ln();
        (log_seen / log_6).clamp(0.0, 1.0)
    };

    // Signal balance component (30% weight)
    let signal_balance: f64 = {
        let positive_or_manual =
            entry.positive_signal_count + entry.manual_signal_count;
        let negative = entry.correction_signal_count + entry.failure_signal_count;

        let mut balance: f64 = 0.55;
        if positive_or_manual > 0 {
            balance += 0.3;
        }
        if entry.manual_signal_count > 0 {
            balance += 0.1;
        }
        if negative > 0 && positive_or_manual == 0 {
            balance -= 0.25;
            if negative > 1 {
                balance += 0.25;
            }
        }
        balance.clamp(0.0, 1.0)
    };

    // Source confidence (20% weight)
    let source_confidence: f64 = if entry.source_kind == "memory_file" {
        0.75
    } else {
        0.50
    };

    // Consolidation (15% weight)
    let consolidation: f64 = {
        let days = entry.source_days.len() as f64;
        (days / 3.0).clamp(0.0, 1.0)
    };

    // Combined score
    let score = 0.35 * frequency + 0.30 * signal_balance + 0.20 * source_confidence + 0.15 * consolidation;
    score.clamp(0.0, 1.0)
}

/// Check if entry is pure negative (corrections/failures but no positive/manual).
fn is_pure_negative(entry: &PromotionEvidenceEntry) -> bool {
    let negative = entry.correction_signal_count + entry.failure_signal_count;
    let positive = entry.positive_signal_count + entry.manual_signal_count;
    negative > 0 && positive == 0
}

/// Rank promotion candidates from an evidence store.
///
/// # Arguments
///
/// * `store` - The evidence store
/// * `min_score` - Minimum score threshold (0.0 to 1.0)
/// * `negative_recurrence_threshold` - How many times a negative must recur before promotion
/// * `min_seen_count` - Minimum times seen to be eligible
/// * `limit` - Maximum number of candidates to return
pub fn rank_promotion_candidates(
    store: &PromotionEvidenceStore,
    min_score: f64,
    negative_recurrence_threshold: usize,
    min_seen_count: usize,
    limit: Option<usize>,
) -> Vec<PromotionCandidate> {
    let mut candidates: Vec<PromotionCandidate> = Vec::new();

    for entry in store.iter() {
        // Skip non-candidates
        if entry.status != "candidate" || entry.snippet.trim().is_empty() {
            continue;
        }

        // Skip if below minimum seen count
        if entry.seen_count < min_seen_count {
            continue;
        }

        // Collect reasons
        let mut reasons: Vec<String> = Vec::new();
        let signal_counts = SignalCounts::from_entry(entry);

        if entry.positive_signal_count + entry.manual_signal_count > 0 {
            reasons.push("positive_or_manual_signal".to_string());
        }

        // Handle pure negative
        if is_pure_negative(entry) {
            if entry.seen_count < negative_recurrence_threshold {
                continue; // Skip negative entries below threshold
            }
            reasons.push("negative_recurrence".to_string());
        }

        if entry.seen_count > 1 {
            reasons.push(format!("seen_count={}", entry.seen_count));
        }

        // Calculate score
        let score = calculate_score(entry);

        // Skip if below minimum score
        if score < min_score {
            continue;
        }

        candidates.push(PromotionCandidate {
            candidate_id: entry.candidate_id.clone(),
            source_path: entry.source_path.clone(),
            snippet: entry.snippet.clone(),
            snippet_sha256: entry.snippet_sha256.clone(),
            claim_sha256: entry.claim_sha256.clone(),
            score,
            reasons,
            signal_counts,
        });
    }

    // Sort by score (descending), then total signals (descending), then ID
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.signal_counts
                    .total()
                    .cmp(&a.signal_counts.total())
            })
            .then_with(|| a.candidate_id.cmp(&b.candidate_id))
    });

    // Apply limit
    if let Some(limit) = limit {
        candidates.truncate(limit);
    }

    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dream_enhanced::evidence::PromotionEvidenceEntry;

    fn make_entry(
        seen_count: usize,
        positive: usize,
        correction: usize,
        failure: usize,
        manual: usize,
    ) -> PromotionEvidenceEntry {
        PromotionEvidenceEntry {
            seen_count,
            positive_signal_count: positive,
            correction_signal_count: correction,
            failure_signal_count: failure,
            manual_signal_count: manual,
            source_kind: "memory_file".to_string(),
            source_days: vec!["2026-01-01".to_string()],
            status: "candidate".to_string(),
            snippet: "Test snippet".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_signal_type_classification() {
        assert!(matches!(SignalType::from_text("Remember that..."), SignalType::Manual));
        assert!(matches!(SignalType::from_text("memory:"), SignalType::Manual));
        assert!(matches!(SignalType::from_text("Do not do that"), SignalType::Correction));
        assert!(matches!(SignalType::from_text("failed to compile"), SignalType::Failure));
        assert!(matches!(SignalType::from_text("This works well"), SignalType::Positive));
        assert!(matches!(SignalType::from_text("Just some text"), SignalType::Neutral));
    }

    #[test]
    fn test_calculate_score() {
        let entry = make_entry(1, 0, 0, 0, 0);
        let score = calculate_score(&entry);
        assert!(score >= 0.0 && score <= 1.0);
    }

    #[test]
    fn test_is_pure_negative() {
        let entry1 = make_entry(3, 0, 2, 0, 0);
        assert!(is_pure_negative(&entry1));

        let entry2 = make_entry(3, 1, 2, 0, 0);
        assert!(!is_pure_negative(&entry2));
    }

    #[test]
    fn test_ranking_empty_store() {
        let store = PromotionEvidenceStore::new();
        let ranked = rank_promotion_candidates(&store, 0.0, 3, 1, None);
        assert!(ranked.is_empty());
    }

    #[test]
    fn test_ranking_with_candidates() {
        let mut store = PromotionEvidenceStore::new();

        // Entry with higher score
        let entry1 = {
            let mut e = make_entry(5, 2, 0, 0, 1);
            e.candidate_id = "entry1".to_string();
            e.snippet = "This is good".to_string();
            e
        };

        // Entry with lower score
        let entry2 = {
            let mut e = make_entry(1, 0, 0, 0, 0);
            e.candidate_id = "entry2".to_string();
            e.snippet = "Just text".to_string();
            e
        };

        store.upsert(entry1);
        store.upsert(entry2);

        let ranked = rank_promotion_candidates(&store, 0.0, 3, 1, None);
        assert_eq!(ranked.len(), 2);
        // Entry1 should rank higher due to more signals
        assert_eq!(ranked[0].candidate_id, "entry1");
    }

    #[test]
    fn test_ranking_limit() {
        let mut store = PromotionEvidenceStore::new();

        for i in 0..10 {
            let mut entry = make_entry(3, 1, 0, 0, 0);
            entry.candidate_id = format!("entry{}", i);
            store.upsert(entry);
        }

        let ranked = rank_promotion_candidates(&store, 0.0, 3, 1, Some(5));
        assert_eq!(ranked.len(), 5);
    }

    #[test]
    fn test_ranking_skips_below_min_score() {
        let mut store = PromotionEvidenceStore::new();

        // Entry with very low score (single appearance, no signals, no consolidation)
        let mut entry = make_entry(1, 0, 0, 0, 0);
        entry.candidate_id = "low_score".to_string();
        entry.source_days = vec![]; // No source days reduces consolidation score
        store.upsert(entry);

        // Use a high threshold that this entry won't meet
        let ranked = rank_promotion_candidates(&store, 0.8, 3, 1, None);
        assert!(ranked.is_empty());
    }
}
