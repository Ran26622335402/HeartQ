//! Memory prefetch trait and data structures for pre-conversation recall.
//!
//! This module provides the `MemoryPrefetch` trait that implementors can use
//! to provide memory context before each conversation turn. The trait is
//! designed to be implementable by the existing `MemoryBackendImpl` to enable
//! seamless integration with the memory system's search capabilities.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Source of memory block.
///
/// Indicates where a memory block originated from, which affects
/// relevance scoring and temporal decay behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemorySource {
    /// Global memory: `~/.heartq/memory/MEMORY.md`
    ///
    /// Contains curated, long-term knowledge that persists across all
    /// workspaces and sessions. These blocks are exempt from temporal decay.
    Global,
    /// Workspace memory: `~/.heartq/memory/{workspace}/MEMORY.md`
    ///
    /// Contains project-specific curated knowledge for a particular workspace.
    /// These blocks are exempt from temporal decay.
    Workspace,
    /// Session log files: `~/.heartq/memory/{workspace}/sessions/`
    ///
    /// Contains auto-generated session logs that capture conversation context.
    /// These blocks are subject to temporal decay based on their age.
    Session,
}

impl MemorySource {
    /// Convert from the string representation used in the storage/index layer.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "global" => Some(Self::Global),
            "workspace" => Some(Self::Workspace),
            "session" => Some(Self::Session),
            _ => None,
        }
    }

    /// Convert to the string representation used in the storage/index layer.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Workspace => "workspace",
            Self::Session => "session",
        }
    }
}

/// A single block of memory content.
///
/// Represents a discrete chunk of memory that can be injected into a
/// conversation context. Each block has metadata about its source,
/// relevance, and location in the original file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryBlock {
    /// The memory content (text of this chunk).
    pub content: String,

    /// Source of this memory block.
    pub source: MemorySource,

    /// Relevance score in the range [0.0, 1.0].
    ///
    /// Higher scores indicate greater relevance to the prefetch query.
    /// This value is typically computed by the search algorithm.
    pub score: f64,

    /// File path this block came from.
    ///
    /// This is the absolute path to the source file in the memory directory.
    pub path: String,

    /// Start line in the source file (1-indexed).
    pub start_line: usize,

    /// End line in the source file (1-indexed, inclusive).
    pub end_line: usize,

    /// Creation timestamp (Unix timestamp in seconds).
    ///
    /// This is typically derived from the file's modification time plus
    /// the chunk index to ensure uniqueness.
    pub timestamp: i64,
}

impl MemoryBlock {
    /// Estimate token count for this block.
    ///
    /// Uses a rough heuristic: characters / 4 + overhead. This is a
    /// quick approximation suitable for budget estimation. For exact
    /// counts, use a proper tokenizer.
    ///
    /// # Returns
    /// Estimated number of tokens in this block's content.
    pub fn estimate_tokens(&self) -> u64 {
        // Rough estimate: chars / 4 + overhead for structure/markers
        (self.content.len() as u64 / 4) + 20
    }
}

/// Result of a prefetch operation.
///
/// Contains the memory blocks that should be injected into the conversation
/// context, along with metadata about the prefetch operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefetchResult {
    /// Memory blocks to inject into context.
    ///
    /// Blocks are ordered by relevance (highest first) and may be
    /// truncated to fit within the token budget.
    pub blocks: Vec<MemoryBlock>,

    /// Total estimated tokens across all blocks.
    ///
    /// This is the sum of `estimate_tokens()` for each block.
    pub total_tokens: usize,

    /// Whether the result exceeded the requested token budget.
    ///
    /// When `true`, some content had to be excluded to fit within
    /// the `max_context_tokens` limit.
    pub exceeded: bool,
}

