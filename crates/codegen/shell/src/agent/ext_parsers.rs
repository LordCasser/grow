//! Wire-shape parsers for ext-notification params handled by `MvpAgent`.
//!
//! Pure parsing only (params JSON → `SessionCommand`); session lookup and
//! command dispatch stay in `mvp_agent::ext_notification`.

use crate::session::SessionCommand;

pub(super) fn is_queue_notification_method(method: &str) -> bool {
    matches!(
        method,
        "grow/queue/reorder" | "grow/queue/clear" | "grow/queue/interject"
    )
}

/// Parse retained queue operation notifications into their corresponding
/// [`SessionCommand`].
/// `owner` scopes clear and describes the client that initiated an interject.
/// Returns `None` for unrecognized methods or malformed interject requests.
pub(super) fn parse_queue_notification_command(
    method: &str,
    params: &serde_json::Value,
    owner: Option<String>,
) -> Option<SessionCommand> {
    match method {
        "grow/queue/reorder" => {
            let ordered_ids = params
                .get("orderedIds")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Some(SessionCommand::ReorderQueue { ordered_ids })
        }
        "grow/queue/clear" => Some(SessionCommand::ClearQueue { owner }),
        "grow/queue/interject" => {
            let id = params.get("id").and_then(|v| v.as_str())?.to_string();
            let expected_turn_id = params
                .get("expectedTurnId")
                .and_then(|v| v.as_str())?
                .to_string();
            // The client supplies the version it last saw; the handler acts
            // only on an exact match (stale = benign no-op + rebroadcast).
            let expected_version = params
                .get("expectedVersion")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            // Optional client-edited replacement text (atomic edit+interject).
            // Blank overrides are dropped (degrade to the stored queue text) —
            // never interject an empty prompt on a malformed client param.
            let new_text = params
                .get("newText")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string);
            Some(SessionCommand::SteerQueuedPrompt {
                id,
                expected_turn_id,
                expected_version,
                owner,
                new_text,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only reorder, clear, and interject remain on the notification path;
    /// item edit controls use the dedicated request method.
    #[test]
    fn parse_retained_queue_notifications_and_reject_legacy_controls() {
        for method in [
            "grow/queue/reorder",
            "grow/queue/clear",
            "grow/queue/interject",
        ] {
            assert!(is_queue_notification_method(method));
        }
        for method in [
            "grow/queue/remove",
            "grow/queue/edit",
            "grow/queue/hold_edit",
            "grow/queue/release_edit",
            "grow/queue/unknown",
        ] {
            assert!(!is_queue_notification_method(method));
            assert!(
                parse_queue_notification_command(method, &serde_json::json!({}), None).is_none()
            );
        }

        // reorder: orderedIds array.
        let p = serde_json::json!({ "sessionId": "s1", "orderedIds": ["a", "b", "c"] });
        match parse_queue_notification_command("grow/queue/reorder", &p, None) {
            Some(SessionCommand::ReorderQueue { ordered_ids }) => {
                assert_eq!(ordered_ids, vec!["a", "b", "c"]);
            }
            _ => panic!("expected ReorderQueue"),
        }

        // clear: owner-scoped.
        match parse_queue_notification_command(
            "grow/queue/clear",
            &serde_json::json!({ "sessionId": "s1" }),
            Some("grow-tui".into()),
        ) {
            Some(SessionCommand::ClearQueue { owner }) => {
                assert_eq!(owner.as_deref(), Some("grow-tui"));
            }
            _ => panic!("expected ClearQueue"),
        }

        // Steer atomically moves one queued row into the active turn. The
        // expected turn id prevents a late UI action from steering a newer
        // foreground turn.
        let p = serde_json::json!({
            "sessionId": "s1", "id": "p10", "expectedTurnId": "turn-1", "expectedVersion": 2
        });
        match parse_queue_notification_command("grow/queue/interject", &p, Some("grow-tui".into()))
        {
            Some(SessionCommand::SteerQueuedPrompt {
                id,
                expected_turn_id,
                expected_version,
                owner,
                new_text,
            }) => {
                assert_eq!(id, "p10");
                assert_eq!(expected_turn_id, "turn-1");
                assert_eq!(expected_version, 2);
                assert_eq!(owner.as_deref(), Some("grow-tui"));
                assert_eq!(new_text, None, "newText absent → None");
            }
            _ => panic!("expected SteerQueuedPrompt"),
        }

        // Steer with newText (client-edited row) carries the override.
        let p = serde_json::json!({
            "sessionId": "s1", "id": "p10", "expectedTurnId": "turn-1", "expectedVersion": 2, "newText": "edited"
        });
        match parse_queue_notification_command("grow/queue/interject", &p, None) {
            Some(SessionCommand::SteerQueuedPrompt { new_text, .. }) => {
                assert_eq!(new_text.as_deref(), Some("edited"));
            }
            _ => panic!("expected SteerQueuedPrompt"),
        }

        // Blank newText is dropped → degrades to the stored queue text.
        let p = serde_json::json!({
            "sessionId": "s1", "id": "p10", "expectedTurnId": "turn-1", "expectedVersion": 2, "newText": "   "
        });
        match parse_queue_notification_command("grow/queue/interject", &p, None) {
            Some(SessionCommand::SteerQueuedPrompt { new_text, .. }) => {
                assert_eq!(new_text, None, "blank override must be dropped");
            }
            _ => panic!("expected SteerQueuedPrompt"),
        }

        // Steer without expectedVersion defaults to 0.
        match parse_queue_notification_command(
            "grow/queue/interject",
            &serde_json::json!({ "sessionId": "s1", "id": "p11", "expectedTurnId": "turn-1" }),
            None,
        ) {
            Some(SessionCommand::SteerQueuedPrompt {
                expected_version, ..
            }) => assert_eq!(expected_version, 0),
            _ => panic!("expected SteerQueuedPrompt"),
        }

        // Steer without id or without the expected foreground owner → None.
        assert!(
            parse_queue_notification_command(
                "grow/queue/interject",
                &serde_json::json!({ "expectedTurnId": "turn-1" }),
                None
            )
            .is_none()
        );
        assert!(
            parse_queue_notification_command(
                "grow/queue/interject",
                &serde_json::json!({ "id": "p11" }),
                None
            )
            .is_none()
        );
    }
}
