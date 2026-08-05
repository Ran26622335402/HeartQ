//! Obligation extractor for compaction.
//!
//! Extracts high-signal continuity facts from conversation entries before
//! they are removed during transcript compaction.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::patterns::{
    after_label, clean_text, contains_marker, starts_with_prefix, ARTIFACT_MARKERS,
    ARTIFACT_NAME_RE, COMMAND_RE, CONSTRAINT_PREFIXES, DECISION_PREFIXES, DO_NOT_REPEAT_MARKERS,
    ERROR_MARKERS, GOAL_PREFIXES, IDENTIFIER_RE, MAX_COMMAND_CHARS, MAX_OBLIGATION_VALUE_CHARS,
    NEXT_ACTION_MARKERS, PATH_RE, QUESTION_MARK_RE,
};

/// Maximum obligations to extract
const DEFAULT_MAX_OBLIGATIONS: usize = 64;

/// Kind of obligation extracted
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationKind {
    /// Tool call or result ID
    ToolResultId,
    /// User-defined goal (goal: prefix)
    UserGoal,
    /// User constraint (constraint: prefix)
    UserConstraint,
    /// Decision or rationale (decision: prefix)
    Decision,
    /// Next action (assistant message)
    NextAction,
    /// Do-not-repeat instruction
    DoNotRepeat,
    /// Artifact path or name
    ArtifactPath,
    /// Unresolved question
    UnresolvedQuestion,
    /// Failed command or error
    FailedCommand,
    /// Tool result fact
    ToolResultFact,
    /// Important identifier (UUID, hash)
    ImportantIdentifier,
    /// File path
    FilePath,
    /// Shell command
    Command,
}

impl std::fmt::Display for ObligationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ObligationKind::ToolResultId => write!(f, "tool_result_id"),
            ObligationKind::UserGoal => write!(f, "user_goal"),
            ObligationKind::UserConstraint => write!(f, "user_constraint"),
            ObligationKind::Decision => write!(f, "decision"),
            ObligationKind::NextAction => write!(f, "next_action"),
            ObligationKind::DoNotRepeat => write!(f, "do_not_repeat"),
            ObligationKind::ArtifactPath => write!(f, "artifact_path"),
            ObligationKind::UnresolvedQuestion => write!(f, "unresolved_question"),
            ObligationKind::FailedCommand => write!(f, "failed_command"),
            ObligationKind::ToolResultFact => write!(f, "tool_result_fact"),
            ObligationKind::ImportantIdentifier => write!(f, "important_identifier"),
            ObligationKind::FilePath => write!(f, "file_path"),
            ObligationKind::Command => write!(f, "command"),
        }
    }
}

/// An extracted obligation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionObligation {
    /// Kind of obligation
    pub kind: ObligationKind,
    /// Extracted value
    pub value: String,
    /// Source role (user/assistant/system)
    pub source_role: Option<String>,
    /// Source entry ID
    pub source_entry_id: Option<i64>,
    /// Whether this is critical for continuity
    #[serde(default = "default_critical")]
    pub critical: bool,
}

fn default_critical() -> bool {
    true
}

impl CompactionObligation {
    /// Create a new obligation
    pub fn new(
        kind: ObligationKind,
        value: String,
        source_role: Option<String>,
        source_entry_id: Option<i64>,
    ) -> Self {
        Self {
            kind,
            value,
            source_role,
            source_entry_id,
            critical: true,
        }
    }

    /// Create a label for this obligation (for coverage checking)
    pub fn label(&self) -> String {
        format!("{}: {}", self.kind, self.value)
    }
}

/// Trait for accessing entry fields
pub trait CompactionEntry {
    fn role(&self) -> Option<&str>;
    fn entry_id(&self) -> Option<i64>;
    fn content(&self) -> &str;
    fn tool_calls(&self) -> Vec<ToolCallInfo>;
}

/// Tool call information
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub id: Option<String>,
    pub name: Option<String>,
    pub tool_use_id: Option<String>,
    pub is_result: bool,
    pub result_content: Option<String>,
}

/// Obligation extractor
pub struct ObligationExtractor {
    max_obligations: usize,
    seen: HashSet<(String, String)>,
}

