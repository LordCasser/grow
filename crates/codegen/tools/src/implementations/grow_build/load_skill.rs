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
    /// Resource path approved by preflight; never supplied by model arguments.
    pub resource_path: Option<std::path::PathBuf>,
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
    /// Optional UTF-8 resource path relative to the skill root, e.g. references/api.md.
    /// Omit to load SKILL.md. Load only a needed resource; do not combine with args.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
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
        "Load a skill by its exact Grow catalog name, never a filesystem path. Omit resource to load SKILL.md with optional args. For progressive disclosure, request only a needed UTF-8 reference using the same name and resource relative to the skill root, without args. Resources are read literally; scripts are never executed by this tool. If unavailable, disabled, or denied, report that the skill cannot be loaded; do not bypass through Read, Shell, or another copy. Guidance remains subject to ordinary tool authorization."
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
        if input.resource.is_some() && !input.args.is_empty() {
            return Err(tool_runtime::ToolError::invalid_arguments(
                "Resource loads do not accept args",
            ));
        }
        let resources = crate::types::tool_metadata::shared_resources(&ctx)?;
        let selection = ctx
            .get::<SkillLoadSelection>()
            .map(|selection| (*selection).clone());
        let (skill, fs, owner_session_id) = {
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
            let fs = if input.resource.is_some()
                || (skill.body.is_none() && !skill.path.contains("://"))
            {
                Some(resources.require::<FileSystem>()?.0.clone())
            } else {
                None
            };
            let owner = resources
                .get::<OwnerSessionId>()
                .map(|session| session.0.clone());
            (skill, fs, owner)
        };
        let cancellation = ctx
            .get::<tool_runtime::Cancellation>()
            .map_or_else(tokio_util::sync::CancellationToken::new, |token| {
                token.0.clone()
            });
        let resource_path = tokio::select! {
            _ = cancellation.cancelled() => return Err(tool_runtime::ToolError::custom("skill_load_failed", "Skill loading was cancelled")),
            result = resolve_skill_resource(&skill, input.resource.as_deref()) => result?,
        };
        if let Some(selection) = &selection {
            if selection.resource_path != resource_path {
                return Err(tool_runtime::ToolError::invalid_arguments(
                    "Skill resource identity changed after authorization",
                ));
            }
        }
        let source = resource_path
            .as_deref()
            .unwrap_or_else(|| std::path::Path::new(&skill.path));
        {
            let resources = resources.lock().await;
            if resources
                .get::<RespectGitignore>()
                .is_some_and(|value| value.0)
                && resources
                    .get::<GitignoreFilter>()
                    .is_some_and(|filter| filter.is_ignored(source))
            {
                return Err(tool_runtime::ToolError::custom(
                    "skill_load_failed",
                    "Skill source is ignored by .gitignore; do not bypass this restriction",
                ));
            }
        }
        if let Some(resource_path) = resource_path.as_ref() {
            let fs = fs.expect("resource reads require configured filesystem");
            let bytes = tokio::select! {
                _ = cancellation.cancelled() => return Err(tool_runtime::ToolError::custom("skill_load_failed", "Skill loading was cancelled")),
                result = fs.read_file_bounded(resource_path, crate::implementations::skills::skill::MAX_PREPARED_SKILL_BODY_BYTES) => result.map_err(|error| tool_runtime::ToolError::custom("skill_load_failed", error.to_string()))?,
            };
            let max = crate::implementations::skills::skill::MAX_PREPARED_SKILL_BODY_BYTES;
            if bytes.len() > max {
                return Err(tool_runtime::ToolError::custom(
                    "skill_load_failed",
                    "Skill resource exceeds 1 MiB",
                ));
            }
            let body = String::from_utf8(bytes).map_err(|error| {
                tool_runtime::ToolError::custom(
                    "skill_load_failed",
                    format!("Skill resource is not UTF-8: {error}"),
                )
            })?;
            // Identity lives in the committed body, so replay needs no new
            // schema or reopening of a mutable resource file.
            let prepared_body = format!(
                "<skill_resource name=\"{}\" resource=\"{}\">\n{}\n</skill_resource>",
                crate::implementations::skills::skill::escape_xml(&input.name),
                crate::implementations::skills::skill::escape_xml(
                    input.resource.as_deref().unwrap()
                ),
                body
            );
            if prepared_body.len() > max || cancellation.is_cancelled() {
                return Err(tool_runtime::ToolError::custom(
                    "skill_load_failed",
                    "Skill resource preparation exceeds 1 MiB or was cancelled",
                ));
            }
            return Ok(LoadSkillOutput {
                skill_name: skill.name,
                source: resource_path.to_string_lossy().into_owned(),
                args: input.args,
                prepared_body,
            });
        }
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

