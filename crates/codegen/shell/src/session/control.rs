//! Timeline-backed session control-plane snapshot.
//!
//! Timeline control events are the only persisted source for Behavior selection
//! and the Goal runtime. Runtime leases, cancellation handles, activity
//! projections and UI clocks are intentionally absent and reconstructed after
//! reload.

pub const SESSION_CONTROL_ARCHITECTURE_VERSION: u32 = 1;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionControlSnapshot {
    pub architecture_version: u32,
    pub control_revision: u64,
    pub behavior: crate::session::behavior::BehaviorSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<crate::session::goal_tracker::GoalOrchestration>,
}

impl SessionControlSnapshot {
    pub fn new(
        control_revision: u64,
        behavior: crate::session::behavior::BehaviorSnapshot,
        goal: Option<crate::session::goal_tracker::GoalOrchestration>,
    ) -> Self {
        Self {
            architecture_version: SESSION_CONTROL_ARCHITECTURE_VERSION,
            control_revision,
            behavior,
            goal,
        }
    }

    pub fn architecture_is_current(&self) -> bool {
        self.architecture_version == SESSION_CONTROL_ARCHITECTURE_VERSION
    }

    pub fn timeline_kind(&self) -> std::io::Result<chat_state::TimelineEventKind> {
        let snapshot = serde_json::to_value(self)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        Ok(chat_state::TimelineEventKind::Control(
            chat_state::ControlEvent {
                revision: self.control_revision,
                snapshot,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(revision: u64) -> SessionControlSnapshot {
        SessionControlSnapshot::new(
            revision,
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
    fn malformed_earlier_control_event_fails_closed() {
        let mut timeline = chat_state::Timeline::default();
        timeline
            .record(chat_state::TimelineEventKind::Control(
                chat_state::ControlEvent {
                    revision: 1,
                    snapshot: serde_json::json!({ "broken": true }),
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
}