impl ObligationExtractor {
    /// Create a new extractor with default max obligations (64)
    pub fn new() -> Self {
        Self::with_max( DEFAULT_MAX_OBLIGATIONS)
    }

    /// Create a new extractor with custom max obligations
    pub fn with_max(max_obligations: usize) -> Self {
        Self {
            max_obligations,
            seen: HashSet::new(),
        }
    }

    /// Extract obligations from entries
    pub fn extract<T: CompactionEntry>(&mut self, entries: &[T]) -> Vec<CompactionObligation> {
        let mut obligations = Vec::new();

        for entry in entries {
            self.extract_from_entry(entry, &mut obligations);
        }

        obligations
    }

    fn extract_from_entry<T: CompactionEntry>(
        &mut self,
        entry: &T,
        obligations: &mut Vec<CompactionObligation>,
    ) {
        if obligations.len() >= self.max_obligations {
            return;
        }

        let role = entry.role().map(String::from);
        let entry_id = entry.entry_id();
        let content = entry.content();

        // Process each line
        for line in content.lines() {
            let cleaned = clean_text(line, MAX_OBLIGATION_VALUE_CHARS);
            if cleaned.is_empty() {
                continue;
            }
            let lower = cleaned.to_lowercase();

            // User goals
            if role.as_deref() == Some("user") && starts_with_prefix(&lower, GOAL_PREFIXES) {
                self.add_obligation(
                    obligations,
                    ObligationKind::UserGoal,
                    after_label(&cleaned),
                    role.as_deref(),
                    entry_id,
                );
            }

            // User constraints
            if role.as_deref() == Some("user") && starts_with_prefix(&lower, CONSTRAINT_PREFIXES) {
                self.add_obligation(
                    obligations,
                    ObligationKind::UserConstraint,
                    after_label(&cleaned),
                    role.as_deref(),
                    entry_id,
                );
            }

            // Decisions and rationale
            if starts_with_prefix(&lower, DECISION_PREFIXES) {
                self.add_obligation(
                    obligations,
                    ObligationKind::Decision,
                    after_label(&cleaned),
                    role.as_deref(),
                    entry_id,
                );
            }

            // Next actions (assistant only)
            if role.as_deref() == Some("assistant") && contains_marker(&lower, NEXT_ACTION_MARKERS) {
                self.add_obligation(
                    obligations,
                    ObligationKind::NextAction,
                    &cleaned,
                    role.as_deref(),
                    entry_id,
                );
            }

            // Do not repeat
            if contains_marker(&lower, DO_NOT_REPEAT_MARKERS) {
                self.add_obligation(
                    obligations,
                    ObligationKind::DoNotRepeat,
                    &cleaned,
                    role.as_deref(),
                    entry_id,
                );
            }

            // Artifacts
            if contains_marker(&lower, ARTIFACT_MARKERS) {
                for cap in ARTIFACT_NAME_RE.captures_iter(&cleaned) {
                    if let Some(m) = cap.get(0) {
                        let path = m.as_str();
                        if !path.contains('/') && !path.contains('\\') {
                            self.add_obligation(
                                obligations,
                                ObligationKind::ArtifactPath,
                                path.trim_end_matches('.'),
                                role.as_deref(),
                                entry_id,
                            );
                        }
                    }
                }
            }

            // Unresolved questions
            if QUESTION_MARK_RE.is_match(&cleaned) {
                self.add_obligation(
                    obligations,
                    ObligationKind::UnresolvedQuestion,
                    &cleaned,
                    role.as_deref(),
                    entry_id,
                );
            }

            // Failed commands
            if contains_marker(&lower, ERROR_MARKERS) {
                self.add_obligation(
                    obligations,
                    ObligationKind::FailedCommand,
                    &cleaned,
                    role.as_deref(),
                    entry_id,
                );
            }
        }

        // Extract tool calls
        for tool_call in entry.tool_calls() {
            // Tool call ID
            let tool_id = tool_call
                .id
                .or(tool_call.tool_use_id);
            if let Some(id) = tool_id {
                self.add_obligation(
                    obligations,
                    ObligationKind::ToolResultId,
                    &id,
                    role.as_deref(),
                    entry_id,
                );
            }

            // Tool result fact
            if tool_call.is_result {
                if let Some(result) = tool_call.result_content {
                    self.add_obligation(
                        obligations,
                        ObligationKind::ToolResultFact,
                        &result,
                        role.as_deref(),
                        entry_id,
                    );
                }
            }
        }

        // Important identifiers (UUIDs, hashes)
        for cap in IDENTIFIER_RE.captures_iter(content) {
            if let Some(m) = cap.get(0) {
                self.add_obligation(
                    obligations,
                    ObligationKind::ImportantIdentifier,
                    m.as_str().trim_end_matches('.'),
                    role.as_deref(),
                    entry_id,
                );
            }
        }

        // File paths
        for cap in PATH_RE.captures_iter(content) {
            if let Some(m) = cap.get(0) {
                self.add_obligation(
                    obligations,
                    ObligationKind::FilePath,
                    m.as_str().trim_end_matches('.'),
                    role.as_deref(),
                    entry_id,
                );
            }
        }

        // Commands (with higher char limit)
        for cap in COMMAND_RE.captures_iter(content) {
            if let Some(m) = cap.get(0) {
                let cleaned_cmd = clean_text(m.as_str(), MAX_COMMAND_CHARS);
                self.add_obligation(
                    obligations,
                    ObligationKind::Command,
                    &cleaned_cmd,
                    role.as_deref(),
                    entry_id,
                );
            }
        }
    }

