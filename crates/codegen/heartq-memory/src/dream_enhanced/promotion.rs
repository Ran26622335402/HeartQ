//! Dream promotion pipeline — collect session snippets, rank, quarantine-filter.

use std::path::Path;

use chrono::Utc;

use super::evidence::{PromotionEvidenceEntry, PromotionEvidenceStore};
use super::quarantine::QuarantineRules;
use super::ranking::{rank_promotion_candidates, PromotionCandidate, SignalType};

/// Collect promotion candidates from session log files referenced by `stems`.
pub fn collect_candidates_from_sessions(
    sessions_dir: &Path,
    stems: &[String],
) -> PromotionEvidenceStore {
    let mut store = PromotionEvidenceStore::new();
    let now = Utc::now().to_rfc3339();
    let day = Utc::now().format("%Y-%m-%d").to_string();

    for stem in stems {
        let rel_path = format!("sessions/{stem}.md");
        let path = sessions_dir.join(format!("{stem}.md"));
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for snippet in extract_snippets(&content) {
            let trimmed = snippet.trim();
            if trimmed.len() < 20 {
                continue;
            }
            let signal = SignalType::from_text(trimmed);
            let snippet_sha256 = hash_hex(trimmed);
            let claim_sha256 = hash_hex(&trimmed.to_lowercase());
            let candidate_id = format!("{stem}:{snippet_sha256}");
            let mut entry = PromotionEvidenceEntry {
                candidate_id: candidate_id.clone(),
                agent_id: "main".into(),
                source_path: rel_path.clone(),
                source_kind: "session_log".into(),
                snippet: trimmed.to_string(),
                snippet_sha256,
                claim_sha256,
                first_seen_at: now.clone(),
                last_seen_at: now.clone(),
                seen_count: 1,
                source_days: vec![day.clone()],
                status: "candidate".into(),
                ..PromotionEvidenceEntry::default()
            };
            match signal {
                SignalType::Positive => entry.positive_signal_count = 1,
                SignalType::Correction => entry.correction_signal_count = 1,
                SignalType::Failure => entry.failure_signal_count = 1,
                SignalType::Manual => entry.manual_signal_count = 1,
                SignalType::Neutral => {}
            }
            store.upsert(entry);
        }
    }
    store
}

/// Rank candidates then drop any that fail quarantine rules.
pub fn filter_and_rank_candidates(
    store: &PromotionEvidenceStore,
    rules: &QuarantineRules,
    min_score: f64,
    limit: Option<usize>,
) -> Vec<PromotionCandidate> {
    rank_promotion_candidates(store, min_score, 3, 1, limit)
        .into_iter()
        .filter(|c| {
            !rules.is_quarantined_path(&c.source_path) && !rules.is_quarantined_text(&c.snippet)
        })
        .collect()
}

/// Append ranked promotion snippets to workspace MEMORY.md.
pub fn apply_promotion_candidates(
    storage: &crate::storage::MemoryStorage,
    candidates: &[PromotionCandidate],
) -> usize {
    if candidates.is_empty() {
        return 0;
    }
    let path = storage.workspace_memory_file();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut section = String::from("\n\n## Dream promotions\n\n");
    for c in candidates {
        let line = c.snippet.lines().next().unwrap_or(&c.snippet);
        section.push_str("- ");
        section.push_str(line);
        section.push('\n');
    }
    let merged = format!("{existing}{section}");
    match storage.write_long_term(crate::storage::MemoryScope::Workspace, &merged) {
        Ok(()) => candidates.len(),
        Err(e) => {
            tracing::warn!(
                target: "xai_memory",
                error = %e,
                "DREAM_ENHANCED: failed to apply promotion candidates"
            );
            0
        }
    }
}

/// Extract ##-section bodies and substantive bullet lines from session markdown.
fn extract_snippets(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current_section = String::new();

    for line in content.lines() {
        if line.starts_with("## ") {
            if !current_section.trim().is_empty() {
                out.push(current_section.trim().to_string());
            }
            current_section = line.to_string();
            current_section.push('\n');
        } else if line.starts_with("- ") || line.starts_with("* ") {
            let bullet = line.trim_start_matches(|c| c == '-' || c == '*' || c == ' ');
            if bullet.len() >= 20 {
                out.push(bullet.to_string());
            }
            current_section.push_str(line);
            current_section.push('\n');
        } else {
            current_section.push_str(line);
            current_section.push('\n');
        }
    }
    if !current_section.trim().is_empty() {
        out.push(current_section.trim().to_string());
    }
    out
}

fn hash_hex(s: &str) -> String {
    blake3::hash(s.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_drops_quarantined_text() {
        let mut store = PromotionEvidenceStore::new();
        store.upsert(PromotionEvidenceEntry {
            candidate_id: "a".into(),
            snippet: "opensquilla-dream-promotion: secret".into(),
            seen_count: 2,
            positive_signal_count: 1,
            status: "candidate".into(),
            ..PromotionEvidenceEntry::default()
        });
        let rules = QuarantineRules::default();
        let ranked = filter_and_rank_candidates(&store, &rules, 0.0, None);
        assert!(ranked.is_empty());
    }
}
