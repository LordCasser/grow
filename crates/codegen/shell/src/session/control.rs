//! Timeline-backed session control-plane snapshot.
//!
//! Timeline control events are the only persisted source for Agent selection,
//! Behavior selection, and the Goal runtime. Runtime leases, cancellation
//! handles, activity projections and UI clocks are intentionally absent and
//! reconstructed after reload.

pub const SESSION_CONTROL_ARCHITECTURE_VERSION: u32 = 5;

/// Client intent whose successful desired-state transition is committed in
/// the same Timeline fact as the authoritative domain state. This closes the
/// crash window between applying a model/Agent/Behavior and publishing its
/// UI-only terminal notification.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DurableControlReceipt {
    pub domain: crate::extensions::notification::ControlDomain,
    pub intent: crate::session::ControlIntent,
    pub target: crate::extensions::notification::ControlTarget,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionControlSnapshot {
    pub architecture_version: u32,
    pub control_revision: u64,
    pub agent_name: String,
    pub behavior: crate::session::behavior::BehaviorSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<crate::session::goal_tracker::GoalState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_control: Option<DurableControlReceipt>,
}

impl SessionControlSnapshot {
    pub fn new(
        control_revision: u64,
        agent_name: impl Into<String>,
        behavior: crate::session::behavior::BehaviorSnapshot,
        goal: Option<crate::session::goal_tracker::GoalState>,
    ) -> Self {
        Self {
            architecture_version: SESSION_CONTROL_ARCHITECTURE_VERSION,
            control_revision,
            agent_name: agent_name.into(),
            behavior,
            goal,
            applied_control: None,
        }
    }

    pub fn with_applied_control(
        mut self,
        domain: crate::extensions::notification::ControlDomain,
        target: crate::extensions::notification::ControlTarget,
        intent: Option<crate::session::ControlIntent>,
    ) -> Self {
        self.applied_control = intent.map(|intent| DurableControlReceipt {
            domain,
            intent,
            target,
        });
        self
    }

    pub fn architecture_is_current(&self) -> bool {
        self.architecture_version == SESSION_CONTROL_ARCHITECTURE_VERSION
    }

