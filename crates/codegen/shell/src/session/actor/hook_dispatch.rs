//! Hook dispatch concern for `SessionActor`: run contexts, hook execution
//! notifications and diagnostics, and turn/tool outcome mapping.

use super::*;

/// Encode a [`CancellationCategory`](crate::session::events::CancellationCategory)
/// as its bare snake_case wire string for the `after_turn` hook payload.
/// Deliberately `serde_json::to_value` + `as_str`, NOT `to_string` — the
/// latter yields the quoted form and fails the workspace decode.
pub(super) fn cancellation_category_to_wire_string(
    category: Option<crate::session::events::CancellationCategory>,
) -> Option<String> {
    let category = category?;
    serde_json::to_value(category)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
}

/// Returns `(notification_type, message, title, level)` when this update should
/// trigger a vendor-compatible `Notification` hook.
///
/// Internal/high-frequency updates (hook scrollback, retry progress, config
/// changes) are excluded so migrated hooks only fire on user-attention
/// events — not on every tool call or session tick.
#[allow(clippy::type_complexity)]
pub(super) fn notification_hook_for_update(
    update: &GrowSessionUpdate,
) -> Option<(String, Option<String>, Option<String>, Option<String>)> {
    match update {
        GrowSessionUpdate::DiffReview { .. } => Some((
            "permission_prompt".into(),
            Some("Diff review requested".into()),
            None,
            Some("info".into()),
        )),
        GrowSessionUpdate::RetryState(RetryState::Exhausted { reason, .. }) => Some((
            "agent_error".into(),
            Some(reason.clone()),
            None,
            Some("error".into()),
        )),
        GrowSessionUpdate::RetryState(RetryState::Failed { message, .. }) => Some((
            "agent_error".into(),
            Some(message.clone()),
            None,
            Some("error".into()),
        )),
        _ => None,
    }
}

impl SessionActor {
    pub(super) fn hook_run_ctx(&self) -> ::hooks::runner::RunContext<'_> {
        ::hooks::runner::RunContext {
            session_id: &self.session_info.id.0,
            workspace_root: &self.hooks.resolved_workspace_root,
            process_scope: self.tool_context.process_scope.clone(),
        }
    }

    /// Send a hook annotation to the TUI scrollback.
    ///
    /// Rendered inline with the preceding tool call block rather than as
    /// a separate agent message.
    pub(super) async fn send_hook_annotation(&self, message: &str) {
        self.send_grow_notification(GrowSessionUpdate::HookAnnotation {
            message: message.to_string(),
        })
        .await;
    }

    /// Send structured hook execution data for rich scrollback rendering.
    ///
    /// `prompt_id` is `None` for session-level dispatches (session_start /
    /// session-end stop).
    pub(super) async fn send_hook_execution(
        &self,
        event_name: &str,
        tool_name: Option<&str>,
        prompt_id: Option<&str>,
        results: &[::hooks::result::HookRunResult],
    ) {
        if results.is_empty()
            || results
                .iter()
                .all(|r| matches!(r, ::hooks::result::HookRunResult::Skipped { .. }))
        {
            return;
        }
        use crate::extensions::notification::{HookRunEntryDto, HookRunStatusDto};
        use ::hooks::result::HookRunResult;

        let runs: Vec<HookRunEntryDto> = results
            .iter()
            .map(|r| {
                let (name, status) = match r {
                    HookRunResult::Success {
                        hook_name, elapsed, ..
                    } => (
                        hook_name.clone(),
                        HookRunStatusDto::Success {
                            elapsed_ms: elapsed.as_millis() as u64,
                        },
                    ),
                    HookRunResult::Skipped { hook_name } => {
                        (hook_name.clone(), HookRunStatusDto::Skipped)
                    }
                    HookRunResult::Blocked {
                        hook_name,
                        detail,
                        elapsed,
                        ..
                    } => (
                        hook_name.clone(),
                        HookRunStatusDto::Blocked {
                            detail: detail.clone(),
                            elapsed_ms: elapsed.as_millis() as u64,
                        },
                    ),
                    HookRunResult::Failed {
                        hook_name,
                        error,
                        elapsed,
                        ..
                    } => (
                        hook_name.clone(),
                        HookRunStatusDto::Failed {
                            error: error.clone(),
                            elapsed_ms: elapsed.as_millis() as u64,
                        },
                    ),
                };
                HookRunEntryDto {
                    name,
                    status,
                    output: None,
                }
            })
            .collect();

        self.send_grow_notification(GrowSessionUpdate::HookExecution {
            event_name: event_name.to_string(),
            tool_name: tool_name.map(|s| s.to_string()),
            prompt_id: prompt_id.map(|s| s.to_string()),
            runs,
        })
        .await;
    }

    /// Returns the resolved workspace root for hook envelopes.
    pub(super) fn hook_workspace_root(&self) -> String {
        self.hooks.resolved_workspace_root.clone()
    }

    /// Subagent type for tool-hook attribution, or `None` for the top-level session. Prefers
    /// the task `subagent_type`, falling back to the agent definition name for older spawns.
    pub(super) fn subagent_type_label(&self) -> Option<String> {
        if !self.startup_hints.is_subagent {
            return None;
        }
        Some(
            self.startup_hints
                .subagent_type
                .clone()
                .unwrap_or_else(|| self.agent.borrow().definition().name.clone()),
        )
    }

    /// The session's current permission mode. Behavior is intentionally
    /// independent and never masquerades as an approval policy.
    pub(super) fn permission_mode_label(&self) -> &'static str {
        let request_mode = self.startup_hints.permission_request_mode();
        match self.permissions.effective_request_mode(request_mode) {
            workspace::permission::types::EffectivePermissionMode::AlwaysApprove => {
                "always-approve"
            }
            workspace::permission::types::EffectivePermissionMode::Auto => "auto",
            workspace::permission::types::EffectivePermissionMode::Ask => "ask",
        }
    }

    /// Dispatch a non-blocking hook event: build the envelope, fire observe-only
    /// client hooks, then run the on-disk registry. No-op (no payload built) when no
    /// hook listens for `event`, so it stays inert when unused.
    pub(super) async fn dispatch_hook(
        &self,
        event: ::hooks::event::HookEventName,
        payload: ::hooks::event::HookPayload,
        prompt_id: Option<&str>,
        tool_name: Option<&str>,
    ) {
        if !self.hook_event_active(event) {
            return;
        }
        // Fires observe-only client hooks before (and independent of) the on-disk registry guard below.
        let envelope = self.fire_hook(event, prompt_id.map(|s| s.to_string()), payload);
        let Some(registry) = self.hooks.registry.borrow().clone() else {
            return;
        };
        let ctx = self.hook_run_ctx();
        let results =
            ::hooks::dispatcher::dispatch_non_blocking(&registry, event, &envelope, &ctx).await;
        self.send_hook_execution(&event.to_string(), tool_name, prompt_id, &results)
            .await;
        self.emit_hook_executed_diagnostics(&event.to_string(), tool_name, &results)
            .await;
    }

    pub(super) async fn emit_hook_executed_diagnostics(
        &self,
        event_name: &str,
        tool_name: Option<&str>,
        results: &[::hooks::result::HookRunResult],
    ) {
        let tool = tool_name.map(|s| s.to_string());
        for r in results {
            let (hook_name, elapsed, outcome) = match r {
                ::hooks::result::HookRunResult::Success {
                    hook_name, elapsed, ..
                } => (
                    hook_name,
                    elapsed,
                    ::diagnostics::events::HookOutcome::Success,
                ),
                ::hooks::result::HookRunResult::Blocked {
                    hook_name, elapsed, ..
                } => (
                    hook_name,
                    elapsed,
                    ::diagnostics::events::HookOutcome::Blocked,
                ),
                ::hooks::result::HookRunResult::Failed {
                    hook_name, elapsed, ..
                } => (
                    hook_name,
                    elapsed,
                    ::diagnostics::events::HookOutcome::Error,
                ),
                ::hooks::result::HookRunResult::Skipped { .. } => continue,
            };
            ::diagnostics::session_ctx::log_event(::diagnostics::events::HookExecuted {
                hook_name: hook_name.clone(),
                event: event_name.to_string(),
                tool_name: tool.clone(),
                duration_ms: elapsed.as_millis() as u64,
                outcome,
            });
        }
    }
}

