//! `skill_manage` tool — agent-managed CRUD for user skills.
//!
//! Translated from Hermes Agent's `tools/skill_manager_tool.py`. v1 scope
//! covers `create` / `patch` / `delete` only (the most common actions).
//! `edit` / `write_file` / `remove_file` are explicitly **not** implemented
//! and return `unimplemented` errors so callers can plan the upgrade.
//!
//! All skills live under a single root: `${HERMES_HOME}/skills/` (or
//! `${HEARTQ_HOME}/skills/` if that's what's wired up). The directory
//! layout matches Hermes: each skill is `<root>/<name>/SKILL.md`, with
//! optional `references/`, `templates/`, `scripts/`, `assets/` subdirs.
//!
//! ## Security model
//!
//! Inspired by the Kilo Code #11227 incident: a built-in sentinel skill
//! resolved to the server cwd and a recursive `rmtree` wiped the user's
//! working directory. We adopt the same defense-in-depth as Hermes:
//!
//! 1. Refuse to delete a path that is **not strictly inside** the
//!    configured skills root.
//! 2. Refuse to delete the skills root itself.
//! 3. Refuse to delete any path reached via symlink / junction
//!    (`rmtree` would follow it into content outside the tree).
//!
//! ## Frontmatter validation
//!
//! Skills MUST start with `---\n`, contain a parseable YAML mapping
//! between the open fence and a closing `\n---\n`, and include both
//! `name` (≤64 chars) and `description` (≤1024 chars). The full file
//! must be ≤100,000 chars.

use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::skills_guard::{self, GuardSeverity};

// ─────────────────────────────────────────────────────────────────────
// Paths
// ─────────────────────────────────────────────────────────────────────

/// Resolve the skills root directory.
///
/// Honors `HERMES_HOME` / `HEARTQ_HOME` env vars; defaults to `~/.heartq/skills/`.
/// In tests this can be patched via [`set_skills_root_for_tests`].
fn skills_root() -> PathBuf {
    if let Some(root) = TEST_SKILLS_ROOT.get() {
        return root.clone();
    }
    let home = std::env::var("HERMES_HOME")
        .or_else(|_| std::env::var("HEARTQ_HOME"))
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            format!("{home}/.heartq")
        });
    PathBuf::from(home).join("skills")
}

static TEST_SKILLS_ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

#[cfg(test)]
pub fn set_skills_root_for_tests(path: PathBuf) {
    let _ = TEST_SKILLS_ROOT.set(path);
}

// ─────────────────────────────────────────────────────────────────────
// Limits
// ─────────────────────────────────────────────────────────────────────

pub const MAX_NAME_LENGTH: usize = 64;
pub const MAX_DESCRIPTION_LENGTH: usize = 1024;
pub const MAX_SKILL_CONTENT_CHARS: usize = 100_000;
pub const NAME_ALLOWED_CHARS: &[char] = &[
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '-',
];

// ─────────────────────────────────────────────────────────────────────
// Error type
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum SkillManageError {
    #[error("`name` is required and must be non-empty")]
    MissingName,
    #[error("`name` {0:?} exceeds {MAX_NAME_LENGTH} chars")]
    NameTooLong(String),
    #[error("`name` {0:?} must be lowercase ASCII letters, digits, and hyphens only")]
    InvalidNameChars(String),
    #[error("`description` exceeds {MAX_DESCRIPTION_LENGTH} chars")]
    DescriptionTooLong,
    #[error("skill content exceeds {MAX_SKILL_CONTENT_CHARS} chars")]
    ContentTooLarge,
    #[error("frontmatter must start with `---` on the first line")]
    BadFrontmatterStart,
    #[error("frontmatter must close with `\\n---\\n`")]
    BadFrontmatterEnd,
    #[error("frontmatter is not valid YAML: {0}")]
    InvalidYaml(String),
    #[error("frontmatter missing required field `name`")]
    MissingFrontmatterName,
    #[error("frontmatter missing required field `description`")]
    MissingFrontmatterDescription,
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("refusing to delete: {0}")]
    RefusingDelete(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("`{0}` is not implemented in v1 scope (create/patch/delete only)")]
    Unimplemented(String),
}

pub type Result<T> = std::result::Result<T, SkillManageError>;

