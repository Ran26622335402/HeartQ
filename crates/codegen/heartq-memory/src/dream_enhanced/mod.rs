//! Dream enhancement module for heartq-memory.
//!
//! This module contains the dream consolidation enhancements
//! derived from OpenSquilla's dream functionality, including:
//! - Evidence tracking for promotion candidates
//! - Quarantine rules for filtering
//! - Ranking with signal classification

pub mod evidence;
pub mod promotion;
pub mod quarantine;
pub mod ranking;

pub use evidence::{
    PromotionEvidenceEntry, PromotionEvidenceStore, mark_promoted, mark_represented, mark_skipped,
};
pub use promotion::{
    apply_promotion_candidates, collect_candidates_from_sessions, filter_and_rank_candidates,
};
pub use quarantine::QuarantineRules;
pub use ranking::{
    rank_promotion_candidates, SignalCounts, SignalType, PromotionCandidate,
};
