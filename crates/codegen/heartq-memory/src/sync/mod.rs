use serde::{Deserialize, Serialize};

/// Source of the insight
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InsightSource {
    UserMessage,
    AssistantResponse,
    ToolResult,
    SessionSummary,
}

/// A piece of knowledge worth preserving
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Insight {
    pub content: String,
    pub source: InsightSource,
    pub importance: f32,
    pub tags: Vec<String>,
}

impl Insight {
    pub fn new(content: String, source: InsightSource, importance: f32) -> Self {
        Self { content, source, importance, tags: Vec::new() }
    }
    
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }
}

/// Scope for memory storage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MemoryScope {
    Global,
    #[default]
    Workspace,
    Session,
}

impl MemoryScope {
    pub fn directory_name(&self) -> &str {
        match self {
            MemoryScope::Global => "MEMORY.md",
            MemoryScope::Workspace => "MEMORY.md",
            MemoryScope::Session => "sessions",
        }
    }
}

use async_trait::async_trait;
use crate::prefetch::MemoryError;
use crate::storage::{MemoryScope as StorageScope, MemoryStorage};

/// Trait for synchronizing conversation turns to memory
#[async_trait]
pub trait TurnSync: Send + Sync {
    fn name(&self) -> &str;
    fn is_available(&self) -> bool { true }
    
    async fn sync_turn(
        &self,
        user_content: &str,
        assistant_content: &str,
        scope: MemoryScope,
        session_id: Option<&str>,
    ) -> Result<(), MemoryError>;
    
    async fn extract_insights(
        &self,
        user_content: &str,
        assistant_content: &str,
    ) -> Result<Vec<Insight>, MemoryError>;
    
    async fn write_insight(&self, insight: &Insight, scope: MemoryScope) -> Result<(), MemoryError>;
    
    async fn flush(&self) -> Result<(), MemoryError> { Ok(()) }
}

/// Default implementation that does nothing
pub struct NoOpTurnSync;

#[async_trait]
impl TurnSync for NoOpTurnSync {
    fn name(&self) -> &str { "noop" }
    
    async fn sync_turn(&self, _: &str, _: &str, _: MemoryScope, _: Option<&str>) -> Result<(), MemoryError> {
        Ok(())
    }
    
    async fn extract_insights(&self, _: &str, _: &str) -> Result<Vec<Insight>, MemoryError> {
        Ok(Vec::new())
    }
    
    async fn write_insight(&self, _: &Insight, _: MemoryScope) -> Result<(), MemoryError> {
        Ok(())
    }
}

/// Storage-backed turn sync — writes session logs and curated insights.
///
/// Wired from `heartq-shell` spawn when memory is enabled so
/// `MemoryManager::sync_turn` is no longer a no-op.
pub struct StorageTurnSync {
    storage: MemoryStorage,
    extractor: RuleBasedInsightExtractor,
    session_id: String,
    /// Minimum insight importance to promote into workspace MEMORY.md.
    promote_min_importance: f32,
}

impl StorageTurnSync {
    pub fn new(storage: MemoryStorage, session_id: impl Into<String>) -> Self {
        Self {
            storage,
            extractor: RuleBasedInsightExtractor::new(0.5),
            session_id: session_id.into(),
            promote_min_importance: 0.7,
        }
    }

    pub fn with_promote_threshold(mut self, threshold: f32) -> Self {
        self.promote_min_importance = threshold;
        self
    }

    fn map_scope(scope: MemoryScope) -> StorageScope {
        match scope {
            MemoryScope::Global => StorageScope::Global,
            MemoryScope::Workspace | MemoryScope::Session => StorageScope::Workspace,
        }
    }

    fn slug_from_user(user_content: &str) -> String {
        let raw = user_content
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-')
            .take(48)
            .collect::<String>()
            .split_whitespace()
            .take(4)
            .collect::<Vec<_>>()
            .join("-")
            .to_lowercase();
        if raw.is_empty() {
            "turn".to_string()
        } else {
            raw
        }
    }

    fn format_turn(user_content: &str, assistant_content: &str) -> String {
        let user = user_content.trim();
        let assistant = assistant_content.trim();
        let mut out = String::new();
        out.push_str("### User\n\n");
        out.push_str(if user.is_empty() { "_(empty)_" } else { user });
        out.push_str("\n\n### Assistant\n\n");
        // Cap assistant body so session logs stay bounded.
        // Truncate on a UTF-8 char boundary — byte slicing mid-codepoint
        // (common with CJK) panics and aborts the session thread.
        const MAX_ASSISTANT: usize = 4000;
        if assistant.len() > MAX_ASSISTANT {
            let cut = assistant.floor_char_boundary(MAX_ASSISTANT);
            out.push_str(&assistant[..cut]);
            out.push_str("\n\n_(truncated)_");
        } else if assistant.is_empty() {
            out.push_str("_(empty)_");
        } else {
            out.push_str(assistant);
        }
        out
    }
}

