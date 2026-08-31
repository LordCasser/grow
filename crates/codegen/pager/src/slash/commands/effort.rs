//! `/effort` — set reasoning effort on the current model without re-picking it.
//!
//! Emits an effort-only Sampling patch. The current model is retained only as
//! a local validation/display hint; Shell composes the patch with its newest
//! desired Sampling target so concurrent clients cannot resurrect an old
//! model.

use crate::app::actions::Action;
use crate::slash::command::{AppCtx, ArgItem, CommandExecCtx, CommandResult, SlashCommand};
use crate::slash::commands::effort_levels::build_effort_arg_items;

/// Set reasoning effort for the active model.
pub struct EffortCommand;

impl SlashCommand for EffortCommand {
    fn name(&self) -> &str {
        "effort"
    }

    fn description(&self) -> &str {
        "Set reasoning effort for the current model"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn offered_when_session_less(&self) -> bool {
        // The dashboard stages effort together with the model for the next
        // spawned Agent.
        true
    }

    fn usage(&self) -> &str {
        // Levels are model-specific; empty-args and UnknownToken errors list
        // the active model's offered option ids instead of a hardcoded set.
        "/effort [level]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("<level>")
    }

    fn suggest_args(&self, ctx: &AppCtx, _args_query: &str) -> Option<Vec<ArgItem>> {
        let options = ctx.models.reasoning_effort_options();
        if options.is_empty() {
            return None;
        }
        Some(build_effort_arg_items(
            &options,
            ctx.models.reasoning_effort,
            true,
            |option| option.id.clone(),
        ))
    }

    fn run(&self, ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let trimmed = args.trim();
        let Some(model_id) = ctx.models.current.clone() else {
            return CommandResult::Error("No active model".into());
        };

        if trimmed.is_empty() {
            return CommandResult::Action(Action::OpenCommandPicker {
                command: "effort".to_string(),
                args_query: String::new(),
            });
        }

        // Same gate-first policy as the CLI (`--reasoning-effort`) and headless.
        match ctx.models.resolve_effort_for_model(&model_id, trimmed) {
            Ok(effort) => CommandResult::Action(Action::PatchEffort { model_id, effort }),
            Err(err) => CommandResult::Error(err.message()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use shell::sampling::types::ReasoningEffort;
    use std::sync::Arc;

    fn model_with_reasoning(
        id: &str,
        name: &str,
    ) -> (
        shell::agent::models::ModelId,
        shell::agent::models::ModelInfo,
    ) {
        let id = shell::agent::models::ModelId::new(Arc::from(id));
        let mut meta = serde_json::Map::new();
        meta.insert(
            "reasoningEfforts".into(),
            serde_json::json!(["xhigh", "high", "medium", "low"]),
        );
        let info = shell::agent::models::ModelInfo::new(id.clone(), name.to_string())
            .meta(serde_json::Value::Object(meta).as_object().cloned());
        (id, info)
    }

    fn plain_model(
        id: &str,
        name: &str,
    ) -> (
        shell::agent::models::ModelId,
        shell::agent::models::ModelInfo,
    ) {
        let id = shell::agent::models::ModelId::new(Arc::from(id));
        let info = shell::agent::models::ModelInfo::new(id.clone(), name.to_string());
        (id, info)
    }

    static EMPTY_BUNDLE: crate::app::bundle::BundleState = crate::app::bundle::BundleState {
        has_cache: false,
        version: String::new(),
        agents: Vec::new(),
        skills: Vec::new(),
    };

    fn dummy_exec_ctx(models: &ModelState) -> CommandExecCtx<'_> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: &EMPTY_BUNDLE,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: crate::settings::PagerLocalSnapshot {
                multiline_mode: false,
                permission_mode: shell::util::config::PermissionMode::Ask,
                ..crate::settings::PagerLocalSnapshot::default()
            },
        }
    }

    #[test]
    fn empty_args_opens_picker() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id.clone(), info);
        state.current = Some(id);
        state.reasoning_effort = Some(ReasoningEffort::Medium);
        let mut ctx = dummy_exec_ctx(&state);
        let result = EffortCommand.run(&mut ctx, "");
        assert!(matches!(
            result,
            CommandResult::Action(Action::OpenCommandPicker { ref command, .. })
                if command == "effort"
        ));
    }

    #[test]
    fn unknown_level_errors() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id.clone(), info);
        state.current = Some(id);
        let mut ctx = dummy_exec_ctx(&state);
        let result = EffortCommand.run(&mut ctx, "turbo");
        match result {
            CommandResult::Error(msg) => {
                assert!(msg.contains("unknown effort level 'turbo'"), "msg={msg}");
                assert!(msg.contains("use one of:"), "msg={msg}");
                assert!(msg.contains("xhigh"), "msg={msg}");
                assert!(!msg.contains("none"), "msg={msg}");
                assert!(!msg.contains("minimal"), "msg={msg}");
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn valid_level_dispatches_effort_patch_on_current() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id.clone(), info);
        state.current = Some(id.clone());
        let mut ctx = dummy_exec_ctx(&state);
        let result = EffortCommand.run(&mut ctx, "high");
        match result {
            CommandResult::Action(Action::PatchEffort { model_id, effort }) => {
                assert_eq!(model_id, id);
                assert_eq!(effort, ReasoningEffort::High);
            }
            other => panic!("expected PatchEffort, got {other:?}"),
        }
    }

    #[test]
    fn none_and_minimal_rejected_when_model_menu_omits_them() {
        // Legacy fallback menu is low..xhigh — `none`/`minimal` used to pass
        // through and 400 on grow-4.5; reject at the TUI instead.
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id.clone(), info);
        state.current = Some(id);
        let mut ctx = dummy_exec_ctx(&state);
        for token in ["none", "minimal"] {
            let result = EffortCommand.run(&mut ctx, token);
            match result {
                CommandResult::Error(ref msg) => {
                    assert!(
                        msg.contains(&format!("unknown effort level '{token}'")),
                        "expected Error for {token}, got {msg}"
                    );
                    // Must not re-advertise the rejected token as a valid choice
                    // (aside from quoting it in "unknown effort level '…'").
                    let after_prefix = msg
                        .split_once("; ")
                        .map(|(_, rest)| rest)
                        .unwrap_or(msg.as_str());
                    assert!(
                        !after_prefix.contains(token),
                        "error must not list {token} as offered: {msg}"
                    );
                    assert!(!msg.contains("unset"), "msg={msg}");
                }
                other => panic!("expected Error for {token}, got {other:?}"),
            }
        }
    }

    #[test]
    fn none_accepted_when_model_menu_offers_it() {
        let mut state = ModelState::default();
        let id = shell::agent::models::ModelId::new(Arc::from("effort-dual"));
        let info = shell::agent::models::ModelInfo::new(id.clone(), "Effort Dual".to_string())
            .meta(
                serde_json::json!({
                    "reasoningEfforts": [
                        { "value": "none", "label": "None", "default": true },
                        { "value": "high", "label": "High" },
                    ],
                })
                .as_object()
                .cloned(),
            );
        state.available.insert(id.clone(), info);
        state.current = Some(id.clone());
        let mut ctx = dummy_exec_ctx(&state);
        let result = EffortCommand.run(&mut ctx, "none");
        match result {
            CommandResult::Action(Action::PatchEffort { model_id, effort }) => {
                assert_eq!(model_id, id);
                assert_eq!(effort, ReasoningEffort::None);
            }
            other => panic!("expected PatchEffort with none, got {other:?}"),
        }
    }

    #[test]
    fn remap_id_dispatches_mapped_canonical_effort() {
        let mut state = ModelState::default();
        let id = shell::agent::models::ModelId::new(Arc::from("reasoning-x"));
        let info = shell::agent::models::ModelInfo::new(id.clone(), "Reasoning X".to_string())
            .meta(
                serde_json::json!({
                    "reasoningEfforts": [{ "id": "deep", "value": "xhigh", "label": "Deep" }],
                })
                .as_object()
                .cloned(),
            );
        state.available.insert(id.clone(), info);
        state.current = Some(id.clone());
        let mut ctx = dummy_exec_ctx(&state);
        // The rendered row inserts the id; `/effort deep` must send `xhigh`.
        match EffortCommand.run(&mut ctx, "deep") {
            CommandResult::Action(Action::PatchEffort { model_id, effort }) => {
                assert_eq!(model_id, id);
                assert_eq!(effort, ReasoningEffort::Xhigh);
            }
            other => panic!("expected PatchEffort with remapped effort, got {other:?}"),
        }
    }

    #[test]
    fn non_reasoning_model_errors() {
        let mut state = ModelState::default();
        let (id, info) = plain_model("grow-4.5", "Grow 4.5");
        state.available.insert(id.clone(), info);
        state.current = Some(id);
        let mut ctx = dummy_exec_ctx(&state);
        let result = EffortCommand.run(&mut ctx, "high");
        assert!(matches!(
            result,
            CommandResult::Error(msg) if msg.contains("does not support reasoning effort")
        ));
    }

    #[test]
    fn no_current_model_errors() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id, info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = EffortCommand.run(&mut ctx, "high");
        assert!(matches!(result, CommandResult::Error(msg) if msg.contains("No active model")));
    }

    #[test]
    fn suggest_args_none_without_current_or_support() {
        let cmd = EffortCommand;
        let empty = ModelState::default();
        let ctx = AppCtx {
            models: &empty,
            agents: &[],
            current_agent: None,
            behavior_mode: tools::types::BehaviorId::Normal,
            goal_available: false,
            current_goal_objective: None,
            auto_permission_available: false,
            current_permission: "ask",
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            workflows_available: true,
            screen_mode: crate::app::ScreenMode::Fullscreen,
        };
        assert!(cmd.suggest_args(&ctx, "").is_none());

        let mut plain = ModelState::default();
        let (id, info) = plain_model("grow-4.5", "Grow 4.5");
        plain.available.insert(id.clone(), info);
        plain.current = Some(id);
        let ctx = AppCtx {
            models: &plain,
            agents: &[],
            current_agent: None,
            behavior_mode: tools::types::BehaviorId::Normal,
            goal_available: false,
            current_goal_objective: None,
            auto_permission_available: false,
            current_permission: "ask",
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            workflows_available: true,
            screen_mode: crate::app::ScreenMode::Fullscreen,
        };
        assert!(cmd.suggest_args(&ctx, "").is_none());
    }

    #[test]
    fn suggest_args_lists_levels_with_active_marker() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id.clone(), info);
        state.current = Some(id);
        state.reasoning_effort = Some(ReasoningEffort::High);

        let cmd = EffortCommand;
        let ctx = AppCtx {
            models: &state,
            agents: &[],
            current_agent: None,
            behavior_mode: tools::types::BehaviorId::Normal,
            goal_available: false,
            current_goal_objective: None,
            auto_permission_available: false,
            current_permission: "ask",
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            workflows_available: true,
            screen_mode: crate::app::ScreenMode::Fullscreen,
        };
        let items = cmd.suggest_args(&ctx, "").unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].insert_text, "xhigh");
        assert_eq!(items[1].insert_text, "high");
        assert_eq!(items[1].display, "High (active)");
        assert_eq!(items[2].insert_text, "medium");
        assert_eq!(items[3].insert_text, "low");
        assert!(items[0].match_text.starts_with("a "));
        assert!(items[3].match_text.starts_with("d "));
    }
}
