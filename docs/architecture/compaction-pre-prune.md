# Compaction Pre-Prune Architecture

> **Status**: Implemented (Task C)
> **Date**: 2026-08-13
> **Scope**: shell, diagnostics, config-types, chat-state (command consumer only)
> **Author**: software-architect / coder (Task C)

## 1. What This Is

Pre-prune is the **model-free** half of session load shedding. When auto-compaction
would fire (context window over the trigger threshold), `run_compact_only` first
tries to trim oversized tool results in the stored conversation. If the trim alone
brings the token estimate back under the trigger threshold, the summarizer LLM call
is skipped entirely. If not, the existing summary path runs — with a smaller input.

The deterministic core lives in `compaction` (`crates/common/compaction/src/prune.rs`,
Tasks A) and the actor-side application in `chat-state`
(`ChatStateHandle::prune_tool_results`, Task B). This document covers the shell-side
ladder that wires them together (Task C).

## 2. Trigger and Ladder

Insertion point: `run_compact_only` (shell `session/compaction.rs`), after the
pre-compaction flush and **before** `run_compact_inner`. The ladder function is
`maybe_pre_prune(&trigger_info) -> Result<bool, acp::Error>`:

1. **Config gate**: `compaction.pre_prune == false`, or the plan is empty →
   return `false` (no pruning, summary path unchanged).

   **Suppress gate**: account-state suppression (`SUPPRESS_UNTIL_SUCCESS`,
   `SUPPRESS_AUTH`) and per-turn suppression (`SUPPRESS_TURN`) block the
   ladder — account state is unrelated to model-free pruning, and per-turn
   failures self-heal at the next turn start. `SUPPRESS_STICKY` (deterministic
   size failures) and `SUPPRESS_NONE` let it through: pruning is exactly the
   model-free remedy for the size failures STICKY marks, and a prune whose
   strict gate passes clears the sticky bit (§5) — the existing
   "context-budget change" STICKY clear condition. A gate that does not pass
   never touches the suppress state.
2. **Budget derivation**:
   - `item_budget = compaction.pre_prune_token_budget`
     or `context_window × 5%` (lower bound 1 token);
   - `target = context_window × threshold_percent / 100` — the same absolute
     count the trigger threshold represents (`exceeds_threshold` scales by
     `×100`, so the absolute form divides by 100).
3. **Plan**: `compaction::plan_tool_result_pruning(&conversation,
   &EstimatedItemTokenCounter, item_budget, target)` — oldest oversized tool
   results first, stop as soon as the projected post-prune total is `<= target`.
   While the fork's inherited prefix is still pinned
   (`prefix_released == false`, see §5), planning runs over
   `conversation[inherited_prefix_len..]` only (clamped to the conversation
   length) and the returned plan indexes are re-based onto the full
   conversation before applying. `target` and `item_budget` keep their
   full-conversation derivation; the planner counts tokens over the slice it
   receives, so its stop condition compares the slice's own total against
   that absolute target. The strict gate (step 5) still re-checks the full
   estimate before the summary is skipped, so this only leans conservative.
4. **Apply**: `chat_state_handle.prune_tool_results(plan)` — actor-serialized,
   idempotent, commits one complete `tool_result_prune` Surface replacement
   through the durable Timeline acknowledgement path. The replacement is
   accepted in memory only after storage confirms it.
   **Any `Err` fails open**: `warn` log + `false` (continue the summary path).
   Pruning is an optimization, never a correctness requirement.
5. **Strict gate**: re-read `get_projected_tokens()`; only when it is
   **below** the trigger threshold (same `exceeds_threshold` helper as
   `should_auto_compact`) does the ladder return `true` — the caller skips
   `run_compact_inner`. Otherwise `false`: the summary path runs, and its input
   is now the pruned (smaller) conversation.

### 2.1 Projection transaction

Pruning applies the signed before/after Surface estimate to chat-state's latest
provider anchor. Appends, pruning, compaction, rewind, and repair all share this
same projection transaction, so the strict gate reads one canonical current-
context waterline. Lifetime and per-prompt billing remain separate
`UsageLedger` state and never participate in the gate.

Immediately before sampling, `build_request` performs the final request-only
projection in the same actor transaction: Goal directive shadows, model-bound
ImageShadows, tool schemas, tool choice, and native JSON schemas are measured as
one input envelope. The meter replaces the previous `(request − Surface)`
adjustment with the new signed adjustment; repeated assembly is therefore
idempotent, and a provider anchor keeps its protocol/tool overhead until the
next envelope changes. The pre-sampling auto-compact check runs only after this
final assembly, so dynamic schemas cannot bypass pressure accounting.
Provider usage below the complete final-envelope estimate is rejected as an
anchor (billing is still recorded), preventing an invalid low sample from
destroying the signed-adjustment basis.

## 3. Success Path Observability

When the gate passes, the ladder emits, in order:

- `AutoCompactStarted` (already sent by `run_compact_only` before the ladder),
- diagnostics event `AutoCompactPruned`:

  | Field | Semantics |
  |---|---|
  | `tokens_before` | chat-state projected pressure before pruning |
  | `tokens_after` | post-prune `get_projected_tokens()` |
  | `pruned_count` | number of tool results actually trimmed |
  | `threshold_percent` | the trigger threshold the gate compared against |
  | `budget_tokens` | the per-item token budget the plan applied |
  | `source` | invocation site (`pre_sampling` / `preflight_overflow` / `model_switch` / `context_window_exceeded` / `sampler_error_recovery`) |

- `AutoCompactCompleted` notification with `tokens_before` /
  `tokens_after` / `elapsed_ms` and a short `summary_preview` describing the
  prune (e.g. `"pruned 1 tool result (101039 → 54820 tokens)"`).

