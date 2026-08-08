//! The ephemeral tip primitive: a single-slot, TTL'd, dedup-keyed banner hint
//! and the show/seen-count gating state that drives it.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::text::Line;

/// Default tip lifetime. UI expiry is wall-clock based and independent from
/// the configured animation sampling rate.
pub const DEFAULT_TIP_DURATION: Duration = Duration::from_secs(3);

/// Whether the tip row can render given UI occlusion and the screen height.
/// Single predicate shared by the show gate, the banner-height reservation,
/// and the paint so the three can never drift. Shares the layout's
/// short-terminal threshold so the tip row appears exactly when the optional
/// rows above the prompt do.
pub fn tip_row_renderable(occluded: bool, area_height: u16) -> bool {
    !occluded && area_height > crate::views::agent::SHORT_TERMINAL_ROWS
}

/// A transient one-line hint shown in the banner row above the prompt.
#[derive(Debug, Clone)]
pub struct EphemeralTip {
    /// Dedup key: re-showing the same key refreshes the TTL instead of
    /// stacking, and [`EphemeralTipState::clear`] only removes a match.
    pub key: &'static str,
    /// Pre-styled spans (dim text with a highlighted key chord).
    pub line: Line<'static>,
    /// Visible lifetime before the tip expires.
    pub lifetime: Duration,
    /// In-memory seen-count map key paired with the per-session show cap:
    /// `Some((key, cap))` stops showing once this session's count for `key`
    /// reaches `cap`; `None` for tips that are never seen-gated. The count
    /// lives only in `AppView::tip_seen_counts` (per session, never on disk).
    pub session_seen: Option<(&'static str, u32)>,
    /// Ambient hint, not contextual to the draft being edited: submission
    /// ([`EphemeralTipState::clear_on_submit`]) does NOT retire it, and its
    /// TTL burns only while the tip row can actually paint (occlusion pauses
    /// instead of expiring it off-screen — see
    /// `AgentView::reconcile_ephemeral_tip_clock`). Default `false` keeps the
    /// edit-contextual tips' retire-on-submit + burn-while-occluded behavior.
    pub ambient: bool,
}

impl EphemeralTip {
    /// Build a tip with the default TTL and no seen-gating.
    pub fn new(key: &'static str, line: Line<'static>) -> Self {
        Self {
            key,
            line,
            lifetime: DEFAULT_TIP_DURATION,
            session_seen: None,
            ambient: false,
        }
    }

    /// Gate the tip on a per-session in-memory seen count: it stops showing
    /// once this session's count reaches `cap` (resets every new pager run).
    pub fn with_session_seen_cap(mut self, key: &'static str, cap: u32) -> Self {
        self.session_seen = Some((key, cap));
        self
    }

    /// Mark the tip ambient (survives submission; TTL pauses while occluded).
    pub fn ambient(mut self) -> Self {
        self.ambient = true;
        self
    }
}

/// Single-slot ephemeral tip state. Seen counts are NOT stored here — gating
/// runs against the app-level map passed into [`Self::show`], so there is
/// exactly one copy of that state.
#[derive(Debug, Default)]
pub struct EphemeralTipState {
    slot: Option<EphemeralTip>,
    expires_at: Option<Instant>,
    paused_remaining: Option<Duration>,
}

impl EphemeralTipState {
    /// Show `tip`, replacing any currently shown tip. Re-showing the key
    /// already on screen only refreshes the TTL (no second count increment).
    ///
    /// Seen-gating runs against `seen_counts` (the app-level per-session map):
    /// a tip whose count reached its cap is a no-op. A passing show increments
    /// the map in place. Returns true when the tip was newly shown (false on a
    /// same-key TTL refresh or a gated no-op).
    ///
    /// Pager code must go through `AgentView::show_ephemeral_tip`, which
    /// adds the renderability gate — calling this directly skips it and can
    /// burn a seen count on a tip the user never sees (tests only).
    pub(crate) fn show(
        &mut self,
        tip: EphemeralTip,
        seen_counts: &mut HashMap<&'static str, u32>,
    ) -> bool {
        self.show_at(tip, seen_counts, Instant::now())
    }

