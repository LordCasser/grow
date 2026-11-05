use super::{
    DeliveryReview, DependencyKind, LocalPlan, PlanLink, PlanPriority, PlanStep, PlanStepStatus,
    PlanUpdate, PlanUpdateError, PlanUpdateMode, PlanWait, ReplanChange, ReplanError,
    ReplanOutcome, ReplanProgress, ReviewDecision, WaitCondition, WaitDependency,
};

fn linked_plan() -> LocalPlan {
    let mut plan = LocalPlan::new("parent", "work");
    plan.update_steps(
        PlanUpdateMode::Merge,
        vec![PlanStep {
            id: "s1".into(),
            content: "first objective".into(),
            priority: PlanPriority::Normal,
            status: PlanStepStatus::Pending,
            requirements: None,
            removed: false,
        }],
    )
    .unwrap();
    add_link(&mut plan, "l1", "s1", "child");
    plan
}

fn add_link(plan: &mut LocalPlan, id: &str, step_id: &str, child: &str) {
    let previous = plan.clone();
    plan.links.push(PlanLink {
        id: id.into(),
        parent_step_id: step_id.into(),
        child_session_id: child.into(),
        terminal: false,
        required: true,
        pending_source_revision: None,
        review_required_revision: plan.revision,
        result_id: None,
        dispatch_failure: None,
    });
    plan.validate_transition_from(&previous).unwrap();
}

fn apply(plan: &mut LocalPlan, step: PlanStep) -> PlanUpdate {
    let previous = plan.clone();
    let update = plan
        .update_steps(PlanUpdateMode::Merge, vec![step])
        .unwrap();
    plan.validate_transition_from(&previous).unwrap();
    update
}

fn edit(plan: &mut LocalPlan, content: &str, status: PlanStepStatus) -> PlanUpdate {
    let mut step = plan.steps[0].clone();
    step.content = content.into();
    step.status = status;
    apply(plan, step)
}

fn advance(plan: &mut LocalPlan, id: &str, progress: ReplanProgress) {
    let previous = plan.clone();
    plan.advance_replan(id, progress).unwrap();
    plan.validate_transition_from(&previous).unwrap();
}

fn record<'a>(plan: &'a LocalPlan, id: &str) -> &'a ReplanChange {
    plan.replans.iter().find(|record| record.id == id).unwrap()
}

fn link<'a>(plan: &'a LocalPlan, id: &str) -> &'a PlanLink {
    plan.links.iter().find(|link| link.id == id).unwrap()
}

fn deliver(parent: &mut LocalPlan, child: &mut LocalPlan, id: &str) {
    if record(parent, id).progress == ReplanProgress::Pending {
        advance(parent, id, ReplanProgress::Dispatching);
    }
    let mut incoming = record(parent, id).clone();
    incoming.progress = ReplanProgress::Received;
    let previous = child.clone();
    assert!(child.receive_replan(incoming.clone()).unwrap());
    child.validate_transition_from(&previous).unwrap();
    let received = child.clone();
    assert!(!child.receive_replan(incoming).unwrap());
    assert_eq!(*child, received);
    advance(parent, id, ReplanProgress::Received);
}

fn consume(parent: &mut LocalPlan, child: &mut LocalPlan, id: &str) {
    advance(child, id, ReplanProgress::Consumed);
    advance(parent, id, ReplanProgress::Consumed);
}

fn handle(parent: &mut LocalPlan, child: &mut LocalPlan, id: &str, outcome: ReplanOutcome) {
    let progress = ReplanProgress::Handled { outcome };
    advance(child, id, progress.clone());
    advance(parent, id, progress);
}

fn cancel(plan: &mut LocalPlan) -> ReplanChange {
    let content = plan.steps[0].content.clone();
    edit(plan, &content, PlanStepStatus::Cancelled);
    plan.replans
        .iter()
        .find(|record| record.link_id == "l1" && record.is_stop())
        .unwrap()
        .clone()
}

