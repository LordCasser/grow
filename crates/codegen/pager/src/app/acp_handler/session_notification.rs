use super::*;
use crate::scrollback::blocks::{NoticeCategory, NoticeTone};
use shell::sampling::error::format_rate_limited_user_message;

fn ui_notice_block(
    notice: shell::extensions::notification::UiNotice,
    event_id: Option<String>,
) -> RenderBlock {
    let tone = match notice.tone {
        shell::extensions::notification::UiNoticeTone::Info => NoticeTone::Info,
        shell::extensions::notification::UiNoticeTone::Success => NoticeTone::Success,
        shell::extensions::notification::UiNoticeTone::Warning => NoticeTone::Warning,
        shell::extensions::notification::UiNoticeTone::Error => NoticeTone::Error,
    };
    let category = match notice.category {
        shell::extensions::notification::UiNoticeCategory::Command => NoticeCategory::Command,
        shell::extensions::notification::UiNoticeCategory::Coordination => {
            NoticeCategory::Coordination
        }
        shell::extensions::notification::UiNoticeCategory::Lifecycle => NoticeCategory::Lifecycle,
    };
    let mut metadata = Vec::new();
    if matches!(category, NoticeCategory::Coordination) {
        metadata.push(format!("Inquiry ID: {}", notice.correlation_id));
    }
    if let Some(subject) = notice.subject {
        metadata.push(match category {
            NoticeCategory::Command => format!("Command: {subject}"),
            _ => format!("Subject: {subject}"),
        });
    }
    if let Some(description) = notice.description {
        metadata.push(description);
    }
    if let Some(details) = notice.details {
        metadata.push(details);
    }
    let details = (!metadata.is_empty()).then(|| metadata.join("\n"));
    match event_id {
        Some(event_id) => {
            RenderBlock::terminal_notice(event_id, tone, category, notice.message, details)
        }
        None => RenderBlock::typed_notice(tone, category, notice.message, details),
    }
}

/// Timeline audit identity and transcript presentation are different things:
/// source tools own their UI; target events update one passive tool-style row.
fn apply_ui_notice(
    scrollback: &mut crate::scrollback::state::ScrollbackState,
    mut notice: shell::extensions::notification::UiNotice,
    event_id: Option<String>,
    is_replay: bool,
) -> bool {
    use crate::scrollback::blocks::tool::{CoordinationRow, OtherToolCallBlock};
    if notice.category == shell::extensions::notification::UiNoticeCategory::Coordination {
        match notice.subject.as_deref() {
            Some("outgoing inquiry" | "outgoing inquiry completed") => return false,
            Some("incoming inquiry" | "inquiry approval" | "inquiry completed") => {
                let Some(audit) = shell::coordination::IncomingInquiryAudit::from_notice(&notice)
                else {
                    // Without structured identity we cannot safely merge or
                    // hold the native history frontier. Preserve the raw fact.
                    scrollback.push_block(ui_notice_block(notice, event_id));
                    return true;
                };
                let terminal = audit.outcome.is_some();
                let coordination = CoordinationRow {
                    source_peer_id: audit.source_peer_id.clone(),
                    inquiry_id: notice.correlation_id.clone(),
                    terminal,
                };
                notice.details = Some(audit.display_details());
                let failed = terminal
                    && notice.tone != shell::extensions::notification::UiNoticeTone::Success;
                let RenderBlock::Notice(notice) = ui_notice_block(notice, event_id) else {
                    unreachable!()
                };
                let mut block = OtherToolCallBlock::new(notice.text, "")
                    .with_output(notice.details.unwrap_or_default());
                if failed {
                    block.error = Some(block.name.clone());
                }
                block.coordination = Some(coordination);
                return scrollback.upsert_coordination_row(block, is_replay);
            }
            _ => {} // Runtime health errors remain visible notices.
        }
    }
    scrollback.push_block(ui_notice_block(notice, event_id));
    true
}

/// Merge the durable descriptor carried by a replayed spawn into an existing
/// live entity. A reconnect replay is not a new spawn: replacing the whole
/// `SubagentInfo` would erase a live/terminal status, counters, kill state and
/// activity observed while the reload was in flight.
fn merge_replayed_subagent(
    existing: &mut crate::app::subagent::SubagentInfo,
    incoming: crate::app::subagent::SubagentInfo,
) {
    existing.subagent_id = incoming.subagent_id;
    existing.child_session_id = incoming.child_session_id;
    existing.description = incoming.description;
    existing.subagent_type = incoming.subagent_type;
    if incoming.model.is_some() {
        existing.model = incoming.model;
    }
    if incoming.context_source.is_some() {
        existing.context_source = incoming.context_source;
    }
    if incoming.resumed_from.is_some() {
        existing.resumed_from = incoming.resumed_from;
    }
    if incoming.capability_mode.is_some() {
        existing.capability_mode = incoming.capability_mode;
    }
    if incoming.permission_mode.is_some() {
        existing.permission_mode = incoming.permission_mode;
    }
    if incoming.effective_permission_mode.is_some() {
        existing.effective_permission_mode = incoming.effective_permission_mode;
    }
    if incoming.workflow_run_id.is_some() {
        existing.workflow_run_id = incoming.workflow_run_id;
    }
    existing.context_normalized = incoming.context_normalized;
    if incoming.parent_prompt_id.is_some() {
        existing.parent_prompt_id = incoming.parent_prompt_id;
    }
}

/// Stash a live stop/stop_failure batch under `stash_pid` for the turn marker
/// to fold. `merge_same_name` merges a same-name repeat instead of standalone.
pub(super) fn stash_live_stop_batch(
    agent: &mut AgentView,
    stash_pid: Option<String>,
    event_name: String,
    hook_entries: Vec<crate::scrollback::blocks::tool::HookRunEntry>,
    merge_same_name: bool,
) {
    if let Some(stale) = agent
        .pending_stop_hooks
        .take_if(|p| p.prompt_id != stash_pid)
    {
        for (name, runs) in stale.groups {
            agent.scrollback.push_lifecycle_hooks(name, runs);
        }
    }
    let pending = agent.pending_stop_hooks.get_or_insert_with(|| {
        super::super::agent_view::PendingStopHooks {
            prompt_id: stash_pid,
            groups: Vec::new(),
        }
    });
    match pending
        .groups
        .iter()
        .position(|(name, _)| *name == event_name)
    {
        Some(idx) if merge_same_name => {
            pending.groups[idx].1.extend(hook_entries);
        }
        Some(_) => {
            agent
                .scrollback
                .push_lifecycle_hooks(event_name, hook_entries);
        }
        None => {
            pending.groups.push((event_name, hook_entries));
        }
    }
}
pub(super) fn refresh_context_used(view: &mut AgentView, used: u64) {
    let total = view.session.models.get_context_window().unwrap_or(0);
    view.apply_context_used(used, total);
}
/// Refresh the bar and record `used` as the confirmed count for a pending
/// compaction message; call only from the `meta.totalTokens` path.
pub(super) fn confirm_context_used(view: &mut AgentView, used: u64) {
    refresh_context_used(view, used);
    view.session.note_context_used(used);
}

/// Replay gate shared by the ACP and Grow session-update paths. Returns `true`
/// when the update must be dropped.
///
/// Replay is only expected while a `session/load` is in flight for this agent
/// (fresh-view load or reconnect reload window). Anything else is misrouted —
/// e.g. a leader falling through to broadcast another client's replay, or a
/// replay landing after its reload already timed out — and applying it would
/// append duplicated history below the live transcript. An expected replay is
/// recorded on the open reload window instead (see
/// [`AgentView::mark_reload_replay_seen`]). One `warn!` per incident; the rest
/// of the burst (one line per replayed event) logs at `debug!`.
pub(super) fn drop_unexpected_replay(
    agent: &mut AgentView,
    meta: &NotificationMeta,
    session_id: &str,
    source: &'static str,
) -> bool {
    if !meta.is_replay {
        return false;
    }
    if agent.session.loading_replay {
        agent.mark_reload_replay_seen();
        return false;
    }
    if agent.session.unexpected_replay_drops == 0 {
        tracing::warn!(
            session_id,
            source,
            event_id = meta.event_id.as_deref(),
            "Dropping unexpected replay update (no session load in flight); further drops logged at debug"
        );
    } else {
        tracing::debug!(
            session_id,
            source,
            event_id = meta.event_id.as_deref(),
            "Dropping unexpected replay update"
        );
    }
    agent.session.unexpected_replay_drops = agent.session.unexpected_replay_drops.saturating_add(1);
    true
}
/// Advance the reconnect cursor to an APPLIED update's eventId. Called from
/// every applied arm (Plan, bg-stdout, tracker) — dropped updates (dedup,
/// promptId gate, unexpected replay) deliberately don't move it.
pub(super) fn advance_reconnect_cursor(agent: &mut AgentView, meta: &mut NotificationMeta) {
    if let Some(id) = meta.event_id.take() {
        agent.advance_reconnect_cursor(id, meta.is_replay);
    }
}
/// Handle `grow/session_notification` and replay-path `grow/session/update`.
///
/// Routes by `session_id` so events for an inactive agent still mutate that
/// agent's state. The redraw decision is gated on whether the matched agent
/// is the currently visible one.
pub(crate) fn handle_session_notification(notif: &acp::ExtNotification, app: &mut AppView) -> bool {
    handle_session_notification_inner(notif, app, None)
}

