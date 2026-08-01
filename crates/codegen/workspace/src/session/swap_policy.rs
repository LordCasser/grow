//! Toolset-swap guard policy: every trigger evaluates the one decision table,
//! [`SwapPolicy::evaluate`] over a [`SessionSnapshot`]. The exhaustive match
//! is the spec; the matrix test's `expected_decision` mirrors it row by row.

use prometheus::{IntCounterVec, register_int_counter_vec};

use crate::activity::ActivityTracker;
use crate::session::WorkspaceSession;

/// Toolset installs/swaps by trigger (`create`/`fork`/
/// `update_tool_config`/`mcp_snapshot`/`other`) plus the guard
/// state at swap time; record via [`record_toolset_swap`] only.
pub(crate) static WORKSPACE_TOOLSET_SWAP_TOTAL: std::sync::LazyLock<IntCounterVec> =
    std::sync::LazyLock::new(|| {
        register_int_counter_vec!(
            "grow_workspace_toolset_swap_total",
            "Session toolset installs and swaps, by trigger and guard state",
            &["trigger", "turn_active", "in_flight"]
        )
        .unwrap()
    });

/// Toolset swaps rejected by the turn-safety guards, by reason
/// (`turn_active` = RPC entry check, `turn_active_late` = post-resolve
/// re-check) and trigger.
pub(crate) static WORKSPACE_TOOLSET_SWAP_REJECTED_TOTAL: std::sync::LazyLock<IntCounterVec> =
    std::sync::LazyLock::new(|| {
        register_int_counter_vec!(
            "grow_workspace_toolset_swap_rejected_total",
            "Session toolset swaps rejected by the turn-safety guards, by reason and trigger",
            &["reason", "trigger"]
        )
        .unwrap()
    });

/// Zero-init this module's metric families. See [`crate::init_metrics`].
pub(crate) fn init_metrics() {
    const TRIGGERS: &[SwapTrigger] = &[
        SwapTrigger::UpdateRpc,
        SwapTrigger::McpSnapshot,
        SwapTrigger::Other,
    ];
    for trigger in TRIGGERS {
        for turn_active in ["true", "false"] {
            for in_flight in ["true", "false"] {
                WORKSPACE_TOOLSET_SWAP_TOTAL
                    .with_label_values(&[trigger.metric_label(), turn_active, in_flight])
                    .inc_by(0);
            }
        }
    }
    // The two turn-active guards fire on local tool-config updates.
    for (reason, trigger) in [
        (DeferReason::TurnActive, SwapTrigger::UpdateRpc),
        (DeferReason::TurnActiveLate, SwapTrigger::UpdateRpc),
    ] {
        WORKSPACE_TOOLSET_SWAP_REJECTED_TOTAL
            .with_label_values(&[reason.metric_reason(), trigger.metric_label()])
            .inc_by(0);
    }
}

/// Record a toolset install/swap on [`WORKSPACE_TOOLSET_SWAP_TOTAL`],
/// stamping the session's turn/in-flight state at swap time.
pub(crate) fn record_toolset_swap(tracker: &ActivityTracker, trigger: &str, session_id: &str) {
    let turn_active = bool_label(tracker.is_turn_active(session_id));
    let in_flight = bool_label(tracker.session_active_tool_calls(session_id) > 0);
    WORKSPACE_TOOLSET_SWAP_TOTAL
        .with_label_values(&[trigger, turn_active, in_flight])
        .inc();
}

fn bool_label(v: bool) -> &'static str {
    if v { "true" } else { "false" }
}

/// What initiated a toolset swap attempt. The trigger fixes both the metric
/// `trigger` label and the guard set the policy applies (see the module table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SwapTrigger {
    /// The `workspace.update_tool_config` RPC.
    UpdateRpc,
    /// `re_resolve_all_sessions` after an MCP snapshot change.
    McpSnapshot,
    /// `re_resolve_all_sessions` from an unrecognized source (test callers
    /// only today). Snapshot-rebuild policy, `other` metric label.
    Other,
}

