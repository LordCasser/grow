//! Ignored release-mode baseline for the normal response storage commit path.
//!
//! This measures only the Timeline and updates ledgers. The probe counts below
//! do not include summary, artifact, or other system calls.

use super::*;
use crate::session::info::Info;
use crate::session::persistence::default_model_id;
use crate::session::storage::StorageAdapter;
use acp_transport::protocol as acp;
use chat_state::{
    MessageCause, MessageEvent, RequestEvent, RequestUsage, ResponseAdmission,
    ResponseAdmissionIdentity, StepEvent, Timeline, TimelineEventKind, TurnEvent, TurnIdentity,
    TurnInputKind, TurnTerminal, TurnTerminalSource,
};
use sampling_types::ConversationItem;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

const WARMUP_ROUNDS: usize = 3;
const MEASURED_ROUNDS: usize = 21;

#[derive(Clone, Copy, Default)]
struct BarrierCounts {
    update_file: usize,
    update_directory: usize,
    timeline_file: usize,
    timeline_directory: usize,
}

struct ProbeCounts {
    update_file: AtomicUsize,
    update_directory: AtomicUsize,
    timeline_file: AtomicUsize,
    timeline_directory: AtomicUsize,
}

impl ProbeCounts {
    fn new() -> Self {
        Self {
            update_file: AtomicUsize::new(0),
            update_directory: AtomicUsize::new(0),
            timeline_file: AtomicUsize::new(0),
            timeline_directory: AtomicUsize::new(0),
        }
    }

    fn take(&self) -> BarrierCounts {
        BarrierCounts {
            update_file: self.update_file.swap(0, Ordering::Relaxed),
            update_directory: self.update_directory.swap(0, Ordering::Relaxed),
            timeline_file: self.timeline_file.swap(0, Ordering::Relaxed),
            timeline_directory: self.timeline_directory.swap(0, Ordering::Relaxed),
        }
    }
}

#[derive(Default)]
struct PhaseTimes {
    candidate_anchor: Duration,
    timeline_admission: Duration,
    projection_build: Duration,
    projection_commit: Duration,
    total: Duration,
}

fn candidate_anchor(info: &Info, request_id: &str, event_id: &str) -> acp::SessionNotification {
    let mut notification = acp::SessionNotification::new(
        info.id.clone(),
        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new(String::new()),
        ))),
    );
    notification.meta = Some(
        serde_json::json!({
            "eventId": event_id,
            "samplingRequestId": request_id,
            "samplingAttempt": 1,
        })
        .as_object()
        .cloned()
        .expect("candidate metadata object"),
    );
    notification
}

async fn record_durable(
    adapter: &JsonlStorageAdapter,
    info: &Info,
    timeline: &mut Timeline,
    kind: TimelineEventKind,
) -> chat_state::TimelineEvent {
    let event = timeline
        .record(kind)
        .expect("valid benchmark Timeline event");
    adapter
        .append_timeline_event_durable(info, &event)
        .await
        .expect("durably append benchmark Timeline event");
    event
}

async fn setup_request(
    adapter: &JsonlStorageAdapter,
    info: &Info,
    timeline: &mut Timeline,
    round: usize,
) -> (chat_state::TurnId, chat_state::StepId, String) {
    let turn = chat_state::TurnId(u64::try_from(round + 1).expect("round fits u64"));
    let step = chat_state::StepId { turn, index: 0 };
    let request_id = format!("normal-response-commit-{round:04}");
    let prompt = format!("benchmark prompt {round:04}");
    let input_message_count = timeline.surface_len();
    record_durable(
        adapter,
        info,
        timeline,
        TimelineEventKind::Turn(TurnEvent::Started {
            id: turn,
            prompt_id: format!("benchmark-prompt-{round:04}"),
            input_ids: Vec::new(),
            identity: TurnIdentity {
                origin: "user".into(),
                turn_kind: "internal".into(),
                goal_id: None,
                goal_definition_revision: None,
                stage_id: None,
            },
            model_id: "benchmark-model".into(),
            input_message_count,
            prompt_index: round,
            prompt_text: prompt.clone(),
            input_kind: TurnInputKind::Prompt,
            redirect_kind: None,
        }),
    )
    .await;
    timeline
        .append(ConversationItem::user(prompt), MessageCause::User)
        .expect("record benchmark user input");
    let user_event = timeline.events().last().expect("user input event").clone();
    adapter
        .append_timeline_event_durable(info, &user_event)
        .await
        .expect("durably append benchmark user input");
    let input_message_count = timeline.surface_len();
    record_durable(
        adapter,
        info,
        timeline,
        TimelineEventKind::Step(StepEvent::Started { id: step }),
    )
    .await;
    record_durable(
        adapter,
        info,
        timeline,
        TimelineEventKind::Request(RequestEvent::Started {
            id: request_id.clone(),
            turn,
            step,
            model_id: "benchmark-model".into(),
            input_message_count,
            tool_count: 0,
        }),
    )
    .await;
    record_durable(
        adapter,
        info,
        timeline,
        TimelineEventKind::Request(RequestEvent::Completed {
            id: request_id.clone(),
            duration_ms: 1,
            time_to_first_token_ms: None,
            usage: RequestUsage::default(),
            response_message_count: 1,
            attempt: 1,
            provider_terminal: None,
        }),
    )
    .await;
    (turn, step, request_id)
}

