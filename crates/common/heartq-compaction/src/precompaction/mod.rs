//! Pre-compaction module for tool result pruning and preparation.
//!
//! This module implements Hermes-style tool result pruning before LLM summarization.
//! It handles deduplication, summarization, and truncation of tool results to
//! reduce token count before compression.

pub mod tool_pruner;

pub use tool_pruner::{
    CompactionItem, PruneStats, ToolPrunerConfig, ToolResultPruner,
};
