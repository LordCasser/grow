//! Compaction configuration and runtime state for the session actor.

use std::cell::Cell;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

const COMPACTION_IDLE: u8 = 0;
const COMPACTION_MANUAL: u8 = 1;
const COMPACTION_AUTO: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionOwner {
    Manual,
    Auto,
}

/// Single publication lease for conversation replacement. Manual and auto
/// compaction can never both enter the mutating pipeline.
#[derive(Default)]
pub struct CompactionLease {
    owner: AtomicU8,
}

pub struct CompactionLeaseGuard<'a>(&'a CompactionLease);

impl Drop for CompactionLeaseGuard<'_> {
    fn drop(&mut self) {
        self.0.owner.store(COMPACTION_IDLE, Ordering::Release);
    }
}

impl CompactionLease {
    pub fn try_enter(&self, owner: CompactionOwner) -> Option<CompactionLeaseGuard<'_>> {
        let value = match owner {
            CompactionOwner::Manual => COMPACTION_MANUAL,
            CompactionOwner::Auto => COMPACTION_AUTO,
        };
        self.owner
            .compare_exchange(COMPACTION_IDLE, value, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| CompactionLeaseGuard(self))
    }

    pub fn is_in_flight(&self) -> bool {
        self.owner.load(Ordering::Acquire) != COMPACTION_IDLE
    }
}

/// Auto-compaction is gated whenever `auto_compact_suppressed` is not [`SUPPRESS_NONE`].
pub(crate) const SUPPRESS_NONE: u8 = 0;
/// Resolvable failure (`other`): suppressed for the current turn, then
/// cleared at the next turn start so compaction self-heals once the cause clears.
pub(crate) const SUPPRESS_TURN: u8 = 1;
/// Fatal failure (size/schema) retrying can never fix: survives turn boundaries,
/// cleared only when the context budget changes — a successful compaction, a
/// rewind (context shrank), or a model switch (a larger window may now fit).
pub(crate) const SUPPRESS_STICKY: u8 = 2;
/// Provider account/quota limit: suppress until a model `200` because the
/// provider's account state is not client-observable.
/// Survives turns; context changes can't fix it. Token refresh must not clear this.
pub(crate) const SUPPRESS_UNTIL_SUCCESS: u8 = 3;
/// Auth-expired auto-compact: suppress until login/token refresh, not until 200
/// (waiting for a sample deadlocks when context is already over the window).
pub(crate) const SUPPRESS_AUTH: u8 = 4;

/// Model slug and context window from the previous turn.
#[derive(Clone, Debug)]
pub struct PreviousModelInfo {
    pub model_slug: String,
    pub context_window: u64,
}

/// Cancel gate for an in-flight compaction sample.
///
/// Holder count (not a bool): the first `enter` installs a token; nested enters
/// reuse it; `in_flight` stays true until the last scope drops. A normal turn
/// stop is a no-op when idle.
#[derive(Default)]
pub struct CompactCancelGate {
    token: RefCell<tokio_util::sync::CancellationToken>,
    holders: AtomicUsize,
    background: Cell<bool>,
}

/// Decrements the holder count when a compaction scope ends.
pub struct CompactCancelScope<'a>(&'a CompactCancelGate);

impl Drop for CompactCancelScope<'_> {
    fn drop(&mut self) {
        self.0.end();
    }
}

impl CompactCancelGate {
    /// Start or join a compaction scope. Nested callers share one token,
    /// including a token already cancelled by stop. A later independent enter
    /// after holders drain installs a fresh token.
    pub fn enter(&self) -> (tokio_util::sync::CancellationToken, CompactCancelScope<'_>) {
        self.enter_scope(None)
    }

    /// Transfer a background job's cancellation authority to its foreground
    /// publisher without replacing the token observed by generation/commit.
    pub(crate) fn enter_background(
        &self,
        token: tokio_util::sync::CancellationToken,
    ) -> (tokio_util::sync::CancellationToken, CompactCancelScope<'_>) {
        self.enter_scope(Some(token))
    }

    fn enter_scope(
        &self,
        background: Option<tokio_util::sync::CancellationToken>,
    ) -> (tokio_util::sync::CancellationToken, CompactCancelScope<'_>) {
        let prev = self.holders.fetch_add(1, Ordering::AcqRel);
        let token = if prev == 0 {
            self.background.set(background.is_some());
            let token = background.unwrap_or_default();
            self.token.replace(token.clone());
            token
        } else {
            self.token.borrow().clone()
        };
        (token, CompactCancelScope(self))
    }

    fn end(&self) {
        self.holders.fetch_sub(1, Ordering::AcqRel);
    }

    pub fn request_cancel(&self) {
        if self.holders.load(Ordering::Acquire) > 0 {
            self.token.borrow().cancel();
        }
    }

