use chat_state::{PendingTimelineCommit, TimelineWriteError};
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FinalizationStage {
    ChildSettlement,
    Output,
    ChildResult,
    ParentUsage,
    ParentTerminal,
    Receipt,
    RecoveryOwnership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommitCertainty {
    NotDispatched,
    Unknown,
    Committed,
    IntegrityConflict,
}

/// Display text is never used to decide whether another write is safe.
#[derive(Debug, Clone)]
pub(super) struct FinalizationFailure {
    pub stage: FinalizationStage,
    pub certainty: CommitCertainty,
    pub error_kind: Option<io::ErrorKind>,
    pub pending: Option<PendingTimelineCommit>,
    pub message: String,
}

impl FinalizationFailure {
    pub fn observation(stage: FinalizationStage, message: impl Into<String>) -> Self {
        Self {
            stage,
            certainty: CommitCertainty::Unknown,
            error_kind: None,
            pending: None,
            message: message.into(),
        }
    }

    pub fn io(
        stage: FinalizationStage,
        error: &io::Error,
        pending: Option<PendingTimelineCommit>,
    ) -> Self {
        Self {
            stage,
            certainty: if error.kind() == io::ErrorKind::InvalidData {
                CommitCertainty::IntegrityConflict
            } else {
                CommitCertainty::Unknown
            },
            error_kind: Some(error.kind()),
            pending,
            message: error.to_string(),
        }
    }

    pub fn timeline(
        stage: FinalizationStage,
        error: &TimelineWriteError,
        pending: Option<PendingTimelineCommit>,
    ) -> Self {
        let (certainty, error_kind) = match error {
            TimelineWriteError::Invalid(_) => (CommitCertainty::NotDispatched, None),
            TimelineWriteError::CommittedProjectionInvalid(_) => (CommitCertainty::Committed, None),
            TimelineWriteError::Persistence(error) => (
                if error.kind() == io::ErrorKind::InvalidData {
                    CommitCertainty::IntegrityConflict
                } else {
                    CommitCertainty::Unknown
                },
                Some(error.kind()),
            ),
            TimelineWriteError::DeadlineExceeded => (
                if pending.as_ref().is_some_and(|p| p.acknowledgement_lost) {
                    CommitCertainty::Unknown
                } else {
                    CommitCertainty::NotDispatched
                },
                None,
            ),
            TimelineWriteError::AcknowledgementLost | TimelineWriteError::Cancelled => {
                (CommitCertainty::Unknown, None)
            }
            _ => (CommitCertainty::IntegrityConflict, None),
        };
        Self {
            stage,
            certainty,
            error_kind,
            pending,
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for FinalizationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?}/{:?} ({:?}, pending_seq={:?}): {}",
            self.stage,
            self.certainty,
            self.error_kind,
            self.pending.as_ref().map(|p| p.event.seq.get()),
            self.message
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_failure_mapping_preserves_certainty_and_io_kind() {
        let full = TimelineWriteError::Persistence(io::ErrorKind::StorageFull.into());
        let failure = FinalizationFailure::timeline(FinalizationStage::Output, &full, None);
        assert_eq!(failure.certainty, CommitCertainty::Unknown);
        assert_eq!(failure.error_kind, Some(io::ErrorKind::StorageFull));
        let lost = FinalizationFailure::timeline(
            FinalizationStage::ChildResult,
            &TimelineWriteError::AcknowledgementLost,
            None,
        );
        assert_eq!(lost.certainty, CommitCertainty::Unknown);
        assert_eq!(lost.error_kind, None);
        let conflict = FinalizationFailure::timeline(
            FinalizationStage::ParentTerminal,
            &TimelineWriteError::SubagentUsageConflict,
            None,
        );
        assert_eq!(conflict.certainty, CommitCertainty::IntegrityConflict);
        let invalid = FinalizationFailure::io(
            FinalizationStage::RecoveryOwnership,
            &io::ErrorKind::InvalidData.into(),
            None,
        );
        assert_eq!(invalid.certainty, CommitCertainty::IntegrityConflict);
    }

    #[test]
    fn timeline_deadline_certainty_tracks_whether_the_append_was_dispatched() {
        let not_dispatched = FinalizationFailure::timeline(
            FinalizationStage::ChildResult,
            &TimelineWriteError::DeadlineExceeded,
            None,
        );
        assert_eq!(not_dispatched.stage, FinalizationStage::ChildResult);
        assert_eq!(not_dispatched.certainty, CommitCertainty::NotDispatched);
        assert_eq!(not_dispatched.error_kind, None);
        assert!(not_dispatched.pending.is_none());

        let pending = PendingTimelineCommit {
            event: chat_state::Timeline::default()
                .record(chat_state::TimelineEventKind::Observation(
                    chat_state::ObservationEvent {
                        scope: "finalization".into(),
                        name: "deadline-after-dispatch".into(),
                        turn: None,
                        step: None,
                        data: None,
                    },
                ))
                .expect("test event is valid"),
            error_kind: None,
            error_message: None,
            acknowledgement_lost: true,
        };
        let unknown = FinalizationFailure::timeline(
            FinalizationStage::Receipt,
            &TimelineWriteError::DeadlineExceeded,
            Some(pending.clone()),
        );
        assert_eq!(unknown.stage, FinalizationStage::Receipt);
        assert_eq!(unknown.certainty, CommitCertainty::Unknown);
        assert_eq!(unknown.error_kind, None);
        assert_eq!(
            unknown.pending.as_ref().unwrap().event.seq,
            pending.event.seq
        );
        assert!(unknown.pending.as_ref().unwrap().acknowledgement_lost);

        let mut prepared_only = pending;
        prepared_only.acknowledgement_lost = false;
        let not_dispatched_with_pending_identity = FinalizationFailure::timeline(
            FinalizationStage::ParentTerminal,
            &TimelineWriteError::DeadlineExceeded,
            Some(prepared_only),
        );
        assert_eq!(
            not_dispatched_with_pending_identity.certainty,
            CommitCertainty::NotDispatched
        );
    }

    #[test]
    fn acknowledged_projection_failure_is_committed_not_retryable_as_a_new_fact() {
        let pending = PendingTimelineCommit {
            event: chat_state::Timeline::default()
                .record(chat_state::TimelineEventKind::Observation(
                    chat_state::ObservationEvent {
                        scope: "finalization".into(),
                        name: "parent-terminal".into(),
                        turn: None,
                        step: None,
                        data: None,
                    },
                ))
                .expect("test event is valid"),
            error_kind: None,
            error_message: None,
            acknowledgement_lost: false,
        };
        let failure = FinalizationFailure::timeline(
            FinalizationStage::ParentTerminal,
            &TimelineWriteError::CommittedProjectionInvalid(
                chat_state::TimelineError::InvalidSubagent,
            ),
            Some(pending.clone()),
        );

        assert_eq!(failure.stage, FinalizationStage::ParentTerminal);
        assert_eq!(failure.certainty, CommitCertainty::Committed);
        assert_eq!(failure.error_kind, None);
        assert_eq!(
            serde_json::to_value(&failure.pending.unwrap().event).unwrap(),
            serde_json::to_value(pending.event).unwrap()
        );
    }
}
