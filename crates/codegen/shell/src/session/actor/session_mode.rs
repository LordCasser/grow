//! Session Behavior transitions, reminders, and persistence.
use super::*;
use crate::session::behavior::BehaviorChangeOutcome;

/// Separates the durable Behavior disposition from cleanup that happens only
/// after that disposition is committed. A failed old-turn cancellation is a
/// session-fatal causal error, but it cannot rewrite an already-Applied
/// control receipt as Rejected.
struct BehaviorApplication {
    disposition: Result<BehaviorChangeOutcome, acp::Error>,
    post_commit_fatal: Option<acp::Error>,
    cancelled_by_shutdown: bool,
}

impl BehaviorApplication {
    fn disposition(disposition: Result<BehaviorChangeOutcome, acp::Error>) -> Self {
        Self {
            disposition,
            post_commit_fatal: None,
            cancelled_by_shutdown: false,
        }
    }

    fn cancelled_by_shutdown() -> Self {
        Self {
            disposition: Err(acp::Error::internal_error().data("session is shutting down")),
            post_commit_fatal: None,
            cancelled_by_shutdown: true,
        }
    }
}

/// Plan is a frozen, human-approved execution protocol. The Workflow
/// launcher is therefore not advertised in any Plan phase; the runtime gate
/// remains as defense in depth for stale or forged calls.
pub(super) fn filter_cursor_tools_by_plan_mode(
    defs: Vec<ToolDefinition>,
    plan_active: bool,
) -> Vec<ToolDefinition> {
    if !plan_active {
        return defs;
    }
    defs.into_iter()
        .filter(|def| {
            def.function.name != tools::implementations::grow_build::workflow::WORKFLOW_TOOL_NAME
        })
        .collect()
}

#[cfg(test)]
mod desired_state_tests {
    use super::*;

    #[tokio::test]
    async fn behavior_admission_keeps_only_the_latest_target() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = super::super::tests::support::build_actor().await;

                let (first_tx, first_rx) = tokio::sync::oneshot::channel();
                assert!(
                    actor
                        .admit_behavior_selection(
                            acp::SessionModeId::new("clarify"),
                            None,
                            first_tx,
                        )
                        .await,
                    "the first desired target owns worker startup"
                );

                let (final_tx, mut final_rx) = tokio::sync::oneshot::channel();
                assert!(
                    !actor
                        .admit_behavior_selection(acp::SessionModeId::new("plan"), None, final_tx)
                        .await,
                    "a later target reuses the active worker"
                );

                assert_eq!(
                    first_rx.await.unwrap().unwrap(),
                    BehaviorChangeOutcome::Superseded
                );
                assert!(matches!(
                    final_rx.try_recv(),
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                ));

                let admission = actor.state.lock().await;
                let pending = admission
                    .pending_behavior_control
                    .as_ref()
                    .expect("latest behavior target");
                assert_eq!(pending.session_mode.0.as_ref(), "plan");
                assert_eq!(pending.revision, admission.behavior_control_revision);
            })
            .await;
    }

    #[tokio::test]
    async fn claimed_behavior_control_cancelled_by_shutdown_is_not_fatal() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = super::super::tests::support::build_actor().await;
                let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                assert!(
                    actor
                        .admit_behavior_selection(
                            acp::SessionModeId::new("clarify"),
                            None,
                            response_tx,
                        )
                        .await
                );
                actor
                    .state
                    .lock()
                    .await
                    .termination
                    .request(TerminationState::Graceful);
                let (completion_tx, _completion_rx) = tokio::sync::mpsc::unbounded_channel();

                assert!(
                    actor
                        .clone()
                        .drain_behavior_selections(completion_tx)
                        .await
                        .is_ok(),
                    "a control claimed before the shutdown latch is a cancellation, not a fatal worker failure"
                );
                let error = response_rx
                    .await
                    .expect("control response")
                    .expect_err("shutdown cancels the claimed control");
                assert!(error.to_string().contains("shutting down"));
                let state = actor.state.lock().await;
                assert_eq!(state.termination, TerminationState::Graceful);
                assert!(!state.behavior_control_worker_active);
                assert!(state.applying_behavior_control.is_none());
                assert!(state.foreground.is_idle());
            })
            .await;
    }

    #[tokio::test]
    async fn behavior_admission_does_not_mutate_confirmation_before_commit_boundary() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = super::super::tests::support::build_actor().await;
                actor
                    .behavior
                    .lock()
                    .select_behavior(tools::types::BehaviorId::Plan);
                let decision = actor.behavior.lock().decide_switch(
                    tools::types::BehaviorId::Normal,
                    crate::session::behavior::BehaviorSwitchFacts {
                        source_owned_work_active: true,
                        ..Default::default()
                    },
                    std::time::Duration::from_secs(8),
                );
                assert!(matches!(
                    decision.outcome,
                    BehaviorChangeOutcome::ConfirmationRequired { .. }
                ));

                let commit_boundary = actor.state.lock().await;
                let replacement_actor = std::sync::Arc::clone(&actor);
                let (responds_to, _response) = tokio::sync::oneshot::channel();
                let replacement = tokio::task::spawn_local(async move {
                    replacement_actor
                        .admit_behavior_selection(
                            acp::SessionModeId::new("clarify"),
                            None,
                            responds_to,
                        )
                        .await
                });
                tokio::task::yield_now().await;
                assert!(
                    actor.behavior.lock().pending_switch().is_some(),
                    "a request waiting outside the commit boundary must not clear its latch"
                );

                drop(commit_boundary);
                assert!(replacement.await.expect("replacement task"));
                assert!(
                    actor.behavior.lock().pending_switch().is_none(),
                    "the replacement clears the old latch once its revision is admitted"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn picker_projection_and_admission_share_the_host_foreground_rejection() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (actor, _gateway_rx) = super::super::tests::support::build_actor().await;
                *actor.agent.borrow_mut() =
                    super::super::tests::support::test_agent_with_plan_tools().await;
                let mut task = super::super::tests::support::running_task_stub("host-command");
                task.origin = crate::session::PromptOrigin::HostCommand;
                task.turn_kind = crate::session::TurnKind::Internal;
                actor.state.lock().await.foreground = ForegroundState::RegularTurn(task);

                let projection = actor.behavior_availability_projection().await;
                let choice = projection
                    .choice(tool_types::BehaviorId::Plan)
                    .cloned()
                    .expect("Plan projection");
                assert_eq!(
                    choice.disposition,
                    tool_types::BehaviorAvailabilityDisposition::Unavailable
                );
                let projected_reason = choice.reason.expect("busy reason");

                let application = actor
                    .apply_behavior_change_with_admission(
                        acp::SessionModeId::new("plan"),
                        crate::session::behavior::BehaviorRequestAuthority::Picker,
                        None,
                        Some("client-a:1"),
                        None,
                    )
                    .await;
                assert_eq!(
                    application.disposition.unwrap(),
                    BehaviorChangeOutcome::Rejected {
                        message: projected_reason
                    },
                    "a Picker cannot borrow an unrelated HostCommand foreground exception"
                );
                assert!(actor.behavior.lock().pending_switch().is_none());

                assert!(matches!(
                    actor
                        .request_behavior_change(acp::SessionModeId::new("plan"))
                        .await,
                    Ok(BehaviorChangeOutcome::Applied)
                ));
            })
            .await;
    }
}
impl SessionActor {
    pub(super) fn workflow_behavior_context(&self) -> Result<String, acp::Error> {
        crate::session::workflow::workspace::WorkflowWorkspace::compact_context_in_session_observational(
            &self.session_directory,
            std::path::Path::new(self.session_info.cwd.as_str()),
        )
        .map_err(|error| {
            acp::Error::internal_error()
                .data(format!("active Workflow workspace is unavailable: {error}"))
        })
    }

