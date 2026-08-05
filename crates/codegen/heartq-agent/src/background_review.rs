//! Deterministic background review heuristics (v1 — no LLM).
//!
//! Watches recent turn text for repeatable signals that suggest a skill
//! patch or a memory note would help future sessions. Intended to run on
//! a low-frequency timer from the host between user turns.

use std::collections::HashMap;

/// Configuration for the background reviewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundReviewConfig {
    pub enabled: bool,
    pub interval_turns: u32,
}

impl Default for BackgroundReviewConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_turns: 5,
        }
    }
}

/// Action proposed by a background review pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewAction {
    NoOp,
    ProposeSkillPatch {
        skill_name: String,
        hint: String,
    },
    ProposeMemoryNote {
        content: String,
    },
}

/// Stateful reviewer that accumulates turn text and fires heuristics
/// every `interval_turns` when enabled.
#[derive(Debug, Clone)]
pub struct BackgroundReviewer {
    config: BackgroundReviewConfig,
    turns_seen: u32,
    recent_turns: Vec<String>,
    error_counts: HashMap<String, u32>,
}

impl BackgroundReviewer {
    pub fn new(config: BackgroundReviewConfig) -> Self {
        Self {
            config,
            turns_seen: 0,
            recent_turns: Vec::new(),
            error_counts: HashMap::new(),
        }
    }

    /// Ingest one turn's text. Returns a proposed action when the review
    /// interval elapses and a heuristic matches.
    pub fn review_turn(&mut self, turn_text: &str) -> ReviewAction {
        if !self.config.enabled {
            return ReviewAction::NoOp;
        }

        self.turns_seen += 1;
        self.recent_turns.push(turn_text.to_string());
        if self.recent_turns.len() > 12 {
            self.recent_turns.remove(0);
        }

        for line in turn_text.lines() {
            if let Some(err) = extract_error_signature(line) {
                *self.error_counts.entry(err).or_insert(0) += 1;
            }
        }

        if self.turns_seen % self.config.interval_turns != 0 {
            return ReviewAction::NoOp;
        }

        if let Some(note) = detect_remember_phrase(turn_text) {
            return ReviewAction::ProposeMemoryNote { content: note };
        }

        if let Some((skill, hint)) = detect_multi_step_pattern(&self.recent_turns) {
            return ReviewAction::ProposeSkillPatch { skill_name: skill, hint };
        }

        if let Some((err, count)) = top_repeated_error(&self.error_counts) {
            if count >= 3 {
                return ReviewAction::ProposeSkillPatch {
                    skill_name: "error-recovery".into(),
                    hint: format!("Repeated error ({count}x): {err}"),
                };
            }
        }

        ReviewAction::NoOp
    }
}

fn extract_error_signature(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let markers = ["error:", "error[", "failed", "panic:", "exception:"];
    for marker in markers {
        if let Some(idx) = lower.find(marker) {
            let sig = line[idx..].trim();
            if sig.len() >= 8 {
                return Some(sig.chars().take(120).collect());
            }
        }
    }
    None
}

fn detect_remember_phrase(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let triggers = [
        "remember this",
        "don't forget",
        "do not forget",
        "keep in mind",
        "note for future",
        "always use",
    ];
    for trigger in triggers {
        if let Some(idx) = lower.find(trigger) {
            let snippet = text[idx..].lines().next().unwrap_or("").trim();
            if !snippet.is_empty() {
                return Some(snippet.to_string());
            }
        }
    }
    None
}

fn detect_multi_step_pattern(recent: &[String]) -> Option<(String, String)> {
    let joined = recent.join("\n");
    let lower = joined.to_lowercase();
    if !(lower.contains("step 1") || lower.contains("1.") || lower.contains("first,")) {
        return None;
    }
    if !(lower.contains("success") || lower.contains("completed") || lower.contains("done")) {
        return None;
    }
    let steps = recent
        .iter()
        .filter(|t| t.to_lowercase().contains("step"))
        .count();
    if steps >= 2 {
        Some((
            "multi-step-workflow".into(),
            "Detected a successful multi-step workflow; consider capturing as a skill.".into(),
        ))
    } else {
        None
    }
}

fn top_repeated_error(counts: &HashMap<String, u32>) -> Option<(String, u32)> {
    counts
        .iter()
        .max_by_key(|(_, c)| **c)
        .map(|(k, c)| (k.clone(), *c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_reviewer_is_noop() {
        let mut r = BackgroundReviewer::new(BackgroundReviewConfig {
            enabled: false,
            interval_turns: 1,
        });
        assert_eq!(r.review_turn("remember this: use nightly"), ReviewAction::NoOp);
    }

    #[test]
    fn remember_phrase_proposes_memory_note() {
        let mut r = BackgroundReviewer::new(BackgroundReviewConfig {
            enabled: true,
            interval_turns: 1,
        });
        let action = r.review_turn("Please remember this: run fmt before commit.");
        assert!(matches!(action, ReviewAction::ProposeMemoryNote { .. }));
    }

    #[test]
    fn repeated_errors_propose_skill_patch() {
        let mut r = BackgroundReviewer::new(BackgroundReviewConfig {
            enabled: true,
            interval_turns: 1,
        });
        for _ in 0..3 {
            r.review_turn("error: connection refused to db");
        }
        let action = r.review_turn("error: connection refused to db");
        assert!(matches!(action, ReviewAction::ProposeSkillPatch { .. }));
    }
}
