//! Local session event vocabulary and writers.
//! The orphan-rule items (`From<&permission::Decision>`
//! and the doom-loop categorizer) stay here since they need shell-local
//! types.

// Re-exports from the shell-owned local event modules.
pub(crate) use crate::session::event_tracker::EventTracker;
pub(crate) use crate::session::event_types::{
    CancellationCategory, EVENT_SCHEMA_VERSION, Event, InterjectionSource, Phase, RedirectKind,
    SessionRelationship, ToolCompletedSource, ToolOutcome, TurnOutcomeLabel,
};
pub(crate) use crate::session::event_writer::EventWriter;

// ── Laziness detector (Layer 3) discriminator vocabulary ─────────────
//
// Single source of truth for the `category` field on
// `Event::LazinessClassifierFired` / `LazinessNudgeFired` and the
// `reason` field on `Event::LazinessClassifierAborted`. The producer
// wraps the category strings in `LazinessCategory::as_const_str()`
// (acp_session.rs) for compile-time closure over the set, and the
// abort reasons are emitted only via the `LAZINESS_ABORT_*` consts
// below — no string literals at any producer site.
//
// The category strings are also the lowercase serde representation of
// the classifier's JSON output, so producer (classifier prompt) and
// consumer (Rust enum + diagnostics) share one vocabulary.

/// Stalled — the model emitted prose narration claiming progress
/// without any real tool calls.
pub const LAZINESS_STALLED_NARRATION: &str = "stalled_narration";

/// Stalled — the model asked the user for permission to continue a
/// task that is already in flight.
pub const LAZINESS_STALLED_PERMISSION_ASKING: &str = "stalled_permission_asking";

/// Stalled — the model has no todo list but a multi-step task is
/// clearly in flight (no active plan tool calls despite a complex
/// pending task).
pub const LAZINESS_STALLED_NO_TODOS_BUT_TASK_IN_FLIGHT: &str =
    "stalled_no_todos_but_task_in_flight";

/// Stalled — the agent declared completion/success but the transcript
/// shows substantive claims unbacked by tool-call evidence (e.g. claims
/// running `make test` but no `make` tool_call appears; claims
/// "overnight 8+ hour run" but elapsed time is minutes; claims N review
/// rounds but only M happened).
pub const LAZINESS_STALLED_FALSE_COMPLETION: &str = "stalled_false_completion";

/// Not stalled — the model has genuinely completed its task.
pub const LAZINESS_NOT_STALLED_COMPLETE: &str = "not_stalled_complete";

/// Not stalled — the model is correctly waiting on a backgrounded task
/// it cannot drive forward.
pub const LAZINESS_NOT_STALLED_WAITING_BG: &str = "not_stalled_waiting_on_background";

/// Not stalled — the model is correctly waiting on user input for a
/// genuine ambiguity.
pub const LAZINESS_NOT_STALLED_WAITING_USER: &str = "not_stalled_waiting_on_user";

/// Aborted because a fresh user prompt arrived before classification
/// completed.
pub const LAZINESS_ABORT_USER_INPUT: &str = "user_input";

/// Aborted because the user switched models mid-classification.
pub const LAZINESS_ABORT_MODEL_SWITCH: &str = "model_switch";

/// Aborted because the classifier exceeded its wall-clock budget.
pub const LAZINESS_ABORT_TIMEOUT: &str = "timeout";

/// Aborted because the classifier response failed to parse after the
/// tolerant parser exhausted all three passes.
pub const LAZINESS_ABORT_CLASSIFIER_ERROR: &str = "classifier_error";

// Compile-time guard against an accidentally-empty const breaking the
// dashboards' group-by. `const _: () = assert!(…)` fires at build
// time, not at first test run.
#[allow(clippy::const_is_empty)]
const _: () = assert!(
    !LAZINESS_STALLED_NARRATION.is_empty()
        && !LAZINESS_STALLED_PERMISSION_ASKING.is_empty()
        && !LAZINESS_STALLED_NO_TODOS_BUT_TASK_IN_FLIGHT.is_empty()
        && !LAZINESS_STALLED_FALSE_COMPLETION.is_empty()
        && !LAZINESS_NOT_STALLED_COMPLETE.is_empty()
        && !LAZINESS_NOT_STALLED_WAITING_BG.is_empty()
        && !LAZINESS_NOT_STALLED_WAITING_USER.is_empty()
        && !LAZINESS_ABORT_USER_INPUT.is_empty()
        && !LAZINESS_ABORT_MODEL_SWITCH.is_empty()
        && !LAZINESS_ABORT_TIMEOUT.is_empty()
        && !LAZINESS_ABORT_CLASSIFIER_ERROR.is_empty(),
    "Laziness discriminator consts must be non-empty",
);