No `AutoCompactFailed` is sent on this path. The caller returns `Ok(())`
immediately; the outer turn loop proceeds to sampling.

## 4. Config Contract

| Key | Type | Default | Env override | Semantics |
|---|---|---|---|---|
| `[compaction] pre_prune` (user TOML) / remote `compaction_pre_prune` | bool | `true` | `GROW_COMPACTION_PRE_PRUNE` | Master gate for the ladder |
| `[compaction] pre_prune_token_budget` (user TOML) / remote `compaction_pre_prune_token_budget` | u64 \| none | `None` (derive 5% of window) | `GROW_COMPACTION_PRE_PRUNE_TOKEN_BUDGET` | Per-item pruning token budget |

Resolution chain (env > user config > remote > default), mirroring
`resolve_compaction_verbatim_input`
(see `shell/src/util/config/resolve/compaction.rs`). A budget value that fails
to parse or is `0` falls through to `None` (default derivation). Runtime state
lives on `CompactionConfig` as `Cell<bool>` / `Cell<Option<u64>>` (the `!Send`
`SessionActor` pattern). Subagents resolve the same three tiers through
`SubagentSpawnContext`.

## 5. Display / Logging Layering

- **No UI events from the prune itself**: the `PruneToolResults` command emits
  no chat-state events and no shell persistence messages. The user-visible
  surface is exactly the `AutoCompactStarted` / `AutoCompactCompleted`
  notifications described in §3 (the compaction attempt's own notifications,
  not conversation-content events).
- **`timeline.jsonl`**: appends a `tool_result_prune` replacement event. Prior
  tool output remains in the transcript projection while Surface carries the
  pruned content (head + marker + tail).
- **No second conversation file**: the replacement event is the complete
  durable mutation; no snapshot cache is written alongside it.
- **Recall-derived content is not summarized back into itself**: before the
  Sideband request is assembled, the complete causal tool exchange containing
  any `context_recall` call is removed from the summary source. This includes
  parallel tool results and the assistant continuation that consumed them;
  filtering only the recall result would let derived text recursively harden
  into later summaries.
- **`updates.jsonl`**: untouched by pruning and remains UI/diagnostic replay
  only. Rewind constructs its new Surface from Timeline.
- **Suppress state**: a prune whose strict gate passes stores
  `SUPPRESS_NONE` — pruning changed the effective context budget, which is
  the existing STICKY clear condition (same rule as a successful compaction,
  rewind, or model switch). Every gate failure leaves the suppress state
  untouched; only the summary path's own failure classification (or the
  convergence check, §6) sets suppression.
- **Fork prefix protection**: while `prefix_released == false`, the fork's
  inherited parent transcript (`conversation[..inherited_prefix_len]`) is
  re-pinned verbatim at compaction time by `preserve_inherited_prefix`, so
  pre-prune excludes that region from planning entirely: an oversized tool
  result inside it is never trimmed, no matter how large. `prefix_released`
  takes precedence over `inherited_prefix_len` — once the prefix is released
  under compaction pressure, the whole conversation is prunable again. An
  out-of-range `inherited_prefix_len` degrades to an empty plan (no pruning),
  never a panic.

## 6. Post-Compaction Convergence (fail-safe)

`run_compact_inner` now runs a **unified** post-replace convergence check on
every path (previously fork-scenario-only): after
the Timeline range replacement, if `get_projected_tokens()` still exceeds
the context window itself, the outcome is:

- `SUPPRESS_STICKY` on `auto_compact_suppressed` (reusing the existing state;
  no new suppress value),
- a `warn` log,
- `Err(acp::Error)` carrying `data.compact_error =
  "compact_converged_over_window"` plus the typed
  `error_kind: context_window_exceeded` marker.

The fork-scenario threshold check (sticky-suppress when a released inherited
prefix still lands over the *trigger threshold*) is unchanged. The convergence
check gates the *window* dimension; the fork check gates the *trigger*
dimension.

The `ModelContextWindowExceeded` turn branch matches this error and **fails the
turn** with a diagnostic message (`unified_log` +
`shell.turn.compact_converged_over_window`) instead of resampling — previously
a still-over-window session resampled forever. All other error paths are
unchanged. Compaction success that lands under the window continues the
resample as before.

Pre-prune runs **before** this path: when a prune alone resolves the pressure
(§2/§5), the summary — and therefore this convergence check — is skipped, and
the successful prune clears `SUPPRESS_STICKY` (§5). The convergence check's
own sticky-suppress behavior is unchanged.

## 7. P2 Not Done (Deferred)

**Per-model budget override** (e.g.
`[provider.<id>.models.<model>.pre_prune_token_budget]`)
is not implemented:

- **ceiling**: the current session-level budget (5% of the window) is uniform
  across models; a model with a materially different bytes/token ratio gets a
  budget that is too loose or too tight for its true token cost.
- **trigger**: fleet evidence that pre-prune produces over-aggressive or
  under-aggressive trims on specific models (observable via
  `AutoCompactPruned.budget_tokens` vs. reported usage, or a rise in
  post-prune `ModelContextWindowExceeded`/compaction calls per model).
- **upgrade path**: add `auto_compact_threshold_percent`-style per-model tiers
  (env > `[provider.<id>.models.<model>]` > `[compaction]` > managed per-model
  > managed global)
  next to `resolve_auto_compact_threshold_percent_from_tiers` in
  `shell/src/util/config/resolve/compaction.rs`, and thread the resolved value
  through `spawn_session_actor` / `spawn_session_on_thread` like the existing
  two fields.