#[test]
fn stop_classification_is_derived_from_cancelled_or_removed_payload() {
    let mut parent = linked_plan();
    let change = cancel(&mut parent);
    for status in [
        PlanStepStatus::Pending,
        PlanStepStatus::InProgress,
        PlanStepStatus::Completed,
        PlanStepStatus::Cancelled,
    ] {
        for removed in [false, true] {
            let mut candidate = change.clone();
            candidate.status = status;
            candidate.removed = removed;
            assert_eq!(
                candidate.is_stop(),
                removed || status == PlanStepStatus::Cancelled
            );
        }
    }
    assert!(parent.link_stop_requested("l1"));
    assert!(!parent.link_stop_requested("missing"));
    assert_eq!(
        serde_json::to_value(ReplanOutcome::Stopping).unwrap(),
        "stopping"
    );
    assert_eq!(
        serde_json::from_str::<ReplanOutcome>("\"stopping\"").unwrap(),
        ReplanOutcome::Stopping
    );
}

#[test]
fn ordinary_pending_coalesces_into_first_stop_which_survives_reopening_and_handling() {
    let mut parent = linked_plan();
    edit(&mut parent, "second objective", PlanStepStatus::Pending);
    let ordinary_id = parent.replans[0].id.clone();
    edit(&mut parent, "third objective", PlanStepStatus::InProgress);
    assert_eq!(parent.replans.len(), 1);
    assert_ne!(parent.replans[0].id, ordinary_id);
    let replaced_id = parent.replans[0].id.clone();
    let stop = cancel(&mut parent);
    assert_eq!(parent.replans, vec![stop.clone()]);
    assert!(!parent.replans.iter().any(|record| record.id == replaced_id));

    edit(&mut parent, "reopened objective", PlanStepStatus::Pending);
    assert_eq!(record(&parent, &stop.id), &stop);
    assert!(matches!(
        &parent.replans[1].progress,
        ReplanProgress::Unapplied { reason } if reason.contains("requested to stop")
    ));
    assert_eq!(
        link(&parent, "l1").pending_source_revision,
        Some(stop.source_revision)
    );
    assert!(parent.revision > stop.source_revision);

    let mut child = LocalPlan::new("child", "child-work");
    deliver(&mut parent, &mut child, &stop.id);
    consume(&mut parent, &mut child, &stop.id);
    handle(&mut parent, &mut child, &stop.id, ReplanOutcome::Stopping);
    let handled = record(&parent, &stop.id).clone();
    handle(&mut parent, &mut child, &stop.id, ReplanOutcome::Stopping);
    assert_eq!(record(&parent, &stop.id), &handled);
    assert_eq!(parent.links[0].pending_source_revision, None);
    assert_eq!(child.revision, 0);
    assert!(parent.revision > child.revision);
    assert!(!parent.links[0].terminal);
    assert_eq!(parent.links[0].result_id, None);
    assert_eq!(parent.unresolved_required_deliveries().len(), 1);

    edit(&mut parent, "another objective", PlanStepStatus::InProgress);
    assert_eq!(record(&parent, &stop.id), &handled);
    assert_eq!(parent.links[0].pending_source_revision, None);
    assert!(parent.link_stop_requested("l1"));
    assert!(matches!(
        parent.replans.last().unwrap().progress,
        ReplanProgress::Unapplied { .. }
    ));
}

#[test]
fn later_intent_and_duplicate_cancel_preserve_stop_at_every_delivery_stage() {
    for stage in [
        ReplanProgress::Pending,
        ReplanProgress::Dispatching,
        ReplanProgress::Received,
        ReplanProgress::Consumed,
        ReplanProgress::Handled {
            outcome: ReplanOutcome::Stopping,
        },
    ] {
        let mut parent = linked_plan();
        let stop = cancel(&mut parent);
        if stage != ReplanProgress::Pending {
            advance(&mut parent, &stop.id, ReplanProgress::Dispatching);
        }
        if matches!(
            stage,
            ReplanProgress::Received | ReplanProgress::Consumed | ReplanProgress::Handled { .. }
        ) {
            advance(&mut parent, &stop.id, ReplanProgress::Received);
        }
        if matches!(
            stage,
            ReplanProgress::Consumed | ReplanProgress::Handled { .. }
        ) {
            advance(&mut parent, &stop.id, ReplanProgress::Consumed);
        }
        if matches!(stage, ReplanProgress::Handled { .. }) {
            advance(&mut parent, &stop.id, stage.clone());
        }
        let original = record(&parent, &stop.id).clone();
        let expected_pending = parent.links[0].pending_source_revision;
        let quiet = parent.clone();
        let identical = parent.steps[0].clone();
        assert!(apply(&mut parent, identical).changes.is_empty());
        assert_eq!(parent, quiet);

        let mut new_intent = parent.steps[0].clone();
        new_intent.content = "different work".into();
        new_intent.requirements = Some(vec!["new acceptance".into()]);
        new_intent.priority = PlanPriority::High;
        new_intent.status = PlanStepStatus::Pending;
        apply(&mut parent, new_intent);
        edit(&mut parent, "different work", PlanStepStatus::Cancelled);
        edit(
            &mut parent,
            "cancelled work edited again",
            PlanStepStatus::Cancelled,
        );
        let mut removed = parent.steps[0].clone();
        removed.removed = true;
        apply(&mut parent, removed);
        assert_eq!(record(&parent, &stop.id), &original, "stage {stage:?}");
        assert_eq!(parent.links[0].pending_source_revision, expected_pending);
        assert!(parent.replans[1..].iter().all(|record| matches!(
            &record.progress,
            ReplanProgress::Unapplied { reason } if reason.contains("requested to stop")
        )));
    }
}

