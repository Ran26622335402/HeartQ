//! Structured summary builder.
//!
//! Provides the StructuredCompactionSummary type (15+ fields) and
//! StructuredSummaryBuilder for constructing summaries programmatically.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::obligations::{
    verify_summary_coverage, CompactionObligation, CoverageResult, ObligationKind,
};

/// Schema version for structured summaries
pub const SCHEMA_VERSION: u32 = 1;

/// A file or artifact reference
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileOrArtifact {
    /// File path or artifact name
    pub path: Option<String>,
    /// Whether this is an artifact (vs a regular file)
    pub artifact: Option<String>,
}

impl FileOrArtifact {
    /// Create a new file reference
    pub fn file(path: impl Into<String>) -> Self {
        Self {
            path: Some(path.into()),
            artifact: None,
        }
    }

    /// Create a new artifact reference
    pub fn artifact(name: impl Into<String>) -> Self {
        Self {
            path: None,
            artifact: Some(name.into()),
        }
    }
}

/// A tool result to remember
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolResultRef {
    /// Tool result ID
    pub id: Option<String>,
    /// Tool result fact/content
    pub fact: Option<String>,
}

impl ToolResultRef {
    /// Create from an ID
    pub fn from_id(id: impl Into<String>) -> Self {
        Self {
            id: Some(id.into()),
            fact: None,
        }
    }

    /// Create from a fact
    pub fn from_fact(fact: impl Into<String>) -> Self {
        Self {
            id: None,
            fact: Some(fact.into()),
        }
    }
}

/// A decision or rationale entry
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DecisionOrRationale {
    /// Decision or rationale detail
    pub detail: Option<String>,
    /// Decision category
    pub category: Option<String>,
}

impl DecisionOrRationale {
    /// Create with detail
    pub fn with_detail(detail: impl Into<String>) -> Self {
        Self {
            detail: Some(detail.into()),
            category: None,
        }
    }
}

/// A known failure entry
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KnownFailure {
    /// Failure detail
    pub detail: Option<String>,
    /// Error type
    pub error_type: Option<String>,
}

impl KnownFailure {
    /// Create with detail
    pub fn with_detail(detail: impl Into<String>) -> Self {
        Self {
            detail: Some(detail.into()),
            error_type: None,
        }
    }
}

/// Structured compaction summary with 15+ fields.
///
/// This is derived from OpenSquilla's StructuredCompactionSummary and provides
/// a rich representation of task state for context compaction.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StructuredCompactionSummary {
    /// Schema version
    pub schema_version: u32,
    /// User's goal for this session
    pub user_goal: String,
    /// Current status of the task
    pub current_status: String,
    /// Next action to take
    pub next_action: Option<String>,
    /// Steps that have been completed
    pub completed_steps: Vec<String>,
    /// Steps that are in progress or pending
    pub open_steps: Vec<String>,
    /// Files and artifacts involved
    pub files_and_artifacts: Vec<FileOrArtifact>,
    /// Tool results to remember
    pub tool_results_to_remember: Vec<ToolResultRef>,
    /// Decisions and their rationale
    pub decisions_and_rationale: Vec<DecisionOrRationale>,
    /// Known failures or errors
    pub known_failures: Vec<KnownFailure>,
    /// Important identifiers (UUIDs, hashes, etc.)
    pub important_identifiers: Vec<String>,
    /// User constraints and preferences
    pub constraints_and_preferences: Vec<String>,
    /// Things to avoid repeating
    pub do_not_repeat: Vec<String>,
    /// Questions that remain unresolved
    pub unresolved_questions: Vec<String>,
    /// Critical items that must carry forward
    pub critical_carry_forward: Vec<String>,
    /// Source coverage information
    pub source_coverage: HashMap<String, serde_json::Value>,
}

impl StructuredCompactionSummary {
    /// Create a new empty summary
    pub fn new() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            ..Default::default()
        }
    }

    /// Check if the summary is empty
    pub fn is_empty(&self) -> bool {
        self.user_goal.is_empty()
            && self.current_status.is_empty()
            && self.next_action.is_none()
            && self.completed_steps.is_empty()
            && self.open_steps.is_empty()
            && self.files_and_artifacts.is_empty()
            && self.tool_results_to_remember.is_empty()
            && self.decisions_and_rationale.is_empty()
            && self.known_failures.is_empty()
            && self.important_identifiers.is_empty()
            && self.constraints_and_preferences.is_empty()
            && self.do_not_repeat.is_empty()
            && self.unresolved_questions.is_empty()
            && self.critical_carry_forward.is_empty()
    }
}

