//! Synchronous state dispatch: [`Action`](crate::app::actions::Action) → state mutations + [`Effect`](crate::app::actions::Effect)s.
//!
//! This is the core business logic of the application.  It takes an action,
//! mutates application state, and returns a list of async effects to execute.
//!
//! **Invariants:**
//! - This module never touches the terminal, network, or filesystem.
//! - All mutations are synchronous and deterministic.
//! - Async work is described as [`Effect`](crate::app::actions::Effect) values, not executed.
//! - This makes dispatch fully testable without tokio or a terminal.
//!
//! Imports in this tree use at most one `super::` hop (absolute `crate::` paths
//! otherwise); tests/ shares a fixture prelude via `use super::*;`.

mod cta;
mod ctx;
mod dashboard;
mod dashboard_diagnostics;
pub(crate) mod external_editor;
mod interject;
mod jump;
mod modes;
mod notes;
mod permissions;
mod prompt;
mod queue;
mod rewind;
mod router;
mod session;
mod settings;
mod status;
mod task_result;
mod transcript;
mod turn;

pub(crate) use modes::{downgrade_displayed_auto_if_gated, inherit_permission_mode};
pub(crate) use notes::{recap_unavailable_toast, scrollback_has_user_messages};
pub(crate) use permissions::drain_root_permission_queue;
pub(crate) use permissions::{resolve_permission_queue_transition, respond_permission};
pub(crate) use prompt::dispatch_initial_prompt;
pub(in crate::app) use prompt::{show_small_screen_tip, show_ssh_wrap_tip};
pub(crate) use queue::{
    apply_turn_start_shim, maybe_drain_queue, next_control_effect, note_peek_page_flip,
};
pub(in crate::app) use rewind::{find_user_prompt_entry_for_shell_index, shell_prompt_index_at};
pub(crate) use router::dispatch;
pub(crate) use session::load::reconcile_controls_after_reconnect;
pub(crate) use settings::ui::refresh_open_settings_modals;
pub(crate) use status::commit_minimal_update_notice;
pub(crate) use turn::{next_prompt_watchdog_deadline, poll_stalled_prompt_submissions};

// Test-only consumers (cfg(test) mods elsewhere in the crate); a plain
// re-export trips -D unused-imports in the lib build.
#[cfg(test)]
pub(crate) use ctx::{SwitchCause, switch_to_agent};
#[cfg(test)]
pub(crate) use settings::ui::{ROLLBACK_NO_ARM_TOAST, build_pager_snapshot};
#[cfg(test)]
pub(crate) use turn::PROMPT_STATUS_RUNNING_WATCHDOG_DELAY;
#[cfg(test)]
pub(crate) use turn::PROMPT_STATUS_WATCHDOG_DELAY;

#[cfg(test)]
mod tests;