#[test]
fn removal_and_replace_omission_keep_tombstone_stop_and_reject_id_reuse() {
    for replace in [false, true] {
        let mut parent = linked_plan();
        edit(
            &mut parent,
            "edited before removal",
            PlanStepStatus::InProgress,
        );
        let previous = parent.clone();
        if replace {
            parent
                .update_steps(PlanUpdateMode::Replace, Vec::new())
                .unwrap();
        } else {
            let mut removed = parent.steps[0].clone();
            removed.removed = true;
            parent
                .update_steps(PlanUpdateMode::Merge, vec![removed])
                .unwrap();
        }
        parent.validate_transition_from(&previous).unwrap();
        assert_eq!(parent.replans.len(), 1);
        let stop = parent.replans[0].clone();
        assert!(stop.is_stop());
        assert!(stop.removed);
        assert!(parent.steps[0].removed);
        assert_eq!(parent.links[0].parent_step_id, "s1");
        let tombstoned = parent.clone();
        let mut reused = parent.steps[0].clone();
        reused.removed = false;
        reused.content = "reused identity".into();
        assert_eq!(
            parent.update_steps(PlanUpdateMode::Merge, vec![reused]),
            Err(PlanUpdateError::TombstonedIdReuse("s1".into()))
        );
        assert_eq!(parent, tombstoned);
        assert!(
            parent
                .update_steps(PlanUpdateMode::Replace, Vec::new())
                .unwrap()
                .changes
                .is_empty()
        );
        assert_eq!(parent, tombstoned);
        let mut child = LocalPlan::new("child", "child-work");
        deliver(&mut parent, &mut child, &stop.id);
        consume(&mut parent, &mut child, &stop.id);
        handle(&mut parent, &mut child, &stop.id, ReplanOutcome::Stopping);
        assert!(parent.link_stop_requested("l1"));
        assert!(!parent.links[0].terminal);
        assert_eq!(parent.unresolved_required_deliveries().len(), 1);
    }
}

#[test]
fn in_flight_ordinary_ack_cannot_clear_a_later_pending_stop() {
    for stage in [
        ReplanProgress::Dispatching,
        ReplanProgress::Received,
        ReplanProgress::Consumed,
    ] {
        let mut parent = linked_plan();
        let mut child = LocalPlan::new("child", "child-work");
        edit(
            &mut parent,
            "old objective update",
            PlanStepStatus::InProgress,
        );
        let ordinary_id = parent.replans[0].id.clone();
        advance(&mut parent, &ordinary_id, ReplanProgress::Dispatching);
        if stage != ReplanProgress::Dispatching {
            deliver(&mut parent, &mut child, &ordinary_id);
        }
        if stage == ReplanProgress::Consumed {
            consume(&mut parent, &mut child, &ordinary_id);
        }
        let immutable_ordinary = record(&parent, &ordinary_id).clone();
        let stop = cancel(&mut parent);
        edit(
            &mut parent,
            "later reopened intent",
            PlanStepStatus::Pending,
        );
        assert_eq!(record(&parent, &ordinary_id), &immutable_ordinary);
        assert_eq!(
            parent.links[0].pending_source_revision,
            Some(stop.source_revision)
        );
        if stage == ReplanProgress::Dispatching {
            deliver(&mut parent, &mut child, &ordinary_id);
        }
        if stage != ReplanProgress::Consumed {
            consume(&mut parent, &mut child, &ordinary_id);
        }
        for _ in 0..2 {
            handle(
                &mut parent,
                &mut child,
                &ordinary_id,
                ReplanOutcome::NoChange {
                    reason: "already covered before the stop".into(),
                },
            );
            assert_eq!(
                parent.links[0].pending_source_revision,
                Some(stop.source_revision)
            );
            assert_eq!(record(&parent, &stop.id), &stop);
        }
        deliver(&mut parent, &mut child, &stop.id);
        consume(&mut parent, &mut child, &stop.id);
        handle(&mut parent, &mut child, &stop.id, ReplanOutcome::Stopping);
        assert_eq!(parent.links[0].pending_source_revision, None);
    }
}

