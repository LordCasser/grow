//! Per-prompt and per-session usage ledgers.
//!
//! `total_tokens()` is input + output: Responses wire `total` is live context
//! length. Compaction and other side calls use `record_auxiliary_call` instead
//! of incrementing the main-loop turn count.
//!
//! # Completeness ownership
//!
//! Wire incomplete is the OR of these stores (each has a distinct role):
//!
//! - **`UsageLedger.incomplete`** — durable on the usage snapshot. Set by nested
//!   subagent incomplete fold, drain timeout, true apply-miss, and
//!   `mark_usage_incomplete`. Monotonic for a ledger instance.
//! - **Sticky (`subagent_usage_not_applied` on the coordinator)** — pin-scoped
//!   **report** signal (session-only attribution or apply-miss report). Not a
//!   second token sink; does not stain ledgers by itself.
//! - **Foreground live IDs** — fold may still land; freeze drains ≤120s or fails
//!   closed. Cancel skips multi-second drain (actor-loop safety).
//! - **Background live** — never waits; prompt report incomplete immediately;
//!   spend still folds into the session ledger at completion (no session-ledger
//!   incomplete).
//!
//! Freeze and cancel share one outcome policy: ledger marks only on fail-closed;
//! sticky and background_live are report-level only.
//!
//! Projection (`PromptUsage`) never invents tokens; it only ORs completeness
//! and scrubs costs when partial or incomplete.

use indexmap::IndexMap;
use sampling_types::TokenUsage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub reasoning_tokens: u64,
    pub model_calls: u64,
    pub api_duration_ms: u64,
    /// USD ticks (1e10 per USD). Absent when no call reported cost.
    pub cost_usd_ticks: Option<i64>,
    pub cost_missing_calls: u64,
}

/// Agent identity within one [`UsageLedger`]. The owning agent is the agent
/// whose chat-state actor owns the ledger; folded children retain their
/// externally stable subagent IDs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UsageAgent {
    Owner,
    Subagent(String),
}

/// Usage accumulated during one actor incarnation. The first entry represents
/// the initial run; each later entry starts at a durable cold-resume boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageSegment {
    pub resume_event_seq: Option<crate::EventSeq>,
    pub started_at_ms: Option<i64>,
    pub totals: UsageTotals,
    pub by_model: IndexMap<String, UsageTotals>,
    pub by_agent: IndexMap<UsageAgent, UsageTotals>,
    pub main_loop_model_calls: u64,
    pub incomplete: bool,
}

impl UsageSegment {
    fn fold_main_loop_call(&mut self, model_id: &str, call: &UsageTotals) {
        self.main_loop_model_calls = self.main_loop_model_calls.saturating_add(1);
        self.fold_entry(model_id, call);
        self.by_agent
            .entry(UsageAgent::Owner)
            .or_default()
            .fold_totals(call);
    }

    fn fold_auxiliary_call(&mut self, model_id: &str, call: &UsageTotals) {
        self.fold_entry(model_id, call);
        self.by_agent
            .entry(UsageAgent::Owner)
            .or_default()
            .fold_totals(call);
    }

    fn fold_subagent(
        &mut self,
        subagent_id: &str,
        by_model: &[(String, UsageTotals)],
        incomplete: bool,
    ) {
        let mut agent_totals = UsageTotals::default();
        for (model_id, totals) in by_model {
            self.fold_entry(model_id, totals);
            agent_totals.fold_totals(totals);
        }
        self.by_agent
            .entry(UsageAgent::Subagent(subagent_id.to_owned()))
            .or_default()
            .fold_totals(&agent_totals);
        if incomplete {
            self.incomplete = true;
        }
    }

    fn fold_entry(&mut self, model_id: &str, totals: &UsageTotals) {
        self.totals.fold_totals(totals);
        self.by_model
            .entry(model_id.to_owned())
            .or_default()
            .fold_totals(totals);
    }
}

