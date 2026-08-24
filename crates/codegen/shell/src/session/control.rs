//! Timeline-backed session control-plane snapshot.
//!
//! Timeline control events are the only persisted source for Agent selection,
//! Behavior selection, and the Goal runtime. Runtime leases, cancellation
//! handles, activity projections and UI clocks are intentionally absent and
//! reconstructed after reload.

pub const SESSION_CONTROL_ARCHITECTURE_VERSION: u32 = 3;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionControlSnapshot {
    pub architecture_version: u32,
    pub control_revision: u64,
    pub agent_name: String,
    pub behavior: crate::session::behavior::BehaviorSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<crate::session::goal_tracker::GoalState>,
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
        }
    }

    pub fn architecture_is_current(&self) -> bool {
        self.architecture_version == SESSION_CONTROL_ARCHITECTURE_VERSION
    }

    pub fn timeline_kind(&self) -> std::io::Result<chat_state::TimelineEventKind> {
        self.timeline_kind_inner(None)
    }

    pub fn timeline_kind_with_model_context(
        &self,
        layer: chat_state::ControlContextLayer,
        activation: chat_state::ControlContextActivation,
        context: impl Into<String>,
    ) -> std::io::Result<chat_state::TimelineEventKind> {
        self.timeline_kind_inner(Some(chat_state::ControlContext {
            layer,
            activation,
            item: sampling_types::ConversationItem::system_reminder(context),
        }))
    }

    fn timeline_kind_inner(
        &self,
        model_context: Option<chat_state::ControlContext>,
    ) -> std::io::Result<chat_state::TimelineEventKind> {
        let snapshot = serde_json::to_value(self)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        Ok(chat_state::TimelineEventKind::Control(
            chat_state::ControlEvent {
                revision: self.control_revision,
                snapshot,
                model_context,
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
            let snapshot: Self = serde_json::from_value(control.snapshot.clone())
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            if !snapshot.architecture_is_current() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "unsupported session control architecture {}",
                        snapshot.architecture_version
                    ),
                ));
            }
            if snapshot.agent_name.trim().is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "session control Agent name is empty",
                ));
            }
            if snapshot.control_revision != control.revision {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "control event revision {} does not match snapshot revision {}",
                        control.revision, snapshot.control_revision
                    ),
                ));
            }
            latest = Some(snapshot);
        }
        Ok(latest)
    }
}

/// Render one append-only model context item for an Agent transition.
///
/// Agent identity and its rendered role are committed in the same Control
/// event. An Agent without an authored body still emits an explicit reset so
/// an earlier role cannot remain active by omission.
pub fn agent_role_transition_context(agent_name: &str, role_prompt: Option<&str>) -> String {
    let role = role_prompt
        .filter(|role| !role.trim().is_empty())
        .unwrap_or("This Agent defines no additional role instructions beyond the stable Grow system guidance.");
    let content = format!(
        "The active Agent is `{agent_name}`. This Agent role replaces every earlier Agent role; earlier role instructions are historical and no longer apply.\n\n{role}"
    )
    .replace("</agent-role>", "<\\/agent-role>");
    format!("<agent-role>\n{content}\n</agent-role>")
}

#[cfg(test)]
mod tests {
    use super::*;

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
            control.model_context.unwrap().item.text_content(),
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
                    model_context: None,
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
    fn agent_role_transition_always_retires_the_previous_role() {
        let custom = agent_role_transition_context("reviewer", Some("Review carefully."));
        assert!(custom.contains("active Agent is `reviewer`"));
        assert!(custom.contains("replaces every earlier Agent role"));
        assert!(custom.contains("Review carefully."));

        let reset = agent_role_transition_context("grow", None);
        assert!(reset.contains("no additional role instructions"));
        assert_eq!(reset.matches("<agent-role>").count(), 1);
    }
}
