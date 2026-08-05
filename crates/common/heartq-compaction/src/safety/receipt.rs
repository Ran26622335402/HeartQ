//! Compaction receipt struct and validation.
//!
//! A receipt is produced by the flush pipeline and contains
//! metadata about what was indexed and archived.

use serde::{Deserialize, Serialize};

/// Compaction receipt from the flush pipeline.
///
/// Derived from OpenSquilla's `CompactionReceipt` and related validation
/// functions like `flush_receipt_allows_destructive_compaction()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionReceipt {
    /// Mode of the flush (must be "llm" for safe)
    pub mode: String,
    /// Number of chunks indexed
    pub indexed_chunk_count: usize,
    /// Integrity status ("ok" or other)
    pub integrity_status: String,
    /// Coverage status of the output ("ok" or other)
    pub output_coverage_status: String,
    /// Number of invalid candidates
    pub invalid_candidate_count: usize,
    /// IDs of missing candidates
    pub candidate_missing_ids: Vec<String>,
    /// Number of obligations
    pub obligation_count: usize,
    /// Obligation status ("ok", "backfilled", or other)
    pub obligation_status: String,
    /// IDs of missing obligations
    pub obligation_missing_ids: Vec<String>,
}

impl CompactionReceipt {
    /// Create a new receipt with default values.
    pub fn new() -> Self {
        Self {
            mode: String::new(),
            indexed_chunk_count: 0,
            integrity_status: String::new(),
            output_coverage_status: String::new(),
            invalid_candidate_count: 0,
            candidate_missing_ids: Vec::new(),
            obligation_count: 0,
            obligation_status: String::new(),
            obligation_missing_ids: Vec::new(),
        }
    }

    /// Create a builder for a receipt.
    pub fn builder() -> CompactionReceiptBuilder {
        CompactionReceiptBuilder::new()
    }

    /// Check if this receipt allows destructive compaction.
    ///
    /// This implements the same logic as OpenSquilla's
    /// `flush_receipt_allows_destructive_compaction()` function.
    pub fn allows_destructive_compaction(&self) -> bool {
        // 1. mode must be "llm"
        if self.mode != "llm" {
            return false;
        }

        // 2. indexed_chunk_count > 0
        if self.indexed_chunk_count == 0 {
            return false;
        }

        // 3. integrity_status == "ok"
        if self.integrity_status != "ok" {
            return false;
        }

        // 4. output_coverage_status == "ok"
        if self.output_coverage_status != "ok" {
            return false;
        }

        // 5. invalid_candidate_count == 0
        if self.invalid_candidate_count > 0 {
            return false;
        }

        // 6. candidate_missing_ids must be empty
        if !self.candidate_missing_ids.is_empty() {
            return false;
        }

        // 7. If obligation_count > 0, obligation_status must be "ok" or "backfilled"
        if self.obligation_count > 0
            && self.obligation_status != "ok"
            && self.obligation_status != "backfilled"
        {
            return false;
        }

        // 8. obligation_missing_ids must be empty
        if !self.obligation_missing_ids.is_empty() {
            return false;
        }

        true
    }
}

impl Default for CompactionReceipt {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for CompactionReceipt.
#[derive(Default)]
pub struct CompactionReceiptBuilder {
    receipt: CompactionReceipt,
}

impl CompactionReceiptBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the mode.
    pub fn mode(mut self, mode: impl Into<String>) -> Self {
        self.receipt.mode = mode.into();
        self
    }

    /// Set the indexed chunk count.
    pub fn indexed_chunk_count(mut self, count: usize) -> Self {
        self.receipt.indexed_chunk_count = count;
        self
    }

    /// Set the integrity status.
    pub fn integrity_status(mut self, status: impl Into<String>) -> Self {
        self.receipt.integrity_status = status.into();
        self
    }

    /// Set the output coverage status.
    pub fn output_coverage_status(mut self, status: impl Into<String>) -> Self {
        self.receipt.output_coverage_status = status.into();
        self
    }