impl UsageTotals {
    fn from_call(
        usage: &TokenUsage,
        api_duration_ms: Option<u64>,
        cost_usd_ticks: Option<i64>,
    ) -> Self {
        let cost_usd_ticks = sampling_types::reported_cost_ticks(cost_usd_ticks);
        Self {
            input_tokens: u64::from(usage.prompt_tokens),
            output_tokens: u64::from(usage.completion_tokens),
            cached_read_tokens: u64::from(usage.cached_prompt_tokens),
            cache_creation_tokens: u64::from(usage.cache_creation_prompt_tokens),
            reasoning_tokens: u64::from(usage.reasoning_tokens),
            model_calls: 1,
            api_duration_ms: api_duration_ms.unwrap_or(0),
            cost_usd_ticks,
            cost_missing_calls: u64::from(cost_usd_ticks.is_none()),
        }
    }

    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    /// Model consumption excluding cache reads; cache creation remains ordinary
    /// input. Goal budgets use full input plus output instead.
    pub fn uncached_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_sub(self.cached_read_tokens)
            .saturating_add(self.output_tokens)
    }

    pub fn cost_is_partial(&self) -> bool {
        self.cost_usd_ticks.is_some() && self.cost_missing_calls > 0
    }

    fn fold_totals(&mut self, other: &UsageTotals) {
        let Self {
            input_tokens,
            output_tokens,
            cached_read_tokens,
            cache_creation_tokens,
            reasoning_tokens,
            model_calls,
            api_duration_ms,
            cost_usd_ticks,
            cost_missing_calls,
        } = other;
        self.input_tokens = self.input_tokens.saturating_add(*input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(*output_tokens);
        self.cached_read_tokens = self.cached_read_tokens.saturating_add(*cached_read_tokens);
        self.cache_creation_tokens = self
            .cache_creation_tokens
            .saturating_add(*cache_creation_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(*reasoning_tokens);
        self.model_calls = self.model_calls.saturating_add(*model_calls);
        self.api_duration_ms = self.api_duration_ms.saturating_add(*api_duration_ms);
        self.cost_missing_calls = self.cost_missing_calls.saturating_add(*cost_missing_calls);
        self.cost_usd_ticks = merge_cost_ticks(self.cost_usd_ticks, *cost_usd_ticks);
    }
}

fn merge_cost_ticks(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0).saturating_add(b.unwrap_or(0))),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageLedger {
    pub totals: UsageTotals,
    pub by_model: IndexMap<String, UsageTotals>,
    /// Known usage by agent. This remains internal accounting; current usage
    /// wire/UI projections intentionally expose only aggregate totals.
    pub by_agent: IndexMap<UsageAgent, UsageTotals>,
    /// Initial actor run followed by cold-resume actor incarnations.
    pub segments: Vec<UsageSegment>,
    /// Main-agent loop rounds for `num_turns` (subagents excluded).
    pub main_loop_model_calls: u64,
    /// Usage may under-count (drain timeout, nested subagent incomplete, apply failure).
    pub incomplete: bool,
}

impl UsageLedger {
    /// Seed the initial segment timestamp during Timeline restoration. Ordinary
    /// fresh ledgers create the same segment lazily on their first mutation.
    pub(crate) fn initialize_segment(&mut self, started_at_ms: Option<i64>) {
        if self.segments.is_empty() {
            self.segments.push(UsageSegment {
                started_at_ms,
                ..Default::default()
            });
        }
    }

    /// Begin the actor incarnation created by one durable cold resume.
    pub(crate) fn begin_resume_segment(&mut self, event_seq: crate::EventSeq, started_at_ms: i64) {
        self.initialize_segment(None);
        self.segments.push(UsageSegment {
            resume_event_seq: Some(event_seq),
            started_at_ms: Some(started_at_ms),
            ..Default::default()
        });
    }

    fn current_segment_mut(&mut self) -> &mut UsageSegment {
        self.initialize_segment(None);
        self.segments.last_mut().expect("usage segment initialized")
    }

    pub(crate) fn current_segment_index(&mut self) -> usize {
        self.initialize_segment(None);
        self.segments.len() - 1
    }

    pub(crate) fn mark_segment_incomplete(&mut self, index: usize) {
        self.incomplete = true;
        if let Some(segment) = self.segments.get_mut(index) {
            segment.incomplete = true;
        }
    }