    pub(super) async fn behavior_capability_support(&self) -> (bool, bool, bool) {
        let bridge = self.agent.borrow().tool_bridge().clone();
        let tool_names: Vec<String> = bridge
            .tool_definitions_builtins_only()
            .await
            .into_iter()
            .map(|definition| definition.function.name)
            .collect();
        let plan_supported = bridge
            .tool_for_kind(tools::types::tool::ToolKind::PlanControl)
            .await
            .is_some();
        // Workflow is a Shell-owned control-plane runtime. Its availability
        // is the feature gate plus a live worker, not the selected Agent's
        // authored tool list.
        let workflow_supported = self.background_workflows_enabled
            && !self.startup_hints.is_subagent
            && !self.workflow_service_shutdown.is_cancelled();
        let goal_supported =
            super::goal_support::goal_runtime_available_from_tools(self.goal_enabled, &tool_names);
        (plan_supported, workflow_supported, goal_supported)
    }

    pub(super) fn capture_behavior_admission_facts(
        admission: &AdmissionState,
        expected_revision: Option<u64>,
    ) -> crate::session::behavior::BehaviorSwitchFacts {
        use crate::session::behavior::BehaviorForeground;

        let owns_behavior_foreground = expected_revision.is_some()
            && admission.behavior_control_foreground_claimed
            && matches!(admission.foreground, ForegroundState::ApplyingControl);
        let foreground = if owns_behavior_foreground {
            BehaviorForeground::BehaviorControl
        } else {
            match &admission.foreground {
                ForegroundState::Idle => BehaviorForeground::Idle,
                ForegroundState::RegularTurn(turn)
                    if turn.origin == crate::session::PromptOrigin::HostCommand =>
                {
                    BehaviorForeground::HostCommand
                }
                ForegroundState::RegularTurn(_) => BehaviorForeground::Regular,
                ForegroundState::ApplyingControl
                | ForegroundState::Settling { .. }
                | ForegroundState::Compaction => BehaviorForeground::Busy,
            }
        };
        crate::session::behavior::BehaviorSwitchFacts {
            termination_open: admission.termination.is_open(),
            pending_step_control: !admission.pending_step_controls.is_empty(),
            foreground,
            ..Default::default()
        }
    }

