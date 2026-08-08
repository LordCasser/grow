//! Prompt-queue dispatch: the server-authoritative immediate-send routing
//! helpers, optimistic queue echoes, the local drip-feed drain
//! ([`maybe_drain_queue`]), the turn-start shim, and the queue-interject
//! action arm. Split out of `dispatch.rs` verbatim (pure code motion).

use super::ctx::with_active_agent;
use super::interject::record_interject_prompt_history;
use crate::acp::meta::user_prompt_meta;
use crate::app::actions::Effect;
use crate::app::agent::{AgentCommand, AgentId};
use crate::app::agent_view::{AgentView, PromptMode};
use crate::app::app_view::{ActiveView, AppView};
use crate::scrollback::EntryId;
use crate::scrollback::block::RenderBlock;
use agent_client_protocol as acp;
use std::time::Instant;

fn page_flip_on_send() -> bool {
    crate::appearance::cache::load_page_flip_on_send()
}

fn combine_queued_prompts_enabled() -> bool {
    crate::appearance::cache::load_combine_queued_prompts()
}

/// Whether a prompt/command submitted right now should take the
/// server-authoritative immediate-send path: the **server is busy**
/// (running a turn or still holding queued prompts), the session exists, the
/// local drip-feed queue is empty, and we're not mid-edit / model-switch /
/// replay. Kind-specific extras (e.g. the plain-prompt "no images" rule) are
/// checked by the caller.
///
/// **Server-busy — `is_turn_running() || !shared_queue.is_empty()`:** the
/// immediate-send path is for prompts that must queue server-side rather than
/// start a turn locally. It is NOT enough to check `is_turn_running()`: in
/// leader mode there is a turn-end window where this client has processed the
/// turn-end (so it is locally `Idle`, `current_prompt_id` cleared) but has not
/// yet adopted the leader's broadcast that the next prompt was promoted. In
/// that window `is_turn_running()` is false, yet the agent is busy and its
/// queue is non-empty — which this client sees as a non-empty `shared_queue`
/// mirror. Without the queue check, a prompt sent then takes the local
/// drip-feed path and is optimistically promoted to a running turn on THIS
/// client, while the leader appends it BEHIND the existing queue — it shows as
/// running here but queued on every other client (confirmed via qtrace:
/// `send_route_plain immediate=false is_turn_running=false shared_queue_len=5`
/// followed by `local_drain`). Treating a non-empty `shared_queue` as
/// server-busy routes it to the server queue, where the broadcast then drives
/// adoption consistently for all clients.
///
/// **FIFO guard — `pending_prompts.is_empty()`:** a prompt may only jump onto
/// the server queue when there is nothing ahead of it in the local drip-feed
/// queue. The two queues are merged for display/drain as *server rows first,
/// then local rows* ([`QueuePane::sync_from_merged`]), which is only correct
/// while every server-queued prompt is older than every local one. That
/// invariant breaks during the startup race: prompts typed while the session is
/// still "Starting…" go local (no session/turn yet); once the first one drains
/// and the turn starts, a newly-typed prompt would immediate-send onto the
/// server queue and render *ahead* of the still-pending older local prompt
/// (e.g. `[2, 3]` shown/run as `[3, 2]`). Requiring an empty local queue keeps
/// later prompts behind the older ones (they join the local queue and drain in
/// order), preserving FIFO.
pub(super) fn immediate_server_send_eligible(agent: &AgentView) -> bool {
    let server_busy = agent.session.state.is_turn_running() || !agent.shared_queue.is_empty();
    server_busy
        && agent.session.session_id.is_some()
        && agent.session.pending_prompts.is_empty()
        && !matches!(agent.prompt_mode, PromptMode::EditingQueued { .. })
        && !agent.session.model_switch_pending
        && !agent.session.loading_replay
}

/// Push the optimistic shared-queue echo for an immediate server-authoritative
/// send and mirror it into the owning agent so the queue pane renders it
/// immediately, before the confirming `grow/queue/changed` broadcast.
pub(super) fn push_server_queue_echo(
    app: &mut AppView,
    agent_id: AgentId,
    session_id: &str,
    prompt_id: &str,
    text: &str,
    kind: &str,
) {
    app.push_optimistic_prompt_echo(session_id, prompt_id, text, kind);
    let snapshot = app
        .shared_prompt_queue(session_id)
        .cloned()
        .unwrap_or_default();
    if let Some(agent) = app.agents.get_mut(&agent_id) {
        agent.shared_queue = snapshot;
        // Track the unconfirmed echo so a queue-row send-now against it is
        // parked until the confirming broadcast (see
        // `AgentView::send_now_awaiting_confirm`).
        agent.optimistic_queue_ids.insert(prompt_id.to_string());
    }
}

