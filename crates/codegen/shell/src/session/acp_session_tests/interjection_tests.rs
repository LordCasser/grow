//! Mid-turn interjection tests: formatting, broadcast, and the drain path.
use super::support::*;
use super::*;

/// Draining a mid-turn interjection pushes a standalone synthetic user
/// message tagged [`SyntheticReason::Interjection`] — even when the
/// conversation tail is a `ToolResult`. The tool result content must be
/// left untouched (interjections are never appended to tool results).
#[tokio::test]
async fn drain_interjections_pushes_synthetic_user_message_after_tool_result() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;

            const TOOL_RESULT_CONTENT: &str = "file contents: fn main() {}";
            actor
                .chat_state_handle
                .push_tool_result(ConversationItem::tool_result("call-1", TOOL_RESULT_CONTENT));
            actor.pending_interjections.push(PendingInterjection {
                text: "please also add tests".to_string(),
                attachments: vec![],
                auto_promoted: None,
            });

            assert!(
                actor.drain_pending_interjections().await,
                "drain must report that an interjection was consumed"
            );
            assert!(
                actor.pending_interjections.is_empty(),
                "buffer must be empty after drain"
            );

            let conversation = actor.chat_state_handle.get_conversation().await;

            // The tool result is untouched — no interjection text bundled in.
            let tool_result = conversation
                .iter()
                .find_map(|item| match item {
                    ConversationItem::ToolResult(tr) => Some(tr),
                    _ => None,
                })
                .expect("seeded tool result must still be in the conversation");
            assert_eq!(
                tool_result.content.as_ref(),
                TOOL_RESULT_CONTENT,
                "tool result content must not be mutated by an interjection"
            );

            // The interjection landed as a standalone synthetic user message
            // after the tool result.
            let user_item = match conversation.last() {
                Some(ConversationItem::User(u)) => u,
                other => panic!("conversation tail must be a user item, got: {other:?}"),
            };
            assert_eq!(
                user_item.synthetic_reason,
                Some(SyntheticReason::Interjection),
                "interjection must be tagged SyntheticReason::Interjection"
            );
            let text = conversation
                .last()
                .expect("non-empty conversation")
                .text_content();
            assert!(
                text.contains("<user_query>") && text.contains("please also add tests"),
                "interjection must carry the wrapped user text, got: {text}"
            );
        })
        .await;
}

/// Multiple buffered interjections drain as one standalone synthetic user
/// message EACH, in FIFO order (Ctrl+Enter twice = two tagged user rows).
/// None of them may touch the tool result at the conversation tail.
#[tokio::test]
async fn drain_multiple_interjections_pushes_one_user_message_each_in_order() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;

            const TOOL_RESULT_CONTENT: &str = "tool output";
            actor
                .chat_state_handle
                .push_tool_result(ConversationItem::tool_result("call-1", TOOL_RESULT_CONTENT));
            actor.pending_interjections.push(PendingInterjection {
                text: "first steer".to_string(),
                attachments: vec![],
                auto_promoted: None,
            });
            actor.pending_interjections.push(PendingInterjection {
                text: "second steer".to_string(),
                attachments: vec![],
                auto_promoted: None,
            });
            actor.pending_interjections.push(PendingInterjection {
                text: "third steer".to_string(),
                attachments: vec![],
                auto_promoted: None,
            });

            assert!(actor.drain_pending_interjections().await);
            assert!(actor.pending_interjections.is_empty());

            let conversation = actor.chat_state_handle.get_conversation().await;

            let tool_result = conversation
                .iter()
                .find_map(|item| match item {
                    ConversationItem::ToolResult(tr) => Some(tr),
                    _ => None,
                })
                .expect("seeded tool result must still be in the conversation");
            assert_eq!(
                tool_result.content.as_ref(),
                TOOL_RESULT_CONTENT,
                "tool result must not absorb any of the interjections"
            );

            // Exactly one tagged user row per interjection, in send order.
            let ij_texts: Vec<String> = conversation
                .iter()
                .filter_map(|item| match item {
                    ConversationItem::User(u)
                        if u.synthetic_reason == Some(SyntheticReason::Interjection) =>
                    {
                        Some(item.text_content())
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                ij_texts.len(),
                3,
                "each interjection must land as its own user row, got: {ij_texts:?}"
            );
            for (text, expected) in
                ij_texts
                    .iter()
                    .zip(["first steer", "second steer", "third steer"])
            {
                assert!(
                    text.contains(expected) && text.contains("<user_query>"),
                    "interjection rows must keep FIFO order; expected {expected:?} in {text:?}"
                );
            }
        })
        .await;
}