async fn end_turn(
    adapter: &JsonlStorageAdapter,
    info: &Info,
    timeline: &mut Timeline,
    turn: chat_state::TurnId,
    step: chat_state::StepId,
) {
    record_durable(
        adapter,
        info,
        timeline,
        TimelineEventKind::Step(StepEvent::Ended {
            id: step,
            outcome: "completed".into(),
            duration_ms: 1,
        }),
    )
    .await;
    record_durable(
        adapter,
        info,
        timeline,
        TimelineEventKind::Turn(TurnEvent::Ended {
            id: turn,
            outcome: "completed".into(),
            duration_ms: 1,
            tool_count: 0,
            terminal: TurnTerminal {
                source: TurnTerminalSource::Host,
                stop_reason: "end_turn".into(),
                completion_kind: "completed".into(),
            },
            cancellation_category: None,
            details: None,
        }),
    )
    .await;
}

async fn measure_response(
    adapter: &JsonlStorageAdapter,
    info: &Info,
    timeline: &mut Timeline,
    round: usize,
    body_bytes: usize,
    probes: &ProbeCounts,
) -> (PhaseTimes, BarrierCounts) {
    let (turn, step, request_id) = setup_request(adapter, info, timeline, round).await;
    let body_prefix = format!("response-{round:04}:");
    assert!(body_prefix.len() < body_bytes);
    let body = format!(
        "{}{}",
        body_prefix,
        "x".repeat(body_bytes - body_prefix.len())
    );
    let identity = ResponseAdmissionIdentity {
        request_id: request_id.clone(),
        attempt: 1,
    };

    probes.take(); // Setup barriers are outside this response window.
    let total_start = Instant::now();
    let phase_start = Instant::now();
    let anchor = candidate_anchor(info, &request_id, &format!("benchmark-event-{round:04}"));
    adapter
        .append_acp_event_exact(info, &anchor)
        .await
        .expect("durably append exact payload-free candidate anchor");
    let candidate_anchor = phase_start.elapsed();

    let phase_start = Instant::now();
    let event = timeline
        .record(TimelineEventKind::Messages(MessageEvent {
            cause: MessageCause::Assistant,
            items: vec![ConversationItem::assistant(body)],
            surface: chat_state::SurfaceOp::Append,
            response_admission: Some(ResponseAdmission {
                identity: identity.clone(),
                quarantined_tool_exchanges: 0,
            }),
        }))
        .expect("record admitted response");
    adapter
        .append_timeline_event_durable(info, &event)
        .await
        .expect("durably append real admission Timeline event");
    let timeline_admission = phase_start.elapsed();

    let projection_build = Duration::ZERO;
    let projection_commit = Duration::ZERO;
    let total = total_start.elapsed();
    let barriers = probes.take();
    assert_eq!(
        (
            barriers.update_file,
            barriers.update_directory,
            barriers.timeline_file,
            barriers.timeline_directory
        ),
        (0, 0, 2, 2)
    );

    end_turn(adapter, info, timeline, turn, step).await;
    (
        PhaseTimes {
            candidate_anchor,
            timeline_admission,
            projection_build,
            projection_commit,
            total,
        },
        barriers,
    )
}

fn percentile_ms(samples: &[f64], percentile: f64) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() as f64 * percentile).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[index]
}

fn median_ms(samples: &[f64]) -> f64 {
    percentile_ms(samples, 0.5)
}