/// Closed set of categories the Layer-3 classifier can return.
///
/// Mirrors the JSON schema in the classifier prompt; `serde` uses the
/// `LAZINESS_*` strings above as the wire format (snake_case). The
/// `as_const_str()` mapping is exhaustive over the variants — the
/// `laziness_category_round_trip` test asserts the variant ↔ const
/// pairing is one-to-one. Mirrors the `TodoGateReason` pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LazinessCategory {
    StalledNarration,
    StalledPermissionAsking,
    StalledNoTodosButTaskInFlight,
    StalledFalseCompletion,
    NotStalledComplete,
    NotStalledWaitingOnBackground,
    NotStalledWaitingOnUser,
}

impl LazinessCategory {
    /// Returns the `LAZINESS_*` string for this variant. Exhaustive
    /// `match` — adding a variant forces a new const + arm.
    pub(crate) fn as_const_str(self) -> &'static str {
        match self {
            Self::StalledNarration => LAZINESS_STALLED_NARRATION,
            Self::StalledPermissionAsking => LAZINESS_STALLED_PERMISSION_ASKING,
            Self::StalledNoTodosButTaskInFlight => LAZINESS_STALLED_NO_TODOS_BUT_TASK_IN_FLIGHT,
            Self::StalledFalseCompletion => LAZINESS_STALLED_FALSE_COMPLETION,
            Self::NotStalledComplete => LAZINESS_NOT_STALLED_COMPLETE,
            Self::NotStalledWaitingOnBackground => LAZINESS_NOT_STALLED_WAITING_BG,
            Self::NotStalledWaitingOnUser => LAZINESS_NOT_STALLED_WAITING_USER,
        }
    }

    /// Four variants count as "stalled" and are eligible for a nudge.
    pub(crate) fn is_stalled(self) -> bool {
        match self {
            Self::StalledNarration
            | Self::StalledPermissionAsking
            | Self::StalledNoTodosButTaskInFlight
            | Self::StalledFalseCompletion => true,
            Self::NotStalledComplete
            | Self::NotStalledWaitingOnBackground
            | Self::NotStalledWaitingOnUser => false,
        }
    }

    /// Every variant of this enum. Used by the producer-consistency
    /// tests to enumerate the closed set rather than a hand-coded
    /// array that would silently drift if a new variant were added.
    /// `as_const_str` and `is_stalled` are compiler-enforced
    /// exhaustive matches; the `_assert_exhaustive` helper below is
    /// the cheap drift guard that forces this list to stay in sync
    /// (adding any variant without listing it here is a compile error).
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "exhaustiveness guard for tests; remove expect if used in prod"
        )
    )]
    pub(crate) const fn all() -> &'static [Self] {
        // The exhaustive match in this helper wires the array length
        // to the variant count: adding a variant breaks `match` AND
        // the array's expected count.
        const fn _assert_exhaustive(c: LazinessCategory) {
            match c {
                LazinessCategory::StalledNarration => (),
                LazinessCategory::StalledPermissionAsking => (),
                LazinessCategory::StalledNoTodosButTaskInFlight => (),
                LazinessCategory::StalledFalseCompletion => (),
                LazinessCategory::NotStalledComplete => (),
                LazinessCategory::NotStalledWaitingOnBackground => (),
                LazinessCategory::NotStalledWaitingOnUser => (),
            }
        }
        &[
            Self::StalledNarration,
            Self::StalledPermissionAsking,
            Self::StalledNoTodosButTaskInFlight,
            Self::StalledFalseCompletion,
            Self::NotStalledComplete,
            Self::NotStalledWaitingOnBackground,
            Self::NotStalledWaitingOnUser,
        ]
    }
}

// ── TodoGate discriminator vocabulary ─────────────────────────────────
//
// Source of truth for the `reason` field on `Event::TodoGateFired`.
// Producer wraps these via `TodoGateReason::as_str()` (acp_session.rs).

/// The TodoGate fired because a content-only turn ended with one or more
/// pending or unbacked in-progress todos.
pub const TODO_GATE_IN_FLIGHT: &str = "in_flight";

