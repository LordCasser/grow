//! `/model` — switch model + (optionally) reasoning effort.
//! Chained autocomplete: pick a reasoning-supported model → trailing space
//! re-opens the dropdown into a `low|medium|high|xhigh` sub-menu.

use agent_client_protocol as acp;

use crate::acp::model_state::ModelState;
use crate::app::actions::Action;
use crate::slash::command::{AppCtx, ArgItem, CommandExecCtx, CommandResult, SlashCommand};
use crate::slash::commands::effort_levels::build_effort_arg_items;

/// Switch the active model (and optionally its reasoning effort).
pub struct ModelCommand;

impl SlashCommand for ModelCommand {
    fn name(&self) -> &str {
        "model"
    }

    fn description(&self) -> &str {
        "Switch the active model"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn offered_when_session_less(&self) -> bool {
        // The dashboard offers `/model` to pick the model for the next
        // spawned agent (intercepted in `dispatch_dashboard_dispatch_slash`).
        true
    }

    fn usage(&self) -> &str {
        "/model [name] [effort]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("<model> [effort]")
    }

    fn suggest_args(&self, ctx: &AppCtx, args_query: &str) -> Option<Vec<ArgItem>> {
        if ctx.models.is_empty() {
            return None;
        }

        // Effort phase if input is "<reasoning-model> ", else model phase.
        if let Some(model_id) = detect_effort_phase(ctx.models, args_query) {
            return Some(build_effort_items(ctx.models, &model_id));
        }
        Some(build_model_items(ctx.models))
    }

    fn run(&self, ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            return CommandResult::Action(Action::OpenCommandPicker {
                command: "model".to_string(),
                args_query: String::new(),
            });
        }

        // Exact catalog id match first (case-insensitive). Only `provider/model`
        // ids are accepted — no display-name fallback.
        if let Some(id) = ctx.models.resolve_by_id(trimmed) {
            return CommandResult::Action(Action::SwitchModel {
                model_id: id,
                effort: None,
            });
        }

        // Trailing effort token + reasoning model → session-scoped switch
        // (not persisted as default). Resolve via the shared gate so a rejected
        // level (e.g. `none` on grow-4.5) surfaces the effort error with the
        // model's offered ids — not "Unknown model: … none".
        if let Some((prefix, token)) = split_trailing_token(trimmed)
            && let Some(id) = resolve_model(ctx.models, prefix)
            && ctx
                .models
                .available
                .get(&id)
                .map(supports_reasoning_effort)
                .unwrap_or(false)
        {
            return match ctx.models.resolve_effort_for_model(&id, token) {
                Ok(effort) => CommandResult::Action(Action::SwitchModel {
                    model_id: id,
                    effort: Some(effort),
                }),
                Err(err) => CommandResult::Error(err.message()),
            };
        }

        CommandResult::Error(format!("Unknown model: {trimmed}"))
    }
}

/// Look up a model by case-insensitive catalog id only.
fn resolve_model(models: &ModelState, id: &str) -> Option<acp::ModelId> {
    models.resolve_by_id(id)
}

fn supports_reasoning_effort(info: &acp::ModelInfo) -> bool {
    shell::sampling::types::parse_reasoning_efforts_meta(info.meta.as_ref()).is_some()
}

/// Split `args` into `(prefix, last_token)` on the final whitespace run.
/// Returns `None` when there is no interior whitespace to split on. The token is
/// resolved to an effort against the picked model's options by the caller.
fn split_trailing_token(args: &str) -> Option<(&str, &str)> {
    let (prefix, last) = args.rsplit_once(char::is_whitespace)?;
    let prefix = prefix.trim_end();
    if prefix.is_empty() || last.is_empty() {
        return None;
    }
    Some((prefix, last))
}

/// Returns the matched model id when `args_query` is `"<catalog-id> ..."`.
/// Matches only stable catalog ids (`provider/model`). Longest id first so
/// shared prefixes do not steal the match.
fn detect_effort_phase(models: &ModelState, args_query: &str) -> Option<acp::ModelId> {
    let mut candidates: Vec<(&acp::ModelId, &str)> = models
        .available
        .iter()
        .filter(|(_, info)| supports_reasoning_effort(info))
        .map(|(id, _)| (id, id.0.as_ref()))
        .collect();
    candidates.sort_by_key(|(_, token)| std::cmp::Reverse(token.len()));

    for (id, token) in candidates {
        if args_query.len() > token.len()
            && args_query.is_char_boundary(token.len())
            && args_query[..token.len()].eq_ignore_ascii_case(token)
            && args_query[token.len()..].starts_with(char::is_whitespace)
        {
            return Some(id.clone());
        }
    }
    None
}

