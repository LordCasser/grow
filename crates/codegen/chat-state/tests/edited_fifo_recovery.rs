use chat_state::{
    HookAggregateDecision, HookCause, HookEvent, HookEventType, HookFailurePolicy,
    HookGateDecision, HookGateKind, HookHandlerKind, HookHandlerPlan, HookHandlerPlanAction,
    HookHandlerProvenance, HookRunControl, HookRunOutcome, InputAdmissionDecision,
    InputBlockReason, InputDismissReason, InputEvent, InputIntent, InputPayloadRef, InputRoute,
    InputState, PendingAllowedInput, Timeline, TimelineEvent, TimelineEventKind, TurnEvent, TurnId,
    TurnIdentity, TurnInputKind,
};
use sampling_types::ConversationItem;

// Match timeline.rs's submit_human_input fixture: these bytes stand for an opaque
// payload reference, not a Shell InputPayload or a persisted payload artifact.
fn payload_ref(input_id: &str) -> InputPayloadRef {
    InputPayloadRef {
        blake3: blake3::hash(input_id.as_bytes()).to_hex().to_string(),
        bytes: input_id.len() as u64,
    }
}

fn submit(timeline: &mut Timeline, input_id: &str) {
    timeline
        .record(TimelineEventKind::Input(InputEvent::Submitted {
            input_id: input_id.into(),
            intent: InputIntent::Prompt,
            payload_ref: payload_ref(input_id),
        }))
        .expect("fixture submission must validate");
}

fn trigger_prompt_hook(
    timeline: &mut Timeline,
    input_id: &str,
    handlers: Vec<HookHandlerPlan>,
) -> String {
    let occurrence_id = format!("hook-{input_id}");
    timeline
        .record(TimelineEventKind::Hook(HookEvent::Triggered {
            occurrence_id: occurrence_id.clone(),
            event: HookEventType::UserPromptSubmit,
            gate: HookGateKind::Prompt,
            cause: HookCause::Input {
                input_id: input_id.into(),
            },
            config_generation: 1,
            handlers,
        }))
        .expect("fixture prompt hook must validate");
    occurrence_id
}

fn complete_allow_hook(timeline: &mut Timeline, input_id: &str) {
    let occurrence_id = trigger_prompt_hook(timeline, input_id, Vec::new());
    timeline
        .record(TimelineEventKind::Hook(HookEvent::Completed {
            occurrence_id,
            decision: HookAggregateDecision::Prompt {
                decision: HookGateDecision::Allow,
            },
        }))
        .expect("empty prompt hook must complete as Allow");
}

fn allow_replacement(input_id: &str, supersedes: &[&str]) -> TimelineEventKind {
    TimelineEventKind::Input(InputEvent::AdmissionResolved {
        input_id: input_id.into(),
        decision: InputAdmissionDecision::Allow,
        route: Some(InputRoute::Fifo),
        supersedes: supersedes.iter().map(|id| (*id).into()).collect(),
    })
}

fn admit(timeline: &mut Timeline, input_id: &str, supersedes: &[&str]) {
    submit(timeline, input_id);
    complete_allow_hook(timeline, input_id);
    timeline
        .record(allow_replacement(input_id, supersedes))
        .expect("fixture replacement admission must validate");
}

// Each call rebuilds the complete accepted event prefix through the public
// serializer and replay validator. It models a recovery cut, without file I/O
// or any claim about a storage barrier. Expected IDs are written by each test;
// this helper only supplies their known payload references and FIFO route.
fn rebuild_at_cut(timeline: &Timeline, expected_ids: &[&str]) -> Timeline {
    let bytes = serde_json::to_vec(timeline.events()).expect("serialize accepted event prefix");
    let events: Vec<TimelineEvent> =
        serde_json::from_slice(&bytes).expect("decode complete accepted event prefix");
    let rebuilt = Timeline::from_events(events).expect("accepted event prefix must replay");
    let expected: Vec<_> = expected_ids
        .iter()
        .map(|input_id| PendingAllowedInput {
            input_id: (*input_id).into(),
            intent: InputIntent::Prompt,
            payload_ref: payload_ref(input_id),
            route: InputRoute::Fifo,
        })
        .collect();

    assert_eq!(rebuilt.pending_allowed_inputs(), expected, "replayed query");
    assert_eq!(timeline.pending_allowed_inputs(), expected, "live query");
    assert_eq!(rebuilt.next_seq(), timeline.next_seq());
    assert_eq!(
        serde_json::to_vec(rebuilt.events()).expect("serialize replayed events"),
        bytes,
        "replay must not rewrite event evidence"
    );
    for event in timeline.events() {
        if let TimelineEventKind::Input(InputEvent::Submitted { input_id, .. }) = &event.kind {
            assert_eq!(
                rebuilt.input_state(input_id),
                timeline.input_state(input_id),
                "input lifecycle must round-trip for {input_id}"
            );
        }
    }
    rebuilt
}