    /// Fold one main-agent-loop model call. This is the only writer of
    /// `main_loop_model_calls` (the wire `numTurns`); side calls such as
    /// compaction must not use it.
    pub fn record_main_loop_call(
        &mut self,
        model_id: &str,
        usage: &TokenUsage,
        api_duration_ms: Option<u64>,
        cost_usd_ticks: Option<i64>,
    ) {
        let call = UsageTotals::from_call(usage, api_duration_ms, cost_usd_ticks);
        self.main_loop_model_calls = self.main_loop_model_calls.saturating_add(1);
        self.fold_entry(model_id, &call);
        self.by_agent
            .entry(UsageAgent::Owner)
            .or_default()
            .fold_totals(&call);
        self.current_segment_mut()
            .fold_main_loop_call(model_id, &call);
    }

    /// Fold one auxiliary provider request under this session's owner without
    /// making it a main-loop round (`numTurns`). A child session includes its
    /// own auxiliary calls before its final bill is folded by the parent.
    pub fn record_auxiliary_call(
        &mut self,
        model_id: &str,
        usage: &TokenUsage,
        api_duration_ms: Option<u64>,
        cost_usd_ticks: Option<i64>,
    ) {
        let segment_index = self.current_segment_index();
        self.record_auxiliary_call_at(
            segment_index,
            model_id,
            usage,
            api_duration_ms,
            cost_usd_ticks,
        );
    }

    pub(crate) fn record_auxiliary_call_at(
        &mut self,
        segment_index: usize,
        model_id: &str,
        usage: &TokenUsage,
        api_duration_ms: Option<u64>,
        cost_usd_ticks: Option<i64>,
    ) {
        let call = UsageTotals::from_call(usage, api_duration_ms, cost_usd_ticks);
        self.fold_entry(model_id, &call);
        self.by_agent
            .entry(UsageAgent::Owner)
            .or_default()
            .fold_totals(&call);
        self.segments[segment_index].fold_auxiliary_call(model_id, &call);
    }

    /// Fold subagent usage without incrementing `main_loop_model_calls`.
    pub fn record_subagent(
        &mut self,
        subagent_id: &str,
        by_model: &[(String, UsageTotals)],
        incomplete: bool,
    ) {
        let agent = UsageAgent::Subagent(subagent_id.to_owned());
        let mut agent_totals = UsageTotals::default();
        for (model_id, totals) in by_model {
            self.fold_entry(model_id, totals);
            agent_totals.fold_totals(totals);
        }
        self.by_agent
            .entry(agent)
            .or_default()
            .fold_totals(&agent_totals);
        if incomplete {
            self.incomplete = true;
        }
        self.current_segment_mut()
            .fold_subagent(subagent_id, by_model, incomplete);
    }

    pub fn mark_incomplete(&mut self) {
        self.incomplete = true;
        self.current_segment_mut().incomplete = true;
    }

    fn fold_entry(&mut self, model_id: &str, totals: &UsageTotals) {
        self.totals.fold_totals(totals);
        self.by_model
            .entry(model_id.to_owned())
            .or_default()
            .fold_totals(totals);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tu(prompt: u32, completion: u32) -> TokenUsage {
        TokenUsage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: 999_999,
            reasoning_tokens: 0,
            cached_prompt_tokens: 0,
            cache_creation_prompt_tokens: 0,
        }
    }

