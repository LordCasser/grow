# Verification

## Confirmed defect and minimal fix

The production command/run-loop regression reproduced the original child-grant defect: three completed malformed samples with 5 output tokens each left an 80-token grant at `(80, incomplete)` instead of `(15, complete)`. The sampler had already acknowledged all three attempt settlements. The redundant outer failure handler caused the false exhaustion; removing it preserves exact consumption and remaining capacity. Unknown usage and failed ledger writes retain their existing authoritative closure in `sampling_usage_sink`.

The adversarial boundary is a missing ordered sampler terminal. That is an unconfirmed local lifecycle, rather than an ordinary malformed generation. A second red regression showed that simply removing outer exhaustion would leave `(15, complete)` after the terminal was lost; the new targeted stream-drained-barrier closure instead yields a closed incomplete grant. Earlier exact ledger entries remain intact. Test-controlled persistence also holds the failed Turn terminal ACK while the successor input arrives; no test mutates foreground to Idle or calls completion settlement manually.

## Behavior coverage

| Entry point | Covered invariants |
| --- | --- |
| Built binary, ACP, three provider protocols, retractable client | Three complete malformed samples end one Turn; healthy manual `继续` succeeds; another Turn can use two retries and then succeed; process PID and resident control epoch remain unchanged; attempt ledger keys are unique; valid tool history is retained; rejected calls and siblings never execute. Existing model switch and cold process restart coverage is retained. |
| Built binary, ACP, three protocols, append-only client | Observed output retains its existing no-resampling boundary; manual recovery still succeeds. Client capability is explicitly negotiated and does not depend on the client name. |
| Built binary, malformed wire JSON, three protocols | Deterministic protocol failure does not gain automatic retries; the same resident actor accepts the next user prompt with valid history. |
| Built binary, Goal with known remaining budget | Three failed samples account 45 total tokens, pause autonomous work, and leave manual interaction available outside the stopped Goal. Explicit `/goal restart` finishes the same Goal and retains earlier spending. |
| Built binary, exhausted or unknown Goal budget | Two known attempts consume a 30-token budget; an unknown attempt pauses an exact budget after one request. Manual interaction does not change those stopped Goal ledgers; explicit restart dispatches no autonomous request. |
| Production session run loop, budgeted child context | Known remaining grant, known exhausted grant, unknown usage and missing sampler terminal; a new prompt cannot replenish a grant. The first Turn terminal ACK is held while `继续` is submitted, and the successor starts only after the durable predecessor terminal. |

The budgeted context fixture exercises the production actor, sampler, ChatState event channel and ordinary command scheduler, with simulated persistence acknowledgements. It does not launch the full parent Task coordinator or simulate a cold child restore. Actual disk persistence and real process/session ownership are covered by the binary cases. No broader child-resume or sampler panic framework is introduced.

The production child wiring is confirmed in `shell/src/agent/subagent/handle_request.rs`: it constructs the same `TaskOutputTokenBudget` from `SubagentRuntimeOverrides.output_token_budget`, installs it in the child ToolContext before session spawn, then combines that shared grant's completeness with the durable child usage ledger when finalizing `SubagentResult`. The fix changes that shared grant's failure handling without changing parent result admission or granting a replacement budget.

## Execution record

Preliminary local runs used Homebrew Rust 1.99.1 because its binaries precede rustup in the host PATH: 259 sampler tests and 4264 shell tests passed, with 5 existing ignored shell tests. The binary protocol and malformed-wire cases passed; the final Goal case passed through the exact runner. These are diagnostic evidence, not the pinned release-toolchain result. Final verification explicitly selects the repository's Rust 1.93.1 toolchain.

Pinned local verification uses `PATH=/Users/lordcasser/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin` and `RUSTUP_TOOLCHAIN=1.93.1`, verified as rustc 1.93.1 and cargo 1.93.1 on aarch64 macOS. Builds use a fresh `/tmp/grow-sampling-recovery-193-target`, no incremental artifacts and stripped debug information.

- `cargo test --locked --lib -p shell -p sampler -- --test-threads=4`: **4523 passed**, **5 existing ignored** (259 sampler; 4264 shell), including all four budget/terminal cases.
- `cargo build --locked -p cli --bin grow`: passed with the pinned toolchain.
- The exact runner selected and passed **one test each** for `protocol_failure_preserves_session_across_continue_switch_and_process_restart`, `malformed_wire_json_allows_manual_continuation_in_the_same_resident_session`, and `goal_sampling_failure_requires_explicit_restart_and_preserves_budget_gates`. All used `/tmp/grow-sampling-recovery-193-target/debug/grow` from that build. The first test loops over three protocols and two delivery modes; the wire and Goal tests cover three cases each.
- Python exact-runner and release-DAG contract tests: **16 passed**. Ruby Psych parsed the core workflow and verified its just-built binary path and all three exact integration selections.

- Pinned `cargo fmt --all -- --check`, `git diff --check`, and `openspec validate --all --strict --no-interactive`: passed; OpenSpec reports **24 items passed, 0 failed**.
- Removed both task-owned Cargo target directories (`grow-sampling-recovery-target` and `grow-sampling-recovery-193-target`) and generated Python bytecode caches after validation. Only the small diagnostic logs remain under `/tmp`.

## Why the old coverage was insufficient

Sampler tests proved the bounded retry and settlement logic inside one request. Some adjacent Shell tests installed Idle manually between direct `handle_prompt` calls; they bypassed foreground completion and queue promotion. The pre-existing binary fixture also used an append-only client, so it did not negotiate the candidate-discard capability required for observed-output resampling. The updated binary test retains that append-only boundary and additionally exercises the retractable path. Its actor epoch, request count, durable ledger and filesystem assertions cover different layers of the same user operation. The delayed terminal-ACK fixture specifically protects the transition that the manual reset concealed.

## Release gate and scope

`core-regression.yml` runs all three binary tests through `run_exact_cargo_test.py` against its just-built `GROW_BINARY`, on the immutable release-selected commit. Library coverage is part of the same existing core gate. This adds no new workflow and keeps the missing/ignored/zero-test rejection behavior.

All work remains in the existing isolated hardening worktree. No MCP source, protocol, SDK or elicitation change is included. Native hosted Linux/Windows runs and an actual release are not performed by this local verification.