/// One row per logical model. Reasoning models get a trailing space in
/// `insert_text` so the prompt widget chains into the effort sub-menu.
///
/// Display, match, and insert all use the stable catalog id
/// (`provider/model`). Friendly display names are not shown in selection UI.
fn build_model_items(models: &ModelState) -> Vec<ArgItem> {
    let current_id = models.current.as_ref();
    let mut items: Vec<ArgItem> = Vec::with_capacity(models.available.len());
    for (id, info) in &models.available {
        let is_current = current_id == Some(id);
        let supports = supports_reasoning_effort(info);
        let catalog_id = id.0.as_ref();

        let display = if is_current {
            format!("{catalog_id} (current)")
        } else {
            catalog_id.to_string()
        };

        // Trailing space on reasoning models: signals "more input
        // expected" to the prompt widget so Enter advances to effort
        // phase instead of submitting.
        let insert_text = if supports {
            format!("{catalog_id} ")
        } else {
            catalog_id.to_string()
        };

        items.push(ArgItem {
            display,
            match_text: catalog_id.to_string(),
            insert_text,
            description: info.description.clone().unwrap_or_default(),
        });
    }
    items
}

/// One row per effort level for the `/model` chained effort phase.
/// `insert_text` is `"<catalog-id> high"` so selecting a row completes both tokens.
fn build_effort_items(models: &ModelState, model_id: &acp::ModelId) -> Vec<ArgItem> {
    if models.available.get(model_id).is_none() {
        return Vec::new();
    }
    let catalog_id = model_id.0.to_string();
    let is_current_model = models.current.as_ref() == Some(model_id);
    let options = models.reasoning_effort_options_for(model_id);
    build_effort_arg_items(
        &options,
        models.reasoning_effort,
        is_current_model,
        |option| format!("{catalog_id} {}", option.id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use shell::sampling::types::ReasoningEffort;
    use std::sync::Arc;

    fn model_with_reasoning(id: &str, name: &str) -> (acp::ModelId, acp::ModelInfo) {
        let id = acp::ModelId::new(Arc::from(id));
        let mut meta = serde_json::Map::new();
        meta.insert(
            "reasoningEfforts".into(),
            serde_json::json!(["xhigh", "high", "medium", "low"]),
        );
        let info = acp::ModelInfo::new(id.clone(), name.to_string())
            .meta(serde_json::Value::Object(meta).as_object().cloned());
        (id, info)
    }

    fn plain_model(id: &str, name: &str) -> (acp::ModelId, acp::ModelInfo) {
        let id = acp::ModelId::new(Arc::from(id));
        let info = acp::ModelInfo::new(id.clone(), name.to_string());
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
    fn split_trailing_token_splits_on_final_whitespace() {
        assert_eq!(
            split_trailing_token("Reasoning X high"),
            Some(("Reasoning X", "high"))
        );
        assert_eq!(
            split_trailing_token("reasoning-x  xhigh"),
            Some(("reasoning-x", "xhigh"))
        );
        // No interior whitespace → nothing to split off.
        assert!(split_trailing_token("reasoning-x-pro").is_none());
    }

    #[test]
    fn empty_query_returns_one_row_per_logical_model() {
        let mut state = ModelState::default();
        let (rid, rinfo) = model_with_reasoning("reasoning-x", "Reasoning X");
        let (pid, pinfo) = plain_model("grow-4.5", "Grow 4.5");
        state.available.insert(rid, rinfo);
        state.available.insert(pid, pinfo);

        let cmd = ModelCommand;
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
        assert_eq!(items.len(), 2, "model phase: one row per logical model");

        // Reasoning model has trailing space in insert_text -- this is the
        // signal the prompt widget reads to keep the dropdown open after
        // Enter so the effort sub-menu can render. Insert uses catalog id.
        let reasoning = items
            .iter()
            .find(|i| i.insert_text.starts_with("reasoning-x"))
            .unwrap();
        assert_eq!(reasoning.insert_text, "reasoning-x ");
        assert_eq!(reasoning.display, "reasoning-x");
        assert_eq!(reasoning.match_text, "reasoning-x");

        // Plain model has no trailing space -- Enter commits immediately.
        let plain = items.iter().find(|i| i.insert_text == "grow-4.5").unwrap();
        assert_eq!(plain.insert_text, "grow-4.5");
        assert_eq!(plain.display, "grow-4.5");
        assert_eq!(plain.match_text, "grow-4.5");
    }

    #[test]
    fn trailing_space_after_reasoning_model_enters_effort_phase() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id, info);

        let cmd = ModelCommand;
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
        // Args query has a trailing space after catalog id -> effort phase.
        let items = cmd.suggest_args(&ctx, "reasoning-x ").unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].insert_text, "reasoning-x xhigh");
        assert_eq!(items[1].insert_text, "reasoning-x high");
        assert_eq!(items[2].insert_text, "reasoning-x medium");
        assert_eq!(items[3].insert_text, "reasoning-x low");
        // Display is just the level so the user sees a clean column.
        assert_eq!(items[0].display, "Xhigh");
        // match_text carries the sort-key prefix that forces the matcher's
        // alphabetical tiebreak to preserve the model-declared order.
        assert!(items[0].match_text.starts_with("a "));
        assert!(items[3].match_text.starts_with("d "));
        // Display names are not accepted for effort-phase entry.
        let by_name = cmd.suggest_args(&ctx, "Reasoning X ").unwrap();
        assert!(
            by_name.iter().all(|i| i.display != "Xhigh"),
            "display-name query must stay in model phase, got {by_name:?}"
        );
    }

    #[test]
    fn partial_effort_query_still_in_effort_phase() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id, info);

        let cmd = ModelCommand;
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
        // Still in effort phase; matcher upstream narrows to high / xhigh.
        let items = cmd.suggest_args(&ctx, "reasoning-x h").unwrap();
        assert_eq!(items.len(), 4);
    }

    #[test]
    fn partial_model_query_stays_in_model_phase() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id, info);

        let cmd = ModelCommand;
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
        // No trailing space, user is still typing the catalog id.
        let items = cmd.suggest_args(&ctx, "reason").unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].insert_text, "reasoning-x ");
    }

    #[test]
    fn run_parses_model_plus_effort_when_supported() {
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id, info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "reasoning-x xhigh");
        match result {
            CommandResult::Action(Action::SwitchModel { model_id, effort }) => {
                assert_eq!(model_id.0.as_ref(), "reasoning-x");
                assert_eq!(effort, Some(ReasoningEffort::Xhigh));
            }
            other => panic!("expected SwitchModel with effort, got {other:?}"),
        }
    }

    #[test]
    fn run_rejects_unoffered_effort_with_effort_error_not_unknown_model() {
        // Regression: previously `resolve_effort_token_for` returned None and
        // the handler fell through to `Unknown model: reasoning-x none`.
        let mut state = ModelState::default();
        let (id, info) = model_with_reasoning("reasoning-x", "Reasoning X");
        state.available.insert(id, info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "reasoning-x none");
        match result {
            CommandResult::Error(msg) => {
                assert!(
                    msg.contains("unknown effort level 'none'"),
                    "expected effort error, got {msg}"
                );
                assert!(
                    msg.contains("use one of:"),
                    "expected offered levels in message, got {msg}"
                );
                assert!(
                    !msg.to_lowercase().contains("unknown model"),
                    "must not misreport as unknown model: {msg}"
                );
                let offered = msg.split_once("; ").map(|(_, r)| r).unwrap_or("");
                assert!(
                    !offered.contains("none"),
                    "must not list none as offered: {msg}"
                );
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn run_prefers_full_catalog_id_over_prefix_plus_effort() {
        // Catalog has both "grow" (reasoning) and "grow-4.5". `/model grow-4.5`
        // must select the full id, not treat a suffix as effort on "grow".
        let mut state = ModelState::default();
        let (short_id, short_info) = model_with_reasoning("grow", "Grow");
        let (long_id, long_info) = model_with_reasoning("grow-4.5", "Grow 4.5");
        state.available.insert(short_id, short_info);
        state.available.insert(long_id.clone(), long_info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "grow-4.5");
        match result {
            CommandResult::Action(Action::SwitchModel {
                model_id: resolved_id,
                effort: None,
            }) => {
                assert_eq!(resolved_id, long_id);
            }
            other => panic!("expected SwitchModel(grow-4.5), got {other:?}"),
        }
    }

    #[test]
    fn run_rejects_effort_for_non_reasoning_model() {
        let mut state = ModelState::default();
        let (id, info) = plain_model("grow-4.5", "Grow 4.5");
        state.available.insert(id, info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "grow-4.5 high");
        // Falls through — not a reasoning model — Unknown error.
        assert!(matches!(result, CommandResult::Error(_)));
    }

    /// Display names are rejected; only catalog ids switch the session.
    #[test]
    fn run_rejects_display_name() {
        let mut state = ModelState::default();
        let (id, info) = plain_model("grow-4.5", "Grow 4.5");
        state.available.insert(id, info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "Grow 4.5");
        assert!(
            matches!(result, CommandResult::Error(ref msg) if msg.contains("Unknown model")),
            "got {result:?}"
        );
    }

    /// The bare `/model <catalog-id>` form changes only the current session.
    #[test]
    fn run_bare_catalog_id_switches_session_model() {
        let mut state = ModelState::default();
        let (id, info) = plain_model("grow-4.5", "Grow 4.5");
        state.available.insert(id.clone(), info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "grow-4.5");
        match result {
            CommandResult::Action(Action::SwitchModel {
                model_id: resolved_id,
                effort: None,
            }) => {
                assert_eq!(resolved_id, id);
            }
            other => panic!("expected Action::SwitchModel(<id>), got {other:?}"),
        }
    }

    /// Case-insensitive matching against catalog ids only.
    #[test]
    fn run_switch_model_resolves_case_insensitively() {
        let mut state = ModelState::default();
        let (id, info) = plain_model("grow-4.5", "Grow 4.5");
        state.available.insert(id.clone(), info);
        let mut ctx = dummy_exec_ctx(&state);
        let result = ModelCommand.run(&mut ctx, "GROW-4.5");
        match result {
            CommandResult::Action(Action::SwitchModel {
                model_id: resolved_id,
                effort: None,
            }) => {
                assert_eq!(resolved_id, id);
            }
            other => panic!("expected Action::SwitchModel(<id>), got {other:?}"),
        }
    }
}
