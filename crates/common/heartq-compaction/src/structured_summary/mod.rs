//! Structured compaction summary module.
//!
//! This module provides a structured representation of compacted context,
//! with 15+ fields for rich task state representation. It is designed
//! to work with the obligations module for coverage tracking.
//!
//! # Usage
//!
//! ```rust
//! use heartq_compaction::structured_summary::{StructuredSummaryBuilder, render_structured_summary};
//! use heartq_compaction::obligations::{verify_summary_coverage, ObligationExtractor};
//!
//! let mut extractor = ObligationExtractor::new();
//! // extractor.extract(&entries) would be called with actual compaction entries
//!
//! // let obligations = extractor.extract(&entries);
//! // let (builder, coverage) = StructuredSummaryBuilder::from_obligations(
//! //     &summary_text,
//! //     &obligations,
//! // );
//! // let summary = builder.build();
//! // let rendered = render_structured_summary(&summary);
//! ```

pub mod builder;
pub mod render;

pub use builder::{StructuredCompactionSummary, StructuredSummaryBuilder, FileOrArtifact, ToolResultRef, DecisionOrRationale, KnownFailure};
pub use render::render_structured_summary;
