use agent_client_protocol as acp;
use tools::types::TaskSnapshot;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowPhaseInfo {
    pub title: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkflowAgentInfo {
    pub agent_id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub state: String,
    #[serde(default)]
    pub tokens_used: u64,
    #[serde(default)]
    pub duration_ms: u64,
}

/// Grow-specific session notification (parallel to acp::SessionNotification)
/// This wraps an GrowSessionUpdate with session context for persistence and replay.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNotification {
    /// The ID of the session this update pertains to.
    pub session_id: acp::SessionId,
    /// The actual update content.
    pub update: SessionUpdate,
    /// Extension point for implementations
    #[serde(skip_serializing_if = "Option::is_none", rename = "_meta")]
    pub meta: Option<serde_json::Value>,
}

/// The client projection uses the same immutable turn ownership identity as
/// Timeline; it does not define a second terminal schema.
pub use chat_state::TurnIdentity;

/// Wire usage for ACP `_meta.usage` and `TurnCompleted.usage`.
///
/// # Wire contract (ACP vs headless)
///
/// | Surface | `input_tokens` / `inputTokens` | Cost |
/// |---------|--------------------------------|------|
/// | **ACP** (`PromptUsage`) | **Full** prompt sum (includes cache reads) | `costUsdTicks` (1e10 ticks = $1), scrubbed when partial/incomplete |
/// | **Headless** ([`project_result_usage`]) | **Uncached only** (`full − cache_read`) | Float `total_cost_usd` + exact `total_cost_usd_ticks`, only when complete |
/// | ACP `_meta` sibling fields | **Last model call only** (not whole-prompt) | — |
///
/// Trust cost only when present **and** not `usageIsIncomplete` **and** not
/// `costIsPartial`. Absence of cost means untrustworthy or unknown — not free.
///
/// Mixed headless shape is frozen for external-tool compatibility: snake_case
/// on totals (`usage.input_tokens`, `total_cost_usd`) and camelCase under
/// `modelUsage` (`inputTokens`, `costUSD`). Per-model rows are a reduced
/// schema (no reasoning/duration on the wire).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptUsage {
    #[serde(flatten)]
    pub totals: PromptUsageModel,
    #[serde(
        default,
        rename = "modelUsage",
        skip_serializing_if = "indexmap::IndexMap::is_empty"
    )]
    pub model_usage: indexmap::IndexMap<String, PromptUsageModel>,
    /// Main-agent loop rounds (same unit as `--max-turns`).
    #[serde(default, rename = "numTurns")]
    pub num_turns: u64,
    /// Bill may under-count (open subagents, usage not applied, or drain timeout).
    #[serde(
        default,
        rename = "usageIsIncomplete",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub usage_is_incomplete: bool,
}

impl PromptUsage {
    /// Project a ledger snapshot for the wire. Returns `Some` whenever
    /// `incomplete` is set — even if `ledger` is `None` — so the flag is never
    /// dropped by omission. Always scrubs untrustworthy costs.
    pub fn project_from_ledger(
        ledger: Option<&chat_state::UsageLedger>,
        incomplete: bool,
    ) -> Option<Self> {
        let mut usage = match ledger {
            Some(ledger) => {
                let mut usage = Self::from(ledger);
                if incomplete {
                    usage.usage_is_incomplete = true;
                }
                usage
            }
            None if incomplete => Self {
                usage_is_incomplete: true,
                ..Default::default()
            },
            None => return None,
        };
        usage.scrub_untrustworthy_costs();
        Some(usage)
    }

    /// Error-path attach: any open ledger is always incomplete (may under-count
    /// without a freeze drain). `may_undercount` only matters when the ledger is empty.
    pub fn for_error_path(
        ledger: Option<&chat_state::UsageLedger>,
        may_undercount: bool,
    ) -> Option<Self> {
        match (ledger, may_undercount) {
            (Some(l), _) => Self::project_from_ledger(Some(l), true),
            (None, true) => Self::project_from_ledger(None, true),
            (None, false) => None,
        }
    }

    /// Drop cost ticks when partial or incomplete so all wire surfaces fail closed.
    /// Incomplete bills clear ticks even when `cost_is_partial` is false.
    pub fn scrub_untrustworthy_costs(&mut self) {
        if !(self.usage_is_incomplete || self.totals.cost_is_partial) {
            return;
        }
        self.totals.cost_usd_ticks = None;
        for m in self.model_usage.values_mut() {
            m.cost_usd_ticks = None;
            if self.totals.cost_is_partial {
                m.cost_is_partial = true;
            }
        }
    }