/// Retire the optimistic placeholder for a prompt that has definitively left
/// the server-authoritative queue (restored on cancel, removed, drained, or
/// otherwise resolved without becoming the running turn).
///
/// The agent's `pending_inputs` is the single source of truth for queue
/// contents and order; the only client-side queue state is the optimistic echo
/// that bridges the round-trip before the confirming `grow/queue/changed`
/// broadcast. Once a prompt's RPC resolves (or we pull it back into the input
/// on cancel) it will never reappear in a future broadcast, so its echo must be
/// dropped — otherwise the reconcile in [`AppView::apply_queue_changed`] keeps
/// re-pinning the stale row onto the END of every subsequent broadcast, which
/// both resurrects the removed prompt and scrambles the queue order.
///
/// Takes the two backing maps by `&mut` (rather than `&mut AppView`) so callers
/// can invoke it while holding a disjoint borrow of `app.agents`.
pub(super) fn retire_optimistic_echo(
    optimistic: &mut std::collections::HashMap<
        String,
        Vec<crate::app::prompt_queue::QueueEntryWire>,
    >,
    shared: &mut std::collections::HashMap<String, Vec<crate::app::prompt_queue::QueueEntryWire>>,
    session_id: &str,
    prompt_id: &str,
) {
    if let Some(opt) = optimistic.get_mut(session_id) {
        opt.retain(|e| e.id != prompt_id);
        if opt.is_empty() {
            optimistic.remove(session_id);
        }
    }
    if let Some(q) = shared.get_mut(session_id) {
        q.retain(|e| e.id != prompt_id);
        if q.is_empty() {
            shared.remove(session_id);
        }
    }
}

/// Drain prompt-side images and snapshot all chip elements (paste blocks,
/// @-file refs, image chips) into the most recently enqueued `QueuedPrompt`.
///
/// Must be called after `enqueue_prompt` / `push_back` and before
/// `prompt.set_text("")` (which clears element and image state). If the
/// last queued entry already has `wire_blocks` (skill injection), images
/// are dropped with a toast instead of merged.
pub(super) fn drain_prompt_state_to_last_queued(agent: &mut AgentView) {
    let prompt_state = agent.prompt.stash();
    let (_, images, chip_elements) = prompt_state.into_submission();

    let Some(entry) = agent.session.pending_prompts.back_mut() else {
        return;
    };

    entry.chip_elements = chip_elements;

    if images.is_empty() {
        return;
    }

    // wire_blocks policy: skill-injected prompts do not carry prompt images.
    if entry.wire_blocks.is_some() {
        agent.show_toast("Images removed (skill prompt)");
        return;
    }

    entry.images = images;
}

/// Prepend `<system-reminder>` framing to a cron prompt for the model.
///
/// Delegates to the shared implementation in `tools::reminders`.
/// The UI shows the raw `prompt` text via `RenderBlock::cron_prompt`; this
/// wrapped version is only sent to the model via `Effect::SendPrompt` so
/// the model knows the message is a scheduled task execution, not a human.
fn format_cron_prompt(prompt: &str, task_id: &str, human_schedule: &str) -> String {
    tools::reminders::format_scheduled_task_prompt(prompt, task_id, human_schedule)
}

/// Try to send the next queued entry (prompt, command, bash, or cron) if the agent is idle.
///
/// Called after enqueue operations and task completions to advance the queue.
///
/// Branches on `QueueEntryKind`:
/// - **Prompt**: pushes user prompt block to scrollback, starts turn, returns `Effect::SendPrompt`
/// - **Command**: starts command, returns the appropriate `Effect` (e.g., `Effect::Compact`)
/// - **BashCommand**: starts turn (no user block), returns `Effect::SendBashCommand`
/// - **Cron**: pushes cron prompt block to scrollback, starts turn, returns `Effect::SendPrompt`
pub(in crate::app) struct QueueDrain {
    pub(in crate::app) effects: Vec<Effect>,
    pub(in crate::app) page_flip_entry: Option<EntryId>,
}

