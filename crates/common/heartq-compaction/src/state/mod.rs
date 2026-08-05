//! Compression state module for tracking anti-jitter behavior.
//!
//! This module implements the CompressionState struct which tracks
//! ineffective compression attempts and blocks further compression
//! after exceeding a threshold to prevent thrashing.

use serde::{Deserialize, Serialize};

/// Compression state for anti-jitter tracking.
///
/// Tracks consecutive ineffective compression attempts and blocks
/// further compression after exceeding a threshold. This prevents
/// the system from repeatedly attempting compression that doesn't
/// reduce context size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionState {
    /// Number of consecutive ineffective compressions
    pub ineffective_compression_count: usize,

    /// Maximum ineffective compressions before blocking
    #[serde(default = "default_max_ineffective")]
    pub max_ineffective: usize,

    /// Whether compression is currently blocked
    #[serde(default)]
    is_blocked: bool,

    /// Total compressions attempted
    #[serde(default)]
    total_attempts: usize,

    /// Successful compressions
    #[serde(default)]
    successful_compressions: usize,
}

fn default_max_ineffective() -> usize {
    3
}

impl Default for CompressionState {
    fn default() -> Self {
        Self::new(3)
    }
}

impl CompressionState {
    /// Create a new compression state with the specified threshold.
    pub fn new(max_ineffective: usize) -> Self {
        Self {
            max_ineffective,
            ineffective_compression_count: 0,
            is_blocked: false,
            total_attempts: 0,
            successful_compressions: 0,
        }
    }

    /// Check if compression should be blocked due to ineffective attempts.
    pub fn is_blocked(&self) -> bool {
        self.ineffective_compression_count >= self.max_ineffective || self.is_blocked
    }

    /// Record a compression attempt that didn't reduce context.
    pub fn record_ineffective(&mut self) {
        self.total_attempts += 1;
        self.ineffective_compression_count += 1;
    }

    /// Record successful compression.
    pub fn record_successful(&mut self) {
        self.total_attempts += 1;
        self.successful_compressions += 1;
        self.ineffective_compression_count = 0;
        self.is_blocked = false;
    }

    /// Reset state (e.g., on new conversation turn).
    pub fn reset(&mut self) {
        self.ineffective_compression_count = 0;
        self.is_blocked = false;
    }

    /// Force block compression (e.g., after fallback was used).
    pub fn force_block(&mut self) {
        self.is_blocked = true;
    }

    /// Get the success rate of compressions.
    pub fn success_rate(&self) -> f64 {
        if self.total_attempts == 0 {
            return 1.0;
        }
        self.successful_compressions as f64 / self.total_attempts as f64
    }

    /// Get the current compression effectiveness ratio.
    pub fn effectiveness_ratio(&self) -> f64 {
        let remaining = self.max_ineffective.saturating_sub(self.ineffective_compression_count);
        remaining as f64 / self.max_ineffective as f64
    }

    /// Check if we should attempt compression based on current state.
    pub fn should_attempt(&self) -> bool {
        !self.is_blocked()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_new() {
        let state = CompressionState::new(3);
        assert!(!state.is_blocked());
        assert_eq!(state.ineffective_compression_count, 0);
        assert_eq!(state.max_ineffective, 3);
    }

    #[test]
    fn test_state_default() {
        let state = CompressionState::default();
        assert_eq!(state.max_ineffective, 3);
        assert!(!state.is_blocked());
    }

    #[test]
    fn test_record_ineffective() {
        let mut state = CompressionState::new(2);

        state.record_ineffective();
        assert_eq!(state.ineffective_compression_count, 1);
        assert!(!state.is_blocked());

        state.record_ineffective();
        assert_eq!(state.ineffective_compression_count, 2);
        assert!(state.is_blocked());
    }

    #[test]
    fn test_record_successful() {
        let mut state = CompressionState::new(2);

        state.record_ineffective();
        state.record_ineffective();
        assert!(state.is_blocked());

        state.record_successful();
        assert!(!state.is_blocked());
        assert_eq!(state.ineffective_compression_count, 0);
        assert_eq!(state.successful_compressions, 1);
    }

    #[test]
    fn test_reset() {
        let mut state = CompressionState::new(1);

        state.record_ineffective();
        assert!(state.is_blocked());

        state.reset();
        assert!(!state.is_blocked());
        assert_eq!(state.ineffective_compression_count, 0);
    }

    #[test]
    fn test_force_block() {
        let mut state = CompressionState::new(10);

        assert!(!state.is_blocked());

        state.force_block();
        assert!(state.is_blocked());
    }

    #[test]
    fn test_should_attempt() {
        let state = CompressionState::new(3);
        assert!(state.should_attempt());

        let mut blocked_state = CompressionState::new(1);
        blocked_state.record_ineffective();
        assert!(!blocked_state.should_attempt());
    }

    #[test]
    fn test_success_rate() {
        let mut state = CompressionState::new(3);

        // No attempts yet
        assert_eq!(state.success_rate(), 1.0);

        // Record some successful compressions
        state.record_successful();
        state.record_successful();
        assert_eq!(state.success_rate(), 1.0);

        // Record an ineffective one
        state.record_ineffective();
        assert_eq!(state.success_rate(), 2.0 / 3.0);
    }

    #[test]
    fn test_effectiveness_ratio() {
        let state = CompressionState::new(3);
        assert_eq!(state.effectiveness_ratio(), 1.0);

        let mut state = CompressionState::new(3);
        state.record_ineffective();
        assert_eq!(state.effectiveness_ratio(), 2.0 / 3.0);
    }

    #[test]
    fn test_serialization() {
        let state = CompressionState::new(5);
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: CompressionState = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.max_ineffective, 5);
        assert!(!deserialized.is_blocked());
    }
}
