//! Memory system for cross-session knowledge persistence.
//!
//! This crate provides a markdown-based memory storage layer that allows
//! HeartQ to persist important information across sessions. Memory files are
//! stored under `~/.heartq/memory/` with workspace-scoped subdirectories
//! keyed by a blake3 hash of the workspace path.
//!
//! ## Data Layout
//!
//! ```text
//! ~/.heartq/memory/
//!   ├── MEMORY.md                         # Global curated knowledge
//!   └── {workspace_hash}/                 # Per-workspace (blake3(cwd)[..16])
//!       ├── MEMORY.md                     # Project-level curated knowledge
//!       └── sessions/
//!           └── YYYY-MM-DD-{slug}-{sid8}.md  # Session logs
//! ```
//!
//! ## Feature Flag
//!
//! Memory is gated behind `--experimental-memory` CLI flag or
//! `HEARTQ_MEMORY=1` environment variable. When disabled, this crate
//! is not initialized by the host.

pub mod archive;
pub mod auto_propose;
pub mod backend;
pub mod chunker;
pub mod dream;
pub mod dream_enhanced;
pub mod learning;
pub mod meta_skill;
pub mod dream_lock;
pub mod embedding;
pub mod error;
pub mod index;
pub mod manager;
pub mod mmr;
pub mod prefetch;
pub mod query_expansion;
pub mod references;
pub mod schema;
pub mod search;
pub mod storage;
pub mod sync;
pub mod text_utils;
pub mod utils;
pub mod watcher;

// Re-export commonly used types
pub use backend::{EndpointScopedCredentials, MemoryBackendImpl, MemoryBackendParams};
pub use dream_enhanced::{
    PromotionEvidenceEntry, PromotionEvidenceStore, QuarantineRules,
    apply_promotion_candidates, collect_candidates_from_sessions, filter_and_rank_candidates,
    rank_promotion_candidates, SignalCounts, SignalType, PromotionCandidate,
};
pub use index::{MemoryIndex, init_sqlite_vec};
pub use prefetch::{MemoryBlock, MemorySource, PrefetchResult, MemoryPrefetch};
pub use sync::{
    Insight, InsightSource, TurnSync, NoOpTurnSync, RuleBasedInsightExtractor, StorageTurnSync,
};
pub use manager::{MemoryManager, MemoryManagerConfig, MemoryStats};
// Legacy `MemoryStorage` and `MemoryScope` live in `storage` and are
// the canonical implementations that shell and other crates depend on.
// Re-export at the crate root so external callers can keep using
// `heartq_memory::MemoryStorage` / `heartq_memory::MemoryScope`.
// The new `sync` layer has its own private `MemoryScope` enum — the
// legacy 2-variant one (Global/Workspace) is the public-facing one.
pub use storage::{MemoryScope, MemoryStorage};
pub use utils::*;
pub use error::MemoryError;

/// Embed all chunks that don't have embeddings yet.
///
/// Queries the index for unembedded chunks, batches them through the
/// embedding provider, and upserts the results. Logs progress.
///
/// This is the async glue between the sync `MemoryIndex` and the async
/// `EmbeddingProvider`. Call after reindex, flush writes, or session-end writes.
pub async fn embed_missing_chunks(
    index: &MemoryIndex,
    provider: &dyn embedding::EmbeddingProvider,
) -> usize {
    let chunks = match index.chunks_without_embeddings() {
        Ok(c) if c.is_empty() => return 0,
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                target: heartq_telemetry::memory_log::TARGET,
                error = %e,
                "failed to query chunks without embeddings"
            );
            return 0;
        }
    };

    let total = chunks.len();
    let mut embedded = 0;

    // Batch in groups of 32 (provider's typical max batch size)
    for batch in chunks.chunks(32) {
        let texts: Vec<&str> = batch.iter().map(|(_, text)| text.as_str()).collect();
        match provider.embed_batch(&texts).await {
            Ok(embeddings) => {
                for ((chunk_id, _), embedding) in batch.iter().zip(embeddings.iter()) {
                    if let Err(e) = index.upsert_embedding(chunk_id, embedding) {
                        tracing::warn!(
                            target: heartq_telemetry::memory_log::TARGET,
                            chunk_id,
                            error = %e,
                            "failed to upsert embedding"
                        );
                    } else {
                        embedded += 1;
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    target: heartq_telemetry::memory_log::TARGET,
                    error = %e,
                    batch_size = texts.len(),
                    "embedding batch failed, skipping"
                );
            }
        }
    }

    if embedded > 0 {
        tracing::info!(
            target: heartq_telemetry::memory_log::TARGET,
            embedded,
            total,
            "embedded missing chunks"
        );
    }
    embedded
}