/// Apply descendant lifecycle and control replay to the root view that owns
/// the durable parent→child metadata chain. Disk restoration already resolved
/// ownership; routing it through the global session-id lookup again could
/// select a separately opened copy of the same hidden child session.
pub(crate) fn handle_descendant_state_replay(
    notif: &acp::ExtNotification,
    app: &mut AppView,
    owner_agent_id: AgentId,
) -> bool {
    handle_session_notification_inner(notif, app, Some(owner_agent_id))
}

fn handle_session_notification_inner(
    notif: &acp::ExtNotification,
    app: &mut AppView,
    forced_descendant_owner: Option<AgentId>,
) -> bool {
    let Ok(session_notif) = serde_json::from_str::<SessionNotification>(notif.params.get()) else {
        tracing::warn!("Failed to parse {}", notif.method.as_ref());
        return false;
    };
    match &session_notif.update {
        GrowSessionUpdate::TaskBackgrounded { .. } => {
            return handle_task_backgrounded(notif, app);
        }
        GrowSessionUpdate::TaskCompleted { .. } => {
            return handle_task_completed(notif, app);
        }
        GrowSessionUpdate::ScheduledTaskCreated { .. } => {
            return handle_scheduled_task_created(notif, app);
        }
        GrowSessionUpdate::ScheduledTaskDeleted { .. } => {
            return handle_scheduled_task_deleted(notif, app);
        }
        _ => {}
    }
    let matched = match forced_descendant_owner
        .filter(|owner| {
            app.agents.get(owner).is_some_and(|agent| {
                agent
                    .subagent_views
                    .contains_key(session_notif.session_id.0.as_ref())
            })
        })
        .map(SessionMatch::Child)
        .or_else(|| {
            forced_descendant_owner
                .is_none()
                .then(|| find_session_match(app, &session_notif.session_id))
                .flatten()
        }) {
        Some(m) => m,
        None => {
            tracing::debug!(
                session_id = session_notif.session_id.0.as_ref(),
                method = notif.method.as_ref(),
                "load-race: grow/session_notification DROPPED — no agent matches session_id"
            );
            return false;
        }
    };
    let parent_id = matched.agent_id();
    let is_active = is_matched_agent_active(app, parent_id);
    let spawned_control_handoff = match &session_notif.update {
        GrowSessionUpdate::SubagentSpawned {
            child_session_id, ..
        } => app
            .screen_mode_control_handoffs
            .get(child_session_id)
            .cloned(),
        _ => None,
    };
    let agent = app
        .agents
        .get_mut(&parent_id)
        .expect("find_session_match returned an existing AgentId");
    // Subagent lifecycle notifications are emitted on the immediate parent
    // session. A nested spawn therefore arrives with a child envelope, but it
    // must still update the top-level owner's flat descendant index so later
    // grandchild permissions and lifecycle events can route by their real
    // session id.
    let descendant_lifecycle = matches!(
        &session_notif.update,
        GrowSessionUpdate::SubagentSpawned { .. }
            | GrowSessionUpdate::SubagentProgress { .. }
            | GrowSessionUpdate::SubagentFinished { .. }
    );
    let meta = NotificationMeta::from_json(session_notif.meta.as_ref().and_then(|v| v.as_object()));
    if matches!(matched, SessionMatch::Child(_)) && !descendant_lifecycle {
        let child_sid: &str = session_notif.session_id.0.as_ref();
        let handled = matches!(
            &session_notif.update,
            GrowSessionUpdate::ModelChanged { .. }
                | GrowSessionUpdate::AgentChanged { .. }
                | GrowSessionUpdate::ControlStateUpdate(_)
                | GrowSessionUpdate::UiNotice(_)
                | GrowSessionUpdate::InteractionResolved { .. }
                | GrowSessionUpdate::AutoCompactStarted { .. }
                | GrowSessionUpdate::AutoCompactCompleted { .. }
                | GrowSessionUpdate::AutoCompactFailed { .. }
                | GrowSessionUpdate::AutoCompactCancelled { .. }
                | GrowSessionUpdate::RetryState(_)
                | GrowSessionUpdate::MemoryFlushCompleted { .. }
                | GrowSessionUpdate::MemoryDreamCompleted { .. }
                | GrowSessionUpdate::MemorySessionSaved { .. }
        );
        let Some(child) = agent.subagent_views.get_mut(child_sid) else {
            return false;
        };
        if forced_descendant_owner.is_none()
            && drop_unexpected_replay(child, &meta, child_sid, "grow/session/update child")
        {
            return false;
        }
        if !meta.is_replay
            && meta.event_seq.is_some_and(|seq| {
                child
                    .session
                    .last_applied_grow_event_seq
                    .is_some_and(|last| seq <= last)
            })
        {
            tracing::debug!(
                session_id = child_sid,
                event_seq = meta.event_seq,
                last_applied = child.session.last_applied_grow_event_seq,
                "child grow/session update dropped by dedup highwater"
            );
            return false;
        }
        let controls_pending_before = child.session.controls_pending();
        let changed = handle_child_session_notification(
            session_notif.update,
            child_sid,
            meta.event_id.clone(),
            meta.is_replay,
            agent,
        );
        let control_resolved = controls_pending_before
            && agent
                .subagent_views
                .get(child_sid)
                .is_some_and(|child| !child.session.controls_pending());
        let control_drain = control_resolved
            .then(|| agent.subagent_views.get_mut(child_sid))
            .flatten()
            .map(|child| crate::app::root::dispatch::maybe_drain_queue(child));
        if handled && let Some(child) = agent.subagent_views.get_mut(child_sid) {
            if let Some(seq) = meta.event_seq {
                child.session.last_applied_grow_event_seq = Some(seq);
            }
            if let Some(id) = meta.event_id {
                child.advance_reconnect_cursor(id, meta.is_replay);
            }
        }
        if let Some(drain) = control_drain {
            crate::app::root::dispatch::note_peek_page_flip(app, parent_id, drain.page_flip_entry);
            app.pending_effects.extend(drain.effects);
        }
        return (changed || control_resolved) && is_active;
    }
    let descendant_lifecycle_from_child =
        matches!(matched, SessionMatch::Child(_)) && descendant_lifecycle;
    if descendant_lifecycle_from_child
        && meta.event_seq.is_some_and(|seq| {
            agent
                .subagent_views
                .get(session_notif.session_id.0.as_ref())
                .and_then(|parent_view| parent_view.session.last_applied_grow_event_seq)
                .is_some_and(|last| seq <= last)
        })
    {
        tracing::debug!(
            session_id = session_notif.session_id.0.as_ref(),
            event_seq = meta.event_seq,
            "nested subagent lifecycle update dropped by parent-child highwater"
        );
        return false;
    }
    if !descendant_lifecycle_from_child
        && drop_unexpected_replay(
            agent,
            &meta,
            session_notif.session_id.0.as_ref(),
            "grow/session/update",
        )
    {
        return false;
    }
    let is_workflow_update = matches!(
        session_notif.update,
        GrowSessionUpdate::WorkflowUpdated { .. }
    );
    if !descendant_lifecycle_from_child
        && !is_workflow_update
        && !meta.is_replay
        && meta.event_seq.is_some_and(|seq| {
            agent
                .session
                .last_applied_grow_event_seq
                .is_some_and(|last| seq <= last)
        })
    {
        tracing::debug!(
            session_id = session_notif.session_id.0.as_ref(),
            event_seq = meta.event_seq,
            last_applied = agent.session.last_applied_grow_event_seq,
            "grow/session update DROPPED by dedup highwater (event_seq <= last_applied)"
        );
        return false;
    }
    let mut plugins_changed_needs_skills_refetch = false;
    let mut terminal_outcome: Option<super::super::agent_view::turn_completion::TerminalOutcome> =
        None;
    let root_session_id: &str = session_notif.session_id.0.as_ref();
    let controls_pending_before = agent.session.controls_pending();
    let changed = match session_notif.update {
        GrowSessionUpdate::UiNotice(output) => apply_ui_notice(
            &mut agent.scrollback,
            output,
            meta.event_id.clone(),
            meta.is_replay,
        ),
        GrowSessionUpdate::ControlStateUpdate(update) => {
            apply_control_state_update(agent, update, meta.event_id.clone(), meta.is_replay)
        }
        ref update @ (GrowSessionUpdate::AutoCompactStarted { .. }
        | GrowSessionUpdate::AutoCompactCompleted { .. }
        | GrowSessionUpdate::AutoCompactFailed { .. }
        | GrowSessionUpdate::AutoCompactCancelled { .. }
        | GrowSessionUpdate::RetryState(_)
        | GrowSessionUpdate::ImageDropped { .. }
        | GrowSessionUpdate::ImageProjected { .. }
        | GrowSessionUpdate::MemoryFlushCompleted { .. }
        | GrowSessionUpdate::MemoryDreamCompleted { .. }
        | GrowSessionUpdate::MemorySessionSaved { .. }) => {
            let changed = apply_session_event(update, &mut agent.session, &mut agent.scrollback);
            if let GrowSessionUpdate::AutoCompactCompleted {
                tokens_after,
                async_compact,
                ..
            } = update
            {
                refresh_context_used(agent, *tokens_after);
                if !async_compact {
                    agent.todo.update_todos(Vec::new());
                }
            }
            changed
        }
        GrowSessionUpdate::ImageCompressed {
            ref images,
            ref message,
        } => apply_image_compressed(agent, images, message),
        GrowSessionUpdate::SubagentPermissionDecision {
            child_session_id,
            subagent_type,
            description,
            tool_call_id,
            tool_name,
            access_kind,
            access_summary,
            access_detail,
            outcome,
            source,
            reason,
            classifier_reason,
            latency_ms,
        } => {
            let subagent_title = agent
                .session
                .subagent_sessions
                .get(&child_session_id)
                .map(crate::app::subagent::format_subagent_title);
            agent.scrollback.push_subagent_permission(
                crate::scrollback::blocks::SubagentPermissionEvent {
                    child_session_id,
                    subagent_title,
                    subagent_type,
                    description,
                    tool_call_id,
                    tool_name,
                    access_kind,
                    access_summary,
                    access_detail,
                    outcome,
                    source,
                    reason,
                    classifier_reason,
                    latency_ms,
                },
            );
            true
        }
        GrowSessionUpdate::TurnCompleted {
            prompt_id,
            stop_reason,
            agent_result,
            ..
        } => {
            if agent.session.loading_replay {
                agent.scrollback.seal_subagent_permission_group();
                agent.session.replayed_terminal_prompts.insert(prompt_id);
                false
            } else {
                terminal_outcome = Some(agent.finalize_turn_from_durable_terminal(
                    &prompt_id,
                    Some(&stop_reason),
                    agent_result.as_deref(),
                ));
                false
            }
        }
        GrowSessionUpdate::SubagentSpawned {
            subagent_id,
            child_session_id,
            subagent_type,
            description,
            model,
            model_state,
            workflow_agent_names,
            effective_context_source,
            resumed_from,
            capability_mode,
            permission_mode,
            effective_permission_mode,
            context_normalized,
            parent_prompt_id,
            workflow_run_id,
            ..
        } => {
            tracing::info!(
                child_session_id = %child_session_id,
                subagent_type = %subagent_type,
                "Subagent spawned"
            );
            if !meta.is_replay
                && agent
                    .session
                    .subagent_sessions
                    .contains_key(&child_session_id)
            {
                tracing::debug!(
                    child_session_id = %child_session_id,
                    "ignoring duplicate live SubagentSpawned lifecycle fact"
                );
                return false;
            }
            let is_background = agent
                .session
                .tracker
                .task_tool_background
                .remove(&subagent_id)
                .unwrap_or(false);
            let model_display = model.clone();
            let has_child_model_state = model_state.is_some();
            let child_models = match model_state {
                Some(state) => crate::acp::model_state::ModelState::from(Some(state)),
                None => agent.session.models.clone(),
            };
            // A terminal event can arrive from the live stream before the
            // replayed spawn that causally precedes it. Keep that terminal
            // row as the entity's authoritative lifecycle fact so the later
            // spawn cannot append a visually reversed Started row.
            let replayed_terminal =
                agent
                    .scrollback
                    .iter_entries()
                    .find_map(|(entry_id, entry)| {
                        let RenderBlock::Subagent(block) = &entry.block else {
                            return None;
                        };
                        if block.child_session_id != child_session_id || block.is_running() {
                            return None;
                        }
                        let (status, elapsed, error) = match &block.kind {
                            crate::scrollback::blocks::SubagentBlockKind::Completed { elapsed } => {
                                ("completed", *elapsed, None)
                            }
                            crate::scrollback::blocks::SubagentBlockKind::Failed {
                                elapsed,
                                error,
                            } => ("failed", *elapsed, error.clone()),
                            crate::scrollback::blocks::SubagentBlockKind::Cancelled { elapsed } => {
                                ("cancelled", *elapsed, None)
                            }
                            crate::scrollback::blocks::SubagentBlockKind::Started => return None,
                        };
                        Some((entry_id, status.to_string(), elapsed, error))
                    });
            let incoming = SubagentInfo {
                subagent_id: Arc::from(subagent_id),
                child_session_id: Arc::from(child_session_id.clone()),
                description: Arc::from(description.clone()),
                subagent_type: Arc::from(subagent_type.clone()),
                model: model.map(Arc::from),
                context_source: effective_context_source.map(Arc::from),
                resumed_from: resumed_from.map(Arc::from),
                capability_mode: capability_mode.map(Arc::from),
                permission_mode: permission_mode.clone().map(Arc::from),
                effective_permission_mode: effective_permission_mode.clone().map(Arc::from),
                workflow_run_id: workflow_run_id.clone().map(Arc::from),
                context_normalized,
                parent_prompt_id: parent_prompt_id.map(Arc::from),
                started_at: std::time::Instant::now(),
                last_progress_at: std::time::Instant::now(),
                finished: false,
                status: None,
                error: None,
                duration_ms: None,
                tool_calls: None,
                turns: None,
                turn_count: None,
                tool_call_count: None,
                tokens_used: None,
                context_window_tokens: None,
                context_usage_pct: None,
                tools_used: Vec::new(),
                error_count: None,
                activity_label: None,
                is_background,
                pending_kill: false,
                kill_requested_at: None,
                scrollback_entry_id: None,
                prompt: None,
                child_cwd: None,
                worktree_path: None,
                child_updates_replayed: false,
            };
            let existing_finished = agent
                .session
                .subagent_sessions
                .get(&child_session_id)
                .is_some_and(|info| info.finished);
            let terminal_finished = existing_finished || replayed_terminal.is_some();
            if meta.is_replay {
                if let Some(existing) = agent.session.subagent_sessions.get_mut(&child_session_id) {
                    merge_replayed_subagent(existing, incoming);
                } else {
                    agent
                        .session
                        .subagent_sessions
                        .insert(child_session_id.clone(), incoming);
                }
            } else {
                agent
                    .session
                    .subagent_sessions
                    .insert(child_session_id.clone(), incoming);
            }
            if let Some(ref sid) = agent.session.session_id
                && let Some(info) = agent.session.subagent_sessions.get_mut(&child_session_id)
            {
                crate::app::subagent::enrich_from_timeline(
                    info,
                    &agent.session.cwd,
                    sid.0.as_ref(),
                );
            }
            let (effective_child_cwd, effective_is_worktree) = derive_child_cwd(
                &agent.session.cwd,
                agent.session.subagent_sessions.get(&child_session_id),
            );
            if let Some(child_view) = agent.subagent_views.get_mut(&child_session_id) {
                // A reconnect replay repeats the durable spawn fact. Reuse the
                // existing exact-session view so its rearmed control tokens
                // and queued user intent cannot collapse back to generation 0.
                child_view.session.models = child_models;
                child_view.session.workflow_agent_names = workflow_agent_names.clone();
                if !has_child_model_state && let Some(model_id) = model_display.as_deref() {
                    child_view
                        .session
                        .models
                        .set_current(shell::agent::models::ModelId::new(model_id), None);
                }
                child_view.session.cwd = effective_child_cwd;
                child_view.session.permission_mode = effective_permission_mode
                    .as_deref()
                    .map(shell::util::config::parse_permission_mode_canonical)
                    .unwrap_or(shell::util::config::PermissionMode::Ask);
                child_view
                    .session
                    .apply_agent_name(Some(subagent_type.clone()));
                child_view.session.state = if terminal_finished {
                    AgentState::Idle
                } else {
                    AgentState::TurnRunning
                };
                child_view.session.set_worktree(effective_is_worktree);
                if !child_view.session.controls_pending()
                    && let Some(handoff) = spawned_control_handoff.clone()
                {
                    child_view
                        .session
                        .restore_screen_mode_control_handoff(handoff);
                }
            } else {
                let child_session = {
                    let mut session = AgentSession::new(
                        AgentId(0),
                        agent.session.acp_tx.clone(),
                        Some(acp::SessionId::new(child_session_id.clone())),
                        child_models,
                        effective_child_cwd,
                        effective_permission_mode
                            .as_deref()
                            .map(shell::util::config::parse_permission_mode_canonical)
                            .unwrap_or(shell::util::config::PermissionMode::Ask),
                    );
                    if !has_child_model_state && let Some(model_id) = model_display.as_deref() {
                        session
                            .models
                            .set_current(shell::agent::models::ModelId::new(model_id), None);
                    }
                    session.workflow_agent_names = workflow_agent_names;
                    session.apply_agent_name(Some(subagent_type.clone()));
                    session.state = if terminal_finished {
                        AgentState::Idle
                    } else {
                        AgentState::TurnRunning
                    };
                    session.set_worktree(effective_is_worktree);
                    if let Some(handoff) = spawned_control_handoff.clone() {
                        session.restore_screen_mode_control_handoff(handoff);
                    }
                    session
                };
                let mut child_scrollback = crate::scrollback::state::ScrollbackState::new();
                child_scrollback.set_appearance(agent.scrollback.appearance().clone());
                let mut child_view = AgentView::new(child_session, child_scrollback);
                child_view.input_mode = InputMode::Vim;
                child_view.active_pane = crate::views::agent::ActivePane::Scrollback;
                let dashboard_visible = agent
                    .prompt
                    .slash_controller
                    .registry()
                    .get("dashboard")
                    .is_some();
                child_view.set_dashboard_visible(dashboard_visible);
                child_view.set_has_session_announcements(
                    agent.prompt.slash_controller.has_session_announcements(),
                );
                child_view
                    .prompt
                    .set_screen_mode(agent.prompt.slash_controller.screen_mode());
                let recap_visible = agent
                    .prompt
                    .slash_controller
                    .registry()
                    .get("recap")
                    .is_some();
                child_view.set_session_recap_available(recap_visible);
                agent.insert_subagent_view(child_session_id.clone(), Box::new(child_view));
            }
            let child_updates_replayed = agent
                .session
                .subagent_sessions
                .get(&child_session_id)
                .is_some_and(|info| info.child_updates_replayed);
            if !agent.session.loading_replay && !child_updates_replayed {
                if let Some(child_view) = agent.subagent_views.get_mut(&child_session_id) {
                    crate::app::subagent::replay_inherited_updates(child_view, &child_session_id);
                }
                if let Some(info) = agent.session.subagent_sessions.get_mut(&child_session_id) {
                    info.child_updates_replayed = true;
                }
            }
            if workflow_run_id.is_none() {
                if let Some((entry_id, status, elapsed, error)) = replayed_terminal {
                    if let Some(info) = agent.session.subagent_sessions.get_mut(&child_session_id) {
                        info.finished = true;
                        info.status = Some(Arc::from(status));
                        info.error = error.map(Arc::from);
                        info.duration_ms = Some(elapsed.as_millis() as u64);
                        info.scrollback_entry_id = Some(entry_id);
                        info.pending_kill = false;
                        info.kill_requested_at = None;
                    }
                } else {
                    let block = crate::scrollback::blocks::SubagentBlock::started(
                        &description,
                        &child_session_id,
                        &subagent_type,
                        model_display,
                        is_background,
                    )
                    .with_event_id(meta.event_id.clone());
                    let outcome = agent
                        .scrollback
                        .push_block_if_absent(RenderBlock::Subagent(block));
                    if !outcome.inserted {
                        tracing::debug!(
                            child_session_id = %child_session_id,
                            "reused replayed SubagentStarted row by immutable event id"
                        );
                    }
                    if let Some(info) = agent.session.subagent_sessions.get_mut(&child_session_id) {
                        info.scrollback_entry_id = Some(outcome.entry_id);
                        if !meta.is_replay {
                            info.is_background = is_background;
                        }
                    }
                }
            } else if let Some(info) = agent.session.subagent_sessions.get_mut(&child_session_id) {
                if !meta.is_replay {
                    info.is_background = is_background;
                }
            }
            true
        }
        GrowSessionUpdate::SubagentProgress {
            child_session_id,
            duration_ms,
            turn_count,
            tool_call_count,
            tokens_used,
            context_window_tokens,
            context_usage_pct,
            tools_used,
            error_count,
            ..
        } => {
            if let Some(info) = agent.session.subagent_sessions.get_mut(&child_session_id) {
                info.duration_ms = Some(duration_ms);
                info.turn_count = Some(turn_count);
                info.tool_call_count = Some(tool_call_count);
                info.tokens_used = Some(tokens_used);
                info.context_window_tokens = Some(context_window_tokens);
                info.context_usage_pct = Some(context_usage_pct);
                info.tools_used = tools_used.into_iter().map(Arc::from).collect();
                info.error_count = Some(error_count);
                info.last_progress_at = std::time::Instant::now();
            }
            if let Some(child_view) = agent.subagent_views.get_mut(&child_session_id)
                && context_window_tokens > 0
            {
                child_view
                    .session
                    .models
                    .override_context_window(context_window_tokens);
            }
            let activity_label = agent
                .subagent_views
                .get(&child_session_id)
                .and_then(|cv| subagent_activity_label(cv));
            sync_subagent_activity(agent, &child_session_id, activity_label);
            true
        }
        GrowSessionUpdate::SubagentFinished {
            child_session_id,
            status,
            error,
            tool_calls,
            turns,
            duration_ms,
            tokens_used,
            ..
        } => {
            tracing::info!(
                child_session_id = %child_session_id,
                status = %status,
                tool_calls = tool_calls,
                turns = turns,
                duration_ms = duration_ms,
                "Subagent finished"
            );
            let elapsed_dur = std::time::Duration::from_millis(duration_ms);
            agent.clear_transport_interactions_for_session(&child_session_id);
            let info_ref = agent.session.subagent_sessions.get(&child_session_id);
            let already_finished = info_ref.is_some_and(|s| s.finished);
            let is_workflow_child = info_ref.is_some_and(|s| s.workflow_run_id.is_some());
            let description = info_ref.map(|s| s.description.clone()).unwrap_or_default();
            let subagent_type = info_ref
                .map(|s| s.subagent_type.clone())
                .unwrap_or_else(|| Arc::from("subagent"));
            let model = info_ref
                .and_then(|s| s.model.as_ref())
                .map(|model| model.to_string());
            sync_subagent_activity(agent, &child_session_id, None);
            if !already_finished && !is_workflow_child {
                let block = match status.as_str() {
                    "completed" => RenderBlock::Subagent(
                        crate::scrollback::blocks::SubagentBlock::completed(
                            description.as_ref(),
                            child_session_id.as_str(),
                            elapsed_dur,
                        )
                        .with_identity(subagent_type.as_ref(), model.clone())
                        .with_event_id(meta.event_id.clone()),
                    ),
                    "cancelled" => RenderBlock::Subagent(
                        crate::scrollback::blocks::SubagentBlock::cancelled(
                            description.as_ref(),
                            child_session_id.as_str(),
                            elapsed_dur,
                        )
                        .with_identity(subagent_type.as_ref(), model.clone())
                        .with_event_id(meta.event_id.clone()),
                    ),
                    _ => RenderBlock::Subagent(
                        crate::scrollback::blocks::SubagentBlock::failed(
                            description.as_ref(),
                            child_session_id.as_str(),
                            elapsed_dur,
                            error.clone(),
                        )
                        .with_identity(subagent_type.as_ref(), model)
                        .with_event_id(meta.event_id.clone()),
                    ),
                };
                let outcome = agent.scrollback.push_block_if_absent(block);
                if !outcome.inserted {
                    tracing::debug!(
                        child_session_id = %child_session_id,
                        "reused replayed SubagentFinished row by immutable event id"
                    );
                }
            }
            if let Some(info) = agent.session.subagent_sessions.get_mut(&child_session_id) {
                info.finished = true;
                info.status = Some(Arc::from(status));
                info.error = error.map(Arc::from);
                info.duration_ms = Some(duration_ms);
                info.tool_calls = Some(tool_calls);
                info.turns = Some(turns);
                if tokens_used > 0 {
                    info.tokens_used = Some(tokens_used);
                }
                info.pending_kill = false;
                info.kill_requested_at = None;
                info.last_progress_at = std::time::Instant::now();
            }
            let resuming = agent.session.loading_replay;
            if let Some(child_view) = agent.subagent_views.get_mut(&child_session_id) {
                child_view.session.state = AgentState::Idle;
                if !resuming {
                    crate::app::subagent::finalize_finished_child_view(child_view, elapsed_dur);
                }
            }
            true
        }
        GrowSessionUpdate::HookExecution {
            occurrence_id,
            event_name,
            tool_name: _tool_name,
            prompt_id: batch_prompt_id,
            runs,
            annotations,
        } => {
            use crate::scrollback::blocks::tool::{HookPhase, HookRunEntry, HookRunStatus};
            if !agent.session.tracker.claim_hook_occurrence(&occurrence_id) {
                return false;
            }
            let hook_entries: Vec<HookRunEntry> = runs
                .into_iter()
                .map(|r| {
                    let status = match r.status {
                        shell::extensions::notification::HookRunStatusDto::Success {
                            elapsed_ms,
                        } => HookRunStatus::Success {
                            elapsed: std::time::Duration::from_millis(elapsed_ms),
                        },
                        shell::extensions::notification::HookRunStatusDto::Skipped => {
                            HookRunStatus::Skipped
                        }
                        shell::extensions::notification::HookRunStatusDto::Blocked {
                            detail,
                            elapsed_ms,
                        } => HookRunStatus::Blocked {
                            detail,
                            elapsed: std::time::Duration::from_millis(elapsed_ms),
                        },
                        shell::extensions::notification::HookRunStatusDto::Failed {
                            error,
                            elapsed_ms,
                        } => HookRunStatus::Failed {
                            error,
                            elapsed: std::time::Duration::from_millis(elapsed_ms),
                        },
                    };
                    HookRunEntry {
                        name: r.name,
                        status,
                        output: r.output,
                    }
                })
                .collect();
            let is_tool_hook = event_name == "pre_tool_use" || event_name == "post_tool_use";
            let is_stop_hook = event_name == "stop" || event_name == "stop_failure";
            if is_tool_hook && (meta.is_replay || agent.session.loading_replay) {
                // Reconnect snapshots arrive after the ACP transcript replay.
                // Without the original transport ordering, attaching to the
                // last tool would corrupt an unrelated row; render an explicit
                // lifecycle projection instead.
                agent
                    .scrollback
                    .push_lifecycle_hooks(event_name.clone(), hook_entries);
            } else if is_tool_hook {
                let phase = if event_name == "pre_tool_use" {
                    HookPhase::Pre
                } else {
                    HookPhase::Post
                };
                if let Some(entry_id) = agent.scrollback.last_tool_call_entry_id() {
                    agent.scrollback.attach_hooks(entry_id, phase, hook_entries);
                }
            } else if is_stop_hook && !meta.is_replay && !agent.session.loading_replay {
                let local_turn_active =
                    agent.session.state.is_turn_running() || agent.session.state.is_cancelling();
                let foreign_batch = batch_prompt_id.is_some()
                    && agent.session.current_prompt_id.is_some()
                    && batch_prompt_id != agent.session.current_prompt_id;
                if foreign_batch {
                    agent
                        .scrollback
                        .push_lifecycle_hooks(event_name, hook_entries);
                } else if local_turn_active {
                    let stash_pid = batch_prompt_id
                        .clone()
                        .or_else(|| agent.session.current_prompt_id.clone());
                    stash_live_stop_batch(
                        agent,
                        stash_pid,
                        event_name,
                        hook_entries,
                        batch_prompt_id.is_some(),
                    );
                } else if let Some(entry_id) = agent
                    .scrollback
                    .latest_turn_marker_accepting(&event_name, batch_prompt_id.as_deref())
                {
                    agent.scrollback.attach_stop_hooks_to_marker(
                        entry_id,
                        event_name,
                        hook_entries,
                        batch_prompt_id.as_deref(),
                    );
                } else {
                    agent
                        .scrollback
                        .push_lifecycle_hooks(event_name, hook_entries);
                }
            } else {
                agent
                    .scrollback
                    .push_lifecycle_hooks(event_name, hook_entries);
            }
            for message in annotations {
                agent.scrollback.push_block(RenderBlock::session_event(
                    SessionEvent::HookAnnotation {
                        occurrence_id: occurrence_id.clone(),
                        message,
                    },
                ));
            }
            true
        }
        GrowSessionUpdate::HooksChanged {
            hooks,
            project_trusted,
            load_errors,
        } => {
            if let Some(ref mut modal) = agent.extensions_modal {
                use crate::views::extensions_modal::TabDataState;
                modal.hooks_data = TabDataState::Loaded(extension_types::HooksListResponse {
                    hooks,
                    project_trusted,
                    load_errors,
                });
                true
            } else {
                false
            }
        }
        GrowSessionUpdate::PluginsChanged { plugins } => {
            if let Some(ref mut modal) = agent.extensions_modal {
                use crate::views::extensions_modal::TabDataState;
                modal.seed_plugin_groups_once(&plugins);
                modal.plugins_data =
                    TabDataState::Loaded(extension_types::PluginsListResponse { plugins });
                if !matches!(modal.skills_data, TabDataState::Loading) {
                    modal.skills_data = TabDataState::Loading;
                    plugins_changed_needs_skills_refetch = true;
                }
                true
            } else {
                false
            }
        }
        GrowSessionUpdate::PluginUpdatesInstalled { updates } => {
            if updates.is_empty() {
                false
            } else {
                let details = updates
                    .into_iter()
                    .map(|(name, old_version, new_version)| {
                        format!("{name} {old_version} → {new_version}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                agent.scrollback.push_block(RenderBlock::notice(format!(
                    "Plugin updates installed: {details}"
                )));
                true
            }
        }
        GrowSessionUpdate::SessionRecap { summary, auto } => {
            use crate::scrollback::block::RenderBlock;
            use crate::scrollback::blocks::SessionEvent;
            if should_drop_late_auto_recap(auto, meta.is_replay, agent.session.state.is_idle()) {
                tracing::debug!(
                    "dropping late auto SessionRecap; agent busy (turn or command in flight)"
                );
                false
            } else {
                app.notification_service.focus_tracker.mark_recap_shown();
                let recap_block = RenderBlock::session_event(SessionEvent::Recap { summary, auto });
                apply_recap_block(agent, auto, recap_block);
                true
            }
        }
        GrowSessionUpdate::SessionRecapUnavailable => {
            if meta.is_replay {
                false
            } else {
                agent.session.clear_live_feedback("recap");
                agent.show_toast(crate::app::root::dispatch::recap_unavailable_toast(
                    crate::app::root::dispatch::scrollback_has_user_messages(&agent.scrollback),
                ));
                true
            }
        }
        GrowSessionUpdate::ModelAutoSwitched {
            previous_model_id,
            new_model_id,
            reason,
        } => {
            use crate::scrollback::block::RenderBlock;
            use crate::scrollback::blocks::SessionEvent;
            let available_count = agent.session.models.available.len();
            let available_keys: Vec<&str> = agent
                .session
                .models
                .available
                .keys()
                .take(10)
                .map(|m| m.0.as_ref())
                .collect();
            tracing::warn!(
                session_id = session_notif.session_id.0.as_ref(),
                previous = %previous_model_id,
                new = %new_model_id,
                available_count,
                available_keys = ?available_keys,
                "Model auto-switched: previous model no longer available"
            );
            crate::unified_log::warn(
                "model auto-switched: previous model unavailable",
                Some(session_notif.session_id.0.as_ref()),
                Some(serde_json::json!({
                    "previous_model": previous_model_id.as_str(),
                    "new_model": new_model_id.as_str(),
                    "available_count": available_count,
                    "available_keys": available_keys,
                })),
            );
            agent.scrollback.push_block(RenderBlock::session_event(
                SessionEvent::ModelUnavailable {
                    previous_model_id,
                    new_model_id,
                    reason,
                },
            ));
            true
        }
        GrowSessionUpdate::ModelChanged {
            model_id,
            reasoning_effort,
        } => match apply_model_changed(
            agent,
            session_notif.session_id.0.as_ref(),
            model_id,
            reasoning_effort,
        ) {
            Some(changed) => changed,
            None => return false,
        },
        GrowSessionUpdate::AgentChanged { agent_name } => {
            if agent.session.agent_control_pending() {
                if agent.session.resolve_agent_control(&agent_name) {
                    agent.session.apply_agent_name(Some(agent_name))
                } else {
                    agent.session.defer_authoritative_agent_change(agent_name);
                    false
                }
            } else {
                agent.session.apply_agent_name(Some(agent_name))
            }
        }
        GrowSessionUpdate::MemoryFiles { files } => {
            let entries = crate::views::memory_modal::build_entries(files);
            let modal_state = crate::views::memory_modal::MemoryModalState::new(entries);
            agent.active_modal = Some(crate::views::modal::ActiveModal::MemoryBrowser {
                state: Box::new(modal_state),
            });
            true
        }
        update @ GrowSessionUpdate::WorkflowUpdated { .. } => agent.ingest_workflow_update(update),
        GrowSessionUpdate::GoalUpdated {
            goal_id,
            objective,
            status,
            token_budget,
            tokens_used,
            usage_incomplete,
            elapsed_ms,
            created_at,
            updated_at,
            status_message,
        } => {
            if status == "cleared" {
                agent.clear_goal()
            } else {
                let Some(new_status) = GoalDisplayStatus::parse(&status) else {
                    tracing::warn!(status, "ignored malformed GoalUpdated state");
                    return false;
                };
                agent.apply_goal_update(GoalDisplayState {
                    goal_id,
                    objective,
                    status: new_status,
                    token_budget,
                    tokens_used,
                    usage_incomplete,
                    elapsed_ms,
                    created_at,
                    updated_at,
                    status_message,
                    received_at: std::time::Instant::now(),
                    elapsed_floor_ms: elapsed_ms,
                })
            }
        }
        GrowSessionUpdate::InteractionResolved { tool_call_id } => {
            agent.dismiss_resolved_interaction(root_session_id, &tool_call_id)
        }
        _ => {
            tracing::trace!(
                "Ignoring {}: {:?}",
                notif.method.as_ref(),
                std::mem::discriminant(&session_notif.update)
            );
            return false;
        }
    };
    let control_resolved = controls_pending_before && !agent.session.controls_pending();
    let control_drain =
        control_resolved.then(|| crate::app::root::dispatch::maybe_drain_queue(agent));
    if plugins_changed_needs_skills_refetch {
        if let Some(agent) = app.agents.get(&parent_id)
            && let Some(session_id) = agent.session.session_id.clone()
        {
            app.pending_effects.push(Effect::FetchSkillsList {
                agent_id: parent_id,
                session_id,
            });
        } else if let Some(agent) = app.agents.get_mut(&parent_id)
            && let Some(ref mut modal) = agent.extensions_modal
        {
            modal.skills_data =
                crate::views::extensions_modal::TabDataState::Error("No active session".into());
        } else {
            tracing::warn!("PluginsChanged: agent or modal disappeared before skills re-fetch");
        }
    }
    if let Some(agent) = app.agents.get_mut(&parent_id) {
        if descendant_lifecycle_from_child {
            if let Some(parent_view) = agent
                .subagent_views
                .get_mut(session_notif.session_id.0.as_ref())
            {
                if let Some(seq) = meta.event_seq {
                    parent_view.session.last_applied_grow_event_seq = Some(seq);
                }
                if let Some(id) = meta.event_id {
                    parent_view.advance_reconnect_cursor(id, meta.is_replay);
                }
            }
        } else {
            if let Some(seq) = meta.event_seq
                && !meta.is_replay
                && !is_workflow_update
            {
                agent.session.last_applied_grow_event_seq = Some(seq);
            }
            if let Some(id) = meta.event_id {
                agent.advance_reconnect_cursor(id, meta.is_replay);
            }
        }
    }
    if let Some(drain) = control_drain {
        crate::app::root::dispatch::note_peek_page_flip(app, parent_id, drain.page_flip_entry);
        app.pending_effects.extend(drain.effects);
    }
    if let Some(handoff) = spawned_control_handoff {
        app.screen_mode_control_handoffs.remove(&handoff.session_id);
    }
    if let Some(outcome) = terminal_outcome {
        return app.apply_terminal_outcome(outcome, parent_id, is_active);
    }
    (changed || control_resolved) && is_active
}
/// Handle an Grow session notification that targets a child (subagent) session.
///
/// Events like compaction, retry, and memory flush are emitted by the child's
/// `acp_session` with the *child's* `session_id`. This routes them to the
/// correct child view and updates `SubagentInfo` where appropriate.
pub(super) fn handle_child_session_notification(
    update: GrowSessionUpdate,
    child_sid: &str,
    event_id: Option<String>,
    is_replay: bool,
    agent: &mut AgentView,
) -> bool {
    let changed = match update {
        GrowSessionUpdate::UiNotice(output) => {
            agent
                .subagent_views
                .get_mut(child_sid)
                .is_some_and(|child| {
                    apply_ui_notice(&mut child.scrollback, output, event_id, is_replay)
                })
        }
        GrowSessionUpdate::ControlStateUpdate(update) => agent
            .subagent_views
            .get_mut(child_sid)
            .is_some_and(|child| apply_control_state_update(child, update, event_id, is_replay)),
        GrowSessionUpdate::ModelChanged {
            model_id,
            reasoning_effort,
        } => agent
            .subagent_views
            .get_mut(child_sid)
            .and_then(|child| apply_model_changed(child, child_sid, model_id, reasoning_effort))
            .unwrap_or(false),
        GrowSessionUpdate::AgentChanged { agent_name } => agent
            .subagent_views
            .get_mut(child_sid)
            .is_some_and(|child| {
                if child.session.agent_control_pending() {
                    if child.session.resolve_agent_control(&agent_name) {
                        child.session.apply_agent_name(Some(agent_name))
                    } else {
                        child.session.defer_authoritative_agent_change(agent_name);
                        false
                    }
                } else {
                    child.session.apply_agent_name(Some(agent_name))
                }
            }),
        GrowSessionUpdate::InteractionResolved { tool_call_id } => {
            // Permission prompts from every child are centralized on the
            // owning primary task so they cannot time out invisibly. Other
            // child interactions keep their concrete fullscreen ownership.
            agent.dismiss_resolved_interaction(child_sid, &tool_call_id)
                || agent
                    .subagent_views
                    .get_mut(child_sid)
                    .is_some_and(|child| {
                        child.dismiss_resolved_interaction(child_sid, &tool_call_id)
                    })
        }
        GrowSessionUpdate::AutoCompactStarted { .. }
        | GrowSessionUpdate::AutoCompactCompleted { .. }
        | GrowSessionUpdate::AutoCompactFailed { .. }
        | GrowSessionUpdate::AutoCompactCancelled { .. }
        | GrowSessionUpdate::RetryState(_) => {
            let compact_tokens = match &update {
                GrowSessionUpdate::AutoCompactCompleted { tokens_after, .. } => Some(*tokens_after),
                _ => None,
            };
            let mut changed = false;
            if let Some(child_view) = agent.subagent_views.get_mut(child_sid) {
                changed = apply_session_event(
                    &update,
                    &mut child_view.session,
                    &mut child_view.scrollback,
                );
                if let Some(tokens_after) = compact_tokens {
                    refresh_context_used(child_view, tokens_after);
                }
            }
            if let Some(tokens_after) = compact_tokens
                && let Some(info) = agent.session.subagent_sessions.get_mut(child_sid)
            {
                info.tokens_used = Some(tokens_after);
                if let Some(cw) = info.context_window_tokens.filter(|&cw| cw > 0) {
                    info.context_usage_pct =
                        Some(token_estimation::usage_percentage_u8(tokens_after, cw));
                }
            }
            changed
        }
        ref update @ (GrowSessionUpdate::MemoryFlushCompleted { .. }
        | GrowSessionUpdate::MemoryDreamCompleted { .. }
        | GrowSessionUpdate::MemorySessionSaved { .. }) => {
            if let Some(child_view) = agent.subagent_views.get_mut(child_sid) {
                apply_session_event(update, &mut child_view.session, &mut child_view.scrollback)
            } else {
                false
            }
        }
        _ => false,
    };
    sync_child_control_projection(agent, child_sid);
    changed
}

