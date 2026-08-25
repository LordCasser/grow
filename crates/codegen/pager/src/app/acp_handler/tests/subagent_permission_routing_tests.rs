#![cfg_attr(rustfmt, rustfmt::skip)]
//! Child-session (subagent) permission routing on the pager side.
//!
//! Task C contract tests for suspicious points 2 & 3 of the "subagent bash
//! permission approved but tool does not execute" investigation:
//!
//! 1. A `request_permission` carrying the **child** session id is routed to
//!    the owning primary task's interaction layer. This makes it visible even
//!    when the child is running in the background; the request retains the
//!    child session id for provenance and response routing.
//! 2. A request whose session id is neither a root agent nor a registered
//!    child view is cancelled (never left dangling in a queue nobody can
//!    answer).
//!
//! The registration dependency exercised here is the same one the leader
//! relays on (`child_sessions` → `SubagentSpawned` → `subagent_views`), so
//! this also pins the ordering contract: the spawn notification must be
//! processed before the child's first permission request, otherwise the
//! request is cancelled (pager) or dropped (leader).

use super::*;

/// A child-session permission request must penetrate the primary task and the
/// ordinary approval path must resolve its response channel even while a
/// child transcript is attached.
#[test]
fn child_permission_queues_on_primary_and_approval_resolves() {
    let mut app = make_app_with_agent("sess-parent");
    // Register the child view exactly as the SubagentSpawned notification
    // does (leader forwards it to the pager before any child activity).
    let _ = handle(
        make_ext_session_notification(
            "sess-parent",
            test_subagent_spawned("sess-parent", "child-1"),
        ),
        &mut app,
    );
    assert!(
        app.agents
            .get(&AgentId(0))
            .unwrap()
            .subagent_views
            .contains_key("child-1"),
        "SubagentSpawned must register the child view"
    );
    app.agents.get_mut(&AgentId(0)).unwrap().active_subagent = Some("child-1".into());

    let (msg, mut rx) = make_permission_message("child-1");
    let affected = handle(msg, &mut app);
    assert!(
        affected,
        "a child permission for the active parent must request a redraw"
    );

    let parent = app.agents.get(&AgentId(0)).unwrap();
    assert_eq!(
        parent.permission_queue.len(),
        1,
        "the child request must queue on the primary interaction layer"
    );
    let queued = parent.permission_queue.front().unwrap();
    assert_eq!(
        queued.request.request.session_id.0.as_ref(),
        "child-1",
        "the queued request must be the child's (routing must not swap requests)"
    );
    assert!(
        rx.try_recv().is_err(),
        "the request must stay pending until the user answers"
    );

    // Standard approval path (same dispatch the Enter key on the modal uses).
    let _ = crate::app::root::dispatch::dispatch(
        crate::app::actions::Action::PermissionSelect(acp::PermissionOptionId::new(
            std::sync::Arc::from("allow-once"),
        )),
        &mut app,
    );
    match rx.try_recv() {
        Ok(Ok(resp)) => assert!(
            matches!(
                resp.outcome,
                acp::RequestPermissionOutcome::Selected(ref selected)
                    if selected.option_id.0.as_ref() == "allow-once"
            ),
            "approval must carry the selected option, got {:?}",
            resp.outcome
        ),
        other => panic!(
            "approving the child permission must resolve its response channel, got {other:?}"
        ),
    }
    assert!(
        app.agents[&AgentId(0)].permission_queue.is_empty(),
        "approval must pop the queue"
    );
}

/// A request whose session id is not a root agent and not a registered child
/// view must be cancelled — the pager can never answer it, and leaving it
/// pending would block the subagent forever (the "unknown session_id;
/// cancelling" contract).
#[test]
fn unregistered_child_permission_is_cancelled() {
    let mut app = make_app_with_agent("sess-parent");
    let (msg, mut rx) = make_permission_message("ghost-child");
    let _ = handle(msg, &mut app);
    match rx.try_recv() {
        Ok(Ok(resp)) => assert!(
            matches!(resp.outcome, acp::RequestPermissionOutcome::Cancelled),
            "an unregistered-session permission must be cancelled, got {:?}",
            resp.outcome
        ),
        other => panic!(
            "an unregistered-session permission must be answered with Cancelled, got {other:?}"
        ),
    }
    assert!(
        app.agents.get(&AgentId(0)).unwrap().permission_queue.is_empty(),
        "nothing may be queued for an unknown session"
    );
}