    fn show_at(
        &mut self,
        tip: EphemeralTip,
        seen_counts: &mut HashMap<&'static str, u32>,
        now: Instant,
    ) -> bool {
        // Refresh before gating so a visible tip never goes dark mid-TTL
        // just because its first show already reached the cap.
        if self.slot.as_ref().is_some_and(|cur| cur.key == tip.key) {
            self.expires_at = Some(now + tip.lifetime);
            self.paused_remaining = None;
            self.slot = Some(tip);
            return false;
        }
        if let Some((seen_key, cap)) = tip.session_seen
            && seen_counts.get(seen_key).copied().unwrap_or(0) >= cap
        {
            return false;
        }
        if let Some(replaced) = self.slot.take() {
            log_dismissed(replaced.key, DismissReason::Replaced);
        }
        crate::unified_log::info(
            "tip.shown",
            None,
            Some(serde_json::json!({ "key": tip.key })),
        );
        if let Some((key, _cap)) = tip.session_seen {
            let count = seen_counts.get(key).copied().unwrap_or(0).saturating_add(1);
            seen_counts.insert(key, count);
        }
        self.expires_at = Some(now + tip.lifetime);
        self.paused_remaining = None;
        self.slot = Some(tip);
        true
    }

    /// Reconcile whether the absolute TTL is currently counting down without
    /// consuming an already-due deadline. The scheduler uses this to pause or
    /// resume clocks before choosing its next wakeup.
    pub(crate) fn sync_clock_policy(&mut self, now: Instant, counting: bool) {
        let Some(tip) = self.slot.as_ref() else {
            self.expires_at = None;
            self.paused_remaining = None;
            return;
        };
        if counting {
            if self.expires_at.is_none() {
                let remaining = self.paused_remaining.take().unwrap_or(tip.lifetime);
                self.expires_at = Some(now + remaining);
            }
        } else if let Some(deadline) = self.expires_at.take() {
            self.paused_remaining = Some(deadline.saturating_duration_since(now));
        }
    }

    /// Apply clock policy and consume an expired deadline.
    pub(crate) fn maintain(&mut self, now: Instant, counting: bool) -> bool {
        self.sync_clock_policy(now, counting);
        if self.expires_at.is_some_and(|deadline| now >= deadline) {
            let key = self.slot.as_ref().expect("deadline requires tip").key;
            self.slot = None;
            self.expires_at = None;
            self.paused_remaining = None;
            log_dismissed(key, DismissReason::Expired);
            return true;
        }
        false
    }

    /// Whether a tip is on screen (drives `needs_animation` when it can tick).
    pub fn is_active(&self) -> bool {
        self.slot.is_some()
    }

    /// Absolute expiry deadline while the clock is running.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.expires_at
    }

    #[cfg(test)]
    pub(crate) fn remaining_at(&self, now: Instant) -> Option<Duration> {
        self.slot.as_ref()?;
        Some(
            self.expires_at
                .map(|deadline| deadline.saturating_duration_since(now))
                .or(self.paused_remaining)
                .unwrap_or_default(),
        )
    }

    /// The active tip's pre-styled line, if any.
    pub fn line(&self) -> Option<&Line<'static>> {
        self.slot.as_ref().map(|t| &t.line)
    }

    /// The active tip's dedup key, if any (drives accept-site attribution).
    pub fn current_key(&self) -> Option<&'static str> {
        self.slot.as_ref().map(|t| t.key)
    }

    /// Clear the tip only when `key` matches the one on screen.
    /// Returns true when a tip was removed (needs redraw).
    pub fn clear(&mut self, key: &str) -> bool {
        if self.slot.as_ref().is_some_and(|t| t.key == key) {
            self.dismiss();
            return true;
        }
        false
    }

    /// Clear any tip (e.g. on prompt submit).
    /// Returns true when a tip was removed (needs redraw).
    pub fn clear_all(&mut self) -> bool {
        if self.slot.is_some() {
            self.dismiss();
            return true;
        }
        false
    }

    /// Submission retire: clear the tip unless it is ambient (an ambient tip
    /// is not about the draft that was just submitted, so it lives out its
    /// TTL across the submit). Returns true when a tip was removed.
    pub fn clear_on_submit(&mut self) -> bool {
        if self.slot.as_ref().is_some_and(|t| t.ambient) {
            return false;
        }
        self.clear_all()
    }

    /// Whether the active tip (if any) is ambient — drives the TTL pause
    /// while the tip row cannot paint.
    pub(crate) fn active_is_ambient(&self) -> bool {
        self.slot.as_ref().is_some_and(|t| t.ambient)
    }

    fn dismiss(&mut self) {
        if let Some(tip) = self.slot.take() {
            self.expires_at = None;
            self.paused_remaining = None;
            log_dismissed(tip.key, DismissReason::Cleared);
        }
    }
}