impl SwapTrigger {
    pub(crate) fn from_rebuild_source(source: &str) -> Self {
        match source {
            "mcp_snapshot_changed" => Self::McpSnapshot,
            _ => Self::Other,
        }
    }

    /// The `trigger` label on [`WORKSPACE_TOOLSET_SWAP_TOTAL`] and
    /// [`WORKSPACE_TOOLSET_SWAP_REJECTED_TOTAL`]. Dashboards depend on these
    /// exact values.
    pub(crate) fn metric_label(self) -> &'static str {
        match self {
            Self::UpdateRpc => "update_tool_config",
            Self::McpSnapshot => "mcp_snapshot",
            Self::Other => "other",
        }
    }

    /// Whether the apply path re-evaluates post-resolve, pre-install. Only the
    /// update path: a turn can start mid-resolve because turn hooks are
    /// lock-free.
    pub(crate) fn rechecks_after_resolve(self) -> bool {
        self == Self::UpdateRpc
    }
}

/// Why a swap was skipped (nothing resolved, toolset and fingerprint kept).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkipReason {
    /// The toolset `Terminal` is not the session-owned backend (local/shell
    /// bind): a rebuild would detach tools from the shell's live task table.
    ExternallyOwned,
}

/// Why a swap was deferred (existing toolset kept; a later attempt applies).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeferReason {
    /// The session's turn is active and the config differs (update-RPC entry
    /// check); retryable at the turn boundary.
    TurnActive,
    /// [`Self::TurnActive`] detected by the post-resolve re-check: the turn
    /// started during the re-resolve and the resolved toolset was discarded.
    TurnActiveLate,
}

impl DeferReason {
    /// The `reason` label on [`WORKSPACE_TOOLSET_SWAP_REJECTED_TOTAL`].
    pub(crate) fn metric_reason(self) -> &'static str {
        match self {
            Self::TurnActive => "turn_active",
            Self::TurnActiveLate => "turn_active_late",
        }
    }
}

/// What a trigger should do with its candidate config, per the module table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "a non-Apply decision means the config must NOT be installed"]
pub(crate) enum SwapDecision {
    /// Resolve and install the candidate config.
    Apply,
    /// Identical fingerprint: the live toolset already reflects the candidate.
    Reuse,
    /// Deliberate skip: leave toolset AND fingerprint untouched.
    Skip(SkipReason),
    /// Keep the existing toolset for now; a later attempt applies.
    Defer(DeferReason),
}

/// How the candidate config's fingerprint relates to the session's stored one.
/// Produced under a single lock acquisition (see [`classify`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindFingerprintTransition {
    /// Candidate fingerprint equals the stored one.
    Unchanged,
    /// Stored fingerprint is `None` (default resolution) and the candidate
    /// differs.
    FromDefault,
    /// Stored fingerprint is explicit and the candidate differs.
    FromExplicit,
}

/// Classify `candidate` against the stored bind fingerprint in one poison-safe
/// lock acquisition, so the decision cannot straddle a concurrent
/// fingerprint write (`set_if_unset` runs outside `update_lock`).
fn classify(
    stored: &std::sync::Mutex<Option<serde_json::Value>>,
    candidate: Option<&serde_json::Value>,
) -> BindFingerprintTransition {
    let guard = stored.lock().unwrap_or_else(|e| e.into_inner());
    if guard.as_ref() == candidate {
        BindFingerprintTransition::Unchanged
    } else if guard.is_some() {
        BindFingerprintTransition::FromExplicit
    } else {
        BindFingerprintTransition::FromDefault
    }
}

