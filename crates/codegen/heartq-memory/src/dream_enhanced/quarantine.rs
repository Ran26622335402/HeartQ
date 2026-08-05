//! Quarantine rules for Dream.
//!
//! Defines paths and text patterns that should be quarantined
//! from dream processing. Derived from OpenSquilla's quarantine module.

/// Quarantine rules for dream candidates.
#[derive(Debug, Clone)]
pub struct QuarantineRules {
    /// Path patterns to quarantine (glob-style)
    pub quarantined_paths: Vec<String>,
    /// Text substrings to quarantine
    pub quarantined_text: Vec<String>,
}

impl Default for QuarantineRules {
    fn default() -> Self {
        Self {
            quarantined_paths: vec![
                "memory/.dream*".to_string(),
                "logs/*".to_string(),
                "dream-*.jsonl".to_string(),
            ],
            quarantined_text: vec![
                "opensquilla-dream-promotion:".to_string(),
                "dream receipt".to_string(),
            ],
        }
    }
}

impl QuarantineRules {
    /// Create new quarantine rules with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a path should be quarantined.
    pub fn is_quarantined_path(&self, path: &str) -> bool {
        let normalized = normalize_path(path);

        // Special cases
        if normalized == "memory/.dream_cursor" {
            return true;
        }
        if normalized.starts_with("memory/.dream") {
            return true;
        }
        if normalized == "logs" || normalized.starts_with("logs/") {
            return true;
        }

        // Check against quarantine patterns
        for pattern in &self.quarantined_paths {
            if matches_glob_pattern(&normalized, pattern) {
                return true;
            }
        }

        false
    }

    /// Check if text should be quarantined.
    pub fn is_quarantined_text(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        for marker in &self.quarantined_text {
            if lower.contains(&marker.to_lowercase()) {
                return true;
            }
        }
        false
    }

    /// Add a path pattern to quarantine.
    pub fn add_path_pattern(&mut self, pattern: impl Into<String>) {
        self.quarantined_paths.push(pattern.into());
    }

    /// Add a text marker to quarantine.
    pub fn add_text_marker(&mut self, marker: impl Into<String>) {
        self.quarantined_text.push(marker.into());
    }
}

/// Normalize a path for comparison.
fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_string()
}

/// Check if a path matches a glob pattern.
/// This is a simplified implementation supporting:
/// - `*` at the end of a path segment (matches anything in that segment)
/// - Exact matching
fn matches_glob_pattern(path: &str, pattern: &str) -> bool {
    // Handle patterns like "dream-*.jsonl" -> check if path ends with the suffix after *
    if let Some(glob_pos) = pattern.find('*') {
        let prefix = &pattern[..glob_pos];
        let suffix = &pattern[glob_pos + 1..];
        // If pattern is "foo*", check path.starts_with("foo")
        // If pattern is "*.jsonl", check path.ends_with(".jsonl")
        // If pattern is "foo*bar", check path.contains("foo") and path.contains("bar")
        if suffix.is_empty() {
            path.starts_with(prefix)
        } else if prefix.is_empty() {
            path.ends_with(suffix)
        } else if suffix.contains('/') || prefix.contains('/') {
            // Complex case - need proper glob
            false
        } else {
            path.starts_with(prefix) && path.ends_with(suffix)
        }
    } else {
        path == pattern
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_rules() {
        let rules = QuarantineRules::default();
        assert!(!rules.quarantined_paths.is_empty());
        assert!(!rules.quarantined_text.is_empty());
    }

    #[test]
    fn test_quarantine_dream_cursor() {
        let rules = QuarantineRules::new();
        assert!(rules.is_quarantined_path("memory/.dream_cursor"));
    }

    #[test]
    fn test_quarantine_dream_prefix() {
        let rules = QuarantineRules::new();
        assert!(rules.is_quarantined_path("memory/.dream_state/file.json"));
    }

    #[test]
    fn test_quarantine_logs() {
        let rules = QuarantineRules::new();
        assert!(rules.is_quarantined_path("logs"));
        assert!(rules.is_quarantined_path("logs/app.log"));
    }

    #[test]
    fn test_quarantine_dream_jsonl() {
        let rules = QuarantineRules::new();
        // The default rules have "dream-*.jsonl" which should match "dream-123.jsonl"
        // Our simple glob matches prefix with *, so "dream-123.jsonl".starts_with("dream-") = true
        assert!(rules.is_quarantined_path("dream-123.jsonl"));
    }

    #[test]
    fn test_quarantine_text() {
        let rules = QuarantineRules::new();
        assert!(rules.is_quarantined_text("opensquilla-dream-promotion: yes"));
        assert!(rules.is_quarantined_text("This has a dream receipt"));
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("./src/lib.rs"), "src/lib.rs");
        assert_eq!(normalize_path("C:\\Users\\test"), "C:/Users/test");
    }

    #[test]
    fn test_matches_glob_pattern() {
        assert!(matches_glob_pattern("src/main.rs", "src/*"));
        assert!(matches_glob_pattern("src/lib.rs", "src/*"));
        assert!(!matches_glob_pattern("lib.rs", "src/*"));
        assert!(matches_glob_pattern("exact_match", "exact_match"));
    }
}
