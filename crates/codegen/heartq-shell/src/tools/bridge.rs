//! ToolBridge: re-exported from `heartq-tools`.
//!
//! The bridge implementation now lives in `heartq_tools::bridge`.
//! This module re-exports everything for backward compatibility.

pub use heartq_tools::bridge::{ToolBridge, ToolBridgeResult};
