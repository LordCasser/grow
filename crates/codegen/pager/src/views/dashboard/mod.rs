//! Agent Dashboard — top-level overview of every session in flight.
//!
//! Centralized overview of top-level sessions and their subagents, with
//! stable session identity, read-only preview, attach, and new-session
//! composition. Open order is the default; state and directory groupings are
//! optional overview arrangements.
//!
//! Owned by `AppView::dashboard` (`Option<DashboardState>`); active only when
//! `app.active_view == ActiveView::AgentDashboard`. State survives the user
//! closing and reopening the dashboard within a single pager process (the
//! `Option` is reset only on shutdown).
//!
//! ## Module layout
//!
//! - [`state`] — public `DashboardState`, `DashboardRowId`, `RowState`,
//!   `Grouping`, `Filter`, `FilterValue`, `PersistedDashboard`.
//! - [`row`] — `DashboardRow`, `build_rows()`, classifiers, sort.
//! - [`layout`] — pure rect computation.
//! - [`render`] — `Widget`-style rendering routine.
//! - [`peek`] — read-only preview fields and rendering.
//!
//! ## Lifetime
//!
//! Rows are rebuilt every render frame from `app.agents`; the Overview
//! reflects current session state without caching row data.

pub mod layout;
pub mod peek;
pub mod peek_tail;
pub mod render;
pub mod row;
pub mod state;

pub use render::render_dashboard;
pub use render::{DashboardOverlayChrome, HeaderPromoCta, render_dashboard_session_header};
pub use row::{
    DashboardRow, RowBadge, build_rows, build_rows_with_roster, classify_subagent,
    classify_top_level, roster_activity_to_state, sort_rows,
};
pub use state::{
    DashboardRowId, DashboardState, Filter, FilterValue, Focusable, Grouping, LocationCandidate,
    LocationPickerState, PendingDispatchModel, PersistedDashboard, PersistedRowId, RowState,
    SectionKey, SessionIdResolver, ShortcutsModalState, load_persisted, parse_filter,
    parse_row_state_token,
};

/// Stable detail-cycle order from `IndexMap` residency. Overview filters,
/// grouping, pinning, and activity do not affect which open session is next.
pub fn overlay_cycle_order(
    agents: &indexmap::IndexMap<crate::app::session::AgentId, crate::app::agent_view::AgentView>,
) -> Vec<crate::app::session::AgentId> {
    agents.keys().copied().collect()
}

/// Whether the dashboard feature is enabled.
///
/// Order: env override (`GROW_AGENT_DASHBOARD=0` → off) wins, else the
/// persisted `[dashboard].enabled` flag (default `true`).
///
/// The slash command and CLI subcommand check this before opening; on
/// `false` they print a friendly toast and stay where they are.
///
/// `var_os` avoids the per-call allocation of `var`.
pub fn dashboard_enabled() -> bool {
    if std::env::var_os("GROW_AGENT_DASHBOARD")
        .as_deref()
        .is_some_and(|v| v == std::ffi::OsStr::new("0"))
    {
        return false;
    }
    state::load_persisted_enabled().unwrap_or(true)
}

/// Command to name in the "use /X to switch between sessions" session
/// banners (the `/new` session-created banner and the fork marker).
///
/// Minimal mode has no dashboard — `/agents` is refused there — but the
/// `/resume` session picker still works, so point at it instead (regardless
/// of the dashboard flag, which gates a surface minimal doesn't have).
/// Outside minimal, `/agents` when the feature is enabled; `None` when it
/// is off — the tip would point at a refused command, so callers fall back
/// to a plain session-id banner.
pub(crate) fn session_switch_hint_command(minimal: bool) -> Option<&'static str> {
    if minimal {
        Some("/resume")
    } else if dashboard_enabled() {
        Some("/agents")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal mode always points at `/resume`: the dashboard is refused
    /// there no matter what the feature flag says, so the hint must not
    /// depend on it. Runs under the same serial key as the other
    /// `GROW_AGENT_DASHBOARD` env-mutating tests.
    #[serial_test::serial(GROW_AGENT_DASHBOARD)]
    #[test]
    fn switch_hint_minimal_is_resume_even_with_dashboard_disabled() {
        // SAFETY: the test temporarily mutates a process-wide env var.
        // `serial_test`'s lock ensures no other test marked with the same
        // `GROW_AGENT_DASHBOARD` key reads it concurrently.
        unsafe { std::env::set_var("GROW_AGENT_DASHBOARD", "0") };
        assert_eq!(session_switch_hint_command(true), Some("/resume"));
        unsafe { std::env::remove_var("GROW_AGENT_DASHBOARD") };
    }

    /// Outside minimal the hint mirrors the dashboard flag: `None` when the
    /// env override disables it (the tip would name a refused command),
    /// otherwise whatever `dashboard_enabled()` says — asserted as
    /// consistency, not a fixed value, so the test doesn't depend on the
    /// machine's persisted `[dashboard].enabled`.
    #[serial_test::serial(GROW_AGENT_DASHBOARD)]
    #[test]
    fn switch_hint_non_minimal_follows_dashboard_flag() {
        // SAFETY: see above — serialized on the GROW_AGENT_DASHBOARD key.
        unsafe { std::env::set_var("GROW_AGENT_DASHBOARD", "0") };
        assert_eq!(session_switch_hint_command(false), None);
        unsafe { std::env::remove_var("GROW_AGENT_DASHBOARD") };
        assert_eq!(
            session_switch_hint_command(false),
            dashboard_enabled().then_some("/agents")
        );
    }
}
