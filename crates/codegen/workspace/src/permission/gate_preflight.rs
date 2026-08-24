//! Managed-policy preflight for one permission request.
//!
//! Evaluates the direct rule pass and both bash security gates once and keeps
//! each gate's `Ask` provenance. Both a matched Ask and a fail-closed Ask bind
//! the request to a human prompt; parser uncertainty is never delegated to a
//! model classifier or a child permission mode.

use std::path::Path;

use crate::permission::manager::reasons;
use crate::permission::policy::{CompiledPolicy, GateDecision};
use crate::permission::shell_access::combine_decisions;
use crate::permission::types::{AccessKind, Decision};

/// One request's managed-policy evaluation, computed before any fast path.
pub(crate) struct GatePreflight {
    direct: Option<Decision>,
    bash_command: Option<GateDecision>,
    shell_file: Option<GateDecision>,
}

impl GatePreflight {
    pub(crate) fn evaluate(
        policy: Option<&CompiledPolicy>,
        access: &AccessKind,
        cwd: &Path,
    ) -> Self {
        let direct = policy.and_then(|policy| policy.evaluate(access));
        let (bash_command, shell_file) = match (policy, access) {
            (Some(policy), AccessKind::Bash(cmd)) => (
                policy.evaluate_bash_command_gate(cmd),
                policy.evaluate_shell_file_access_gate(cmd, cwd),
            ),
            _ => (None, None),
        };
        Self {
            direct,
            bash_command,
            shell_file,
        }
    }

    /// Combined managed decision (deny > ask > allow), as the manager applied
    /// it before provenance existed.
    pub(crate) fn policy_decision(&self) -> Option<Decision> {
        let bash_command = self.bash_command.clone().map(GateDecision::into_decision);
        let shell_file = self.shell_file.clone().map(GateDecision::into_decision);
        combine_decisions(
            combine_decisions(self.direct.clone(), bash_command),
            shell_file,
        )
    }

    pub(crate) fn policy_forced_prompt(&self) -> bool {
        matches!(self.policy_decision(), Some(Decision::Ask))
    }

    /// An `Ask` from either bash gate; blocks the always-approve fast path.
    pub(crate) fn shell_forced_prompt(&self) -> bool {
        self.bash_command.as_ref().is_some_and(GateDecision::is_ask)
            || self.shell_file_forced_prompt()
    }

    /// Blocks bash grants from satisfying a Read/Edit ask escalated from
    /// shell-file access.
    pub(crate) fn shell_file_forced_prompt(&self) -> bool {
        self.shell_file.as_ref().is_some_and(GateDecision::is_ask)
    }

    /// The auto classifier is only admissible when managed policy has not
    /// required a prompt. This includes parser/gate uncertainty.
    pub(crate) fn admits_auto_classifier(&self) -> bool {
        !self.policy_forced_prompt()
    }

    /// The gate-owned prompt trigger for diagnostics, or `None` when a bash floor
    /// or plain needs-user forced the prompt. Rule-match Asks keep their gate
    /// label; a deferrable Ask does not.
    pub(crate) fn prompt_trigger(
        &self,
        auto_prompt_reason: Option<&'static str>,
    ) -> Option<&'static str> {
        if matches!(self.direct, Some(Decision::Ask)) {
            return Some(reasons::POLICY_ASK);
        }
        if self.bash_command.as_ref().is_some_and(GateDecision::is_ask) {
            return Some(reasons::BASH_COMMAND_GATE_ASK);
        }
        if self.shell_file_forced_prompt() {
            return Some(reasons::SHELL_FILE_GATE_ASK);
        }
        auto_prompt_reason
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::types::{
        PatternMode, PermissionConfig, PermissionRule, RuleAction, ToolFilter,
    };

    fn bash_rule(action: RuleAction, pattern: &str) -> PermissionRule {
        PermissionRule {
            action,
            tool: ToolFilter::Bash,
            pattern: Some(pattern.to_owned()),
            pattern_mode: PatternMode::Glob,
        }
    }

    fn policy() -> CompiledPolicy {
        CompiledPolicy::new(PermissionConfig::new(vec![
            bash_rule(RuleAction::Deny, "rm -rf *"),
            bash_rule(RuleAction::Ask, "git push*"),
        ]))
    }

    #[test]
    fn preflight_reports_gate_state_coherently() {
        let policy = policy();
        let cwd = Path::new("/work");
        let bash = |cmd: &str| AccessKind::Bash(cmd.to_owned());

        // Gate uncertainty is a binding managed prompt in every session mode.
        let fail_closed = GatePreflight::evaluate(Some(&policy), &bash("echo \"$(date)\""), cwd);
        assert!(fail_closed.policy_forced_prompt());
        assert!(!fail_closed.admits_auto_classifier());
        assert_eq!(
            fail_closed.prompt_trigger(Some(reasons::AUTO_CLASSIFIER_BLOCK)),
            Some(reasons::BASH_COMMAND_GATE_ASK)
        );

        // Rule-match Ask in auto mode stays binding with its gate label — a
        // rule match never defers, even alongside a fail-closed floor.
        let rule_match = GatePreflight::evaluate(
            Some(&policy),
            &bash("echo hi && git push origin main"),
            cwd,
        );
        assert!(!rule_match.admits_auto_classifier());
        assert_eq!(
            rule_match.prompt_trigger(None),
            Some(reasons::BASH_COMMAND_GATE_ASK)
        );

        // No policy at all: inert preflight.
        let inert = GatePreflight::evaluate(None, &bash("echo hi"), cwd);
        assert!(inert.policy_decision().is_none());
        assert!(inert.admits_auto_classifier());
        assert_eq!(inert.prompt_trigger(None), None);
    }
}