    fn add_obligation(
        &mut self,
        obligations: &mut Vec<CompactionObligation>,
        kind: ObligationKind,
        value: &str,
        source_role: Option<&str>,
        source_entry_id: Option<i64>,
    ) {
        if obligations.len() >= self.max_obligations {
            return;
        }

        let cleaned_value = clean_text(value, MAX_OBLIGATION_VALUE_CHARS);
        if cleaned_value.is_empty() {
            return;
        }

        let key = (kind.to_string(), cleaned_value.to_lowercase());
        if self.seen.contains(&key) {
            return;
        }

        self.seen.insert(key);
        obligations.push(CompactionObligation::new(
            kind,
            cleaned_value,
            source_role.map(String::from),
            source_entry_id,
        ));
    }
}

impl Default for ObligationExtractor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Blanket implementation for string types (for testing)
// ---------------------------------------------------------------------------

/// Simple entry for testing
#[derive(Debug, Clone)]
pub struct SimpleEntry {
    pub role: Option<String>,
    pub id: Option<i64>,
    pub content: String,
    pub tool_calls: Vec<ToolCallInfo>,
}

impl SimpleEntry {
    pub fn user(content: &str) -> Self {
        Self {
            role: Some("user".to_string()),
            id: None,
            content: content.to_string(),
            tool_calls: Vec::new(),
        }
    }

    pub fn assistant(content: &str) -> Self {
        Self {
            role: Some("assistant".to_string()),
            id: None,
            content: content.to_string(),
            tool_calls: Vec::new(),
        }
    }
}

impl CompactionEntry for SimpleEntry {
    fn role(&self) -> Option<&str> {
        self.role.as_deref()
    }

    fn entry_id(&self) -> Option<i64> {
        self.id
    }

    fn content(&self) -> &str {
        &self.content
    }