    fn is_token_empty(&self) -> bool {
        // Exhaustive destructure: a new token field must decide whether it
        // counts as "billed something" here.
        let PromptUsageModel {
            input_tokens,
            output_tokens,
            total_tokens: _, // derived from input + output
            cached_read_tokens,
            cache_creation_tokens, // subset of input_tokens on the wire
            reasoning_tokens: _,   // subset of output_tokens
            model_calls,
            api_duration_ms: _, // timing, not tokens
            cost_usd_ticks: _,  // cost without usage cannot occur
            cost_is_partial: _,
            cost_missing_calls: _,
        } = self.totals;
        model_calls == 0
            && input_tokens == 0
            && output_tokens == 0
            && cached_read_tokens == 0
            && cache_creation_tokens == 0
            && self.model_usage.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptUsageModel {
    /// Full prompt input tokens including cache reads (ACP identity).
    /// Headless projects uncached only — see [`project_result_usage`].
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cached_read_tokens: u64,
    /// Cache-creation prompt tokens, folded into `input_tokens` on the ACP wire
    /// but projected as a disjoint bucket in the headless shape.
    #[serde(default)]
    pub cache_creation_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub model_calls: u64,
    #[serde(default)]
    pub api_duration_ms: u64,
    /// Server cost in USD ticks (`USD_TICKS_PER_USD` = 1e10 ticks per $1).
    /// Absent when scrubbed, missing, or zero on the wire. Headless projects
    /// the totals as float `total_cost_usd` (plus exact `total_cost_usd_ticks`)
    /// and per-model rows as float `costUSD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd_ticks: Option<i64>,
    /// Some folded calls lacked cost, so any cost shown is a partial sum.
    /// After a scrub of a partial bill, complete per-model rows are also
    /// stamped `true`: the flag means "do not trust this row's cost", not
    /// "this row's own cost was partial".
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cost_is_partial: bool,
    /// How many calls reported usage but no cost. Internal accounting for
    /// `cost_is_partial` only — never on the public ACP wire.
    #[serde(default, skip_serializing)]
    pub cost_missing_calls: u64,
}

/// One model call's token usage: the four Messages API `message.usage` fields
/// (`input_tokens` is the uncached prompt portion) plus `reasoning_tokens`.
/// Distinct from [`PromptUsageModel`], which sums the whole prompt.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResponseUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
}

impl From<&chat_state::UsageTotals> for PromptUsageModel {
    fn from(t: &chat_state::UsageTotals) -> Self {
        // Exhaustive destructure: a new ledger field cannot silently miss the
        // wire. When one is added here, also extend `project_result_usage`.
        let chat_state::UsageTotals {
            input_tokens,
            output_tokens,
            cached_read_tokens,
            cache_creation_tokens,
            reasoning_tokens,
            model_calls,
            api_duration_ms,
            cost_usd_ticks,
            cost_missing_calls,
        } = *t;
        Self {
            input_tokens,
            output_tokens,
            total_tokens: t.total_tokens(),
            cached_read_tokens,
            cache_creation_tokens,
            reasoning_tokens,
            model_calls,
            api_duration_ms,
            cost_usd_ticks,
            cost_is_partial: t.cost_is_partial(),
            cost_missing_calls,
        }
    }
}

impl From<&chat_state::UsageLedger> for PromptUsage {
    fn from(ledger: &chat_state::UsageLedger) -> Self {
        let mut usage = Self {
            totals: PromptUsageModel::from(&ledger.totals),
            model_usage: ledger
                .by_model
                .iter()
                .map(|(k, v)| (k.clone(), PromptUsageModel::from(v)))
                .collect(),
            num_turns: ledger.main_loop_model_calls,
            usage_is_incomplete: ledger.incomplete,
        };
        usage.scrub_untrustworthy_costs();
        usage
    }
}

/// Server cost scale: 1 USD = 10^10 ticks. ACP exposes ticks; headless converts to float USD.
pub const USD_TICKS_PER_USD: f64 = 1e10;

/// Convert server cost ticks to float USD (headless only).
pub fn ticks_to_usd(ticks: i64) -> f64 {
    ticks as f64 / USD_TICKS_PER_USD
}

/// Full ACP input → headless uncached input (`full − cache_read`).
pub fn uncached_input_tokens(full_input: u64, cached_read: u64) -> u64 {
    full_input.saturating_sub(cached_read)
}

/// Project usage onto a headless result object.
///
/// - `usage.input_tokens` = uncached (`full − cache_read − cache_creation`), so
///   the three prompt buckets are disjoint; identity
///   `input_tokens + cache_read + cache_creation + output = total_tokens`.
/// - Omits all cost floats when partial or incomplete (absence ≠ free).
/// - Incomplete with no tokens emits only `usage_is_incomplete` (no zero usage object).
/// - `modelUsage` rows are a reduced external-compat schema (camelCase; no reasoning/duration).
pub fn project_result_usage(result: &mut serde_json::Value, usage: &PromptUsage) {
    if usage.usage_is_incomplete && usage.is_token_empty() {
        result["usage_is_incomplete"] = true.into();
        return;
    }

    // Exhaustive destructure: a new wire field is a compile error until it is
    // either projected or named as deliberately dropped from the headless shape.
    let PromptUsageModel {
        input_tokens,
        output_tokens,
        total_tokens,
        cached_read_tokens,
        cache_creation_tokens,
        reasoning_tokens,
        model_calls: _,     // totals-level; headless carries num_turns instead
        api_duration_ms: _, // dropped: not part of the frozen headless shape
        cost_usd_ticks,
        cost_is_partial,
        cost_missing_calls: _, // internal partiality count; the flag suffices
    } = usage.totals;
    result["usage"] = serde_json::json!({
        "input_tokens": uncached_input_tokens(input_tokens, cached_read_tokens)
            .saturating_sub(cache_creation_tokens),
        "cache_read_input_tokens": cached_read_tokens,
        "cache_creation_input_tokens": cache_creation_tokens,
        "output_tokens": output_tokens,
        "reasoning_tokens": reasoning_tokens,
        "total_tokens": total_tokens,
    });
    result["num_turns"] = usage.num_turns.into();
    if usage.usage_is_incomplete {
        result["usage_is_incomplete"] = true.into();
    }
    let hide_costs = cost_is_partial || usage.usage_is_incomplete;
    if hide_costs {
        if cost_is_partial {
            result["cost_is_partial"] = true.into();
        }
    } else if let Some(ticks) = cost_usd_ticks {
        result["total_cost_usd"] = serde_json::json!(ticks_to_usd(ticks));
        // Exact integer ticks beside the float, under the same trust gate:
        // reconciliation sums ticks exactly, which floats cannot guarantee.
        result["total_cost_usd_ticks"] = serde_json::json!(ticks);
    }
    if !usage.model_usage.is_empty() {
        let mut model_usage = serde_json::Map::new();
        for (name, m) in &usage.model_usage {
            let PromptUsageModel {
                input_tokens,
                output_tokens,
                total_tokens: _, // derivable per row
                cached_read_tokens,
                cache_creation_tokens,
                reasoning_tokens: _, // dropped: reduced per-model schema
                model_calls,
                api_duration_ms: _, // dropped: reduced per-model schema
                cost_usd_ticks,
                cost_is_partial,
                cost_missing_calls: _,
            } = *m;
            let mut entry = serde_json::json!({
                "inputTokens": uncached_input_tokens(input_tokens, cached_read_tokens)
                    .saturating_sub(cache_creation_tokens),
                "outputTokens": output_tokens,
                "cacheReadInputTokens": cached_read_tokens,
                "cacheCreationInputTokens": cache_creation_tokens,
                "modelCalls": model_calls,
            });
            if !hide_costs
                && let Some(ticks) = cost_usd_ticks
                && !cost_is_partial
            {
                entry["costUSD"] = serde_json::json!(ticks_to_usd(ticks));
            }
            model_usage.insert(name.clone(), entry);
        }
        result["modelUsage"] = model_usage.into();
    }
}

/// Fail-closed attach for headless results: parse failure becomes
/// `usage_is_incomplete` (never omit silently — absence must not look free).
pub fn attach_result_usage_fail_closed(result: &mut serde_json::Value, usage: &serde_json::Value) {
    match serde_json::from_value::<PromptUsage>(usage.clone()) {
        Ok(parsed) => project_result_usage(result, &parsed),
        Err(e) => {
            tracing::warn!(
                error = %e,
                "headless: _meta.usage failed to parse; marking usage_is_incomplete"
            );
            result["usage_is_incomplete"] = true.into();
        }
    }
}

/// Status of a single hook run (wire format).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum HookRunStatusDto {
    Success { elapsed_ms: u64 },
    Skipped,
    Blocked { detail: String, elapsed_ms: u64 },
    Failed { error: String, elapsed_ms: u64 },
}

/// A single hook run entry (wire format).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HookRunEntryDto {
    pub name: String,
    pub status: HookRunStatusDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

/// Why auto-compaction stopped before completing.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    strum::Display,
    strum::EnumString,
    strum::AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AutoCompactCancelReason {
    UserCancelled,
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubagentPermissionOutcome {
    Approved,
    Denied,
    TimedOut,
    Unavailable,
    Cancelled,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "sessionUpdate", deny_unknown_fields)]