#[async_trait]
impl TurnSync for StorageTurnSync {
    fn name(&self) -> &str {
        "storage"
    }

    async fn sync_turn(
        &self,
        user_content: &str,
        assistant_content: &str,
        scope: MemoryScope,
        session_id: Option<&str>,
    ) -> Result<(), MemoryError> {
        let sid = session_id.unwrap_or(self.session_id.as_str());
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let slug = Self::slug_from_user(user_content);
        let content = Self::format_turn(user_content, assistant_content);

        self.storage
            .write_daily_log(&date, &slug, sid, &content, true)
            .map_err(|e| MemoryError::Storage(e.to_string()))?;

        // Promote high-importance insights into curated MEMORY.md.
        let insights = self.extract_insights(user_content, assistant_content).await?;
        for insight in insights
            .into_iter()
            .filter(|i| i.importance >= self.promote_min_importance)
        {
            self.write_insight(&insight, scope).await?;
        }

        Ok(())
    }

    async fn extract_insights(
        &self,
        user_content: &str,
        assistant_content: &str,
    ) -> Result<Vec<Insight>, MemoryError> {
        let mut insights = self.extractor.extract_from_user(user_content);
        // Light pass over assistant for explicit "remember/note" style lines.
        for line in assistant_content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let importance = self.extractor.assess_importance(trimmed);
            if importance >= self.extractor.min_importance {
                insights.push(Insight::new(
                    trimmed.to_string(),
                    InsightSource::AssistantResponse,
                    importance,
                ));
            }
        }
        Ok(insights)
    }

    async fn write_insight(&self, insight: &Insight, scope: MemoryScope) -> Result<(), MemoryError> {
        let heading = match insight.source {
            InsightSource::UserMessage => "User insight",
            InsightSource::AssistantResponse => "Assistant insight",
            InsightSource::ToolResult => "Tool insight",
            InsightSource::SessionSummary => "Session summary",
        };
        let block = format!(
            "## {heading}\n\n{}\n\n<!-- importance: {:.2} tags: {} -->",
            insight.content.trim(),
            insight.importance,
            insight.tags.join(","),
        );
        match scope {
            MemoryScope::Session => {
                let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
                self.storage
                    .write_daily_log(&date, "insight", &self.session_id, &block, true)
                    .map_err(|e| MemoryError::Storage(e.to_string()))?;
            }
            MemoryScope::Global | MemoryScope::Workspace => {
                self.storage
                    .append_to_memory(Self::map_scope(scope), &block)
                    .map_err(|e| MemoryError::Storage(e.to_string()))?;
            }
        }
        Ok(())
    }
}

/// Simple rule-based insight extractor
pub struct RuleBasedInsightExtractor {
    pub min_importance: f32,
}

impl RuleBasedInsightExtractor {
    pub fn new(min_importance: f32) -> Self {
        Self { min_importance }
    }
    
    pub fn extract_from_user(&self, content: &str) -> Vec<Insight> {
        let mut insights = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            let importance = self.assess_importance(trimmed);
            if importance >= self.min_importance {
                insights.push(Insight::new(
                    trimmed.to_string(),
                    InsightSource::UserMessage,
                    importance,
                ));
            }
        }
        insights
    }
    
    pub fn assess_importance(&self, text: &str) -> f32 {
        let mut score = 0.3f32;
        if text.contains("remember") || text.contains("note") || text.contains("important") {
            score += 0.3;
        }
        if text.contains("always") || text.contains("never") || text.contains("must") {
            score += 0.2;
        }
        if text.len() > 50 && text.len() < 500 { score += 0.1; }
        if text.starts_with("- ") || text.starts_with("* ") { score += 0.1; }
        score.min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::StorageTurnSync;

    #[test]
    fn format_turn_truncates_on_utf8_char_boundary() {
        // Build an assistant reply where byte index 4000 falls inside a CJK
        // codepoint (same shape as the production panic on '添').
        let prefix = "a".repeat(3998);
        let assistant = format!("{prefix}添加更多内容以超过截断阈值");
        assert!(assistant.len() > 4000);
        assert!(!assistant.is_char_boundary(4000));

        let out = StorageTurnSync::format_turn("ping", &assistant);
        assert!(out.contains("_(truncated)_"));
        assert!(!out.contains('\u{FFFD}'));
        // Truncated body must be valid UTF-8 (already guaranteed by &str slice)
        // and must not include the full original suffix past the cut.
        assert!(out.len() < assistant.len() + 64);
    }

    #[test]
    fn format_turn_keeps_short_assistant() {
        let out = StorageTurnSync::format_turn("hi", "你好");
        assert!(out.contains("你好"));
        assert!(!out.contains("_(truncated)_"));
    }
}