    /// Set the invalid candidate count.
    pub fn invalid_candidate_count(mut self, count: usize) -> Self {
        self.receipt.invalid_candidate_count = count;
        self
    }

    /// Add a missing candidate ID.
    pub fn add_missing_candidate(mut self, id: impl Into<String>) -> Self {
        self.receipt.candidate_missing_ids.push(id.into());
        self
    }

    /// Set the obligation count.
    pub fn obligation_count(mut self, count: usize) -> Self {
        self.receipt.obligation_count = count;
        self
    }

    /// Set the obligation status.
    pub fn obligation_status(mut self, status: impl Into<String>) -> Self {
        self.receipt.obligation_status = status.into();
        self
    }

    /// Add a missing obligation ID.
    pub fn add_missing_obligation(mut self, id: impl Into<String>) -> Self {
        self.receipt.obligation_missing_ids.push(id.into());
        self
    }

    /// Build the receipt.
    pub fn build(self) -> CompactionReceipt {
        self.receipt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_receipt() {
        let receipt = CompactionReceipt::default();
        assert!(!receipt.allows_destructive_compaction());
    }

    #[test]
    fn test_safe_receipt() {
        let receipt = CompactionReceipt::builder()
            .mode("llm")
            .indexed_chunk_count(5)
            .integrity_status("ok")
            .output_coverage_status("ok")
            .obligation_count(3)
            .obligation_status("ok")
            .build();

        assert!(receipt.allows_destructive_compaction());
    }

    #[test]
    fn test_receipt_with_backfill() {
        let receipt = CompactionReceipt::builder()
            .mode("llm")
            .indexed_chunk_count(5)
            .integrity_status("ok")
            .output_coverage_status("ok")
            .obligation_count(3)
            .obligation_status("backfilled")
            .build();

        assert!(receipt.allows_destructive_compaction());
    }

    #[test]
    fn test_receipt_wrong_mode() {
        let receipt = CompactionReceipt::builder()
            .mode("system")
            .indexed_chunk_count(5)
            .integrity_status("ok")
            .output_coverage_status("ok")
            .build();

        assert!(!receipt.allows_destructive_compaction());
    }

    #[test]
    fn test_receipt_zero_chunks() {
        let receipt = CompactionReceipt::builder()
            .mode("llm")
            .indexed_chunk_count(0)
            .integrity_status("ok")
            .output_coverage_status("ok")
            .build();

        assert!(!receipt.allows_destructive_compaction());
    }

    #[test]
    fn test_receipt_bad_integrity() {
        let receipt = CompactionReceipt::builder()
            .mode("llm")
            .indexed_chunk_count(5)
            .integrity_status("error")
            .output_coverage_status("ok")
            .build();

        assert!(!receipt.allows_destructive_compaction());
    }

    #[test]
    fn test_receipt_missing_candidates() {
        let receipt = CompactionReceipt::builder()
            .mode("llm")
            .indexed_chunk_count(5)
            .integrity_status("ok")
            .output_coverage_status("ok")
            .add_missing_candidate("id123")
            .build();

        assert!(!receipt.allows_destructive_compaction());
    }

    #[test]
    fn test_receipt_bad_obligation_status() {
        let receipt = CompactionReceipt::builder()
            .mode("llm")
            .indexed_chunk_count(5)
            .integrity_status("ok")
            .output_coverage_status("ok")
            .obligation_count(3)
            .obligation_status("fail")
            .build();

        assert!(!receipt.allows_destructive_compaction());
    }

    #[test]
    fn test_builder_chaining() {
        let receipt = CompactionReceipt::builder()
            .mode("llm")
            .indexed_chunk_count(10)
            .integrity_status("ok")
            .output_coverage_status("ok")
            .invalid_candidate_count(0)
            .obligation_count(5)
            .obligation_status("ok")
            .build();

        assert_eq!(receipt.mode, "llm");
        assert_eq!(receipt.indexed_chunk_count, 10);
        assert!(receipt.allows_destructive_compaction());
    }
}