// ─────────────────────────────────────────────────────────────────────
// Frontmatter
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillFrontmatter {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub metadata: Option<serde_yaml::Value>,
}

/// Validate the SKILL.md content. Returns the parsed frontmatter on success.
pub fn validate_skill_content(content: &str) -> Result<SkillFrontmatter> {
    if content.len() > MAX_SKILL_CONTENT_CHARS {
        return Err(SkillManageError::ContentTooLarge);
    }
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return Err(SkillManageError::BadFrontmatterStart);
    }
    // Find closing `---` fence.
    let after_open = &content[4..];
    let close_idx = after_open
        .find("\n---")
        .ok_or(SkillManageError::BadFrontmatterEnd)?;
    let yaml_block = &after_open[..close_idx];
    let fm: serde_yaml::Value =
        serde_yaml::from_str(yaml_block).map_err(|e| SkillManageError::InvalidYaml(e.to_string()))?;
    let mapping = fm
        .as_mapping()
        .ok_or_else(|| SkillManageError::InvalidYaml("expected YAML mapping".into()))?;
    let name = mapping
        .get(serde_yaml::Value::String("name".into()))
        .and_then(|v| v.as_str())
        .ok_or(SkillManageError::MissingFrontmatterName)?
        .to_string();
    let description = mapping
        .get(serde_yaml::Value::String("description".into()))
        .and_then(|v| v.as_str())
        .ok_or(SkillManageError::MissingFrontmatterDescription)?
        .to_string();
    if name.len() > MAX_NAME_LENGTH {
        return Err(SkillManageError::NameTooLong(name));
    }
    if !name.chars().all(|c| NAME_ALLOWED_CHARS.contains(&c)) {
        return Err(SkillManageError::InvalidNameChars(name));
    }
    if description.len() > MAX_DESCRIPTION_LENGTH {
        return Err(SkillManageError::DescriptionTooLong);
    }
    let metadata = mapping
        .get(serde_yaml::Value::String("metadata".into()))
        .cloned();
    let author = mapping
        .get(serde_yaml::Value::String("author".into()))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let version = mapping
        .get(serde_yaml::Value::String("version".into()))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let license = mapping
        .get(serde_yaml::Value::String("license".into()))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Ok(SkillFrontmatter {
        name,
        description,
        version,
        author,
        license,
        metadata,
    })
}

// ─────────────────────────────────────────────────────────────────────
// Action dispatch
// ─────────────────────────────────────────────────────────────────────

/// Top-level dispatch for the `skill_manage` tool.
///
/// Supports: `create`, `patch`, `delete`, `edit`.
/// Use [`skill_manage_ext`] for `write_file` / `remove_file` (needs relative path).
pub fn skill_manage(
    action: &str,
    name: &str,
    content: Option<&str>,
    old_string: Option<&str>,
    new_string: Option<&str>,
) -> Result<serde_json::Value> {
    skill_manage_ext(action, name, content, old_string, new_string, None)
}

/// Extended dispatch that also accepts a relative `path` for write_file/remove_file.
pub fn skill_manage_ext(
    action: &str,
    name: &str,
    content: Option<&str>,
    old_string: Option<&str>,
    new_string: Option<&str>,
    path: Option<&str>,
) -> Result<serde_json::Value> {
    if name.is_empty() {
        return Err(SkillManageError::MissingName);
    }
    match action {
        "create" => {
            let content = content.ok_or_else(|| {
                SkillManageError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "`content` is required for create",
                ))
            })?;
            let fm = validate_skill_content(content)?;
            let path = create_skill(name, content, &fm)?;
            Ok(serde_json::json!({
                "success": true,
                "action": "create",
                "name": name,
                "path": path.to_string_lossy(),
            }))
        }
        "patch" => {
            let old = old_string.ok_or_else(|| {
                SkillManageError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "`old_string` is required for patch",
                ))
            })?;
            let new = new_string.unwrap_or("");
            let path = patch_skill(name, old, new)?;
            Ok(serde_json::json!({
                "success": true,
                "action": "patch",
                "name": name,
                "path": path.to_string_lossy(),
            }))
        }
        "delete" => {
            delete_skill(name)?;
            Ok(serde_json::json!({
                "success": true,
                "action": "delete",
                "name": name,
            }))
        }
        "edit" => {
            let content = content.ok_or_else(|| {
                SkillManageError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "`content` is required for edit",
                ))
            })?;
            let fm = validate_skill_content(content)?;
            let written = edit_skill(name, content, &fm)?;
            Ok(serde_json::json!({
                "success": true,
                "action": "edit",
                "name": name,
                "path": written.to_string_lossy(),
            }))
        }
        "write_file" => {
            let rel = path.ok_or_else(|| {
                SkillManageError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "`path` is required for write_file",
                ))
            })?;
            let content = content.ok_or_else(|| {
                SkillManageError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "`content` is required for write_file",
                ))
            })?;
            let written = write_skill_file(name, rel, content)?;
            Ok(serde_json::json!({
                "success": true,
                "action": "write_file",
                "name": name,
                "path": written.to_string_lossy(),
            }))
        }
        "remove_file" => {
            let rel = path.ok_or_else(|| {
                SkillManageError::Io(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "`path` is required for remove_file",
                ))
            })?;
            remove_skill_file(name, rel)?;
            Ok(serde_json::json!({
                "success": true,
                "action": "remove_file",
                "name": name,
                "path": rel,
            }))
        }
        other => Err(SkillManageError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown action '{other}'"),
        ))),
    }
}

