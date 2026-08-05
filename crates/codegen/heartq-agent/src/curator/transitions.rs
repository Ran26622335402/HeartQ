//! Deterministic automatic transitions for the curator.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use super::state::TransitionStats;
use super::{CuratorConfig, ShouldRun};

/// Per-skill usage metadata. Stored as a sidecar JSON file at
/// `<skill_dir>/.usage.json` (skipped when absent — the skill is treated
/// as freshly used at creation time).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillUsage {
    /// Unix epoch seconds the skill was last invoked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<i64>,
    /// Number of times the skill has been invoked.
    #[serde(default)]
    pub use_count: u64,
    /// Explicit pin flag — when true, the curator never archives the skill.
    #[serde(default)]
    pub pinned: bool,
}

/// Walk every user skill directory and apply the three automatic
/// transitions based on `last_used_at` vs the configured thresholds.
///
/// - `mark_stale` — `last_used_at + stale_after_days < now`
/// - `archive` — `last_used_at + archive_after_days < now` and not pinned
/// - `reactivate` — currently marked stale but referenced since
///
/// Returns aggregate stats. When `cfg.dry_run` is `true`, no on-disk
/// changes are made; the returned `TransitionStats` reports what *would*
/// have happened.
pub fn apply_automatic_transitions(cfg: &CuratorConfig) -> TransitionStats {
    let mut stats = TransitionStats::default();
    let now = SystemTime::now();
    let now_epoch = system_time_to_epoch(now);

    let entries = match std::fs::read_dir(&cfg.skills_root) {
        Ok(e) => e,
        Err(_) => return stats, // no skills root → nothing to do
    };

    for entry in entries.flatten() {
        let category_path = entry.path();
        if !category_path.is_dir() {
            continue;
        }
        let Ok(skill_entries) = std::fs::read_dir(&category_path) else {
            continue;
        };
        for skill_entry in skill_entries.flatten() {
            let skill_path = skill_entry.path();
            if !skill_path.is_dir() {
                continue;
            }
            if !skill_path.join("SKILL.md").exists() {
                continue;
            }
            stats.checked += 1;
            process_one_skill(&skill_path, cfg, now_epoch, &mut stats);
        }
    }
    stats
}

fn process_one_skill(
    skill_path: &Path,
    cfg: &CuratorConfig,
    now_epoch: i64,
    stats: &mut TransitionStats,
) {
    let usage_path = skill_path.join(".usage.json");
    let mut usage = read_usage(&usage_path);
    let stale_threshold = (cfg.stale_after_days as i64) * 86_400;
    let archive_threshold = (cfg.archive_after_days as i64) * 86_400;

    // If we have no recorded usage yet, treat "now" as the last-used
    // moment so a freshly-installed skill doesn't immediately go stale.
    if usage.last_used_at.is_none() {
        usage.last_used_at = Some(now_epoch);
        // Don't persist the imputed timestamp in dry-run mode.
        if !cfg.dry_run {
            let _ = write_usage(&usage_path, &usage);
        }
        return;
    }

    let last_used = usage.last_used_at.unwrap_or(now_epoch);
    let idle_secs = now_epoch.saturating_sub(last_used);

    // ── archive ──
    if !usage.pinned && idle_secs >= archive_threshold {
        if !cfg.dry_run {
            if let Err(e) = archive_skill(skill_path) {
                tracing::warn!(
                    skill = %skill_path.display(),
                    error = %e,
                    "curator: failed to archive skill"
                );
                return;
            }
        }
        stats.archived += 1;
        return;
    }

    // ── mark stale ──
    let stale_flag = skill_path.join(".stale");
    let already_stale = stale_flag.exists();
    if idle_secs >= stale_threshold && !already_stale {
        if !cfg.dry_run {
            let _ = std::fs::write(&stale_flag, b"");
        }
        stats.marked_stale += 1;
    } else if idle_secs < stale_threshold && already_stale {
        // ── reactivate ──
        if !cfg.dry_run {
            let _ = std::fs::remove_file(&stale_flag);
        }
        stats.reactivated += 1;
    }
}