/// Draining with an empty buffer reports false and leaves the conversation
/// untouched. The turn loop's checkpoint gates rely on this.
#[tokio::test]
async fn drain_with_empty_buffer_is_a_noop() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            let before = actor.chat_state_handle.get_conversation().await.len();
            assert!(!actor.drain_pending_interjections().await);
            let after = actor.chat_state_handle.get_conversation().await.len();
            assert_eq!(before, after, "empty drain must not touch the conversation");
        })
        .await;
}

/// A steer is scoped to the exact foreground turn against which it was
/// admitted. Completion/cancellation must discard a late residual instead of
/// letting the next turn consume it.
#[tokio::test]
async fn terminal_boundary_discards_residual_same_turn_interjections() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            actor.pending_interjections.push(PendingInterjection {
                text: "too late for this turn".to_string(),
                attachments: vec![],
                auto_promoted: None,
            });

            actor.discard_residual_interjections_at_turn_end().await;

            assert!(actor.pending_interjections.is_empty());
            assert!(matches!(
                actor.chat_state_handle.get_conversation().await.as_slice(),
                [sampling_types::ConversationItem::System(_)]
            ));
        })
        .await;
}

/// An auto-promoted follow-up (`follow_up_behavior = "steer"`) that missed
/// the turn's final safe-point drain is NOT discarded at the terminal fence:
/// it turns back into the user FIFO front as a fresh turn, preserving its
/// original prompt id / origin / turn kind. Explicit-steer residuals in the
/// same buffer are still discarded.
#[tokio::test]
async fn terminal_boundary_requeues_auto_promoted_follow_ups_and_discards_explicit() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;

            let requeue = |prompt_id: &str| super::AutoPromotedRequeue {
                prompt_id: prompt_id.to_string(),
                origin: crate::session::PromptOrigin::User,
                turn_kind: crate::session::TurnKind::User,
                client_identifier: Some("pager".to_string()),
                screen_mode: None,
                verbatim: false,
                json_schema: None,
            };
            actor.pending_interjections.push(PendingInterjection {
                text: "first follow-up".to_string(),
                attachments: vec![],
                auto_promoted: Some(requeue("follow-up-1")),
            });
            actor.pending_interjections.push(PendingInterjection {
                text: "explicit steer".to_string(),
                attachments: vec![],
                auto_promoted: None,
            });
            actor.pending_interjections.push(PendingInterjection {
                text: "second follow-up".to_string(),
                attachments: vec![],
                auto_promoted: Some(requeue("follow-up-2")),
            });

            actor.discard_residual_interjections_at_turn_end().await;

            assert!(
                actor.pending_interjections.is_empty(),
                "terminal fence must drain the whole buffer"
            );

            let state = actor.state.lock().await;
            assert_eq!(state.pending_inputs.len(), 2, "only auto entries re-queue");
            let front = state
                .pending_inputs
                .front()
                .expect("first requeued entry at the FIFO front");
            assert_eq!(front.prompt_id, "follow-up-1", "FIFO order preserved");
            assert_eq!(front.origin, crate::session::PromptOrigin::User);
            assert_eq!(front.turn_kind, crate::session::TurnKind::User);
            let meta = front.queue_meta.as_ref().expect("user-visible queue row");
            assert_eq!(meta.id, "follow-up-1");
            assert_eq!(meta.owner.as_deref(), Some("pager"));
            assert_eq!(meta.kind, "prompt");
            assert_eq!(meta.text, "first follow-up");
            let back = state.pending_inputs.back().expect("second requeued entry");
            assert_eq!(back.prompt_id, "follow-up-2");
            assert!(
                state.pending_inputs.iter().all(|item| {
                    item.prompt_blocks.iter().all(|b| {
                        matches!(b, acp::ContentBlock::Text(_) | acp::ContentBlock::Image(_))
                    })
                }),
                "rebuilt blocks are plain text/image prompts"
            );
            drop(state);

            assert!(matches!(
                actor.chat_state_handle.get_conversation().await.as_slice(),
                [sampling_types::ConversationItem::System(_)]
            ));
        })
        .await;
}

