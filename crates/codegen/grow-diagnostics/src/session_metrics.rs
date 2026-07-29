//! Session lifecycle event structs.
//!
//! These events are emitted into Grow's local structured diagnostic log.

use serde::Serialize;

#[derive(Serialize)]
pub struct SessionStarted {
    pub session_id: String,
}

#[derive(Serialize)]
pub struct Turn {
    pub session_id: String,
    pub turn_number: u64,
}

#[derive(Serialize)]
pub struct TurnCompletedLifecycle {
    pub session_id: String,
    pub turn_number: u64,
}

/// Doom-loop recovery acted this turn: poisoned attempts were resampled
/// and/or a response was accepted with confident signals after the budget
/// was spent. Trigger labels only — never generation content.
#[derive(Serialize)]
pub struct DoomLoopRecovery {
    pub session_id: String,
    pub turn_number: u64,
    /// Resamples this turn (doomed attempts discarded).
    pub attempts: u32,
    /// Whether the final response kept confident signals (budget spent).
    pub accepted_after_budget: bool,
    /// Tightest raw trigger label observed this turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_trigger: Option<String>,
    /// Model that produced the doomed attempts.
    pub model: String,
}

#[cfg(test)]
mod tests {

    /// The `grow-shell-doom_loop_recovery` event's name and
    /// property keys are dashboard contracts — pin them.
    #[test]
    fn doom_loop_recovery_event_shape_is_stable() {
        use crate::events::DiagnosticEvent;
        assert_eq!(super::DoomLoopRecovery::NAME, "doom_loop_recovery");
        let with_trigger = serde_json::to_value(super::DoomLoopRecovery {
            session_id: "s1".to_string(),
            turn_number: 7,
            attempts: 2,
            accepted_after_budget: true,
            top_trigger: Some("tail_repetition:4@thinking".to_string()),
            model: "grow-4.5".to_string(),
        })
        .unwrap();
        assert_eq!(
            with_trigger,
            serde_json::json!({
                "session_id": "s1",
                "turn_number": 7,
                "attempts": 2,
                "accepted_after_budget": true,
                "top_trigger": "tail_repetition:4@thinking",
                "model": "grow-4.5",
            })
        );
        let no_trigger = serde_json::to_value(super::DoomLoopRecovery {
            session_id: "s1".to_string(),
            turn_number: 7,
            attempts: 1,
            accepted_after_budget: false,
            top_trigger: None,
            model: "grow-4.5".to_string(),
        })
        .unwrap();
        assert!(no_trigger.get("top_trigger").is_none(), "None is omitted");
    }
}