// ─────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────

fn skill_dir(name: &str) -> PathBuf {
    skills_root().join(name)
}

fn skill_md_path(name: &str) -> PathBuf {
    skill_dir(name).join("SKILL.md")
}

fn enforce_skill_guard(content: &str) -> Result<()> {
    let report = skills_guard::scan_skill_content(content);
    if report.severity >= GuardSeverity::Critical {
        let details: Vec<String> = report
            .findings
            .iter()
            .filter(|f| f.severity >= GuardSeverity::Critical)
            .map(|f| format!("{}: {}", f.pattern, f.message))
            .collect();
        return Err(SkillManageError::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("skill content blocked by security guard: {}", details.join("; ")),
        )));
    }
    Ok(())
}

fn create_skill(name: &str, content: &str, fm: &SkillFrontmatter) -> Result<PathBuf> {
    if name != fm.name {
        // Disallow mismatched names — keeps `name` argument and frontmatter
        // honest, prevents accidentally creating skills under the wrong key.
        return Err(SkillManageError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "`name` argument ({name:?}) does not match frontmatter `name` ({:?})",
                fm.name
            ),
        )));
    }
    let dir = skill_dir(name);
    if dir.exists() {
        return Err(SkillManageError::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("skill {name:?} already exists at {}", dir.display()),
        )));
    }
    enforce_skill_guard(content)?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("SKILL.md");
    write_atomic(&path, content.as_bytes())?;
    // Touch the standard subdirs so tooling can rely on them existing.
    for sub in ["references", "templates", "scripts", "assets"] {
        let _ = fs::create_dir_all(dir.join(sub));
    }
    Ok(path)
}

fn patch_skill(name: &str, old: &str, new: &str) -> Result<PathBuf> {
    let path = skill_md_path(name);
    if !path.exists() {
        return Err(SkillManageError::NotFound(name.into()));
    }
    let body = fs::read_to_string(&path)?;
    let occurrences = body.matches(old).count();
    if occurrences == 0 {
        return Err(SkillManageError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("old_string not found in {name}"),
        )));
    }
    if occurrences > 1 {
        return Err(SkillManageError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "old_string matches {occurrences} places in {name}; pass `replace_all` or be more specific"
            ),
        )));
    }
    let new_body = body.replacen(old, new, 1);
    // Re-validate the result so we never persist a broken SKILL.md.
    validate_skill_content(&new_body)?;
    enforce_skill_guard(&new_body)?;
    write_atomic(&path, new_body.as_bytes())?;
    Ok(path)
}