    fn decode_persisted(value: serde_json::Value) -> std::io::Result<Self> {
        let architecture_version = value
            .get("architecture_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "session control snapshot has no valid architecture version",
                )
            })?;
        if architecture_version != u64::from(SESSION_CONTROL_ARCHITECTURE_VERSION) {
            return Err(crate::session::persistence::session_version_mismatch(
                "Session Control architecture",
                architecture_version,
                u64::from(SESSION_CONTROL_ARCHITECTURE_VERSION),
            ));
        }
        if let Some(goal) = value.get("goal").filter(|goal| !goal.is_null()) {
            let goal_architecture = goal
                .get("architecture_version")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Goal snapshot has no valid architecture version",
                    )
                })?;
            if goal_architecture
                != u64::from(crate::session::goal_tracker::GOAL_ARCHITECTURE_VERSION)
            {
                return Err(crate::session::persistence::session_version_mismatch(
                    "Goal architecture",
                    goal_architecture,
                    u64::from(crate::session::goal_tracker::GOAL_ARCHITECTURE_VERSION),
                ));
            }
        }
        let snapshot: Self = serde_json::from_value(value)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    fn retired_context_layers(&self) -> Vec<chat_state::ControlContextLayer> {
        let plan_phase_active = self.behavior.behavior() == tool_types::BehaviorId::Plan;
        let goal_definition_active = self.behavior.behavior() == tool_types::BehaviorId::Goal
            && self.goal.as_ref().is_some_and(|goal| {
                goal.status == crate::session::goal_tracker::GoalStatus::Active
            });
        let mut retired = Vec::new();
        if !goal_definition_active {
            retired.push(chat_state::ControlContextLayer::GoalDefinition);
        }
        if !plan_phase_active {
            retired.push(chat_state::ControlContextLayer::PlanPhase);
        }
        retired
    }

    fn validate(&self) -> std::io::Result<()> {
        if !self.architecture_is_current() {
            return Err(crate::session::persistence::session_version_mismatch(
                "Session Control architecture",
                u64::from(self.architecture_version),
                u64::from(SESSION_CONTROL_ARCHITECTURE_VERSION),
            ));
        }
        if self.agent_name.trim().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "session control Agent name is empty",
            ));
        }
        if !self.behavior.runtime_fields_match_selection() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "session control Behavior contains cross-runtime state",
            ));
        }
        if !self.behavior.plan_runtime_is_valid() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "session control Plan handoff does not match its artifact and phase",
            ));
        }
        if let Some(receipt) = self.applied_control.as_ref() {
            if receipt.domain == crate::extensions::notification::ControlDomain::Sampling {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Sampling receipts belong in model.changed observations",
                ));
            }
            if receipt.target.domain() != receipt.domain {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "session control receipt target does not match its domain",
                ));
            }
            let receipt_matches_authoritative_state = match (&receipt.domain, &receipt.target) {
                (
                    crate::extensions::notification::ControlDomain::Agent,
                    crate::extensions::notification::ControlTarget::Agent { agent_name },
                ) => agent_name == &self.agent_name,
                (
                    crate::extensions::notification::ControlDomain::Behavior,
                    crate::extensions::notification::ControlTarget::Behavior { behavior_id },
                ) => behavior_id == self.behavior.behavior().as_id(),
                _ => false,
            };
            if !receipt_matches_authoritative_state {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "session control receipt target does not match authoritative state",
                ));
            }
            receipt
                .intent
                .validate()
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        }
        if let Some(goal) = self.goal.as_ref() {
            if goal.architecture_version != crate::session::goal_tracker::GOAL_ARCHITECTURE_VERSION
            {
                return Err(crate::session::persistence::session_version_mismatch(
                    "Goal architecture",
                    u64::from(goal.architecture_version),
                    u64::from(crate::session::goal_tracker::GOAL_ARCHITECTURE_VERSION),
                ));
            }
            crate::session::goal_tracker::GoalTracker::validate_snapshot(goal)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        }
        let active_goal = self
            .goal
            .as_ref()
            .is_some_and(|goal| goal.status == crate::session::goal_tracker::GoalStatus::Active);
        let goal_behavior = self.behavior.behavior() == tool_types::BehaviorId::Goal;
        if active_goal != goal_behavior && (active_goal || self.goal.is_some()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "active Goal and Goal Behavior ownership do not agree",
            ));
        }
        Ok(())
    }

    pub fn timeline_kind(&self) -> std::io::Result<chat_state::TimelineEventKind> {
        self.timeline_kind_inner(Vec::new())
    }

    pub fn timeline_kind_with_model_context(
        &self,
        layer: chat_state::ControlContextLayer,
        activation: chat_state::ControlContextActivation,
        context: impl Into<String>,
    ) -> std::io::Result<chat_state::TimelineEventKind> {
        self.timeline_kind_with_model_context_item(
            layer,
            activation,
            sampling_types::ConversationItem::system_reminder(context),
        )
    }

    pub fn timeline_kind_with_model_context_item(
        &self,
        layer: chat_state::ControlContextLayer,
        activation: chat_state::ControlContextActivation,
        item: sampling_types::ConversationItem,
    ) -> std::io::Result<chat_state::TimelineEventKind> {
        self.timeline_kind_with_model_context_items(vec![chat_state::ControlContext {
            layer,
            activation,
            item,
        }])
    }

    pub fn timeline_kind_with_model_context_items(
        &self,
        model_contexts: Vec<chat_state::ControlContext>,
    ) -> std::io::Result<chat_state::TimelineEventKind> {
        self.timeline_kind_inner(model_contexts)
    }

    fn timeline_kind_inner(
        &self,
        model_contexts: Vec<chat_state::ControlContext>,
    ) -> std::io::Result<chat_state::TimelineEventKind> {
        self.validate()?;
        let snapshot = serde_json::to_value(self)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        let retired_context_layers = self.retired_context_layers();
        Ok(chat_state::TimelineEventKind::Control(
            chat_state::ControlEvent {
                revision: self.control_revision,
                snapshot,
                retired_context_layers,
                model_contexts,
            },
        ))
    }

    pub fn latest_from_timeline(
        events: &[chat_state::TimelineEvent],
    ) -> std::io::Result<Option<Self>> {
        let mut latest = None;
        for control in events.iter().filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Control(control) => Some(control),
            _ => None,
        }) {
            let snapshot = Self::decode_persisted(control.snapshot.clone())?;
            if snapshot.control_revision != control.revision {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "control event revision {} does not match snapshot revision {}",
                        control.revision, snapshot.control_revision
                    ),
                ));
            }
            if snapshot.retired_context_layers() != control.retired_context_layers {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "session control context retirement does not match its snapshot",
                ));
            }
            latest = Some(snapshot);
        }
        Ok(latest)
    }

    pub fn durable_receipts_from_timeline(
        events: &[chat_state::TimelineEvent],
    ) -> std::io::Result<Vec<DurableControlReceipt>> {
        let mut receipts = Vec::new();
        for control in events.iter().filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Control(control) => Some(control),
            _ => None,
        }) {
            let snapshot = Self::decode_persisted(control.snapshot.clone())?;
            if let Some(receipt) = snapshot.applied_control {
                receipts.push(receipt);
            }
        }
        Ok(receipts)
    }
}