/// Resolve only package-local resources. Validation happens before ordinary
/// read authorization and is repeated at execution to detect changed aliases.
pub async fn resolve_skill_resource(
    skill: &crate::implementations::skills::types::SkillInfo,
    resource: Option<&str>,
) -> Result<Option<std::path::PathBuf>, tool_runtime::ToolError> {
    let Some(resource) = resource else {
        return Ok(None);
    };
    let invalid = |reason: &str| {
        tool_runtime::ToolError::invalid_arguments(format!(
            "Skill resource cannot be loaded: {reason}. Do not bypass with another path or tool."
        ))
    };
    if resource.is_empty()
        || resource.contains(['\\', ':'])
        || resource
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || std::path::Path::new(resource).is_absolute()
    {
        return Err(invalid(
            "expected a relative path inside the selected skill",
        ));
    }
    if resource.eq_ignore_ascii_case("SKILL.md") {
        return Err(invalid("omit resource to load the main skill instructions"));
    }
    if skill.path.contains("://") {
        return Err(invalid("this skill has no local resource root"));
    }
    let root = std::path::Path::new(&skill.path)
        .parent()
        .ok_or_else(|| invalid("missing skill root"))?;
    let root = tokio::fs::canonicalize(root)
        .await
        .map_err(|error| invalid(&error.to_string()))?;
    let target = tokio::fs::canonicalize(root.join(resource))
        .await
        .map_err(|error| invalid(&error.to_string()))?;
    if !target.starts_with(&root) || target == root {
        return Err(invalid("resolved resource escapes the skill root"));
    }
    if !tokio::fs::metadata(&target)
        .await
        .map_err(|error| invalid(&error.to_string()))?
        .is_file()
    {
        return Err(invalid("resource is not a regular file"));
    }
    Ok(Some(target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementations::skills::types::SkillInfo;
    use crate::types::resources::Resources;
    use crate::types::tool_metadata::test_ctx;
    use std::sync::Arc;
    use tool_runtime::Tool;

    fn context(skill: SkillInfo) -> tool_runtime::ToolCallContext {
        let mut manager = SkillManager::new();
        manager.seed(None, None, vec![skill], None, None);
        let mut resources = Resources::new();
        resources.insert(manager);
        resources.insert(FileSystem(Arc::new(crate::computer::local::LocalFs)));
        test_ctx(Arc::new(tokio::sync::Mutex::new(resources)))
    }

    fn input(resource: Option<&str>) -> LoadSkillInput {
        LoadSkillInput {
            name: "review".into(),
            args: String::new(),
            resource: resource.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn name_only_main_and_single_resource_are_lazy_and_literal() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("SKILL.md");
        std::fs::write(
            &main,
            "---\nname: review\ndescription: review\n---\nMAIN ONLY",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("references")).unwrap();
        // An invalid sibling must neither fail the main read nor enter context.
        std::fs::write(dir.path().join("references/bad.bin"), [0xff]).unwrap();
        let ctx = context(SkillInfo {
            name: "review".into(),
            path: main.to_string_lossy().into_owned(),
            ..Default::default()
        });
        let loaded = LoadSkillTool.run(ctx.clone(), input(None)).await.unwrap();
        assert_eq!(loaded.prepared_body, "MAIN ONLY");
        // Creating the reference after the main load proves no eager snapshot.
        let reference = dir.path().join("references/细节.md");
        let literal = "---\nkeep: frontmatter\n---\n$ARGUMENTS ${GROW_SESSION_ID}\nOnly this file";
        std::fs::write(&reference, literal).unwrap();
        let loaded = LoadSkillTool
            .run(ctx, input(Some("references/细节.md")))
            .await
            .unwrap();
        assert!(loaded.prepared_body.contains(literal));
        assert!(
            loaded
                .prepared_body
                .contains("resource=\"references/细节.md\"")
        );
        assert!(!loaded.prepared_body.contains("MAIN ONLY"));
        assert_eq!(
            std::path::Path::new(&loaded.source),
            std::fs::canonicalize(reference).unwrap()
        );
    }

    #[tokio::test]
    async fn disabled_skill_fails_before_source_access_for_main_and_resource() {
        let ctx = context(SkillInfo {
            name: "review".into(),
            path: "/does-not-exist/SKILL.md".into(),
            enabled: false,
            ..Default::default()
        });
        for resource in [None, Some("references/api.md")] {
            let error = LoadSkillTool
                .run(ctx.clone(), input(resource))
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains("disabled by configuration"), "{error}");
            assert!(error.contains("do not bypass"), "{error}");
        }
    }

    #[tokio::test]
    async fn resources_reject_invalid_sources_and_never_return_partial_text() {
        let dir = tempfile::tempdir().unwrap();
        let skill = SkillInfo {
            name: "review".into(),
            path: dir.path().join("SKILL.md").to_string_lossy().into_owned(),
            body: Some("frozen main".into()),
            ..Default::default()
        };
        let ctx = context(skill.clone());
        for resource in [
            "../secret",
            "/etc/passwd",
            "C:/secret",
            "C:\\secret",
            "\\\\host\\share",
            "a/../secret",
            "",
            "SKILL.md",
            "missing",
        ] {
            assert!(
                LoadSkillTool
                    .run(ctx.clone(), input(Some(resource)))
                    .await
                    .is_err(),
                "{resource}"
            );
        }
        for (name, bytes) in [
            ("invalid.txt", vec![0xff]),
            ("huge.txt", vec![b'x'; 1024 * 1024 + 1]),
        ] {
            std::fs::write(dir.path().join(name), bytes).unwrap();
            assert!(
                LoadSkillTool
                    .run(ctx.clone(), input(Some(name)))
                    .await
                    .is_err()
            );
        }
        let mut mixed = input(Some("missing"));
        mixed.args = "must fail before reading".into();
        assert!(
            LoadSkillTool
                .run(ctx.clone(), mixed)
                .await
                .unwrap_err()
                .to_string()
                .contains("do not accept args")
        );
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();
        let mut cancelled_ctx = ctx;
        cancelled_ctx
            .extensions
            .insert(tool_runtime::Cancellation(token));
        assert!(LoadSkillTool.run(cancelled_ctx, input(None)).await.is_err());
    }

    #[tokio::test]
    async fn resource_requires_an_accessible_local_skill_root() {
        let dir = tempfile::tempdir().unwrap();
        for path in [
            "skill://remote/review".to_owned(),
            dir.path()
                .join("missing/SKILL.md")
                .to_string_lossy()
                .into_owned(),
        ] {
            let ctx = context(SkillInfo {
                name: "review".into(),
                path,
                body: Some("frozen main".into()),
                ..Default::default()
            });
            assert!(LoadSkillTool.run(ctx.clone(), input(None)).await.is_ok());
            assert!(
                LoadSkillTool
                    .run(ctx, input(Some("reference.md")))
                    .await
                    .is_err()
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn resource_symlink_escape_and_post_authorization_change_fail() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let skill = SkillInfo {
            name: "review".into(),
            path: dir.path().join("SKILL.md").to_string_lossy().into_owned(),
            body: Some("main".into()),
            ..Default::default()
        };
        let external = outside.path().join("secret");
        std::fs::write(&external, "DO NOT LOAD").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&external, &link).unwrap();
        assert!(
            resolve_skill_resource(&skill, Some("link"))
                .await
                .unwrap_err()
                .to_string()
                .contains("escapes")
        );
        std::fs::remove_file(&link).unwrap();
        for name in ["a", "b"] {
            std::fs::write(dir.path().join(name), name).unwrap();
        }
        std::os::unix::fs::symlink(dir.path().join("a"), &link).unwrap();
        let approved = resolve_skill_resource(&skill, Some("link")).await.unwrap();
        let mut ctx = context(skill.clone());
        ctx.extensions.insert(SkillLoadSelection {
            skill,
            user_invocation: false,
            resource_path: approved,
        });
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(dir.path().join("b"), &link).unwrap();
        let error = LoadSkillTool
            .run(ctx, input(Some("link")))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("identity changed"));
    }
}