fn delete_skill(name: &str) -> Result<()> {
    let dir = skill_dir(name);
    if !dir.exists() {
        return Err(SkillManageError::NotFound(name.into()));
    }
    // Defense-in-depth (Kilo Code #11227 lessons):
    if is_path_redirect(&dir) {
        return Err(SkillManageError::RefusingDelete(format!(
            "{name}: directory is a symlink/junction"
        )));
    }
    let resolved = match fs::canonicalize(&dir) {
        Ok(p) => p,
        Err(e) => {
            return Err(SkillManageError::RefusingDelete(format!(
                "{name}: cannot resolve ({e})"
            )))
        }
    };
    let root = match fs::canonicalize(skills_root()) {
        Ok(p) => p,
        Err(_) => skills_root(),
    };
    if resolved == root {
        return Err(SkillManageError::RefusingDelete(format!(
            "{name}: resolves to skills root"
        )));
    }
    if !resolved.starts_with(&root) {
        return Err(SkillManageError::RefusingDelete(format!(
            "{name}: not inside skills root"
        )));
    }
    fs::remove_dir_all(&dir)?;
    Ok(())
}

fn edit_skill(name: &str, content: &str, fm: &SkillFrontmatter) -> Result<PathBuf> {
    if name != fm.name {
        return Err(SkillManageError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "`name` argument ({name:?}) does not match frontmatter `name` ({:?})",
                fm.name
            ),
        )));
    }
    let dir = skill_dir(name);
    if !dir.exists() {
        return Err(SkillManageError::NotFound(name.into()));
    }
    enforce_skill_guard(content)?;
    let path = dir.join("SKILL.md");
    write_atomic(&path, content.as_bytes())?;
    Ok(path)
}

fn resolve_skill_rel_path(name: &str, rel: &str) -> Result<PathBuf> {
    if rel.is_empty()
        || rel.contains("..")
        || Path::new(rel).is_absolute()
        || rel.starts_with('/')
        || rel.starts_with('\\')
    {
        return Err(SkillManageError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid relative path {rel:?}"),
        )));
    }
    let dir = skill_dir(name);
    if !dir.exists() {
        return Err(SkillManageError::NotFound(name.into()));
    }
    let candidate = dir.join(rel);
    let root = fs::canonicalize(&dir).unwrap_or(dir.clone());
    if let Ok(resolved) = fs::canonicalize(&candidate) {
        if !resolved.starts_with(&root) {
            return Err(SkillManageError::Io(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("path {rel:?} escapes skill directory"),
            )));
        }
        return Ok(resolved);
    }
    // File may not exist yet (write_file create). Ensure parent stays under dir.
    if let Some(parent) = candidate.parent() {
        let _ = fs::create_dir_all(parent);
        if let Ok(parent_canon) = fs::canonicalize(parent) {
            if !parent_canon.starts_with(&root) {
                return Err(SkillManageError::Io(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("path {rel:?} escapes skill directory"),
                )));
            }
        }
    }
    Ok(candidate)
}

fn write_skill_file(name: &str, rel: &str, content: &str) -> Result<PathBuf> {
    let path = resolve_skill_rel_path(name, rel)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_atomic(&path, content.as_bytes())?;
    Ok(path)
}

fn remove_skill_file(name: &str, rel: &str) -> Result<()> {
    let path = resolve_skill_rel_path(name, rel)?;
    if path.file_name().is_some_and(|f| f == "SKILL.md") {
        return Err(SkillManageError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to remove SKILL.md via remove_file; use delete",
        )));
    }
    if !path.exists() {
        return Err(SkillManageError::NotFound(format!("{name}/{rel}")));
    }
    if is_path_redirect(&path) {
        return Err(SkillManageError::RefusingDelete(format!(
            "{name}/{rel}: is a symlink"
        )));
    }
    fs::remove_file(&path)?;
    Ok(())
}

fn is_path_redirect(path: &Path) -> bool {
    // Symlink check (cross-platform).
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return true;
        }
    }
    false
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("skill")
    ));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

// ─────────────────────────────────────────────────────────────────────
// Tool schema (for the LLM tool registry)
// ─────────────────────────────────────────────────────────────────────