    #[test]
    fn ledger_sums_partial_subagent_and_zero_cost() {
        let mut ledger = UsageLedger::default();
        ledger.record_main_loop_call("m", &tu(1, 1), None, Some(0));
        assert_eq!(ledger.totals.cost_usd_ticks, None);
        assert_eq!(ledger.totals.cost_missing_calls, 1);

        ledger.record_main_loop_call("a", &tu(100, 10), Some(100), None);
        ledger.record_main_loop_call("a", &tu(50, 5), Some(50), Some(70));
        assert_eq!(ledger.totals.cost_usd_ticks, Some(70));
        assert!(ledger.totals.cost_is_partial());
        assert_eq!(ledger.main_loop_model_calls, 3);

        ledger.record_subagent(
            "child-1",
            &[(
                "b".into(),
                UsageTotals {
                    input_tokens: 5,
                    model_calls: 1,
                    ..Default::default()
                },
            )],
            false,
        );
        assert_eq!(ledger.by_model["b"].input_tokens, 5);
        assert_eq!(ledger.main_loop_model_calls, 3);
        assert_eq!(ledger.totals.model_calls, 4);
        assert_eq!(ledger.by_agent[&UsageAgent::Owner].total_tokens(), 167);
        assert_eq!(
            ledger.by_agent[&UsageAgent::Subagent("child-1".into())].input_tokens,
            5
        );

        ledger.record_subagent(
            "child-2",
            &[
                (
                    "b".into(),
                    UsageTotals {
                        input_tokens: 7,
                        output_tokens: 3,
                        ..Default::default()
                    },
                ),
                (
                    "c".into(),
                    UsageTotals {
                        input_tokens: 2,
                        output_tokens: 1,
                        ..Default::default()
                    },
                ),
            ],
            false,
        );
        assert_eq!(
            ledger.by_agent[&UsageAgent::Subagent("child-2".into())].total_tokens(),
            13
        );
        assert_eq!(ledger.totals.total_tokens(), 185);
        assert!(!ledger.incomplete);

        ledger.record_subagent("child-3", &[], true);
        assert!(ledger.incomplete);
        assert!(
            ledger
                .by_agent
                .contains_key(&UsageAgent::Subagent("child-3".into()))
        );
    }

    #[test]
    fn resume_segments_partition_the_lifetime_ledger() {
        let mut ledger = UsageLedger::default();
        ledger.record_main_loop_call("a", &tu(10, 2), None, Some(1));
        ledger.record_subagent(
            "child-1",
            &[(
                "b".into(),
                UsageTotals {
                    input_tokens: 5,
                    output_tokens: 1,
                    model_calls: 1,
                    cost_usd_ticks: Some(2),
                    ..Default::default()
                },
            )],
            false,
        );
        ledger.begin_resume_segment(crate::EventSeq::new(7), 123);
        ledger.record_main_loop_call("a", &tu(3, 1), None, Some(3));
        ledger.mark_incomplete();

        assert_eq!(ledger.segments.len(), 2);
        assert_eq!(ledger.segments[0].totals.total_tokens(), 18);
        assert_eq!(ledger.segments[1].totals.total_tokens(), 4);
        assert!(ledger.segments[1].incomplete);
        assert_eq!(
            ledger.segments[1].resume_event_seq,
            Some(crate::EventSeq::new(7))
        );
        assert_eq!(
            ledger
                .segments
                .iter()
                .map(|segment| segment.totals.total_tokens())
                .sum::<u64>(),
            ledger.totals.total_tokens()
        );
    }

    #[test]
    fn auxiliary_calls_charge_owner_without_incrementing_main_rounds() {
        let mut ledger = UsageLedger::default();
        ledger.record_main_loop_call("primary", &tu(10, 2), None, None);
        ledger.record_auxiliary_call("recap", &tu(7, 3), Some(25), Some(100));
        assert_eq!(ledger.totals.total_tokens(), 22);
        assert_eq!(ledger.by_model["recap"].total_tokens(), 10);
        assert_eq!(ledger.by_agent[&UsageAgent::Owner].total_tokens(), 22);
        assert_eq!(ledger.main_loop_model_calls, 1);
        assert_eq!(ledger.totals.model_calls, 2);
        assert_eq!(ledger.segments[0].main_loop_model_calls, 1);
        assert_eq!(ledger.segments[0].totals.total_tokens(), 22);
    }

    #[test]
    fn child_final_bill_folds_its_auxiliary_calls_once_into_parent() {
        let mut child = UsageLedger::default();
        child.record_main_loop_call("main", &tu(10, 2), None, None);
        child.record_auxiliary_call("recap", &tu(7, 3), None, None);
        let bill = child
            .by_model
            .iter()
            .map(|(model, totals)| (model.clone(), totals.clone()))
            .collect::<Vec<_>>();

        let mut parent = UsageLedger::default();
        parent.record_subagent("child-1", &bill, child.incomplete);
        assert_eq!(parent.totals.total_tokens(), 22);
        assert_eq!(parent.totals.model_calls, 2);
        assert_eq!(parent.main_loop_model_calls, 0);
        assert_eq!(
            parent.by_agent[&UsageAgent::Subagent("child-1".into())].total_tokens(),
            22
        );
        assert_eq!(parent.by_model["recap"].total_tokens(), 10);
    }
}