pub enum SessionUpdate {
    /// A diff review request containing one or more file diffs for user review.
    DiffReview {
        /// The diff content to be reviewed.
        content: Vec<DiffContent>,
    },
    /// Notification that a retry is in progress due to a transient error.
    RetryState(RetryState),
    /// Auto-compact is starting due to context window threshold
    AutoCompactStarted {
        /// Current token usage
        tokens_used: u64,
        /// Total context window size
        context_window: u64,
        /// Percentage used (e.g., 82)
        percentage: u8,
        /// Reason for compaction
        reason: String,
    },
    /// Auto-compact completed successfully
    AutoCompactCompleted {
        /// Tokens used before compaction.
        tokens_before: u64,
        /// Tokens used after compaction
        tokens_after: u64,
        /// How long the compaction took (milliseconds)
        #[serde(skip_serializing_if = "Option::is_none")]
        elapsed_ms: Option<i64>,
        /// Summary preview (first ~100 chars of summary)
        summary_preview: Option<String>,
    },
    /// Auto-compact failed
    AutoCompactFailed {
        /// Error message
        error: String,
    },
    /// Memory flush is starting before compaction
    MemoryFlushStarted,
    /// Memory flush completed
    MemoryFlushCompleted {
        /// Outcome description
        result: String,
        /// Path to the written memory file (if any)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    /// Memory dream consolidation completed
    MemoryDreamCompleted {
        /// Outcome description
        result: String,
        /// Path to the written memory file (if any)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    /// Session-end memory save completed
    MemorySessionSaved {
        /// Path to the written session log
        path: String,
    },
    /// Auto-compact was cancelled (user pressed Ctrl+C)
    AutoCompactCancelled {
        /// Reason for cancellation
        reason: AutoCompactCancelReason,
    },
    /// Auto-continue completed after compaction
    /// This signals the TUI to flush pending agent messages and end the turn
    AutoContinueCompleted {
        /// Total tokens used after auto-continue
        total_tokens: u64,
    },
    /// Auto-recovery is starting after a prompt failure (e.g. remote/workspace recovery)
    AutoRecoveryStarted {
        /// Current recovery attempt number (1-indexed)
        attempt: u32,
        /// Maximum number of recovery attempts allowed
        max_retries: u32,
        /// The error that triggered recovery
        error: String,
        /// Delay in milliseconds before the retry
        delay_ms: u64,
    },
    /// Auto-recovery exhausted all retries and the turn is failing
    AutoRecoveryExhausted {
        /// Total attempts made
        attempts: u32,
        /// The final error message
        error: String,
    },
    /// A hook annotation message for the TUI scrollback.
    /// Rendered inline with the preceding tool call block.
    HookAnnotation {
        /// The hook message to display (e.g., "🪝 Running post_tool_use hooks for `Edit`...")
        message: String,
    },
    /// Structured hook execution data attached to tool call blocks.
    HookExecution {
        /// The hook event name ("pre_tool_use" or "post_tool_use").
        event_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        /// The prompt turn this batch belongs to, when known; lets the
        /// client keep a delayed `stop`/`stop_failure` batch off the wrong
        /// turn's marker.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_id: Option<String>,
        runs: Vec<HookRunEntryDto>,
    },
    /// Hooks registry changed (after reload or trust/untrust).
    /// Sent so the pager modal can auto-refresh if open.
    HooksChanged {
        hooks: Vec<extension_types::HookInfo>,
        project_trusted: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        load_errors: Vec<String>,
    },
    /// Plugins registry changed (after reload).
    /// Sent so the pager modal can auto-refresh if open.
    PluginsChanged {
        plugins: Vec<extension_types::PluginInfo>,
    },
    /// Marketplace plugin updates were auto-installed on session start.
    /// Sent so the pager or another ACP client can notify the user.
    PluginUpdatesInstalled {
        /// List of (plugin_name, old_version, new_version).
        updates: Vec<(String, String, String)>,
    },
    /// A short "where was I" recap of the session so far.
    ///
    /// Emitted by the `grow/recap` ext method: on demand via the `/recap`
    /// slash command (`auto = false`), or automatically when the user
    /// returns to the terminal after being away (`auto = true`). The pager
    /// renders it as an informational scrollback line; it is never added to
    /// the model conversation.
    SessionRecap {
        /// The one-line recap text (~25–40 words; capped at a generous safety
        /// limit, so a normal recap is shown in full).
        summary: String,
        /// `true` when generated automatically on return-from-away,
        /// `false` for an explicit `/recap`.
        #[serde(default)]
        auto: bool,
    },
    /// A manual `/recap` produced no recap — no assistant turns yet, a failed
    /// prepare/model call, or an empty summary. The pager shows a loading
    /// spinner for `/recap`, so without this signal that spinner would animate
    /// forever; on receipt the pager clears it. Never emitted for an automatic
    /// recap (those show no spinner).
    SessionRecapUnavailable,
    /// A rewind marker written to `updates.jsonl` when a rewind occurs.
    ///
    /// This is **persist-only** — it is never sent to the gateway/UI. Because
    /// `updates.jsonl` is append-only, rewinding creates a timeline branch.
    /// The marker tells the replay algorithm to discard accumulated state
    /// beyond `target_prompt_index` and continue from that point.
    RewindMarker {
        /// The prompt index being rewound to (0-based).
        target_prompt_index: usize,
        /// When the rewind occurred.
        created_at: String,
    },
    /// Task completed notification
    TaskCompleted { task_snapshot: TaskSnapshot },
    /// A subagent session has been spawned.
    ///
    /// Sent on the PARENT session's notification channel so the client
    /// knows this `child_session_id` is a subagent and can route its events.
    /// Emitted BEFORE dispatching `SessionCommand::QueuePrompt` to the child,
    /// preventing a race where child events arrive before the client has
    /// the session ID mapping.
    SubagentSpawned {
        /// Unique subagent identifier (same as child session ID).
        subagent_id: String,
        /// The parent session that spawned this subagent.
        parent_session_id: String,
        /// The parent prompt/turn that spawned this subagent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_prompt_id: Option<String>,
        /// The child session's ACP session ID.
        child_session_id: String,
        /// Agent type used for the subagent ("general-purpose", "explore", "plan", or custom).
        subagent_type: String,
        /// Short human-readable description of the task.
        description: String,
        /// Effective context source after bootstrap: "new" or "resumed".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effective_context_source: Option<String>,
        /// Whether the forked context was normalized into <background_context>.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        context_normalized: bool,
        /// Capability mode applied to this subagent (e.g. "read-only").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capability_mode: Option<String>,
        /// Independent permission decision route used by this child.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permission_mode: Option<String>,
        /// Effective permission mode after resolving `follow` and applying
        /// managed-policy clamps at spawn time. Each request still resolves
        /// `follow` again against the live parent mode.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effective_permission_mode: Option<String>,
        /// Effective model ID used by the subagent (may differ from the parent).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        /// ID of the source subagent this session was resumed from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resumed_from: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workflow_run_id: Option<String>,
        /// Goal that owned this child at request creation. This is stamped by
        /// the producer; consumers must not infer it from current Goal state.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        goal_id: Option<String>,
    },
    /// Final authorization outcome for a permission requested by a child
    /// session. This is a durable UI/audit projection only; it is never added
    /// to either the primary or child model conversation.
    SubagentPermissionDecision {
        child_session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subagent_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        tool_call_id: String,
        tool_name: String,
        access_kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        access_summary: Option<String>,
        /// Full unredacted request detail for a live UI notification. The
        /// permission audit bridge clears this field in the durable copy.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        access_detail: Option<String>,
        outcome: SubagentPermissionOutcome,
        source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// Full classifier explanation for the live detail modal. Never
        /// persisted by the permission audit bridge.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        classifier_reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        latency_ms: Option<u64>,
    },
    /// Periodic progress update for a running subagent.
    ///
    /// Sent on the PARENT session's notification channel at a rate-limited
    /// cadence (every ~2s while the subagent is active). Stops automatically
    /// when the subagent completes or is cancelled. The TUI merges these
    /// into the same state path used by ACP poll responses.
    SubagentProgress {
        /// Unique subagent identifier.
        subagent_id: String,
        /// The parent session that owns this subagent.
        parent_session_id: String,
        /// The child session's ACP session ID.
        child_session_id: String,
        /// Elapsed wall-clock time in milliseconds.
        duration_ms: u64,
        /// Number of completed turns so far.
        turn_count: u32,
        /// Total tool calls executed so far.
        tool_call_count: u32,
        /// Current tokens used in the context window.
        tokens_used: u64,
        /// Total context window capacity (tokens).
        context_window_tokens: u64,
        /// Context window usage as a percentage (0-100).
        context_usage_pct: u8,
        /// Distinct tool names called so far.
        tools_used: Vec<String>,
        /// Number of errors encountered so far.
        error_count: u32,
    },
    /// A subagent session has finished (success, failure, or cancellation).
    ///
    /// Sent on the PARENT session's notification channel.
    SubagentFinished {
        /// Unique subagent identifier.
        subagent_id: String,
        /// The child session's ACP session ID.
        child_session_id: String,
        /// Outcome: "completed", "failed", or "cancelled".
        status: String,
        /// Error message if the subagent failed.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        /// Number of tool calls made by the subagent.
        tool_calls: u32,
        /// Number of conversation turns taken by the subagent.
        turns: u32,
        /// Total wall-clock duration in milliseconds.
        duration_ms: u64,
        /// Total tokens consumed by the subagent's context window.
        tokens_used: u64,
        /// Final output text from the subagent (if completed).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    /// Task backgrounded notification — a bash command transitioned to background execution.
    /// Sent for both direct `is_background=true` tasks and foreground→background transitions.
    TaskBackgrounded {
        /// The tool_call_id of the bash tool invocation.
        tool_call_id: String,
        /// The background task registry ID.
        task_id: String,
        /// The shell command being executed.
        command: String,
        /// Absolute path of the working directory.
        cwd: String,
        /// Absolute path to the output log file on disk.
        output_file: String,
        /// For monitor tasks: the monitor's human-readable description.
        /// `None` for ordinary backgrounded bash commands. Lets the pager
        /// render monitors with a "Monitor" tag instead of bash-highlighting
        /// the command string.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        monitor_description: Option<String>,
        /// Model-supplied tool `description` for ordinary bash bg tasks
        /// (e.g. "Wait for the server to start"). Prefer over raw `command`
        /// in the pager "Task started" line / tasks pane. `None` when omitted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    ScheduledTaskCreated {
        task_id: String,
        prompt: String,
        human_schedule: String,
        next_fire_at: Option<String>,
    },
    ScheduledTaskFired {
        task_id: String,
        prompt: String,
        human_schedule: String,
        next_fire_at: Option<String>,
        subagent_id: String,
    },
    /// A scheduled task was deleted/cancelled.
    ScheduledTaskDeleted { task_id: String },
    /// A monitor event (stdout line from a monitor background process).
    MonitorEvent {
        task_id: String,
        description: String,
        /// Raw event text (NOT XML-wrapped -- for pager stdout display).
        event_text: String,
    },
    /// The session's model was auto-switched because the persisted model
    /// is no longer available for this user.
    ModelAutoSwitched {
        /// The model ID that was persisted in the session but is no longer available.
        previous_model_id: String,
        /// The model ID that was selected as a replacement.
        new_model_id: String,
        /// Human-readable reason for the switch.
        reason: String,
    },
    /// The session's model was switched via `session/setModel`.
    ///
    /// Broadcast to every client subscribed to the session in leader mode so
    /// follower clients (TUI / IDE / web) mirror the change in their local
    /// state — status bar, `/model` dropdown, prompt header, etc. The
    /// originating client also receives this (the leader broadcasts to all
    /// subscribers of the session) but skips applying it because its in-flight
    /// `SetSessionModel` response is the authority for its local state and
    /// drives the single "Switched to X" scrollback entry. Followers gate on
    /// their own `model_switch_pending` flag to distinguish "I'm waiting on
    /// my own switch" from "someone else's switch arrived."
    ModelChanged {
        /// The newly-selected model id (catalog key).
        model_id: String,
        /// Effective reasoning effort, post-resolution. `None` when the model
        /// does not support reasoning effort or no effort override was applied.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_effort: Option<String>,
    },
    /// The session's prompt Agent changed independently of its model and
    /// permission mode.
    AgentChanged { agent_name: String },
    /// Streaming chunk of a tool call's arguments.
    ///
    /// Behaves like `acp::SessionUpdate::AgentMessageChunk` /
    /// `AgentThoughtChunk`: flows through the replay buffer, gets merged
    /// with adjacent chunks for the same `tool_call_id`, and is debounced
    /// at the session's buffering interval.
    /// Only persisted as a full `acp::SessionUpdate::ToolCall`.
    ToolCallDeltaChunk {
        /// Stable model-provided id (e.g. `"call_abc"`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<String>,
        /// Positional index assigned within the assistant tool calls.
        tool_index: u32,
        /// Tool name (e.g. `"search_replace"`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Raw JSON-fragment string. NOT valid JSON in isolation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arguments_delta: Option<String>,
    },
    /// One or more prompt images were resized to fit within API limits.
    ImageCompressed {
        images: Vec<ImageCompressedEntry>,
        /// Human-readable summary for display.
        message: String,
    },
    /// Prompt images dropped before send (integrity / upscale-cap). The
    /// model is told via a system-reminder; this surfaces them to the UI.
    ImageDropped { notes: Vec<String> },
    /// Memory file listing for the pager's /memory modal.
    MemoryFiles { files: Vec<MemoryFileInfo> },
    WorkflowUpdated {
        run_id: String,
        #[serde(default)]
        private: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        definition_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        definition_scope: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        definition_hash: Option<String>,
        #[serde(default)]
        revision: u64,
        name: String,
        objective: String,
        status: String,
        #[serde(default)]
        foreground: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        phases: Vec<WorkflowPhaseInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        current_phase: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_budget: Option<u64>,
        #[serde(default)]
        agents_used: u64,
        #[serde(default)]
        agents_reserved: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agents_remaining: Option<u64>,
        #[serde(default)]
        agent_usage_incomplete: bool,
        elapsed_ms: u64,
        #[serde(default)]
        active_agents: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        current_agent_label: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        agents: Vec<WorkflowAgentInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_event: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_event_detail: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_event_timestamp: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pause_message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result_summary: Option<String>,
    },
    /// Goal mode orchestration progress update.
    ///
    /// Sent on the parent session's notification channel at phase transitions
    /// and rate-limited from the progress handler (max 1/s). Fire-and-forget
    /// to pager — not actionable.
    GoalUpdated {
        goal_id: String,
        objective: String,
        objective_revision: u64,
        /// `"active"`, `"paused"`, `"blocked"`, `"budget_limited"`,
        /// `"complete"`, or the one-shot removal signal `"cleared"`.
        status: String,
        /// `"planning"`, `"executing"`, `"verifying"`, `"summarizing"`.
        phase: String,
        plan_revision: u64,
        board_revision: u64,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tasks: Vec<tool_types::GoalTaskProjection>,
        plan_markdown: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        verifier_feedback: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        token_budget: Option<i64>,
        #[serde(default)]
        tokens_used: i64,
        elapsed_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        current_subagent_role: Option<String>,
        total_worker_rounds: u32,
        total_verify_rounds: u32,
        #[serde(default)]
        token_baseline: i64,
        #[serde(default)]
        finished_subagent_tokens: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        live_subagent_tokens: Option<u64>,
        /// Per-model marginal-token breakdown `(model_id, tokens)`, sorted
        /// by tokens descending. The producer (`build_goal_updated`) only
        /// populates this when ≥2 distinct models appear; a single-model
        /// goal collapses to the single tokens line, so the field is empty
        /// (and omitted on the wire). The pager re-checks ≥2 as defence in
        /// depth.
        ///
        /// This is a live, active-subagent-window field (it mirrors
        /// `live_subagent_tokens` and is cleared on `SubagentFinished`): the
        /// pager renders it only under the "Active subagent" block. The
        /// producer must therefore keep its populate gate on that same
        /// axis so the wire and render gates stay aligned.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        live_tokens_by_model: Vec<(String, u64)>,
        #[serde(skip_serializing_if = "Option::is_none")]
        live_context_pct: Option<u8>,
        #[serde(skip_serializing_if = "Option::is_none")]
        live_turn_count: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        live_tool_call_count: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        last_event: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        last_event_detail: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        last_event_timestamp: Option<String>,
        /// Human-readable explanation for paused or blocked state.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pause_message: Option<String>,
    },
    /// A blocking reverse-request (permission / `ask_user_question` /
    /// plan-approval) is now **pending** on the agent, keyed by `tool_call_id`
    /// Fire-and-forget, **never persisted** — it is a request,
    /// not a notification. Subscribers show ⏳ NeedsInput for this session.
    PendingInteraction {
        tool_call_id: String,
        kind: crate::session::pending_interaction::PendingKind,
    },
    /// A previously-pending reverse-request **resolved** (answered, cancelled,
    /// or errored). Fire-and-forget, **never persisted**. Subscribers clear the
    /// pending ⏳ for this `tool_call_id`.
    InteractionResolved { tool_call_id: String },
    /// The durable, replayable signal that a turn reached its terminal
    /// outcome. Rides the persisted `_grow/session/update` rail so a viewer
    /// that reattaches mid-turn can finalize the turn from replay instead of
    /// staying stuck on "Waiting…".
    TurnCompleted {
        /// Correlation key the re-attaching viewer finalizes the turn on:
        /// the prompt/turn whose terminal outcome this carries.
        prompt_id: String,
        /// Structured immutable owner captured when the turn was admitted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        identity: Option<TurnIdentity>,
        /// Why the turn ended (the model's stop reason, or e.g. "cancelled").
        stop_reason: String,
        /// Final agent result text, when the turn produced one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_result: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<PromptUsage>,
    },
    /// One model response opened (Messages `message_start`), carrying the real
    /// message id, model, and input-side token counts. Rides the buffered chunk
    /// rail so it is ordered AHEAD of this response's agent chunks: headless
    /// partial-mode framing consumes it to emit the real `message_start` id and
    /// input usage instead of a synthesized placeholder / zero-seeded usage.
    /// Messages backend only; other backends never emit it (the reducer keeps
    /// its placeholder fallback there).
    ///
    /// `input_tokens` is the uncached prompt portion; `cache_read_input_tokens`
    /// and `cache_creation_input_tokens` are the separate prompt-side cache
    /// buckets, both known at `message_start` on the Messages backend.
    ResponseStarted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default)]
        input_tokens: u64,
        #[serde(default)]
        cache_read_input_tokens: u64,
        #[serde(default)]
        cache_creation_input_tokens: u64,
    },
    /// This response's reasoning (thinking) block finished; carries its
    /// encrypted signature. Rides the buffered chunk rail so it is ordered right
    /// AFTER this response's thought chunks (and before its text): headless
    /// partial-mode framing consumes it to emit `signature_delta` before the
    /// thinking block's `content_block_stop`, in order. Messages backend only.
    ReasoningCompleted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    /// One completed model response, so headless can emit a Messages API
    /// assistant frame per response. Ordered with the response's chunks; a tool
    /// loop emits several. The durable outcome rides `TurnCompleted`.
    ResponseCompleted {
        /// Provider message id (Messages `message.id`), when reported.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        /// Verbatim wire stop reason (`end_turn`, `tool_use`, …), when reported.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<ResponseUsage>,
        /// Reasoning signature (encrypted content) for this response's thinking.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
        /// The provider's matched stop sequence (Messages API
        /// `message.stop_sequence`), present only when the model stopped on a
        /// configured stop sequence; `None` otherwise. Headless
        /// `streaming-messages-json` stamps it onto the assistant frame.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stop_sequence: Option<String>,
    },
}