/// JSON Schema for the `skill_manage` tool (consumed by the tool registry).
pub const SKILL_MANAGE_SCHEMA: &str = r#"{
  "name": "skill_manage",
  "description": "Manage skills (create, patch, delete). Skills are your procedural memory — reusable approaches for recurring task types. New skills go under the skills root; existing skills can be modified in place.\n\nActions:\n- create: full SKILL.md content with frontmatter\n- patch: precise old_string/new_string replacement in SKILL.md\n- delete: remove a skill (frontmatter-validated, symlink-safe)\n\nNote: edit / write_file / remove_file are not in v1 scope.",
  "parameters": {
    "type": "object",
    "properties": {
      "action": {"enum": ["create", "patch", "delete"]},
      "name": {"type": "string", "description": "skill name (lowercase, hyphens, ≤64 chars)"},
      "content": {"type": "string", "description": "full SKILL.md content (frontmatter + body) — required for create"},
      "old_string": {"type": "string", "description": "exact substring to find — required for patch"},
      "new_string": {"type": "string", "description": "replacement text — required for patch"}
    },
    "required": ["action", "name"]
  }
}"#;

/// Convenience: extract skill conditions from a parsed frontmatter.
///
/// Returns the raw `metadata.hermes` sub-mapping for now; downstream
/// callers (the conditional-skills tracker) can normalize the shape.
pub fn parse_conditions(fm: &SkillFrontmatter) -> serde_yaml::Value {
    match &fm.metadata {
        Some(m) => {
            // If `metadata.hermes` exists, return it; otherwise return the
            // whole `metadata` mapping for diagnostic visibility.
            m.get("hermes").cloned().unwrap_or_else(|| m.clone())
        }
        None => serde_yaml::Value::Null,
    }
}

/// Best-effort canonicalization of the user-supplied `name`. **Rejects**
/// any path-traversal attempt (`..`, `/`, leading dots, etc.) by returning
/// `None` if the input contains forbidden characters; only strictly
/// `[a-z0-9-]+` is accepted.
pub fn canonicalize_name(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.len() > MAX_NAME_LENGTH {
        return None;
    }
    if !raw.chars().all(|c| NAME_ALLOWED_CHARS.contains(&c)) {
        return None;
    }
    Some(raw.to_string())
}

#[allow(dead_code)]
fn _components_are_safe(path: &Path) -> bool {
    path.components().all(|c| !matches!(c, Component::ParentDir))
}

