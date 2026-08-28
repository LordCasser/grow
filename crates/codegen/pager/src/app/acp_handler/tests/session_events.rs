#![cfg_attr(rustfmt, rustfmt::skip)]
    use super::*;

    // ── apply_session_event ────────────────────────────────────────────

    #[test]
    fn command_notices_without_event_ids_do_not_deduplicate_by_correlation() {
        let mut app = make_app_with_agent("s1");
        let update = GrowSessionUpdate::UiNotice(
            shell::extensions::notification::UiNotice {
                correlation_id: "invoke-1".into(),
                category: shell::extensions::notification::UiNoticeCategory::Command,
                subject: Some("/workflow demo".into()),
                description: Some("Run a workflow".into()),
                message: "Workflow launch failed".into(),
                tone: shell::extensions::notification::UiNoticeTone::Error,
                details: Some("Reason: invalid definition. Fix it and retry.".into()),
            },
        );
        assert!(handle(
            make_ext_session_notification("s1", update.clone()),
            &mut app,
        ));
        let before = app.agents[&AgentId(0)].scrollback.len();
        assert!(handle(
            make_ext_session_notification("s1", update),
            &mut app,
        ));
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        assert_eq!(
            agent.scrollback.len(),
            before + 1,
            "correlation groups notices but is not immutable event identity"
        );
        let entry = agent.scrollback.entries_mut().last().expect("command notice");
        match &entry.block {
            RenderBlock::Notice(notice) => {
                assert_eq!(notice.tone, crate::scrollback::blocks::NoticeTone::Error);
                assert_eq!(
                    notice.category,
                    crate::scrollback::blocks::NoticeCategory::Command
                );
                assert_eq!(notice.event_id, None);
            }
            other => panic!("expected command Notice, got {other:?}"),
        }
    }

    #[test]
    fn lifecycle_notice_keeps_its_category_and_recovery_details() {
        let mut app = make_app_with_agent("s1");
        let update = GrowSessionUpdate::UiNotice(
            shell::extensions::notification::UiNotice {
                correlation_id: "goal-stop-1".into(),
                category: shell::extensions::notification::UiNoticeCategory::Lifecycle,
                subject: Some("goal".into()),
                description: None,
                message: "Goal stopped due to turn error".into(),
                tone: shell::extensions::notification::UiNoticeTone::Warning,
                details: Some("Recovery: use /goal restart.".into()),
            },
        );
        assert!(handle(make_ext_session_notification("s1", update), &mut app));
        let entry = app.agents.get_mut(&AgentId(0)).unwrap()
            .scrollback.entries_mut().last().expect("lifecycle notice");
        match &entry.block {
            RenderBlock::Notice(notice) => {
                assert_eq!(notice.category, crate::scrollback::blocks::NoticeCategory::Lifecycle);
                assert_eq!(notice.tone, crate::scrollback::blocks::NoticeTone::Warning);
                assert!(notice.details.as_deref().is_some_and(|details|
                    details.contains("Recovery: use /goal restart.")));
            }
            other => panic!("expected lifecycle Notice, got {other:?}"),
        }
    }

    #[test]
    fn control_projection_keeps_transients_live_and_commits_only_latest_terminal() {
        use shell::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };

        let mut app = make_app_with_agent("s1");
        let update = |revision, phase, target: &str, message: Option<&str>| {
            GrowSessionUpdate::ControlStateUpdate(ControlStateUpdate {
                epoch: "epoch-a".into(),
                domain: ControlDomain::Sampling,
                revision,
                intent: None,
                snapshot: false,
                receipt_only: false,
                phase,
                current: ControlTarget::Sampling {
                    model_id: "provider/old".into(),
                    reasoning_effort: None,
                },
                desired: Some(ControlTarget::Sampling {
                    model_id: target.into(),
                    reasoning_effort: Some("high".into()),
                }),
                message: message.map(str::to_owned),
            })
        };
        assert!(handle(
            make_ext_session_notification(
                "s1",
                GrowSessionUpdate::ControlStateUpdate(ControlStateUpdate {
                    epoch: "epoch-a".into(),
                    domain: ControlDomain::Sampling,
                    revision: 0,
                    intent: None,
                    snapshot: true,
                    receipt_only: false,
                    phase: ControlPhase::Applied,
                    current: ControlTarget::Sampling {
                        model_id: "provider/old".into(),
                        reasoning_effort: None,
                    },
                    desired: None,
                    message: None,
                }),
            ),
            &mut app,
        ));
        assert!(handle(
            make_ext_session_notification(
                "s1",
                update(1, ControlPhase::Pending, "provider/first", None),
            ),
            &mut app,
        ));
        assert!(handle(
            make_ext_session_notification(
                "s1",
                update(2, ControlPhase::Pending, "provider/final", None),
            ),
            &mut app,
        ));
        assert_eq!(
            app.agents[&AgentId(0)].scrollback.len(),
            0,
            "Pending replacement must remain live status only"
        );
        assert_eq!(
            app.agents[&AgentId(0)].session.control_status(100).as_deref(),
            Some("model old→final (high)")
        );

        assert!(handle(
            make_ext_session_notification(
                "s1",
                update(
                    2,
                    ControlPhase::Applied,
                    "provider/final",
                    Some("Sampling switched to provider/final (high)"),
                ),
            ),
            &mut app,
        ));
        let committed = app.agents[&AgentId(0)].scrollback.len();
        assert_eq!(committed, 1);
        assert_eq!(app.agents[&AgentId(0)].session.control_status(100), None);
        assert!(
            !handle(
                make_ext_session_notification(
                    "s1",
                    update(
                        1,
                        ControlPhase::Applied,
                        "provider/first",
                        Some("stale terminal"),
                    ),
                ),
                &mut app,
            ),
            "stale terminal projection must be ignored"
        );
        assert_eq!(app.agents[&AgentId(0)].scrollback.len(), committed);
    }

    #[test]
    fn fresh_session_commits_terminal_feedback_for_every_control_domain() {
        use shell::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };

        let mut app = make_app_with_agent("s1");
        assert!(handle(
            make_ext_session_notification(
                "s1",
                GrowSessionUpdate::ControlStateUpdate(ControlStateUpdate {
                    epoch: "fresh-epoch".into(),
                    domain: ControlDomain::Sampling,
                    revision: 0,
                    intent: None,
                    snapshot: true,
                    receipt_only: false,
                    phase: ControlPhase::Applied,
                    current: ControlTarget::Sampling {
                        model_id: "provider/old".into(),
                        reasoning_effort: None,
                    },
                    desired: None,
                    message: None,
                }),
            ),
            &mut app,
        ));
        let updates = [
            (
                ControlDomain::Sampling,
                ControlTarget::Sampling {
                    model_id: "provider/new".into(),
                    reasoning_effort: Some("high".into()),
                },
                "Sampling switched to provider/new (high)",
            ),
            (
                ControlDomain::Agent,
                ControlTarget::Agent {
                    agent_name: "reviewer".into(),
                },
                "Agent switched to reviewer",
            ),
            (
                ControlDomain::Behavior,
                ControlTarget::Behavior {
                    behavior_id: "goal".into(),
                },
                "Behavior switched to goal",
            ),
        ];

        for (index, (domain, target, message)) in updates.into_iter().enumerate() {
            assert!(handle(
                make_ext_session_notification(
                    "s1",
                    GrowSessionUpdate::ControlStateUpdate(ControlStateUpdate {
                        epoch: "fresh-epoch".into(),
                        domain,
                        revision: 1,
                        intent: None,
                        snapshot: false,
                        receipt_only: false,
                        phase: ControlPhase::Applied,
                        current: target.clone(),
                        desired: Some(target),
                        message: Some(message.into()),
                    }),
                ),
                &mut app,
            ));
            assert_eq!(
                app.agents[&AgentId(0)].scrollback.len(),
                index + 1,
                "each terminal control fact must append one visible Notice"
            );
        }
    }

    #[test]
    fn pre_assignment_snapshot_seeds_epoch_before_new_session_response() {
        use shell::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };

        let mut app = make_app_with_agent("placeholder");
        let agent_id = AgentId(0);
        app.agents.get_mut(&agent_id).unwrap().session.session_id = None;
        let snapshot = GrowSessionUpdate::ControlStateUpdate(ControlStateUpdate {
            epoch: "new-actor".into(),
            domain: ControlDomain::Agent,
            revision: 0,
            intent: None,
            snapshot: true,
            receipt_only: false,
            phase: ControlPhase::Applied,
            current: ControlTarget::Agent {
                agent_name: "builder".into(),
            },
            desired: None,
            message: None,
        });
        assert!(handle(
            make_ext_session_notification("fresh-session", snapshot),
            &mut app,
        ));

        app.agents
            .get_mut(&agent_id)
            .unwrap()
            .bind_session_id(acp::SessionId::new("fresh-session"));
        assert!(handle(
            make_ext_session_notification(
                "fresh-session",
                GrowSessionUpdate::ControlStateUpdate(ControlStateUpdate {
                    epoch: "new-actor".into(),
                    domain: ControlDomain::Agent,
                    revision: 1,
                    intent: None,
                    snapshot: false,
                    receipt_only: false,
                    phase: ControlPhase::Applied,
                    current: ControlTarget::Agent {
                        agent_name: "reviewer".into(),
                    },
                    desired: Some(ControlTarget::Agent {
                        agent_name: "reviewer".into(),
                    }),
                    message: Some("Agent switched to reviewer".into()),
                }),
            ),
            &mut app,
        ));
        assert_eq!(app.agents[&agent_id].scrollback.len(), 1);
    }

    #[test]
    fn recovered_receipt_does_not_obscure_a_newer_live_target() {
        use crate::app::session::PendingSessionControl;
        use shell::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };

        let mut app = make_app_with_agent("s1");
        let old_token = app
            .agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .enqueue_control(PendingSessionControl::Agent {
                agent_name: "old-target".into(),
            })
            .expect("local control")
            .0;
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .rearm_controls_for_reconnect();

        let target = |name: &str| ControlTarget::Agent {
            agent_name: name.into(),
        };
        assert!(handle(
            make_ext_session_notification(
                "s1",
                GrowSessionUpdate::ControlStateUpdate(ControlStateUpdate {
                    epoch: "epoch-new".into(),
                    domain: ControlDomain::Agent,
                    revision: 1,
                    intent: Some(shell::session::ControlIntent {
                        client_id: "other-client".into(),
                        generation: 0,
                        sequence: 1,
                    }),
                    snapshot: true,
                    receipt_only: false,
                    phase: ControlPhase::Pending,
                    current: target("builder"),
                    desired: Some(target("new-target")),
                    message: None,
                }),
            ),
            &mut app,
        ));

        assert!(handle(
            make_ext_session_notification(
                "s1",
                GrowSessionUpdate::ControlStateUpdate(ControlStateUpdate {
                    epoch: "epoch-new".into(),
                    domain: ControlDomain::Agent,
                    revision: 2,
                    intent: Some(old_token.shell_intent()),
                    snapshot: false,
                    receipt_only: true,
                    phase: ControlPhase::Applied,
                    current: target("builder"),
                    desired: Some(target("old-target")),
                    message: Some("Agent switched to old-target".into()),
                }),
            ),
            &mut app,
        ));
        assert_eq!(
            app.agents[&AgentId(0)].session.control_status(100).as_deref(),
            Some("agent builder→new-target"),
            "a historical receipt must not replace the live desired projection"
        );

        assert!(handle(
            make_ext_session_notification(
                "s1",
                GrowSessionUpdate::ControlStateUpdate(ControlStateUpdate {
                    epoch: "epoch-new".into(),
                    domain: ControlDomain::Agent,
                    revision: 1,
                    intent: Some(shell::session::ControlIntent {
                        client_id: "other-client".into(),
                        generation: 0,
                        sequence: 1,
                    }),
                    snapshot: false,
                    receipt_only: false,
                    phase: ControlPhase::Applied,
                    current: target("new-target"),
                    desired: Some(target("new-target")),
                    message: Some("Agent switched to new-target".into()),
                }),
            ),
            &mut app,
        ));
        assert_eq!(app.agents[&AgentId(0)].session.control_status(100), None);
        assert_eq!(
            app.agents[&AgentId(0)].scrollback.len(),
            2,
            "both immutable terminal facts remain visible exactly once"
        );
    }

    #[test]
    fn cross_epoch_control_terminals_replay_once_and_retired_epoch_stays_retired() {
        use shell::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };

        let terminal = |epoch: &str, revision, agent_name: &str| {
            GrowSessionUpdate::ControlStateUpdate(ControlStateUpdate {
                epoch: epoch.into(),
                domain: ControlDomain::Agent,
                revision,
                intent: None,
                snapshot: revision == 7 || revision == 1,
                receipt_only: false,
                phase: ControlPhase::Applied,
                current: ControlTarget::Agent {
                    agent_name: agent_name.into(),
                },
                desired: None,
                message: Some(format!("Agent switched to {agent_name}")),
            })
        };
        let mut app = make_app_with_agent("s1");

        for update in [
            terminal("old", 7, "old-agent"),
            terminal("old", 7, "old-agent"),
            terminal("new", 1, "new-agent"),
            terminal("new", 1, "new-agent"),
        ] {
            handle(make_ext_session_notification("s1", update), &mut app);
        }
        assert_eq!(
            app.agents[&AgentId(0)].scrollback.len(),
            2,
            "each epoch's immutable terminal is visible exactly once"
        );

        handle(
            make_ext_session_notification("s1", terminal("old", 8, "late-old-agent")),
            &mut app,
        );
        assert_eq!(
            app.agents[&AgentId(0)].scrollback.len(),
            2,
            "a retired epoch cannot append a late terminal"
        );
    }

    #[test]
    fn apply_compaction_started_sets_activity() {
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        session.in_flight_prompt = Some(InFlightPrompt {
            text: "hi".into(),
            images: Vec::new(),
            scrollback_entry: EntryId::new(1),
            combined_scrollback_entries: Vec::new(),
            chip_elements: Vec::new(),
        });
        let update = GrowSessionUpdate::AutoCompactStarted {
            tokens_used: 90000,
            context_window: 131072,
            percentage: 85,
            reason: "threshold".into(),
        };
        assert!(apply_session_event(&update, &mut session, &mut scrollback));
        assert!(
            session.in_flight_prompt.is_none(),
            "compaction start implies server activity — cancel must not rewind prompt"
        );
        assert_eq!(
            session.compact_held_prompt.as_ref().map(|p| p.text.as_str()),
            Some("hi"),
            "hold prompt text for re-auth auto-resubmit if compact fails with auth"
        );
    }

    /// Compact failure keeps the held prompt for the terminal response path.
    #[test]
    fn apply_compaction_failed_keeps_held_prompt() {
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        session.compact_held_prompt = Some(InFlightPrompt {
            text: "retry after login".into(),
            images: Vec::new(),
            scrollback_entry: EntryId::new(1),
            combined_scrollback_entries: Vec::new(),
            chip_elements: Vec::new(),
        });
        for error in [
            "the model provider rejected its credentials. Check the provider authentication and retry.",
            "this conversation is too large to compact.",
        ] {
            let update = GrowSessionUpdate::AutoCompactFailed {
                error: error.into(),
            };
        assert!(apply_session_event(&update, &mut session, &mut scrollback));
            assert_eq!(
                session.compact_held_prompt.as_ref().map(|p| p.text.as_str()),
                Some("retry after login"),
            );
        }
    }

    /// `ImageDropped` joins notes with `\n` and pushes a system block.
    /// Pin the `\n` separator so a `notes.join(" ")` regression is caught.
    #[test]
    fn apply_image_dropped_pushes_scrollback_block() {
        use crate::scrollback::block::RenderBlock;
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        let before = scrollback.len();
        let notes = vec![
            "Image 1 was dropped: corrupt.".to_string(),
            "Image 2 was dropped: too small (4×3).".to_string(),
        ];
        let update = GrowSessionUpdate::ImageDropped {
            notes: notes.clone(),
        };
        let changed = apply_session_event(&update, &mut session, &mut scrollback);
        assert!(changed);
        assert_eq!(scrollback.len(), before + 1);
        let entry = scrollback.entries_mut().last().expect("entry pushed");
        match &entry.block {
            RenderBlock::Notice(b) => {
                assert!(b.text.contains(&notes[0]));
                assert!(b.text.contains(&notes[1]));
                assert!(
                    b.text.contains('\n'),
                    "expected \\n separator between dropped notes, got: {:?}",
                    b.text
                );
            }
            other => panic!("expected System block, got {other:?}"),
        }
    }

    /// A successful compression needs no user action: log-only — no toast,
    /// no scrollback block, no redraw. Same live and on session replay.
    #[test]
    fn image_compressed_is_invisible_in_tui() {
        for replay in [false, true] {
            let mut agent = make_agent(Some("s1"));
            agent.session.loading_replay = replay;
            assert!(!apply_image_compressed(
                &mut agent,
                &[compressed_entry(1), compressed_entry(2)],
                "Compressed Image 1: 4.2 MB (3024x1964) \u{2192} 780 KB (1568x1018)",
            ));
            assert!(agent.toast.is_none(), "no toast (replay={replay})");
            assert_eq!(agent.scrollback.len(), 0, "no block (replay={replay})");
        }
    }

    /// The re-encode fallback (empty `images`) means the oversized original
    /// was kept — a persistent warning line, not a transient toast.
    #[test]
    fn image_compressed_fallback_warning_stays_in_scrollback() {
        use crate::scrollback::block::RenderBlock;
        let mut agent = make_agent(Some("s1"));
        let msg = "Image 1 could not be re-encoded under the 1.5 MB limit; the original attachment was kept.";
        assert!(apply_image_compressed(&mut agent, &[], msg));
        assert!(agent.toast.is_none(), "warning must not be transient");
        let entry = agent.scrollback.entries_mut().last().expect("block pushed");
        match &entry.block {
            RenderBlock::Notice(b) => assert_eq!(b.text, msg),
            other => panic!("expected System block, got {other:?}"),
        }
    }

    #[test]
    fn apply_retry_state_retrying_clears_in_flight_prompt() {
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        session.in_flight_prompt = Some(InFlightPrompt {
            text: "retry me".into(),
            images: Vec::new(),
            scrollback_entry: EntryId::new(2),
            combined_scrollback_entries: Vec::new(),
            chip_elements: Vec::new(),
        });
        let retry = RetryState::Retrying {
            attempt: 1,
            max_retries: 3,
            reason: "rate limited".into(),
        };
        apply_retry_state(&retry, &mut session, &mut scrollback);
        assert!(
            session.in_flight_prompt.is_none(),
            "RetryState bypasses session/update in_flight hook"
        );
    }

    #[test]
    fn retry_exhausted_rate_limited_sets_flag() {
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();

        assert!(!session.rate_limited);
        apply_retry_state(
            &RetryState::Exhausted {
                attempts: 3,
                reason: "rate limited".into(),
                is_rate_limited: true,
            },
            &mut session,
            &mut scrollback);
        assert!(
            session.rate_limited,
            "rate_limited flag must be set when is_rate_limited is true"
        );
    }

    #[test]
    fn retry_exhausted_rate_limited_empty_reason_uses_neutral_fallback() {
        use shell::sampling::error::RATE_LIMITED_USER_MESSAGE;

        let empty = RetryState::Exhausted {
            attempts: 3,
            reason: "".into(),
            is_rate_limited: true,
        };

        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        apply_retry_state(&empty, &mut session, &mut scrollback);
        match last_session_event(&scrollback) {
            Some(SessionEvent::RetryFailed { error, .. }) => {
                assert_eq!(error, RATE_LIMITED_USER_MESSAGE);
            }
            other => panic!("expected empty-rate-limit RetryFailed, got {other:?}"),
        }
    }

    /// Production `RetryState::Exhausted.reason` is `SamplingError::Api`'s
    /// Display: `API error (status 429 Too Many Requests): …`.
    #[test]
    fn retry_exhausted_rate_limited_surfaces_server_detail() {
        let body = "The model is currently at capacity due to high demand. Please try again.";
        let reason = format!("API error (status 429 Too Many Requests): {body}");
        let exhausted = RetryState::Exhausted {
            attempts: 3,
            reason: reason.clone(),
            is_rate_limited: true,
        };

        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        apply_retry_state(&exhausted, &mut session, &mut scrollback);
        match last_session_event(&scrollback) {
            Some(SessionEvent::RetryFailed { error, .. }) => {
                assert_eq!(error, body);
                assert!(!error.contains("API error (status"));
            }
            other => panic!("expected detail RetryFailed, got {other:?}"),
        }
    }

    #[test]
    fn retry_exhausted_non_rate_limited_does_not_set_flag() {
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();

        apply_retry_state(
            &RetryState::Exhausted {
                attempts: 3,
                reason: "server error".into(),
                is_rate_limited: false,
            },
            &mut session,
            &mut scrollback);
        assert!(
            !session.rate_limited,
            "rate_limited flag must not be set when is_rate_limited is false"
        );
    }

    /// An untyped provider 401 remains a provider failure.
    #[test]
    fn apply_retry_state_401_message_keeps_provider_error() {
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        apply_retry_state(
            &RetryState::Failed {
                error_type: "api".into(),
                message: "Unauthorized (401) from https://proxy/v1/responses: invalid credentials"
                    .into(),
            },
            &mut session,
            &mut scrollback);
        assert!(matches!(
            last_session_event(&scrollback),
            Some(SessionEvent::RetryFailed { .. })
        ));
    }

    #[test]
    fn apply_retry_state_byok_rejection_clears_in_flight_prompt() {
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        session.in_flight_prompt = Some(InFlightPrompt {
            text: "failed BYOK request".into(),
            images: Vec::new(),
            scrollback_entry: EntryId::new(7),
            combined_scrollback_entries: Vec::new(),
            chip_elements: Vec::new(),
        });
        apply_retry_state(
            &RetryState::Failed {
                error_type: "provider_credentials".into(),
                message: "The configured BYOK credential was rejected; check api_key/env_key"
                    .into(),
            },
            &mut session,
            &mut scrollback,
        );
        assert!(matches!(
            last_session_event(&scrollback),
            Some(SessionEvent::RetryFailed { .. })
        ));
        assert!(
            session.in_flight_prompt.is_none(),
            "a rejected BYOK request must not remain in flight"
        );
    }

    /// Non-auth terminal failures still render the standard RetryFailed.
    #[test]
    fn apply_retry_state_generic_failure_still_shows_retry_failed() {
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        apply_retry_state(
            &RetryState::Failed {
                error_type: "server_error".into(),
                message: "internal server error".into(),
            },
            &mut session,
            &mut scrollback);
        assert!(matches!(
            last_session_event(&scrollback),
            Some(SessionEvent::RetryFailed { .. })
        ));
    }

    /// A context overflow surfaces the actionable `ContextTooLarge` prompt (not the
    /// raw `RetryFailed`); `PromptResponse` then suppresses the redundant `TurnFailed`.
    #[test]
    fn apply_retry_state_context_length_shows_context_too_large() {
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        apply_retry_state(
            &RetryState::Failed {
                error_type: "context_length".into(),
                message: "API error (status 500): the prompt is too long for this model's \
                          context window"
                    .into(),
            },
            &mut session,
            &mut scrollback);
        assert!(
            matches!(
                last_session_event(&scrollback),
                Some(SessionEvent::ContextTooLarge)
            ),
            "context overflow must surface the actionable ContextTooLarge prompt"
        );
    }

    /// When the compaction handler already showed its "too large to compact" message,
    /// the overflow path does NOT stack a second `ContextTooLarge` prompt on top.
    #[test]
    fn apply_retry_state_context_length_does_not_duplicate_compaction_failed() {
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        scrollback.push_block(RenderBlock::session_event(SessionEvent::CompactionFailed {
            error: "this conversation is too large to compact.".into(),
        }));
        apply_retry_state(
            &RetryState::Failed {
                error_type: "context_length".into(),
                message: "the prompt is too long for this model's context window".into(),
            },
            &mut session,
            &mut scrollback);
        assert!(
            matches!(
                last_session_event(&scrollback),
                Some(SessionEvent::CompactionFailed { .. })
            ),
            "must not push a duplicate prompt on top of CompactionFailed"
        );
    }

    #[test]
    fn apply_compaction_completed_defers_message_until_turn_end() {
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        session.set_compaction_activity(Some(TurnActivity::AutoCompacting));
        let update = GrowSessionUpdate::AutoCompactCompleted {
            tokens_before: 858_000,
            tokens_after: 66_000,
            elapsed_ms: Some(500),
            summary_preview: None,
        };
        assert!(apply_session_event(&update, &mut session, &mut scrollback));
        assert_eq!(
            scrollback.len(),
            0,
            "live compaction completion must be deferred, not pushed immediately"
        );

        session.note_context_used(43_000);

        session.finish_turn(&mut scrollback,
        );
        match last_session_event(&scrollback) {
            Some(SessionEvent::CompactionCompleted {
                tokens_before,
                tokens_after,
                ..
            }) => {
                assert_eq!(tokens_before, 858_000);
                assert_eq!(
                    tokens_after, 43_000,
                    "must flush the model-confirmed count, not the 66k estimate"
                );
            }
            other => panic!("expected deferred CompactionCompleted, got {other:?}"),
        }
    }

    #[test]
    fn apply_compaction_completed_falls_back_to_estimate_without_confirmation() {
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        let update = GrowSessionUpdate::AutoCompactCompleted {
            tokens_before: 90_000,
            tokens_after: 20_000,
            elapsed_ms: Some(500),
            summary_preview: None,
        };
        assert!(apply_session_event(&update, &mut session, &mut scrollback));
        session.finish_turn(&mut scrollback,
        );
        match last_session_event(&scrollback) {
            Some(SessionEvent::CompactionCompleted { tokens_after, .. }) => {
                assert_eq!(
                    tokens_after, 20_000,
                    "fallback to estimate when unconfirmed"
                );
            }
            other => panic!("expected fallback CompactionCompleted, got {other:?}"),
        }
    }

    #[test]
    fn apply_compaction_completed_renders_immediately_during_replay() {
        let mut session = make_session(Some("s1"));
        session.loading_replay = true;
        let mut scrollback = ScrollbackState::new();
        let update = GrowSessionUpdate::AutoCompactCompleted {
            tokens_before: 90_000,
            tokens_after: 20_000,
            elapsed_ms: Some(500),
            summary_preview: None,
        };
        assert!(apply_session_event(&update, &mut session, &mut scrollback));
        match last_session_event(&scrollback) {
            Some(SessionEvent::CompactionCompleted { tokens_after, .. }) => {
                assert_eq!(
                    tokens_after, 20_000,
                    "replay renders the recorded count immediately"
                );
            }
            other => panic!("expected immediate CompactionCompleted on replay, got {other:?}"),
        }
    }

    #[test]
    fn deferred_compaction_flushes_confirmed_count_over_estimate_refresh() {
        let mut agent = make_agent(Some("s1"));
        agent
            .session
            .set_compaction_activity(Some(TurnActivity::AutoCompacting));

        let update = GrowSessionUpdate::AutoCompactCompleted {
            tokens_before: 858_000,
            tokens_after: 66_000,
            elapsed_ms: Some(500),
            summary_preview: None,
        };
        assert!(apply_session_event(
            &update,
            &mut agent.session,
            &mut agent.scrollback));

        refresh_context_used(&mut agent, 66_000);
        confirm_context_used(&mut agent, 43_000);

        agent.session.finish_turn(&mut agent.scrollback,
        );
        match last_session_event(&agent.scrollback) {
            Some(SessionEvent::CompactionCompleted {
                tokens_before,
                tokens_after,
                ..
            }) => {
                assert_eq!(tokens_before, 858_000);
                assert_eq!(
                    tokens_after, 43_000,
                    "deferred line must flush the confirmed 43k, not the 66k \
                     estimate refresh that updated the bar first"
                );
            }
            other => panic!("expected deferred CompactionCompleted, got {other:?}"),
        }
    }

    #[test]
    fn apply_unhandled_event_returns_false() {
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();
        let update = GrowSessionUpdate::MemoryFlushStarted;
        assert!(!apply_session_event(&update, &mut session, &mut scrollback));
    }

    // ── handle_child_session_notification ──────────────────────────────

    #[test]
    fn child_compact_completed_updates_subagent_info() {
        let mut agent = make_agent(Some("root-sess"));
        let child_sid = "child-sess-1";
        agent
            .session.subagent_sessions
            .insert(child_sid.into(), make_subagent_info(child_sid));
        let child_view = make_agent(Some(child_sid));
        agent
            .subagent_views
            .insert(child_sid.into(), Box::new(child_view));

        let update = GrowSessionUpdate::AutoCompactCompleted {
            tokens_before: 90000,
            tokens_after: 25000,
            elapsed_ms: Some(300),
            summary_preview: None,
        };
        let changed = handle_child_session_notification(update, child_sid, None, &mut agent);
        assert!(changed);

        let info = agent.session.subagent_sessions.get(child_sid).unwrap();
        assert_eq!(info.tokens_used, Some(25000));
        // 25000 / 131072 * 100 ~= 19
        assert_eq!(info.context_usage_pct, Some(19));

        // The child view's context_state.used (context-bar numerator) must
        // also be reset — see the comment in handle_child_session_notification.
        let child_view = agent.subagent_views.get(child_sid).unwrap();
        assert_eq!(
            child_view.session.context_state.as_ref().map(|c| c.used),
            Some(25000)
        );
    }

    #[test]
    fn child_compact_started_does_not_reset_context_used() {
        // Sibling variants in the same outer arm must not touch the numerator;
        // guards against accidental widening of the AutoCompactCompleted gate.
        let mut agent = make_agent(Some("root-sess"));
        let child_sid = "child-sess-3";
        agent
            .session.subagent_sessions
            .insert(child_sid.into(), make_subagent_info(child_sid));
        let mut child_view = make_agent(Some(child_sid));
        child_view.session.context_state = Some(shell::session::ContextInfo::from_notification(
            90_000, 131_072,
        ));
        agent
            .subagent_views
            .insert(child_sid.into(), Box::new(child_view));

        let update = GrowSessionUpdate::AutoCompactStarted {
            tokens_used: 95_000,
            context_window: 131_072,
            percentage: 72,
            reason: "threshold".into(),
        };
        let _ = handle_child_session_notification(update, child_sid, None, &mut agent);

        let child_view = agent.subagent_views.get(child_sid).unwrap();
        assert_eq!(
            child_view.session.context_state.as_ref().map(|c| c.used),
            Some(90_000)
        );
    }

    #[test]
    fn child_notification_without_view_returns_false() {
        let mut agent = make_agent(Some("root-sess"));
        // No child view registered.
        let update = GrowSessionUpdate::AutoCompactStarted {
            tokens_used: 90000,
            context_window: 131072,
            percentage: 85,
            reason: "threshold".into(),
        };
        let changed =
            handle_child_session_notification(update, "unknown-child", None, &mut agent);
        assert!(!changed);
    }

    #[test]
    fn child_compact_completed_without_view_returns_false() {
        let mut agent = make_agent(Some("root-sess"));
        let child_sid = "child-sess-2";
        // SubagentInfo exists but no child view (race between notification and spawn).
        agent
            .session.subagent_sessions
            .insert(child_sid.into(), make_subagent_info(child_sid));

        let update = GrowSessionUpdate::AutoCompactCompleted {
            tokens_before: 90000,
            tokens_after: 25000,
            elapsed_ms: Some(300),
            summary_preview: None,
        };
        let changed = handle_child_session_notification(update, child_sid, None, &mut agent);
        // No child_view means nothing visible changed — must not trigger redraw.
        assert!(!changed);
        // SubagentInfo should still be updated (data correctness).
        let info = agent.session.subagent_sessions.get(child_sid).unwrap();
        assert_eq!(info.tokens_used, Some(25000));
        assert_eq!(info.context_usage_pct, Some(19));
    }

    #[test]
    fn child_unknown_event_returns_false() {
        let mut agent = make_agent(Some("root-sess"));
        let update = GrowSessionUpdate::MemoryFlushStarted;
        let changed = handle_child_session_notification(update, "child-1", None, &mut agent);
        assert!(!changed);
    }

    #[test]
    fn child_command_notice_preserves_tone_and_durable_event_identity() {
        let mut agent = make_agent(Some("root-sess"));
        let child_sid = "child-command";
        agent
            .subagent_views
            .insert(child_sid.into(), Box::new(make_agent(Some(child_sid))));

        let update = GrowSessionUpdate::UiNotice(
            shell::extensions::notification::UiNotice {
                correlation_id: "invoke-child".into(),
                category: shell::extensions::notification::UiNoticeCategory::Command,
                subject: Some("/workflow child".into()),
                description: Some("Run a child workflow".into()),
                message: "Child workflow failed".into(),
                tone: shell::extensions::notification::UiNoticeTone::Error,
                details: Some("Reason: invalid definition. Fix it and retry.".into()),
            },
        );
        assert!(handle_child_session_notification(
            update,
            child_sid,
            Some("event-child-command".into()),
            &mut agent,
        ));

        let child = agent.subagent_views.get_mut(child_sid).unwrap();
        let entry = child.scrollback.entries_mut().last().expect("command notice");
        match &entry.block {
            RenderBlock::Notice(notice) => {
                assert_eq!(notice.tone, crate::scrollback::blocks::NoticeTone::Error);
                assert_eq!(notice.event_id.as_deref(), Some("event-child-command"));
            }
            other => panic!("expected child command Notice, got {other:?}"),
        }
    }

    #[test]
    fn child_control_terminal_preserves_durable_event_identity() {
        use shell::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };

        let mut agent = make_agent(Some("root-sess"));
        let child_sid = "child-control";
        agent
            .subagent_views
            .insert(child_sid.into(), Box::new(make_agent(Some(child_sid))));
        assert!(handle_child_session_notification(
            GrowSessionUpdate::ControlStateUpdate(ControlStateUpdate {
                epoch: "epoch-a".into(),
                domain: ControlDomain::Agent,
                revision: 0,
                intent: None,
                snapshot: true,
                receipt_only: false,
                phase: ControlPhase::Applied,
                current: ControlTarget::Agent {
                    agent_name: "builder".into(),
                },
                desired: None,
                message: None,
            }),
            child_sid,
            None,
            &mut agent,
        ));
        let update = GrowSessionUpdate::ControlStateUpdate(ControlStateUpdate {
            epoch: "epoch-a".into(),
            domain: ControlDomain::Agent,
            revision: 7,
            intent: None,
            snapshot: false,
            receipt_only: false,
            phase: ControlPhase::Applied,
            current: ControlTarget::Agent {
                agent_name: "reviewer".into(),
            },
            desired: None,
            message: Some("Agent switched to reviewer".into()),
        });

        assert!(handle_child_session_notification(
            update,
            child_sid,
            Some("event-child-control".into()),
            &mut agent,
        ));
        let child = agent.subagent_views.get_mut(child_sid).unwrap();
        let entry = child.scrollback.entries_mut().last().expect("control notice");
        match &entry.block {
            RenderBlock::Notice(notice) => {
                assert_eq!(notice.tone, crate::scrollback::blocks::NoticeTone::Success);
                assert_eq!(notice.event_id.as_deref(), Some("event-child-control"));
            }
            other => panic!("expected child control Notice, got {other:?}"),
        }
    }

    // ── apply_retry_state ─────────────────────────────────────────────

    #[test]
    fn retry_failed_encrypted_content_sets_model_incompatible() {
        use shell::extensions::notification::RetryState;
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();

        assert!(!session.model_incompatible);
        apply_retry_state(
            &RetryState::Failed {
                error_type: "encrypted_content_mismatch".into(),
                message: "incompatible history".into(),
            },
            &mut session,
            &mut scrollback);
        assert!(
            session.model_incompatible,
            "encrypted_content_mismatch should set model_incompatible flag"
        );
    }

    #[test]
    fn retry_failed_other_type_does_not_set_model_incompatible() {
        use shell::extensions::notification::RetryState;
        let mut session = make_session(Some("s1"));
        let mut scrollback = ScrollbackState::new();

        apply_retry_state(
            &RetryState::Failed {
                error_type: "api_400".into(),
                message: "bad request".into(),
            },
            &mut session,
            &mut scrollback);
        assert!(
            !session.model_incompatible,
            "non-encrypted_content error types must not set model_incompatible"
        );
    }