impl PrefetchResult {
    /// Create a new prefetch result.
    ///
    /// # Arguments
    /// * `blocks` - Memory blocks to include
    /// * `max_tokens` - Maximum token budget for the caller
    ///
    /// # Returns
    /// A `PrefetchResult` with computed token counts and exceeded flag.
    pub fn new(blocks: Vec<MemoryBlock>, max_tokens: usize) -> Self {
        let total_tokens = blocks.iter().map(|b| b.estimate_tokens() as usize).sum();
        Self {
            exceeded: total_tokens > max_tokens,
            total_tokens,
            blocks,
        }
    }

    /// Create an empty prefetch result.
    pub fn empty() -> Self {
        Self {
            blocks: Vec::new(),
            total_tokens: 0,
            exceeded: false,
        }
    }

    /// Returns true if there are no blocks in this result.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Returns the number of blocks in this result.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }
}

impl Default for PrefetchResult {
    fn default() -> Self {
        Self::empty()
    }
}

/// Error type for memory operations.
///
/// Errors can occur during backend operations, storage access,
/// search, or IO operations.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// Error from the memory backend layer.
    #[error("Backend error: {0}")]
    Backend(String),

    /// Error from storage operations.
    #[error("Storage error: {0}")]
    Storage(String),

    /// Error during search operations.
    #[error("Search error: {0}")]
    Search(String),

    /// IO error (file not found, permission denied, etc.).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Operation timed out.
    #[error("Timeout: {0}")]
    Timeout(String),
}

impl From<Box<dyn std::error::Error + Send + Sync>> for MemoryError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::Backend(err.to_string())
    }
}

impl From<rusqlite::Error> for MemoryError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Storage(err.to_string())
    }
}

/// Trait for memory prefetch functionality.
///
/// Implementors provide pre-conversation memory recall by searching the
/// memory index for relevant content before each turn. This enables
/// the agent to have context from previous relevant conversations.
///
/// The trait is designed to be implementable by `MemoryBackendImpl`
/// to provide seamless integration with the existing memory system.
#[async_trait]
pub trait MemoryPrefetch: Send + Sync {
    /// Get the name of this prefetch implementation.
    ///
    /// This is used for logging and telemetry purposes to identify
    /// which prefetch implementation is in use.
    fn name(&self) -> &str;

    /// Check if prefetch is available.
    ///
    /// Returns `true` if the prefetch implementation is ready to
    /// serve requests. Substrate implementations can return `false`
    /// if required resources (database, storage) are not available.
    ///
    /// The default implementation returns `true`.
    fn is_available(&self) -> bool {
        true
    }

    /// Prefetch relevant memory context for the upcoming turn.
    ///
    /// This method searches the memory index for content relevant to
    /// the user's query and returns blocks up to the token budget.
    ///
    /// # Arguments
    /// * `query` - The user's query or greeting text
    /// * `max_context_tokens` - Maximum tokens to return (budget limit)
    /// * `session_id` - Current session ID (optional, for session-specific context)
    ///
    /// # Returns
    /// Memory blocks relevant to the query, capped at token budget.
    /// Returns `Ok(PrefetchResult::empty())` if no relevant content found.
    async fn prefetch(
        &self,
        query: &str,
        max_context_tokens: usize,
        session_id: Option<&str>,
    ) -> Result<PrefetchResult, MemoryError>;

    /// Get all queued memory blocks from previous prefetch.
    ///
    /// Some implementations may queue blocks for later retrieval.
    /// This method returns all queued blocks and clears the queue.
    ///
    /// The default implementation returns an empty vector.
    fn get_memory_blocks(&self) -> Vec<MemoryBlock> {
        Vec::new()
    }

