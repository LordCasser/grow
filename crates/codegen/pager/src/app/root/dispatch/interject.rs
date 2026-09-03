//! Mid-turn interjection dispatch: optimistic local echo, the
//! `grow/steer` effect, and prompt-history recording. Split out of
//! `dispatch.rs` verbatim (pure code motion).

use crate::app::actions::Effect;
use crate::app::agent_view::AgentView;
use crate::app::root::{ActiveView, AppView};
use crate::scrollback::block::RenderBlock;

/// Send a mid-turn interjection. Pushes a standard user prompt block locally
/// for instant feedback, records the text in prompt history, clears the
/// prompt, and fires the `grow/steer` ext method carrying a client-minted
/// id.
///
/// The shell broadcasts `grow/session/interjection` to every attached pane so
/// other clients viewing the same session render it too (multi-client /
/// dashboard mode). Our own broadcast echoes back carrying the same id; the id
/// is recorded in `self_interjection_ids` so `handle_interjection` drops the
/// echo instead of rendering a duplicate. Other panes lack the id and render
/// it. (Optimistic-echo + reconcile-by-id, mirroring the shared prompt queue.)
pub(super) fn dispatch_interject(
    app: &mut AppView,
    text: String,
    images: Vec<crate::prompt_images::PastedImage>,
) -> Vec<Effect> {
    // Hard-reset only — `text` may not be from the composer.
    let ActiveView::Agent(id) = app.active_view else {
        return vec![];
    };
    let Some(agent) = app.agents.get_mut(&id) else {
        return vec![];
    };

    // Submitting an interjection retires any edit-contextual ephemeral tip —
    // even when there is no active session, matching the prompt/bash/
    // feedback/remember paths.
    agent.ephemeral_tip.clear_on_submit();

    let Some(session_id) = agent.session.session_id.clone() else {
        agent.show_toast("No active session");
        return vec![];
    };
    let Some(expected_turn_id) = agent.session.current_prompt_id.clone() else {
        agent.show_toast("No active turn to steer");
        return vec![];
    };

    record_interject_prompt_history(agent, &text);

    // Push a standard user prompt block locally for instant feedback, and
    // record its id so the broadcast echo (`grow/session/interjection`) is
    // deduped instead of rendering a second copy on this pane.
    let interjection_id = uuid::Uuid::new_v4().to_string();
    agent
        .session
        .remember_self_interjection(interjection_id.clone());
    agent
        .scrollback
        .push_block(RenderBlock::interjection_prompt(&text));

    // The composer is NOT touched here: the producer that consumed composer
    // text (the InterjectPrompt registry arm) clears it at the call site;
    // every other producer (Send now, edit-interject, plan review comments)
    // carries non-composer text and must keep the user's draft/stash.
    agent.show_toast("Interjection sent");

    vec![Effect::SendInterject {
        agent_id: id,
        session_id,
        expected_turn_id,
        text,
        interjection_id,
        blocks: None,
        images,
    }]
}

/// User-facing "Send now" is steering, never cancel-and-send.
pub(super) fn dispatch_steer_prompt(
    app: &mut AppView,
    text: String,
    images: Vec<crate::prompt_images::PastedImage>,
) -> Vec<Effect> {
    dispatch_interject(app, text, images)
}

/// Record an interjection in prompt history (Ctrl+R finds interjections).
/// Shared by `dispatch_interject` and the edited-queued-interject arm — the
/// user typed both, so both must be recallable.
pub(super) fn record_interject_prompt_history(agent: &mut AgentView, text: &str) {
    let trimmed_key = text.trim().to_string();
    if trimmed_key.is_empty() {
        return;
    }
    agent
        .session
        .prompt_history
        .retain(|p| p.trim() != trimmed_key);
    agent.session.prompt_history.insert(0, text.to_string());
    if agent.session.prompt_history.len() > 200 {
        agent.session.prompt_history.truncate(200);
    }
}
