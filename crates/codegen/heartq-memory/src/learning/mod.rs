//! Learning-graph submodule: skill/memory relationship graphs.
//!
//! Phase 4 of the Hermes integration plan.
//!
//! - [`graph`] — data structures and pure builder (no I/O).
//! - [`builder`] — `LearningGraphBuilder` that reads skills from
//!   `~/.heartq/skills/` and memory from the storage layer to feed the
//!   pure builder.

pub mod graph;
pub mod builder;

pub use graph::{
    Cluster, GraphEdge, GraphNode, GraphStats, LearningGraph, MemoryCard, SkillCardInputs,
    SkillNode,
};
pub use builder::{LearningGraphBuilder, LearningGraphInputs};