/// Render one append-only model context item for an Agent transition.
///
/// Agent identity and its rendered role are committed in the same Control
/// event. An Agent without an authored body still emits an explicit reset so
/// an earlier role cannot remain active by omission.
pub fn agent_role_transition_context(
    agent_name: &str,
    role_prompt: Option<&str>,
    capability_catalog: Option<&str>,
) -> String {
    let role = role_prompt
        .filter(|role| !role.trim().is_empty())
        .unwrap_or("This Agent defines no additional role instructions beyond the stable Grow system guidance.");
    let mut content = format!(
        "The active Agent is `{agent_name}`. This Agent role replaces every earlier Agent role; earlier role instructions are historical and no longer apply.\n\n{role}"
    );
    if let Some(catalog) = capability_catalog.filter(|catalog| !catalog.trim().is_empty()) {
        content.push_str(&format!(
            "\n\n<{}>\n{}\n</{}>",
            crate::session::subagent_capability::CAPABILITY_CATALOG_TAG,
            catalog,
            crate::session::subagent_capability::CAPABILITY_CATALOG_TAG,
        ));
    }
    let content = content.replace("</agent-role>", "<\\/agent-role>");
    format!("<agent-role>\n{content}\n</agent-role>")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goal(
        status: crate::session::goal_tracker::GoalStatus,
    ) -> crate::session::goal_tracker::GoalState {
        let mut tracker = crate::session::goal_tracker::GoalTracker::new();
        tracker
            .create_goal(
                "goal-1".into(),
                "ship safely".into(),
                None,
                "2026-08-26T00:00:00Z".into(),
            )
            .unwrap();
        match status {
            crate::session::goal_tracker::GoalStatus::Active => {}
            crate::session::goal_tracker::GoalStatus::Paused => {
                tracker.pause(crate::session::goal_tracker::GoalPauseReason::User);
            }
            crate::session::goal_tracker::GoalStatus::Blocked => {
                for index in 1..=3 {
                    tracker
                        .report_blocked(
                            "waiting for user".into(),
                            index,
                            (index > 1).then_some(index - 1),
                        )
                        .unwrap();
                }
            }
            crate::session::goal_tracker::GoalStatus::BudgetLimited => {
                tracker.budget_limit();
            }
            crate::session::goal_tracker::GoalStatus::Complete => {
                tracker.complete();
            }
        }
        tracker.snapshot().unwrap().clone()
    }

    fn snapshot(revision: u64) -> SessionControlSnapshot {
        SessionControlSnapshot::new(
            revision,
            "grow",
            crate::session::behavior::BehaviorSnapshot::normal(),
            None,
        )
    }

    #[test]
    fn latest_control_is_folded_from_timeline() {
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(snapshot(2).timeline_kind().unwrap())
            .unwrap();
        timeline
            .record(snapshot(5).timeline_kind().unwrap())
            .unwrap();

        let restored = SessionControlSnapshot::latest_from_timeline(timeline.events())
            .unwrap()
            .unwrap();
        assert_eq!(restored.control_revision, 5);
    }

    #[test]
    fn applied_control_receipt_is_folded_from_its_authoritative_fact() {
        let intent = crate::session::ControlIntent {
            client_id: "pager-a".into(),
            generation: 4,
            sequence: 9,
        };
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(
                snapshot(6)
                    .with_applied_control(
                        crate::extensions::notification::ControlDomain::Agent,
                        crate::extensions::notification::ControlTarget::Agent {
                            agent_name: "grow".into(),
                        },
                        Some(intent.clone()),
                    )
                    .timeline_kind()
                    .unwrap(),
            )
            .unwrap();

        assert_eq!(
            SessionControlSnapshot::durable_receipts_from_timeline(timeline.events()).unwrap(),
            vec![DurableControlReceipt {
                domain: crate::extensions::notification::ControlDomain::Agent,
                intent,
                target: crate::extensions::notification::ControlTarget::Agent {
                    agent_name: "grow".into(),
                },
            }]
        );
    }

    #[test]
    fn control_snapshot_rejects_sampling_or_empty_client_receipts() {
        let empty_client = crate::session::ControlIntent {
            client_id: "   ".into(),
            generation: 1,
            sequence: 1,
        };
        assert!(
            snapshot(1)
                .with_applied_control(
                    crate::extensions::notification::ControlDomain::Agent,
                    crate::extensions::notification::ControlTarget::Agent {
                        agent_name: "grow".into(),
                    },
                    Some(empty_client),
                )
                .timeline_kind()
                .is_err()
        );

        let sampling = crate::session::ControlIntent {
            client_id: "pager-a".into(),
            generation: 1,
            sequence: 2,
        };
        assert!(
            snapshot(2)
                .with_applied_control(
                    crate::extensions::notification::ControlDomain::Sampling,
                    crate::extensions::notification::ControlTarget::Sampling {
                        model_id: "provider/model".into(),
                        reasoning_effort: None,
                    },
                    Some(sampling),
                )
                .timeline_kind()
                .is_err()
        );
    }

    #[test]
    fn control_snapshot_rejects_receipts_that_disagree_with_authoritative_state() {
        let intent = crate::session::ControlIntent {
            client_id: "pager-a".into(),
            generation: 1,
            sequence: 1,
        };

        let wrong_agent = snapshot(1).with_applied_control(
            crate::extensions::notification::ControlDomain::Agent,
            crate::extensions::notification::ControlTarget::Agent {
                agent_name: "reviewer".into(),
            },
            Some(intent.clone()),
        );
        assert!(wrong_agent.timeline_kind().is_err());

        let wrong_behavior = snapshot(2).with_applied_control(
            crate::extensions::notification::ControlDomain::Behavior,
            crate::extensions::notification::ControlTarget::Behavior {
                behavior_id: "goal".into(),
            },
            Some(intent),
        );
        assert!(wrong_behavior.timeline_kind().is_err());
    }

    #[test]
    fn control_snapshot_and_model_context_share_one_event() {
        let mut timeline =
            chat_state::Timeline::from_seed(vec![sampling_types::ConversationItem::system(
                "system",
            )])
            .unwrap();
        let state = SessionControlSnapshot::new(
            1,
            "grow",
            crate::session::behavior::BehaviorSnapshot::selected(tool_types::BehaviorId::Plan),
            None,
        );
        let context =
            crate::session::behavior::behavior_transition_context(tool_types::BehaviorId::Plan);
        let event = timeline
            .record(
                state
                    .timeline_kind_with_model_context(
                        chat_state::ControlContextLayer::Behavior,
                        chat_state::ControlContextActivation::Transition,
                        context.clone(),
                    )
                    .unwrap(),
            )
            .unwrap();

        let chat_state::TimelineEventKind::Control(control) = event.kind else {
            unreachable!();
        };
        assert_eq!(
            control
                .model_contexts
                .first()
                .expect("control context")
                .item
                .text_content(),
            context,
            "the state transition and provider-visible protocol must be one durable fact"
        );
        assert_eq!(timeline.surface().last().unwrap().text_content(), context);
        assert_eq!(
            SessionControlSnapshot::latest_from_timeline(timeline.events())
                .unwrap()
                .unwrap()
                .behavior
                .behavior(),
            tool_types::BehaviorId::Plan
        );
    }

    #[test]
    fn malformed_earlier_control_event_fails_closed() {
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Control(
                chat_state::ControlEvent {
                    revision: 1,
                    snapshot: serde_json::json!({ "broken": true }),
                    retired_context_layers: vec![],
                    model_contexts: vec![],
                },
            ))
            .unwrap();
        timeline
            .record(snapshot(2).timeline_kind().unwrap())
            .unwrap();

        let error = SessionControlSnapshot::latest_from_timeline(timeline.events()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn control_architecture_is_checked_before_snapshot_shape() {
        let persisted = u64::from(SESSION_CONTROL_ARCHITECTURE_VERSION) + 1;
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Control(
                chat_state::ControlEvent {
                    revision: 1,
                    snapshot: serde_json::json!({
                        "architecture_version": persisted,
                        "future_field": true,
                    }),
                    retired_context_layers: vec![],
                    model_contexts: vec![],
                },
            ))
            .unwrap();

        let error = SessionControlSnapshot::latest_from_timeline(timeline.events()).unwrap_err();
        let mismatch = crate::session::persistence::session_version_mismatch_from(&error).unwrap();
        assert_eq!(mismatch.component, "Session Control architecture");
        assert_eq!(mismatch.persisted, persisted);
        assert_eq!(
            mismatch.current,
            u64::from(SESSION_CONTROL_ARCHITECTURE_VERSION),
        );
    }

    #[test]
    fn nested_goal_architecture_is_reported_separately() {
        let persisted = u64::from(crate::session::goal_tracker::GOAL_ARCHITECTURE_VERSION) - 1;
        let snapshot = SessionControlSnapshot::new(
            1,
            "grow",
            crate::session::behavior::BehaviorSnapshot::selected(tool_types::BehaviorId::Goal),
            Some(goal(crate::session::goal_tracker::GoalStatus::Active)),
        );
        let mut value = serde_json::to_value(snapshot).unwrap();
        value["goal"]["architecture_version"] = persisted.into();
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Control(
                chat_state::ControlEvent {
                    revision: 1,
                    snapshot: value,
                    retired_context_layers: vec![],
                    model_contexts: vec![],
                },
            ))
            .unwrap();

        let error = SessionControlSnapshot::latest_from_timeline(timeline.events()).unwrap_err();
        let mismatch = crate::session::persistence::session_version_mismatch_from(&error).unwrap();
        assert_eq!(mismatch.component, "Goal architecture");
        assert_eq!(mismatch.persisted, persisted);
        assert_eq!(
            mismatch.current,
            u64::from(crate::session::goal_tracker::GOAL_ARCHITECTURE_VERSION),
        );
    }

    #[test]
    fn event_and_snapshot_revision_must_match() {
        let mut timeline = chat_state::Timeline::default();
        let mut kind = snapshot(3).timeline_kind().unwrap();
        let chat_state::TimelineEventKind::Control(control) = &mut kind else {
            unreachable!();
        };
        control.revision = 4;
        timeline.record(kind).unwrap();

        let error = SessionControlSnapshot::latest_from_timeline(timeline.events()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn goal_runtime_ownership_must_be_atomic_with_behavior() {
        let active_outside_goal = SessionControlSnapshot::new(
            1,
            "grow",
            crate::session::behavior::BehaviorSnapshot::normal(),
            Some(goal(crate::session::goal_tracker::GoalStatus::Active)),
        );
        assert!(active_outside_goal.timeline_kind().is_err());

        let stopped_inside_goal = SessionControlSnapshot::new(
            1,
            "grow",
            crate::session::behavior::BehaviorSnapshot::selected(tool_types::BehaviorId::Goal),
            Some(goal(crate::session::goal_tracker::GoalStatus::Paused)),
        );
        assert!(stopped_inside_goal.timeline_kind().is_err());

        let awaiting_objective = SessionControlSnapshot::new(
            1,
            "grow",
            crate::session::behavior::BehaviorSnapshot::selected(tool_types::BehaviorId::Goal),
            None,
        );
        assert!(awaiting_objective.timeline_kind().is_ok());
    }

    #[test]
    fn malformed_goal_payload_cannot_be_silently_dropped_on_load() {
        let mut snapshot = SessionControlSnapshot::new(
            1,
            "grow",
            crate::session::behavior::BehaviorSnapshot::selected(tool_types::BehaviorId::Goal),
            Some(goal(crate::session::goal_tracker::GoalStatus::Active)),
        );
        snapshot.goal.as_mut().unwrap().objective = "   ".into();
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Control(
                chat_state::ControlEvent {
                    revision: 1,
                    snapshot: serde_json::to_value(snapshot).unwrap(),
                    retired_context_layers: vec![],
                    model_contexts: vec![],
                },
            ))
            .unwrap();

        assert!(SessionControlSnapshot::latest_from_timeline(timeline.events()).is_err());
    }

    #[test]
    fn agent_role_transition_always_retires_the_previous_role() {
        let custom = agent_role_transition_context(
            "reviewer",
            Some("Review carefully."),
            Some("- available native tools: read(Read)"),
        );
        assert!(custom.contains("active Agent is `reviewer`"));
        assert!(custom.contains("replaces every earlier Agent role"));
        assert!(custom.contains("Review carefully."));
        assert!(custom.contains("<subagent-capability-catalog>"));

        let reset = agent_role_transition_context("grow", None, None);
        assert!(reset.contains("no additional role instructions"));
        assert_eq!(reset.matches("<agent-role>").count(), 1);
    }

    #[test]
    fn normal_behavior_retires_any_goal_definition_context() {
        let kind = snapshot(1).timeline_kind().unwrap();
        let chat_state::TimelineEventKind::Control(control) = kind else {
            unreachable!();
        };
        assert_eq!(
            control.retired_context_layers,
            [
                chat_state::ControlContextLayer::GoalDefinition,
                chat_state::ControlContextLayer::PlanPhase,
            ]
        );
    }

    #[test]
    fn active_goal_keeps_its_goal_definition_context() {
        let state = SessionControlSnapshot::new(
            1,
            "grow",
            crate::session::behavior::BehaviorSnapshot::selected(tool_types::BehaviorId::Goal),
            Some(goal(crate::session::goal_tracker::GoalStatus::Active)),
        );
        let kind = state.timeline_kind().unwrap();
        let chat_state::TimelineEventKind::Control(control) = kind else {
            unreachable!();
        };
        assert_eq!(
            control.retired_context_layers,
            [chat_state::ControlContextLayer::PlanPhase]
        );
    }
}