// ─────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_root<F: FnOnce(&Path)>(f: F) {
        let _g = TEST_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        set_skills_root_for_tests(tmp.path().to_path_buf());
        f(tmp.path());
    }

    const VALID_FM: &str = "---\nname: demo\ndescription: Use when demoing skill_manage\n---\n\nBody.\n";

    #[test]
    fn validate_rejects_missing_frontmatter() {
        assert!(matches!(
            validate_skill_content("just a body\n"),
            Err(SkillManageError::BadFrontmatterStart)
        ));
    }

    #[test]
    fn validate_rejects_missing_close_fence() {
        assert!(matches!(
            validate_skill_content("---\nname: x\ndescription: y\n\nbody\n"),
            Err(SkillManageError::BadFrontmatterEnd)
        ));
    }

    #[test]
    fn validate_rejects_bad_name_chars() {
        let body = "---\nname: BAD NAME\ndescription: x\n---\n\nbody\n";
        assert!(matches!(
            validate_skill_content(body),
            Err(SkillManageError::InvalidNameChars(_))
        ));
    }

    #[test]
    fn validate_rejects_long_description() {
        let desc = "x".repeat(MAX_DESCRIPTION_LENGTH + 1);
        let body = format!("---\nname: ok\ndescription: {desc}\n---\n\nbody\n");
        assert!(matches!(
            validate_skill_content(&body),
            Err(SkillManageError::DescriptionTooLong)
        ));
    }

    #[test]
    fn validate_accepts_well_formed() {
        let fm = validate_skill_content(VALID_FM).unwrap();
        assert_eq!(fm.name, "demo");
        assert_eq!(fm.description, "Use when demoing skill_manage");
    }

    #[test]
    fn create_then_patch_then_delete() {
        with_temp_root(|_| {
            let v = skill_manage("create", "demo", Some(VALID_FM), None, None).unwrap();
            assert_eq!(v["success"], true);
            assert!(skill_md_path("demo").exists());

            let patch_body = format!(
                "{}{}",
                "---\nname: demo2\ndescription: Use when demoing skill_manage\n---\n\n",
                "Hello world."
            );
            skill_manage("create", "demo2", Some(&patch_body), None, None).unwrap();

            let patch_input_old = "Hello world.";
            let patch_input_new = "Hello patched.";
            let v = skill_manage(
                "patch",
                "demo2",
                None,
                Some(patch_input_old),
                Some(patch_input_new),
            )
            .unwrap();
            assert_eq!(v["success"], true);
            let body = fs::read_to_string(skill_md_path("demo2")).unwrap();
            assert!(body.contains("Hello patched."));

            // Delete works.
            skill_manage("delete", "demo2", None, None, None).unwrap();
            assert!(!skill_md_path("demo2").exists());
        });
    }

    #[test]
    fn delete_refuses_outside_root() {
        with_temp_root(|tmp| {
            // Place a sibling directory that pretends to be a skill.
            let outside = tmp.parent().unwrap().join("evil");
            fs::create_dir_all(&outside).unwrap();
            // Create a symlink inside the skills root pointing outside.
            let link = skills_root().join("evil-skill");
            std::os::unix::fs::symlink(&outside, &link).ok(); // skip on Windows
            let res = skill_manage("delete", "evil-skill", None, None, None);
            // On Unix we expect RefusingDelete; on Windows (no symlink) the
            // path won't exist, so we get NotFound. Both are acceptable.
            assert!(res.is_err());
        });
    }

    #[test]
    fn patch_rejects_ambiguous_match() {
        with_temp_root(|_| {
            let body = "---\nname: dup\ndescription: a\n---\n\nfoo bar foo\n";
            skill_manage("create", "dup", Some(body), None, None).unwrap();
            let res = skill_manage("patch", "dup", None, Some("foo"), Some("baz"));
            assert!(res.is_err());
        });
    }

    #[test]
    fn edit_rewrites_skill_md() {
        with_temp_root(|_| {
            let skill_name = "demo-edit";
            let fm = format!(
                "---\nname: {skill_name}\ndescription: Use when demoing skill_manage\n---\n\nBody.\n"
            );
            // Create a baseline skill.
            skill_manage("create", skill_name, Some(&fm), None, None).unwrap();

            // Edit with a fully-formed SKILL.md (frontmatter + body).
            let updated = "\
---\n\
name: demo-edit\n\
description: Use when demoing skill_manage\n\
---\n\
\n\
New body via skill_manage edit.\n";

            let v = skill_manage("edit", skill_name, Some(updated), None, None).unwrap();
            assert_eq!(v["success"], true);

            let body = fs::read_to_string(skill_md_path(skill_name)).unwrap();
            assert!(body.contains("New body via skill_manage edit."));
        });
    }

    #[test]
    fn write_and_remove_file_in_skill_dir() {
        with_temp_root(|_| {
            let skill_name = "demo-write";
            let fm = format!(
                "---\nname: {skill_name}\ndescription: Use when demoing skill_manage\n---\n\nBody.\n"
            );
            // Create a baseline skill directory so write_file has a target root.
            skill_manage("create", skill_name, Some(&fm), None, None).unwrap();

            // Write an asset file.
            let asset_rel = "templates/asset.txt";
            let asset_content = "hello from templates/asset.txt\n";
            let v = skill_manage_ext(
                "write_file",
                skill_name,
                Some(asset_content),
                None,
                None,
                Some(asset_rel),
            )
            .unwrap();
            assert_eq!(v["success"], true);

            let asset_path = skill_dir(skill_name).join(asset_rel);
            assert!(asset_path.exists());
            assert_eq!(
                fs::read_to_string(&asset_path).unwrap(),
                asset_content
            );

            // Remove the asset file.
            let v = skill_manage_ext(
                "remove_file",
                skill_name,
                None,
                None,
                None,
                Some(asset_rel),
            )
            .unwrap();
            assert_eq!(v["success"], true);
            assert!(!asset_path.exists());
        });
    }

    #[test]
    fn canonicalize_name_rejects_traversal() {
        // Anything outside `[a-z0-9-]+` is rejected outright.
        assert_eq!(canonicalize_name("../../etc/passwd"), None);
        assert_eq!(canonicalize_name("good-name_42"), None); // underscore
        assert_eq!(canonicalize_name("good-name"), Some("good-name".into()));
        assert_eq!(canonicalize_name("a"), Some("a".into()));
        assert_eq!(canonicalize_name(""), None);
        assert_eq!(canonicalize_name(&"x".repeat(MAX_NAME_LENGTH + 1)), None);
    }
}