pub(crate) fn sync_child_control_projection(agent: &mut AgentView, child_sid: &str) -> bool {
    let Some(child) = agent.subagent_views.get(child_sid) else {
        return false;
    };
    let model = child
        .session
        .models
        .current
        .as_ref()
        .map(|id| Arc::<str>::from(id.0.to_string()));
    let agent_name = child.session.agent_name().map(Arc::<str>::from);
    let Some(info) = agent.session.subagent_sessions.get_mut(child_sid) else {
        return false;
    };
    let changed = info.model != model
        || agent_name
            .as_ref()
            .is_some_and(|name| name != &info.subagent_type);
    info.model = model;
    if let Some(agent_name) = agent_name {
        info.subagent_type = agent_name;
    }
    changed
}

fn apply_control_state_update(
    agent: &mut AgentView,
    update: shell::extensions::notification::ControlStateUpdate,
    event_id: Option<String>,
    is_replay: bool,
) -> bool {
    use shell::extensions::notification::{ControlPhase, ControlTarget};
    let phase = update.phase;
    let epoch = update.epoch.clone();
    let revision = update.revision;
    let domain = update.domain;
    let current = update.current.clone();
    let desired = update.desired.clone();
    let intent = update.intent.clone();
    let receipt_only = update.receipt_only;
    let terminal_message = update.message.clone();
    let receipt_valid = current.domain() == domain
        && desired
            .as_ref()
            .is_some_and(|target| target.domain() == domain)
        && intent.is_some()
        && matches!(
            phase,
            ControlPhase::Applied | ControlPhase::Rejected | ControlPhase::Superseded
        );
    let apply_outcome = if receipt_only {
        if receipt_valid {
            crate::app::session::ShellControlApplyOutcome::Accepted { changed: false }
        } else {
            crate::app::session::ShellControlApplyOutcome::Rejected
        }
    } else {
        agent
            .session
            .apply_shell_control_state_outcome(update, agent.session.loading_replay)
    };
    if !apply_outcome.accepted() {
        return false;
    }
    let intent_resolved = agent.session.resolve_reconnect_control_projection(
        domain,
        phase,
        &current,
        desired.as_ref(),
        intent.as_ref(),
    );
    let state_changed = apply_outcome.changed() || intent_resolved;
    if phase == ControlPhase::Superseded || terminal_message.is_none() {
        return state_changed;
    }
    let terminal = matches!(phase, ControlPhase::Applied | ControlPhase::Rejected);
    if !terminal {
        return state_changed;
    }
    // A replayed terminal is historical control state, not a new action in
    // this TUI process. Hydrate the authoritative projection silently unless
    // it settles an exact local intent preserved across reconnect or a screen
    // mode relaunch. Without this gate every clean resume appends the last
    // model/effort/Agent success below the transcript even though the user did
    // nothing.
    if is_replay && !intent_resolved {
        return state_changed;
    }
    let target = desired.as_ref();
    let details = (phase == ControlPhase::Rejected).then(|| match target {
        Some(ControlTarget::Sampling { .. }) => {
            "Choose another /model or /effort target and retry.".to_string()
        }
        Some(ControlTarget::Agent { .. }) => {
            "Retry /agent after checking that the Agent is still available.".to_string()
        }
        Some(ControlTarget::Behavior { .. }) => {
            "Resolve the reported ownership or confirmation condition, then select the Behavior again."
                .to_string()
        }
        None => "Retry the control after the session returns to a stable boundary.".to_string(),
    });
    let tone = if phase == ControlPhase::Applied {
        NoticeTone::Success
    } else {
        NoticeTone::Error
    };
    let fallback_domain = desired
        .as_ref()
        .map(|target| format!("{:?}", target.domain()).to_lowercase())
        .unwrap_or_else(|| "unknown".to_string());
    let before = agent.scrollback.len();
    agent.scrollback.push_block(RenderBlock::terminal_notice(
        event_id.unwrap_or_else(|| format!("control:{epoch}:{fallback_domain}:{revision}")),
        tone,
        NoticeCategory::Control,
        terminal_message.expect("checked above"),
        details,
    ));
    state_changed || agent.scrollback.len() != before
}