impl QueueDrain {
    fn blocked() -> Self {
        Self {
            effects: Vec::new(),
            page_flip_entry: None,
        }
    }
}

pub(in crate::app) fn maybe_drain_queue(agent: &mut AgentView) -> QueueDrain {
    use crate::app::agent::QueueEntryKind;
    use crate::unified_log as ulog;

    let sid = agent.session.session_id.as_ref().map(|s| s.0.as_ref());
    let queue_depth = agent.session.pending_prompts.len();

    let log_blocked = |reason: &str, sid: Option<&str>| {
        if queue_depth > 0 {
            ulog::debug(
                "prompt.drain_blocked",
                sid,
                Some(serde_json::json!({"reason": reason, "queue_depth": queue_depth})),
            );
        }
    };

    if !agent.session.state.is_idle() {
        log_blocked("turn_running", sid);
        return QueueDrain::blocked();
    }
    if agent.session.current_prompt_id.is_some() {
        // A Prompt RPC has been submitted but the shell has not yet confirmed
        // queue admission or foreground ownership. Keep it separate from the
        // visual Running state and do not optimistically drain another row.
        log_blocked("prompt_submitting", sid);
        return QueueDrain::blocked();
    }
    // Hold the drain during an in-flight model switch. See the
    // `model_switch_pending` field doc for why a reconnect must clear it.
    if agent.session.model_switch_pending {
        log_blocked("model_switch_pending", sid);
        return QueueDrain::blocked();
    }
    if agent.session.loading_replay {
        log_blocked("loading_replay", sid);
        return QueueDrain::blocked();
    }
    // Server-owned next turn: a non-running server row (including this
    // client's own in-flight send-now echo) drains shell-side — the
    // `queue/changed(running_prompt_id)` adoption starts it. Draining a LOCAL
    // row now would optimistically promote it as the running turn while the
    // shell runs the server row, whose deltas then fail the prompt-id gate
    // and render nothing (the FIFO invariant documented on
    // `immediate_server_send_eligible`).
    let running = agent.session.current_prompt_id.as_deref();
    if agent
        .shared_queue
        .iter()
        .any(|e| Some(e.id.as_str()) != running)
    {
        log_blocked("server_queue_owns_next_turn", sid);
        return QueueDrain::blocked();
    }
    let Some(session_id) = agent.session.session_id.clone() else {
        log_blocked("no_session_id", None);
        return QueueDrain::blocked();
    };

    // Block drain if the user is editing the front prompt.
    if let PromptMode::EditingQueued { id, .. } = &agent.prompt_mode
        && agent
            .session
            .pending_prompts
            .front()
            .is_some_and(|p| p.id == *id)
    {
        // The prompt being edited is next to send — don't drain it
        // from under the user. The turn status line will show a
        // "waiting on your edit" indicator.
        log_blocked("user_editing_front", Some(&session_id.0));
        return QueueDrain::blocked();
    }

    // Row the user is actively editing (if any). The front-row case is already
    // handled above; pass it so a combined drain also stops before an edited
    // *follower* instead of merging it away.
    let editing_id = match &agent.prompt_mode {
        PromptMode::EditingQueued { id, .. } => Some(*id),
        _ => None,
    };
    let queued = match if combine_queued_prompts_enabled() {
        agent.session.dequeue_combined_prompt(editing_id)
    } else {
        agent.session.dequeue_prompt()
    } {
        Some(q) => q,
        None => return QueueDrain::blocked(),
    };

    // A new turn is starting: follow-up chips belong to the previous
    // response and must not linger into it.
    agent.clear_follow_ups();

    // This client is now sending its own prompt — it "takes the wheel" and is
    // no longer a passive viewer. Clearing this restores strict prompt-id gate
    // semantics (so stale chunks from a later rewind/cancel of THIS turn are
    // dropped, not adopted). See `AgentView::attached_as_viewer`.
    agent.attached_as_viewer = false;

    ulog::info(
        "prompt.drain",
        Some(&session_id.0),
        Some(serde_json::json!({
            "kind": queued.kind.as_label(),
            "remaining_in_queue": agent.session.pending_prompts.len(),
            "prompt_len": queued.text.len(),
        })),
    );
    // qtrace: a LOCAL drip-feed drain promotes this prompt to the running turn
    // client-side (renders a scrollback block + sets current_prompt_id). In
    // leader mode this is the suspected divergence point — the server may queue
    // the prompt behind others instead of running it.
    tracing::debug!(
        target: "qtrace",
        pid = std::process::id(),
        event = "local_drain",
        kind = queued.kind.as_label(),
        remaining = agent.session.pending_prompts.len(),
        shared_queue_len = agent.shared_queue.len(),
        session = session_id.0.as_ref(),
        text = %queued.text.chars().take(48).collect::<String>(),
        "draining prompt LOCALLY as a new running turn",
    );

    let agent_id = agent.session.id;

    // Track whether this turn is a bash-mode command for post-turn focus.
    agent.bash_turn = queued.kind == QueueEntryKind::BashCommand;
    agent.cron_task_id = if queued.kind == QueueEntryKind::Cron {
        queued.task_id.clone()
    } else {
        None
    };
    // Generate a fresh prompt_id for every outgoing prompt/command. This is
    // threaded through PromptRequest._meta to the agent and echoed on every
    // SessionNotification + the PromptResponse, letting us correlate
    // notifications back to the originating prompt for cancel/rewind.
    let prompt_id = uuid::Uuid::new_v4().to_string();

    // Record it as self-originated so the ACP gate treats this turn's deltas as
    // ours (drive it; drop a stale post-rewind chunk on a mismatch) rather than
    // adopting them as another client's turn. The `Cron` arm overrides
    // `prompt_id` with a `scheduler-fired-` prefix and records that id itself.
    if queued.kind != QueueEntryKind::Cron {
        agent.note_self_originated_prompt(&prompt_id);
    }

    match queued.kind {
        QueueEntryKind::Prompt => {
            // Submission is not a turn boundary. QueueChanged carrying this id
            // as `running_prompt_id` is the authority that starts the visual
            // foreground turn; a terminal response may instead resolve it as
            // queued/removed without ever showing a fake LLM response.
            agent.session.current_prompt_id = Some(prompt_id.clone());
            agent.session.state = crate::app::agent::AgentState::TurnSubmitting;
            agent.turn_started_at = Some(Instant::now());
            // Scrollback shows display text (never raw skill XML). Combined
            // drains paint one bubble per original follow-up.
            let is_skill = queued.display_as_skill;
            let multi = prompt_queue::is_combined(&queued.combined_texts);
            let (prompt_idx, prompt_entry_id, combined_entries) = if multi {
                let (first_idx, _, last_id, all_ids) =
                    paint_or_reuse_combined_user_bubbles(agent, &queued.combined_texts, &prompt_id);
                (first_idx, last_id, all_ids)
            } else {
                let block = if is_skill {
                    RenderBlock::skill_prompt(&queued.text)
                } else if !queued.skill_token_ranges.is_empty() {
                    RenderBlock::user_prompt_with_skill_tokens(
                        &queued.text,
                        queued.skill_token_ranges.clone(),
                    )
                } else {
                    RenderBlock::user_prompt(&queued.text)
                };
                let id = agent.scrollback.push_block(block);
                (agent.scrollback.len().saturating_sub(1), id, vec![id])
            };
            for entry_id in &combined_entries {
                if let Some(entry) = agent.scrollback.get_by_id_mut(*entry_id)
                    && let RenderBlock::UserPrompt(block) = &mut entry.block
                {
                    block.message_id = Some(prompt_id.clone());
                }
            }
            // Stash for cancel-with-restore. Only plain (non-skill) prompts
            // can be reversed back into the input box.
            if queued.wire_blocks.is_none() {
                let earlier = combined_entries
                    .iter()
                    .copied()
                    .filter(|id| *id != prompt_entry_id)
                    .collect();
                agent.session.in_flight_prompt = Some(crate::app::agent::InFlightPrompt {
                    text: queued.text.clone(),
                    images: queued.images.clone(),
                    scrollback_entry: prompt_entry_id,
                    combined_scrollback_entries: earlier,
                    chip_elements: queued.chip_elements.clone(),
                });
            }
            agent.turn_started_at = Some(Instant::now());
            let flip = page_flip_on_send();
            agent.scrollback.follow_new_turn(Some(prompt_idx), flip);

            let combined_segs = queued.combined_texts.clone();
            let effects = if let Some(mut blocks) = queued.wire_blocks {
                // Skill injection: send structured blocks.
                // Annotate the first text block's meta with the display text
                // so the pager can reconstruct the clean prompt on session
                // restore (replay). Without this, replay shows the raw skill
                // instructions instead of the user-facing display text.
                if let Some(acp::ContentBlock::Text(tb)) = blocks.first_mut() {
                    let map = tb.meta.get_or_insert_with(acp::Meta::new);
                    map.insert(
                        user_prompt_meta::DISPLAY_TEXT.into(),
                        serde_json::Value::String(queued.text),
                    );
                    if is_skill {
                        map.insert(
                            user_prompt_meta::DISPLAY_AS_SKILL.into(),
                            serde_json::Value::Bool(true),
                        );
                    }
                    prompt_queue::stamp_combined_display_texts(map, &combined_segs);
                } else {
                    tracing::debug!(
                        "wire_blocks[0] is not TextContent — displayText annotation skipped"
                    );
                }
                vec![Effect::SendPromptBlocks {
                    agent_id,
                    session_id,
                    blocks,
                    prompt_id,
                }]
            } else if !queued.images.is_empty() {
                // Image-bearing prompt: build text + image content blocks.
                // Pass the session cwd so orphan `[Image #N: <path>]`
                // placeholders (paste from a previous session, etc.)
                // can be recovered from disk via the shared helper.
                // Token ranges are NOT stamped here: the builder rewrites the
                // text (placeholder stripping), which would shift byte offsets.
                let mut blocks = crate::prompt_images::build_content_blocks_with_workspace(
                    queued.text,
                    queued.images,
                    Some(std::path::Path::new(&agent.session.cwd)),
                );
                if let Some(acp::ContentBlock::Text(tb)) = blocks.first_mut() {
                    let map = tb.meta.get_or_insert_with(acp::Meta::new);
                    prompt_queue::stamp_combined_display_texts(map, &combined_segs);
                }
                vec![Effect::SendPromptBlocks {
                    agent_id,
                    session_id,
                    blocks,
                    prompt_id,
                }]
            } else if multi {
                // Stamp combinedDisplayTexts so reload paints multi-bubble. No
                // skillTokenRanges: dequeue_combined_prompt clears them on every
                // combined drain (multi paints plain per-segment bubbles).
                let mut tb = acp::TextContent::new(queued.text);
                let map = tb.meta.get_or_insert_with(acp::Meta::new);
                prompt_queue::stamp_combined_display_texts(map, &combined_segs);
                vec![Effect::SendPromptBlocks {
                    agent_id,
                    session_id,
                    blocks: vec![acp::ContentBlock::Text(tb)],
                    prompt_id,
                }]
            } else {
                // Normal prompt: send text as-is.
                vec![Effect::SendPrompt {
                    agent_id,
                    session_id,
                    text: queued.text,
                    prompt_id,
                    skill_token_ranges: queued.skill_token_ranges,
                }]
            };
            QueueDrain {
                effects,
                page_flip_entry: flip.then_some(prompt_entry_id),
            }
        }
        QueueEntryKind::Command => {
            // Currently only `/compact` — future slash commands will branch here.
            agent.session.start_command(AgentCommand::Compact);
            agent.turn_started_at = Some(Instant::now());

            QueueDrain {
                effects: vec![Effect::Compact {
                    agent_id,
                    session_id,
                    user_context: queued
                        .text
                        .strip_prefix("/compact")
                        .map(str::trim)
                        .filter(|context| !context.is_empty())
                        .map(str::to_string),
                    track_foreground: true,
                }],
                page_flip_entry: None,
            }
        }
        QueueEntryKind::BashCommand => {
            // Start turn but do NOT push a user prompt block.
            // The execute block from the shell IS the visual entry.
            agent.start_turn_boundary(Some(&prompt_id));
            agent.session.current_prompt_id = Some(prompt_id.clone());
            agent.turn_started_at = Some(Instant::now());

            agent.scrollback.follow_new_turn(None, page_flip_on_send());

            QueueDrain {
                effects: vec![Effect::SendBashCommand {
                    agent_id,
                    session_id,
                    command: queued.text,
                    prompt_id,
                }],
                page_flip_entry: None,
            }
        }
        QueueEntryKind::Cron => {
            let prompt_id = format!("scheduler-fired-{prompt_id}");
            agent.note_self_originated_prompt(&prompt_id);
            agent.start_turn_boundary(Some(&prompt_id));
            agent.session.current_prompt_id = Some(prompt_id.clone());
            let prompt_entry_id = agent
                .scrollback
                .push_block(RenderBlock::cron_prompt(&queued.text));
            agent.turn_started_at = Some(Instant::now());

            let prompt_idx = agent.scrollback.len().saturating_sub(1);
            let flip = page_flip_on_send();
            agent.scrollback.follow_new_turn(Some(prompt_idx), flip);

            let framed_text = format_cron_prompt(
                &queued.text,
                queued.task_id.as_deref().unwrap_or("unknown"),
                queued.human_schedule.as_deref().unwrap_or("unknown"),
            );

            let mut meta_map = serde_json::Map::new();
            meta_map.insert(
                user_prompt_meta::DISPLAY_TEXT.into(),
                serde_json::Value::String(queued.text),
            );
            meta_map.insert(
                user_prompt_meta::DISPLAY_AS_CRON.into(),
                serde_json::Value::Bool(true),
            );
            let blocks = vec![acp::ContentBlock::Text(
                acp::TextContent::new(framed_text).meta(Some(meta_map)),
            )];

            QueueDrain {
                effects: vec![Effect::SendPromptBlocks {
                    agent_id,
                    session_id,
                    blocks,
                    prompt_id,
                }],
                page_flip_entry: flip.then_some(prompt_entry_id),
            }
        }
    }
}

