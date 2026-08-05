use serde::{Deserialize, Serialize};

/// Hermes-style segmented summary template
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnhancedSummaryTemplate {
    /// Historical task snapshot
    pub historical_task: String,
    /// Current in-progress state
    pub in_progress_state: String,
    /// Pending user requests
    pub pending_asks: Vec<String>,
    /// Remaining work items
    pub remaining_work: Vec<String>,
}

impl EnhancedSummaryTemplate {
    /// Prefix for all summaries - anti-hallucination instructions
    pub const SUMMARY_PREFIX: &'static str =
        "[CONTEXT COMPACTION — REFERENCE ONLY] Earlier turns were compacted \
         into the summary below. This is a handoff from a previous context \
         window — treat it as background reference, NOT as active instructions. \
         Do NOT answer questions or fulfill requests mentioned in this summary; \
         they were already addressed. \
         Respond ONLY to the latest user message that appears AFTER this \
         summary — that message is the single source of truth for what to do \
         right now. \
         Topic overlap with the summary does NOT mean you should resume its \
         task: even on similar topics, the latest user message WINS...\n";

    pub const TASK_HEADING: &'static str = "## Historical Task Snapshot";
    pub const IN_PROGRESS_HEADING: &'static str = "## Historical In-Progress State";
    pub const PENDING_HEADING: &'static str = "## Historical Pending User Asks";
    pub const REMAINING_HEADING: &'static str = "## Historical Remaining Work";
    pub const END_MARKER: &'static str =
        "--- END OF CONTEXT SUMMARY — respond to the message below ---";

    /// Build the LLM summarization prompt by embedding the supplied turns
    /// as a JSON block surrounded by the four section headings. The model
    /// is expected to fill in each section; the result is then re-parseable
    /// by [`EnhancedSummaryTemplate::parse`].
    ///
    /// Generic over `AsRef<str>` so callers can pass `&[T]` where each item
    /// exposes its text via `as_ref()`. Each item is wrapped as a tiny
    /// `{role, text}` JSON object so the model sees structured data.
    pub fn format_prompt_for_turns<T: AsRef<str>>(turns: &[T]) -> String {
        let mut prompt = String::new();
        prompt.push_str(Self::SUMMARY_PREFIX);
        prompt.push_str(Self::TASK_HEADING);
        prompt.push_str("\n\n");
        // Build a JSON array of {text: "..."} for each turn.
        prompt.push('[');
        for (i, t) in turns.iter().enumerate() {
            if i > 0 {
                prompt.push(',');
            }
            let text = t.as_ref();
            // Escape JSON string minimally.
            let escaped = text
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t");
            prompt.push_str(&format!("{{\"text\":\"{}\"}}", escaped));
        }
        prompt.push_str("]\n\n");
        prompt.push_str(Self::IN_PROGRESS_HEADING);
        prompt.push_str("\n\n(derive from the snapshot above)\n\n");
        prompt.push_str(Self::PENDING_HEADING);
        prompt.push_str("\n\n(extract from the snapshot)\n\n");
        prompt.push_str(Self::REMAINING_HEADING);
        prompt.push_str("\n\n(extract from the snapshot)\n\n");
        prompt.push_str(Self::END_MARKER);
        prompt
    }

    /// Create a new empty template
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Parse a summary string into a structured template
    pub fn parse(summary: &str) -> Self {
        let mut template = Self::new();

        let sections: Vec<&str> = summary.split("## ").collect();

        for section in sections {
            let trimmed = section.trim();
            if trimmed.is_empty() {
                continue;
            }

            if trimmed.starts_with("Historical Task Snapshot") {
                template.historical_task = Self::extract_section_content(trimmed);
            } else if trimmed.starts_with("Historical In-Progress State") {
                template.in_progress_state = Self::extract_section_content(trimmed);
            } else if trimmed.starts_with("Historical Pending User Asks") {
                template.pending_asks = Self::extract_list_items(trimmed);
            } else if trimmed.starts_with("Historical Remaining Work") {
                template.remaining_work = Self::extract_list_items(trimmed);
            }
        }

        template
    }

    fn extract_section_content(section: &str) -> String {
        // Skip the heading line and get content until next heading or end marker
        let lines: Vec<&str> = section.lines().collect();
        if lines.len() < 2 {
            return String::new();
        }

        let content_lines: Vec<&str> = lines[1..]
            .iter()
            .take_while(|l| !l.contains("## ") && !l.contains(Self::END_MARKER))
            .map(|l| l.trim())
            .collect();

        content_lines.join("\n").trim().to_string()
    }

    fn extract_list_items(section: &str) -> Vec<String> {
        let lines: Vec<&str> = section.lines().collect();
        if lines.len() < 2 {
            return Vec::new();
        }

        lines[1..]
            .iter()
            .take_while(|l| !l.contains("## ") && !l.contains(Self::END_MARKER))
            .filter(|l| {
                let trimmed = l.trim();
                trimmed.starts_with('-') || trimmed.starts_with('*') || trimmed.starts_with('•')
            })
            .map(|l| l.trim_start_matches(|c: char| c == '-' || c == '*' || c == '•' || c.is_whitespace()).to_string())
            .collect()
    }
    
