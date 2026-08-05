//! MemoryManager - coordinates all memory operations with prefetch and turn sync.
//!
//! This module provides the `MemoryManager` struct that wraps the existing
//! `MemoryBackend` and adds prefetch/sync capabilities for enhanced memory usage.

use std::sync::Arc;
use tokio::sync::RwLock;

use heartq_tools::types::memory_backend::MemoryBackend;

use crate::prefetch::{MemoryBlock, MemoryError, MemoryPrefetch, MemorySource};
use crate::sync::{Insight, MemoryScope, TurnSync};

/// Configuration for MemoryManager
#[derive(Debug, Clone)]
pub struct MemoryManagerConfig {
    /// Maximum tokens for prefetch results
    pub max_prefetch_tokens: usize,
    /// Minimum score for prefetch results
    pub min_prefetch_score: f64,
    /// Timeout for prefetch operations
    pub prefetch_timeout_secs: f64,
    /// Whether sync is enabled
    pub sync_enabled: bool,
    /// Default memory scope
    pub default_scope: MemoryScope,
}

impl Default for MemoryManagerConfig {
    fn default() -> Self {
        Self {
            max_prefetch_tokens: 4000,
            min_prefetch_score: 0.35,
            prefetch_timeout_secs: 8.0,
            sync_enabled: true,
            default_scope: MemoryScope::Workspace,
        }
    }
}

/// MemoryManager coordinates all memory operations.
/// It wraps the existing MemoryBackend and adds prefetch/sync capabilities.
pub struct MemoryManager {
    /// The underlying memory backend
    backend: Arc<dyn MemoryBackend>,
    /// Optional prefetch implementation
    prefetch: Option<Arc<dyn MemoryPrefetch>>,
    /// Optional turn sync implementation  
    turn_sync: Option<Arc<dyn TurnSync>>,
    /// Configuration
    config: MemoryManagerConfig,
    /// Cached memory blocks from last prefetch
    cached_blocks: RwLock<Vec<MemoryBlock>>,
    /// Statistics
    stats: RwLock<MemoryStats>,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    pub prefetch_count: u64,
    pub sync_count: u64,
    pub total_tokens_injected: u64,
    pub last_prefetch_at: Option<i64>,
    pub last_sync_at: Option<i64>,
}

impl MemoryManager {
    /// Create a new MemoryManager
    pub fn new(
        backend: Arc<dyn MemoryBackend>,
        config: MemoryManagerConfig,
    ) -> Self {
        Self {
            backend,
            prefetch: None,
            turn_sync: None,
            config,
            cached_blocks: RwLock::new(Vec::new()),
            stats: RwLock::new(MemoryStats::default()),
        }
    }
    
    /// Set the prefetch implementation
    pub fn with_prefetch(mut self, prefetch: Arc<dyn MemoryPrefetch>) -> Self {
        self.prefetch = Some(prefetch);
        self
    }
    
    /// Set the turn sync implementation
    pub fn with_turn_sync(mut self, turn_sync: Arc<dyn TurnSync>) -> Self {
        self.turn_sync = Some(turn_sync);
        self
    }
    
    // ==================== Prefetch Operations ====================
    
    /// Build prefetch context for a query
    /// 
    /// This queries the memory backend and returns formatted memory blocks
    /// to inject into the system prompt.
    pub async fn build_prefetch(&self, query: &str, session_id: Option<&str>) -> Result<String, MemoryError> {
        // Use prefetch implementation if available
        if let Some(prefetch) = &self.prefetch {
            let result = prefetch
                .prefetch(query, self.config.max_prefetch_tokens, session_id)
                .await?;
            
            // Cache blocks for later retrieval
            {
                let mut cached = self.cached_blocks.write().await;
                *cached = result.blocks.clone();
            }
            
            // Update stats
            {
                let mut stats = self.stats.write().await;
                stats.prefetch_count += 1;
                stats.total_tokens_injected += result.total_tokens as u64;
                stats.last_prefetch_at = Some(chrono::Utc::now().timestamp());
            }
            
            return self.format_memory_context(&result.blocks);
        }
        
        // Fallback: search directly with the backend
        let results = self.backend
            .search(query, 6, self.config.min_prefetch_score)
            .await
            .map_err(|e| MemoryError::Search(e.to_string()))?;
        
        let blocks: Vec<MemoryBlock> = results
            .into_iter()
            .map(|r| {
                let source = MemorySource::from_str(&r.source).unwrap_or(MemorySource::Workspace);
                MemoryBlock {
                    content: r.snippet,
                    source,
                    score: r.score,
                    path: r.path,
                    start_line: r.start_line,
                    end_line: r.end_line,
                    timestamp: r.created_at.unwrap_or_else(|| chrono::Utc::now().timestamp()),
                }
            })
            .collect();
        
        {
            let mut cached = self.cached_blocks.write().await;
            *cached = blocks.clone();
        }
        
        {
            let mut stats = self.stats.write().await;
            stats.prefetch_count += 1;
            stats.last_prefetch_at = Some(chrono::Utc::now().timestamp());
        }
        
        self.format_memory_context(&blocks)
    }
    
