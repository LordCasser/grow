//! Time evidence retained from the already validated, captured ledgers.
//!
//! A lack of display updates is not evidence of idleness. In particular,
//! ordinary tool spans include permission gates, and recovered terminals are
//! written at reopen time rather than at the instant the process stopped.

use std::collections::BTreeMap;
use std::ops::Range;
use std::time::{SystemTime, UNIX_EPOCH};

use chat_state::{
    CompactionEvent, HookEvent, RequestEvent, SidebandEvent, SidebandEventKind, TimelineEvent,
    TimelineEventKind, ToolEvent, WorkflowEvent,
};

#[derive(Debug, Default)]
pub(super) struct Activity {
    pub protected: Vec<Range<u64>>,
    pub recovery_gaps: Vec<Range<u64>>,
    pub open_starts: Vec<u64>,
    pub unknown_activity: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Work {
    Request(String),
    Tool(String),
    Hook(String, String),
    Compaction(String),
    Workflow(String, u64),
}

fn interval(start: u64, end: u64) -> Option<Range<u64>> {
    (end > start).then_some(start..end)
}

fn recover_or_protect(
    out: &mut Activity,
    start: u64,
    end: u64,
    recovered: bool,
    last_evidence: Option<u64>,
    known_waits: &[Range<u64>],
) {
    if !recovered {
        if let Some(span) = interval(start, end) {
            let mut cursor = span.start;
            for wait in known_waits {
                if wait.end <= cursor || wait.start >= span.end {
                    continue;
                }
                if wait.start > cursor {
                    out.protected.push(cursor..wait.start.min(span.end));
                }
                cursor = cursor.max(wait.end).min(span.end);
            }
            if cursor < span.end {
                out.protected.push(cursor..span.end);
            }
        }
        return;
    }
    let boundary = last_evidence.unwrap_or(start).clamp(start, end);
    if let Some(span) = interval(start, boundary) {
        out.protected.push(span);
    }
    if let Some(span) = interval(boundary, end) {
        out.recovery_gaps.push(span);
    }
}

/// These intervals are deliberately conservative. A normal request/tool/hook
/// protects its whole span, even if it may contain an unpersisted human wait.
pub(super) fn from_timeline(events: &[TimelineEvent]) -> Activity {
    let mut out = Activity::default();
    let mut open = BTreeMap::<Work, u64>::new();
    let mut last_non_recovery_at = None;
    // `recover_interrupted` writes a Recovery marker before its synthetic
    // terminal batch. The marker and subsequent terminals are reopen-time
    // writes, not proof that the old operation ran until that instant.
    let mut recovery_boundary: Option<Option<u64>> = None;
    let mut horizon = None;
    let mut plan_tools = BTreeMap::<String, ()>::new();
    let mut pending_plan_wait: Option<(String, u64)> = None;
    let mut known_plan_waits = BTreeMap::<String, Vec<Range<u64>>>::new();
    let mut completed_plan_waits = Vec::<Range<u64>>::new();

    for event in events {
        let Some(at) = u64::try_from(event.at_ms).ok() else {
            continue;
        };
        horizon = Some(horizon.map_or(at, |old: u64| old.max(at)));
        let mut recovered = false;
        let finished = match &event.kind {
            TimelineEventKind::Recovery(recovery)
                if recovery.action == "close_interrupted_work" =>
            {
                recovery_boundary = Some(last_non_recovery_at);
                None
            }
            TimelineEventKind::Request(RequestEvent::Started { id, .. }) => {
                recovery_boundary = None;
                open.insert(Work::Request(id.clone()), at);
                None
            }
            TimelineEventKind::Request(RequestEvent::Completed { id, .. })
            | TimelineEventKind::Request(RequestEvent::Failed { id, .. }) => {
                Some(Work::Request(id.clone()))
            }
            TimelineEventKind::Request(RequestEvent::Cancelled { id, reason, .. }) => {
                recovered = reason == "process_interrupted";
                Some(Work::Request(id.clone()))
            }
            TimelineEventKind::Tool(ToolEvent::Started { call_id, name, .. }) => {
                recovery_boundary = None;
                open.insert(Work::Tool(call_id.clone()), at);
                if name == "exit_plan_mode" {
                    plan_tools.insert(call_id.clone(), ());
                }
                None
            }
            TimelineEventKind::Tool(ToolEvent::Completed {
                call_id,
                outcome,
                details,
                ..
            }) => {
                recovered = outcome == "outcome_unknown"
                    && details
                        .as_ref()
                        .and_then(|value| value.get("recovered"))
                        .and_then(serde_json::Value::as_bool)
                        == Some(true);
                Some(Work::Tool(call_id.clone()))
            }
            TimelineEventKind::Hook(HookEvent::RunStarted {
                occurrence_id,
                run_id,
                ..
            }) => {
                open.insert(Work::Hook(occurrence_id.clone(), run_id.clone()), at);
                None
            }
            TimelineEventKind::Hook(HookEvent::RunFinished {
                occurrence_id,
                run_id,
                outcome,
                ..
            }) => {
                recovered = matches!(
                    outcome,
                    chat_state::HookRunOutcome::InterruptedOutcomeUnknown
                );
                Some(Work::Hook(occurrence_id.clone(), run_id.clone()))
            }
            TimelineEventKind::Compaction(CompactionEvent::Started { id, .. }) => {
                recovery_boundary = None;
                open.insert(Work::Compaction(id.clone()), at);
                None
            }
            TimelineEventKind::Compaction(CompactionEvent::Completed { id, .. }) => {
                recovered = recovery_boundary.is_some();
                Some(Work::Compaction(id.clone()))
            }
            TimelineEventKind::Compaction(CompactionEvent::Failed { id, error, .. }) => {
                recovered = error == "process_interrupted";
                Some(Work::Compaction(id.clone()))
            }
            TimelineEventKind::Workflow(WorkflowEvent::Spawned {
                run_id,
                execution_epoch,
                ..
            })
            | TimelineEventKind::Workflow(WorkflowEvent::Resumed {
                run_id,
                execution_epoch,
            }) => {
                recovery_boundary = None;
                open.insert(Work::Workflow(run_id.clone(), *execution_epoch), at);
                None
            }
            TimelineEventKind::Workflow(WorkflowEvent::Ended {
                run_id,
                execution_epoch,
                status,
                message,
                ..
            }) => {
                recovered = *status == chat_state::WorkflowExecutionStatus::Interrupted
                    && message.as_deref() == Some("process_interrupted");
                Some(Work::Workflow(run_id.clone(), *execution_epoch))
            }
            TimelineEventKind::Workflow(WorkflowEvent::Closed {
                run_id,
                execution_epoch,
                ..
            }) => Some(Work::Workflow(run_id.clone(), *execution_epoch)),
            TimelineEventKind::Control(control) => {
                let pending = control
                    .snapshot
                    .get("behavior")
                    .and_then(|value| value.get("approval_pending"))
                    .and_then(serde_json::Value::as_bool);
                if pending == Some(true) && pending_plan_wait.is_none() && plan_tools.len() == 1 {
                    let call_id = plan_tools.keys().next().expect("one candidate").clone();
                    pending_plan_wait = Some((call_id, at));
                } else if pending == Some(false)
                    && let Some((call_id, began)) = pending_plan_wait.take()
                    && plan_tools.contains_key(&call_id)
                    && let Some(wait) = interval(began, at)
                {
                    known_plan_waits.entry(call_id).or_default().push(wait);
                }
                None
            }
            _ => None,
        };
        if let Some(work) = finished {
            if let Some(start) = open.remove(&work) {
                let waits = match &work {
                    Work::Tool(call_id) => known_plan_waits.remove(call_id).unwrap_or_default(),
                    _ => Vec::new(),
                };
                if matches!(&work, Work::Tool(_)) {
                    completed_plan_waits.extend(waits.iter().cloned());
                }
                let request_waits =
                    matches!(&work, Work::Request(_)).then(|| merged(completed_plan_waits.clone()));
                let waits = request_waits.as_deref().unwrap_or(&waits);
                let evidence = if recovered {
                    recovery_boundary.unwrap_or(last_non_recovery_at)
                } else {
                    last_non_recovery_at
                };
                recover_or_protect(&mut out, start, at, recovered, evidence, waits);
            }
            if let Work::Tool(call_id) = work {
                plan_tools.remove(&call_id);
                if pending_plan_wait
                    .as_ref()
                    .is_some_and(|(id, _)| id == &call_id)
                {
                    pending_plan_wait = None;
                }
            }
        }
        if !recovered && !matches!(&event.kind, TimelineEventKind::Recovery(_)) {
            last_non_recovery_at = Some(at);
        }
    }
    if let Some(end) = horizon {
        for start in open.into_values() {
            out.open_starts.push(start);
            if let Some(span) = interval(start, end) {
                out.protected.push(span);
            }
        }
    }
    out.protected = merged(out.protected);
    out.recovery_gaps = merged(out.recovery_gaps);
    out
}

/// Sideband request/end facts are validated in the same capture. A sideband
/// without a terminal remains potentially active to its captured horizon.
pub(super) fn sideband_intervals(events: &[SidebandEvent]) -> Vec<Range<u64>> {
    let mut start = None;
    let mut end = None;
    for event in events {
        let Some(at) = u64::try_from(event.at_ms).ok() else {
            continue;
        };
        match &event.kind {
            SidebandEventKind::Request(_) => start.get_or_insert(at),
            SidebandEventKind::End(_) => end.get_or_insert(at),
            _ => continue,
        };
    }
    start
        // An unterminated sideband may still be running while a sibling emits
        // updates. The global time map clips this protection at its frontier.
        .zip(end.or(Some(u64::MAX)))
        .and_then(|(a, b)| interval(a, b))
        .into_iter()
        .collect()
}

fn absolute_ms(time: SystemTime) -> Option<u64> {
    u64::try_from(time.duration_since(UNIX_EPOCH).ok()?.as_millis()).ok()
}

/// Persisted task notifications protect work after a foreground bash call
/// becomes a background task. The captured snapshot is authoritative; this
/// never opens the task's current output file or contacts its runtime store.
pub(super) fn background_task_activity(events: &[super::TranscriptEvent]) -> Activity {
    use crate::extensions::notification::SessionUpdate as GrowUpdate;
    use crate::session::storage::SessionUpdate;

    let mut activity = Activity::default();
    let mut started = BTreeMap::<String, Option<u64>>::new();
    for event in events {
        let SessionUpdate::Grow(notification) = &event.update else {
            continue;
        };
        let at = notification
            .meta
            .as_ref()
            .and_then(|meta| meta.get("agentTimestampMs"))
            .and_then(serde_json::Value::as_u64)
            .or(event.timestamp_ms);
        match &notification.update {
            GrowUpdate::TaskBackgrounded { task_id, .. } => {
                started.entry(task_id.clone()).or_insert(at);
            }
            GrowUpdate::TaskCompleted { task_snapshot } => {
                let observed = started.remove(&task_snapshot.task_id).flatten();
                let start = observed.or_else(|| absolute_ms(task_snapshot.start_time));
                let end = task_snapshot.end_time.and_then(absolute_ms).or(at);
                match (start, end) {
                    (Some(start), Some(end)) => {
                        if let Some(span) = interval(start, end) {
                            activity.protected.push(span);
                        } else {
                            activity.unknown_activity = true;
                        }
                    }
                    (Some(start), None) => activity.open_starts.push(start),
                    _ => activity.unknown_activity = true,
                }
            }
            _ => {}
        }
    }
    for start in started.into_values() {
        match start {
            Some(start) => activity.open_starts.push(start),
            None => activity.unknown_activity = true,
        }
    }
    activity.protected = merged(activity.protected);
    activity
}

pub(super) fn merged(mut spans: Vec<Range<u64>>) -> Vec<Range<u64>> {
    spans.sort_by_key(|span| (span.start, span.end));
    let mut out: Vec<Range<u64>> = Vec::new();
    for span in spans {
        if let Some(previous) = out.last_mut()
            && span.start <= previous.end
        {
            previous.end = previous.end.max(span.end);
            continue;
        }
        out.push(span);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::notification::{SessionNotification, SessionUpdate as GrowUpdate};
    use crate::session::storage::SessionUpdate;
    use acp_transport::protocol as acp;

    fn task_event(update: GrowUpdate, at: Option<u64>) -> super::super::TranscriptEvent {
        super::super::TranscriptEvent {
            update: SessionUpdate::Grow(Box::new(SessionNotification {
                session_id: acp::SessionId::new("root"),
                update,
                meta: None,
            })),
            timestamp_ms: at,
            simulated_source: false,
        }
    }

    fn backgrounded(at: Option<u64>) -> super::super::TranscriptEvent {
        task_event(
            GrowUpdate::TaskBackgrounded {
                tool_call_id: "call".into(),
                task_id: "task".into(),
                command: "sleep 90".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/task.log".into(),
                monitor_description: None,
                description: None,
            },
            at,
        )
    }

    fn completed() -> super::super::TranscriptEvent {
        let start = UNIX_EPOCH + std::time::Duration::from_millis(1_000);
        task_event(
            GrowUpdate::TaskCompleted {
                task_snapshot: tools::types::TaskSnapshot {
                    task_id: "task".into(),
                    command: "sleep 90".into(),
                    display_command: None,
                    cwd: "/tmp".into(),
                    start_time: start,
                    end_time: Some(UNIX_EPOCH + std::time::Duration::from_millis(90_000)),
                    output: String::new(),
                    output_file: "/tmp/task.log".into(),
                    truncated: false,
                    exit_code: Some(0),
                    signal: None,
                    completed: true,
                    kind: Default::default(),
                    block_waited: false,
                    explicitly_killed: false,
                    owner_session_id: None,
                    goal_id: None,
                    goal_definition_revision: None,
                    description: None,
                    is_backgrounded: true,
                },
            },
            Some(90_000),
        )
    }

    #[test]
    fn recovery_gap_is_not_treated_as_executing_time() {
        let mut out = Activity::default();
        recover_or_protect(&mut out, 1_000, 3_600_000, true, Some(1_500), &[]);
        assert_eq!(out.protected, vec![1_000..1_500]);
        assert_eq!(out.recovery_gaps, vec![1_500..3_600_000]);
    }

    #[test]
    fn background_task_protects_its_recorded_runtime_and_open_tail() {
        let activity = background_task_activity(&[backgrounded(Some(2_000)), completed()]);
        assert_eq!(activity.protected, vec![2_000..90_000]);
        assert!(activity.open_starts.is_empty());
        assert!(!activity.unknown_activity);

        let open = background_task_activity(&[backgrounded(Some(2_000))]);
        assert_eq!(open.open_starts, vec![2_000]);
        assert!(!open.unknown_activity);

        let unknown = background_task_activity(&[backgrounded(None)]);
        assert!(unknown.unknown_activity);
    }

    #[test]
    fn overlapping_protection_is_coalesced() {
        assert_eq!(merged(vec![20..35, 0..25, 50..60]), vec![0..35, 50..60]);
    }

    #[test]
    fn only_a_verified_plan_wait_is_removed_from_its_execution_span() {
        let mut out = Activity::default();
        recover_or_protect(&mut out, 0, 100_000, false, None, &[10_000..90_000]);
        assert_eq!(out.protected, vec![0..10_000, 90_000..100_000]);
        // An unrelated concurrent operation still protects that same wait.
        out.protected.push(20_000..80_000);
        assert_eq!(
            merged(out.protected),
            vec![0..10_000, 20_000..80_000, 90_000..100_000]
        );
    }

    #[test]
    fn reopen_marker_does_not_prove_old_request_was_running_until_reopen() {
        use chat_state::{EventSeq, RecoveryEvent, StepId, TurnId};
        let turn = TurnId(1);
        let kinds = vec![
            TimelineEventKind::Request(RequestEvent::Started {
                id: "request".into(),
                turn,
                step: StepId { turn, index: 0 },
                model_id: "model".into(),
                input_message_count: 1,
                tool_count: 0,
            }),
            TimelineEventKind::Recovery(RecoveryEvent {
                action: "close_interrupted_work".into(),
                correlation_id: None,
                reason: "process ended".into(),
                details: None,
            }),
            TimelineEventKind::Request(RequestEvent::Cancelled {
                id: "request".into(),
                duration_ms: 3_599_001,
                reason: "process_interrupted".into(),
            }),
        ];
        let at = [1_000, 3_600_000, 3_600_001];
        let events = kinds
            .into_iter()
            .enumerate()
            .map(|(index, kind)| TimelineEvent {
                version: chat_state::TIMELINE_SCHEMA_VERSION,
                seq: EventSeq::new(index as u64 + 1),
                at_ms: at[index],
                kind,
            })
            .collect::<Vec<_>>();
        let activity = from_timeline(&events);
        assert!(activity.protected.is_empty());
        assert_eq!(activity.recovery_gaps, vec![1_000..3_600_001]);
    }
}