    pub(super) fn behavior_availability_from_tracker(
        &self,
        workflow_tracker: &crate::session::workflow::tracker::WorkflowTracker,
        (plan_supported, workflow_supported, goal_supported): (bool, bool, bool),
        authority: crate::session::behavior::BehaviorRequestAuthority,
        admission_facts: crate::session::behavior::BehaviorSwitchFacts,
    ) -> tool_types::BehaviorAvailability {
        use tool_types::{BehaviorAvailability, BehaviorId};

        let current = self.behavior.lock().behavior();
        let goal_status = self.goal_tracker.lock().status();
        let active_goal = goal_status == Some(crate::session::goal_tracker::GoalStatus::Active);
        let public_workflow_active = workflow_tracker.has_active_run();
        let source_owned_work_active = current == BehaviorId::Plan
            || (current == BehaviorId::Workflow && public_workflow_active);
        let controller = self.behavior.lock();
        let choices = [
            BehaviorId::Normal,
            BehaviorId::Clarify,
            BehaviorId::Plan,
            BehaviorId::Workflow,
            BehaviorId::Goal,
        ]
        .into_iter()
        .map(|target| {
            let unavailable_reason = match target {
                BehaviorId::Plan if !plan_supported => Some(
                    "Plan behavior is unavailable because PlanControl is not registered."
                        .to_string(),
                ),
                BehaviorId::Workflow if !workflow_supported => {
                    Some("Workflow behavior is unavailable in this session.".to_string())
                }
                BehaviorId::Goal if !goal_supported => {
                    Some("Goal behavior is unavailable in this session.".to_string())
                }
                BehaviorId::Goal
                    if goal_status.is_some_and(|status| {
                        status != crate::session::goal_tracker::GoalStatus::Active
                    }) => Some(match goal_status {
                        Some(crate::session::goal_tracker::GoalStatus::Complete) =>
                            "The saved Goal is complete. Use /goal edit to reactivate it, or /goal clear before starting another Goal."
                                .to_string(),
                        Some(crate::session::goal_tracker::GoalStatus::BudgetLimited) =>
                            "The saved Goal is budget-limited. Raise or remove its budget, then restart it."
                                .to_string(),
                        _ =>
                            "The saved Goal is stopped. Use /goal restart to reactivate it before selecting Goal Behavior."
                                .to_string(),
                    }),
                _ => None,
            };
            controller.assess_switch(
                target,
                &crate::session::behavior::BehaviorSwitchFacts {
                    unavailable_reason,
                    active_goal,
                    public_workflow_active,
                    source_owned_work_active,
                    ..admission_facts.clone()
                },
                authority,
            )
        })
        .collect();
        BehaviorAvailability { current, choices }
    }

    /// Capture the Shell-authoritative Behavior choice projection from one
    /// control-plane snapshot. Pager clients render this value but every
    /// transition is revalidated by calling this method again.
    pub(super) async fn behavior_availability_projection(
        &self,
    ) -> tool_types::BehaviorAvailability {
        let support = self.behavior_capability_support().await;
        let admission_facts = {
            let admission = self.state.lock().await;
            Self::capture_behavior_admission_facts(&admission, None)
        };
        let workflow_tracker = self.workflow_tracker().await;
        let workflow_tracker = workflow_tracker.lock();
        self.behavior_availability_from_tracker(
            &workflow_tracker,
            support,
            crate::session::behavior::BehaviorRequestAuthority::Picker,
            admission_facts,
        )
    }

    pub(super) fn apply_behavior_to_snapshot(&self, snapshot: &mut TurnDeltaSnapshot) {
        let behavior = self.turn_behavior.lock().to_string();
        snapshot.admitted_behavior = Some(behavior.clone());
        snapshot.completed_behavior = Some(behavior);
    }

