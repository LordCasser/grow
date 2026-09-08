//! `/doctor` — diagnose terminal, color/theme, and clipboard behavior.
//!
//! Runs the shared TUI probe and diagnostics path, including live runtime
//! evidence that the standalone command cannot observe.

use crate::slash::command::{
    AppCtx, ArgItem, CommandExecCtx, CommandResult, DoctorRequest, SlashCommand,
};

const USAGE: &str = "Usage: /doctor [fix [ssh-wrap|tmux-clipboard|dcs-passthrough|tmux-extended-keys] [--config PATH]]";

pub struct DoctorCommand;

impl DoctorCommand {
    pub(crate) fn report_for_terminal(
        terminal: &crate::terminal::TerminalContext,
        evidence: crate::diagnostics::probes::TuiProbeEvidence<'_>,
        runtime: crate::diagnostics::TuiRuntimeRequest<'_>,
    ) -> crate::diagnostics::DiagnosticReport {
        let query = crate::diagnostics::probes::LiveTmuxProbe;
        let snapshot = crate::diagnostics::probes::collect_doctor_tui(terminal, evidence, &query);
        let runtime_findings = crate::diagnostics::collect_tui_runtime_findings(
            &snapshot.common,
            runtime.notification_method,
            runtime.notification_protocol,
            runtime.notification_condition,
            runtime.workspace,
        );
        let agent_findings =
            crate::diagnostics::collect_agent_definition_findings(runtime.workspace);
        let mut report = crate::diagnostics::view(snapshot.into());
        crate::diagnostics::merge_tui_runtime_findings(&mut report, runtime_findings);
        crate::diagnostics::merge_tui_runtime_findings(&mut report, agent_findings);
        crate::diagnostics::configure_doctor_report(report, terminal)
    }
}

impl SlashCommand for DoctorCommand {
    fn name(&self) -> &str {
        "doctor"
    }

    fn description(&self) -> &str {
        "Check this session and show available fixes"
    }

    fn usage(&self) -> &str {
        "/doctor [fix [FIX] [--config PATH]]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("[fix [FIX]]")
    }