fn apply_model_changed(
    agent: &mut AgentView,
    session_id: &str,
    model_id: String,
    reasoning_effort: Option<String>,
) -> Option<bool> {
    use shell::sampling::types::ReasoningEffort;
    let new_model_id = shell::agent::models::ModelId::new(model_id.clone());
    let effort = reasoning_effort
        .as_deref()
        .and_then(|value| value.parse::<ReasoningEffort>().ok());
    if agent.session.sampling_control_pending() {
        if !agent
            .session
            .resolve_sampling_control(&new_model_id, effort)
        {
            tracing::debug!(
                session_id,
                model_id = %model_id,
                "deferring non-matching ModelChanged behind local Sampling intent"
            );
            agent
                .session
                .defer_authoritative_model_change(model_id, reasoning_effort);
            return Some(false);
        }
    }
    if !agent.session.models.available.contains_key(&new_model_id) {
        tracing::warn!(
            session_id,
            model_id = %model_id,
            "holding ModelChanged broadcast until the local catalog catches up"
        );
        agent
            .session
            .defer_authoritative_model_change(model_id, reasoning_effort);
        return Some(false);
    }
    agent.session.clear_deferred_authoritative_model_change();
    let previous_model = agent.session.models.current.clone();
    let previous_effort = agent.session.models.reasoning_effort;
    agent
        .session
        .models
        .set_current(new_model_id.clone(), effort);
    agent.session.user_model_preference = Some(new_model_id.clone());
    let applied_effort = agent.session.models.reasoning_effort;
    let changed =
        previous_model.as_ref() != Some(&new_model_id) || previous_effort != applied_effort;
    if changed {
        tracing::info!(
            session_id,
            model_id = %model_id,
            effort = ?applied_effort,
            "ModelChanged broadcast applied (remote switch)"
        );
    }
    Some(changed)
}