// Compile-time non-empty check — empty would silently break dashboards' group-by.
#[allow(clippy::const_is_empty)]
const _: () = assert!(
    !TODO_GATE_IN_FLIGHT.is_empty(),
    "TodoGate discriminator consts must be non-empty",
);

/// Map a [`CancellationCategory`] to the [`PriorTurnInterrupt`] marker stamped
/// onto the *next* real user turn — but only for the user-interruption subset.
/// Automatic terminations (doom-loop, hook-denied) return `None`: they are not
/// user interruptions, so the follow-up user message carries no marker.
/// Exhaustive `match` (no wildcard) so a new `CancellationCategory` forces an
/// explicit decision here. (Interjection has no `CancellationCategory` — it
/// never cancels a turn — so it is mapped directly at the drain site.)
pub(crate) fn prior_turn_interrupt_from_cancellation(
    category: CancellationCategory,
) -> Option<sampling_types::PriorTurnInterrupt> {
    use sampling_types::PriorTurnInterrupt;
    match category {
        CancellationCategory::MidTurnAbort => Some(PriorTurnInterrupt::MidTurnAbort),
        CancellationCategory::PermissionRejected => Some(PriorTurnInterrupt::PermissionRejected),
        CancellationCategory::PermissionCancelled => Some(PriorTurnInterrupt::PermissionCancelled),
        CancellationCategory::HookDenied | CancellationCategory::PermissionTimedOut => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prior_turn_interrupt_from_cancellation_maps_user_interrupts_only() {
        use sampling_types::PriorTurnInterrupt;
        // The three user-interrupt causes map to a marker.
        assert_eq!(
            prior_turn_interrupt_from_cancellation(CancellationCategory::MidTurnAbort),
            Some(PriorTurnInterrupt::MidTurnAbort)
        );
        assert_eq!(
            prior_turn_interrupt_from_cancellation(CancellationCategory::PermissionRejected),
            Some(PriorTurnInterrupt::PermissionRejected)
        );
        assert_eq!(
            prior_turn_interrupt_from_cancellation(CancellationCategory::PermissionCancelled),
            Some(PriorTurnInterrupt::PermissionCancelled)
        );
        // Automatic terminations are NOT user interrupts → no marker.
        assert_eq!(
            prior_turn_interrupt_from_cancellation(CancellationCategory::HookDenied),
            None
        );
        assert_eq!(
            prior_turn_interrupt_from_cancellation(CancellationCategory::PermissionTimedOut),
            None
        );
    }

    /// Hand-curated expected pairings — kept here (not in `all()`) so
    /// the test can detect a desync between `as_const_str` and the
    /// `pub const` set. If a variant is added to the enum, `all()`
    /// grows AND `as_const_str` grows (both compiler-enforced); this
    /// table must also be extended, which the `assert_eq!(len)` below
    /// catches.
    const EXPECTED_CATEGORY_CONSTS: &[(LazinessCategory, &str)] = &[
        (
            LazinessCategory::StalledNarration,
            LAZINESS_STALLED_NARRATION,
        ),
        (
            LazinessCategory::StalledPermissionAsking,
            LAZINESS_STALLED_PERMISSION_ASKING,
        ),
        (
            LazinessCategory::StalledNoTodosButTaskInFlight,
            LAZINESS_STALLED_NO_TODOS_BUT_TASK_IN_FLIGHT,
        ),
        (
            LazinessCategory::StalledFalseCompletion,
            LAZINESS_STALLED_FALSE_COMPLETION,
        ),
        (
            LazinessCategory::NotStalledComplete,
            LAZINESS_NOT_STALLED_COMPLETE,
        ),
        (
            LazinessCategory::NotStalledWaitingOnBackground,
            LAZINESS_NOT_STALLED_WAITING_BG,
        ),
        (
            LazinessCategory::NotStalledWaitingOnUser,
            LAZINESS_NOT_STALLED_WAITING_USER,
        ),
    ];

    #[test]
    fn laziness_category_all_covers_every_variant() {
        // Closed-set guard: if a 7th variant were added,
        // `LazinessCategory::all()`'s array would grow (and the
        // exhaustive `_assert_exhaustive` match in the impl would
        // force the array to grow too). The test then forces the
        // expected-table to grow via the length equality, and the
        // per-variant assertions below verify nothing was missed.
        let all = LazinessCategory::all();
        assert_eq!(
            all.len(),
            EXPECTED_CATEGORY_CONSTS.len(),
            "LazinessCategory::all() and EXPECTED_CATEGORY_CONSTS drifted",
        );
        let all_set: std::collections::BTreeSet<&LazinessCategory> = all.iter().collect();
        let expected_set: std::collections::BTreeSet<&LazinessCategory> =
            EXPECTED_CATEGORY_CONSTS.iter().map(|(c, _)| c).collect();
        assert_eq!(
            all_set, expected_set,
            "every variant in `all()` must also appear in EXPECTED_CATEGORY_CONSTS",
        );
    }

    #[test]
    fn laziness_category_round_trip_through_const_str_and_serde() {
        // Single source of truth: every variant maps to one and only
        // one const, and the same const deserializes back to the
        // variant. Drives off `LazinessCategory::all()` so a new
        // variant must be added there (compiler-enforced via
        // `_assert_exhaustive`) for this test to even see it.
        for &(variant, expected_const) in EXPECTED_CATEGORY_CONSTS {
            assert_eq!(variant.as_const_str(), expected_const);
            let json = format!("\"{expected_const}\"");
            let parsed: LazinessCategory =
                serde_json::from_str(&json).expect("deserialize const back to variant");
            assert_eq!(parsed, variant);
        }
        let unique: std::collections::BTreeSet<&'static str> = LazinessCategory::all()
            .iter()
            .map(|c| c.as_const_str())
            .collect();
        assert_eq!(unique.len(), LazinessCategory::all().len());
    }

    #[test]
    fn laziness_stalled_false_completion_const_value_and_round_trip() {
        // Pin the wire string for the new category and prove the
        // const ↔ enum mapping is bijective.
        assert_eq!(
            LAZINESS_STALLED_FALSE_COMPLETION,
            "stalled_false_completion"
        );
        assert_eq!(
            LazinessCategory::StalledFalseCompletion.as_const_str(),
            LAZINESS_STALLED_FALSE_COMPLETION,
        );
        let parsed: LazinessCategory =
            serde_json::from_str("\"stalled_false_completion\"").expect("deserialize");
        assert_eq!(parsed, LazinessCategory::StalledFalseCompletion);
        assert!(LazinessCategory::StalledFalseCompletion.is_stalled());
    }

    #[test]
    fn laziness_is_stalled_matches_stalled_consts_only() {
        // Drive off `all()` so a new variant must be classified as
        // stalled or not-stalled — no variant escapes the test.
        let stalled_consts: std::collections::BTreeSet<&'static str> = [
            LAZINESS_STALLED_NARRATION,
            LAZINESS_STALLED_PERMISSION_ASKING,
            LAZINESS_STALLED_NO_TODOS_BUT_TASK_IN_FLIGHT,
            LAZINESS_STALLED_FALSE_COMPLETION,
        ]
        .into_iter()
        .collect();
        for &variant in LazinessCategory::all() {
            let expected = stalled_consts.contains(variant.as_const_str());
            assert_eq!(
                variant.is_stalled(),
                expected,
                "is_stalled() disagrees with stalled-const set for {variant:?}",
            );
        }
    }

    #[test]
    fn laziness_abort_reason_consts_are_distinct() {
        // Driven off `LazinessAbortReason::all()` so a new variant
        // must be added there (compiler-enforced via the exhaustive
        // match in `as_const_str`) before this test can see it. The
        // closed-set guarantee then lives in code, not in a hand-coded
        // test array. Note: the const array here is preserved
        // separately so a desync between `as_const_str` and the
        // `pub const` set is caught.
        let from_enum: std::collections::BTreeSet<&'static str> =
            crate::session::acp_session::LazinessAbortReason::all()
                .iter()
                .map(|r| r.as_const_str())
                .collect();
        let from_consts: std::collections::BTreeSet<&'static str> = [
            LAZINESS_ABORT_USER_INPUT,
            LAZINESS_ABORT_MODEL_SWITCH,
            LAZINESS_ABORT_TIMEOUT,
            LAZINESS_ABORT_CLASSIFIER_ERROR,
        ]
        .into_iter()
        .collect();
        assert_eq!(
            from_enum, from_consts,
            "LazinessAbortReason variants must map 1:1 to LAZINESS_ABORT_* consts",
        );
    }
}