    /// Get cached memory blocks from last prefetch
    pub async fn get_cached_blocks(&self) -> Vec<MemoryBlock> {
        self.cached_blocks.read().await.clone()
    }
    
    /// Queue a prefetch for the next turn (background)
    pub fn queue_prefetch(&self, query: &str, session_id: Option<&str>) {
        if let Some(prefetch) = &self.prefetch {
            let query = query.to_string();
            let session_id = session_id.map(|s| s.to_string());
            let prefetch = Arc::clone(prefetch);
            
            tokio::spawn(async move {
                let _ = prefetch.prefetch(&query, 4000, session_id.as_deref()).await;
            });
        }
    }
    
    // ==================== Turn Sync Operations ====================
    
    /// Synchronize a conversation turn to memory
    pub async fn sync_turn(
        &self,
        user_content: &str,
        assistant_content: &str,
    ) -> Result<(), MemoryError> {
        if !self.config.sync_enabled {
            return Ok(());
        }
        
        if let Some(sync) = &self.turn_sync {
            sync.sync_turn(
                user_content,
                assistant_content,
                self.config.default_scope,
                None,
            ).await
            .map_err(|e| MemoryError::Storage(e.to_string()))?;
            
            let mut stats = self.stats.write().await;
            stats.sync_count += 1;
            stats.last_sync_at = Some(chrono::Utc::now().timestamp());
        }
        
        Ok(())
    }
    
    /// Extract and write insights from a turn
    pub async fn extract_and_sync_insights(
        &self,
        user_content: &str,
        assistant_content: &str,
    ) -> Result<Vec<Insight>, MemoryError> {
        if !self.config.sync_enabled {
            return Ok(Vec::new());
        }
        
        if let Some(sync) = &self.turn_sync {
            let insights = sync.extract_insights(user_content, assistant_content)
                .await
                .map_err(|e| MemoryError::Storage(e.to_string()))?;
            
            for insight in &insights {
                sync.write_insight(insight, self.config.default_scope)
                    .await
                    .map_err(|e| MemoryError::Storage(e.to_string()))?;
            }
            
            return Ok(insights);
        }
        
        Ok(Vec::new())
    }
    
    // ==================== Utility Methods ====================
    
    /// Format memory blocks into a memory context string
    fn format_memory_context(&self, blocks: &[MemoryBlock]) -> Result<String, MemoryError> {
        if blocks.is_empty() {
            return Ok(String::new());
        }
        
        let mut output = String::new();
        output.push_str("<memory-context>\n");
        output.push_str("## Relevant Memory from Past Sessions\n\n");
        
        for (i, block) in blocks.iter().enumerate() {
            let source_label = block.source.as_str();
            
            output.push_str(&format!(
                "### Result {} (score: {:.2}, source: {})\n",
                i + 1,
                block.score,
                source_label
            ));
            output.push_str(&format!("**File:** {} (lines {}-{})\n\n", 
                block.path, block.start_line, block.end_line));
            output.push_str("```\n");
            output.push_str(&block.content);
            output.push_str("\n```\n\n");
        }
        
        output.push_str("</memory-context>");
        Ok(output)
    }
    
    /// Get current statistics
    pub async fn get_stats(&self) -> MemoryStats {
        self.stats.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = MemoryManagerConfig::default();
        assert_eq!(config.max_prefetch_tokens, 4000);
        assert_eq!(config.min_prefetch_score, 0.35);
        assert!(config.sync_enabled);
    }

    #[test]
    fn test_stats_default() {
        let stats = MemoryStats::default();
        assert_eq!(stats.prefetch_count, 0);
        assert_eq!(stats.sync_count, 0);
        assert!(stats.last_prefetch_at.is_none());
    }
}
