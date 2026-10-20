use super::*;
#[tokio::test]
async fn audit_post_240_coordinator_keeps_freeze_owner_until_release() {
    let harness = harness(false, std::time::Duration::from_secs(60));
    let sender = harness.backend.sender();
    for (operation, accepted) in [
        ("first-freeze", true),
        ("new-retry", false),
        ("first-freeze", true),
    ] {
        let (respond_to, response) = tokio::sync::oneshot::channel();
        sender
            .send(SubagentEvent::FreezeAdmission {
                root_session_id: "parent".into(),
                operation_id: operation.into(),
                respond_to,
            })
            .unwrap();
        let result = response.await.unwrap();
        assert_eq!(result.is_ok(), accepted, "{operation}: {result:?}");
        if !accepted {
            assert_eq!(result.unwrap_err(), "conflicting suspension owner");
        }
    }
    harness.actor.abort();
    eprintln!("AUDIT real coordinator: new owner rejected, original owner retry accepted");
}