/// Whether [`apply_turn_start_shim`] renders its own user block (i.e.
/// `display_block` is `Some`). When true the pager owns the block and must
/// correlate the leader's user echo by message id; when false (bash/internal
/// with no local text) the echo is the only source and must render. Kept in
/// sync with the shim's match via a `debug_assert!` there.
pub(crate) fn shim_renders_own_user_block(
    kind: &str,
    text: Option<&str>,
    _prompt_id: Option<&str>,
) -> bool {
    match kind {
        "bash" | "internal" => false,
        _ => text.is_some(),
    }
}

/// Paint one user bubble per combined segment, or reuse bubbles with the same
/// stable message identity.
///
/// Returns `(first_idx, first_id, last_id, all_segment_ids oldest→newest)`.
fn paint_or_reuse_combined_user_bubbles(
    agent: &mut AgentView,
    segments: &[String],
    message_id: &str,
) -> (
    usize,
    crate::scrollback::EntryId,
    crate::scrollback::EntryId,
    Vec<crate::scrollback::EntryId>,
) {
    let existing: Vec<_> = (0..agent.scrollback.len())
        .filter_map(|idx| {
            let entry = agent.scrollback.entry(idx)?;
            matches!(
                &entry.block,
                RenderBlock::UserPrompt(block)
                    if block.message_id.as_deref() == Some(message_id)
            )
            .then_some((idx, entry.id))
        })
        .collect();
    if existing.len() == segments.len() && !existing.is_empty() {
        let (first_idx, first_id) = existing[0];
        let (_, last_id) = existing[existing.len() - 1];
        let ids = existing.iter().map(|(_, id)| *id).collect();
        return (first_idx, first_id, last_id, ids);
    }

    let mut first_idx = None;
    let mut first_id = None;
    let mut last_id = None;
    let mut all_ids = Vec::with_capacity(segments.len());
    for seg in segments {
        let block = crate::scrollback::blocks::UserPromptBlock::new(seg.clone())
            .with_message_id(message_id);
        let id = agent.scrollback.push_block(RenderBlock::UserPrompt(block));
        all_ids.push(id);
        if first_idx.is_none() {
            first_idx = Some(agent.scrollback.len().saturating_sub(1));
            first_id = Some(id);
        }
        last_id = Some(id);
    }
    (
        first_idx.expect("segments non-empty"),
        first_id.expect("segments non-empty"),
        last_id.expect("segments non-empty"),
        all_ids,
    )
}

