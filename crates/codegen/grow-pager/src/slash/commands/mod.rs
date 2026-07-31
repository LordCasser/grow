//! Concrete slash command implementations.
//!
//! Each command lives in its own submodule. This module re-exports
//! command structs and provides `builtin_commands()` for registry
//! construction.
pub mod agent;
pub mod always_approve;
pub mod announcements;
pub mod auto;
pub mod behavior;
pub mod btw;
pub mod cd;
pub mod compact;
pub mod compact_mode;
pub mod config_agents;
pub mod context;
pub mod copy;
pub mod dashboard;
pub mod debug;
pub mod docs;
pub mod doctor;
pub mod edit_prompt;
pub mod effort;
pub mod effort_levels;
pub mod exit;
pub mod expand;
pub mod export;
pub mod feedback;
pub mod find;
pub mod fork;
pub mod gboom;
pub mod help;
pub mod history;
pub mod home;
pub mod import_claude;
pub mod jump;
pub mod login;
pub mod logout;
pub mod loop_cmd;
pub mod mcps;
pub mod model;
pub mod multiline;
pub mod new;
pub mod permission;
pub mod personas;
pub mod plan;
pub mod plugin;
pub mod queue;
pub mod recap;
pub mod release_notes;
pub mod remember;
pub mod rename;
pub mod resume;
pub mod rewind;
pub mod screen_mode_switch;
pub mod scroll_debug;
pub mod session_info;
pub mod settings_cmd;
pub mod shortcuts;
pub mod tasks;
pub mod theme;
pub mod timeline;
pub mod timestamps;
pub mod toggle_mouse_reporting;
pub mod transcript;
pub mod tutorial;
pub mod usage;
pub mod view_plan;
pub mod vim_mode;
pub mod workflow;
pub mod workflows;
use super::command::SlashCommand;
use std::sync::Arc;
/// All pager-local builtin commands, in display order.
///
/// This is the single source of truth for the builtin command set.
/// The registry is constructed from this list.
pub fn builtin_commands() -> Vec<Arc<dyn SlashCommand>> {
    vec![
        Arc::new(exit::ExitCommand),
        Arc::new(help::HelpCommand),
        Arc::new(shortcuts::ShortcutsCommand),
        Arc::new(docs::DocsCommand),
        Arc::new(home::HomeCommand),
        Arc::new(new::NewCommand),
        Arc::new(fork::ForkCommand),
        Arc::new(compact::CompactCommand),
        Arc::new(copy::CopyCommand),
        Arc::new(find::FindCommand),
        Arc::new(history::HistoryCommand),
        Arc::new(export::ExportCommand),
        Arc::new(transcript::TranscriptCommand),
        Arc::new(edit_prompt::EditPromptCommand),
        Arc::new(expand::ExpandCommand),
        Arc::new(context::ContextCommand),
        // Screen-mode switchers: visible only in the opposite mode.
        Arc::new(screen_mode_switch::ScreenModeSwitchCommand::minimal()),
        Arc::new(screen_mode_switch::ScreenModeSwitchCommand::fullscreen()),
        Arc::new(model::ModelCommand),
        Arc::new(effort::EffortCommand),
        Arc::new(agent::AgentCommand),
        Arc::new(permission::PermissionCommand),
        Arc::new(permission::AskCommand),
        Arc::new(always_approve::AlwaysApproveCommand),
        Arc::new(auto::AutoCommand),
        Arc::new(behavior::BehaviorCommand),
        Arc::new(behavior::BehaviorShortcutCommand::normal()),
        Arc::new(behavior::BehaviorShortcutCommand::clarify()),
        Arc::new(multiline::MultilineCommand),
        Arc::new(compact_mode::CompactModeCommand),
        Arc::new(vim_mode::VimModeCommand),
        Arc::new(plugin::HooksCommand),
        Arc::new(plugin::PluginsCommand),
        Arc::new(plugin::MarketplaceCommand),
        Arc::new(plugin::SkillsCommand),
        Arc::new(session_info::SessionInfoCommand),
        Arc::new(rename::RenameCommand),
        Arc::new(dashboard::DashboardCommand),
        Arc::new(cd::CdCommand),
        Arc::new(theme::ThemeCommand),
        Arc::new(feedback::FeedbackCommand),
        Arc::new(announcements::AnnouncementsCommand),
        Arc::new(remember::RememberCommand),
        Arc::new(plan::PlanCommand),
        Arc::new(view_plan::ViewPlanCommand),
        Arc::new(resume::ResumeCommand),
        Arc::new(mcps::McpsCommand),
        Arc::new(workflows::WorkflowsCommand),
        Arc::new(workflow::WorkflowCommand),
        Arc::new(btw::BtwCommand),
        Arc::new(recap::RecapCommand),
        Arc::new(doctor::DoctorCommand),
        Arc::new(loop_cmd::LoopCommand),
        Arc::new(timestamps::TimestampsCommand),
        Arc::new(timeline::TimelineCommand),
        Arc::new(toggle_mouse_reporting::ToggleMouseReportingCommand),
        Arc::new(settings_cmd::SettingsCommand),
        Arc::new(rewind::RewindCommand),
        Arc::new(jump::JumpCommand),
        Arc::new(login::LoginCommand),
        Arc::new(logout::LogoutCommand),
        Arc::new(import_claude::ImportClaudeCommand),
        Arc::new(usage::UsageCommand),
        Arc::new(queue::QueueCommand),
        Arc::new(tasks::TasksCommand),
        Arc::new(release_notes::ReleaseNotesCommand),
        Arc::new(tutorial::TutorialCommand),
        Arc::new(config_agents::ConfigAgentsCommand),
        Arc::new(personas::PersonasCommand),
        // Hidden easter egg: never listed, runs on bare `/gboom`.
        Arc::new(gboom::GboomCommand),
        // Hidden diagnostic: never listed, toggles the scroll-debug HUD.
        Arc::new(scroll_debug::ScrollDebugCommand),
        // Debug toggles: always registered, listed only on debug binaries.
        Arc::new(debug::DebugCommand),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::app::actions::Action;
    use crate::slash::command::{CommandExecCtx, CommandResult};
    use crate::slash::registry::CommandRegistry;
    use agent_client_protocol as acp;
    /// Build a ModelState with two models for testing.
    fn sample_models() -> ModelState {
        let mut models = ModelState::default();
        let id_fast = acp::ModelId::new(Arc::from("grow-4.5"));
        models.available.insert(
            id_fast.clone(),
            acp::ModelInfo::new(id_fast.clone(), "Grow 4.5".to_string()),
        );
        let id_pro = acp::ModelId::new(Arc::from("grow-4.3"));
        models.available.insert(
            id_pro.clone(),
            acp::ModelInfo::new(id_pro.clone(), "Grow 4.3".to_string()),
        );
        models.current = Some(id_fast);
        models
    }
    static DEFAULT_BUNDLE_STATE: crate::app::bundle::BundleState =
        crate::app::bundle::BundleState {
            has_cache: false,
            version: String::new(),
            personas: Vec::new(),
            roles: Vec::new(),
            agents: Vec::new(),
            skills: Vec::new(),
            persona_details: Vec::new(),
            role_details: Vec::new(),
        };
    pub(crate) fn make_ctx(models: &ModelState) -> CommandExecCtx<'_> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: &DEFAULT_BUNDLE_STATE,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: crate::settings::PagerLocalSnapshot {
                multiline_mode: false,
                yolo_mode: false,
                ..crate::settings::PagerLocalSnapshot::default()
            },
        }
    }
    #[test]
    fn builtin_registry_lookup_by_canonical() {
        let mut reg = CommandRegistry::new(builtin_commands());
        assert!(reg.get("quit").is_some());
        assert!(reg.get("new").is_some());
        assert!(reg.get("compact").is_some());
        assert!(reg.get("model").is_some());
        assert!(reg.get("home").is_some());
        assert!(reg.get("view-plan").is_some());
        reg.set_available_tools(std::collections::HashSet::from([
            "scheduler_create".to_string()
        ]));
        assert!(reg.get("loop").is_some(), "/loop should be registered");
        assert!(
            reg.get("vim-mode").is_some(),
            "/vim-mode should be registered"
        );
        assert!(reg.get("find").is_some(), "/find should be registered");
    }
    #[test]
    fn loop_command_declares_scheduler_tool_requirement() {
        let loop_cmd = loop_cmd::LoopCommand;
        assert_eq!(loop_cmd.required_tools(), &["scheduler_create"]);
    }
    #[test]
    fn loop_command_hidden_when_scheduler_tools_absent() {
        let mut reg = CommandRegistry::new(builtin_commands());
        reg.set_available_tools(std::collections::HashSet::from([
            "read_file".to_string(),
            "grep".to_string(),
        ]));
        assert!(reg.get("loop").is_none(), "/loop should be hidden");
        assert!(reg.get("quit").is_some());
        assert!(reg.get("compact").is_some());
        reg.set_available_tools(std::collections::HashSet::from([
            "scheduler_create".to_string()
        ]));
        assert!(reg.get("loop").is_some());
    }
    #[test]
    fn shell_collision_contract_covers_every_pager_command_and_alias() {
        const SHELL_RESERVED: &[&str] = &[
            "agent",
            "agents",
            "always-approve",
            "announcements",
            "ask",
            "auto",
            "behavior",
            "btw",
            "cd",
            "changelog",
            "chat",
            "clear",
            "cloud",
            "clarify",
            "compact",
            "compact-mode",
            "config",
            "config-agents",
            "context",
            "copy",
            "cost",
            "debug",
            "docs",
            "doctor",
            "edit-prompt",
            "effort",
            "exit",
            "expand",
            "export",
            "feedback",
            "find",
            "fork",
            "full",
            "fullscreen",
            "gboom",
            "guides",
            "help",
            "history",
            "home",
            "hooks",
            "howto",
            "import-claude",
            "jump",
            "login",
            "logout",
            "log",
            "loop",
            "m",
            "marketplace",
            "mcps",
            "minimal",
            "ml",
            "model",
            "multiline",
            "new",
            "normal",
            "onboarding",
            "personas",
            "permission",
            "plan",
            "plan-view",
            "plugins",
            "preferences",
            "prefs",
            "queue",
            "quit",
            "recap",
            "release-notes",
            "remember",
            "rename",
            "resume",
            "rewind",
            "scroll-debug",
            "session-info",
            "settings",
            "show-plan",
            "shortcuts",
            "skills",
            "summarize",
            "tasks",
            "terminal-check",
            "terminal-info",
            "terminal-setup",
            "theme",
            "timeline",
            "timestamps",
            "title",
            "toggle-mouse-reporting",
            "tour",
            "transcript",
            "tutorial",
            "t",
            "usage",
            "view-plan",
            "vim-mode",
            "welcome",
            "workflow",
            "workflows",
            "yolo",
        ];
        for command in builtin_commands() {
            for key in std::iter::once(command.name()).chain(command.aliases().iter().copied()) {
                assert!(SHELL_RESERVED.contains(&key), "unreserved pager key {key}");
            }
        }
    }
    #[test]
    fn builtin_registry_lookup_by_alias() {
        let reg = CommandRegistry::new(builtin_commands());
        assert!(reg.get("exit").is_some());
        assert!(reg.get("clear").is_some());
        assert!(reg.get("m").is_some());
        assert!(reg.get("welcome").is_some());
        assert!(reg.get("show-plan").is_some());
        assert!(reg.get("plan-view").is_some());
        assert!(reg.get("?").is_none());
    }
    #[test]
    fn aliases_resolve_to_same_command() {
        let reg = CommandRegistry::new(builtin_commands());
        let exit_cmd = reg.get("exit").unwrap();
        let quit_cmd = reg.get("quit").unwrap();
        assert_eq!(exit_cmd.name(), quit_cmd.name());
        let doctor = reg.get("doctor").unwrap();
        assert_eq!(doctor.usage(), "/doctor [fix [FIX]]");
        for alias in ["terminal-setup", "terminal-check", "terminal-info"] {
            assert_eq!(reg.get(alias).unwrap().name(), doctor.name());
            assert_eq!(reg.get(alias).unwrap().usage(), doctor.usage());
        }
    }
    #[test]
    fn exit_returns_quit_action() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = exit::ExitCommand;
        let result = cmd.run(&mut ctx, "");
        assert!(matches!(result, CommandResult::Action(Action::Quit)));
    }
    #[test]
    fn new_returns_new_session_action() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = new::NewCommand;
        let result = cmd.run(&mut ctx, "");
        assert!(matches!(result, CommandResult::Action(Action::NewSession)));
    }
    #[test]
    fn home_returns_exit_session_action() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = home::HomeCommand;
        let result = cmd.run(&mut ctx, "");
        assert!(matches!(result, CommandResult::Action(Action::ExitSession)));
    }
    #[test]
    fn view_plan_returns_show_plan_action() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = view_plan::ViewPlanCommand;
        let result = cmd.run(&mut ctx, "");
        assert!(matches!(result, CommandResult::Action(Action::ShowPlan)));
    }
    #[test]
    fn compact_no_args_returns_queue_command() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = compact::CompactCommand;
        let result = cmd.run(&mut ctx, "");
        match result {
            CommandResult::QueueCommand(text) => assert_eq!(text, "/compact"),
            other => panic!("expected QueueCommand, got {other:?}"),
        }
    }
    #[test]
    fn compact_with_context_returns_queue_command_with_args() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = compact::CompactCommand;
        let result = cmd.run(&mut ctx, "focus on auth");
        match result {
            CommandResult::QueueCommand(text) => {
                assert_eq!(text, "/compact focus on auth")
            }
            other => panic!("expected QueueCommand, got {other:?}"),
        }
    }
    #[test]
    fn compact_whitespace_only_args_treated_as_no_args() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = compact::CompactCommand;
        let result = cmd.run(&mut ctx, "   ");
        match result {
            CommandResult::QueueCommand(text) => assert_eq!(text, "/compact"),
            other => panic!("expected QueueCommand, got {other:?}"),
        }
    }
    /// Display names are rejected; only catalog ids switch the session.
    #[test]
    fn model_rejects_display_name() {
        let models = sample_models();
        let mut ctx = make_ctx(&models);
        let cmd = model::ModelCommand;
        let result = cmd.run(&mut ctx, "Grow 4.5");
        assert!(
            matches!(result, CommandResult::Error(ref msg) if msg.contains("Unknown model")),
            "got {result:?}"
        );
    }
    #[test]
    fn model_resolves_by_model_id() {
        let models = sample_models();
        let mut ctx = make_ctx(&models);
        let cmd = model::ModelCommand;
        let result = cmd.run(&mut ctx, "grow-4.3");
        match result {
            CommandResult::Action(Action::SwitchModel {
                model_id: id,
                effort: None,
            }) => {
                assert_eq!(id.0.as_ref(), "grow-4.3");
            }
            other => panic!("expected Action(SwitchModel), got {other:?}"),
        }
    }
    #[test]
    fn model_resolves_case_insensitively() {
        let models = sample_models();
        let mut ctx = make_ctx(&models);
        let cmd = model::ModelCommand;
        let result = cmd.run(&mut ctx, "GROW-4.5");
        match result {
            CommandResult::Action(Action::SwitchModel {
                model_id: id,
                effort: None,
            }) => {
                assert_eq!(id.0.as_ref(), "grow-4.5");
            }
            other => panic!("expected Action(SwitchModel), got {other:?}"),
        }
    }
    #[test]
    fn model_invalid_arg_returns_error() {
        let models = sample_models();
        let mut ctx = make_ctx(&models);
        let cmd = model::ModelCommand;
        let result = cmd.run(&mut ctx, "nonexistent-model");
        match result {
            CommandResult::Error(msg) => {
                assert!(
                    msg.contains("nonexistent-model"),
                    "error should contain the arg"
                );
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }
    #[test]
    fn model_empty_arg_opens_picker() {
        let models = sample_models();
        let mut ctx = make_ctx(&models);
        let cmd = model::ModelCommand;
        let result = cmd.run(&mut ctx, "");
        assert!(matches!(
            result,
            CommandResult::Action(Action::OpenCommandPicker { ref command, .. })
                if command == "model"
        ));
    }
    #[test]
    fn model_whitespace_only_arg_opens_picker() {
        let models = sample_models();
        let mut ctx = make_ctx(&models);
        let cmd = model::ModelCommand;
        let result = cmd.run(&mut ctx, "   ");
        assert!(matches!(
            result,
            CommandResult::Action(Action::OpenCommandPicker { ref command, .. })
                if command == "model"
        ));
    }
    #[test]
    fn model_suggest_args_returns_available_models() {
        let models = sample_models();
        let ctx = crate::slash::command::AppCtx {
            models: &models,
            agents: &[],
            current_agent: None,
            behavior_mode: grow_tools::types::SessionMode::Default,
            deep_research_available: false,
            goal_available: false,
            auto_permission_available: false,
            current_permission: "ask",
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            workflows_available: true,
            screen_mode: crate::app::ScreenMode::Fullscreen,
        };
        let cmd = model::ModelCommand;
        let items = cmd.suggest_args(&ctx, "").expect("should have suggestions");
        assert_eq!(items.len(), 2);
        assert!(
            items
                .iter()
                .any(|i| i.display.starts_with("grow-4.5") && i.insert_text == "grow-4.5")
        );
        assert!(
            items
                .iter()
                .any(|i| i.display == "grow-4.3" && i.insert_text == "grow-4.3")
        );
    }
    #[test]
    fn model_suggest_args_empty_models_returns_none() {
        let models = ModelState::default();
        let ctx = crate::slash::command::AppCtx {
            models: &models,
            agents: &[],
            current_agent: None,
            behavior_mode: grow_tools::types::SessionMode::Default,
            deep_research_available: false,
            goal_available: false,
            auto_permission_available: false,
            current_permission: "ask",
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            workflows_available: true,
            screen_mode: crate::app::ScreenMode::Fullscreen,
        };
        let cmd = model::ModelCommand;
        assert!(cmd.suggest_args(&ctx, "").is_none());
    }
    #[test]
    fn remember_no_args_enters_remember_mode() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = remember::RememberCommand;
        let result = cmd.run(&mut ctx, "");
        assert!(matches!(
            result,
            CommandResult::Action(Action::EnterRememberMode)
        ));
    }
    #[test]
    fn remember_with_args_sends_note() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = remember::RememberCommand;
        let result = cmd.run(&mut ctx, "important detail");
        match result {
            CommandResult::Action(Action::SendRememberNote(text)) => {
                assert_eq!(text, "important detail");
            }
            other => panic!("expected SendRememberNote, got {other:?}"),
        }
    }
    #[test]
    fn remember_whitespace_only_enters_remember_mode() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = remember::RememberCommand;
        let result = cmd.run(&mut ctx, "   ");
        assert!(matches!(
            result,
            CommandResult::Action(Action::EnterRememberMode)
        ));
    }
    fn run_usage(args: &str) -> CommandResult {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        usage::UsageCommand.run(&mut ctx, args)
    }
    #[test]
    fn usage_is_local_and_takes_no_arguments() {
        assert!(matches!(
            run_usage(""),
            CommandResult::Action(Action::ShowUsage)
        ));
        assert!(matches!(run_usage("show"), CommandResult::Error(_)));
        assert!(matches!(run_usage("manage"), CommandResult::Error(_)));
    }
    #[test]
    fn usage_registered_in_builtin_commands() {
        assert!(
            CommandRegistry::new(builtin_commands())
                .get("usage")
                .is_some()
        );
    }
    #[test]
    fn cd_registered_in_builtin_commands() {
        let reg = CommandRegistry::new(builtin_commands());
        assert!(
            reg.get("cd").is_some(),
            "/cd should be registered in builtins"
        );
    }
    #[test]
    fn queue_registered_in_builtin_commands() {
        let reg = CommandRegistry::new(builtin_commands());
        assert!(
            reg.get("queue").is_some(),
            "/queue should be registered in builtins"
        );
    }
    #[test]
    fn tasks_registered_in_builtin_commands() {
        let reg = CommandRegistry::new(builtin_commands());
        assert!(
            reg.get("tasks").is_some(),
            "/tasks should be registered in builtins"
        );
    }
    #[test]
    fn cost_aliases_usage() {
        let reg = CommandRegistry::new(builtin_commands());
        assert_eq!(reg.get("cost").expect("/cost").name(), "usage");
    }
    #[test]
    fn debug_is_registered_and_executable() {
        let reg = CommandRegistry::new(builtin_commands());
        assert!(reg.get("debug").is_some(), "/debug must be executable");
    }
    #[test]
    fn gboom_is_registered_and_executable() {
        let reg = CommandRegistry::new(builtin_commands());
        assert!(reg.get("gboom").is_some(), "/gboom must be executable");
    }
    #[test]
    fn gboom_is_invisible() {
        let models = ModelState::default();
        let ctx = crate::slash::command::AppCtx {
            models: &models,
            agents: &[],
            current_agent: None,
            behavior_mode: grow_tools::types::SessionMode::Default,
            deep_research_available: false,
            goal_available: false,
            auto_permission_available: false,
            current_permission: "ask",
            cwd: std::path::Path::new("."),
            has_session_announcements: false,
            workflows_available: true,
            screen_mode: crate::app::ScreenMode::Fullscreen,
        };
        assert!(
            !gboom::GboomCommand.visible(&ctx),
            "/gboom must never appear in the dropdown"
        );
    }
    #[test]
    fn minimal_and_fullscreen_registered_in_builtin_commands() {
        let reg = CommandRegistry::new(builtin_commands());
        assert!(reg.get("minimal").is_some());
        assert!(reg.get("fullscreen").is_some());
        assert!(reg.get("full").is_some());
        assert_eq!(
            reg.get("full").unwrap().name(),
            reg.get("fullscreen").unwrap().name()
        );
    }
    #[test]
    fn recap_registered_in_builtin_commands() {
        let mut reg = CommandRegistry::new(builtin_commands());
        assert!(reg.get("recap").is_none());
        assert!(reg.get("summarize").is_none());
        reg.set_recap_visible(true);
        assert!(
            reg.get("recap").is_some(),
            "/recap should be registered in builtins"
        );
        assert_eq!(
            reg.get("summarize").map(|c| c.name()),
            Some("recap"),
            "/summarize should alias /recap"
        );
    }
    #[test]
    fn gboom_bare_invocation_opens_game() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let result = gboom::GboomCommand.run(&mut ctx, "");
        assert!(matches!(result, CommandResult::Action(Action::OpenGboom)));
        let result = gboom::GboomCommand.run(&mut ctx, "   ");
        assert!(matches!(result, CommandResult::Action(Action::OpenGboom)));
    }
    #[test]
    fn gboom_with_args_passes_through_to_shell() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        match gboom::GboomCommand.run(&mut ctx, "guide me") {
            CommandResult::PassThrough(text) => assert_eq!(text, "/gboom guide me"),
            other => panic!("expected PassThrough, got {other:?}"),
        }
    }
    #[test]
    fn recap_returns_manual_send_recap_action() {
        let models = ModelState::default();
        let mut ctx = make_ctx(&models);
        let cmd = recap::RecapCommand;
        let result = cmd.run(&mut ctx, "");
        assert!(matches!(
            result,
            CommandResult::Action(Action::SendRecap { auto: false })
        ));
    }
    #[test]
    fn recap_hidden_by_default_in_registry_until_revealed() {
        let mut reg = CommandRegistry::new(builtin_commands());
        assert!(
            reg.get("recap").is_none(),
            "/recap must be fail-closed until shell advertises sessionRecap"
        );
        reg.set_recap_visible(true);
        assert!(reg.get("recap").is_some());
        reg.set_recap_visible(false);
        assert!(reg.get("recap").is_none());
    }
}
