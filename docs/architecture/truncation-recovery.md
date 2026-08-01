# Truncation Recovery Architecture

> **Status**: Implemented（D1–D8 已全部落地；§1 描述的是实现前的行为基线，仅作设计记录）
> **Date**: 2026-07-31
> **Scope**: grow-sampler, grow-sampling-types, grow-shell
> **Author**: software-architect

实现对照（2026-07-31 核验）：

- `StopReason` 新增 `ModelContextWindowExceeded` / `PauseTurn` 变体（grow-sampling-types，`messages.rs` / `conversation.rs`）。
- `request_task.rs` 中 `StopReason::Length` 映射为 `AttemptOutcome::Truncated { partial_response }`（不再丢弃部分输出）。
- `SyntheticReason::TruncationContinue` 已加入会话层，持久化部分输出并注入继续提示（见 grow-shell `session/acp_session_tests/truncation_recovery_tests.rs`）。
- D8 crate 改名 `xai-chat-state` → `grow-chat-state` 已完成，workspace 版本 `1.0.0`。

## 1. Problem Statement

### 1.1 Current Behavior

When a model's output reaches the `max_tokens` limit (derived from `output_limit` config),
Grow currently:

1. **Streams partial content to the UI** -- the user sees partial text/reasoning/tool-call
   deltas during generation.
2. **Discards the partial response** -- `request_task.rs:622-626` converts
   `StopReason::Length` to `AttemptOutcome::Failed { error: SamplingError::MaxTokensTruncation }`,
   discarding the `ConversationResponse`. The messages backend (`messages.rs:456-462`)
   emits `SamplingEvent::Failed` before constructing the final response at all.
3. **Marks the turn as fatally failed** -- `MaxTokensTruncation` is non-retryable
   (`is_retryable() => false`), classified as `Fatal` in the retry loop. The turn ends
   with `StopFailureKind::MaxOutputTokens`.
4. **Does not persist the partial output** -- the assistant content never enters
   conversation history. The next turn's model cannot see what was already generated.

### 1.2 Root Cause

`StopReason::Length` is used for three semantically distinct conditions:

| Condition | Wire signal | Current mapping | Correct semantics |
|---|---|---|---|
| Output token limit reached | Anthropic `max_tokens`, OpenAI `finish_reason=length` / `status=incomplete` | `StopReason::Length` | Truncation -- continue to get more output |
| Context window exhausted mid-generation | Anthropic `model_context_window_exceeded` (Claude 4.5+) | `StopReason::Length` (wrong) | Input overflow -- compact the input |
| Server-tool iteration limit | Anthropic `pause_turn` | `StopReason::Stop` (wrong) | Resend assistant content to continue |

All three are funneled into `Failed`, losing the partial response and preventing recovery.

### 1.3 User Impact

- User sees half a response, then a "Response truncated by max_tokens." error.
- The partial output is lost from conversation history -- the next turn starts fresh.
- Truncated `tool_use` blocks (incomplete JSON) cannot be executed.
- No automatic recovery; user must manually re-request.

## 2. Design Decisions (User-Confirmed)

| # | Decision | Rationale |
|---|---|---|
| D1 | **Auto-continue on truncation** -- persist partial response, inject continue prompt, start new sampling cycle | Aligns with Claude/ChatGPT behavior; user expects seamless long-output handling |
| D2 | **Default on, no limits** -- auto-continue is always enabled, no max-continue-count or condition gates | User wants the system to be as transparent as possible; if the model needs more turns, let it |
| D3 | **Fix ModelContextWindowExceeded** -- split from `StopReason::Length`, trigger compaction instead of continue | Input overflow cannot be solved by more output; compacting is the correct recovery |
| D4 | **Fix PauseTurn** -- resend assistant content to continue, per Anthropic spec | Current `-> Stop` is wrong; Anthropic expects resend-to-continue |
| D5 | **ACP StopFailure behavior change** -- successful continue does not emit `StopFailure`; `MaxOutputTokens` StopFailureKind retained for unrecoverable cases | A successfully continued turn is not a failure; documented as public contract change |
| D6 | **Thinking blocks: discard incomplete** -- truncated thinking blocks cannot be recovered (Anthropic API constraint: signature must be complete and unmodified) | API-level hard constraint; cannot be worked around |
| D7 | **Tool-call truncation: discard incomplete** -- incomplete `tool_use` blocks are dropped; only complete blocks are persisted | Anthropic: "incomplete tool use blocks cannot be used"; model re-generates on continue |
| D8 | **Crate rename: `xai-chat-state` -> `grow-chat-state`, version 1.0.0** -- the chat-state crate joins the grow-* family; performed as a pure mechanical rename (Task 0) before any feature work | User decision; rename-first avoids re-renaming new feature code; version 1.0.0 matches workspace version (root `Cargo.toml:83`, `[workspace.package] version = "1.0.0"`) |

## 3. Architecture Overview

### 3.1 Module Boundaries and Changes

