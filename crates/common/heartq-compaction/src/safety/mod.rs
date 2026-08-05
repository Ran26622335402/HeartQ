//! Safety mode system for compaction.
//!
//! This module provides safety modes and receipt-based authorization
//! for destructive compaction operations. It is derived from
//! OpenSquilla's compaction lifecycle module.
//!
//! # Safety Modes
//!
//! - `Protect` (default): Requires safe receipt for destructive compaction
//! - `BestEffort`: Allows degraded forensic mode
//! - `Block`: Strictly requires safe receipt
//! - `Off`: Compaction completely disabled
//!
//! # Usage
//!
//! ```rust
//! use heartq_compaction::safety::{SafetyMode, SafetyGuard, CompactionReceipt};
//!
//! let guard = SafetyGuard::new(SafetyMode::Protect);
//! let receipt = CompactionReceipt::new();
//! let decision = guard.check(Some(&receipt));
//! ```

pub mod guard;
pub mod mode;
pub mod receipt;

pub use guard::{CompactionDecision, SafetyGuard, SafetyDecision};
pub use mode::SafetyMode;
pub use receipt::CompactionReceipt;