/// One coherent read (under `update_lock`) of the session state the policy
/// keys on. Turn/in-flight reads are tracker-side lock-free, so a decision
/// can go stale during a long resolve — see `rechecks_after_resolve`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionSnapshot {
    /// `None` = no candidate config: an in-place rebuild of the session's
    /// current baseline (snapshot-driven triggers), never Reuse-exempt.
    transition: Option<BindFingerprintTransition>,
    turn_active: bool,
    toolset_terminal_session_owned: bool,
    /// The last snapshot-driven rebuild failed and kept a stale toolset
    /// ([`WorkspaceSession::stale_resolve`]): an identical fingerprint does
    /// not prove the live toolset is current, only that the *config* is.
    stale_resolve: bool,
}

impl SessionSnapshot {
    /// Capture against a candidate config fingerprint for a local update.
    pub(crate) async fn capture(
        session: &WorkspaceSession,
        tracker: &ActivityTracker,
        candidate_fingerprint: Option<&serde_json::Value>,
    ) -> Self {
        let transition = classify(&session.tool_config_fingerprint, candidate_fingerprint);
        Self::with_transition(session, tracker, Some(transition)).await
    }

    /// Capture for an in-place rebuild of the session's current baseline (the
    /// snapshot-driven triggers): no candidate config, so no fingerprint
    /// transition to exempt on.
    pub(crate) async fn capture_for_rebuild(
        session: &WorkspaceSession,
        tracker: &ActivityTracker,
    ) -> Self {
        Self::with_transition(session, tracker, None).await
    }

    async fn with_transition(
        session: &WorkspaceSession,
        tracker: &ActivityTracker,
        transition: Option<BindFingerprintTransition>,
    ) -> Self {
        let session_id = session.session_id();
        Self {
            transition,
            turn_active: tracker.is_turn_active(session_id),
            toolset_terminal_session_owned: session.toolset_terminal_is_session_owned().await,
            stale_resolve: session.stale_resolve(),
        }
    }
}

/// The toolset-swap guard policy. Stateless: the whole table lives in
/// [`Self::evaluate`].
pub(crate) struct SwapPolicy;

impl SwapPolicy {
    /// Decide what `trigger` should do with its candidate, per the module
    /// table. Pure function of the snapshot — callers act on the decision
    /// under the same `update_lock` hold the snapshot was captured under.
    pub(crate) fn evaluate(snap: &SessionSnapshot, trigger: SwapTrigger) -> SwapDecision {
        use BindFingerprintTransition::Unchanged;
        match (trigger, snap.transition) {
            (SwapTrigger::UpdateRpc, Some(Unchanged)) if !snap.stale_resolve => SwapDecision::Reuse,
            (SwapTrigger::McpSnapshot | SwapTrigger::Other, Some(Unchanged)) => SwapDecision::Reuse,

            (SwapTrigger::UpdateRpc, _) if snap.turn_active => {
                SwapDecision::Defer(DeferReason::TurnActive)
            }
            (SwapTrigger::UpdateRpc, _) if !snap.toolset_terminal_session_owned => {
                SwapDecision::Skip(SkipReason::ExternallyOwned)
            }
            (SwapTrigger::UpdateRpc, _) => SwapDecision::Apply,

            (SwapTrigger::McpSnapshot | SwapTrigger::Other, _)
                if !snap.toolset_terminal_session_owned =>
            {
                SwapDecision::Skip(SkipReason::ExternallyOwned)
            }
            (SwapTrigger::McpSnapshot | SwapTrigger::Other, _) => SwapDecision::Apply,
        }
    }
}

/// What acting on a [`SwapDecision`] ultimately did — the key of
/// [`record_swap_decision`]. No `Reused` action: a reuse changes nothing
/// and no metric family counts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SwapAction {
    /// [`SwapDecision::Skip`] honored.
    Skipped(SkipReason),
    /// [`SwapDecision::Defer`] honored (existing toolset kept).
    Deferred(DeferReason),
    /// [`SwapDecision::Apply`] succeeded: toolset resolved and installed.
    Applied,
    /// [`SwapDecision::Apply`] failed: the re-resolve errored, existing kept.
    ApplyFailed,
}