```
grow-sampling-types (types layer)
  ├── StopReason: add ModelContextWindowExceeded, PauseTurn variants
  ├── AttemptOutcome: add Truncated / ContextWindowExceeded / PauseTurn variants
  │   (each carries the partial ConversationResponse)
  └── SyntheticReason: add TruncationContinue variant
      (no new ConversationResponse field: the truncation signal is the
       stop_reason itself; continue bookkeeping lives in the session layer)

grow-sampler (sampling layer)
  ├── stream/messages.rs:    split MaxTokens vs ModelContextWindowExceeded vs PauseTurn;
  │                          remove the early Failed path for Length
  ├── stream/responses.rs:   keep Incomplete -> Length (no change)
  ├── stream/chat_completions.rs: keep finish_reason=length -> Length (no change)
  ├── actor/request_task.rs: drive_l2 classifies stop_reason to Truncated /
  │                          ContextWindowExceeded / PauseTurn instead of Failed;
  │                          run_request_task emits Completed + sends Ok(partial)
  │                          (no retry -- truncation is deterministic)
  ├── retry.rs:              unchanged (MaxTokensTruncation no longer produced
  │                          for Length; variant retained for unrecoverable cases)
  └── events.rs:             unchanged (reuse SamplingEvent::Completed; the
                             session layer distinguishes via response.stop_reason)

grow-chat-state (chat state layer, formerly xai-chat-state)
  ├── compaction_utils.rs: add TRUNCATION_CONTINUE_PROMPT (next to AUTO_CONTINUE_PROMPT)
  └── (rename performed first as Task 0: xai-chat-state -> grow-chat-state, v1.0.0)

grow-shell (session layer)
  ├── session/acp_session_impl/sampler_turn.rs: continue loop (detect Truncated, persist, inject prompt, re-sample)
  ├── session/acp_session_impl/turn_end.rs:     StopFailure only for unrecoverable truncation
  ├── sampling/error.rs:                        ACP error mapping update
  └── session/helpers/session_compact.rs:       ModelContextWindowExceeded -> compact trigger
```

### 3.2 Data Flow: Truncation Continue

```
User sends request
  │
  ▼
sampler_turn.rs: build SamplingConfig, call sampler
  │
  ▼
grow-sampler: run_request_task
  │
  ├── stream tokens to UI (ChannelToken, ToolCallDelta)
  │
  ├── stop_reason == Length?
  │   ├── YES: return AttemptOutcome::Truncated { partial_response }
  │   ├── NO (Stop/ToolCalls/ContentFilter): return AttemptOutcome::Completed
  │   ├── ModelContextWindowExceeded: return AttemptOutcome::ContextWindowExceeded
  │   └── PauseTurn: return AttemptOutcome::PauseTurn { response }
  │
  ▼
sampler_turn.rs: receive outcome
  │
  ├── Truncated:
  │   1. Persist partial_response to conversation history
  │      (discard incomplete thinking blocks, discard incomplete tool_use)
  │   2. Inject synthetic user message: ConversationItem::truncation_continue(prompt)
  │   3. Re-call sampler with updated conversation
  │   4. Accumulate output (text appended, new tool_calls collected)
  │   5. Repeat until non-Truncated outcome
  │   6. Final Completed: merge all partial outputs into final ConversationResponse
  │
  ├── ContextWindowExceeded:
  │   1. Trigger inline compaction (same as current auto-compact)
  │   2. After compaction, re-call sampler
  │   3. (compaction's auto_continue handles the continue prompt)
  │
  ├── PauseTurn:
  │   1. Persist the response (it is complete -- just server-tool-limited)
  │   2. Resend assistant content as-is to continue
  │   3. (no continue prompt needed -- Anthropic expects raw resend)
  │
  └── Completed/Failed/Empty: existing behavior
```

### 3.3 Continue Loop Location: Session Layer

The continue loop lives in **grow-shell's `sampler_turn.rs`**, not in grow-sampler.

**Rationale**:
- The session layer owns conversation history (via `chat_state_handle`). Persisting partial
  responses and injecting continue prompts requires history access.
- This mirrors the existing compaction auto_continue pattern, which also operates at the
  session layer.
- The sampler remains a stateless request executor: it receives a conversation + config and
  returns an outcome. It does not manage history.
- Keeping the loop in the sampler would require passing history-management capabilities
  down into the sampler, violating the existing layering.

## 4. StopReason Semantics Redefinition

### 4.1 New StopReason Variants

```rust
pub enum StopReason {
    Stop,                    // Model finished naturally
    Length,                  // Hit output token limit (max_tokens) -- truncation, can continue
    ToolCalls,               // Model wants to call tools
    ContentFilter,           // Content was filtered
    ModelContextWindowExceeded,  // NEW: input+output filled context window -- compact input
    PauseTurn,               // NEW: server-tool iteration limit -- resend to continue
}
```

### 4.2 Wire-to-Internal Mapping

| Provider | Wire signal | Internal StopReason | Recovery |
|---|---|---|---|
| Anthropic messages | `max_tokens` | `Length` | Auto-continue |
| Anthropic messages | `model_context_window_exceeded` | `ModelContextWindowExceeded` | Compact input |
| Anthropic messages | `pause_turn` | `PauseTurn` | Resend assistant content |
| Anthropic messages | `end_turn` | `Stop` | None |
| OpenAI responses | `status=incomplete` | `Length` | Auto-continue |
| OpenAI chat_completions | `finish_reason=length` | `Length` | Auto-continue |
| OpenAI (all) | `finish_reason=stop` / `status=completed` | `Stop` | None |

### 4.3 Deprecation of MaxTokensTruncation Error

`SamplingError::MaxTokensTruncation` is **no longer produced** for `StopReason::Length`.
The truncation path produces `AttemptOutcome::Truncated` instead.

`MaxTokensTruncation` is retained as an error variant for:
- Unrecoverable truncation where compaction also fails (context window exhausted after compaction).
- Test backward compatibility during migration.

## 5. AttemptOutcome Changes

### 5.1 New Variants

```rust
enum AttemptOutcome {
    Completed { response: Box<ConversationResponse>, metrics: InferenceLatencyStats },
    Empty { context: EmptyResponseContext },
    Failed { error: SamplingError },
    Cancelled,
    InitFailed { error: SamplingError },
    // NEW:
    Truncated { partial_response: Box<ConversationResponse>, metrics: InferenceLatencyStats },
    ContextWindowExceeded { partial_response: Box<ConversationResponse>, metrics: InferenceLatencyStats },
    PauseTurn { response: Box<ConversationResponse>, metrics: InferenceLatencyStats },
}
```