fn archive_skill(skill_path: &Path) -> std::io::Result<()> {
    let archive_root = skill_path.parent().and_then(Path::parent).map(|p| p.join(".archive"));
    let archive_root = archive_root.unwrap_or_else(|| skill_path.with_extension("archive"));
    std::fs::create_dir_all(&archive_root)?;
    let dest = archive_root.join(
        skill_path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("skill")),
    );
    std::fs::rename(skill_path, dest)
}

fn read_usage(path: &Path) -> SkillUsage {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_usage(path: &Path, usage: &SkillUsage) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(usage).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

fn system_time_to_epoch(t: SystemTime) -> i64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// One-shot deterministic curator pass. Loads/saves state at
/// `cfg.skills_root/.curator_state.json`, applies the automatic
/// transitions, and returns the run summary + stats.
///
/// This is the **public entry point** shell session-maintenance code
/// calls (e.g. on idle, on `/reset`, etc.).
pub fn run_curator_review(cfg: &CuratorConfig) -> RunReport {
    let mut stats = apply_automatic_transitions(cfg);

    let state_path = cfg.skills_root.join(".curator_state.json");
    let mut state = super::state::CuratorState::load(&state_path);

    if !cfg.dry_run {
        state.last_run_at = Some(SystemTime::now());
        state.run_count += 1;
        state.cumulative_marked_stale += stats.marked_stale as u64;
        state.cumulative_archived += stats.archived as u64;
        let summary = format!(
            "stale={} archived={} reactivated={}",
            stats.marked_stale, stats.archived, stats.reactivated
        );
        state.last_run_summary = Some(summary.clone());
        let _ = state.save(&state_path);
    } else {
        // dry-run still records a summary but doesn't bump counters
        stats.checked = stats.checked;
    }

    let summary = format!(
        "{} stale={} archived={} reactivated={} checked={}",
        if cfg.dry_run { "dry-run" } else { "auto" },
        stats.marked_stale,
        stats.archived,
        stats.reactivated,
        stats.checked,
    );

    RunReport { summary, stats, dry_run: cfg.dry_run }
}

/// Result of a single curator pass.
#[derive(Debug, Clone)]
pub struct RunReport {
    pub summary: String,
    pub stats: TransitionStats,
    pub dry_run: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_skill(root: &Path, category: &str, name: &str, last_used_epoch: Option<i64>) -> PathBuf {
        let dir = root.join(category).join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            "---\nname: x\ndescription: y\n---\nbody\n",
        )
        .unwrap();
        if let Some(epoch) = last_used_epoch {
            let usage = SkillUsage {
                last_used_at: Some(epoch),
                use_count: 1,
                pinned: false,
            };
            fs::write(
                dir.join(".usage.json"),
                serde_json::to_vec(&usage).unwrap(),
            )
            .unwrap();
        }
        dir
    }

    #[test]
    fn empty_root_is_a_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = CuratorConfig {
            skills_root: tmp.path().to_path_buf(),
            ..CuratorConfig::default()
        };
        let stats = apply_automatic_transitions(&cfg);
        assert_eq!(stats.checked, 0);
        assert!(stats.is_empty());
    }

    #[test]
    fn fresh_skill_is_imputed_now_and_not_stale() {
        let tmp = tempfile::tempdir().unwrap();
        make_skill(tmp.path(), "cat", "fresh", None);
        let cfg = CuratorConfig {
            skills_root: tmp.path().to_path_buf(),
            ..CuratorConfig::default()
        };
        let stats = apply_automatic_transitions(&cfg);
        assert_eq!(stats.checked, 1);
        assert!(stats.is_empty(), "fresh skill should not transition");
    }

    #[test]
    fn old_skill_is_marked_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let now = system_time_to_epoch(SystemTime::now());
        // 60 days ago — past the 30-day stale threshold.
        let old = now - 60 * 86_400;
        make_skill(tmp.path(), "cat", "old", Some(old));
        let cfg = CuratorConfig {
            skills_root: tmp.path().to_path_buf(),
            stale_after_days: 30,
            archive_after_days: 90,
            ..CuratorConfig::default()
        };
        let stats = apply_automatic_transitions(&cfg);
        assert_eq!(stats.marked_stale, 1);
        assert!(tmp.path().join("cat").join("old").join(".stale").exists());
    }

    #[test]
    fn ancient_skill_is_archived() {
        let tmp = tempfile::tempdir().unwrap();
        let now = system_time_to_epoch(SystemTime::now());
        let ancient = now - 200 * 86_400; // > 90d archive threshold
        make_skill(tmp.path(), "cat", "ancient", Some(ancient));
        let cfg = CuratorConfig {
            skills_root: tmp.path().to_path_buf(),
            stale_after_days: 30,
            archive_after_days: 90,
            ..CuratorConfig::default()
        };
        let stats = apply_automatic_transitions(&cfg);
        assert_eq!(stats.archived, 1);
        assert!(!tmp.path().join("cat").join("ancient").exists());
        assert!(tmp.path().join(".archive").join("ancient").exists());
    }

    #[test]
    fn pinned_skill_is_never_archived() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = make_skill(tmp.path(), "cat", "pinned", None);
        let now = system_time_to_epoch(SystemTime::now());
        let ancient = now - 365 * 86_400;
        fs::write(
            dir.join(".usage.json"),
            serde_json::to_vec(&SkillUsage {
                last_used_at: Some(ancient),
                use_count: 5,
                pinned: true,
            })
            .unwrap(),
        )
        .unwrap();
        let cfg = CuratorConfig {
            skills_root: tmp.path().to_path_buf(),
            archive_after_days: 90,
            ..CuratorConfig::default()
        };
        let stats = apply_automatic_transitions(&cfg);
        assert_eq!(stats.archived, 0, "pinned skill must survive");
        assert_eq!(stats.marked_stale, 1);
    }

    #[test]
    fn dry_run_does_not_mutate_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let now = system_time_to_epoch(SystemTime::now());
        let old = now - 60 * 86_400;
        let dir = make_skill(tmp.path(), "cat", "dry", Some(old));
        let cfg = CuratorConfig {
            skills_root: tmp.path().to_path_buf(),
            dry_run: true,
            ..CuratorConfig::default()
        };
        let stats = apply_automatic_transitions(&cfg);
        assert_eq!(stats.marked_stale, 1);
        assert!(!dir.join(".stale").exists(), "dry-run must not write");
    }

    #[test]
    fn run_curator_review_persists_state_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = CuratorConfig {
            enabled: true,
            skills_root: tmp.path().to_path_buf(),
            dry_run: false,
            ..CuratorConfig::default()
        };
        let report = run_curator_review(&cfg);
        assert!(!report.dry_run);
        let state_path = tmp.path().join(".curator_state.json");
        assert!(
            state_path.is_file(),
            "expected curator state at {}",
            state_path.display()
        );
        let state = super::super::state::CuratorState::load(&state_path);
        assert_eq!(state.run_count, 1);
        assert!(state.last_run_at.is_some());
        assert!(state.last_run_summary.is_some());
    }

    #[test]
    fn should_run_now_respects_interval() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = CuratorConfig {
            enabled: true,
            interval_hours: 24,
            ..CuratorConfig::default()
        };
        let cfg = CuratorConfig { skills_root: tmp.path().to_path_buf(), ..cfg };
        let mut state = super::super::state::CuratorState::default();
        // No prior run → should run.
        assert_eq!(
            cfg.should_run_now(&state, Some(7200.0), SystemTime::now()),
            super::ShouldRun::Yes
        );
        // Recent run → too soon.
        state.last_run_at = Some(SystemTime::now());
        assert_eq!(
            cfg.should_run_now(&state, Some(7200.0), SystemTime::now()),
            super::ShouldRun::TooSoon
        );
        // Disabled config → disabled.
        let mut cfg = cfg;
        cfg.enabled = false;
        assert_eq!(
            cfg.should_run_now(&state, Some(7200.0), SystemTime::now()),
            super::ShouldRun::Disabled
        );
        // Idle but no measurement → ignored (returns Yes).
        cfg.enabled = true;
        assert_eq!(
            cfg.should_run_now(&state, None, SystemTime::now()),
            super::ShouldRun::TooSoon
        );
    }
}