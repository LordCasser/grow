//! `/agents` — open the Agent Dashboard view.
//!
//! Centralised overview of every running session (top-level + subagents)
//! with peek, attach, and dispatch from one screen. The dashboard reuses
//! the existing fullscreen subagent takeover for "attach to subagent",
//! so attaching never bypasses `active_subagent`.
//!
//! Same `Action`-only run path as other session-less commands, no args.
//! Visibility is feature-flag-gated: the
//! command is hidden by default in the registry and revealed when the
//! dashboard feature is enabled (`dashboard_enabled()`), via
//! [`crate::app::agent_view::AgentView::set_dashboard_visible`]. When
//! `[dashboard].enabled = false` or `GROW_AGENT_DASHBOARD=0` is set, the
//! dispatcher prints a friendly toast and refuses to open. The dashboard is
//! independent of leader mode.

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};
use crate::slash::{ModeSupport, Remedy};

/// Open the Agent Dashboard view.
pub struct DashboardCommand;

impl SlashCommand for DashboardCommand {
    fn name(&self) -> &str {
        "agents"
    }

    fn description(&self) -> &str {
        "Open the Agent Dashboard — a fullscreen overview of every running session"
    }

    fn usage(&self) -> &str {
        "/agents"
    }

    /// The agent dashboard is intentionally out of scope in minimal mode
    /// (single-session standalone — K14/§6.15).
    fn mode_support(&self) -> ModeSupport {
        ModeSupport::FullscreenOnly(Remedy::SwitchMode {
            why: "minimal is single-session",
        })
    }

    fn run(&self, _ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        CommandResult::Action(Action::OpenDashboard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::app::bundle::BundleState;
    use crate::slash::command::{AppCtx, CommandExecCtx, CommandResult};

    #[test]
    fn run_returns_open_dashboard_action() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut ctx = CommandExecCtx {
            models: &models,
            session_id: None,
            bundle_state: &bundle,
            screen_mode: crate::app::ScreenMode::Inline,
            billing_surface_visible: true,
            usage_command_visible: true,
            pager_state: crate::settings::PagerLocalSnapshot {
                multiline_mode: false,
                yolo_mode: false,
                ..crate::settings::PagerLocalSnapshot::default()
            },
        };
        let cmd = DashboardCommand;
        assert!(matches!(
            cmd.run(&mut ctx, ""),
            CommandResult::Action(Action::OpenDashboard)
        ));
    }

    /// Feature-flag gating is applied externally by the registry
    /// (`set_dashboard_visible`), not via `visible()` — `AppCtx` carries no
    /// dashboard state. `visible()` only gates on screen mode: offered in
    /// fullscreen/inline, hidden from the minimal-mode dropdown (where the
    /// dashboard has nothing to open and dispatch would just refuse).
    #[test]
    fn visible_everywhere_except_minimal() {
        let models = ModelState::default();
        let cmd = DashboardCommand;
        let ctx = |screen_mode| AppCtx {
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
            billing_surface_visible: true,
            usage_command_visible: true,
            workflows_available: true,
            screen_mode,
        };
        assert!(cmd.visible(&ctx(crate::app::ScreenMode::Fullscreen)));
        assert!(cmd.visible(&ctx(crate::app::ScreenMode::Inline)));
        assert!(
            matches!(cmd.mode_support(), ModeSupport::FullscreenOnly(_)),
            "the Agent Dashboard must not be offered in minimal mode"
        );
    }

    #[test]
    fn does_not_take_args() {
        let cmd = DashboardCommand;
        assert!(!cmd.takes_args());
    }

    #[test]
    fn name_is_agents() {
        let cmd = DashboardCommand;
        assert_eq!(cmd.name(), "agents");
    }

    #[test]
    fn has_no_legacy_aliases() {
        let cmd = DashboardCommand;
        assert!(cmd.aliases().is_empty());
    }
}