### 5.2 Truncated Variant

Carries the partial `ConversationResponse` (with completed text, completed tool_calls,
**discarded** incomplete thinking blocks and tool_use blocks) and latency metrics from
the truncated attempt.

### 5.3 ContextWindowExceeded Variant

Carries the partial response (for UI continuity) but signals that the session layer
should trigger compaction rather than continue.

### 5.4 PauseTurn Variant

Carries the complete response (PauseTurn means the assistant content is complete but
the server-tool loop hit its limit). The session layer resends this content to continue.

## 6. Continue Loop Design

### 6.1 Continue Prompt

A new constant, separate from compaction's `AUTO_CONTINUE_PROMPT`:

```rust
/// Prompt injected after a truncated response to continue generation.
///
/// Uses a user message (not assistant message) per Anthropic's Claude 4.6+
/// error recovery guidance. For Claude 4.5 and earlier, the partial assistant
/// response is already in conversation history as the last assistant turn --
/// this prompt simply asks the model to continue.
pub const TRUNCATION_CONTINUE_PROMPT: &str = r#"Your previous response was interrupted. Continue from where you left off. Do not repeat what you already said. Resume directly."#;
```

**Note on Anthropic version differences**:
- Claude 4.5 and earlier: partial assistant response is persisted as the last assistant
  message. The continue prompt is a new user message. The model sees: `... -> assistant
  (partial) -> user ("continue from where you left off")`.
- Claude 4.6 and later: same pattern. Anthropic's own guidance for 4.6+ uses a user message
  containing the partial response text, but since Grow persists the partial response as a
  proper assistant turn, the user message only needs the continue instruction.

### 6.2 SyntheticReason

```rust
pub enum SyntheticReason {
    // ... existing variants ...
    /// Injected by the truncation-recovery logic after a max_tokens
    /// truncation. Not real user input.
    TruncationContinue,
}
```

### 6.3 Continue Loop Pseudocode

```text
function run_turn_with_continue(conversation, sampling_config):
    accumulated_text = ""
    accumulated_tool_calls = []
    continue_count = 0

    loop:
        outcome = sampler.run_request_task(conversation, sampling_config)

        match outcome:
            Completed { response }:
                # Merge accumulated text with final response text
                response.assistant_text = accumulated_text + response.assistant_text
                response.assistant_tool_calls = accumulated_tool_calls + response.assistant_tool_calls
                response.continue_count = continue_count
                return Ok(response)

            Truncated { partial_response }:
                # 1. Sanitize: discard incomplete thinking blocks, incomplete tool_use
                sanitized = sanitize_partial_response(partial_response)

                # 2. Persist partial response to conversation history
                conversation.push_assistant(sanitized)

                # 3. Accumulate for final merge
                accumulated_text += sanitized.assistant_text
                accumulated_tool_calls.extend(sanitized.assistant_tool_calls)

                # 4. Inject continue prompt
                conversation.push_user(ConversationItem::truncation_continue(TRUNCATION_CONTINUE_PROMPT))

                # 5. Increment counter
                continue_count += 1

                # 6. Loop back -- no limit (per user decision D2)
                continue

            ContextWindowExceeded { partial_response }:
                # Trigger inline compaction
                compacted = run_inline_compaction(conversation)
                # After compaction, re-loop (compaction's auto_continue handles the prompt)
                continue

            PauseTurn { response }:
                # Persist the complete response
                conversation.push_assistant(response)
                # Resend assistant content to continue (no prompt needed)
                # Anthropic expects the assistant content to be sent back as-is
                # This is handled by the messages backend's request construction
                continue

            Failed { error }:
                return Err(error)

            Empty { context }:
                # Existing retry behavior
                ...

            Cancelled / InitFailed:
                # Existing behavior
                ...
```

### 6.4 Partial Response Sanitization

When truncation occurs mid-generation, the partial response may contain:

1. **Complete text blocks**: Keep as-is.
2. **Incomplete text block** (truncated mid-sentence): Keep -- it's valid text, just unfinished.
   The continue prompt tells the model to resume.
3. **Complete thinking/reasoning blocks** (with valid signature): Keep.
4. **Incomplete thinking/reasoning blocks** (signature_delta not received): **Discard**.
   Anthropic API requires thinking blocks to have valid, complete signatures. Sending an
   incomplete thinking block causes a 400 error. This is an API-level hard constraint.
5. **Complete tool_use blocks** (valid JSON): Keep.
6. **Incomplete tool_use blocks** (truncated JSON): **Discard**.
   Anthropic: "incomplete tool use blocks cannot be used." The model will re-generate
   them on continue.

### 6.5 UI Considerations

During the continue loop:
- The user has already seen the partial output via streaming.
- The continue cycle produces new streaming tokens that are appended to the UI display.
- No special UI indicator is required for the continue itself (per D2: "user尽量无感知").
- The `continue_count` metadata is available for diagnostics/debugging but is not
  displayed to the user by default.

## 7. Provider-Specific Continue Behavior

### 7.1 Anthropic Messages

**Truncation (max_tokens)**:
- `stop_reason: "max_tokens"` -> `StopReason::Length` -> `Truncated`
- Continue: inject user message with `TRUNCATION_CONTINUE_PROMPT`
- Partial assistant response persisted as last assistant turn (with thinking blocks sanitized)

**Context window exceeded**:
- `stop_reason: "model_context_window_exceeded"` -> `StopReason::ModelContextWindowExceeded`
- Recovery: trigger inline compaction

**Pause turn**:
- `stop_reason: "pause_turn"` -> `StopReason::PauseTurn`
- Recovery: resend assistant content (the messages backend constructs a request that
  includes the last assistant message, signaling continuation)

### 7.2 OpenAI Responses