    fn suggest_args(&self, _ctx: &AppCtx, args_query: &str) -> Option<Vec<ArgItem>> {
        let query = args_query.trim();
        if query.is_empty() {
            return None;
        }
        if query == "fix" || query.starts_with("fix ") {
            let value = query.strip_prefix("fix").unwrap_or_default().trim();
            if !value.is_empty() && crate::diagnostics::resolve_fix_id(value).is_ok() {
                return None;
            }
            let items = crate::diagnostics::automatic_fix_choices()
                .filter(|(id, handle, _)| {
                    value.is_empty() || handle.contains(value) || id.to_string().starts_with(value)
                })
                .map(|(id, handle, label)| ArgItem {
                    display: handle.into(),
                    match_text: format!("fix {handle} {id}"),
                    insert_text: format!("fix {handle}"),
                    description: label.into(),
                })
                .collect::<Vec<_>>();
            return (!items.is_empty()).then_some(items);
        }
        Some(vec![ArgItem {
            display: "fix".into(),
            match_text: "fix".into(),
            insert_text: "fix".into(),
            description: "Show automatic fixes available here".into(),
        }])
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let Some(tokens) = shlex::split(args) else {
            return CommandResult::Error(format!("Unclosed quote in doctor arguments.\n{USAGE}"));
        };
        let words = tokens.iter().map(String::as_str).collect::<Vec<_>>();
        let (value, config) = match words.as_slice() {
            [] => return CommandResult::Doctor(DoctorRequest::Report),
            ["fix"] => return CommandResult::Doctor(DoctorRequest::ListFixes),
            ["fix", value] => (*value, None),
            ["fix", value, "--config", path] if !path.is_empty() => {
                (*value, Some(std::path::PathBuf::from(path)))
            }
            _ => return CommandResult::Error(USAGE.to_owned()),
        };
        match crate::diagnostics::resolve_fix_id(value) {
            Ok(id) if config.is_some() && id == crate::diagnostics::SSH_WRAP_ID => {
                CommandResult::Error("--config is only supported for tmux fixes.".to_owned())
            }
            Ok(id) => CommandResult::Doctor(DoctorRequest::Fix(id, config)),
            Err(error) => CommandResult::Error(format!("{error}\n{USAGE}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::app::bundle::BundleState;

    fn run(args: &str) -> CommandResult {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut context = CommandExecCtx {
            models: &models,
            session_id: None,
            bundle_state: &bundle,
            screen_mode: crate::app::ScreenMode::Inline,
            pager_state: crate::settings::PagerLocalSnapshot::default(),
        };
        DoctorCommand.run(&mut context, args)
    }

    #[test]
    fn parses_report_list_short_and_canonical_fix_forms() {
        assert!(matches!(
            run(""),
            CommandResult::Doctor(DoctorRequest::Report)
        ));
        assert!(matches!(
            run("fix"),
            CommandResult::Doctor(DoctorRequest::ListFixes)
        ));
        for (value, id) in [
            ("ssh-wrap", crate::diagnostics::SSH_WRAP_ID),
            ("terminal.ssh-wrap", crate::diagnostics::SSH_WRAP_ID),
            ("tmux-clipboard", crate::diagnostics::TMUX_CLIPBOARD_ID),
            (
                "terminal.tmux-clipboard",
                crate::diagnostics::TMUX_CLIPBOARD_ID,
            ),
            ("dcs-passthrough", crate::diagnostics::DCS_PASSTHROUGH_ID),
            (
                "terminal.dcs-passthrough",
                crate::diagnostics::DCS_PASSTHROUGH_ID,
            ),
            (
                "tmux-extended-keys",
                crate::diagnostics::TMUX_EXTENDED_KEYS_ID,
            ),
            (
                "terminal.tmux-extended-keys",
                crate::diagnostics::TMUX_EXTENDED_KEYS_ID,
            ),
        ] {
            assert!(matches!(
                run(&format!("fix {value}")),
                CommandResult::Doctor(DoctorRequest::Fix(parsed, None)) if parsed == id
            ));
        }
    }

    #[test]
    fn rejects_unknown_and_extra_arguments() {
        for value in ["unknown", "fix unknown", "fix ssh-wrap extra", "report now"] {
            assert!(matches!(run(value), CommandResult::Error(message) if message.contains(USAGE)));
        }
    }

    #[test]
    fn completion_stays_closed_until_an_argument_starts() {
        let models = ModelState::default();
        let context = AppCtx {
            models: &models,
            agents: &[],
            current_agent: None,
            behavior_mode: tools::types::BehaviorId::Normal,
            goal_available: false,
            current_goal_objective: None,
            auto_permission_available: false,
            current_permission: "ask",
            cwd: std::path::Path::new("/tmp"),
            has_session_announcements: false,
            workflows_available: false,
            screen_mode: crate::app::ScreenMode::Inline,
        };
        let command = DoctorCommand;
        assert!(command.suggest_args(&context, "").is_none());
        assert!(command.suggest_args(&context, "   ").is_none());
        assert_eq!(
            command.suggest_args(&context, "f").unwrap()[0].insert_text,
            "fix"
        );
        for query in ["fix", "fix ", "fix s", "fix ssh", "fix terminal."] {
            assert_eq!(
                command.suggest_args(&context, query).unwrap()[0].insert_text,
                "fix ssh-wrap"
            );
        }
        for query in [
            "fix ssh-wrap",
            " fix ssh-wrap ",
            "fix terminal.ssh-wrap",
            "  fix terminal.ssh-wrap  ",
            "fix tmux-clipboard",
            "fix terminal.tmux-clipboard",
            "fix dcs-passthrough",
            "fix terminal.dcs-passthrough",
            "fix tmux-extended-keys",
            "fix terminal.tmux-extended-keys",
        ] {
            assert!(command.suggest_args(&context, query).is_none(), "{query:?}");
        }
    }
    #[test]
    fn explicit_tmux_target_preserves_quoted_path() {
        assert!(matches!(
            run("fix tmux-clipboard --config '/tmp/custom config.tmux'"),
            CommandResult::Doctor(DoctorRequest::Fix(id, Some(path)))
                if id == crate::diagnostics::TMUX_CLIPBOARD_ID && path == std::path::Path::new("/tmp/custom config.tmux")
        ));
        for input in [
            "fix tmux-clipboard --config",
            "fix tmux-clipboard --config ''",
            "fix tmux-clipboard --config 'unterminated",
            "fix ssh-wrap --config /tmp/config",
            "fix tmux-clipboard --config /tmp/a --config /tmp/b",
        ] {
            assert!(matches!(run(input), CommandResult::Error(_)), "{input}");
        }
    }
}