    /// Admit an ACP Behavior request into the Shell-owned latest-wins slot.
    /// The caller never waits inside the actor mailbox for capability checks,
    /// confirmation, persistence, or foreground cancellation; a dedicated
    /// local worker performs those steps while later requests remain able to
    /// replace the not-yet-claimed target.
    pub(super) async fn admit_behavior_selection(
        &self,
        session_mode: acp::SessionModeId,
        intent: Option<crate::session::ControlIntent>,
        responds_to: tokio::sync::oneshot::Sender<Result<BehaviorChangeOutcome, acp::Error>>,
    ) -> bool {
        let requested_behavior = tools::types::BehaviorId::try_from_id(session_mode.0.as_ref());
        let (projection, should_start) = {
            let mut admission = self.state.lock().await;
            if !admission.termination.is_open() {
                let _ = responds_to.send(Err(
                    acp::Error::internal_error().data("session is shutting down")
                ));
                return false;
            }
            match admission.admit_control_intent(
                crate::extensions::notification::ControlDomain::Behavior,
                intent.as_ref(),
            ) {
                ControlIntentAdmission::New => {}
                ControlIntentAdmission::DuplicateInFlight => {
                    let _ = responds_to.send(Ok(BehaviorChangeOutcome::InFlight));
                    return false;
                }
                ControlIntentAdmission::Older => {
                    let _ = responds_to.send(Ok(BehaviorChangeOutcome::Superseded));
                    return false;
                }
                ControlIntentAdmission::ExactTerminal(terminal) => {
                    let revision = (!terminal.ui_terminal_durable).then(|| {
                        admission.behavior_control_revision =
                            admission.behavior_control_revision.saturating_add(1);
                        admission.behavior_control_revision
                    });
                    drop(admission);
                    let response =
                        if let (Some(intent), Some(revision)) = (intent.as_ref(), revision) {
                            self.recover_missing_terminal_projection(
                                crate::extensions::notification::ControlDomain::Behavior,
                                intent,
                                &terminal,
                                revision,
                            )
                            .await
                        } else {
                            Ok(())
                        }
                        .map(|()| match terminal.phase {
                            crate::extensions::notification::ControlPhase::Applied => {
                                BehaviorChangeOutcome::Applied
                            }
                            crate::extensions::notification::ControlPhase::Rejected => {
                                BehaviorChangeOutcome::Rejected {
                                    message: terminal.message.unwrap_or_else(|| {
                                        "the Behavior request was previously rejected".to_string()
                                    }),
                                }
                            }
                            crate::extensions::notification::ControlPhase::Superseded => {
                                BehaviorChangeOutcome::Superseded
                            }
                            crate::extensions::notification::ControlPhase::Pending
                            | crate::extensions::notification::ControlPhase::Applying => {
                                unreachable!("persisted Behavior receipt must be terminal")
                            }
                        });
                    let _ = responds_to.send(response);
                    return false;
                }
            }
            // Confirmation replacement is part of the same admission
            // linearization as the desired-state revision. An applying
            // Behavior holds this state guard through its durable commit; a
            // later request must therefore become the next target instead of
            // mutating the applying request's confirmation latch mid-commit.
            let mut behavior = self.behavior.lock();
            if behavior
                .pending_switch()
                .is_some_and(|(_, target, _)| Some(target) != requested_behavior)
            {
                behavior.clear_pending_switch();
            }
            drop(behavior);
            admission.behavior_control_revision =
                admission.behavior_control_revision.saturating_add(1);
            let revision = admission.behavior_control_revision;
            if let Some(previous) = admission.pending_behavior_control.take() {
                let previous_projection = StepControlProjection {
                    revision: previous.revision,
                    target: crate::extensions::notification::ControlTarget::Behavior {
                        behavior_id: previous.session_mode.0.to_string(),
                    },
                    intent: previous.intent.clone(),
                };
                // Superseded is intentionally UI-silent: only the newest
                // desired target remains visible. In particular, admission
                // must not await an exact durable UI append while holding the
                // actor state mutex, because Stop/Shutdown must remain able to
                // cancel a stuck persistence retry.
                admission.mark_control_intent_terminal(
                    crate::extensions::notification::ControlDomain::Behavior,
                    previous.intent.as_ref(),
                    ControlIntentTerminal {
                        phase: crate::extensions::notification::ControlPhase::Superseded,
                        target: previous_projection.target.clone(),
                        message: None,
                        ui_terminal_durable: true,
                    },
                );
                let _ = previous
                    .responds_to
                    .send(Ok(BehaviorChangeOutcome::Superseded));
            }
            let confirmation_owner = intent
                .as_ref()
                .map(|intent| format!("{}:{}", intent.client_id, intent.generation));
            admission.pending_behavior_control = Some(PendingBehaviorSelection {
                session_mode: session_mode.clone(),
                revision,
                confirmation_owner,
                responds_to,
                intent,
            });
            let should_start = !admission.behavior_control_worker_active;
            if should_start
                && admission.foreground.is_idle()
                && admission.pending_step_controls.is_empty()
                && admission.applying_step_control.is_none()
            {
                admission.foreground = ForegroundState::ApplyingControl;
                admission.behavior_control_foreground_claimed = true;
            }
            admission.behavior_control_worker_active = true;
            (
                StepControlProjection {
                    revision,
                    target: crate::extensions::notification::ControlTarget::Behavior {
                        behavior_id: session_mode.0.to_string(),
                    },
                    intent: admission
                        .pending_behavior_control
                        .as_ref()
                        .and_then(|pending| pending.intent.clone()),
                },
                should_start,
            )
        };
        let _ = self
            .publish_control_projection(
                &projection,
                crate::extensions::notification::ControlPhase::Pending,
                None,
                false,
            )
            .await;
        should_start
    }