#[test]
fn late_receipt_of_an_older_immutable_batch_preserves_the_received_stop() {
    let mut parent = linked_plan();
    edit(
        &mut parent,
        "ordinary in transit",
        PlanStepStatus::InProgress,
    );
    let old_id = parent.replans[0].id.clone();
    advance(&mut parent, &old_id, ReplanProgress::Dispatching);
    let stop = cancel(&mut parent);
    let mut child = LocalPlan::new("child", "child-work");
    deliver(&mut parent, &mut child, &stop.id);
    let received_stop = record(&child, &stop.id).clone();
    deliver(&mut parent, &mut child, &old_id);
    consume(&mut parent, &mut child, &old_id);
    handle(
        &mut parent,
        &mut child,
        &old_id,
        ReplanOutcome::NoChange {
            reason: "the newer stop supersedes this objective".into(),
        },
    );
    assert_eq!(record(&child, &stop.id), &received_stop);
    assert_eq!(
        parent.links[0].pending_source_revision,
        Some(stop.source_revision)
    );
    let mut altered = received_stop.clone();
    altered.content = "rewritten stop".into();
    let before = child.clone();
    assert_eq!(
        child.receive_replan(altered),
        Err(ReplanError::IdentityPayloadConflict(stop.id))
    );
    assert_eq!(child, before);
}

#[test]
fn stops_are_per_execution_and_a_later_link_can_take_the_reopened_step() {
    let mut parent = linked_plan();
    add_link(&mut parent, "l2", "s1", "second-child");
    let mut other_step = parent.steps[0].clone();
    other_step.id = "s2".into();
    other_step.content = "unrelated objective".into();
    apply(&mut parent, other_step);
    add_link(&mut parent, "unrelated", "s2", "other-child");
    let first_stop = cancel(&mut parent);
    let second_stop = parent
        .replans
        .iter()
        .find(|record| record.link_id == "l2")
        .unwrap()
        .clone();
    assert!(second_stop.is_stop());
    edit(
        &mut parent,
        "new execution can take this",
        PlanStepStatus::Pending,
    );
    add_link(&mut parent, "l3", "s1", "new-child");
    assert!(!parent.link_stop_requested("l3"));
    edit(&mut parent, "new requirements", PlanStepStatus::InProgress);
    assert_eq!(record(&parent, &first_stop.id), &first_stop);
    assert_eq!(record(&parent, &second_stop.id), &second_stop);
    let new_change = parent
        .replans
        .iter()
        .find(|record| record.link_id == "l3")
        .unwrap()
        .clone();
    assert!(!new_change.is_stop());
    assert_eq!(new_change.progress, ReplanProgress::Pending);
    assert_eq!(
        link(&parent, "l3").pending_source_revision,
        Some(new_change.source_revision)
    );
    assert_eq!(link(&parent, "unrelated").pending_source_revision, None);
    assert!(
        !parent
            .replans
            .iter()
            .any(|record| record.link_id == "unrelated")
    );
    let mut child = LocalPlan::new("child", "child-work");
    deliver(&mut parent, &mut child, &first_stop.id);
    consume(&mut parent, &mut child, &first_stop.id);
    handle(
        &mut parent,
        &mut child,
        &first_stop.id,
        ReplanOutcome::Stopping,
    );
    assert_eq!(link(&parent, "l1").pending_source_revision, None);
    assert_eq!(
        link(&parent, "l2").pending_source_revision,
        Some(second_stop.source_revision)
    );
    assert_eq!(
        link(&parent, "l3").pending_source_revision,
        Some(new_change.source_revision)
    );
    assert_eq!(record(&parent, &second_stop.id), &second_stop);
    assert_eq!(record(&parent, &new_change.id), &new_change);
}