/// Apply the latest server-authoritative model and Agent states that arrived
/// while this client had a matching local control in flight. Each domain is
/// released independently so an Agent preparation cannot delay a committed
/// model update (or vice versa).
pub(crate) fn apply_deferred_authoritative_controls(
    agent: &mut AgentView,
    session_id: &str,
) -> bool {
    let (model_change, agent_change) = agent.session.take_deferred_authoritative_controls();
    let model_changed = model_change
        .and_then(|(model_id, reasoning_effort)| {
            apply_model_changed(agent, session_id, model_id, reasoning_effort)
        })
        .unwrap_or(false);
    let agent_changed =
        agent_change.is_some_and(|agent_name| agent.session.apply_agent_name(Some(agent_name)));
    model_changed || agent_changed
}
/// Apply a compaction or retry event to a session's activity state and scrollback.
///
/// Shared between the root agent and child (subagent) notification paths.
pub(super) fn apply_session_event(
    update: &GrowSessionUpdate,
    session: &mut AgentSession,
    scrollback: &mut crate::scrollback::state::ScrollbackState,
) -> bool {
    match update {
        GrowSessionUpdate::AutoCompactStarted { percentage, .. } => {
            tracing::info!("Auto-compact started: {percentage}% context used");
            if session.compact_held_prompt.is_none() {
                session.compact_held_prompt = session.in_flight_prompt.clone();
            }
            session.in_flight_prompt = None;
            session.set_compaction_activity(Some(TurnActivity::AutoCompacting));
            session.set_live_feedback(
                "compaction",
                crate::scrollback::blocks::NoticeTone::Progress,
                format!("Compacting context ({percentage}%)\u{2026}"),
            );
            true
        }
        GrowSessionUpdate::AutoCompactCompleted {
            tokens_before,
            tokens_after,
            elapsed_ms,
            async_compact,
            ..
        } => {
            if *async_compact {
                scrollback.push_block(RenderBlock::notice(format!(
                    "async compact applied · {tokens_before} → {tokens_after} tokens"
                )));
                return true;
            }
            tracing::info!("Auto-compact completed: {tokens_after} tokens after");
            session.set_compaction_activity(None);
            session.clear_live_feedback("compaction");
            session.compact_held_prompt = None;
            if session.loading_replay {
                scrollback.push_block(RenderBlock::session_event(
                    SessionEvent::CompactionCompleted {
                        tokens_before: *tokens_before,
                        tokens_after: *tokens_after,
                        elapsed_ms: *elapsed_ms,
                    },
                ));
            } else {
                session.defer_compaction(*tokens_before, *tokens_after, *elapsed_ms);
            }
            true
        }
        GrowSessionUpdate::AutoCompactFailed { error } => {
            tracing::error!(error = %error, "Auto-compaction failed");
            session.set_compaction_activity(None);
            session.clear_live_feedback("compaction");
            scrollback.push_block(RenderBlock::session_event(SessionEvent::CompactionFailed {
                error: error.clone(),
            }));
            true
        }
        GrowSessionUpdate::AutoCompactCancelled { .. } => {
            tracing::info!("Auto-compact cancelled");
            session.set_compaction_activity(None);
            session.clear_live_feedback("compaction");
            session.compact_held_prompt = None;
            scrollback.push_block(RenderBlock::session_event(
                SessionEvent::CompactionCancelled,
            ));
            true
        }
        GrowSessionUpdate::RetryState(retry) => {
            tracing::debug!("Retry state: {retry:?}");
            apply_retry_state(retry, session, scrollback);
            true
        }
        GrowSessionUpdate::ImageDropped { notes } => {
            let message = notes.join("\n");
            tracing::info!("Image dropped: {message}");
            scrollback.push_block(RenderBlock::notice(message));
            true
        }
        GrowSessionUpdate::ImageProjected { notes } => {
            let message = notes.join("\n");
            tracing::info!("Image projected: {message}");
            scrollback.push_block(RenderBlock::notice(message));
            true
        }
        _ => false,
    }
}
/// True if the trailing run of session/system blocks contains a
/// [`SessionEvent::CompactionFailed`]. Used so we don't stack a [`SessionEvent::ContextTooLarge`]
/// prompt on top of the compaction handler's "too large to compact" message.
pub(super) fn scrollback_has_recent_compaction_failed(
    scrollback: &crate::scrollback::state::ScrollbackState,
) -> bool {
    use crate::scrollback::block::RenderBlock;
    for idx in (0..scrollback.len()).rev() {
        match scrollback.entry(idx).map(|e| &e.block) {
            Some(RenderBlock::SessionEvent(ev)) => {
                if matches!(ev.event, SessionEvent::CompactionFailed { .. }) {
                    return true;
                }
            }
            Some(RenderBlock::Notice(_)) => {}
            _ => break,
        }
    }
    false
}
/// Handle an `ImageCompressed` notification. A successful compression is
/// deliberately invisible in the TUI (log-only): it needs no user action,
/// and the model-facing `<image_compression_notice>` reminder is attached
/// to the prompt independently. Only the re-encode *fallback* — the
/// oversized original was KEPT — surfaces, as a persistent scrollback
/// warning (and is re-materialized on session replay).
pub(super) fn apply_image_compressed(
    agent: &mut AgentView,
    images: &[shell::extensions::notification::ImageCompressedEntry],
    message: &str,
) -> bool {
    if images.is_empty() {
        tracing::warn!("Image re-encode fallback: {message}");
        agent
            .scrollback
            .push_block(RenderBlock::notice(message.to_owned()));
        return true;
    }
    tracing::info!("Image compressed: {message}");
    false
}
pub(super) fn apply_retry_state(
    retry: &shell::extensions::notification::RetryState,
    session: &mut AgentSession,
    scrollback: &mut crate::scrollback::state::ScrollbackState,
) {
    use shell::extensions::notification::RetryState;
    match retry {
        RetryState::Retrying {
            attempt,
            max_retries,
            reason,
        } => {
            session.set_retry_activity(Some(TurnActivity::Retrying {
                attempt: *attempt,
                max_retries: *max_retries,
                reason: reason.clone(),
            }));
        }
        RetryState::Exhausted {
            attempts,
            reason,
            is_rate_limited: rate_limited,
        } => {
            session.set_retry_activity(None);
            session.rate_limited = *rate_limited;
            if *rate_limited {
                diagnostics::session_ctx::log_event(diagnostics::events::RateLimitHit {
                    model_id: session
                        .models
                        .current
                        .as_ref()
                        .map(|m| m.0.to_string())
                        .unwrap_or_default(),
                    attempts: *attempts,
                });
            }
            let error = if *rate_limited {
                crate::app::root::effects::sanitize_user_error(&format_rate_limited_user_message(
                    Some(reason.as_str()),
                ))
            } else {
                format!("failed after {attempts} retries: {reason}")
            };
            scrollback.push_block(RenderBlock::session_event(SessionEvent::RetryFailed {
                error,
                error_type: None,
            }));
        }
        RetryState::Failed {
            error_type,
            message,
        } => {
            session.set_retry_activity(None);
            if error_type == "encrypted_content_mismatch" {
                session.model_incompatible = true;
            }
            if error_type == "context_length" {
                if !scrollback_has_recent_compaction_failed(scrollback) {
                    scrollback
                        .push_block(RenderBlock::session_event(SessionEvent::ContextTooLarge));
                }
            } else {
                scrollback.push_block(RenderBlock::session_event(SessionEvent::RetryFailed {
                    error: message.clone(),
                    error_type: Some(error_type.clone()),
                }));
            }
        }
    }
    session.in_flight_prompt = None;
}
/// Single source of truth for Behavior state on the pager side.
///
/// The shell emits `CurrentModeUpdate` for every Behavior transition. Tools
/// never form a second Behavior state-transition channel.
///
/// Do not be tempted to infer mode from tool-call titles: titles incorporate
/// raw model/user input (Grep pattern, Bash command, search query, ...), so
/// a substring match silently bricks sessions whenever any tool happens to
/// mention a Behavior control tool.
///
/// Returns `true` when a `CurrentModeUpdate` was processed so the
/// caller can refresh open settings modals after the per-agent borrow
/// releases.
pub(super) fn detect_plan_mode_change(update: &acp::SessionUpdate, agent: &mut AgentView) -> bool {
    use tools::types::BehaviorId;
    let acp::SessionUpdate::CurrentModeUpdate(cmu) = update else {
        return false;
    };
    let Some(mode) = BehaviorId::try_from_id(cmu.current_mode_id.0.as_ref()) else {
        tracing::warn!(
            mode_id = %cmu.current_mode_id.0,
            "ignoring CurrentModeUpdate with unknown Behavior id"
        );
        return false;
    };
    if !matches!(
        mode,
        BehaviorId::Normal
            | BehaviorId::Clarify
            | BehaviorId::Plan
            | BehaviorId::Workflow
            | BehaviorId::Goal
    ) {
        tracing::warn!(
            mode_id = %cmu.current_mode_id.0,
            "ignoring CurrentModeUpdate for an unsupported pager Behavior"
        );
        return false;
    }
    let previous = agent.session.behavior_mode;
    if previous != mode {
        agent.clear_behavior_switch_confirmation();
    }
    agent.session.behavior_mode = mode;
    if mode != BehaviorId::Workflow {
        agent.show_workflows = false;
    }
    agent.session.plan_phase = cmu
        .meta
        .as_ref()
        .and_then(|meta| meta.get("grow/planPhase"))
        .and_then(|phase| phase.as_str())
        .map(str::to_owned);
    if let Some(change) = cmu
        .meta
        .as_ref()
        .and_then(|meta| meta.get("grow/behaviorChange"))
    {
        let status = change.get("status").and_then(serde_json::Value::as_str);
        if status == Some("confirmation_required") {
            if let Some(target) = change
                .get("target")
                .and_then(serde_json::Value::as_str)
                .and_then(BehaviorId::try_from_id)
            {
                let remaining_ms = change
                    .get("remainingMs")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(1);
                agent.show_behavior_switch_warning(target, remaining_ms);
            }
        } else if matches!(status, Some("applied" | "rejected")) {
            agent.clear_behavior_switch_confirmation();
        }
    }
    let was_active = agent.session.plan_mode_active;
    let now_active = mode.is_plan();
    agent.session.plan_mode_active = now_active;
    if previous != mode || was_active != now_active {
        tracing::info!(
            mode_id = %cmu.current_mode_id.0,
            plan_active = now_active,
            behavior = mode.as_id(),
            "Behavior state updated (from CurrentModeUpdate)"
        );
    }
    true
}