    pub(super) async fn drain_behavior_selections(
        self: std::sync::Arc<Self>,
        completion_tx: mpsc::UnboundedSender<(String, PromptTurnResult)>,
    ) -> Result<(), ()> {
        loop {
            let Some((pending, projection)) = ({
                let mut admission = self.state.lock().await;
                let pending = admission.pending_behavior_control.take();
                let projection = pending.as_ref().map(|pending| StepControlProjection {
                    revision: pending.revision,
                    target: crate::extensions::notification::ControlTarget::Behavior {
                        behavior_id: pending.session_mode.0.to_string(),
                    },
                    intent: pending.intent.clone(),
                });
                admission.applying_behavior_control = projection.clone();
                if pending.is_none() {
                    admission.behavior_control_worker_active = false;
                    if admission.behavior_control_foreground_claimed {
                        debug_assert!(matches!(
                            admission.foreground,
                            ForegroundState::ApplyingControl
                        ));
                        if matches!(admission.foreground, ForegroundState::ApplyingControl) {
                            admission.foreground = ForegroundState::Idle;
                        }
                        admission.behavior_control_foreground_claimed = false;
                    }
                }
                pending.zip(projection)
            }) else {
                self.idle_arbiter.notify_waiters();
                super::idle_arbitration::arbitrate_idle_wake(self.clone(), completion_tx.clone())
                    .await;
                self.emit_session_idle_if_idle().await;
                return Ok(());
            };
            let _ = self
                .publish_control_projection(
                    &projection,
                    crate::extensions::notification::ControlPhase::Applying,
                    None,
                    false,
                )
                .await;
            let application = self
                .apply_behavior_change_with_admission(
                    pending.session_mode,
                    crate::session::behavior::BehaviorRequestAuthority::Picker,
                    Some(pending.revision),
                    pending.confirmation_owner.as_deref(),
                    pending.intent.as_ref(),
                )
                .await;
            if application.cancelled_by_shutdown {
                let result = application.disposition;
                {
                    let mut admission = self.state.lock().await;
                    if admission.applying_behavior_control.as_ref() == Some(&projection) {
                        admission.applying_behavior_control = None;
                    }
                    admission.mark_control_intent_terminal(
                        crate::extensions::notification::ControlDomain::Behavior,
                        pending.intent.as_ref(),
                        ControlIntentTerminal {
                            phase: crate::extensions::notification::ControlPhase::Rejected,
                            target: projection.target.clone(),
                            message: Some("session is shutting down".to_string()),
                            // Shutdown itself is authoritative for this intent;
                            // no UI terminal is expected or repairable.
                            ui_terminal_durable: true,
                        },
                    );
                    admission.behavior_control_worker_active = false;
                    if admission.behavior_control_foreground_claimed {
                        if matches!(admission.foreground, ForegroundState::ApplyingControl) {
                            admission.foreground = ForegroundState::Idle;
                        }
                        admission.behavior_control_foreground_claimed = false;
                    }
                }
                let _ = pending.responds_to.send(result);
                self.idle_arbiter.notify_waiters();
                return Ok(());
            }
            let result = application.disposition;
            let ui_terminal_durable = matches!(result, Ok(BehaviorChangeOutcome::Superseded));
            let terminal_fact =
                Self::behavior_terminal_fact(&projection, &result, ui_terminal_durable);
            {
                let mut admission = self.state.lock().await;
                if admission.applying_behavior_control.as_ref() == Some(&projection) {
                    admission.applying_behavior_control = None;
                }
                // The Behavior transition is already authoritative. Record its
                // terminal receipt before the repairable UI append so teardown
                // cannot leave a committed intent looking in-flight.
                admission.mark_control_intent_terminal(
                    crate::extensions::notification::ControlDomain::Behavior,
                    pending.intent.as_ref(),
                    terminal_fact,
                );
            }
            let terminal = if matches!(result, Ok(BehaviorChangeOutcome::Superseded)) {
                Ok(false)
            } else {
                self.publish_behavior_terminal(&projection, &result).await
            };
            if matches!(terminal, Ok(true))
                && let Some(intent) = pending.intent.as_ref()
            {
                self.state.lock().await.mark_control_terminal_ui_durable(
                    crate::extensions::notification::ControlDomain::Behavior,
                    intent,
                );
            }
            let fatal = result
                .as_ref()
                .err()
                .is_some_and(crate::session::commands::is_fatal_turn_boundary_error)
                || terminal.is_err()
                || application.post_commit_fatal.is_some();
            if let Some(error) = application.post_commit_fatal.as_ref() {
                tracing::error!(?error, "Behavior was applied but foreground cleanup failed");
            }
            let response = match terminal {
                Ok(terminal_published) => result.map_err(|error| {
                    if terminal_published {
                        crate::session::mark_control_terminal_published(error)
                    } else {
                        error
                    }
                }),
                Err(error) => Err(acp::Error::internal_error().data(format!(
                    "Behavior state changed, but its terminal UI event was not durably recorded: {error}"
                ))),
            };
            let _ = pending.responds_to.send(response);
            if fatal {
                let pending = {
                    let mut admission = self.state.lock().await;
                    admission.termination.request(TerminationState::Fatal);
                    admission.behavior_control_worker_active = false;
                    admission.applying_behavior_control = None;
                    if admission.behavior_control_foreground_claimed {
                        if matches!(admission.foreground, ForegroundState::ApplyingControl) {
                            admission.foreground = ForegroundState::Idle;
                        }
                        admission.behavior_control_foreground_claimed = false;
                    }
                    admission.pending_behavior_control.take().map(|pending| {
                        admission.mark_control_intent_terminal(
                            crate::extensions::notification::ControlDomain::Behavior,
                            pending.intent.as_ref(),
                            ControlIntentTerminal {
                                phase: crate::extensions::notification::ControlPhase::Rejected,
                                target: crate::extensions::notification::ControlTarget::Behavior {
                                    behavior_id: pending.session_mode.0.to_string(),
                                },
                                message: Some(
                                    "Behavior control worker stopped after a terminal persistence failure"
                                        .to_string(),
                                ),
                                ui_terminal_durable: false,
                            },
                        );
                        pending
                    })
                };
                if let Some(pending) = pending {
                    let _ = pending
                        .responds_to
                        .send(Err(acp::Error::internal_error().data(
                            "Behavior control worker stopped after a terminal persistence failure",
                        )));
                }
                self.idle_arbiter.notify_waiters();
                return Err(());
            }
            super::idle_arbitration::arbitrate_idle_wake(self.clone(), completion_tx.clone()).await;
        }
    }

    pub(super) async fn request_behavior_change(
        &self,
        session_mode_id: acp::SessionModeId,
    ) -> Result<BehaviorChangeOutcome, acp::Error> {
        self.request_behavior_change_with_admission(
            session_mode_id,
            crate::session::behavior::BehaviorRequestAuthority::HostCommand,
        )
        .await
    }

    /// Goal entry is an out-of-band control operation: from Normal/Clarify it
    /// may commit while their regular turn is still active, after which the
    /// command-plane caller cancels that exact foreground turn. The generic
    /// Behavior picker remains idle-only, and Plan/Workflow ownership rules
    /// are unchanged.
    pub(super) async fn request_goal_behavior_entry(
        &self,
    ) -> Result<BehaviorChangeOutcome, acp::Error> {
        self.request_behavior_change_with_admission(
            acp::SessionModeId::new("goal"),
            crate::session::behavior::BehaviorRequestAuthority::GoalLifecycle,
        )
        .await
    }

