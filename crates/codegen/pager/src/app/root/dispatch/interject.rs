//! Mid-turn interjection dispatch:
//! `grow/steer` effect, and prompt-history recording. Split out of
//! `dispatch.rs` verbatim (pure code motion).

use crate::app::actions::Effect;
use crate::app::agent_view::AgentView;
use crate::app::root::{ActiveView, AppView};

/// Send a mid-turn interjection. Records the text in prompt history and fires
/// `grow/steer`. The durable consumption broadcast supplies the transcript
/// row to every attached pane.
pub(super) fn dispatch_interject(
    app: &mut AppView,
    text: String,
    images: Vec<crate::prompt_images::PastedImage>,
) -> Vec<Effect> {
    dispatch_interject_with_identity(app, text, images, None, None)
}

pub(super) fn dispatch_interject_with_identity(
    app: &mut AppView,
    text: String,
    images: Vec<crate::prompt_images::PastedImage>,
    requested_turn_id: Option<String>,
    requested_prompt_id: Option<String>,
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
    let Some(expected_turn_id) =
        requested_turn_id.or_else(|| agent.session.current_prompt_id.clone())
    else {
        agent.show_toast("No active turn to steer");
        return vec![];
    };

    record_interject_prompt_history(agent, &text);

    let interjection_id = requested_prompt_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // The composer is NOT touched here: the producer that consumed composer
    // text (the InterjectPrompt registry arm) clears it at the call site;
    // every other producer (Send now, edit-interject, plan review comments)
    // carries non-composer text and must keep the user's draft/stash.
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
pub(super) fn dispatch_steer_prompt_with_identity(
    app: &mut AppView,
    text: String,
    images: Vec<crate::prompt_images::PastedImage>,
    expected_turn_id: Option<String>,
    prompt_id: Option<String>,
) -> Vec<Effect> {
    dispatch_interject_with_identity(app, text, images, expected_turn_id, prompt_id)
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
