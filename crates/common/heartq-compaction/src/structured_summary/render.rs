//! Structured summary renderer.
//!
//! Renders a StructuredCompactionSummary to a human-readable text format.

use super::builder::{
    DecisionOrRationale, FileOrArtifact, KnownFailure, StructuredCompactionSummary, ToolResultRef,
};

/// Render a structured compaction summary to text.
///
/// This function renders the structured summary to a text format that mirrors
/// OpenSquilla's `render_structured_summary()` function.
pub fn render_structured_summary(summary: &StructuredCompactionSummary) -> String {
    let mut lines: Vec<String> = Vec::new();

    // Header
    lines.push("[Structured Compaction Summary]".to_string());
    lines.push(String::new());

    // Goal
    append_scalar_section(&mut lines, "Goal", &summary.user_goal);

    // Current Status
    append_scalar_section(&mut lines, "Current Status", &summary.current_status);

    // Next Action
    if let Some(ref action) = summary.next_action {
        append_scalar_section(&mut lines, "Next Action", action);
    }

    // Completed Steps
    append_list_section(&mut lines, "Completed Steps", &summary.completed_steps);

    // Open Steps
    append_list_section(&mut lines, "Open Steps", &summary.open_steps);

    // Files and Artifacts
    append_file_or_artifact_list(&mut lines, &summary.files_and_artifacts);

    // Tool Results To Remember
    append_tool_result_list(&mut lines, &summary.tool_results_to_remember);

    // Decisions and Rationale
    append_decision_list(&mut lines, &summary.decisions_and_rationale);

    // Known Failures
    append_failure_list(&mut lines, &summary.known_failures);

    // Important Identifiers
    append_list_section(&mut lines, "Important Identifiers", &summary.important_identifiers);

    // Constraints and Preferences
    append_list_section(
        &mut lines,
        "Constraints and Preferences",
        &summary.constraints_and_preferences,
    );

    // Do Not Repeat
    append_list_section(&mut lines, "Do Not Repeat", &summary.do_not_repeat);

    // Unresolved Questions
    append_list_section(&mut lines, "Unresolved Questions", &summary.unresolved_questions);

    // Critical Carry Forward
    append_list_section(
        &mut lines,
        "Critical Carry Forward",
        &summary.critical_carry_forward,
    );

    lines.join("\n")
}

/// Append a scalar section to the lines
fn append_scalar_section(lines: &mut Vec<String>, title: &str, value: &str) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }

    lines.push(format!("{}:", title));
    lines.push(trimmed.to_string());
    lines.push(String::new());
}

/// Append a list section to the lines
fn append_list_section<T: AsRef<str>>(lines: &mut Vec<String>, title: &str, values: &[T]) {
    let rendered: Vec<String> = values
        .iter()
        .map(|v| v.as_ref().trim().to_string())
        .filter(|v| !v.is_empty())
        .collect();

    if rendered.is_empty() {
        return;
    }

    lines.push(format!("{}:", title));
    lines.extend(rendered.iter().map(|v| format!("- {}", v)));
    lines.push(String::new());
}

/// Append file/artifact list
fn append_file_or_artifact_list(lines: &mut Vec<String>, values: &[FileOrArtifact]) {
    if values.is_empty() {
        return;
    }

    let mut has_content = false;
    lines.push("Files and Artifacts:".to_string());

    for value in values {
        if let Some(ref path) = value.path {
            lines.push(format!("- path: {}", path));
            has_content = true;
        }
        if let Some(ref artifact) = value.artifact {
            lines.push(format!("- artifact: {}", artifact));
            has_content = true;
        }
    }

    if has_content {
        lines.push(String::new());
    }
}

/// Append tool result list
fn append_tool_result_list(lines: &mut Vec<String>, values: &[ToolResultRef]) {
    if values.is_empty() {
        return;
    }

    let mut has_content = false;
    lines.push("Tool Results To Remember:".to_string());

    for value in values {
        if let Some(ref id) = value.id {
            lines.push(format!("- id: {}", id));
            has_content = true;
        }
        if let Some(ref fact) = value.fact {
            lines.push(format!("- fact: {}", fact));
            has_content = true;
        }
    }

    if has_content {
        lines.push(String::new());
    }
}

/// Append decision list
fn append_decision_list(lines: &mut Vec<String>, values: &[DecisionOrRationale]) {
    if values.is_empty() {
        return;
    }

    let mut has_content = false;
    lines.push("Decisions and Rationale:".to_string());

    for value in values {
        if let Some(ref detail) = value.detail {
            lines.push(format!("- detail: {}", detail));
            has_content = true;
        }
        if let Some(ref category) = value.category {
            lines.push(format!("  category: {}", category));
        }
    }

    if has_content {
        lines.push(String::new());
    }
}

/// Append failure list
fn append_failure_list(lines: &mut Vec<String>, values: &[KnownFailure]) {
    if values.is_empty() {
        return;
    }

    let mut has_content = false;
    lines.push("Known Failures:".to_string());

    for value in values {
        if let Some(ref detail) = value.detail {
            lines.push(format!("- detail: {}", detail));
            has_content = true;
        }
        if let Some(ref error_type) = value.error_type {
            lines.push(format!("  error_type: {}", error_type));
        }
    }

    if has_content {
        lines.push(String::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structured_summary::builder::StructuredSummaryBuilder;

    #[test]
    fn test_render_empty_summary() {
        let summary = StructuredCompactionSummary::new();
        let rendered = render_structured_summary(&summary);
        assert!(rendered.contains("[Structured Compaction Summary]"));
    }

    #[test]
    fn test_render_with_content() {
        let summary = StructuredSummaryBuilder::new()
            .user_goal("Build a Rust compiler")
            .current_status("Implementing the parser")
            .next_action("Write code generation")
            .add_completed_step("Lexical analysis")
            .add_file("src/lexer.rs")
            .add_decision("Use LLVM backend")
            .build();

        let rendered = render_structured_summary(&summary);

        assert!(rendered.contains("Goal:"));
        assert!(rendered.contains("Build a Rust compiler"));
        assert!(rendered.contains("Current Status:"));
        assert!(rendered.contains("Implementing the parser"));
        assert!(rendered.contains("Next Action:"));
        assert!(rendered.contains("Write code generation"));
        assert!(rendered.contains("Completed Steps:"));
        assert!(rendered.contains("- Lexical analysis"));
        assert!(rendered.contains("Files and Artifacts:"));
        assert!(rendered.contains("Decisions and Rationale:"));
    }

    #[test]
    fn test_render_preserves_structure() {
        let summary = StructuredSummaryBuilder::new()
            .user_goal("Test goal")
            .add_do_not_repeat("Don't use global variables")
            .add_unresolved_question("How to handle errors?")
            .build();

        let rendered = render_structured_summary(&summary);

        assert!(rendered.contains("Goal:"));
        assert!(rendered.contains("Do Not Repeat:"));
        assert!(rendered.contains("Unresolved Questions:"));
    }
}