/// Whether this authoritative mode update completed a Behavior selection.
/// Confirmation and rejection responses keep the Shell's selection unchanged
/// and must not release locally-held prompts. Plain updates and explicit
/// `applied` responses may release the prompt queue after the new identity is
/// installed by [`detect_plan_mode_change`].
pub(crate) fn behavior_mode_update_resolution(
    update: &acp::SessionUpdate,
) -> Option<crate::app::session::BehaviorControlResolution> {
    let acp::SessionUpdate::CurrentModeUpdate(cmu) = update else {
        return None;
    };
    let Some(mode) = tools::types::BehaviorId::try_from_id(cmu.current_mode_id.0.as_ref()) else {
        return None;
    };
    if !matches!(
        mode,
        tools::types::BehaviorId::Normal
            | tools::types::BehaviorId::Clarify
            | tools::types::BehaviorId::Plan
            | tools::types::BehaviorId::Workflow
            | tools::types::BehaviorId::Goal
    ) {
        return None;
    }
    match cmu
        .meta
        .as_ref()
        .and_then(|meta| meta.get("grow/behaviorChange"))
        .and_then(|change| change.get("status"))
        .and_then(serde_json::Value::as_str)
    {
        None | Some("applied") => Some(crate::app::session::BehaviorControlResolution::Applied),
        Some("confirmation_required") => {
            Some(crate::app::session::BehaviorControlResolution::ConfirmationRequired)
        }
        Some("rejected") => Some(crate::app::session::BehaviorControlResolution::Rejected),
        Some(_) => None,
    }
}

/// The target named by an explicit rejected/confirmation outcome. Applied
/// updates correlate through their current mode; terminal non-applied outcomes
/// must name their target so a delayed older result cannot release admission.
pub(crate) fn behavior_mode_update_target(
    update: &acp::SessionUpdate,
) -> Option<tools::types::BehaviorId> {
    let acp::SessionUpdate::CurrentModeUpdate(cmu) = update else {
        return None;
    };
    cmu.meta
        .as_ref()
        .and_then(|meta| meta.get("grow/behaviorChange"))
        .and_then(|change| change.get("target"))
        .and_then(serde_json::Value::as_str)
        .and_then(tools::types::BehaviorId::try_from_id)
}
