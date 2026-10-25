## Reproduction

Before the fix, the cold-restore regression reported 20428 tokens for a portable replay containing 428 tokens; the extra 20000 came from retained reasoning that is not sent after continuation reset.

## Verification

- `RUST_MIN_STACK=16777216 cargo test --locked -p chat-state --lib -- --test-threads=4 --quiet` passed: 576 passed, 3 existing ignored tests.
- The new restore test covers Chat Completions, Responses and Messages, both omitted-reasoning inflation and genuine portable overflow. It verifies unchanged durable content, no initialization writes, one signed append delta, and stable first/repeated request projections.
- The seed-state test now explicitly checks that an orphan tool result remains in raw history while contributing no portable request pressure. Two pruning fixtures now include their matching tool call, retaining their original underreported-anchor and zero-budget assertions against provider-visible exchanges.
- Shell's `resumed_context_usage_is_portable_before_first_prompt` passed, checking both `build_session_info` and the command-advertisement metadata used at the end of session load. The existing `build_session_info_used_reflects_recorded_response` also passed, preserving live provider accounting. Both used `RUST_MIN_STACK=16777216` and the freshly built Shell test artifact.
- `cargo fmt --all -- --check`, `git diff --check`, and pre-archive `openspec validate --all --strict --no-interactive` passed (17 items).
- After archival, full OpenSpec validation passed (16 items) and archive task validation passed (709 items). The incremental caches generated for ChatState and Shell were removed after Rust builds stopped.

The initial value remains a portable-message estimate; tools, schemas, current Goal/image projection and provider observations are applied through their existing request path. No exact cold provider measurement or unchanged resume latency is claimed.
