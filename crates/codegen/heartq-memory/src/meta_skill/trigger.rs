//! Trigger matching for meta-skills — substring and word-boundary match
//! against `MetaSkillSpec::triggers`.

use super::model::MetaSkillSpec;

/// A meta-skill whose trigger matched user text, with a simple score.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerMatch {
    pub skill_name: String,
    pub score: f64,
}

/// Match `user_text` against each spec's `triggers` field.
///
/// Returns matches sorted by score (descending), then skill name.
/// Word-boundary matches score higher (1.0) than plain substring (0.5).
pub fn match_triggers(user_text: &str, specs: &[MetaSkillSpec]) -> Vec<TriggerMatch> {
    let lower = user_text.to_lowercase();
    let mut matches = Vec::new();

    for spec in specs {
        let mut best = 0.0_f64;
        for trigger in &spec.triggers {
            if trigger.is_empty() {
                continue;
            }
            let trigger_lower = trigger.to_lowercase();
            if word_boundary_match(&lower, &trigger_lower) {
                best = best.max(1.0);
            } else if lower.contains(&trigger_lower) {
                best = best.max(0.5);
            }
        }
        if best > 0.0 {
            matches.push(TriggerMatch {
                skill_name: spec.name.clone(),
                score: best,
            });
        }
    }

    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.skill_name.cmp(&b.skill_name))
    });
    matches
}

/// True when `trigger` appears in `text` at a word boundary.
fn word_boundary_match(text: &str, trigger: &str) -> bool {
    if trigger.is_empty() {
        return false;
    }
    let mut start = 0;
    while let Some(rel) = text[start..].find(trigger) {
        let abs = start + rel;
        let before_ok = abs == 0 || !text[..abs].ends_with(|c: char| c.is_alphanumeric());
        let after_idx = abs + trigger.len();
        let after_ok = after_idx >= text.len()
            || !text[after_idx..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        start = abs + 1;
        if start >= text.len() {
            break;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, triggers: Vec<&str>) -> MetaSkillSpec {
        MetaSkillSpec {
            name: name.into(),
            version: "1".into(),
            description: String::new(),
            steps: vec![],
            checkpoints: vec![],
            digest: "x".into(),
            triggers: triggers.into_iter().map(str::to_string).collect(),
            max_parallelism: None,
        }
    }

    #[test]
    fn substring_match_scores_half() {
        let specs = vec![spec("deploy", vec!["lint"])];
        let m = match_triggers("use splint tool", &specs);
        assert_eq!(m.len(), 1);
        assert!((m[0].score - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn word_boundary_scores_higher() {
        let specs = vec![spec("lint", vec!["lint"])];
        let m = match_triggers("run lint on the crate", &specs);
        assert_eq!(m.len(), 1);
        assert!((m[0].score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn no_match_returns_empty() {
        let specs = vec![spec("x", vec!["foobar"])];
        assert!(match_triggers("hello world", &specs).is_empty());
    }
}
