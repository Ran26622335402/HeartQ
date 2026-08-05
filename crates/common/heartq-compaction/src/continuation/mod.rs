//! Compaction continuation decision module.
//!
//! This module provides post-compaction continuation decisions.
//! It determines whether to continue, retry, degrade, or block
//! after a compaction operation.
//!
//! # Usage
//!
//! ```rust
//! use heartq_compaction::continuation::{decide_compaction_continuation, ContinuationAction};
//!
//! let decision = decide_compaction_continuation(
//!     true,  // receipt_safe
//!     true,  // raw_session_durable
//!     true,  // semantic_flush_ok
//!     0,     // retry_count
//!     3,     // max_retries
//!     true,  // prompt_changed
//!     false, // finalization_attempted
//!     false, // context_unsalvageable
//! );
//! ```

pub mod decision;
pub mod decider;

pub use decision::{CompactionContinuationDecision, ContinuationAction};
pub use decider::decide_compaction_continuation;