/// Why a tip left the slot, mapped to the `tip.dismissed` diagnostics reason.
#[derive(Debug, Clone, Copy)]
enum DismissReason {
    /// A different-keyed tip took the slot.
    Replaced,
    /// The TTL ran out.
    Expired,
    /// An explicit clear (keyed clear, `clear_all`, or submit).
    Cleared,
}

impl DismissReason {
    /// Diagnostic string — must stay stable for `tip.dismissed` dashboards.
    fn as_str(self) -> &'static str {
        match self {
            Self::Replaced => "replaced",
            Self::Expired => "expired",
            Self::Cleared => "cleared",
        }
    }
}

fn log_dismissed(key: &'static str, reason: DismissReason) {
    crate::unified_log::info(
        "tip.dismissed",
        None,
        Some(serde_json::json!({ "key": key, "reason": reason.as_str() })),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tip(key: &'static str, lifetime: Duration) -> EphemeralTip {
        EphemeralTip {
            lifetime,
            ..EphemeralTip::new(key, Line::from("test tip"))
        }
    }

    #[test]
    fn ttl_expires_and_clears_slot() {
        let mut state = EphemeralTipState::default();
        let now = Instant::now();
        assert!(state.show_at(tip("a", Duration::from_secs(2)), &mut HashMap::new(), now));
        assert!(state.is_active());
        assert!(!state.maintain(now + Duration::from_secs(1), true));
        assert!(state.maintain(now + Duration::from_secs(2), true));
        assert!(!state.is_active());
        assert!(state.line().is_none());
        assert!(
            !state.maintain(now + Duration::from_secs(3), true),
            "empty slot maintenance is a no-op"
        );
    }

    #[test]
    fn show_different_key_replaces_current_tip() {
        let mut state = EphemeralTipState::default();
        let mut counts = HashMap::new();
        let _ = state.show(EphemeralTip::new("a", Line::from("first")), &mut counts);
        let _ = state.show(EphemeralTip::new("b", Line::from("second")), &mut counts);
        assert!(state.is_active());
        assert_eq!(state.line(), Some(&Line::from("second")));
        assert!(!state.clear("a"), "replaced tip key no longer matches");
        assert!(state.clear("b"));
    }

    #[test]
    fn show_same_key_refreshes_ttl() {
        let mut state = EphemeralTipState::default();
        let mut counts = HashMap::new();
        let now = Instant::now();
        let lifetime = Duration::from_secs(3);
        let _ = state.show_at(tip("a", lifetime), &mut counts, now);
        assert!(!state.maintain(now + Duration::from_secs(2), true));
        let refreshed_at = now + Duration::from_secs(2);
        let _ = state.show_at(tip("a", lifetime), &mut counts, refreshed_at);
        assert!(!state.maintain(now + Duration::from_secs(4), true));
        assert!(
            state.maintain(now + Duration::from_secs(5), true),
            "expires from the refreshed deadline, not the original one"
        );
    }

    #[test]
    fn seen_gating_counts_up_to_cap_then_blocks() {
        let mut state = EphemeralTipState::default();
        let mut counts = HashMap::new();
        for expected in 1..=2 {
            assert!(
                state.show(
                    tip("a", Duration::from_secs(5)).with_session_seen_cap("a_seen", 2),
                    &mut counts,
                ),
                "a fresh show takes the slot and counts"
            );
            assert_eq!(counts.get("a_seen"), Some(&expected));
            assert!(state.clear_all());
        }
        assert!(
            !state.show(
                tip("a", Duration::from_secs(5)).with_session_seen_cap("a_seen", 2),
                &mut counts,
            ),
            "gated show must be a no-op"
        );
        assert!(!state.is_active(), "gated show must be a no-op");
        assert_eq!(
            counts.get("a_seen"),
            Some(&2),
            "blocked show must not count"
        );
    }

    #[test]
    fn show_gates_against_preloaded_counts() {
        let mut state = EphemeralTipState::default();
        // A session count already at the cap (e.g. shown earlier this run)
        // blocks the next show.
        let mut counts = HashMap::from([("a_seen", 1u32)]);
        assert!(!state.show(
            tip("a", Duration::from_secs(5)).with_session_seen_cap("a_seen", 1),
            &mut counts
        ));
        assert!(!state.is_active());
    }

    #[test]
    fn same_key_refresh_skips_gate_and_recount() {
        let mut state = EphemeralTipState::default();
        let mut counts = HashMap::new();
        assert!(state.show(
            tip("a", Duration::from_secs(5)).with_session_seen_cap("a_seen", 1),
            &mut counts
        ));
        // Still visible: cap is reached but the refresh must not go dark
        // and must not burn another count.
        assert!(
            !state.show(
                tip("a", Duration::from_secs(5)).with_session_seen_cap("a_seen", 1),
                &mut counts,
            ),
            "same-key refresh neither re-counts nor re-shows"
        );
        assert!(state.is_active());
        assert_eq!(counts.get("a_seen"), Some(&1));
    }

    #[test]
    fn unkeyed_tip_is_never_gated() {
        let mut state = EphemeralTipState::default();
        let mut counts = HashMap::new();
        for _ in 0..3 {
            assert!(
                state.show(tip("a", Duration::from_secs(5)), &mut counts),
                "unkeyed shows always take the slot"
            );
            assert!(state.is_active());
            assert!(state.clear_all());
        }
        assert!(counts.is_empty(), "unkeyed shows never touch the map");
    }

    #[test]
    fn clear_only_removes_matching_key() {
        let mut state = EphemeralTipState::default();
        let _ = state.show(tip("a", Duration::from_secs(5)), &mut HashMap::new());
        assert!(!state.clear("other"));
        assert!(state.is_active());
        assert!(state.clear("a"));
        assert!(!state.is_active());
        assert!(!state.clear("a"), "second clear is a no-op");
    }

    #[test]
    fn current_key_tracks_the_active_tip() {
        let mut state = EphemeralTipState::default();
        assert_eq!(state.current_key(), None, "empty slot has no key");
        let _ = state.show(tip("undo_tip", Duration::from_secs(5)), &mut HashMap::new());
        assert_eq!(state.current_key(), Some("undo_tip"));
        // A different-keyed show replaces the reported key.
        let _ = state.show(
            tip("plan_nudge", Duration::from_secs(5)),
            &mut HashMap::new(),
        );
        assert_eq!(state.current_key(), Some("plan_nudge"));
        assert!(state.clear("plan_nudge"));
        assert_eq!(state.current_key(), None, "cleared slot reports no key");
    }

    #[test]
    fn clear_all_reports_whether_a_tip_was_removed() {
        let mut state = EphemeralTipState::default();
        assert!(!state.clear_all());
        let _ = state.show(tip("a", Duration::from_secs(5)), &mut HashMap::new());
        assert!(state.clear_all());
        assert!(!state.clear_all());
    }

    #[test]
    fn clear_on_submit_retires_edit_contextual_but_keeps_ambient() {
        let mut state = EphemeralTipState::default();

        // Default (edit-contextual) tip: submit retires it.
        let _ = state.show(tip("a", Duration::from_secs(5)), &mut HashMap::new());
        assert!(state.clear_on_submit());
        assert!(!state.is_active());

        // Ambient tip: submit is a no-op; an explicit clear_all still works.
        let _ = state.show(
            tip("b", Duration::from_secs(5)).ambient(),
            &mut HashMap::new(),
        );
        assert!(!state.clear_on_submit());
        assert!(state.is_active(), "ambient tip must survive the submit");
        assert!(state.active_is_ambient());
        assert!(state.clear_all());
    }

    #[test]
    fn tip_row_renderable_gates_on_occlusion_and_terminal_height() {
        assert!(tip_row_renderable(false, 30));
        assert!(
            !tip_row_renderable(true, 30),
            "occluded by permission/question/modal"
        );
        assert!(!tip_row_renderable(false, 16), "short terminal");
        assert!(tip_row_renderable(false, 17));
        assert!(
            !tip_row_renderable(false, 0),
            "unknown size before first draw"
        );
    }
}