    async fn request_behavior_change_with_admission(
        &self,
        session_mode_id: acp::SessionModeId,
        authority: crate::session::behavior::BehaviorRequestAuthority,
    ) -> Result<BehaviorChangeOutcome, acp::Error> {
        let projection = {
            let mut admission = self.state.lock().await;
            admission.behavior_control_revision =
                admission.behavior_control_revision.saturating_add(1);
            let projection = StepControlProjection {
                revision: admission.behavior_control_revision,
                target: crate::extensions::notification::ControlTarget::Behavior {
                    behavior_id: session_mode_id.0.to_string(),
                },
                intent: None,
            };
            admission.applying_behavior_control = Some(projection.clone());
            projection
        };
        let _ = self
            .publish_control_projection(
                &projection,
                crate::extensions::notification::ControlPhase::Pending,
                None,
                false,
            )
            .await;
        let _ = self
            .publish_control_projection(
                &projection,
                crate::extensions::notification::ControlPhase::Applying,
                None,
                false,
            )
            .await;

        let application = self
            .apply_behavior_change_with_admission(
                session_mode_id,
                authority,
                Some(projection.revision),
                None,
                None,
            )
            .await;
        if application.cancelled_by_shutdown {
            let mut admission = self.state.lock().await;
            if admission.applying_behavior_control.as_ref() == Some(&projection) {
                admission.applying_behavior_control = None;
            }
            return application.disposition;
        }
        let result = application.disposition;
        let result_is_fatal = result
            .as_ref()
            .err()
            .is_some_and(crate::session::commands::is_fatal_turn_boundary_error);
        let terminal = if matches!(result, Ok(BehaviorChangeOutcome::Superseded)) {
            Ok(false)
        } else {
            self.publish_behavior_terminal(&projection, &result).await
        };
        {
            let mut admission = self.state.lock().await;
            if admission.applying_behavior_control.as_ref() == Some(&projection) {
                admission.applying_behavior_control = None;
            }
        }
        let response = match &terminal {
            Ok(terminal_published) => result.map_err(|error| {
                if *terminal_published {
                    crate::session::mark_control_terminal_published(error)
                } else {
                    error
                }
            }),
            Err(error) => Err(acp::Error::internal_error().data(format!(
                "Behavior state changed, but its terminal UI event was not durably recorded: {error}"
            ))),
        };
        let mut fatal = Vec::new();
        if let Err(error) = terminal {
            fatal.push(format!(
                "Behavior terminal UI event was not durably recorded: {error}"
            ));
        }
        if let Some(error) = application.post_commit_fatal {
            fatal.push(format!(
                "Behavior was applied but foreground cleanup failed: {error}"
            ));
        }
        if result_is_fatal {
            fatal.push("Behavior durable transition failed".to_string());
        }
        if !fatal.is_empty() {
            self.state
                .lock()
                .await
                .termination
                .request(TerminationState::Fatal);
            let _ = self.event_tx.send(SessionEvent::ControlWorkerFailed {
                message: fatal.join("; "),
            });
        }
        response
    }

    async fn publish_behavior_terminal(
        &self,
        projection: &StepControlProjection,
        result: &Result<BehaviorChangeOutcome, acp::Error>,
    ) -> Result<bool, crate::session::persistence::DurableAppendError> {
        let terminal = Self::behavior_terminal_fact(projection, result, false);
        self.publish_control_projection(projection, terminal.phase, terminal.message, true)
            .await?;
        Ok(true)
    }

    fn behavior_terminal_fact(
        projection: &StepControlProjection,
        result: &Result<BehaviorChangeOutcome, acp::Error>,
        ui_terminal_durable: bool,
    ) -> ControlIntentTerminal {
        let (phase, message) = match result {
            Ok(BehaviorChangeOutcome::Applied) => (
                crate::extensions::notification::ControlPhase::Applied,
                Some(Self::control_terminal_message(&projection.target, &Ok(()))),
            ),
            Ok(BehaviorChangeOutcome::Superseded) => (
                crate::extensions::notification::ControlPhase::Superseded,
                None,
            ),
            Ok(BehaviorChangeOutcome::InFlight) => {
                unreachable!("resident duplicate intents do not enter the Behavior worker")
            }
            Ok(BehaviorChangeOutcome::Rejected { message }) => (
                crate::extensions::notification::ControlPhase::Rejected,
                Some(format!("Behavior switch rejected: {message}")),
            ),
            Ok(BehaviorChangeOutcome::ConfirmationRequired { message, .. }) => (
                crate::extensions::notification::ControlPhase::Rejected,
                Some(format!("Behavior switch needs confirmation: {message}")),
            ),
            Err(error) => (
                crate::extensions::notification::ControlPhase::Rejected,
                Some(format!("Behavior switch failed: {error}")),
            ),
        };
        ControlIntentTerminal {
            phase,
            target: projection.target.clone(),
            message,
            ui_terminal_durable,
        }
    }