/// Turn-start shim for a server-authoritative prompt the leader just drained
/// into the running slot.
///
/// Mirrors the matching arm of [`maybe_drain_queue`] EXCEPT it does NOT mint a
/// `prompt_id` (it adopts the one the leader reported) and does NOT emit a send
/// `Effect` (the prompt was already sent at enqueue time). The scrollback block
/// and focus flag are branched on the adopted entry's `kind`:
///
/// - `"bash"`     — no user block (the shell's execute block IS the entry); set
///   `agent.bash_turn = true` for post-turn focus + TurnComplete suppression.
/// - `"cron"`     — render the cron text via `cron_prompt`.
/// - otherwise (plain `"prompt"`) — render the user-prompt block + stash an
///   `in_flight_prompt` for Ctrl+C rewind.
///
/// The optimistic block and ACP echo reconcile by `messageId`.
pub(crate) fn apply_turn_start_shim(
    agent: &mut AgentView,
    prompt_id: String,
    text: Option<String>,
    kind: &str,
    combined_texts: Option<Vec<String>>,
) -> Option<EntryId> {
    // Re-derive the per-turn viewer flag (see the ACP gate). This shim adopts a
    // turn the leader drained into the running slot: if THIS client originated
    // it (its own queued/immediate prompt), it drives it; otherwise it is
    // viewing a turn another client drives, so `attached_as_viewer` must flip
    // back to true even if this pane has sent prompts before (the flag is no
    // longer a one-way latch) — that drives `handle_prompt_complete` + the
    // viewer chrome correctly.
    //
    let adopted_from_other_client = !agent.is_self_originated_prompt(&prompt_id);
    tracing::debug!(
        target: "qtrace",
        pid = std::process::id(),
        event = "turn_start_shim",
        prompt_id = %prompt_id,
        kind,
        adopted_from_other_client,
        prev_current_prompt_id = agent.session.current_prompt_id.as_deref().unwrap_or(""),
        shared_queue_len = agent.shared_queue.len(),
        text = %text.as_deref().unwrap_or("").chars().take(48).collect::<String>(),
        "adopting server-driven running turn (turn-start shim)",
    );
    agent.start_turn_boundary(Some(&prompt_id));
    agent.session.current_prompt_id = Some(prompt_id.clone());
    agent.attached_as_viewer = adopted_from_other_client;
    // A new (adopted) turn is starting: drop the prior turn's chips but KEEP the
    // seen ring, so a buffer-replayed `grow/follow_ups` for an older response
    // stays rejected (no stale revival). This is correct for BOTH passive-viewer
    // and self-driven adoption: the adopted turn's OWN follow_ups still
    // re-render via the stamped `promptId` match in `apply_follow_ups` (the
    // current_prompt_id set just above), so no seen-ring un-recording is needed.
    agent.clear_follow_ups();
    // The adopted turn's follow_ups may have arrived on the ext channel BEFORE
    // this turn-start adoption (separate channels) and been buffered — render
    // them now that the turn is current.
    agent.flush_pending_follow_ups(&prompt_id);

    // Combined turn: one user bubble per original follow-up (painted below).
    let multi_segments: Option<Vec<String>> = combined_texts.filter(|v| v.len() >= 2);

    // Display block (if any) + whether Ctrl+C can restore into the composer.
    let (display_block, rewindable): (Option<RenderBlock>, bool) = match kind {
        "bash" => {
            agent.bash_turn = true;
            (None, false)
        }
        "internal" => (None, false),
        "cron" => (text.as_deref().map(RenderBlock::cron_prompt), false),
        _ if multi_segments.is_some() => (None, true),
        _ => (text.as_deref().map(RenderBlock::user_prompt), true),
    };

    debug_assert!(
        multi_segments.is_some()
            || display_block.is_some()
                == shim_renders_own_user_block(kind, text.as_deref(), Some(&prompt_id)),
        "shim_renders_own_user_block must mirror apply_turn_start_shim's display_block"
    );

    let page_flip_entry = if let Some(segments) = multi_segments {
        let (prompt_idx, first_id, last_id, all_ids) =
            paint_or_reuse_combined_user_bubbles(agent, &segments, &prompt_id);
        if rewindable {
            let restore = text
                .clone()
                .unwrap_or_else(|| prompt_queue::join_texts(segments.iter().map(String::as_str)));
            let earlier = all_ids.into_iter().filter(|id| *id != last_id).collect();
            // An adopted turn arrives with text only, never the original
            // attachments, so a Ctrl+C rewind restores just the joined text.
            // The local drain path, which owns the data, restores images/chips.
            agent.session.in_flight_prompt = Some(crate::app::agent::InFlightPrompt {
                text: restore,
                images: Vec::new(),
                scrollback_entry: last_id,
                combined_scrollback_entries: earlier,
                chip_elements: Vec::new(),
            });
        }
        let flip = page_flip_on_send();
        agent.scrollback.follow_new_turn(Some(prompt_idx), flip);
        flip.then_some(first_id)
    } else if let Some(block) = display_block {
        let already_painted = (0..agent.scrollback.len()).rev().find_map(|idx| {
            let entry = agent.scrollback.entry(idx)?;
            matches!(
                &entry.block,
                RenderBlock::UserPrompt(user)
                    if user.message_id.as_deref() == Some(prompt_id.as_str())
            )
            .then_some((idx, entry.id))
        });
        let (prompt_idx, prompt_entry_id) = if let Some(found) = already_painted {
            found
        } else {
            let id = agent.scrollback.push_block(block);
            (agent.scrollback.len().saturating_sub(1), id)
        };
        if let Some(entry) = agent.scrollback.get_by_id_mut(prompt_entry_id)
            && let RenderBlock::UserPrompt(block) = &mut entry.block
        {
            block.message_id = Some(prompt_id.clone());
        }
        if rewindable && let Some(text) = text {
            // The rewind restore must match the on-screen (possibly edited)
            // block text, not the adoption's stale mirror text.
            let restore_text = match agent.scrollback.entry(prompt_idx).map(|e| &e.block) {
                Some(RenderBlock::UserPrompt(ub)) if ub.text != text => ub.text.clone(),
                _ => text,
            };
            agent.session.in_flight_prompt = Some(crate::app::agent::InFlightPrompt {
                text: restore_text,
                images: Vec::new(),
                scrollback_entry: prompt_entry_id,
                combined_scrollback_entries: Vec::new(),
                chip_elements: Vec::new(),
            });
        }
        let flip = page_flip_on_send();
        agent.scrollback.follow_new_turn(Some(prompt_idx), flip);
        flip.then_some(prompt_entry_id)
    } else {
        // No local block to render; the ACP user-message chunk is authoritative.
        agent.scrollback.follow_new_turn(None, page_flip_on_send());
        None
    };

    agent.turn_started_at = Some(Instant::now());

    if agent.session.tracker.activity().is_some() {
        agent.session.in_flight_prompt = None;
    }
    if let Some(commands) = agent.session.tracker.take_pending_acp_commands() {
        agent.session.available_commands = commands;
        agent.session.available_commands_generation += 1;
    }
    if let Some(tools) = agent.session.tracker.take_pending_acp_tools() {
        agent.session.available_tools = Some(tools.into_iter().collect());
    }
    page_flip_entry
}

