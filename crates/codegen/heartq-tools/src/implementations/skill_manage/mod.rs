//! `skill_manage` tool — agent-managed CRUD for user skills.
//!
//! Wraps [`crate::skill_manager::skill_manage`] as a registry `Tool`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::output::ToolOutput;
use crate::types::tool::{ToolKind, ToolNamespace};

/// Registered wire name.
pub const SKILL_MANAGE_TOOL_NAME: &str = "skill_manage";

#[derive(Debug, Default)]
pub struct SkillManageImpl;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SkillManageInput {
    /// Action: `create`, `patch`, `delete`, `edit`, `write_file`, or `remove_file`.
    pub action: String,
    /// Skill directory name (kebab-case).
    pub name: String,
    /// Full SKILL.md content (required for create/edit).
    #[serde(default)]
    pub content: Option<String>,
    /// Find string for patch.
    #[serde(default)]
    pub old_string: Option<String>,
    /// Replacement string for patch.
    #[serde(default)]
    pub new_string: Option<String>,
    /// Relative path under the skill directory (write_file / remove_file).
    #[serde(default)]
    pub path: Option<String>,
}

impl crate::types::tool_metadata::ToolMetadata for SkillManageImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::Skill
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::HeartQBuild
    }

    fn description_template(&self) -> &str {
        "Create, update, or delete user skills under ~/.heartq/skills/. \
         Use `create` with full SKILL.md frontmatter+body, `patch` for find/replace, \
         `edit` to rewrite SKILL.md, `delete` to remove a skill, and \
         `write_file`/`remove_file` for skill assets. Prefer patch over edit for \
         small changes."
    }
}

impl xai_tool_runtime::Tool for SkillManageImpl {
    type Args = SkillManageInput;
    type Output = ToolOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(SKILL_MANAGE_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            SKILL_MANAGE_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(xai_tool_protocol::ToolScope::Write),
            ..Default::default()
        }
    }

    async fn run(
        &self,
        _ctx: xai_tool_runtime::ToolCallContext,
        input: SkillManageInput,
    ) -> Result<ToolOutput, xai_tool_runtime::ToolError> {
        let result = crate::skill_manager::skill_manage_ext(
            &input.action,
            &input.name,
            input.content.as_deref(),
            input.old_string.as_deref(),
            input.new_string.as_deref(),
            input.path.as_deref(),
        )
        .map_err(|e| {
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new(SKILL_MANAGE_TOOL_NAME).expect("valid"),
                e.to_string(),
            )
        })?;
        Ok(ToolOutput::Text(result.to_string().into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_id_matches_constant() {
        assert_eq!(
            xai_tool_runtime::Tool::id(&SkillManageImpl).to_string(),
            SKILL_MANAGE_TOOL_NAME
        );
    }
}