    /// Format the template into a summary string
    pub fn format(&self) -> String {
        let mut output = String::new();
        
        output.push_str(Self::SUMMARY_PREFIX);
        output.push('\n');
        
        // Historical Task Snapshot
        if !self.historical_task.is_empty() {
            output.push_str(Self::TASK_HEADING);
            output.push_str("\n\n");
            output.push_str(&self.historical_task);
            output.push_str("\n\n");
        }
        
        // In-Progress State
        if !self.in_progress_state.is_empty() {
            output.push_str(Self::IN_PROGRESS_HEADING);
            output.push_str("\n\n");
            output.push_str(&self.in_progress_state);
            output.push_str("\n\n");
        }
        
        // Pending User Asks
        if !self.pending_asks.is_empty() {
            output.push_str(Self::PENDING_HEADING);
            output.push_str("\n\n");
            for item in &self.pending_asks {
                output.push_str("- ");
                output.push_str(item);
                output.push('\n');
            }
            output.push('\n');
        }
        
        // Remaining Work
        if !self.remaining_work.is_empty() {
            output.push_str(Self::REMAINING_HEADING);
            output.push_str("\n\n");
            for item in &self.remaining_work {
                output.push_str("- ");
                output.push_str(item);
                output.push('\n');
            }
            output.push('\n');
        }
        
        output.push_str(Self::END_MARKER);
        output
    }
    
    /// Check if this summary has any content
    pub fn is_empty(&self) -> bool {
        self.historical_task.is_empty() 
            && self.in_progress_state.is_empty()
            && self.pending_asks.is_empty()
            && self.remaining_work.is_empty()
    }
    
    /// Update the template with a new iteration (for iterative summarization)
    pub fn update_with(&mut self, new_content: &str, section: SummarySection) {
        match section {
            SummarySection::Task => {
                if !self.historical_task.is_empty() {
                    self.in_progress_state = self.historical_task.clone();
                }
                self.historical_task = new_content.to_string();
            }
            SummarySection::InProgress => {
                self.in_progress_state = new_content.to_string();
            }
            SummarySection::Pending => {
                // Append to pending items
                for line in new_content.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        self.pending_asks.push(trimmed.to_string());
                    }
                }
            }
            SummarySection::Remaining => {
                // Append to remaining work
                for line in new_content.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        self.remaining_work.push(trimmed.to_string());
                    }
                }
            }
        }
    }
}

/// Which section of the summary to update
#[derive(Debug, Clone, Copy)]
pub enum SummarySection {
    Task,
    InProgress,
    Pending,
    Remaining,
}

/// Builder for creating summaries programmatically
#[derive(Default)]
pub struct SummaryBuilder {
    template: EnhancedSummaryTemplate,
}

impl SummaryBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn task(mut self, task: impl Into<String>) -> Self {
        self.template.historical_task = task.into();
        self
    }
    
    pub fn in_progress(mut self, state: impl Into<String>) -> Self {
        self.template.in_progress_state = state.into();
        self
    }
    
    pub fn add_pending(mut self, item: impl Into<String>) -> Self {
        self.template.pending_asks.push(item.into());
        self
    }
    
    pub fn add_remaining(mut self, item: impl Into<String>) -> Self {
        self.template.remaining_work.push(item.into());
        self
    }
    
    pub fn build(self) -> String {
        self.template.format()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_template_new() {
        let template = EnhancedSummaryTemplate::new();
        assert!(template.is_empty());
    }
    
    #[test]
    fn test_template_format() {
        let template = EnhancedSummaryTemplate {
            historical_task: "Test task".to_string(),
            in_progress_state: "Working on it".to_string(),
            pending_asks: vec!["Question 1".to_string()],
            remaining_work: vec!["Item 1".to_string()],
        };
        
        let formatted = template.format();
        assert!(formatted.contains("## Historical Task Snapshot"));
        assert!(formatted.contains("Test task"));
        assert!(formatted.contains("--- END OF CONTEXT SUMMARY"));
    }
    
    #[test]
    fn test_template_parse() {
        let formatted = "## Historical Task Snapshot\n\nTest task\n\n## Historical In-Progress State\n\nWorking on it";
        let template = EnhancedSummaryTemplate::parse(formatted);
        assert_eq!(template.historical_task, "Test task");
        assert_eq!(template.in_progress_state, "Working on it");
    }
    
    #[test]
    fn test_summary_builder() {
        let result = SummaryBuilder::new()
            .task("Main task")
            .in_progress("Current state")
            .add_pending("Pending item")
            .add_remaining("Remaining item")
            .build();
        
        assert!(result.contains("Main task"));
        assert!(result.contains("Pending item"));
    }
    
    #[test]
    fn test_is_empty() {
        let empty = EnhancedSummaryTemplate::new();
        assert!(empty.is_empty());
        
        let non_empty = EnhancedSummaryTemplate {
            historical_task: "task".to_string(),
            ..Default::default()
        };
        assert!(!non_empty.is_empty());
    }
}