/// Metadata for a single memory file, sent to the pager for the memory modal.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct MemoryFileInfo {
    pub path: String,
    /// `"global"`, `"workspace"`, or `"session"`.
    pub source: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_epoch_secs: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ImageCompressedEntry {
    pub index: usize,
    pub original_bytes: usize,
    pub compressed_bytes: usize,
    pub original_width: u32,
    pub original_height: u32,
    pub compressed_width: u32,
    pub compressed_height: u32,
}

impl From<&crate::session::image_normalize::ImageCompressionInfo> for ImageCompressedEntry {
    fn from(c: &crate::session::image_normalize::ImageCompressionInfo) -> Self {
        Self {
            index: c.index,
            original_bytes: c.original_bytes,
            compressed_bytes: c.compressed_bytes,
            original_width: c.original_width,
            original_height: c.original_height,
            compressed_width: c.compressed_width,
            compressed_height: c.compressed_height,
        }
    }
}

/// State of a retry operation or error for visual feedback in the TUI
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum RetryState {
    /// A retry is in progress
    Retrying {
        /// Current retry attempt number (1-indexed)
        attempt: u32,
        /// Maximum number of retries allowed
        max_retries: u32,
        /// Human-readable reason for the retry
        reason: String,
    },
    /// All retries have been exhausted
    Exhausted {
        /// Total number of attempts made
        attempts: u32,
        /// Human-readable reason for the failure
        reason: String,
        /// True when the exhaustion was caused by an HTTP 429 rate limit.
        /// Clients use this to show a user-friendly upgrade message instead
        /// of the raw `reason` string.
        #[serde(default)]
        is_rate_limited: bool,
    },
    /// A non-retryable error occurred (e.g., auth error, invalid params)
    Failed {
        /// Category of the error (e.g., "auth", "invalid_params", "server")
        error_type: String,
        /// Human-readable error message
        message: String,
    },
}

