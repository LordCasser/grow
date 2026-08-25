use super::*;

pub(super) fn handle_queue_changed(notif: &acp::ExtNotification, app: &mut AppView) -> bool {
    let Ok(changed) =
        serde_json::from_str::<crate::app::prompt_queue::QueueChanged>(notif.params.get())
    else {
        tracing::warn!("Failed to parse grow/queue/changed");
        return false;
    };

    let running_prompt_id = changed.running_prompt_id.clone();
    let session_id = changed.session_id.clone();

    // Prefer running_* fields on the payload (authoritative; present when a
    // turn is promoting). Fall back to the local mirror for older shells.
    let running_entry = running_prompt_id.as_ref().and_then(|pid| {
        app.shared_prompt_queue(&session_id)
            .and_then(|q| q.iter().find(|e| &e.id == pid).cloned())
    });
    let running_text: Option<String> = changed
        .running_text
        .clone()
        .or_else(|| running_entry.as_ref().map(|e| e.text.clone()));
    let running_combined: Option<Vec<String>> = changed
        .running_combined_texts
        .clone()
        .filter(|v| v.len() >= 2)
        .or_else(|| {
            running_entry
                .as_ref()
                .and_then(|e| e.combined_texts.clone())
                .filter(|v| v.len() >= 2)
        });
    let queue_kind: String = changed
        .running_kind
        .clone()
        .or_else(|| running_entry.as_ref().map(|e| e.kind.clone()))
        .unwrap_or_else(|| "prompt".to_string());
    let running_kind = match changed.running_origin.as_deref() {
        Some("scheduler_fired") => "cron".to_string(),
        Some("user") | Some("plan_resume") => queue_kind,
        Some(_) => "internal".to_string(),
        None => queue_kind,
    };
    let running_turn_kind = changed.running_turn_kind.clone();

    // Resolve the owning agent before the queue is replaced.
    let sid = acp::SessionId::new(session_id.clone());
    let agent_id = match find_session_match(app, &sid) {
        Some(SessionMatch::Root(id)) => Some(id),
        _ => None,
    };

    let recv_entry_ids: Vec<&str> = changed.entries.iter().map(|e| e.id.as_str()).collect();
    // Raw (pre-merge) broadcast rows for the optimistic-echo reconcile: the
    // post-apply snapshot re-pins unconfirmed echoes, so only the broadcast
    // itself can prove a row landed shell-side.
    let raw_entries: Vec<(String, u64)> = changed
        .entries
        .iter()
        .map(|e| (e.id.clone(), e.version))
        .collect();
    let local_current_prompt_id = agent_id
        .and_then(|aid| app.agents.get(&aid))
        .and_then(|a| a.session.current_prompt_id.clone())
        .unwrap_or_default();
    tracing::debug!(
        target: "qtrace",
        pid = std::process::id(),
        event = "queue_changed_recv",
        session = %session_id,
        running_prompt_id = running_prompt_id.as_deref().unwrap_or(""),
        local_current_prompt_id = %local_current_prompt_id,
        entry_count = changed.entries.len(),
        entries = ?recv_entry_ids,
        "received grow/queue/changed broadcast",
    );

    app.apply_queue_changed(changed);

    // Mirror the reconciled shared queue into the owning agent so the queue
    // pane can render the union of local + server rows without needing
    // `AppView` access during draw / input handling.
    let mut lifecycle_effects = Vec::new();
    if let Some(aid) = agent_id {
        let snapshot = app
            .shared_prompt_queue(&session_id)
            .cloned()
            .unwrap_or_default();
        if let Some(agent) = app.agents.get_mut(&aid) {
            agent.session.shared_queue = snapshot;
            // Cleanup hook: if the user is editing a server-origin row and
            // that row is no longer in the broadcast (started draining,
            // removed by another client, etc.), exit editing mode so the
            // composer isn't stranded on a ghost row. Don't dispatch any
            // follow-up Action — the broadcast already reconciled the
            // queue state for every other client.
            let stranded_server_id = match &agent.prompt_mode {
                super::super::agent_view::PromptMode::EditingQueued {
                    server_id: Some(sid),
                    ..
                } if !agent.session.shared_queue.iter().any(|e| &e.id == sid) => Some(sid.clone()),
                _ => None,
            };
            if let Some(sid) = stranded_server_id {
                tracing::debug!(
                    server_id = %sid,
                    "exiting EditingQueued: row is no longer in the shared queue"
                );
                if let Some(effect) = agent.cancel_editing_queued_for_lost_row() {
                    lifecycle_effects.push(effect);
                }
            }
        }
        app.pending_effects.extend(lifecycle_effects);
        // Resolve a queue-row send-now that was parked while its row was
        // still an optimistic echo: the broadcast just confirmed the row, so
        // fire the interject with the authoritative version (racing it
        // earlier would have no-opped shell-side and dropped the send-now).
        let fire = app.agents.get_mut(&aid).and_then(|agent| {
            agent
                .session
                .resolve_send_now_awaiting_confirm(&raw_entries, running_prompt_id.as_deref())
        });
        if let Some((id, expected_version)) = fire {
            let Some(expected_turn_id) = running_prompt_id.clone() else {
                return true;
            };
            crate::unified_log::info(
                "prompt.queue_send_now_confirmed",
                Some(&session_id),
                Some(serde_json::json!({ "prompt_id": id, "version": expected_version })),
            );
            app.pending_effects
                .push(crate::app::actions::Effect::QueueInterject {
                    session_id: sid.clone(),
                    expected_turn_id,
                    id,
                    expected_version,
                    new_text: None,
                });
        }
    }

    // The structured foreground snapshot is authoritative. Prompt ids are
    // identities only; origin and visibility come from the explicit fields.
    match (running_prompt_id, agent_id) {
        (None, _) => {}
        (Some(_), Some(_)) if running_turn_kind.is_none() => {}
        (Some(pid), Some(aid)) => {
            let current = app
                .agents
                .get(&aid)
                .and_then(|a| a.session.current_prompt_id.clone());
            match current {
                // Already tracking this running prompt — inert.
                Some(c) if c == pid => {
                    // Locally submitted but not optimistically promoted: the
                    // server has now confirmed foreground ownership.
                    if app
                        .agents
                        .get(&aid)
                        .is_some_and(|agent| agent.session.state.is_turn_submitting())
                    {
                        let page_flip_entry = app.agents.get_mut(&aid).and_then(|agent| {
                            super::super::dispatch::apply_turn_start_shim(
                                agent,
                                pid,
                                running_text,
                                &running_kind,
                                running_combined,
                            )
                        });
                        super::super::dispatch::note_peek_page_flip(app, aid, page_flip_entry);
                    }
                }
                // A different local id is a stale snapshot or a cross-channel
                // handoff. Adopt the authoritative foreground immediately;
                // terminals remain keyed by their own turn id.
                None | Some(_) => {
                    let page_flip_entry = app.agents.get_mut(&aid).and_then(|agent| {
                        super::super::dispatch::apply_turn_start_shim(
                            agent,
                            pid,
                            running_text,
                            &running_kind,
                            running_combined,
                        )
                    });
                    super::super::dispatch::note_peek_page_flip(app, aid, page_flip_entry);
                }
            }
        }
        _ => {}
    }
    true
}