#[tokio::test]
#[ignore = "manual release-mode response storage commit baseline"]
async fn normal_response_storage_commit_release() {
    let root = tempfile::tempdir().expect("create benchmark root");
    let info = Info {
        id: acp::SessionId::new("normal-response-storage-commit-perf"),
        cwd: "/benchmark/response-commit".into(),
    };
    let probes = Arc::new(ProbeCounts::new());
    let update_probe_counts = Arc::clone(&probes);
    let timeline_probe_counts = Arc::clone(&probes);
    let adapter = JsonlStorageAdapter {
        update_sync_probe: Some(Arc::new(move |point| {
            match point {
                UpdateSyncPoint::File => {
                    update_probe_counts
                        .update_file
                        .fetch_add(1, Ordering::Relaxed);
                }
                UpdateSyncPoint::Directory => {
                    update_probe_counts
                        .update_directory
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
            Ok(())
        })),
        timeline_sync_probe: Some(Arc::new(move |point| {
            match point {
                TimelineSyncPoint::File => {
                    timeline_probe_counts
                        .timeline_file
                        .fetch_add(1, Ordering::Relaxed);
                }
                TimelineSyncPoint::Directory => {
                    timeline_probe_counts
                        .timeline_directory
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
            Ok(())
        })),
        ..JsonlStorageAdapter::with_root(root.path().to_path_buf())
    };
    adapter
        .init_session(&info, default_model_id())
        .await
        .expect("initialize benchmark session");
    let mut timeline = Timeline::default();

    for body_bytes in [4 * 1024, 32 * 1024] {
        let rounds = WARMUP_ROUNDS + MEASURED_ROUNDS;
        let mut measured = Vec::with_capacity(MEASURED_ROUNDS);
        let mut barriers = BarrierCounts::default();
        for round in 0..rounds {
            let (phases, counts) = measure_response(
                &adapter,
                &info,
                &mut timeline,
                round + if body_bytes == 4 * 1024 { 0 } else { rounds },
                body_bytes,
                &probes,
            )
            .await;
            if round >= WARMUP_ROUNDS {
                measured.push(phases);
                barriers.update_file += counts.update_file;
                barriers.update_directory += counts.update_directory;
                barriers.timeline_file += counts.timeline_file;
                barriers.timeline_directory += counts.timeline_directory;
            }
        }

        let totals = measured
            .iter()
            .map(|p| p.total.as_secs_f64() * 1000.0)
            .collect::<Vec<_>>();
        let candidate = measured
            .iter()
            .map(|p| p.candidate_anchor.as_secs_f64() * 1000.0)
            .collect::<Vec<_>>();
        let admission = measured
            .iter()
            .map(|p| p.timeline_admission.as_secs_f64() * 1000.0)
            .collect::<Vec<_>>();
        let projection_build = measured
            .iter()
            .map(|p| p.projection_build.as_secs_f64() * 1000.0)
            .collect::<Vec<_>>();
        let projection_commit = measured
            .iter()
            .map(|p| p.projection_commit.as_secs_f64() * 1000.0)
            .collect::<Vec<_>>();
        let report = serde_json::json!({
            "body_bytes": body_bytes,
            "rounds": MEASURED_ROUNDS,
            "total_ms": {"median": median_ms(&totals), "p95": percentile_ms(&totals, 0.95)},
            "candidate_anchor_exact_append_ms": {"median": median_ms(&candidate), "p95": percentile_ms(&candidate, 0.95)},
            "timeline_admission_durable_append_ms": {"median": median_ms(&admission), "p95": percentile_ms(&admission, 0.95)},
            "projection_build_with_digest_ms": {"median": median_ms(&projection_build), "p95": percentile_ms(&projection_build, 0.95)},
            "projection_commit_ms": {"median": median_ms(&projection_commit), "p95": percentile_ms(&projection_commit, 0.95)},
            "ledger_barriers": {
                "scope": "Timeline and updates ledgers only; excludes summary, artifact, and all other system calls",
                "update_file": barriers.update_file,
                "update_directory": barriers.update_directory,
                "timeline_file": barriers.timeline_file,
                "timeline_directory": barriers.timeline_directory,
            },
        });
        println!("PERF_RESPONSE_COMMIT {report}");
    }

    // Validate all real stored events and the complete fold outside timing.
    let stored = adapter
        .read_timeline_events_sync(&info)
        .expect("read all stored benchmark Timeline events");
    let restored =
        Timeline::from_events(stored).expect("validate complete benchmark Timeline fold");
    assert_eq!(
        restored.admitted_responses().len(),
        2 * (WARMUP_ROUNDS + MEASURED_ROUNDS),
        "all measured and warmup responses are valid admissions"
    );
}