/// An auto-promoted entry that IS drained at a safe point is consumed as
/// mid-turn steering — its requeue payload is stale and must not resurrect a
/// FIFO turn.
#[tokio::test]
async fn auto_promoted_entry_drained_at_safe_point_is_consumed_not_requeued() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, _gateway_rx) = build_actor().await;
            actor.pending_interjections.push(PendingInterjection {
                text: "steer me".to_string(),
                attachments: vec![],
                auto_promoted: Some(super::AutoPromotedRequeue {
                    prompt_id: "follow-up-1".to_string(),
                    origin: crate::session::PromptOrigin::User,
                    turn_kind: crate::session::TurnKind::User,
                    client_identifier: None,
                    screen_mode: None,
                    verbatim: false,
                    json_schema: None,
                }),
            });

            assert!(actor.drain_pending_interjections().await);

            let state = actor.state.lock().await;
            assert!(
                state.pending_inputs.is_empty(),
                "a drained auto entry is consumed mid-turn; no FIFO turn-back"
            );
            drop(state);
            let conversation = actor.chat_state_handle.get_conversation().await;
            assert!(matches!(
                conversation.as_slice(),
                [
                    sampling_types::ConversationItem::System(_),
                    sampling_types::ConversationItem::User(user),
                ] if user.synthetic_reason == Some(sampling_types::SyntheticReason::Interjection)
            ));
        })
        .await;
}

/// The `queue_input` promotion path steers exactly one interjection into the
/// shared buffer, resolves the submitting client with `RemovedFromQueue`
/// (the same completion a queue-steer emits), broadcasts the interjection
/// with the prompt id, and never touches the FIFO or foreground.
#[tokio::test]
async fn auto_promote_follow_up_uses_the_interjection_path_once() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (actor, mut gateway_rx) = build_actor().await;
            {
                let mut state = actor.state.lock().await;
                state.foreground = ForegroundState::RegularTurn(running_task_stub("turn-1"));
            }

            let (respond_to, rx) = tokio::sync::oneshot::channel();
            let blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(
                "please pivot".to_string(),
            ))];
            actor.auto_promote_follow_up(
                blocks,
                "follow-up-1",
                crate::session::PromptOrigin::User,
                crate::session::TurnKind::User,
                Some("pager".to_string()),
                None,
                false,
                None,
                respond_to,
            );

            // The submitting client's QueuePrompt RPC resolves as removed —
            // never as a failed turn.
            let result = rx.await.expect("client RPC must resolve");
            assert!(
                matches!(
                    result,
                    Ok(crate::session::commands::PromptTurnOk {
                        completion_kind:
                            crate::session::commands::PromptCompletionKind::RemovedFromQueue,
                        ..
                    })
                ),
                "auto-promotion resolves the client with RemovedFromQueue, got {result:?}"
            );

            // Exactly one entry, carrying the requeue payload.
            let entries = actor.pending_interjections.snapshot();
            assert_eq!(entries.len(), 1, "exactly one interjection");
            assert_eq!(entries[0].text, "please pivot");
            let auto = entries[0]
                .auto_promoted
                .as_ref()
                .expect("auto-promoted entry carries its requeue payload");
            assert_eq!(auto.prompt_id, "follow-up-1");
            assert_eq!(auto.origin, crate::session::PromptOrigin::User);
            assert_eq!(auto.turn_kind, crate::session::TurnKind::User);
            assert_eq!(auto.client_identifier.as_deref(), Some("pager"));

            // FIFO and foreground untouched.
            let state = actor.state.lock().await;
            assert!(state.pending_inputs.is_empty());
            assert!(state.foreground.regular().is_some());
            drop(state);

            // One interjection broadcast carrying the prompt id for pager
            // dedup/adoption.
            let mut broadcasts = Vec::new();
            while let Ok(msg) = gateway_rx.try_recv() {
                if let acp_transport::AcpClientMessage::ExtNotification(args) = msg
                    && args.request.method.as_ref() == "grow/session/interjection"
                {
                    broadcasts.push(
                        serde_json::from_str::<serde_json::Value>(args.request.params.get()).ok(),
                    );
                }
            }
            assert_eq!(broadcasts.len(), 1, "exactly one interjection broadcast");
            assert_eq!(
                broadcasts[0]
                    .as_ref()
                    .and_then(|v| v.get("interjectionId"))
                    .and_then(|v| v.as_str()),
                Some("follow-up-1")
            );
        })
        .await;
}

