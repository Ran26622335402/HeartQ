//! Obligation extraction system for compaction.
//!
//! This module extracts high-signal continuity facts (obligations) from
//! conversation entries before they are removed during transcript compaction.
//! The extraction is based on regex patterns and heuristic rules derived from
//! OpenSquilla's compaction state module.
//!
//! # Obligations
//!
//! Obligations are small pieces of information that should survive compaction:
//! - Tool result IDs
//! - User goals and constraints
//! - Decisions and rationale
//! - File paths and artifacts
//! - Unresolved questions
//! - Failed commands
//! - Important identifiers (UUIDs, hashes)
//!
//! # Usage
//!
//! ```rust
//! use heartq_compaction::obligations::{ObligationExtractor, verify_summary_coverage};
//!
//! let mut extractor = ObligationExtractor::new();
//! // extractor.extract(&entries) would be called with actual compaction entries
//! // let obligations = extractor.extract(&entries);
//! // let coverage = verify_summary_coverage(&summary_text, &obligations, true, false);
//! ```

pub mod coverage;
pub mod extractor;
pub mod patterns;

pub use coverage::{CoverageResult, CoverageStatus, verify_summary_coverage, MAX_CRITICAL_CARRY_FORWARD};
pub use extractor::{
    CompactionEntry, CompactionObligation, ObligationExtractor, ObligationKind, SimpleEntry,
    ToolCallInfo,
};
pub use patterns::*;
