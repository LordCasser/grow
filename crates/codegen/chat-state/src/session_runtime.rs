//! Durable execution handoff. Business state (Goal, Plan, Workflow) is separate.

use serde::{Deserialize, Serialize};

use crate::{EventSeq, StepId, TurnId};

pub const MAX_SUSPENDED_NODES: usize = 4096;
pub const MAX_COMMAND_SETTLEMENT_OUTPUT_BYTES: usize = 64 * 1024;
pub const MAX_STRUCTURED_OUTPUT_RETRIES: u32 = 3;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePhase {
    #[default]
    Running,
    Suspending,
    Suspended,
    Restoring,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeNodeRef {
    pub session_id: String,
    pub checkpoint_seq: EventSeq,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuspendedOperationState {
    NotDispatched,
    AwaitChild,
    AwaitUser,
    AwaitPermission,
    Interrupted,
    OutcomeUnknown,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuspendedOperation {
    pub call_id: String,
    pub state: SuspendedOperationState,
    /// Identity of the original child, question, command or permission request.
    pub dependency_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedResponseContinuation {
    pub identity: crate::ResponseAdmissionIdentity,
    pub stop_reason: Option<sampling_types::StopReason>,
    pub stop_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnResumePoint {
    Input,
    Host,
    Model,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnContinuation {
    pub turn_id: TurnId,
    pub prompt_id: String,
    pub prompt_index: usize,
    pub origin: String,
    pub next_step_index: u32,
    pub tool_turns_used: usize,
    pub tool_calls_used: usize,
    pub stop_continuations_used: u32,
    pub output_schema: Option<serde_json::Value>,
    pub resume_point: TurnResumePoint,
    pub task_output_usage: Option<(u64, bool)>,
    pub accepted_response: Option<AcceptedResponseContinuation>,
    /// Invalid schema-tool attempts already returned to the model in this turn.
    pub structured_output_retries: u32,
    pub sampling_attempts_used: u32,
    pub sampling_max_attempts: Option<u32>,
    pub sampling_replay_safe: bool,
    /// Original wall-clock deadline. Offline time does not replenish it.
    pub sampling_deadline_ms: Option<u64>,
    pub operations: Vec<SuspendedOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCheckpoint {
    pub frontier: EventSeq,
    /// Hash of the exact workspace, Agent, and model route needed to restore
    /// this execution. The source itself is never copied into the checkpoint.
    pub runtime_source_hash: String,
    pub turn: Option<TurnContinuation>,
    pub step: Option<StepId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSettlementStatus {
    Completed,
    Interrupted,
}

/// Process evidence survives even when there is no foreground logical Turn.
/// A bounded output excerpt makes the durable settlement sufficient to retry
/// notification and checkpoint work after the terminal's task snapshot expires.
/// It is not a resumable process image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCommandSettlement {
    pub command_id: String,
    pub call_id: String,
    pub outcome: CommandSettlementStatus,
    pub output_file: String,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    pub output: String,
    pub is_backgrounded: bool,
    pub owner: crate::NotificationOwner,
    pub task_kind: crate::NotificationTaskKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionRuntimeEvent {
    SuspendRequested {
        operation_id: String,
        root_session_id: String,
    },
    Checkpoint {
        operation_id: String,
        checkpoint: Box<RuntimeCheckpoint>,
    },
    CommandsSettled {
        operation_id: String,
        commands: Vec<RuntimeCommandSettlement>,
    },
    /// Only the tree root publishes the complete verified membership barrier.
    TreeSuspended {
        operation_id: String,
        nodes: Vec<RuntimeNodeRef>,
    },
    RestorePrepared {
        operation_id: String,
        instance_id: String,
    },
    Resumed {
        operation_id: String,
        instance_id: String,
    },
    /// The restored tree has committed its recovery work and can now admit
    /// execution. This is separate from `Resumed` so a crash during the
    /// all-node commit barrier remains recoverable as Restoring.
    ExecutionReleased {
        operation_id: String,
        instance_id: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeLifecycle {
    pub phase: RuntimePhase,
    pub operation_id: Option<String>,
    pub root_session_id: Option<String>,
    pub checkpoint: Option<RuntimeCheckpoint>,
    pub checkpoint_seq: Option<EventSeq>,
    pub tree: Vec<RuntimeNodeRef>,
    pub instance_id: Option<String>,
    /// `Resumed` was durably committed for the current restore owner, while
    /// execution admission is still closed pending `ExecutionReleased`.
    pub resume_committed: bool,
    pub commands: Option<Vec<RuntimeCommandSettlement>>,
}

fn valid_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}

fn valid_runtime_source_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl RuntimeLifecycle {
    pub fn preserves_open_turn(&self) -> bool {
        matches!(
            self.phase,
            RuntimePhase::Suspended | RuntimePhase::Restoring
        ) && self.checkpoint.is_some()
    }

    /// Prepare and replay use the same transition validation. Callers commit
    /// the event before installing the returned projection.
    pub fn transition(
        &self,
        event: &SessionRuntimeEvent,
        seq: EventSeq,
        active_turn: Option<TurnId>,
        active_step: Option<StepId>,
        has_open_requests: bool,
    ) -> Result<Self, String> {
        let mut next = self.clone();
        match event {
            SessionRuntimeEvent::SuspendRequested {
                operation_id,
                root_session_id,
            } => {
                if self.phase != RuntimePhase::Running
                    || !valid_id(operation_id)
                    || !valid_id(root_session_id)
                {
                    return Err("runtime is not available for a new suspension".into());
                }
                next = Self {
                    phase: RuntimePhase::Suspending,
                    operation_id: Some(operation_id.clone()),
                    root_session_id: Some(root_session_id.clone()),
                    ..Self::default()
                };
            }
            SessionRuntimeEvent::Checkpoint {
                operation_id,
                checkpoint,
            } => {
                if self.phase != RuntimePhase::Suspending
                    || self.operation_id.as_ref() != Some(operation_id)
                    || has_open_requests
                    || self.commands.is_none()
                    || checkpoint.frontier.get().checked_add(1) != Some(seq.get())
                    || !valid_runtime_source_hash(&checkpoint.runtime_source_hash)
                    || checkpoint.turn.as_ref().map(|turn| turn.turn_id) != active_turn
                    || checkpoint.step != active_step
                {
                    return Err("checkpoint does not match the settled runtime frontier".into());
                }
                if let Some(turn) = &checkpoint.turn {
                    let mut calls = std::collections::BTreeSet::new();
                    if !valid_id(&turn.prompt_id)
                        || turn.origin.is_empty()
                        || turn.operations.len() > 4096
                        || turn.structured_output_retries > MAX_STRUCTURED_OUTPUT_RETRIES
                        || turn.operations.iter().any(|op| {
                            !valid_id(&op.call_id)
                                || !calls.insert(&op.call_id)
                                || op.dependency_id.as_deref().is_some_and(|id| !valid_id(id))
                                || (matches!(
                                    op.state,
                                    SuspendedOperationState::AwaitChild
                                        | SuspendedOperationState::AwaitUser
                                        | SuspendedOperationState::AwaitPermission
                                ) && op.dependency_id.is_none())
                        })
                    {
                        return Err("checkpoint has invalid continuation references".into());
                    }
                }
                if serde_json::to_vec(checkpoint)
                    .map_err(|e| e.to_string())?
                    .len()
                    > 1024 * 1024
                {
                    return Err("checkpoint exceeds the continuation budget".into());
                }
                next.phase = RuntimePhase::Suspended;
                next.checkpoint = Some(checkpoint.as_ref().clone());
                next.checkpoint_seq = Some(seq);
            }
            SessionRuntimeEvent::CommandsSettled {
                operation_id,
                commands,
            } => {
                if self.operation_id.as_ref() != Some(operation_id) {
                    return Err("command settlement belongs to another suspension".into());
                }
                if matches!(
                    self.phase,
                    RuntimePhase::Suspending | RuntimePhase::Suspended
                ) && self.commands.as_ref() == Some(commands)
                {
                    // A caller may lose the durable ACK and retry the exact
                    // evidence. Preserve the first projection without
                    // treating a replay as a second settlement.
                    return Ok(next);
                }
                let mut ids = std::collections::BTreeSet::new();
                if self.phase != RuntimePhase::Suspending
                    || self.commands.is_some()
                    || commands.len() > MAX_SUSPENDED_NODES
                    || commands.iter().any(|command| {
                        !valid_id(&command.command_id)
                            || !valid_id(&command.call_id)
                            || !ids.insert(&command.command_id)
                            || command.output_file.is_empty()
                            || command.output_file.len() > 4096
                            || command.output.len() > MAX_COMMAND_SETTLEMENT_OUTPUT_BYTES
                    })
                    || serde_json::to_vec(commands)
                        .map_err(|error| error.to_string())?
                        .len()
                        > 1024 * 1024
                {
                    return Err("command settlement does not match this suspension".into());
                }
                next.commands = Some(commands.clone());
            }
            SessionRuntimeEvent::TreeSuspended {
                operation_id,
                nodes,
            } => {
                let mut ids = std::collections::BTreeSet::new();
                if self.phase != RuntimePhase::Suspended
                    || self.operation_id.as_ref() != Some(operation_id)
                    || !self.tree.is_empty()
                    || nodes.is_empty()
                    || nodes.len() > MAX_SUSPENDED_NODES
                    || nodes
                        .iter()
                        .any(|node| !valid_id(&node.session_id) || !ids.insert(&node.session_id))
                    || !nodes.iter().any(|node| {
                        Some(&node.session_id) == self.root_session_id.as_ref()
                            && Some(node.checkpoint_seq) == self.checkpoint_seq
                    })
                {
                    return Err("invalid suspended-tree membership barrier".into());
                }
                next.tree = nodes.clone();
            }
            SessionRuntimeEvent::RestorePrepared {
                operation_id,
                instance_id,
            } => {
                if self.phase != RuntimePhase::Suspended
                    || self.operation_id.as_ref() != Some(operation_id)
                    || !valid_id(instance_id)
                {
                    return Err("checkpoint is not available for restoration".into());
                }
                next.phase = RuntimePhase::Restoring;
                next.instance_id = Some(instance_id.clone());
                next.resume_committed = false;
            }
            SessionRuntimeEvent::Resumed {
                operation_id,
                instance_id,
            } => {
                if self.phase == RuntimePhase::Restoring
                    && self.operation_id.as_ref() == Some(operation_id)
                    && self.instance_id.as_ref() == Some(instance_id)
                    && self.resume_committed
                {
                    // A lost ACK may retry the same durable commit. Keep the
                    // restore owner and closed admission unchanged.
                    return Ok(next);
                }
                if self.phase != RuntimePhase::Restoring
                    || self.operation_id.as_ref() != Some(operation_id)
                    || self.instance_id.as_ref() != Some(instance_id)
                    || self.resume_committed
                {
                    return Err("restoration does not own this checkpoint".into());
                }
                next.resume_committed = true;
            }
            SessionRuntimeEvent::ExecutionReleased {
                operation_id,
                instance_id,
            } => {
                if self.phase != RuntimePhase::Restoring
                    || !self.resume_committed
                    || self.operation_id.as_ref() != Some(operation_id)
                    || self.instance_id.as_ref() != Some(instance_id)
                {
                    return Err("execution release does not own a committed restore".into());
                }
                // Keep checkpoint references for audit, but consume their
                // execution right only after the explicit release event.
                next.phase = RuntimePhase::Running;
            }
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkpoint() -> SessionRuntimeEvent {
        SessionRuntimeEvent::Checkpoint {
            operation_id: "op".into(),
            checkpoint: Box::new(RuntimeCheckpoint {
                frontier: EventSeq::new(1),
                runtime_source_hash: "a".repeat(64),
                turn: None,
                step: None,
            }),
        }
    }

    fn requested() -> RuntimeLifecycle {
        RuntimeLifecycle::default()
            .transition(
                &SessionRuntimeEvent::SuspendRequested {
                    operation_id: "op".into(),
                    root_session_id: "root".into(),
                },
                EventSeq::new(0),
                None,
                None,
                false,
            )
            .unwrap()
    }

    fn empty_commands_settled() -> SessionRuntimeEvent {
        SessionRuntimeEvent::CommandsSettled {
            operation_id: "op".into(),
            commands: Vec::new(),
        }
    }

    fn settled() -> RuntimeLifecycle {
        requested()
            .transition(
                &empty_commands_settled(),
                EventSeq::new(1),
                None,
                None,
                false,
            )
            .unwrap()
    }

    #[test]
    fn checkpoint_requires_matching_frontier_and_settled_requests() {
        let requested = requested();
        assert!(
            requested
                .transition(&checkpoint(), EventSeq::new(2), None, None, false)
                .is_err()
        );
        assert!(!requested.preserves_open_turn());

        let state = settled();
        assert!(
            state
                .transition(&checkpoint(), EventSeq::new(3), None, None, false)
                .is_err()
        );
        assert!(
            state
                .transition(&checkpoint(), EventSeq::new(2), None, None, true)
                .is_err()
        );
        assert!(
            state
                .transition(
                    &checkpoint(),
                    EventSeq::new(2),
                    Some(TurnId(1)),
                    None,
                    false
                )
                .is_err()
        );
        assert!(
            state
                .transition(&checkpoint(), EventSeq::new(2), None, None, false)
                .unwrap()
                .preserves_open_turn()
        );
    }

    #[test]
    fn command_settlement_is_bound_idempotently_to_one_operation() {
        let command = RuntimeCommandSettlement {
            command_id: "command-1".into(),
            call_id: "call-1".into(),
            outcome: CommandSettlementStatus::Interrupted,
            output_file: "/tmp/command-1.log".into(),
            exit_code: None,
            signal: Some("SIGINT".into()),
            output: "partial output".into(),
            is_backgrounded: true,
            owner: crate::NotificationOwner::Session,
            task_kind: crate::NotificationTaskKind::Task,
        };
        let event = SessionRuntimeEvent::CommandsSettled {
            operation_id: "op".into(),
            commands: vec![command.clone()],
        };
        let state = requested();
        let settled = state
            .transition(&event, EventSeq::new(1), None, None, false)
            .unwrap();
        assert_eq!(settled.commands, Some(vec![command.clone()]));
        assert_eq!(
            settled
                .transition(&event, EventSeq::new(2), None, None, false)
                .unwrap(),
            settled
        );

        let mut conflict = command.clone();
        conflict.output_file = "/tmp/other.log".into();
        assert!(
            settled
                .transition(
                    &SessionRuntimeEvent::CommandsSettled {
                        operation_id: "op".into(),
                        commands: vec![conflict],
                    },
                    EventSeq::new(2),
                    None,
                    None,
                    false,
                )
                .is_err()
        );
        assert!(
            state
                .transition(
                    &SessionRuntimeEvent::CommandsSettled {
                        operation_id: "other-op".into(),
                        commands: vec![command.clone()],
                    },
                    EventSeq::new(1),
                    None,
                    None,
                    false,
                )
                .is_err()
        );

        let mut oversized_output = command.clone();
        oversized_output.output = "x".repeat(MAX_COMMAND_SETTLEMENT_OUTPUT_BYTES + 1);
        assert!(
            state
                .transition(
                    &SessionRuntimeEvent::CommandsSettled {
                        operation_id: "op".into(),
                        commands: vec![oversized_output],
                    },
                    EventSeq::new(1),
                    None,
                    None,
                    false,
                )
                .is_err()
        );

        let checkpointed = settled
            .transition(&checkpoint(), EventSeq::new(2), None, None, false)
            .unwrap();
        assert_eq!(
            checkpointed
                .transition(&event, EventSeq::new(3), None, None, false)
                .unwrap(),
            checkpointed
        );
    }

    #[test]
    fn checkpoint_requires_a_64_character_hex_runtime_source_hash() {
        let state = settled();
        for invalid in [
            String::new(),
            "a".repeat(63),
            "a".repeat(65),
            format!("{}g", "a".repeat(63)),
        ] {
            let mut event = checkpoint();
            let SessionRuntimeEvent::Checkpoint { checkpoint, .. } = &mut event else {
                unreachable!();
            };
            checkpoint.runtime_source_hash = invalid;
            assert!(
                state
                    .transition(&event, EventSeq::new(2), None, None, false)
                    .is_err()
            );
        }

        let mut uppercase = checkpoint();
        let SessionRuntimeEvent::Checkpoint {
            checkpoint: uppercase_checkpoint,
            ..
        } = &mut uppercase
        else {
            unreachable!();
        };
        uppercase_checkpoint.runtime_source_hash = "A".repeat(64);
        assert!(
            state
                .transition(&uppercase, EventSeq::new(2), None, None, false)
                .is_ok()
        );

        let mut serialized = serde_json::to_value(checkpoint()).unwrap();
        serialized["checkpoint"]
            .as_object_mut()
            .unwrap()
            .remove("runtime_source_hash");
        assert!(serde_json::from_value::<SessionRuntimeEvent>(serialized).is_err());
    }

    #[test]
    fn committed_restore_stays_gated_until_same_instance_releases_execution() {
        let state = settled()
            .transition(&checkpoint(), EventSeq::new(2), None, None, false)
            .unwrap();
        let prepared = SessionRuntimeEvent::RestorePrepared {
            operation_id: "op".into(),
            instance_id: "instance".into(),
        };
        let state = state
            .transition(&prepared, EventSeq::new(2), None, None, false)
            .unwrap();
        let wrong = SessionRuntimeEvent::Resumed {
            operation_id: "op".into(),
            instance_id: "other".into(),
        };
        assert!(
            state
                .transition(&wrong, EventSeq::new(3), None, None, false)
                .is_err()
        );
        let resumed = SessionRuntimeEvent::Resumed {
            operation_id: "op".into(),
            instance_id: "instance".into(),
        };
        let committed = state
            .transition(&resumed, EventSeq::new(3), None, None, false)
            .unwrap();
        assert_eq!(committed.phase, RuntimePhase::Restoring);
        assert!(committed.resume_committed);
        assert!(committed.preserves_open_turn());
        assert_eq!(committed.checkpoint, state.checkpoint);
        assert_eq!(
            committed
                .transition(&resumed, EventSeq::new(4), None, None, false)
                .unwrap(),
            committed
        );

        // A crash after the all-node commit but before release keeps the
        // checkpoint protected and the same restore owner can finish release.
        let released = committed
            .transition(
                &SessionRuntimeEvent::ExecutionReleased {
                    operation_id: "op".into(),
                    instance_id: "instance".into(),
                },
                EventSeq::new(5),
                None,
                None,
                false,
            )
            .unwrap();
        assert_eq!(released.phase, RuntimePhase::Running);
        assert!(released.resume_committed);
        assert!(!released.preserves_open_turn());
        assert_eq!(released.checkpoint, committed.checkpoint);
        assert!(
            released
                .transition(&prepared, EventSeq::new(6), None, None, false)
                .is_err()
        );
        assert!(
            released
                .transition(
                    &SessionRuntimeEvent::ExecutionReleased {
                        operation_id: "op".into(),
                        instance_id: "instance".into(),
                    },
                    EventSeq::new(6),
                    None,
                    None,
                    false,
                )
                .is_err()
        );
    }

    #[test]
    fn restore_preparation_cannot_change_instance_after_resume_commit() {
        let state = settled()
            .transition(&checkpoint(), EventSeq::new(2), None, None, false)
            .unwrap();
        let prepared = SessionRuntimeEvent::RestorePrepared {
            operation_id: "op".into(),
            instance_id: "instance".into(),
        };
        let state = state
            .transition(&prepared, EventSeq::new(3), None, None, false)
            .unwrap();
        let resumed = SessionRuntimeEvent::Resumed {
            operation_id: "op".into(),
            instance_id: "instance".into(),
        };
        let committed = state
            .transition(&resumed, EventSeq::new(4), None, None, false)
            .unwrap();
        assert!(
            committed
                .transition(
                    &SessionRuntimeEvent::RestorePrepared {
                        operation_id: "op".into(),
                        instance_id: "replacement".into()
                    },
                    EventSeq::new(5),
                    None,
                    None,
                    false,
                )
                .is_err()
        );
        assert!(
            committed
                .transition(
                    &SessionRuntimeEvent::ExecutionReleased {
                        operation_id: "op".into(),
                        instance_id: "replacement".into()
                    },
                    EventSeq::new(5),
                    None,
                    None,
                    false,
                )
                .is_err()
        );
    }

    #[test]
    fn tree_barrier_requires_unique_nodes_and_the_root_checkpoint() {
        let state = settled()
            .transition(&checkpoint(), EventSeq::new(2), None, None, false)
            .unwrap();
        for nodes in [
            vec![],
            vec![RuntimeNodeRef {
                session_id: "child".into(),
                checkpoint_seq: EventSeq::new(2),
            }],
            vec![RuntimeNodeRef {
                session_id: "root".into(),
                checkpoint_seq: EventSeq::new(1),
            }],
        ] {
            assert!(
                state
                    .transition(
                        &SessionRuntimeEvent::TreeSuspended {
                            operation_id: "op".into(),
                            nodes
                        },
                        EventSeq::new(3),
                        None,
                        None,
                        false
                    )
                    .is_err()
            );
        }
        let nodes = vec![RuntimeNodeRef {
            session_id: "root".into(),
            checkpoint_seq: EventSeq::new(2),
        }];
        assert!(
            state
                .transition(
                    &SessionRuntimeEvent::TreeSuspended {
                        operation_id: "op".into(),
                        nodes
                    },
                    EventSeq::new(3),
                    None,
                    None,
                    false
                )
                .is_ok()
        );
    }
}