mod interjection_format_tests {
    use super::format_interjection;

    #[test]
    fn interjection_wraps_text_in_user_query() {
        let wrapped = format_interjection("please also add tests".to_string());
        assert!(
            wrapped.contains("<user_query>\nplease also add tests\n</user_query>"),
            "interjection should wrap the user's message in <user_query> tags, got: {wrapped}"
        );
    }

    /// The interjection is a real user message: no deferral instruction
    /// telling the model to finish its current task first (the model weighs
    /// the steering itself, like common mid-turn injection semantics). The
    /// wrapped query must be the final content of the message.
    #[test]
    fn interjection_has_no_deferral_instruction() {
        let wrapped = format_interjection("please also add tests".to_string());
        assert!(
            !wrapped.contains("After completing your current task"),
            "interjection must not defer the user's message, got: {wrapped}"
        );
        assert!(
            wrapped.trim_end().ends_with("</user_query>"),
            "nothing may follow the wrapped user query, got: {wrapped}"
        );
    }
}

mod interjection_broadcast_tests {
    use super::support::create_test_actor;
    use super::*;

    /// Multi-client fix: a mid-turn interjection must be broadcast to every
    /// attached client (not just the originator) so all panes viewing the same
    /// session render it. This locks the wire contract the pager's
    /// `handle_interjection` depends on: method `grow/session/interjection`
    /// carrying `sessionId` + `text`.
    #[tokio::test]
    async fn broadcast_interjection_emits_sessionid_and_text() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let (gateway_tx, mut gateway_rx) =
                    tokio::sync::mpsc::unbounded_channel::<acp_transport::AcpClientMessage>();
                let (persistence_tx, _prx) =
                    tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
                let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;

                actor.broadcast_interjection("please also add tests", Some("ij-1"));

                let mut payload = None;
                while let Ok(msg) = gateway_rx.try_recv() {
                    if let acp_transport::AcpClientMessage::ExtNotification(args) = msg
                        && args.request.method.as_ref() == "grow/session/interjection"
                    {
                        payload =
                            serde_json::from_str::<serde_json::Value>(args.request.params.get())
                                .ok();
                    }
                }
                let payload = payload.expect("an grow/session/interjection broadcast");
                assert_eq!(
                    payload.get("sessionId").and_then(|v| v.as_str()),
                    Some("test-actor"),
                    "broadcast must carry the session id"
                );
                assert_eq!(
                    payload.get("text").and_then(|v| v.as_str()),
                    Some("please also add tests"),
                    "broadcast must carry the interjection text verbatim"
                );
                assert_eq!(
                    payload.get("interjectionId").and_then(|v| v.as_str()),
                    Some("ij-1"),
                    "broadcast must echo the interjection id for originator dedup"
                );
            })
            .await;
    }
}