fn assert_superseded(timeline: &Timeline, input_ids: &[&str]) {
    for input_id in input_ids {
        assert_eq!(
            timeline
                .input_state(input_id)
                .expect("submitted input")
                .state,
            InputState::Dismissed {
                reason: InputDismissReason::RouteSuperseded,
            },
            "old input {input_id} must remain ineligible"
        );
    }
}

#[test]
fn successive_edits_keep_a_b_c_slots_at_every_successful_admission_cut() {
    let mut timeline = Timeline::default();
    for (input_id, expected) in [
        ("input-a", &["input-a"][..]),
        ("input-b", &["input-a", "input-b"][..]),
        ("input-c", &["input-a", "input-b", "input-c"][..]),
    ] {
        admit(&mut timeline, input_id, &[]);
        timeline = rebuild_at_cut(&timeline, expected);
    }

    for (input_id, supersedes, expected, dismissed) in [
        (
            "input-a-edit-1",
            "input-a",
            ["input-a-edit-1", "input-b", "input-c"],
            &["input-a"][..],
        ),
        (
            "input-a-edit-2",
            "input-a-edit-1",
            ["input-a-edit-2", "input-b", "input-c"],
            &["input-a", "input-a-edit-1"][..],
        ),
        (
            "input-b-edit-1",
            "input-b",
            ["input-a-edit-2", "input-b-edit-1", "input-c"],
            &["input-a", "input-a-edit-1", "input-b"][..],
        ),
    ] {
        admit(&mut timeline, input_id, &[supersedes]);
        assert_superseded(&timeline, dismissed);
        timeline = rebuild_at_cut(&timeline, &expected);
        assert_superseded(&timeline, dismissed);
    }

    admit(&mut timeline, "input-d", &[]);
    rebuild_at_cut(
        &timeline,
        &["input-a-edit-2", "input-b-edit-1", "input-c", "input-d"],
    );
}

#[test]
fn combined_live_group_inherits_the_earliest_replaced_slot_and_keeps_it_on_reedit() {
    let mut timeline = Timeline::default();
    for input_id in ["input-a-head", "input-b", "input-a-tail", "input-c"] {
        admit(&mut timeline, input_id, &[]);
    }
    timeline = rebuild_at_cut(
        &timeline,
        &["input-a-head", "input-b", "input-a-tail", "input-c"],
    );

    // A live group can contain several pending inputs. Reverse their IDs here
    // so the replacement must inherit the earliest slot, not supersedes[0].
    admit(
        &mut timeline,
        "input-a-combined",
        &["input-a-tail", "input-a-head"],
    );
    timeline = rebuild_at_cut(&timeline, &["input-a-combined", "input-b", "input-c"]);
    assert_superseded(&timeline, &["input-a-head", "input-a-tail"]);

    admit(&mut timeline, "input-a-reedit", &["input-a-combined"]);
    timeline = rebuild_at_cut(&timeline, &["input-a-reedit", "input-b", "input-c"]);
    assert_superseded(
        &timeline,
        &["input-a-head", "input-a-tail", "input-a-combined"],
    );
}

