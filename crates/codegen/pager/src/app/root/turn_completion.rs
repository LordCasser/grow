//! Root-owned settlement after an agent turn reaches a terminal state.

use crate::app::agent_view::turn_completion::{TerminalApply, TerminalOutcome};
use crate::app::session::AgentId;
use crate::notifications::{NotificationEvent, NotificationEventKind};

use super::AppView;

/// Apply the turn-end notification + idle title escapes. Shared by both
/// rails. `queue_empty == false` suppresses the TurnComplete badge (it fires
/// only after the final queued turn) and skips the idle escapes (the next
/// turn starts immediately and would overwrite them).
impl AppView {
    pub(in crate::app) fn apply_terminal_notifications(
        &mut self,
        agent_id: AgentId,
        notification: Option<(NotificationEventKind, String)>,
        queue_empty: bool,
    ) {
        let Some((kind, body)) = notification else {
            return;
        };
        let Some(agent) = self.agents.get_mut(&agent_id) else {
            return;
        };
        let session_name = agent
            .display_name
            .as_deref()
            .or(agent.generated_session_title.as_deref());

        if queue_empty {
            let cwd_str = self.cwd.to_string_lossy();
            let model = agent.session.models.current_model_name();
            let idle_title = crate::notifications::TitleState {
                session_name,
                model: model.as_deref(),
                activity: None,
                has_pending_permissions: false,
                cwd: Some(&cwd_str),
                turn_elapsed: None,
                is_busy: false,
                focused: true,
            };
            let frame = crate::motion::FrameStamp::capture(self.motion_origin);
            self.pending_notification_escapes = self
                .notification_service
                .build_idle_escapes(&idle_title, frame);
        }

        if kind != NotificationEventKind::TurnComplete || queue_empty {
            // Defer the notification so the terminal has time to apply the idle
            // title. Ghostty debounces setTitle() by 75 ms
            // (SurfaceView_AppKit.swift:576), so we need >75 ms before the
            // notification reads self.title for the subtitle.
            let session_id = agent.session.session_id.as_ref().map(|s| s.0.to_string());
            let notif_title = session_name
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Grow".into());
            self.deferred_notification = Some((
                NotificationEvent {
                    kind,
                    title: notif_title,
                    body,
                    session_id,
                },
                std::time::Instant::now() + std::time::Duration::from_millis(100),
            ));
        }
    }
}

/// Map durable terminal finalization to redraw and notification effects.
impl AppView {
    pub(in crate::app) fn apply_terminal_outcome(
        &mut self,
        outcome: TerminalOutcome,
        agent_id: AgentId,
        is_active: bool,
    ) -> bool {
        let TerminalOutcome {
            apply,
            notification,
        } = outcome;
        match apply {
            TerminalApply::Ignored => false,
            TerminalApply::ViewerFinalized => {
                let page_flip_entry;
                let mut queue_empty = true;
                if let Some(agent) = self.agents.get_mut(&agent_id) {
                    queue_empty = agent.session.pending_prompts.is_empty();
                    let drain = super::dispatch::maybe_drain_queue(agent);
                    self.pending_effects.extend(drain.effects);
                    page_flip_entry = drain.page_flip_entry;
                } else {
                    page_flip_entry = None;
                }
                super::dispatch::note_peek_page_flip(self, agent_id, page_flip_entry);
                self.apply_terminal_notifications(agent_id, notification, queue_empty);
                let _ = is_active;
                true
            }
        }
    }
}