#[test]
fn terminal_and_undispatched_disposition_preserve_stop_and_the_true_unapplied_reason() {
    for terminal in [false, true] {
        for dispose_before_stop in [false, true] {
            let mut parent = linked_plan();
            if !dispose_before_stop {
                cancel(&mut parent);
            }
            let before = parent.clone();
            if terminal {
                parent.links[0].result_id =
                    Some(r#"{"timeline_id":"child","first_seq":7,"last_seq":7}"#.into());
                parent
                    .mark_link_terminal("l1", "canonical child result arrived")
                    .unwrap();
            } else {
                assert!(
                    parent
                        .mark_link_undispatched("l1", "startup rejected before admission")
                        .unwrap()
                );
            }
            parent.validate_transition_from(&before).unwrap();
            if dispose_before_stop {
                cancel(&mut parent);
            }
            let original = parent.replans[0].clone();
            let expected_reason = if terminal {
                if dispose_before_stop {
                    "child is already terminal"
                } else {
                    "canonical child result arrived"
                }
            } else {
                "startup rejected before admission"
            };
            assert_eq!(
                original.progress,
                ReplanProgress::Unapplied {
                    reason: expected_reason.into()
                }
            );
            edit(
                &mut parent,
                "reopen after disposition",
                PlanStepStatus::Pending,
            );
            let mut removed = parent.steps[0].clone();
            removed.removed = true;
            apply(&mut parent, removed);
            assert_eq!(record(&parent, &original.id), &original);
            let subsequent_reason = if terminal {
                "child is already terminal"
            } else {
                "startup rejected before admission"
            };
            assert!(parent.replans[1..].iter().all(|record| record.progress
                == ReplanProgress::Unapplied {
                    reason: subsequent_reason.into()
                }));
            assert_eq!(parent.links[0].terminal, terminal);
            assert_eq!(parent.links[0].pending_source_revision, None);
            assert_eq!(parent.links[0].result_id.is_some(), terminal);
            assert!(parent.link_stop_requested("l1"));
            assert_eq!(parent.unresolved_required_deliveries().len(), 1);
        }
    }
}

#[test]
fn direct_transition_rejects_erasing_replacing_or_rewriting_pending_stop() {
    let mut previous = linked_plan();
    let stop = cancel(&mut previous);
    let mut later = previous.clone();
    edit(&mut later, "new target", PlanStepStatus::Pending);
    let mut tampered = later.clone();
    tampered.replans.remove(0);
    tampered.replans[0].progress = ReplanProgress::Pending;
    tampered.links[0].pending_source_revision = Some(tampered.revision);
    tampered.validate().unwrap();
    assert!(tampered.validate_transition_from(&previous).is_err());

    let mut replacement_stop = tampered.clone();
    replacement_stop.replans[0].status = PlanStepStatus::Cancelled;
    replacement_stop.validate().unwrap();
    assert!(
        replacement_stop
            .validate_transition_from(&previous)
            .is_err()
    );

    let mut appended = later;
    appended.replans[1].progress = ReplanProgress::Pending;
    appended.links[0].pending_source_revision = Some(appended.revision);
    appended.validate().unwrap();
    assert!(appended.validate_transition_from(&previous).is_err());

    let mut rewritten = previous.clone();
    rewritten.replans[0].content = "stop payload changed".into();
    rewritten.validate().unwrap();
    assert!(rewritten.validate_transition_from(&previous).is_err());
    assert_eq!(record(&previous, &stop.id), &stop);
}

#[test]
fn direct_transition_rejects_new_objective_after_stop_added_in_the_same_commit() {
    let previous = linked_plan();
    let mut current = previous.clone();
    cancel(&mut current);
    edit(
        &mut current,
        "reopened in same commit",
        PlanStepStatus::Pending,
    );
    current.validate_transition_from(&previous).unwrap();
    current.replans[1].progress = ReplanProgress::Pending;
    current.links[0].pending_source_revision = Some(current.revision);
    current.validate().unwrap();
    assert!(current.validate_transition_from(&previous).is_err());
}

#[test]
fn stop_rejects_non_stopping_ack_in_api_and_direct_transition_without_mutation() {
    for outcome in [
        ReplanOutcome::NoChange {
            reason: "continue the old objective".into(),
        },
        ReplanOutcome::Updated { plan_revision: 20 },
        ReplanOutcome::NotApplied {
            reason: "declining termination".into(),
        },
    ] {
        let mut parent = linked_plan();
        let stop = cancel(&mut parent);
        advance(&mut parent, &stop.id, ReplanProgress::Received);
        advance(&mut parent, &stop.id, ReplanProgress::Consumed);
        let before = parent.clone();
        let next = ReplanProgress::Handled { outcome };
        assert_eq!(
            parent.advance_replan(&stop.id, next.clone()),
            Err(ReplanError::InvalidProgress)
        );
        assert_eq!(parent, before);
        let mut crafted = before.clone();
        crafted.replans[0].progress = next;
        crafted.links[0].pending_source_revision = None;
        crafted
            .validate()
            .expect("historical non-Stopping stop facts remain loadable");
        assert!(crafted.validate_transition_from(&before).is_err());
        assert_eq!(parent, before);
    }
}

#[test]
fn stopping_requires_a_consumed_stop_and_cannot_handle_an_ordinary_change() {
    for is_stop in [false, true] {
        let mut parent = linked_plan();
        let id = if is_stop {
            cancel(&mut parent).id
        } else {
            edit(&mut parent, "ordinary update", PlanStepStatus::Pending);
            parent.replans[0].id.clone()
        };
        let handled = ReplanProgress::Handled {
            outcome: ReplanOutcome::Stopping,
        };
        for stage in [
            ReplanProgress::Pending,
            ReplanProgress::Dispatching,
            ReplanProgress::Received,
        ] {
            advance(&mut parent, &id, stage);
            let before = parent.clone();
            assert_eq!(
                parent.advance_replan(&id, handled.clone()),
                Err(ReplanError::InvalidProgress)
            );
            assert_eq!(parent, before);
        }
        advance(&mut parent, &id, ReplanProgress::Consumed);
        if is_stop {
            advance(&mut parent, &id, handled.clone());
            advance(&mut parent, &id, handled);
        } else {
            let before = parent.clone();
            assert_eq!(
                parent.advance_replan(&id, handled.clone()),
                Err(ReplanError::InvalidProgress)
            );
            assert_eq!(parent, before);
            let mut crafted = before.clone();
            crafted.replans[0].progress = handled;
            crafted.links[0].pending_source_revision = None;
            assert!(crafted.validate().is_err());
            assert!(crafted.validate_transition_from(&before).is_err());
        }
    }
}

#[test]
fn historical_handled_non_stopping_stops_load_and_remain_exact_facts() {
    for outcome in [
        ReplanOutcome::NoChange {
            reason: "historically accepted unchanged".into(),
        },
        ReplanOutcome::Updated { plan_revision: 700 },
    ] {
        let mut historical = linked_plan();
        let stop = cancel(&mut historical);
        historical.replans[0].progress = ReplanProgress::Handled {
            outcome: outcome.clone(),
        };
        historical.links[0].pending_source_revision = None;
        let serialized = serde_json::to_vec(&historical).unwrap();
        let mut loaded: LocalPlan = serde_json::from_slice(&serialized).unwrap();
        loaded.validate().unwrap();
        loaded.validate_transition_from(&historical).unwrap();
        advance(&mut loaded, &stop.id, ReplanProgress::Handled { outcome });
        assert_eq!(loaded, historical);
        let handled = loaded.replans[0].clone();
        edit(
            &mut loaded,
            "reopened after historical handling",
            PlanStepStatus::Pending,
        );
        assert_eq!(record(&loaded, &stop.id), &handled);
        assert!(matches!(
            loaded.replans[1].progress,
            ReplanProgress::Unapplied { .. }
        ));
        assert_eq!(loaded.links[0].pending_source_revision, None);

        let mut child = LocalPlan::new("child", "child-work");
        child.revision = 700;
        child.replans.push(handled.clone());
        child.validate().unwrap();
        let previous = child.clone();
        child.validate_transition_from(&previous).unwrap();
        advance(&mut child, &stop.id, handled.progress);
        assert_eq!(child, previous);
    }
}

#[test]
fn direct_transition_cannot_append_a_new_historical_style_stop_ack() {
    let previous = linked_plan();
    let mut crafted = previous.clone();
    cancel(&mut crafted);
    crafted.replans[0].progress = ReplanProgress::Handled {
        outcome: ReplanOutcome::NoChange {
            reason: "not a historical fact".into(),
        },
    };
    crafted.links[0].pending_source_revision = None;
    crafted.validate().unwrap();
    assert!(crafted.validate_transition_from(&previous).is_err());
}

#[test]
fn completion_noop_and_combined_edit_keep_existing_trigger_semantics() {
    let mut ordinary = linked_plan();
    let complete = edit(&mut ordinary, "first objective", PlanStepStatus::Completed);
    assert!(complete.source_changes.is_empty());
    assert!(ordinary.replans.is_empty());
    let combined = edit(
        &mut ordinary,
        "completed with new content",
        PlanStepStatus::Completed,
    );
    assert_eq!(combined.source_changes.len(), 1);
    assert_eq!(ordinary.replans.len(), 1);
    assert!(!ordinary.replans[0].is_stop());
    assert_eq!(ordinary.replans[0].progress, ReplanProgress::Pending);
    let unchanged = ordinary.clone();
    assert!(
        edit(
            &mut ordinary,
            "completed with new content",
            PlanStepStatus::Completed
        )
        .changes
        .is_empty()
    );
    assert_eq!(ordinary, unchanged);

    let mut stopped = linked_plan();
    let stop = cancel(&mut stopped);
    assert!(
        edit(&mut stopped, "first objective", PlanStepStatus::Completed)
            .source_changes
            .is_empty()
    );
    assert_eq!(stopped.replans, vec![stop.clone()]);
    let combined = edit(
        &mut stopped,
        "completed but changed intent",
        PlanStepStatus::Completed,
    );
    assert_eq!(combined.source_changes.len(), 1);
    assert_eq!(record(&stopped, &stop.id), &stop);
    assert_eq!(
        stopped.links[0].pending_source_revision,
        Some(stop.source_revision)
    );
    assert!(matches!(
        stopped.replans[1].progress,
        ReplanProgress::Unapplied { .. }
    ));
}

#[test]
fn stopping_does_not_dispose_execution_wait_review_or_canonical_result_obligations() {
    let mut parent = linked_plan();
    parent.waits.push(PlanWait {
        id: "wait-child".into(),
        step_ids: vec!["s1".into()],
        dependencies: vec![WaitDependency {
            kind: DependencyKind::Task,
            id: "l1".into(),
        }],
        condition: WaitCondition::All,
    });
    let stop = cancel(&mut parent);
    let mut child = LocalPlan::new("child", "child-work");
    deliver(&mut parent, &mut child, &stop.id);
    consume(&mut parent, &mut child, &stop.id);
    handle(&mut parent, &mut child, &stop.id, ReplanOutcome::Stopping);
    let next_scope = LocalPlan::new("parent", "next-work");
    assert!(next_scope.validate_transition_from(&parent).is_err());
    assert_eq!(parent.waits.len(), 1);
    assert_eq!(parent.unresolved_required_deliveries().len(), 1);
    assert!(!parent.links[0].terminal);
    assert_eq!(parent.links[0].result_id, None);
    parent.reviews.push(DeliveryReview {
        link_id: "l1".into(),
        result_id: None,
        reviewed_source_revision: parent.revision,
        decision: ReviewDecision::Released,
        reason: Some("release delivery requirement while settling".into()),
        verification_refs: Vec::new(),
    });
    parent.validate().unwrap();
    assert!(parent.unresolved_required_deliveries().is_empty());
    let before = parent.clone();
    assert!(next_scope.validate_transition_from(&before).is_err());
    let result_id = r#"{"timeline_id":"child","first_seq":42,"last_seq":42}"#;
    parent.links[0].result_id = Some(result_id.into());
    parent
        .mark_link_terminal("l1", "canonical result finally arrived")
        .unwrap();
    parent.validate_transition_from(&before).unwrap();
    assert_eq!(parent.replans, before.replans);
    assert_eq!(parent.reviews, before.reviews);
    assert_eq!(parent.links[0].result_id.as_deref(), Some(result_id));
    assert!(next_scope.validate_transition_from(&parent).is_err());
    parent.waits.clear();
    parent.validate().unwrap();
    next_scope.validate_transition_from(&parent).unwrap();
}