    /// Clear the prefetch queue.
    ///
    /// Removes any queued memory blocks from previous prefetch calls.
    /// The default implementation is a no-op.
    fn clear_queue(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_source_from_str() {
        assert_eq!(MemorySource::from_str("global"), Some(MemorySource::Global));
        assert_eq!(
            MemorySource::from_str("workspace"),
            Some(MemorySource::Workspace)
        );
        assert_eq!(MemorySource::from_str("session"), Some(MemorySource::Session));
        assert_eq!(MemorySource::from_str("unknown"), None);
    }

    #[test]
    fn test_memory_source_as_str() {
        assert_eq!(MemorySource::Global.as_str(), "global");
        assert_eq!(MemorySource::Workspace.as_str(), "workspace");
        assert_eq!(MemorySource::Session.as_str(), "session");
    }

    #[test]
    fn test_memory_block_estimate_tokens() {
        let block = MemoryBlock {
            content: "This is a test content with approximately 50 characters.".to_string(),
            source: MemorySource::Global,
            score: 0.8,
            path: "/test/path.md".to_string(),
            start_line: 1,
            end_line: 5,
            timestamp: 1234567890,
        };

        // 50 chars / 4 + 20 overhead = 32.5 -> 32
        let tokens = block.estimate_tokens();
        assert!(tokens >= 30 && tokens <= 35, "Expected ~32 tokens, got {tokens}");
    }

    #[test]
    fn test_prefetch_result_new() {
        let blocks = vec![
            MemoryBlock {
                content: "Short content".to_string(),
                source: MemorySource::Global,
                score: 0.9,
                path: "/test/a.md".to_string(),
                start_line: 1,
                end_line: 2,
                timestamp: 1000,
            },
            MemoryBlock {
                content: "Another block of content here".to_string(),
                source: MemorySource::Workspace,
                score: 0.7,
                path: "/test/b.md".to_string(),
                start_line: 1,
                end_line: 3,
                timestamp: 2000,
            },
        ];

        let result = PrefetchResult::new(blocks, 1000);

        assert!(!result.exceeded, "Small content should not exceed budget");
        assert_eq!(result.blocks.len(), 2);
        assert!(result.total_tokens > 0);
    }

    #[test]
    fn test_prefetch_result_exceeded() {
        let blocks = vec![
            MemoryBlock {
                content: "x".repeat(5000), // ~1250 tokens
                source: MemorySource::Global,
                score: 0.9,
                path: "/test/a.md".to_string(),
                start_line: 1,
                end_line: 10,
                timestamp: 1000,
            },
        ];

        let result = PrefetchResult::new(blocks, 100); // Small budget

        assert!(result.exceeded, "Large content should exceed small budget");
    }

    #[test]
    fn test_prefetch_result_empty() {
        let result = PrefetchResult::empty();
        assert!(result.is_empty());
        assert_eq!(result.len(), 0);
        assert!(!result.exceeded);
    }

    #[test]
    fn test_prefetch_result_default() {
        let result = PrefetchResult::default();
        assert!(result.is_empty());
    }

    #[test]
    fn test_memory_error_display() {
        let io_error = MemoryError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(io_error.to_string().contains("IO error"));

        let backend_error = MemoryError::Backend("connection failed".to_string());
        assert!(backend_error.to_string().contains("Backend error"));

        let search_error = MemoryError::Search("invalid query".to_string());
        assert!(search_error.to_string().contains("Search error"));

        let timeout_error = MemoryError::Timeout("operation timed out".to_string());
        assert!(timeout_error.to_string().contains("Timeout"));
    }

    #[test]
    fn test_memory_source_debug() {
        let source = MemorySource::Global;
        let debug_str = format!("{:?}", source);
        assert_eq!(debug_str, "Global");
    }

    #[test]
    fn test_memory_block_debug() {
        let block = MemoryBlock {
            content: "test".to_string(),
            source: MemorySource::Workspace,
            score: 0.5,
            path: "/path".to_string(),
            start_line: 1,
            end_line: 2,
            timestamp: 123,
        };
        let debug_str = format!("{:?}", block);
        assert!(debug_str.contains("MemoryBlock"));
    }

    #[test]
    fn test_prefetch_result_debug() {
        let result = PrefetchResult::empty();
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("PrefetchResult"));
    }
}