    fn tool_calls(&self) -> Vec<ToolCallInfo> {
        self.tool_calls.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_user_goal() {
        let mut extractor = ObligationExtractor::new();
        let entries = vec![SimpleEntry::user("Goal: Build a Rust compiler")];
        let obligations = extractor.extract(&entries);

        assert!(obligations.iter().any(|o| o.kind == ObligationKind::UserGoal));
        let goal = obligations.iter().find(|o| o.kind == ObligationKind::UserGoal).unwrap();
        assert!(goal.value.contains("Build a Rust compiler"));
    }

    #[test]
    fn test_extract_constraint() {
        let mut extractor = ObligationExtractor::new();
        let entries = vec![SimpleEntry::user("Constraint: Must use async/await")];
        let obligations = extractor.extract(&entries);

        assert!(obligations
            .iter()
            .any(|o| o.kind == ObligationKind::UserConstraint));
    }

    #[test]
    fn test_extract_decision() {
        let mut extractor = ObligationExtractor::new();
        let entries = vec![SimpleEntry::assistant(
            "Decision: Using trait objects for abstraction",
        )];
        let obligations = extractor.extract(&entries);

        assert!(obligations.iter().any(|o| o.kind == ObligationKind::Decision));
    }

    #[test]
    fn test_extract_next_action() {
        let mut extractor = ObligationExtractor::new();
        let entries = vec![SimpleEntry::assistant(
            "Next I will implement the parser module",
        )];
        let obligations = extractor.extract(&entries);

        assert!(obligations
            .iter()
            .any(|o| o.kind == ObligationKind::NextAction));
    }

    #[test]
    fn test_extract_do_not_repeat() {
        let mut extractor = ObligationExtractor::new();
        let entries = vec![SimpleEntry::user(
            "Do not repeat the same mistake again",
        )];
        let obligations = extractor.extract(&entries);

        assert!(obligations
            .iter()
            .any(|o| o.kind == ObligationKind::DoNotRepeat));
    }

    #[test]
    fn test_extract_unresolved_question() {
        let mut extractor = ObligationExtractor::new();
        let entries = vec![SimpleEntry::user("How should we handle errors?")];
        let obligations = extractor.extract(&entries);

        assert!(obligations
            .iter()
            .any(|o| o.kind == ObligationKind::UnresolvedQuestion));
    }

    #[test]
    fn test_extract_failed_command() {
        let mut extractor = ObligationExtractor::new();
        let entries = vec![SimpleEntry::assistant("Error: Failed to compile")];
        let obligations = extractor.extract(&entries);

        assert!(obligations
            .iter()
            .any(|o| o.kind == ObligationKind::FailedCommand));
    }

    #[test]
    fn test_extract_file_path() {
        let mut extractor = ObligationExtractor::new();
        let entries = vec![SimpleEntry::assistant(
            "I modified src/main.rs to add the new feature",
        )];
        let obligations = extractor.extract(&entries);

        assert!(obligations
            .iter()
            .any(|o| o.kind == ObligationKind::FilePath && o.value.contains("main.rs")));
    }

    #[test]
    fn test_extract_command() {
        let mut extractor = ObligationExtractor::new();
        let entries = vec![SimpleEntry::assistant(
            "Run: pytest tests/test_main.py -v",
        )];
        let obligations = extractor.extract(&entries);

        assert!(obligations
            .iter()
            .any(|o| o.kind == ObligationKind::Command));
    }

    #[test]
    fn test_extract_uuid() {
        let mut extractor = ObligationExtractor::new();
        let entries = vec![SimpleEntry::assistant(
            "Session ID: 550e8400-e29b-41d4-a716-446655440000",
        )];
        let obligations = extractor.extract(&entries);

        assert!(obligations
            .iter()
            .any(|o| o.kind == ObligationKind::ImportantIdentifier));
    }

    #[test]
    fn test_deduplication() {
        let mut extractor = ObligationExtractor::new();
        let entries = vec![
            SimpleEntry::user("Goal: Build a compiler"),
            SimpleEntry::assistant("Goal: Build a compiler"),
        ];
        let obligations = extractor.extract(&entries);

        // Should only have one obligation despite two identical entries
        let goals: Vec<_> = obligations
            .iter()
            .filter(|o| o.kind == ObligationKind::UserGoal)
            .collect();
        assert_eq!(goals.len(), 1);
    }

    #[test]
    fn test_max_obligations() {
        let mut extractor = ObligationExtractor::with_max(3);

        let mut entries = Vec::new();
        for i in 0..10 {
            entries.push(SimpleEntry::user(&format!("Goal {}: Task {}", i, i)));
        }

        let obligations = extractor.extract(&entries);
        assert!(obligations.len() <= 3);
    }

    #[test]
    fn test_obligation_kind_display() {
        assert_eq!(ObligationKind::UserGoal.to_string(), "user_goal");
        assert_eq!(ObligationKind::ToolResultId.to_string(), "tool_result_id");
    }

    #[test]
    fn test_obligation_label() {
        let obligation = CompactionObligation::new(
            ObligationKind::UserGoal,
            "Build a compiler".to_string(),
            Some("user".to_string()),
            None,
        );
        assert_eq!(obligation.label(), "user_goal: Build a compiler");
    }
}