    async fn apply_behavior_change_with_admission(
        &self,
        session_mode_id: acp::SessionModeId,
        authority: crate::session::behavior::BehaviorRequestAuthority,
        expected_revision: Option<u64>,
        confirmation_owner: Option<&str>,
        control_intent: Option<&crate::session::ControlIntent>,
    ) -> BehaviorApplication {
        use crate::session::behavior::BehaviorEffect;
        use tool_types::BehaviorAvailabilityDisposition;
        use tools::types::BehaviorId;

        // Behavior admission and next-step Agent/model controls share one
        // linearization boundary. An Agent candidate is built asynchronously
        // after leaving the pending deque, so inspecting that deque alone
        // cannot prove there is no in-flight route transition.
        let step_control = self.step_control_gate.lock().await;
        let _goal_transaction = self.goal_transaction_gate.lock().await;
        if !self.state.lock().await.termination.is_open() {
            return BehaviorApplication::cancelled_by_shutdown();
        }

        // Workflow launch and special-Behavior admission share the manager
        // lock as their linearization point. Every public launch rechecks the
        // selected Behavior while holding this lock; keeping it through the
        // durable control commit and in-memory selection prevents both races:
        // a run appearing after the conflict snapshot, or a launch admitted
        // against the old Behavior after the new one was committed.
        let support = self.behavior_capability_support().await;
        let mut workflow_admission = self.workflow_manager.lock().await;
        let mut foreground_admission = self.state.lock().await;
        if expected_revision
            .is_some_and(|revision| revision != foreground_admission.behavior_control_revision)
        {
            return BehaviorApplication::disposition(Ok(BehaviorChangeOutcome::Superseded));
        }
        let admission_facts =
            Self::capture_behavior_admission_facts(&foreground_admission, expected_revision);
        let availability = {
            let workflow_tracker = workflow_admission.tracker();
            let workflow_tracker = workflow_tracker.lock();
            self.behavior_availability_from_tracker(
                &workflow_tracker,
                support,
                authority,
                admission_facts.clone(),
            )
        };
        let previous_behavior = availability.current;
        if !admission_facts.termination_open {
            return BehaviorApplication::cancelled_by_shutdown();
        }
        let Some(mode) = BehaviorId::try_from_id(session_mode_id.0.as_ref()) else {
            let message = format!(
                "Unknown Behavior id: {}. Agent Roles must be selected through the Agent interface.",
                session_mode_id.0
            );
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(previous_behavior.as_id()),
                serde_json::json!({
                    "status": "rejected",
                    "source": previous_behavior.as_id(),
                    "target": session_mode_id.0.as_ref(),
                    "message": message,
                }),
            );
            return BehaviorApplication::disposition(Ok(BehaviorChangeOutcome::Rejected {
                message,
            }));
        };
        let Some(choice) = availability.choice(mode).cloned() else {
            let message = format!("{} behavior is unavailable.", mode.display_label());
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(previous_behavior.as_id()),
                serde_json::json!({
                    "status": "rejected",
                    "source": previous_behavior.as_id(),
                    "target": mode.as_id(),
                    "message": message,
                }),
            );
            return BehaviorApplication::disposition(Ok(BehaviorChangeOutcome::Rejected {
                message,
            }));
        };

        // Confirmation is durable control-plane intent, not a side effect of
        // an inadmissible request. The shared assessment above rejects every
        // busy or otherwise unavailable transition before this latch can move.
        if choice.disposition == BehaviorAvailabilityDisposition::ConfirmationRequired
            && confirmation_owner.is_none()
            && authority == crate::session::behavior::BehaviorRequestAuthority::Picker
        {
            let message = format!(
                "Switching to {} requires a client-scoped control intent; retry from a client that supports Grow control intents.",
                mode.display_label()
            );
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(previous_behavior.as_id()),
                serde_json::json!({
                    "status": "rejected",
                    "source": previous_behavior.as_id(),
                    "target": mode.as_id(),
                    "message": message,
                }),
            );
            return BehaviorApplication::disposition(Ok(BehaviorChangeOutcome::Rejected {
                message,
            }));
        }
        let decision = self.behavior.lock().decide_assessed_switch_owned(
            mode,
            choice,
            std::time::Duration::from_secs(8),
            confirmation_owner,
        );
        if !matches!(&decision.outcome, BehaviorChangeOutcome::Applied) {
            let meta = match &decision.outcome {
                BehaviorChangeOutcome::ConfirmationRequired {
                    message,
                    remaining_ms,
                } => serde_json::json!({
                    "status": "confirmation_required",
                    "source": previous_behavior.as_id(),
                    "target": mode.as_id(),
                    "message": message,
                    "remainingMs": remaining_ms,
                }),
                BehaviorChangeOutcome::Rejected { message } => {
                    serde_json::json!({
                        "status": "rejected",
                        "source": previous_behavior.as_id(),
                        "target": mode.as_id(),
                        "message": message,
                    })
                }
                BehaviorChangeOutcome::Superseded => unreachable!(),
                BehaviorChangeOutcome::InFlight => unreachable!(),
                BehaviorChangeOutcome::Applied => unreachable!(),
            };
            self.enqueue_current_mode_update_with_behavior_change(
                acp::SessionModeId::new(previous_behavior.as_id()),
                meta,
            );
            return BehaviorApplication::disposition(Ok(decision.outcome));
        }

        if !decision.effects.is_empty() {
            let persisted_goal = self.goal_tracker.lock().snapshot().cloned();
            let persisted = if let Some(intent) = control_intent {
                self.persist_behavior_transition_for_control_durably(
                    crate::session::behavior::BehaviorSnapshot::selected(mode),
                    persisted_goal,
                    intent.clone(),
                )
                .await
            } else {
                self.persist_behavior_transition_durably(
                    crate::session::behavior::BehaviorSnapshot::selected(mode),
                    persisted_goal,
                )
                .await
            };
            if let Err(error) = persisted {
                let message = format!(
                    "Could not durably select {} Behavior.",
                    mode.display_label()
                );
                self.enqueue_current_mode_update_with_behavior_change(
                    acp::SessionModeId::new(previous_behavior.as_id()),
                    serde_json::json!({
                        "status": "rejected",
                        "source": previous_behavior.as_id(),
                        "target": mode.as_id(),
                        "message": message,
                    }),
                );
                return BehaviorApplication::disposition(Err(
                    crate::session::commands::fatal_turn_boundary_error(
                        "Behavior control",
                        error.to_string(),
                    ),
                ));
            }
        } else if let Some(intent) = control_intent {
            let (behavior, goal) = self.capture_control_authorities();
            if let Err(error) = self
                .persist_applied_control_receipt_durably(
                    behavior,
                    goal,
                    crate::extensions::notification::ControlDomain::Behavior,
                    crate::extensions::notification::ControlTarget::Behavior {
                        behavior_id: mode.as_id().to_owned(),
                    },
                    intent.clone(),
                )
                .await
            {
                return BehaviorApplication::disposition(Err(
                    crate::session::commands::fatal_turn_boundary_error(
                        "Behavior control",
                        format!("Behavior acknowledgement was not persisted: {error}"),
                    ),
                ));
            }
        }

        // Publish the new ownership identity before either admission lock is
        // released. The next foreground therefore captures exactly the
        // Behavior whose Control context is already durable in Surface.
        if let Some(target) = decision.effects.iter().find_map(|effect| match effect {
            BehaviorEffect::Select(target) => Some(*target),
            _ => None,
        }) {
            self.behavior.lock().select_behavior(target);
            if target != previous_behavior && foreground_admission.foreground.regular().is_some() {
                foreground_admission.terminal_preemption_pending = true;
            }
        }
        drop(foreground_admission);
        drop(workflow_admission);
        drop(_goal_transaction);
        drop(step_control);

        let mut post_commit_fatal = None;
        for effect in decision.effects {
            match effect {
                BehaviorEffect::CancelSourceForeground(source) => {
                    let source_owns_foreground =
                        self.state.lock().await.foreground.regular().is_some()
                            && *self.turn_behavior.lock() == source;
                    if source_owns_foreground {
                        if let Err(error) = self
                            .cancel_running_task(
                                true,
                                false,
                                false,
                                Some("behavior_switch".to_string()),
                            )
                            .await
                        {
                            post_commit_fatal = Some(error);
                            break;
                        }
                    }
                }
                BehaviorEffect::Select(_) => {}
            }
        }
        self.enqueue_current_mode_update(session_mode_id.clone());
        self.send_available_commands_update().await;
        BehaviorApplication {
            disposition: Ok(BehaviorChangeOutcome::Applied),
            post_commit_fatal,
            cancelled_by_shutdown: false,
        }
    }
    /// Inject active Behavior guidance into the conversation.
    ///
    /// Called once per turn before the user's message. Drafting and amending
    /// use the mutable candidate artifact; executing uses the frozen approved
    /// artifact. The phase itself is the edit gate—there is no hidden pending
    /// or re-entry state.
    pub(super) async fn inject_behavior_reminders(&self) -> Result<(), acp::Error> {
        let admitted = *self.turn_behavior.lock();
        if admitted == tool_types::BehaviorId::Workflow {
            let context = self.workflow_behavior_context()?;
            self.push_system_reminder_with_tag(&context, self.reminder_wrapper_tag());
        }
        Ok(())
    }
    /// Render a plan mode template via the tool bridge's `TemplateRenderer`.
    ///
    /// The host reads the session artifact and injects its content directly;
    /// the Agent does not need Read or Edit access to that storage location.
    pub(super) async fn render_plan_template(
        &self,
        template: &str,
        plan_content: &str,
    ) -> Option<String> {
        let extra = serde_json::json!({ "plan_content": plan_content.trim() });
        self.agent
            .borrow()
            .tool_bridge()
            .render_prompt(template, &extra)
            .await
    }
    /// Persist bookkeeping-only Behavior state through the same Goal
    /// transaction gate as user-visible Control changes. A later auxiliary
    /// snapshot must never overwrite a concurrent Goal usage settlement with
    /// stale state.
    pub(super) async fn record_control_snapshot_durably(&self) -> std::io::Result<()> {
        let _transaction = self.goal_transaction_gate.lock().await;
        let (behavior, goal) = self.capture_control_authorities();
        let revision = self
            .control_revision
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .saturating_add(1);
        let snapshot = crate::session::control::SessionControlSnapshot::new(
            revision,
            self.agent.borrow().definition().selector_identity(),
            behavior,
            goal,
        );
        let kind = snapshot.timeline_kind()?;
        self.chat_state_handle
            .record_timeline_event_durably(kind)
            .await
            .map(|_| ())
            .map_err(std::io::Error::other)
    }

    /// Commit a Behavior transition through the same atomic control snapshot
    /// used by Goal. A failed persistence barrier restores the in-memory
    /// coordinator, so callers never publish a transition that cannot survive
    /// reconnect.
    pub(super) async fn commit_behavior_mutation_or_restore(
        &self,
        previous: crate::session::behavior::BehaviorSnapshot,
    ) -> Result<(), String> {
        let _step_control = self.step_control_gate.lock().await;
        if !self.state.lock().await.termination.is_open() {
            *self.behavior.lock() =
                crate::session::behavior::BehaviorCoordinator::from_snapshot(previous);
            return Err("session is shutting down".into());
        }
        let _transaction = self.goal_transaction_gate.lock().await;
        let next = self.behavior.lock().snapshot();
        let goal = self.goal_tracker.lock().snapshot().cloned();
        let selection_changed = previous.behavior() != next.behavior();
        let persisted = if selection_changed {
            self.persist_behavior_transition_durably(next, goal).await
        } else if next.behavior() == tool_types::BehaviorId::Plan && previous != next {
            self.persist_plan_phase_transition_durably(next, goal).await
        } else {
            self.persist_control_snapshot_durably(next, goal).await
        };
        if let Err(error) = persisted {
            *self.behavior.lock() =
                crate::session::behavior::BehaviorCoordinator::from_snapshot(previous);
            return Err(format!("Behavior control state was not persisted: {error}"));
        }
        Ok(())
    }
}