#[cfg(test)]
mod notification_hook_filter_tests {
    use super::*;
    use crate::extensions::notification::{HookRunEntryDto, HookRunStatusDto, RetryState};

    #[test]
    fn hook_updates_do_not_fire_notification_hook() {
        let execution = GrowSessionUpdate::HookExecution {
            event_name: "pre_tool_use".into(),
            tool_name: Some("read_file".into()),
            prompt_id: None,
            runs: vec![HookRunEntryDto {
                name: "test".into(),
                status: HookRunStatusDto::Success { elapsed_ms: 1 },
                output: None,
            }],
        };
        assert!(notification_hook_for_update(&execution).is_none());

        let annotation = GrowSessionUpdate::HookAnnotation {
            message: "running hooks".into(),
        };
        assert!(notification_hook_for_update(&annotation).is_none());
    }

    #[test]
    fn retry_in_progress_does_not_fire_notification_hook() {
        let update = GrowSessionUpdate::RetryState(RetryState::Retrying {
            attempt: 1,
            max_retries: 3,
            reason: "timeout".into(),
        });
        assert!(notification_hook_for_update(&update).is_none());
    }

    #[test]
    fn retry_exhaustion_fires_one_agent_error_hook() {
        let update = GrowSessionUpdate::RetryState(RetryState::Exhausted {
            attempts: 3,
            reason: "completion requirements were not met".into(),
            is_rate_limited: false,
        });

        let (ty, message, title, level) =
            notification_hook_for_update(&update).expect("terminal retry failure should fire");
        assert_eq!(ty, "agent_error");
        assert_eq!(
            message.as_deref(),
            Some("completion requirements were not met")
        );
        assert_eq!(title, None);
        assert_eq!(level.as_deref(), Some("error"));
    }

    #[test]
    fn diff_review_fires_permission_prompt() {
        let update = GrowSessionUpdate::DiffReview { content: vec![] };
        let (ty, message, _, level) = notification_hook_for_update(&update).expect("should fire");
        assert_eq!(ty, "permission_prompt");
        assert_eq!(message.as_deref(), Some("Diff review requested"));
        assert_eq!(level.as_deref(), Some("info"));
    }

    #[test]
    fn task_completed_does_not_fire_via_filter() {
        let update = GrowSessionUpdate::TaskCompleted {
            task_snapshot: tools::types::TaskSnapshot {
                task_id: "task-1".into(),
                command: "echo hi".into(),
                display_command: None,
                cwd: "/tmp".into(),
                start_time: std::time::SystemTime::UNIX_EPOCH,
                end_time: None,
                output: String::new(),
                output_file: std::path::PathBuf::from("/tmp/out"),
                truncated: false,
                exit_code: Some(0),
                signal: None,
                completed: true,
                kind: Default::default(),
                block_waited: false,
                explicitly_killed: false,
                owner_session_id: None,
                description: None,
                is_backgrounded: false,
            },
        };
        assert!(notification_hook_for_update(&update).is_none());
    }
}