**Truncation (incomplete)**:
- `status: "incomplete"` -> `StopReason::Length` -> `Truncated`
- Continue: inject user message with `TRUNCATION_CONTINUE_PROMPT`
- Partial assistant response persisted as conversation item
- No `previous_response_id` chaining (Grow doesn't use it; manual state management)

**Reasoning items**:
- Encrypted reasoning content is included in requests (via `include` field)
- If reasoning was truncated mid-stream, the partial reasoning items are discarded
  (same principle as Anthropic thinking blocks -- incomplete encrypted content cannot
  be validated)

### 7.3 OpenAI Chat Completions

**Truncation (length)**:
- `finish_reason: "length"` -> `StopReason::Length` -> `Truncated`
- Continue: inject user message with `TRUNCATION_CONTINUE_PROMPT`
- Partial assistant message persisted to messages array

## 8. ModelContextWindowExceeded Recovery

### 8.1 Detection

When the model returns `stop_reason: "model_context_window_exceeded"` (Anthropic Claude 4.5+),
or when Grow detects that input + output tokens approach the context window limit:

- `AttemptOutcome::ContextWindowExceeded { partial_response }` is returned.

### 8.2 Recovery: Inline Compaction

The session layer triggers inline compaction (same as the existing auto-compact mechanism):

1. Persist the partial response (sanitized).
2. Run compaction on the conversation history.
3. After compaction, the existing `auto_continue` mechanism injects the compaction
   continue prompt.
4. The sampler is re-called with the compacted conversation.

This is distinct from truncation continue: the problem is that the **input** is too large,
not that the **output budget** was exhausted. More output budget (continue) would not help
because the context window is full.

### 8.3 Fallback

If compaction fails or the compacted conversation still exceeds the context window:
- Emit `StopFailure` with `MaxOutputTokens` kind (the closest existing classification).
- Turn fails with diagnostic message.

## 9. PauseTurn Recovery

### 9.1 Current (Wrong) Behavior

`messages.rs:374-381`: Anthropic `PauseTurn` is mapped to `StopReason::Stop` with a warning.
The turn ends as if the model finished naturally.

### 9.2 Correct Behavior

`PauseTurn` means the provider's server-side tool loop hit its iteration limit.
The assistant content is complete -- the model just needs to be re-prompted to continue
the turn.

Recovery:
1. Return `AttemptOutcome::PauseTurn { response }` (response is complete).
2. Session layer persists the response.
3. Session layer re-calls the sampler with the same conversation (the assistant
   message is already the last message; Anthropic will continue from it).
4. No continue prompt is needed -- Anthropic expects the assistant content to be
   resent as-is.

### 9.3 Implementation Note

The messages backend request construction already sends the full conversation including
the last assistant message. For PauseTurn, the continue is simply a new API call with
the same conversation state -- no special prompt injection.

## 10. ACP / Hook Event Impact

### 10.1 StopFailure Behavior Change

**Before**: Every `max_tokens` truncation emits `StopFailure { error: "max_output_tokens" }`
to external hook scripts.

**After**: 
- Successful auto-continue: **no StopFailure emitted**. The turn completes normally.
- Unrecoverable truncation (compaction fails, context window exhausted after compaction):
  `StopFailure { error: "max_output_tokens" }` emitted.

This is a **public contract change**. External hook scripts that relied on receiving
`StopFailure` for every truncation will no longer receive it when auto-continue succeeds.

### 10.2 Migration Note

This change is intentional and correct: a successfully continued turn is not a failure.
Hook scripts should only observe `StopFailure` when the turn genuinely fails. The
`MaxOutputTokens` StopFailureKind variant is retained for the unrecoverable case.

### 10.3 ACP Error Mapping

`map_sampling_err_to_acp` (`grow-shell/src/sampling/error.rs`):
- `MaxTokensTruncation` -> `acp::Error::internal_error()` with `error_kind: "max_tokens_truncation"`
- This mapping is retained but only triggered when truncation is truly unrecoverable.
- During auto-continue, no ACP error is produced.

## 11. Invariants and Constraints

### 11.1 Hard Constraints (API-level)

1. **Thinking blocks must be complete and unmodified** when sent back to Anthropic.
   Incomplete thinking blocks (missing `signature_delta`) must be discarded. (Anthropic
   API 400 error otherwise.)
2. **Tool-use blocks must be complete** (valid JSON). Incomplete tool-use blocks must be
   discarded. (Anthropic: "incomplete tool use blocks cannot be used.")
3. **`max_tokens` is a hard cap**: the model will be cut off mid-token at the limit.
   No API provides "soft landing" or "finish the sentence" behavior.
4. **Anthropic `max_tokens` is required**: Grow uses `ANTHROPIC_DEFAULT_MAX_TOKENS = 128_000`
   as fallback when `output_limit` is unset.

### 11.2 Design Invariants

1. **Continue loop has no count limit** (per D2). The loop terminates only when:
   - The model returns a non-`Length` stop reason, OR
   - `ContextWindowExceeded` triggers compaction, OR
   - A genuine API error occurs (rate limit, auth, network), OR
   - The user cancels.
2. **Partial output is always persisted** before continue. The conversation history
   always contains the partial assistant response (sanitized).
3. **The sampler remains stateless**: it does not own conversation history. The continue
   loop is in the session layer.
4. **`retry_only_before_output` is not violated**: continue is not a retry -- it's a new
   sampling cycle with an updated conversation. The existing retry-once-observed-output
   guard remains intact.
5. **Continue prompt is synthetic**: tagged with `SyntheticReason::TruncationContinue`,
   excluded from "real user query" extraction (same pattern as `AutoContinue`).

### 11.3 What This Architecture Does NOT Do

- Does not use `previous_response_id` for OpenAI Responses (Grow doesn't use it;
  manual state management is consistent across providers).
- Does not attempt to reconstruct/merge incomplete thinking blocks (API constraint).
- Does not add a config toggle for auto-continue (per D2: always on).
- Does not add a continue count limit (per D2: no limits).
- Does not change compaction's `auto_continue` mechanism (separate concern).
- Does not change the `EmptyResponse` or `DoomLoopDetected` recovery paths.

## 12. Testing Requirements

### 12.1 Unit Tests

| Test | Layer | Description |
|---|---|---|
| `stop_reason_mapping` | grow-sampling-types | Anthropic `max_tokens` -> `Length`; `model_context_window_exceeded` -> `ModelContextWindowExceeded`; `pause_turn` -> `PauseTurn` |
| `truncated_outcome_carries_partial` | grow-sampler | `drive_l2` returns `Truncated` with partial response when stop_reason == Length |
| `context_window_exceeded_outcome` | grow-sampler | `drive_l2` returns `ContextWindowExceeded` when stop_reason == ModelContextWindowExceeded |
| `pause_turn_outcome` | grow-sampler | `drive_l2` returns `PauseTurn` with complete response |
| `incomplete_thinking_discarded` | grow-sampler | Partial response sanitization discards thinking blocks without signature_delta |
| `incomplete_tool_use_discarded` | grow-sampler | Partial response sanitization discards tool_use blocks with invalid JSON |
| `max_tokens_truncation_not_produced_for_length` | grow-sampler | `SamplingError::MaxTokensTruncation` is not produced for Length stop reason |
| `synthetic_reason_truncation_continue` | grow-sampling-types | New variant serializes/deserializes correctly; excluded from real-user-query extraction |

### 12.2 Integration Tests

| Test | Scope | Description |
|---|---|---|
| `truncation_auto_continue_e2e` | grow-shell | Mock provider truncates at N tokens; assert: (1) partial response persisted, (2) continue prompt injected, (3) final output is concatenation, (4) turn completes successfully |
| `truncation_multiple_continues` | grow-shell | Mock provider truncates 3 times then completes; assert all partials persisted and merged |
| `truncation_thinking_block` | grow-shell | Mock provider truncates mid-thinking; assert incomplete thinking discarded, complete thinking retained |
| `truncation_tool_use_incomplete` | grow-shell | Mock provider truncates mid-tool_use JSON; assert incomplete tool_use discarded, model re-generates on continue |
| `context_window_exceeded_triggers_compaction` | grow-shell | Mock provider returns model_context_window_exceeded; assert compaction triggered, not continue |
| `pause_turn_resend` | grow-shell | Mock provider returns pause_turn; assert response persisted, sampler re-called without continue prompt |
| `cross_backend_consistency` | grow-shell | Messages and chat_completions produce the same continue behavior (chat_completions e2e: partial persisted, continue prompt injected, concatenated output, 2 sampling cycles; responses backend pinned at the stream layer by Task 2) — DONE |
| `stop_failure_not_emitted_on_success` | grow-shell | Successful continue: no StopFailure hook event emitted — DONE |
| `stop_failure_emitted_on_unrecoverable` | grow-shell | Unrecoverable recovery failure (compaction HTTP 500): turn fails, StopFailure hook event emitted — DONE |
| `model_context_window_exceeded_test_update` | grow-sampler | Update `messages_tests.rs:378` test to assert new ContextWindowExceeded outcome, not MaxTokensTruncation — DONE (Task 2, renamed `model_context_window_exceeded_completes_with_context_window_stop_reason`) |

### 12.3 Regression Tests

| Test | Scope | Description |
|---|---|---|
| `compaction_auto_continue_unchanged` | grow-shell | Compaction's auto_continue mechanism is not affected |
| `empty_response_retry_unchanged` | grow-sampler | EmptyResponse still triggers retry |
| `doom_loop_detection_unchanged` | grow-sampler | DoomLoopDetected recovery budget unaffected |
| `retry_only_before_output_unchanged` | grow-sampler | output_observed still prevents retry (but not continue) |

### 12.4 Forbidden Test Patterns

- Do not test only that `AttemptOutcome::Truncated` is produced -- must test end-to-end
  that the conversation history contains the partial output after continue.
- Do not test only one backend -- all three backends must be tested for continue behavior.
- Do not mock the continue prompt text -- test that the correct synthetic user message
  is injected with the correct `SyntheticReason`.

## 13. Task Decomposition

### Task 0: Crate Rename `xai-chat-state` -> `grow-chat-state`
**Crate**: xai-chat-state (renamed to grow-chat-state), grow-shell (references only)
**Scope**: Pure mechanical rename, zero behavior change:
1. `git mv crates/codegen/xai-chat-state crates/codegen/grow-chat-state` (preserves git history)
2. `grow-chat-state/Cargo.toml`: `name = "grow-chat-state"`, version per D8
   (`version.workspace = true` recommended -- resolves to 1.0.0, consistent with all
   other grow-* crates; alternatively explicit `version = "1.0.0"` if independent
   versioning is desired -- confirm with user)
3. Root `Cargo.toml`: workspace members entry update
4. `grow-shell/Cargo.toml:134`: dependency name + path update
5. All code references `xai_chat_state::` -> `grow_chat_state::` (100+ in grow-shell)
6. All doc comments `xai-chat-state` -> `grow-chat-state` (cross-crate comments:
   grow-shell x5, grow-sampling-types x2, grow-subagent-resolution x2,
   grow-compaction x2, grow-chat-state internal x4, this document)
7. `Cargo.lock` updated automatically by cargo
**File ownership**: root `Cargo.toml`, `grow-shell/Cargo.toml`,
`crates/codegen/grow-chat-state/**`, `grow-shell/src/**`
**Dependencies**: None (base layer, runs first).
**Forbidden**: no logic changes, no refactoring, no formatting of unrelated code,
no cleanup of the `default-bazel` feature (legacy, out of scope), no semantic changes.
**Tests**: `cargo build --workspace` green; `cargo test` for grow-shell and
grow-chat-state green; grep confirms zero residual `xai_chat_state` / `xai-chat-state`.
**Known observable change (documented)**: tracing log targets start with crate name
(`xai_chat_state::...` -> `grow_chat_state::...`); fine-grained `RUST_LOG=xai_chat_state=...`
filters become ineffective. Low risk (internal crate), recorded here.
**Reject if**: any logic code touched; compile/test failures; residual old-name references.

### Task 1: StopReason and AttemptOutcome Type Changes
**Crate**: grow-sampling-types; grow-sampler (request_task.rs only)
**Scope**: Add `ModelContextWindowExceeded` and `PauseTurn` to `StopReason`; add
`Truncated`, `ContextWindowExceeded`, `PauseTurn` to `AttemptOutcome`; add
`TruncationContinue` to `SyntheticReason`; add `ConversationItem::truncation_continue()`
constructor (content passed in by caller -- no prompt constant here).
**Compile-impact note (verified by architect)**: The new enum variants force
exhaustive-match updates in exactly two places, both in grow-sampling-types:
`StopReason::as_str()` and `SyntheticReason::starts_prompt_turn()`. All other
`SyntheticReason` matches use `Some(_)`/`Unknown` wildcards; grow-shell's
`StopReason` references are the unrelated `acp::StopReason`. `AttemptOutcome` is
defined in `grow-sampler/src/actor/request_task.rs` (not grow-sampling-types);
adding variants forces `run_request_task`'s match to add temporary branches.
Temporary branches must preserve current behavior (fatal: emit_failed +
send_completion(Err)) with a `TODO(Task 2)` marker; the `drive_l2` Length check at
`request_task.rs:622-626` stays untouched (Task 2 owns it).
**Dependencies**: Task 0 (workspace must compile with new crate name first).
**Tests**: Serialization round-trip for new variants; `as_str()` for new StopReason
variants; `starts_prompt_turn() == false` for `TruncationContinue`; `truncation_continue()`
constructs a User item tagged `Some(SyntheticReason::TruncationContinue)`.
**Reject if**: Changes touch files outside grow-sampling-types and
`grow-sampler/src/actor/request_task.rs`; changes compaction's `AutoContinue` semantics;
adds prompt text constants (owned by Task 3); changes `request_task.rs:622-626` or
stream-layer stop-reason mapping (owned by Task 2).

### Task 2: Stream Layer Stop Reason Mapping
**Crate**: grow-sampler
**Scope**: Update `stream/messages.rs`, `stream/responses.rs`, `stream/chat_completions.rs`
to map wire stop reasons to the new `StopReason` variants. Update `messages.rs:368-392`
to split `MaxTokens` / `ModelContextWindowExceeded` / `PauseTurn`. Update `responses.rs:453`
for `Incomplete` -> `Length`. Update `request_task.rs:622-626` to emit `Truncated` instead
of `Failed` for `Length`.
**Dependencies**: Task 1.
**Tests**: `stop_reason_mapping` per backend; `truncated_outcome_carries_partial`;
`context_window_exceeded_outcome`; `pause_turn_outcome`; `incomplete_thinking_discarded`;
`incomplete_tool_use_discarded`; update `messages_tests.rs:378`.
**Reject if**: Changes touch grow-shell; changes retry.rs retry classification for
non-truncation errors; changes compaction path.

**Implementation notes (Task 2, verified)**:
- No new `SamplingEvent` variant and no new `ConversationResponse` field: the
  truncation signal is `response.stop_reason` itself. `run_request_task` reuses
  `SamplingEvent::Completed` + `send_completion(Ok(partial_response))` for all
  three truncation-class outcomes, so the wire contract is unchanged and the
  session layer picks its strategy (continue / compact / resend) from the
  stop_reason. The L2 stream's terminal event is suppressed by `run_one_attempt`
  (see `request_task.rs`), so the emit in `run_request_task` is the only
  terminal event the session sees.
- No retry: truncation-class outcomes return directly without entering the
  retry decision (resampling with the same parameters would truncate again).
- `doom_check` still outranks truncation classification in `drive_l2`.
- The ToolCalls override (completed `tool_use` blocks win over a terminal
  `Length`) is retained: a completed tool call is real model output the agent
  loop must resolve. Behavior change pinned by test: previously
  "completed tool_use + truncation" failed and discarded everything; now it
  completes with `ToolCalls` and the calls are executed.
- In-progress blocks (no `ContentBlockStop`) never enter the partial response:
  this is a natural consequence of the existing construction logic (no new
  sanitization code), and matches Anthropic's "thinking/tool_use blocks cannot
  be partially recovered". Locked by tests
  `truncated_incomplete_thinking_block_discarded` /
  `truncated_incomplete_tool_use_discarded`.

### Task 3: Continue Loop and Session Layer
**Crate**: grow-shell, grow-chat-state (prompt constant only)
**Scope**: Implement continue loop; implement continue prompt injection
using `TRUNCATION_CONTINUE_PROMPT`; add `TRUNCATION_CONTINUE_PROMPT` to
`grow-chat-state/src/compaction_utils.rs` (next to `AUTO_CONTINUE_PROMPT` at
`compaction_utils.rs:309`) and extend `is_synthetic_extracted_query`
(`compaction_utils.rs:329-334`) to exclude it; implement `ContextWindowExceeded` ->
compaction trigger; implement `PauseTurn` resend; update `turn_end.rs` StopFailure
classification; update `sampling/error.rs` ACP mapping.

**Verified insertion point** (facts from code, Task 3 preparation):
- The continue loop lives in the outer turn loop's Response-consumption segment in
  `turn.rs` (NOT `sampler_turn.rs`): `run_turn_via_sampler` (`sampler_turn.rs:1009-1063`)
  returns `SamplerTurnOutcome::Response(response, metrics)`; `turn.rs:2135-2184` destructures
  it, then the segment at `turn.rs:2292-2306` reads `stop_reason` and persists every
  `response.item` into chat state (`record_assistant_response` -> `push_assistant_response`,
  others -> `push_tool_result`). **Partial content is already persisted by this existing
  loop** — the continue branch must be inserted AFTER this loop (line 2306) and BEFORE the
  `tool_calls.is_empty()` turn-end check (line 2340), so the persisted partial is what the
  next request builds from.
- `build_request` (`grow-chat-state/src/actor/request_builder.rs:35-135`) clones the full
  conversation into the request (hot path) — synthetic user items (including the injected
  `truncation_continue` item) are carried through unchanged; there is no filtering of
  synthetic user items.
- `ConversationItem::truncation_continue(content)` (Task 1) constructs the injected item;
  its `prompt_index` is `None` and `starts_prompt_turn()` is `false` (locked by Task 1
  tests), so injection does not advance the prompt counter.
- `run_compact_only(trigger_info: AutoCompactTriggerInfo)` (`compaction.rs:2041`) is the
  compaction entry used by both the error path (`sampler_turn.rs:700-729`) and pre-sampling
  checks; `AutoCompactTriggerInfo` needs `tokens_used` / `context_window` / `percentage`
  (use `get_estimated_total_tokens()` + `get_sampling_config().context_window` +
  `xai_token_estimation::usage_percentage_u8`). For `ModelContextWindowExceeded` compaction
  must be triggered **unconditionally** (client-side estimation can under-count; the server
  reported the overflow), not gated on `check_auto_compact_needed()`.

**Branch semantics** (inserted after the items-persistence loop, only when
`tool_calls.is_empty()`; a completed `tool_use` with `Length` still falls through to tool
execution — tool_use wins, per Task 2 decision):
- `Some(StopReason::Length)` -> push `truncation_continue(TRUNCATION_CONTINUE_PROMPT)` user
  item into chat state, log (`tracing::info` + `unified_log`), `continue` the outer loop.
  The next iteration rebuilds the request (new `max_output_tokens` from config) and
  resubmits. No count limit, no config toggle (user decisions).
- `Some(StopReason::ModelContextWindowExceeded)` -> `run_compact_only(trigger)` +
  `continue` (compaction injects its own auto-continue; the outer loop rebuilds from the
  compacted conversation). Compact failure follows the existing pattern (`is_auth_compact_error`
  -> `surface_compact_auth_failure`, else surface error).
- `Some(StopReason::PauseTurn)` -> resend-to-continue WITHOUT injecting any prompt:
  the assistant content (text + completed tool_use blocks + thinking blocks with signatures)
  is already persisted by the items loop, and `build_request` carries it back verbatim;
  the Anthropic messages backend must already round-trip assistant `tool_use` blocks
  (API hard requirement) and thinking signatures (`ReasoningItem.signature`,
  `conversation.rs:3088-3100`). Log and `continue`.
- Any other stop reason (including `None`, `Stop`, `ToolCalls`, `ContentFilter`) ->
  existing turn-end path untouched.

**MaxTokensTruncation after Task 2**: no production code constructs it anymore (grow-sampler
stream/actor paths are clean; grow-shell `OaiCompatClient` never produced it). The
`map_sampling_err_to_acp` branch (`sampling/error.rs:94-100`), `stop_reason_for_turn_error`
(`:136-144`), `StopFailureKind::MaxOutputTokens` classification (`turn_end.rs:348-352`) and
`classify_sampling_error` (`session_compact.rs:73`) are retained as defensive dead code —
they stay, unchanged, so a future unrecoverable-truncation path can reuse the hook contract
(Q5 decision). No new code should route to them.

**Dependencies**: Task 0, Task 1, Task 2.
**Tests**: All integration tests from section 12.2; all regression tests from 12.3.
**Reject if**: Changes touch grow-sampler stream layer (already owned by Task 2); changes
compaction's auto_continue mechanism; adds continue count limit or config toggle; adds new
SamplingError variants or new ACP error codes.

**Implementation notes (Task 3 done, all verified)**:
- The three branches are implemented in `turn.rs` at the documented insertion point (after
  the items-persistence loop, before the `tool_calls.is_empty()` turn-end check), all guarded
  by `tool_calls.is_empty()`. `Length` -> `push_user_message(truncation_continue(TRUNCATION_CONTINUE_PROMPT))`
  + `tracing::info` + `unified_log` + `continue`; `ModelContextWindowExceeded` -> build
  `AutoCompactTriggerInfo` from `get_estimated_total_tokens()` /
  `get_sampling_config().context_window` / `usage_percentage_u8`, `run_compact_only`,
  `is_auth_compact_error` -> `surface_compact_auth_failure`, else `Err`, then `continue`;
  `PauseTurn` -> log + `continue` (no prompt injection). Plus one `persisted_items` counter
  (`response.items.len()`) for the logs.
- **Test thread stack**: the truncation-recovery integration tests overflow the default 2MB
  test-thread stack (deterministic abort). Root cause: the turn loop's async state-machine
  chain needs 2-4MB (`handle_prompt` frame alone is ~63KB; mock servers complete synchronously,
  stacking several turn iterations into one poll). Production is safe: the session actor
  thread is spawned with `SESSION_THREAD_STACK_SIZE = 8MB` (`spawn.rs:2099`). All tests in
  `truncation_recovery_tests.rs` therefore run via a `run_with_session_stack` helper that
  spawns an 8MB-stack thread (matching production) with an explicit current-thread runtime +
  `LocalSet`. New tests in this file MUST use the same helper.
- `cross_backend_consistency` covers **chat_completions** (`finish_reason: "length"` ->
  `StopReason::Length` -> same continue branch) in addition to the Messages-side e2e. The
  responses backend is pinned by the stream-layer tests (Task 2): its wire has no
  length-equivalent stop reason beyond `incomplete`, and the session branches are backend-
  agnostic (they only read `response.stop_reason`).
- `context_window_exceeded_triggers_compaction` needs (a) a System item in chat state
  (`replace_system_head`; production session startup injects it, the bare test actor does
  not), (b) a summary text clearing the sampler's `is_degenerate_summary` gate
  (>= 500 chars), and (c) asserts the summary in history via full-text scan — compaction
  persists it as a `User(CompactionMeta)` item, not an Assistant item.
- Test-wire fixes discovered during implementation: `messages_turn` start events must carry
  empty content (the stream layer seeds block accumulators from `content_block_start`; the
  real Anthropic wire sends content only via deltas), and incomplete blocks are simulated by
  omitting `content_block_stop` (internal `__no_stop` marker, stripped before the wire).
- **Known limitation**: `truncation_thinking_block` sends a complete thinking block on the
  wire (the incomplete-thinking case is pinned precisely by Task 2's
  `truncated_incomplete_thinking_block_discarded` at the stream layer); its session-level
  assertion relies on the single reasoning slot. Acceptable duplicate coverage; do not treat
  it as a proof of incomplete-thinking discard.

### Merge Order
Task 0 -> Task 1 -> Task 2 -> Task 3 (strict sequential: rename first so all feature
code uses the new crate name; then type dependencies).

### Shared Contract Ownership
- Crate name `grow-chat-state` (rename): Task 0 (only owner)
- `StopReason` enum: Task 1 (only owner)
- `AttemptOutcome` enum: Task 1 (only owner)
- `SyntheticReason` enum: Task 1 (only owner)
- `TRUNCATION_CONTINUE_PROMPT`: Task 3 (owned, in grow-chat-state/src/compaction_utils.rs)
- Stream stop-reason mapping: Task 2 (only owner)
- Continue loop: Task 3 (only owner)

## 14. Review Plan

### 14.1 Per-Task Review

**Task 0**: Verify it is a pure rename -- no logic changes; verify `grow-chat-state`
name and version correct; verify zero residual `xai_chat_state` / `xai-chat-state`
references (code and comments); verify workspace builds and tests pass.

**Task 1**: Verify new variants are serializable; verify `SyntheticReason::TruncationContinue`
is excluded from `is_synthetic_extracted_query`; verify no changes to existing
`AutoContinue`/`AutoRecovery` semantics; verify no prompt constants added (Task 3 owns them).

**Task 2**: Verify all three backends map correctly; verify `MaxTokensTruncation` is
no longer produced for `Length`; verify `ModelContextWindowExceeded` is mapped correctly;
verify `PauseTurn` is mapped to new variant (not `Stop`); verify partial response is
constructed with sanitized thinking/tool_use blocks.

**Task 3**: Verify continue loop persists partial response before injecting prompt;
verify no count limit; verify `ContextWindowExceeded` triggers compaction (not continue);
verify `PauseTurn` resend does not inject continue prompt; verify `StopFailure` not
emitted on successful continue; verify `retry_only_before_output` not violated; verify
`TRUNCATION_CONTINUE_PROMPT` lives in `grow-chat-state/src/compaction_utils.rs`, separate
from `AUTO_CONTINUE_PROMPT`, and is used with `SyntheticReason::TruncationContinue`.

### 14.2 Final Integration Review

- Crate rename: workspace builds green; zero residual `xai_chat_state` / `xai-chat-state`
  references anywhere in the repo; `grow-chat-state` version resolved correctly.
- End-to-end: user sends long request -> truncation -> auto-continue -> completion ->
  conversation history contains full output -> next turn model sees complete output.
- Cross-backend: all three backends produce identical continue behavior.
- Regression: compaction, EmptyResponse, DoomLoopDetected, retry_only_before_output
  all unaffected.
- Hook events: `StopFailure` only emitted on unrecoverable failure.
- Documentation: this document updated if implementation reveals constraints not
  captured here.

## 15. Open Questions for Implementation

1. **~~Continue prompt location~~** (RESOLVED): `TRUNCATION_CONTINUE_PROMPT` lives in
   `grow-chat-state/src/compaction_utils.rs` (next to `AUTO_CONTINUE_PROMPT`), per user
   decision. The `ConversationItem::truncation_continue(content)` constructor lives in
   `grow-sampling-types` (content passed in by caller) -- matching the existing
   `AUTO_CONTINUE_PROMPT` / `ConversationItem::auto_continue()` split.

2. **~~Version mechanism for grow-chat-state~~** (RESOLVED): `version.workspace = true`
   (user confirmed option A; resolves to 1.0.0, consistent with all grow-* crates).

3. **~~PauseTurn resend mechanics~~** (RESOLVED): Plain resend via the existing
   persistence + rebuild path is sufficient. `build_request`
   (`grow-chat-state/src/actor/request_builder.rs:35-135`) clones the full conversation
   verbatim (hot path), so once the partial assistant items are persisted by the
   turn.rs items loop, the next request carries text + completed `tool_use` blocks +
   thinking blocks (with `signature`, `conversation.rs:3088-3100`) back unchanged —
   exactly what Anthropic's pause_turn semantics require. No special request
   construction needed; the PauseTurn branch logs and continues. Implementation must
   still verify the messages backend round-trips assistant `tool_use` blocks (Anthropic
   API hard requirement) with one test.

4. **OpenAI reasoning item truncation**: If reasoning items are truncated mid-stream,
   are they discarded the same way as Anthropic thinking blocks? Implementation should
   verify OpenAI's behavior with incomplete reasoning items.