/// A diff content item that serializes compatibly with `acp::ToolCallContent::Diff`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(tag = "type", rename = "diff")]
pub struct DiffContent {
    /// The diff details.
    #[serde(flatten)]
    pub diff: acp::Diff,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subagent_progress_serializes_snake_case_tag() {
        let update = SessionUpdate::SubagentProgress {
            subagent_id: "sub-1".into(),
            parent_session_id: "parent-1".into(),
            child_session_id: "child-1".into(),
            duration_ms: 5000,
            turn_count: 3,
            tool_call_count: 12,
            tokens_used: 45_000,
            context_window_tokens: 256_000,
            context_usage_pct: 35,
            tools_used: vec!["bash".into(), "grep".into()],
            error_count: 1,
        };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["sessionUpdate"], "subagent_progress");
        // Fields serialize as snake_case (Rust field names) — enum
        // `rename_all` only applies to the tag, not struct fields.
        assert_eq!(json["subagent_id"], "sub-1");
        assert_eq!(json["parent_session_id"], "parent-1");
        assert_eq!(json["child_session_id"], "child-1");
        assert_eq!(json["duration_ms"], 5000);
        assert_eq!(json["turn_count"], 3);
        assert_eq!(json["tool_call_count"], 12);
        assert_eq!(json["tokens_used"], 45_000);
        assert_eq!(json["context_window_tokens"], 256_000);
        assert_eq!(json["context_usage_pct"], 35);
        assert_eq!(json["tools_used"], serde_json::json!(["bash", "grep"]));
        assert_eq!(json["error_count"], 1);
    }

    #[test]
    fn subagent_progress_roundtrips_through_json() {
        let update = SessionUpdate::SubagentProgress {
            subagent_id: "sub-rt".into(),
            parent_session_id: "p".into(),
            child_session_id: "c".into(),
            duration_ms: 100,
            turn_count: 1,
            tool_call_count: 2,
            tokens_used: 1000,
            context_window_tokens: 256_000,
            context_usage_pct: 1,
            tools_used: vec![],
            error_count: 0,
        };
        let json_str = serde_json::to_string(&update).unwrap();
        let parsed: SessionUpdate = serde_json::from_str(&json_str).unwrap();
        assert_eq!(update, parsed);
    }

    #[test]
    fn subagent_permission_decision_roundtrips_as_durable_ui_event() {
        let update = SessionUpdate::SubagentPermissionDecision {
            child_session_id: "child-019ff8d7".into(),
            subagent_type: Some("software-coder".into()),
            description: Some("run focused tests".into()),
            tool_call_id: "tool-7".into(),
            tool_name: "run_terminal_command".into(),
            access_kind: "bash".into(),
            access_summary: Some("cargo test -p shell".into()),
            access_detail: Some("cargo test -p shell -- --nocapture".into()),
            outcome: SubagentPermissionOutcome::Approved,
            source: "main_agent".into(),
            reason: Some("needed to verify the requested change".into()),
            classifier_reason: Some("The command is in task scope.".into()),
            latency_ms: Some(42),
        };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["sessionUpdate"], "subagent_permission_decision");
        assert_eq!(json["child_session_id"], "child-019ff8d7");
        assert_eq!(json["outcome"], "approved");
        assert_eq!(json["source"], "main_agent");
        assert_eq!(
            serde_json::from_value::<SessionUpdate>(json).unwrap(),
            update
        );
    }

    #[test]
    fn subagent_progress_orders_after_spawned_before_finished() {
        // Verify that SubagentProgress appears between SubagentSpawned
        // and SubagentFinished in the enum definition (important for
        // notification ordering expectations).
        let spawned = serde_json::to_value(SessionUpdate::SubagentSpawned {
            subagent_id: "s".into(),
            parent_session_id: "p".into(),
            parent_prompt_id: None,
            child_session_id: "c".into(),
            subagent_type: "explore".into(),
            description: "d".into(),
            effective_context_source: None,
            context_normalized: false,
            capability_mode: None,
            permission_mode: None,
            effective_permission_mode: None,
            model: None,
            resumed_from: None,
            workflow_run_id: None,
            goal_id: None,
        })
        .unwrap();
        let progress = serde_json::to_value(SessionUpdate::SubagentProgress {
            subagent_id: "s".into(),
            parent_session_id: "p".into(),
            child_session_id: "c".into(),
            duration_ms: 100,
            turn_count: 1,
            tool_call_count: 1,
            tokens_used: 100,
            context_window_tokens: 256_000,
            context_usage_pct: 0,
            tools_used: vec![],
            error_count: 0,
        })
        .unwrap();
        let finished = serde_json::to_value(SessionUpdate::SubagentFinished {
            subagent_id: "s".into(),
            child_session_id: "c".into(),
            status: "completed".into(),
            error: None,
            tool_calls: 1,
            turns: 1,
            duration_ms: 200,
            tokens_used: 50_000,
            output: None,
        })
        .unwrap();
        // All three should have distinct tags
        assert_eq!(spawned["sessionUpdate"], "subagent_spawned");
        assert_eq!(progress["sessionUpdate"], "subagent_progress");
        assert_eq!(finished["sessionUpdate"], "subagent_finished");
    }

    #[test]
    fn subagent_finished_with_tokens_used_roundtrips() {
        let update = SessionUpdate::SubagentFinished {
            subagent_id: "sa-rt".into(),
            child_session_id: "cs-rt".into(),
            status: "completed".into(),
            error: None,
            tool_calls: 5,
            turns: 2,
            duration_ms: 10_000,
            tokens_used: 75_000,
            output: Some("done".into()),
        };
        let json_str = serde_json::to_string(&update).unwrap();
        let parsed: SessionUpdate = serde_json::from_str(&json_str).unwrap();
        assert_eq!(update, parsed);

        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["tokens_used"], 75_000);
    }

    #[test]
    fn unknown_variant_in_session_notification_envelope_is_rejected() {
        let json = r#"{
            "sessionId": "sess-123",
            "update": {"sessionUpdate": "git_branch_update", "branch": "main"}
        }"#;
        assert!(serde_json::from_str::<SessionNotification>(json).is_err());
    }

    #[test]
    fn known_variants_still_deserialize_correctly() {
        // MemoryFlushStarted (unit variant)
        let json = r#"{"sessionUpdate": "memory_flush_started"}"#;
        let update: SessionUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(update, SessionUpdate::MemoryFlushStarted);

        // AutoCompactCancelled (strenum reason)
        let json = r#"{"sessionUpdate": "auto_compact_cancelled", "reason": "user_cancelled"}"#;
        let update: SessionUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(
            update,
            SessionUpdate::AutoCompactCancelled {
                reason: AutoCompactCancelReason::UserCancelled,
            }
        );

        // AutoCompactFailed (struct variant)
        let json = r#"{"sessionUpdate": "auto_compact_failed", "error": "oom"}"#;
        let update: SessionUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(
            update,
            SessionUpdate::AutoCompactFailed {
                error: "oom".into()
            }
        );

        // RetryState (newtype variant)
        let json = r#"{"sessionUpdate": "retry_state", "type": "failed", "error_type": "auth", "message": "bad token"}"#;
        let update: SessionUpdate = serde_json::from_str(json).unwrap();
        assert!(matches!(
            update,
            SessionUpdate::RetryState(RetryState::Failed { .. })
        ));
    }

    #[test]
    fn memory_flush_completed_with_path_roundtrips() {
        let update = SessionUpdate::MemoryFlushCompleted {
            result: "written".into(),
            path: Some("/home/user/.grow/memory/ws/sessions/log.md".into()),
        };
        let json_str = serde_json::to_string(&update).unwrap();
        let parsed: SessionUpdate = serde_json::from_str(&json_str).unwrap();
        assert_eq!(update, parsed);
    }

    #[test]
    fn memory_flush_completed_allows_absent_path_when_nothing_was_written() {
        let json = r#"{"sessionUpdate": "memory_flush_completed", "result": "written"}"#;
        let update: SessionUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(
            update,
            SessionUpdate::MemoryFlushCompleted {
                result: "written".into(),
                path: None,
            }
        );
    }

    #[test]
    fn memory_dream_completed_roundtrips() {
        let update = SessionUpdate::MemoryDreamCompleted {
            result: "written (500 chars)".into(),
            path: Some("/home/user/.grow/memory/ws/MEMORY.md".into()),
        };
        let json_str = serde_json::to_string(&update).unwrap();
        let parsed: SessionUpdate = serde_json::from_str(&json_str).unwrap();
        assert_eq!(update, parsed);
    }

    #[test]
    fn memory_session_saved_roundtrips() {
        let update = SessionUpdate::MemorySessionSaved {
            path: "/home/user/.grow/memory/ws/sessions/2026-01-15-fix-auth-abc12345.md".into(),
        };
        let json_str = serde_json::to_string(&update).unwrap();
        let parsed: SessionUpdate = serde_json::from_str(&json_str).unwrap();
        assert_eq!(update, parsed);
    }

    #[test]
    fn memory_files_variant_roundtrips_through_json() {
        let update = SessionUpdate::MemoryFiles {
            files: vec![
                MemoryFileInfo {
                    path: "/home/user/.grow/memory/MEMORY.md".into(),
                    source: "global".into(),
                    size_bytes: 1024,
                    modified_epoch_secs: Some(1_700_000_000),
                },
                MemoryFileInfo {
                    path: "/project/.grow/memory/MEMORY.md".into(),
                    source: "workspace".into(),
                    size_bytes: 512,
                    modified_epoch_secs: None,
                },
            ],
        };
        let json_str = serde_json::to_string(&update).unwrap();
        let parsed: SessionUpdate = serde_json::from_str(&json_str).unwrap();
        assert_eq!(update, parsed);

        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["sessionUpdate"], "memory_files");
        assert_eq!(json["files"].as_array().unwrap().len(), 2);
        // Populated timestamp serializes as a plain u64
        assert_eq!(json["files"][0]["modified_epoch_secs"], 1_700_000_000_u64);
        // None is omitted entirely
        assert!(json["files"][1].get("modified_epoch_secs").is_none());
    }

    #[test]
    fn memory_files_empty_list_serializes() {
        let update = SessionUpdate::MemoryFiles { files: vec![] };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["sessionUpdate"], "memory_files");
        assert!(json["files"].as_array().unwrap().is_empty());
        let json_str = serde_json::to_string(&update).unwrap();
        let parsed: SessionUpdate = serde_json::from_str(&json_str).unwrap();
        assert_eq!(update, parsed);
    }

    #[test]
    fn tool_call_delta_chunk_first_event_serializes_with_id_and_name() {
        // First chunk for a tool: carries id+name, no arguments_delta.
        let update = SessionUpdate::ToolCallDeltaChunk {
            tool_call_id: Some("call_abc".into()),
            tool_index: 0,
            name: Some("search_replace".into()),
            arguments_delta: None,
        };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["sessionUpdate"], "tool_call_delta_chunk");
        assert_eq!(json["tool_call_id"], "call_abc");
        assert_eq!(json["tool_index"], 0);
        assert_eq!(json["name"], "search_replace");
        // None fields are skipped (cleaner wire payload, fewer bytes).
        assert!(json.get("arguments_delta").is_none());
    }

    #[test]
    fn tool_call_delta_chunk_subsequent_event_carries_only_arguments_delta() {
        // Later chunks omit id+name; only the JSON fragment travels.
        let update = SessionUpdate::ToolCallDeltaChunk {
            tool_call_id: None,
            tool_index: 0,
            name: None,
            arguments_delta: Some("{\"file\":\"src/".into()),
        };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["sessionUpdate"], "tool_call_delta_chunk");
        assert_eq!(json["tool_index"], 0);
        assert_eq!(json["arguments_delta"], "{\"file\":\"src/");
        // Optional fields skipped when None.
        assert!(json.get("tool_call_id").is_none());
        assert!(json.get("name").is_none());
    }

    #[test]
    fn tool_call_delta_chunk_roundtrips_through_json() {
        let cases = vec![
            SessionUpdate::ToolCallDeltaChunk {
                tool_call_id: Some("call_1".into()),
                tool_index: 0,
                name: Some("Bash".into()),
                arguments_delta: None,
            },
            SessionUpdate::ToolCallDeltaChunk {
                tool_call_id: None,
                tool_index: 0,
                name: None,
                arguments_delta: Some("{\"command\":\"ls\"}".into()),
            },
            SessionUpdate::ToolCallDeltaChunk {
                tool_call_id: Some("call_2".into()),
                tool_index: 1,
                name: Some("ReadFile".into()),
                arguments_delta: Some("{\"path\":".into()),
            },
        ];
        for update in cases {
            let s = serde_json::to_string(&update).unwrap();
            let parsed: SessionUpdate = serde_json::from_str(&s).unwrap();
            assert_eq!(update, parsed, "round-trip mismatch for {s}");
        }
    }

    #[test]
    fn tool_call_delta_chunk_rejects_unknown_fields() {
        let json = r#"{
            "sessionUpdate": "tool_call_delta_chunk",
            "tool_call_id": "call_x",
            "tool_index": 7,
            "name": "future_tool",
            "arguments_delta": "...",
            "future_field": "ignored"
        }"#;
        assert!(serde_json::from_str::<SessionUpdate>(json).is_err());
    }

    fn goal_update() -> SessionUpdate {
        SessionUpdate::GoalUpdated {
            goal_id: "g-1".into(),
            objective: "Build widget".into(),
            objective_revision: 2,
            status: "active".into(),
            phase: "verifying".into(),
            plan_revision: 4,
            board_revision: 9,
            tasks: Vec::new(),
            plan_markdown: "board".into(),
            verifier_feedback: Some("Run the integration suite".into()),
            token_budget: Some(100_000),
            tokens_used: 25_000,
            elapsed_ms: 5_000,
            current_subagent_role: Some("verifier".into()),
            total_worker_rounds: 4,
            total_verify_rounds: 2,
            token_baseline: 0,
            finished_subagent_tokens: 10_000,
            live_subagent_tokens: Some(2_000),
            live_tokens_by_model: vec![("grow-4".into(), 2_000)],
            live_context_pct: Some(35),
            live_turn_count: Some(3),
            live_tool_call_count: Some(8),
            last_event: Some("verification_rejected".into()),
            last_event_detail: Some("Run the integration suite".into()),
            last_event_timestamp: Some("2026-01-01T00:05:00Z".into()),
            pause_message: None,
        }
    }

    #[test]
    fn goal_updated_v2_round_trips_with_structured_state() {
        let update = goal_update();
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["sessionUpdate"], "goal_updated");
        assert_eq!(json["objective_revision"], 2);
        assert_eq!(json["phase"], "verifying");
        assert_eq!(json["plan_revision"], 4);
        assert_eq!(json["board_revision"], 9);
        assert_eq!(json["plan_markdown"], "board");
        assert_eq!(json["verifier_feedback"], "Run the integration suite");
        assert_eq!(
            serde_json::from_value::<SessionUpdate>(json).unwrap(),
            update
        );
    }

    #[test]
    fn goal_updated_v2_requires_the_blackboard_contract() {
        let obsolete = serde_json::json!({
            "sessionUpdate": "goal_updated",
            "goal_id": "g-old",
            "objective": "obsolete",
            "status": "active",
            "phase": "idle",
            "tokens_used": 0,
            "elapsed_ms": 0,
            "total_worker_rounds": 0,
            "total_verify_rounds": 0
        });
        assert!(serde_json::from_value::<SessionUpdate>(obsolete).is_err());
    }

    // ── ModelChanged (leader-mode multi-client model switch fan-out) ──

    /// Wire format for `ModelChanged` — sanity-check the JSON exactly,
    /// since the pager and any third-party clients consume this on the wire.
    /// Specifically:
    /// - `sessionUpdate` tag is the snake_case variant name.
    /// - Field names use Rust snake_case (struct fields are not subject to
    ///   `rename_all` — that only renames the tag).
    /// - `reasoning_effort` is omitted entirely when `None` (smaller wire +
    ///   distinguishable from explicitly-cleared-by-user, if that ever
    ///   becomes a real distinction).
    #[test]
    fn model_changed_serializes_snake_case_with_optional_effort() {
        let with_effort = SessionUpdate::ModelChanged {
            model_id: "grow-4".into(),
            reasoning_effort: Some("high".into()),
        };
        let json = serde_json::to_value(&with_effort).unwrap();
        assert_eq!(json["sessionUpdate"], "model_changed");
        assert_eq!(json["model_id"], "grow-4");
        assert_eq!(json["reasoning_effort"], "high");

        let without_effort = SessionUpdate::ModelChanged {
            model_id: "grow-3".into(),
            reasoning_effort: None,
        };
        let json = serde_json::to_value(&without_effort).unwrap();
        assert_eq!(json["sessionUpdate"], "model_changed");
        assert_eq!(json["model_id"], "grow-3");
        assert!(
            json.get("reasoning_effort").is_none(),
            "reasoning_effort: None must be omitted because absence is the \
             canonical representation of no override"
        );
    }

    /// `ModelChanged` round-trips through JSON: a follower client deserializes
    /// the exact same value the agent serialized. Pins the field order /
    /// case so an accidental rename fails loudly instead of breaking
    /// multi-client model sync without any signal.
    #[test]
    fn model_changed_roundtrips_through_json() {
        let original = SessionUpdate::ModelChanged {
            model_id: "grow-4".into(),
            reasoning_effort: Some("medium".into()),
        };
        let json_str = serde_json::to_string(&original).unwrap();
        let parsed: SessionUpdate = serde_json::from_str(&json_str).unwrap();
        assert_eq!(original, parsed);
    }

    /// Wrap `ModelChanged` in the full `SessionNotification` envelope and
    /// confirm the result is what the leader's session-scoped fan-out keys
    /// on: top-level `sessionId` (camelCase from the envelope's
    /// `rename_all`) + nested `update.sessionUpdate == "model_changed"`.
    /// Without the top-level `sessionId`, the leader's `extract_session_id`
    /// returns `None` and the notification falls through to the
    /// last-active-client fallback instead of broadcasting — that would
    /// silently break the entire multi-client sync.
    #[test]
    fn model_changed_envelope_carries_session_id_at_top_level() {
        let notif = SessionNotification {
            session_id: acp::SessionId::new("sess-abc"),
            update: SessionUpdate::ModelChanged {
                model_id: "grow-4".into(),
                reasoning_effort: None,
            },
            meta: None,
        };
        let json = serde_json::to_value(&notif).unwrap();
        assert_eq!(json["sessionId"], "sess-abc");
        assert_eq!(json["update"]["sessionUpdate"], "model_changed");
        assert_eq!(json["update"]["model_id"], "grow-4");
    }

    // ── TurnCompleted (durable, replayable turn-end signal) ──

    #[test]
    fn turn_completed_serializes_snake_case_tag_and_fields() {
        // Mirrors the SubagentProgress convention: `rename_all = "snake_case"`
        // only renames the tag, so struct fields keep their Rust snake_case
        // names on the wire.
        let update = SessionUpdate::TurnCompleted {
            prompt_id: "p-1".into(),
            identity: Some(TurnIdentity {
                origin: "goal_finalization".into(),
                turn_kind: "internal".into(),
                goal_id: Some("g-1".into()),
                stage_id: Some(7),
            }),
            stop_reason: "end_turn".into(),
            agent_result: Some("done".into()),
            usage: None,
        };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["sessionUpdate"], "turn_completed");
        assert_eq!(json["prompt_id"], "p-1");
        assert_eq!(json["stop_reason"], "end_turn");
        assert_eq!(json["agent_result"], "done");
        assert_eq!(json["identity"]["goal_id"], "g-1");
    }

    #[test]
    fn turn_completed_optional_fields_skipped_when_none() {
        let update = SessionUpdate::TurnCompleted {
            prompt_id: "p-2".into(),
            identity: None,
            stop_reason: "cancelled".into(),
            agent_result: None,
            usage: None,
        };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["sessionUpdate"], "turn_completed");
        assert!(json.get("agent_result").is_none());
    }

    #[test]
    fn turn_completed_roundtrips_through_json() {
        for update in [
            SessionUpdate::TurnCompleted {
                prompt_id: "p-rt".into(),
                identity: Some(TurnIdentity {
                    origin: "user".into(),
                    turn_kind: "user".into(),
                    goal_id: None,
                    stage_id: None,
                }),
                stop_reason: "end_turn".into(),
                agent_result: Some("result text".into()),
                usage: None,
            },
            SessionUpdate::TurnCompleted {
                prompt_id: "p-min".into(),
                identity: None,
                stop_reason: "error".into(),
                agent_result: None,
                usage: None,
            },
        ] {
            let json_str = serde_json::to_string(&update).unwrap();
            let parsed: SessionUpdate = serde_json::from_str(&json_str).unwrap();
            assert_eq!(update, parsed);
        }
    }

    #[test]
    fn project_result_hides_costs_when_partial_or_incomplete() {
        let mut model_usage = indexmap::IndexMap::new();
        model_usage.insert(
            "m".into(),
            PromptUsageModel {
                input_tokens: 100,
                cached_read_tokens: 40,
                output_tokens: 10,
                total_tokens: 110,
                model_calls: 4,
                cost_usd_ticks: Some(1_000_000_000),
                ..Default::default()
            },
        );
        let partial = PromptUsage {
            totals: PromptUsageModel {
                input_tokens: 100,
                cached_read_tokens: 40,
                output_tokens: 10,
                total_tokens: 110,
                model_calls: 5,
                cost_usd_ticks: Some(1_000_000_000),
                cost_is_partial: true,
                cost_missing_calls: 1,
                ..Default::default()
            },
            model_usage: model_usage.clone(),
            num_turns: 2,
            usage_is_incomplete: false,
        };
        let mut result = serde_json::json!({});
        project_result_usage(&mut result, &partial);
        assert_eq!(result["usage"]["input_tokens"], 60);
        assert!(result.get("total_cost_usd").is_none());
        assert!(result.get("total_cost_usd_ticks").is_none());
        assert_eq!(result["cost_is_partial"], true);
        assert!(result["modelUsage"]["m"].get("costUSD").is_none());

        let mut incomplete = PromptUsage {
            totals: PromptUsageModel {
                input_tokens: 50,
                output_tokens: 5,
                total_tokens: 55,
                model_calls: 1,
                cost_usd_ticks: Some(5_000_000_000),
                ..Default::default()
            },
            model_usage,
            num_turns: 1,
            usage_is_incomplete: true,
        };
        incomplete.scrub_untrustworthy_costs();
        assert!(incomplete.totals.cost_usd_ticks.is_none());
        let mut result = serde_json::json!({});
        project_result_usage(&mut result, &incomplete);
        assert_eq!(result["usage_is_incomplete"], true);
        assert!(result.get("total_cost_usd").is_none());
        assert!(result.get("total_cost_usd_ticks").is_none());
        assert!(result["modelUsage"]["m"].get("costUSD").is_none());
    }

    #[test]
    fn project_result_incomplete_empty_omits_zero_usage() {
        let usage = PromptUsage::project_from_ledger(None, true).unwrap();
        let mut result = serde_json::json!({});
        project_result_usage(&mut result, &usage);
        assert_eq!(result["usage_is_incomplete"], true);
        assert!(result.get("usage").is_none());
        assert!(result.get("num_turns").is_none());
        assert!(result.get("total_cost_usd").is_none());
    }

    #[test]
    fn attach_result_usage_fail_closed_on_parse_error() {
        let mut result = serde_json::json!({"ok": true});
        attach_result_usage_fail_closed(&mut result, &serde_json::json!("not-an-object"));
        assert_eq!(result["usage_is_incomplete"], true);
        assert!(result.get("usage").is_none());
        assert_eq!(result["ok"], true);
    }

    #[test]
    fn cost_missing_calls_not_on_acp_wire() {
        let model = PromptUsageModel {
            input_tokens: 1,
            cost_missing_calls: 3,
            cost_is_partial: true,
            ..Default::default()
        };
        let v = serde_json::to_value(&model).unwrap();
        assert!(v.get("costMissingCalls").is_none());
        assert_eq!(v["costIsPartial"], true);
    }

    #[test]
    fn scrub_untrustworthy_costs_clears_ticks_when_partial() {
        let mut usage = PromptUsage {
            totals: PromptUsageModel {
                input_tokens: 10,
                output_tokens: 1,
                total_tokens: 11,
                cost_usd_ticks: Some(100),
                cost_is_partial: true,
                cost_missing_calls: 1,
                ..Default::default()
            },
            model_usage: Default::default(),
            num_turns: 1,
            usage_is_incomplete: false,
        };
        usage.scrub_untrustworthy_costs();
        assert!(usage.totals.cost_usd_ticks.is_none());
        assert!(usage.totals.cost_is_partial);
    }

    #[test]
    fn project_result_token_identity_uncached_plus_cache_plus_output() {
        let mut model_usage = indexmap::IndexMap::new();
        model_usage.insert(
            "m".into(),
            PromptUsageModel {
                input_tokens: 100,
                cached_read_tokens: 40,
                output_tokens: 10,
                total_tokens: 110,
                model_calls: 1,
                cost_usd_ticks: Some(2_000_000_000),
                ..Default::default()
            },
        );
        let usage = PromptUsage {
            totals: PromptUsageModel {
                input_tokens: 100,
                cached_read_tokens: 40,
                output_tokens: 10,
                total_tokens: 110,
                model_calls: 1,
                cost_usd_ticks: Some(2_000_000_000),
                ..Default::default()
            },
            model_usage,
            num_turns: 1,
            usage_is_incomplete: false,
        };
        let mut result = serde_json::json!({});
        project_result_usage(&mut result, &usage);
        let uncached = result["usage"]["input_tokens"].as_u64().unwrap();
        let cache = result["usage"]["cache_read_input_tokens"].as_u64().unwrap();
        let output = result["usage"]["output_tokens"].as_u64().unwrap();
        let total = result["usage"]["total_tokens"].as_u64().unwrap();
        assert_eq!(uncached, 60);
        assert_eq!(cache, 40);
        assert_eq!(output, 10);
        assert_eq!(total, uncached + cache + output);
        // ACP serde keeps full input_tokens; headless identity differs.
        let acp = serde_json::to_value(&usage).unwrap();
        assert_eq!(acp["inputTokens"], 100);
        assert_eq!(acp["cachedReadTokens"], 40);
        assert_ne!(acp["inputTokens"], result["usage"]["input_tokens"]);
        assert_eq!(result["modelUsage"]["m"]["inputTokens"], 60);
        assert_eq!(result["modelUsage"]["m"]["cacheReadInputTokens"], 40);
        assert_eq!(result["total_cost_usd"], 0.2);
        // Exact ticks accompany the float for tick-exact reconciliation.
        assert_eq!(result["total_cost_usd_ticks"], 2_000_000_000_i64);
    }

    #[test]
    fn turn_completed_missing_required_fields_fail_to_deserialize() {
        let missing_prompt_id = r#"{"sessionUpdate": "turn_completed", "stop_reason": "end_turn"}"#;
        assert!(serde_json::from_str::<SessionUpdate>(missing_prompt_id).is_err());
        let missing_stop_reason = r#"{"sessionUpdate": "turn_completed", "prompt_id": "p-1"}"#;
        assert!(serde_json::from_str::<SessionUpdate>(missing_stop_reason).is_err());
    }
}