#[test]
fn unaccepted_and_hook_blocked_edits_leave_the_old_allowed_row_pending() {
    let mut timeline = Timeline::default();
    for input_id in ["input-a", "input-b", "input-c"] {
        admit(&mut timeline, input_id, &[]);
    }

    submit(&mut timeline, "input-a-unaccepted");
    timeline = rebuild_at_cut(&timeline, &["input-a", "input-b", "input-c"]);
    assert_eq!(
        timeline.input_state("input-a-unaccepted").unwrap().state,
        InputState::Submitted
    );

    complete_allow_hook(&mut timeline, "input-a-unaccepted");
    let prepared = timeline
        .prepare(allow_replacement("input-a-unaccepted", &["input-a"]))
        .expect("replacement can be prepared without accepting it");
    assert_eq!(prepared.seq, timeline.next_seq());
    timeline = rebuild_at_cut(&timeline, &["input-a", "input-b", "input-c"]);
    assert_eq!(prepared.seq, timeline.next_seq());
    assert_eq!(
        timeline.input_state("input-a-unaccepted").unwrap().state,
        InputState::Submitted
    );

    submit(&mut timeline, "input-a-blocked");
    let run_id = "run-input-a-blocked";
    let occurrence_id = trigger_prompt_hook(
        &mut timeline,
        "input-a-blocked",
        vec![HookHandlerPlan {
            index: 0,
            run_id: run_id.into(),
            name: "block-edit".into(),
            provenance: HookHandlerProvenance::UserConfig,
            kind: HookHandlerKind::Command,
            failure_policy: HookFailurePolicy::Allow,
            action: HookHandlerPlanAction::Execute,
        }],
    );
    timeline
        .record(TimelineEventKind::Hook(HookEvent::RunStarted {
            occurrence_id: occurrence_id.clone(),
            run_id: run_id.into(),
            handler_index: 0,
        }))
        .expect("blocking handler must start");
    let reason = "edited input rejected";
    timeline
        .record(TimelineEventKind::Hook(HookEvent::RunFinished {
            occurrence_id: occurrence_id.clone(),
            run_id: run_id.into(),
            handler_index: 0,
            elapsed_ms: 1,
            outcome: HookRunOutcome::Blocked,
            control: HookRunControl::Block {
                reason: reason.into(),
            },
        }))
        .expect("typed blocking result must validate");
    timeline
        .record(TimelineEventKind::Hook(HookEvent::Completed {
            occurrence_id,
            decision: HookAggregateDecision::Prompt {
                decision: HookGateDecision::Block {
                    reason: reason.into(),
                },
            },
        }))
        .expect("actual handler result must aggregate to Block");
    // Block cannot carry supersedes under the existing validator. A rejected
    // attempted edit therefore records no replacement of the original input.
    timeline
        .record(TimelineEventKind::Input(InputEvent::AdmissionResolved {
            input_id: "input-a-blocked".into(),
            decision: InputAdmissionDecision::Block {
                reason: InputBlockReason::Hook {
                    reason: reason.into(),
                },
            },
            route: None,
            supersedes: Vec::new(),
        }))
        .expect("blocked admission must validate without a replacement");
    timeline = rebuild_at_cut(&timeline, &["input-a", "input-b", "input-c"]);
    assert_eq!(
        timeline.input_state("input-a").unwrap().state,
        InputState::Allowed {
            route: InputRoute::Fifo,
        }
    );
    assert_eq!(
        timeline.input_state("input-a-blocked").unwrap().state,
        InputState::Blocked {
            reason: InputBlockReason::Hook {
                reason: reason.into(),
            },
        }
    );
}

#[test]
fn consumed_handled_dismissed_and_reserved_replacements_stay_out_of_pending_after_replay() {
    for transition in ["consumed", "handled", "dismissed", "reserved"] {
        let mut timeline = Timeline::default();
        for input_id in ["input-a", "input-b", "input-c"] {
            admit(&mut timeline, input_id, &[]);
        }
        admit(&mut timeline, "input-a-edit", &["input-a"]);
        let expected_state = if transition == "dismissed" {
            timeline
                .record(TimelineEventKind::Input(InputEvent::Dismissed {
                    input_ids: vec!["input-a-edit".into()],
                    reason: InputDismissReason::UserRemoved,
                }))
                .expect("pending replacement can be dismissed");
            InputState::Dismissed {
                reason: InputDismissReason::UserRemoved,
            }
        } else {
            let turn = TurnId(1);
            timeline
                .record(TimelineEventKind::Turn(TurnEvent::Started {
                    id: turn,
                    prompt_id: "fixture-prompt".into(),
                    input_ids: vec!["input-a-edit".into()],
                    identity: TurnIdentity {
                        origin: "user".into(),
                        turn_kind: "user".into(),
                        goal_id: None,
                        goal_definition_revision: None,
                        stage_id: None,
                    },
                    model_id: "model".into(),
                    input_message_count: 0,
                    prompt_index: 0,
                    prompt_text: "edited A".into(),
                    input_kind: TurnInputKind::Prompt,
                    redirect_kind: None,
                }))
                .expect("user turn must reserve its allowed FIFO input");
            // Reservation is already enough to exclude the input. Canonical
            // lifecycle remains Allowed until Consumed publishes the user.
            timeline = rebuild_at_cut(&timeline, &["input-b", "input-c"]);
            if transition == "consumed" {
                timeline
                    .record(TimelineEventKind::Input(InputEvent::Consumed {
                        input_ids: vec!["input-a-edit".into()],
                        turn,
                        item: ConversationItem::user("edited A"),
                        display_echo: None,
                        skill_loads: Vec::new(),
                    }))
                    .expect("reserved FIFO input can be consumed by its turn");
                InputState::Consumed {
                    route: InputRoute::Fifo,
                }
            } else if transition == "handled" {
                timeline
                    .record(TimelineEventKind::Input(InputEvent::Handled {
                        input_ids: vec!["input-a-edit".into()],
                        turn,
                    }))
                    .expect("reserved FIFO input can be handled by its turn");
                InputState::Handled {
                    route: InputRoute::Fifo,
                }
            } else {
                InputState::Allowed {
                    route: InputRoute::Fifo,
                }
            }
        };
        timeline = rebuild_at_cut(&timeline, &["input-b", "input-c"]);
        assert_superseded(&timeline, &["input-a"]);
        assert_eq!(
            timeline.input_state("input-a-edit").unwrap().state,
            expected_state,
            "{transition} replacement must not return to pending"
        );
    }
}