/// Builder for StructuredCompactionSummary
#[derive(Default)]
pub struct StructuredSummaryBuilder {
    summary: StructuredCompactionSummary,
}

impl StructuredSummaryBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the user goal
    pub fn user_goal(mut self, goal: impl Into<String>) -> Self {
        self.summary.user_goal = goal.into();
        self
    }

    /// Set the current status
    pub fn current_status(mut self, status: impl Into<String>) -> Self {
        self.summary.current_status = status.into();
        self
    }

    /// Set the next action
    pub fn next_action(mut self, action: impl Into<String>) -> Self {
        self.summary.next_action = Some(action.into());
        self
    }

    /// Add a completed step
    pub fn add_completed_step(mut self, step: impl Into<String>) -> Self {
        self.summary.completed_steps.push(step.into());
        self
    }

    /// Add an open/in-progress step
    pub fn add_open_step(mut self, step: impl Into<String>) -> Self {
        self.summary.open_steps.push(step.into());
        self
    }

    /// Add a file reference
    pub fn add_file(mut self, path: impl Into<String>) -> Self {
        self.summary.files_and_artifacts.push(FileOrArtifact::file(path));
        self
    }

    /// Add an artifact reference
    pub fn add_artifact(mut self, name: impl Into<String>) -> Self {
        self.summary.files_and_artifacts.push(FileOrArtifact::artifact(name));
        self
    }

    /// Add a tool result by ID
    pub fn add_tool_result_id(mut self, id: impl Into<String>) -> Self {
        self.summary
            .tool_results_to_remember
            .push(ToolResultRef::from_id(id));
        self
    }

    /// Add a tool result fact
    pub fn add_tool_result_fact(mut self, fact: impl Into<String>) -> Self {
        self.summary
            .tool_results_to_remember
            .push(ToolResultRef::from_fact(fact));
        self
    }

    /// Add a decision
    pub fn add_decision(mut self, detail: impl Into<String>) -> Self {
        self.summary
            .decisions_and_rationale
            .push(DecisionOrRationale::with_detail(detail));
        self
    }

    /// Add a known failure
    pub fn add_failure(mut self, detail: impl Into<String>) -> Self {
        self.summary
            .known_failures
            .push(KnownFailure::with_detail(detail));
        self
    }

    /// Add an important identifier
    pub fn add_identifier(mut self, id: impl Into<String>) -> Self {
        self.summary.important_identifiers.push(id.into());
        self
    }

    /// Add a constraint or preference
    pub fn add_constraint(mut self, constraint: impl Into<String>) -> Self {
        self.summary.constraints_and_preferences.push(constraint.into());
        self
    }

    /// Add a do-not-repeat item
    pub fn add_do_not_repeat(mut self, item: impl Into<String>) -> Self {
        self.summary.do_not_repeat.push(item.into());
        self
    }

    /// Add an unresolved question
    pub fn add_unresolved_question(mut self, question: impl Into<String>) -> Self {
        self.summary.unresolved_questions.push(question.into());
        self
    }

    /// Add a critical carry-forward item
    pub fn add_critical_carry_forward(mut self, item: impl Into<String>) -> Self {
        self.summary.critical_carry_forward.push(item.into());
        self
    }

    /// Build the structured summary
    pub fn build(self) -> StructuredCompactionSummary {
        self.summary
    }

    /// Build from obligations and summary text
    ///
    /// This parses the obligations and creates a structured summary
    /// with appropriate fields populated from the obligation kinds.
    pub fn from_obligations(
        summary_text: &str,
        obligations: &[CompactionObligation],
    ) -> (Self, CoverageResult) {
        let coverage = verify_summary_coverage(summary_text, obligations, true, false);
        let mut builder = Self::new();

        // Group obligations by kind
        let mut by_kind: HashMap<ObligationKind, Vec<&CompactionObligation>> = HashMap::new();
        for obligation in obligations {
            by_kind.entry(obligation.kind).or_default().push(obligation);
        }

        // Populate from obligations
        if let Some(goals) = by_kind.get(&ObligationKind::UserGoal) {
            if let Some(first) = goals.first() {
                builder = builder.user_goal(&first.value);
            }
        }

        if let Some(goals) = by_kind.get(&ObligationKind::NextAction) {
            if let Some(first) = goals.first() {
                builder = builder.next_action(&first.value);
            }
        }

        // Decisions
        if let Some(decisions) = by_kind.get(&ObligationKind::Decision) {
            for d in decisions {
                builder = builder.add_decision(&d.value);
            }
        }

        // Failures
        if let Some(failures) = by_kind.get(&ObligationKind::FailedCommand) {
            for f in failures {
                builder = builder.add_failure(&f.value);
            }
        }

        // Constraints
        if let Some(constraints) = by_kind.get(&ObligationKind::UserConstraint) {
            for c in constraints {
                builder = builder.add_constraint(&c.value);
            }
        }

        // Do not repeat
        if let Some(items) = by_kind.get(&ObligationKind::DoNotRepeat) {
            for item in items {
                builder = builder.add_do_not_repeat(&item.value);
            }
        }

        // Questions
        if let Some(questions) = by_kind.get(&ObligationKind::UnresolvedQuestion) {
            for q in questions {
                builder = builder.add_unresolved_question(&q.value);
            }
        }

        // Identifiers
        if let Some(ids) = by_kind.get(&ObligationKind::ImportantIdentifier) {
            for id in ids {
                builder = builder.add_identifier(&id.value);
            }
        }

        // File paths
        if let Some(paths) = by_kind.get(&ObligationKind::FilePath) {
            for p in paths {
                builder = builder.add_file(&p.value);
            }
        }

        // Artifacts
        if let Some(artifacts) = by_kind.get(&ObligationKind::ArtifactPath) {
            for a in artifacts {
                builder = builder.add_artifact(&a.value);
            }
        }

        // Tool results
        if let Some(tool_ids) = by_kind.get(&ObligationKind::ToolResultId) {
            for t in tool_ids {
                builder = builder.add_tool_result_id(&t.value);
            }
        }

        if let Some(facts) = by_kind.get(&ObligationKind::ToolResultFact) {
            for f in facts {
                builder = builder.add_tool_result_fact(&f.value);
            }
        }

        // Commands
        if let Some(commands) = by_kind.get(&ObligationKind::Command) {
            for c in commands {
                builder = builder.add_completed_step(format!("Command: {}", c.value));
            }
        }

        // Add critical carry-forward from coverage
        for item in &coverage.critical_carry_forward {
            builder = builder.add_critical_carry_forward(item);
        }

        // Set current status to the summary text
        builder = builder.current_status(summary_text);

        (builder, coverage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_new() {
        let builder = StructuredSummaryBuilder::new();
        assert!(builder.summary.is_empty());
    }

    #[test]
    fn test_builder_chaining() {
        let summary = StructuredSummaryBuilder::new()
            .user_goal("Build a compiler")
            .current_status("Implementing lexer")
            .next_action("Write parser")
            .add_completed_step("Design AST")
            .add_open_step("Implement lexer")
            .add_file("src/lexer.rs")
            .add_decision("Use Rust")
            .build();

        assert_eq!(summary.user_goal, "Build a compiler");
        assert_eq!(summary.current_status, "Implementing lexer");
        assert_eq!(summary.next_action, Some("Write parser".to_string()));
        assert_eq!(summary.completed_steps, vec!["Design AST"]);
        assert_eq!(summary.open_steps, vec!["Implement lexer"]);
        assert_eq!(summary.files_and_artifacts.len(), 1);
        assert_eq!(summary.decisions_and_rationale.len(), 1);
    }

    #[test]
    fn test_file_or_artifact() {
        let file = FileOrArtifact::file("src/main.rs");
        assert_eq!(file.path, Some("src/main.rs".to_string()));
        assert_eq!(file.artifact, None);

        let artifact = FileOrArtifact::artifact("output.pdf");
        assert_eq!(artifact.path, None);
        assert_eq!(artifact.artifact, Some("output.pdf".to_string()));
    }

    #[test]
    fn test_tool_result_ref() {
        let id_ref = ToolResultRef::from_id("call_123");
        assert_eq!(id_ref.id, Some("call_123".to_string()));

        let fact_ref = ToolResultRef::from_fact("Found 42 matches");
        assert_eq!(fact_ref.fact, Some("Found 42 matches".to_string()));
    }

    #[test]
    fn test_summary_is_empty() {
        let empty = StructuredCompactionSummary::new();
        assert!(empty.is_empty());

        let non_empty = StructuredCompactionSummary {
            user_goal: "Test".to_string(),
            ..Default::default()
        };
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_schema_version() {
        let summary = StructuredCompactionSummary::new();
        assert_eq!(summary.schema_version, SCHEMA_VERSION);
    }
}
