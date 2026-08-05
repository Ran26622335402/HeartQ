//! Promotion evidence store for Dream.
//!
//! Tracks evidence for dream candidate promotion decisions.
//! Derived from OpenSquilla's evidence module.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Evidence entry for a promotion candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionEvidenceEntry {
    /// Unique identifier for this candidate
    pub candidate_id: String,
    /// Agent ID
    pub agent_id: String,
    /// Source file path
    pub source_path: String,
    /// Source kind (e.g., "memory_file")
    pub source_kind: String,
    /// Source modification time in nanoseconds
    pub source_mtime_ns: i64,
    /// Source file size
    pub source_size: i64,
    /// The snippet text
    pub snippet: String,
    /// SHA256 of snippet
    pub snippet_sha256: String,
    /// SHA256 of normalized (lowercase) snippet
    pub claim_sha256: String,
    /// First seen timestamp (ISO format)
    pub first_seen_at: String,
    /// Last seen timestamp (ISO format)
    pub last_seen_at: String,
    /// Number of times seen
    pub seen_count: usize,
    /// Count of positive signals
    pub positive_signal_count: usize,
    /// Count of correction signals
    pub correction_signal_count: usize,
    /// Count of failure signals
    pub failure_signal_count: usize,
    /// Count of manual signals
    pub manual_signal_count: usize,
    /// Days when this was seen (YYYY-MM-DD format)
    pub source_days: Vec<String>,
    /// Status: "candidate", "promoted", or "represented"
    pub status: String,
    /// When this was promoted (ISO format)
    pub promoted_at: Option<String>,
    /// When this was rejected (ISO format)
    pub rejected_at: Option<String>,
    /// Reason for last skip
    pub last_skip_reason: Option<String>,
}

impl Default for PromotionEvidenceEntry {
    fn default() -> Self {
        Self {
            candidate_id: String::new(),
            agent_id: "main".to_string(),
            source_path: String::new(),
            source_kind: "memory_file".to_string(),
            source_mtime_ns: 0,
            source_size: 0,
            snippet: String::new(),
            snippet_sha256: String::new(),
            claim_sha256: String::new(),
            first_seen_at: String::new(),
            last_seen_at: String::new(),
            seen_count: 0,
            positive_signal_count: 0,
            correction_signal_count: 0,
            failure_signal_count: 0,
            manual_signal_count: 0,
            source_days: Vec::new(),
            status: "candidate".to_string(),
            promoted_at: None,
            rejected_at: None,
            last_skip_reason: None,
        }
    }
}

/// Evidence store containing all entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromotionEvidenceStore {
    /// Schema version
    pub version: u32,
    /// Last update timestamp (ISO format)
    pub updated_at: String,
    /// All evidence entries
    pub entries: HashMap<String, PromotionEvidenceEntry>,
}

impl PromotionEvidenceStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get an entry by candidate ID.
    pub fn get(&self, candidate_id: &str) -> Option<&PromotionEvidenceEntry> {
        self.entries.get(candidate_id)
    }

    /// Get a mutable entry by candidate ID.
    pub fn get_mut(&mut self, candidate_id: &str) -> Option<&mut PromotionEvidenceEntry> {
        self.entries.get_mut(candidate_id)
    }

    /// Insert or update an entry.
    pub fn upsert(&mut self, entry: PromotionEvidenceEntry) {
        self.entries.insert(entry.candidate_id.clone(), entry);
    }

    /// Remove an entry.
    pub fn remove(&mut self, candidate_id: &str) {
        self.entries.remove(candidate_id);
    }

    /// Get the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over entries.
    pub fn iter(&self) -> impl Iterator<Item = &PromotionEvidenceEntry> {
        self.entries.values()
    }

    /// Iterate mutably over entries.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut PromotionEvidenceEntry> {
        self.entries.values_mut()
    }
}

/// Mark entries as promoted.
pub fn mark_promoted(store: &mut PromotionEvidenceStore, candidate_ids: &[String], now_iso: &str) {
    for candidate_id in candidate_ids {
        if let Some(entry) = store.entries.get_mut(candidate_id) {
            entry.status = "promoted".to_string();
            entry.promoted_at = Some(now_iso.to_string());
            entry.last_skip_reason = None;
        }
    }
}

/// Mark entries as skipped.
pub fn mark_skipped(store: &mut PromotionEvidenceStore, candidate_id: &str, reason: &str) {
    if let Some(entry) = store.entries.get_mut(candidate_id) {
        entry.last_skip_reason = Some(reason.to_string());
    }
}

/// Mark entries as represented.
pub fn mark_represented(store: &mut PromotionEvidenceStore, candidate_ids: &[String], reason: &str) {
    for candidate_id in candidate_ids {
        if let Some(entry) = store.entries.get_mut(candidate_id) {
            entry.status = "represented".to_string();
            entry.last_skip_reason = Some(reason.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_new() {
        let store = PromotionEvidenceStore::new();
        assert!(store.is_empty());
    }

    #[test]
    fn test_store_insert() {
        let mut store = PromotionEvidenceStore::new();
        let entry = PromotionEvidenceEntry {
            candidate_id: "test123".to_string(),
            snippet: "Test snippet".to_string(),
            ..Default::default()
        };
        store.upsert(entry);

        assert_eq!(store.len(), 1);
        assert!(store.get("test123").is_some());
    }

    #[test]
    fn test_mark_promoted() {
        let mut store = PromotionEvidenceStore::new();
        let entry = PromotionEvidenceEntry {
            candidate_id: "test123".to_string(),
            status: "candidate".to_string(),
            ..Default::default()
        };
        store.upsert(entry);

        mark_promoted(&mut store, &["test123".to_string()], "2026-01-01T00:00:00Z");

        let entry = store.get("test123").unwrap();
        assert_eq!(entry.status, "promoted");
        assert!(entry.promoted_at.is_some());
    }
}
