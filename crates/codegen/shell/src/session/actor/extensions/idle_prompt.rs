//! Debounced `idle_prompt` notification.

use std::cell::Cell;
use std::sync::Weak;
use std::time::Duration;

use super::super::*;

/// Default `idle_prompt` debounce (60s of user inactivity).
const DEFAULT_IDLE_NOTIFICATION_DELAY: Duration = Duration::from_secs(60);

fn idle_notification_delay() -> Duration {
    resolve_idle_notification_delay(std::env::var("GROW_IDLE_NOTIFICATION_DELAY_MS").ok())
}

fn resolve_idle_notification_delay(raw: Option<String>) -> Duration {
    raw.and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_IDLE_NOTIFICATION_DELAY)
}

/// Fires `idle_prompt` after a completed turn remains idle for the debounce.
///
/// The session actor owns lifecycle detection. This object owns only the timer
/// and completion state, so no generic registry or host abstraction is needed.
pub(crate) struct IdlePromptExtension {
    actor: Weak<SessionActor>,
    timer: TaskSlot<()>,
    last_turn_completed: Cell<bool>,
}

impl IdlePromptExtension {
    pub(crate) fn new(actor: Weak<SessionActor>) -> Self {
        Self {
            actor,
            timer: TaskSlot::new(),
            last_turn_completed: Cell::new(false),
        }
    }

    pub(crate) fn on_turn_start(&self) {
        self.timer.cancel();
    }

    pub(crate) fn on_turn_done(&self) {
        self.last_turn_completed.set(true);
    }

    pub(crate) fn on_turn_failed(&self) {
        self.last_turn_completed.set(false);
    }

    pub(crate) async fn shutdown(&self) -> Result<(), tokio::task::JoinError> {
        self.last_turn_completed.set(false);
        self.timer.abort_and_join().await
    }

    pub(crate) fn on_session_idle(&self) {
        if !self.last_turn_completed.get() {
            return;
        }

        let actor = self.actor.clone();
        let delay = idle_notification_delay();
        let handle = tokio::task::spawn_local(async move {
            tokio::time::sleep(delay).await;
            let Some(actor) = actor.upgrade() else {
                return;
            };
            actor
                .dispatch_notification_hook(
                    "idle_prompt",
                    Some("Turn complete".into()),
                    None,
                    Some("info".into()),
                )
                .await;
        });
        self.timer.arm(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_defaults_to_sixty_seconds() {
        assert_eq!(
            resolve_idle_notification_delay(None),
            DEFAULT_IDLE_NOTIFICATION_DELAY
        );
    }

    #[test]
    fn delay_override_is_milliseconds() {
        assert_eq!(
            resolve_idle_notification_delay(Some("250".into())),
            Duration::from_millis(250)
        );
    }

    #[test]
    fn malformed_delay_uses_default() {
        assert_eq!(
            resolve_idle_notification_delay(Some("not-a-number".into())),
            DEFAULT_IDLE_NOTIFICATION_DELAY
        );
    }
}