/// The single chokepoint for swap metrics.
pub(crate) fn record_swap_decision(
    tracker: &ActivityTracker,
    trigger: SwapTrigger,
    session_id: &str,
    action: SwapAction,
) {
    match action {
        SwapAction::Deferred(reason) => {
            WORKSPACE_TOOLSET_SWAP_REJECTED_TOTAL
                .with_label_values(&[reason.metric_reason(), trigger.metric_label()])
                .inc();
        }
        SwapAction::Skipped(SkipReason::ExternallyOwned) => {}
        SwapAction::Applied => {
            record_toolset_swap(tracker, trigger.metric_label(), session_id);
        }
        SwapAction::ApplyFailed => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(
        transition: Option<BindFingerprintTransition>,
        turn_active: bool,
        owned: bool,
        stale_resolve: bool,
    ) -> SessionSnapshot {
        SessionSnapshot {
            transition,
            turn_active,
            toolset_terminal_session_owned: owned,
            stale_resolve,
        }
    }

    #[test]
    fn unchanged_local_update_reuses_current_toolset() {
        let unchanged = Some(BindFingerprintTransition::Unchanged);
        assert_eq!(
            SwapPolicy::evaluate(&snap(unchanged, false, true, false), SwapTrigger::UpdateRpc),
            SwapDecision::Reuse
        );
        assert_eq!(
            SwapPolicy::evaluate(&snap(unchanged, false, true, true), SwapTrigger::UpdateRpc),
            SwapDecision::Apply
        );
    }

    #[test]
    fn local_update_is_turn_safe_and_requires_owned_terminal() {
        let changed = Some(BindFingerprintTransition::FromExplicit);
        assert_eq!(
            SwapPolicy::evaluate(&snap(changed, true, true, false), SwapTrigger::UpdateRpc),
            SwapDecision::Defer(DeferReason::TurnActive)
        );
        assert_eq!(
            SwapPolicy::evaluate(&snap(changed, false, false, false), SwapTrigger::UpdateRpc),
            SwapDecision::Skip(SkipReason::ExternallyOwned)
        );
        assert_eq!(
            SwapPolicy::evaluate(&snap(changed, false, true, false), SwapTrigger::UpdateRpc),
            SwapDecision::Apply
        );
    }

    #[test]
    fn local_snapshot_rebuilds_ignore_turn_state() {
        for trigger in [SwapTrigger::McpSnapshot, SwapTrigger::Other] {
            assert_eq!(
                SwapPolicy::evaluate(&snap(None, true, true, false), trigger),
                SwapDecision::Apply
            );
            assert_eq!(
                SwapPolicy::evaluate(&snap(None, true, false, false), trigger),
                SwapDecision::Skip(SkipReason::ExternallyOwned)
            );
        }
    }

    #[test]
    fn rebuild_source_and_metric_labels_are_local() {
        assert_eq!(
            SwapTrigger::from_rebuild_source("mcp_snapshot_changed"),
            SwapTrigger::McpSnapshot
        );
        assert_eq!(SwapTrigger::from_rebuild_source("test"), SwapTrigger::Other);
        assert_eq!(SwapTrigger::UpdateRpc.metric_label(), "update_tool_config");
        assert_eq!(SwapTrigger::McpSnapshot.metric_label(), "mcp_snapshot");
        assert_eq!(SwapTrigger::Other.metric_label(), "other");
    }

    #[test]
    fn classify_is_poison_safe() {
        let stored = std::sync::Arc::new(std::sync::Mutex::new(Some(serde_json::json!({"a": 1}))));
        let poisoner = stored.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().unwrap();
            panic!("poison fingerprint lock");
        })
        .join();
        assert_eq!(
            classify(&stored, Some(&serde_json::json!({"a": 1}))),
            BindFingerprintTransition::Unchanged
        );
    }
}