pub(crate) fn note_peek_page_flip(
    app: &mut AppView,
    agent_id: AgentId,
    page_flip_entry: Option<EntryId>,
) {
    let Some(entry_id) = page_flip_entry else {
        return;
    };
    let Some(mut dash) = app.dashboard.take() else {
        return;
    };
    dash.note_page_flip_for_lease(agent_id, entry_id, &app.agents);
    app.dashboard = Some(dash);
}

/// Drain the next queued prompt and, when that page-flips under a lease, note it.
pub(crate) fn maybe_drain_queue_and_note_peek(app: &mut AppView, agent_id: AgentId) -> Vec<Effect> {
    let drain = {
        let Some(agent) = app.agents.get_mut(&agent_id) else {
            return vec![];
        };
        maybe_drain_queue(agent)
    };
    note_peek_page_flip(app, agent_id, drain.page_flip_entry);
    drain.effects
}

/// Try to drain the next queued prompt (triggered after editing completes).
pub(super) fn dispatch_drain_queue(app: &mut AppView) -> Vec<Effect> {
    if app.reconnect_pending {
        return vec![];
    }
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    maybe_drain_queue_and_note_peek(app, id)
}

/// `Action::QueueInterjectShared` arm: map the (possibly edited) queue
/// interject to a fire-and-forget effect scoped to the active agent's
/// session.
pub(super) fn dispatch_queue_interject_shared(
    app: &mut AppView,
    id: String,
    expected_version: u64,
    new_text: Option<String>,
) -> Vec<Effect> {
    let route = app.active_agent().and_then(|agent| {
        Some((
            agent.session.session_id.clone()?,
            agent.session.current_prompt_id.clone()?,
        ))
    });
    match route {
        Some((session_id, expected_turn_id)) => {
            with_active_agent(app, |agent| {
                // Edited override is user-typed text — keep it Ctrl+R recallable.
                if let Some(text) = &new_text {
                    record_interject_prompt_history(agent, text);
                }
            });
            vec![Effect::QueueInterject {
                session_id,
                expected_turn_id,
                id,
                expected_version,
                new_text,
            }]
        }
        None => vec![],
    }
}