    pub(crate) fn request_background_cancel(&self) {
        if self.background.get() {
            self.request_cancel();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.holders.load(Ordering::Acquire) > 0 && self.token.borrow().is_cancelled()
    }
}

pub struct CompactionConfig {
    pub lease: CompactionLease,
    pub(crate) background: RefCell<Option<crate::session::actor::compaction::BackgroundCompaction>>,
    pub(crate) background_failed: Cell<bool>,
    /// Context window usage percentage (0-100) at which auto-compact triggers.
    ///
    /// `Cell` so the value can be re-resolved at model-switch time without
    /// holding `&mut self` on the actor. `SessionActor` is `!Send`, so
    /// `Cell` is sufficient (no atomic ordering needed).
    pub threshold_percent: Cell<u8>,
    /// Whether a memory flush is launched before each eligible compaction.
    pub memory_flush_enabled: bool,
    /// Maximum wall-clock duration for one compaction generation.
    pub wall_clock_budget_secs: u64,
    /// Debug: when set, next auto-compact check triggers unconditionally.
    pub force_compact: Arc<AtomicBool>,
    /// Auto-compaction suppression state (`SUPPRESS_*`) after a deterministic
    /// failure; the gates early-return unless `SUPPRESS_NONE`. Manual `/compact` ignores it.
    pub auto_compact_suppressed: AtomicU8,
    /// Locks the context window when `GROW_DEBUG_CONTEXT_WINDOW` is set.
    pub context_window_override: Option<std::num::NonZeroU64>,
    pub count: AtomicU64,
    /// Refreshed at turn start and after each applied step-boundary route
    /// change; consumed before the next sample for model-switch compaction.
    /// `Cell` because `SessionActor` is `!Send`.
    pub previous_model: Cell<Option<PreviousModelInfo>>,
    /// When `true`, feed the summarizer the verbatim conversation instead of the lossy rewrite (the retry loop may still fall back).
    pub verbatim_input: bool,
    /// Pre-prune gate (`compaction.pre_prune`): when `true`, `run_compact_only`
    /// first tries model-free tool-result pruning; a successful prune that
    /// brings the estimate under the trigger threshold skips the summary call.
    /// `Cell` per the !Send `SessionActor` pattern.
    pub pre_prune: Cell<bool>,
    /// Per-item pruning token budget override; `None` derives 5% of the context
    /// window (lower bound 1 token).
    pub pre_prune_token_budget: Cell<Option<u64>>,
    /// User/stop cancel for the current compact generation.
    pub cancel: CompactCancelGate,
}

#[cfg(test)]
mod compaction_lease_tests {
    use super::*;

    #[test]
    fn manual_and_auto_compaction_are_mutually_exclusive() {
        let lease = CompactionLease::default();
        let manual = lease
            .try_enter(CompactionOwner::Manual)
            .expect("manual lease");
        assert!(lease.is_in_flight());
        assert!(lease.try_enter(CompactionOwner::Auto).is_none());
        drop(manual);
        let auto = lease.try_enter(CompactionOwner::Auto).expect("auto lease");
        assert!(lease.try_enter(CompactionOwner::Manual).is_none());
        drop(auto);
        assert!(!lease.is_in_flight());
    }
}

#[cfg(test)]
mod compact_cancel_gate_tests {
    use super::*;

    #[test]
    fn background_invalidation_tracks_the_original_token_through_publication() {
        let gate = CompactCancelGate::default();
        let original = tokio_util::sync::CancellationToken::new();
        let (published, scope) = gate.enter_background(original.clone());
        let (nested, nested_scope) = gate.enter();
        gate.request_background_cancel();
        assert!(original.is_cancelled());
        assert!(published.is_cancelled());
        assert!(nested.is_cancelled());
        drop(nested_scope);
        drop(scope);
        let (foreground, _scope) = gate.enter();
        gate.request_background_cancel();
        assert!(
            !foreground.is_cancelled(),
            "an unrelated synchronous scope is not a background job"
        );
    }

    #[test]
    fn request_cancel_trips_shared_token() {
        let gate = CompactCancelGate::default();
        let (token, _scope) = gate.enter();
        assert!(!token.is_cancelled());
        gate.request_cancel();
        assert!(token.is_cancelled());
        assert!(gate.is_cancelled());
    }

    #[test]
    fn request_cancel_is_noop_when_idle() {
        let gate = CompactCancelGate::default();
        gate.request_cancel();
        let (token, _scope) = gate.enter();
        assert!(!token.is_cancelled());
        assert!(!gate.is_cancelled());
    }

    #[test]
    fn nested_enter_keeps_in_flight_after_inner_drop() {
        let gate = CompactCancelGate::default();
        let (outer_tok, outer) = gate.enter();
        let (inner_tok, inner) = gate.enter();
        gate.request_cancel();
        assert!(outer_tok.is_cancelled());
        assert!(inner_tok.is_cancelled());
        drop(inner);
        assert!(gate.is_cancelled());
        drop(outer);
        assert!(!gate.is_cancelled());
        let (next, _scope) = gate.enter();
        assert!(!next.is_cancelled());
    }

    #[test]
    fn join_while_cancelled_reuses_cancelled_token() {
        let gate = CompactCancelGate::default();
        let (_outer, outer) = gate.enter();
        gate.request_cancel();
        let (joined, joined_scope) = gate.enter();
        assert!(
            joined.is_cancelled(),
            "nested enter during stop must keep sharing the cancelled token"
        );
        drop(joined_scope);
        drop(outer);
        let (next, _scope) = gate.enter();
        assert!(
            !next.is_cancelled(),
            "fresh enter after scopes drain must not inherit the prior stop"
        );
    }
}
