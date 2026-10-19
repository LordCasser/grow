//! Load an explicitly selected skill for the current model call.

use crate::implementations::skills::skill::{
    SubstitutionContext, prepare_skill_content, prepare_skill_content_with_fs,
};
use crate::types::output::{DynamicOutput, ToolOutput};
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::resources::{FileSystem, GitignoreFilter, OwnerSessionId, RespectGitignore};
use crate::types::skill_discovery_tracker::SkillManager;
use crate::types::tool::{ToolKind, ToolNamespace};
use crate::types::tool_metadata::ToolMetadata;
use serde::{Deserialize, Serialize};

pub const LOAD_SKILL_TOOL_NAME: &str = "load_skill";

/// Host-selected skill identity passed through the tool context. This is not
/// part of the model-visible input schema.
#[derive(Debug, Clone)]
pub struct SkillLoadSelection {
    pub skill: crate::implementations::skills::types::SkillInfo,
    pub user_invocation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoadSkillInput {
    /// Exact callable name from the skill directory.
    pub name: String,
    /// Arguments passed to the selected skill. Defaults to an empty string.
    #[serde(default)]
    #[schemars(default)]
    pub args: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LoadSkillOutput {
    pub skill_name: String,
    /// Actual source path selected by SkillManager.
    pub source: String,
    pub args: String,
    pub prepared_body: String,
}

impl tool_runtime::ToolOutput for LoadSkillOutput {}

impl From<LoadSkillInput> for crate::types::tool_io::ToolInput {
    fn from(input: LoadSkillInput) -> Self {
        Self::Dynamic(serde_json::to_value(input).expect("skill input is serializable"))
    }
}

impl From<LoadSkillOutput> for ToolOutput {
    fn from(output: LoadSkillOutput) -> Self {
        let value = serde_json::to_value(output).expect("skill output is serializable");
        ToolOutput::Dynamic(DynamicOutput { value })
    }
}

#[derive(Debug, Default)]
pub struct LoadSkillTool;

impl ToolMetadata for LoadSkillTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Skill
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        "Load a skill from the current skill directory. Provide its exact advertised name and optional arguments. The returned body is skill guidance; follow its instructions through the ordinary tool authorization rules."
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl tool_runtime::Tool for LoadSkillTool {
    type Args = LoadSkillInput;
    type Output = LoadSkillOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(LOAD_SKILL_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            LOAD_SKILL_TOOL_NAME,
            ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            max_access: tool_protocol::ToolAccess::Read,
            ..Default::default()
        }
    }

    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: LoadSkillInput,
    ) -> Result<LoadSkillOutput, tool_runtime::ToolError> {
        let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
        let selection = ctx
            .get::<SkillLoadSelection>()
            .map(|selection| (*selection).clone());
        let (skill, fs, owner_session_id, ignored) = {
            let resources = resources.lock().await;
            let manager = resources.require::<SkillManager>()?;
            let user_invocation = selection
                .as_ref()
                .is_some_and(|selection| selection.user_invocation);
            let current_skill = manager
                .resolve_skill(&input.name, user_invocation)
                .map_err(tool_runtime::ToolError::invalid_arguments)?;
            let skill = if let Some(selection) = selection.as_ref() {
                if !same_skill_identity(&current_skill, &selection.skill) {
                    return Err(tool_runtime::ToolError::invalid_arguments(
                        "The selected skill identity changed before it could be loaded",
                    ));
                }
                selection.skill.clone()
            } else {
                current_skill
            };
            let fs = if skill.body.is_none() && !skill.path.contains("://") {
                Some(resources.require::<FileSystem>()?.0.clone())
            } else {
                None
            };
            let ignored = fs.is_some()
                && resources
                    .get::<RespectGitignore>()
                    .is_some_and(|filter| filter.0)
                && resources
                    .get::<GitignoreFilter>()
                    .is_some_and(|filter| filter.is_ignored(std::path::Path::new(&skill.path)));
            let owner = resources
                .get::<OwnerSessionId>()
                .map(|session| session.0.clone());
            (skill, fs, owner, ignored)
        };
        if ignored {
            return Err(tool_runtime::ToolError::custom(
                "skill_load_failed",
                format!("Skill source '{}' is ignored by .gitignore", skill.path),
            ));
        }
        let cancellation = ctx
            .get::<tool_runtime::Cancellation>()
            .map_or_else(tokio_util::sync::CancellationToken::new, |token| {
                token.0.clone()
            });
        let skill_dir = std::path::Path::new(&skill.path)
            .parent()
            .map(|path| path.to_string_lossy().into_owned());
        let substitution_context = SubstitutionContext {
            skill_dir: skill_dir.as_deref(),
            session_id: owner_session_id.as_deref(),
            plugin_root: skill.plugin_root.as_deref(),
            plugin_data: skill.plugin_data.as_deref(),
        };
        let prepared = match fs {
            Some(fs) => {
                prepare_skill_content_with_fs(
                    &skill,
                    fs.as_ref(),
                    Some(&input.args),
                    &substitution_context,
                    &cancellation,
                )
                .await
            }
            None => {
                prepare_skill_content(
                    &skill,
                    Some(&input.args),
                    &substitution_context,
                    &cancellation,
                )
                .await
            }
        };
        let prepared_body = prepared.map_err(|error| {
            tool_runtime::ToolError::custom("skill_load_failed", error.to_string())
        })?;

        Ok(LoadSkillOutput {
            skill_name: skill.name,
            source: skill.path,
            args: input.args,
            prepared_body,
        })
    }
}

fn same_skill_identity(
    current: &crate::implementations::skills::types::SkillInfo,
    selected: &crate::implementations::skills::types::SkillInfo,
) -> bool {
    current.name == selected.name
        && current.path == selected.path
        && current.scope == selected.scope
        && current.plugin_name == selected.plugin_name
        && current.plugin_root == selected.plugin_root
        && current.plugin_data == selected.plugin_data
}
