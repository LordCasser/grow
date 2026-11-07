# Runtime admission integration verification — 2026-11-06

Entry point: `crates/codegen/shell/tests/test_advisory_question_admission.rs` (single test
`advisory_ask_admits_bounded_presentation_and_continues_through_real_chain`), built on the
`test_registry_churn` harness — real in-process `MvpAgent` on duplex ACP pipes
(`connect_agent_v1` / `connect_client_v1`), loopback `MockInferenceServer` scripting the
provider via named foreground chat-completions expectations. No hand-built
`UserQuestionRequest`, no forged tool output; the ask tool call is a real model tool call and
the tool result reaches the provider only through the full chain (model tool call →
Factory-built toolset `Tool::run` → shell worker → real ACP reverse request → client reply).

The client side is a protocol-peer fixture (`AcpClientHandler::ext_method`), not the Pager
UI: it parses the typed `AskUserQuestionExtRequest` from the wire and hands the reply sender
to the test, which decides when and what to answer. Pager card/UI behavior remains a
separate pending milestone item.

## Scenarios verified (all inside the one test, one alive session, unique call ids)

1. **Advisory admission (default preference)** — provider emits
   `ask_user_question(blocking=false, questions=[valid recommended_option])`. Client
   observes the effective `blocking=false` on the wire (`toolCallId` `call-advisory-1`),
   replies `{"outcome":"presented"}` only. The provider receives the next model request
   carrying exactly one tool settlement for the call, with the Agent recommendation and an
   explicit `the user has NOT answered` marker; exactly one assistant `tool_calls` entry for
   the id.
2. **Pub setter disables, same alive session** — `shell::util::config::
   set_ask_user_question_allow_non_blocking(false)`; the model still requests
   `blocking=false`, but the client observes effective `blocking=true` on the wire
   (`call-blocking-2`). Causal barrier: (a) the reverse request provably arrived at the
   client; (b) a bounded 750 ms observation window shows no further foreground provider
   request while the real reverse future is unanswered; (c) only the client's real
   `{"outcome":"accepted"}` releases the turn (EndTurn), and the settled result is the user
   answer (`User has answered`). The earlier advisory settlement appears in that request's
   history exactly once (history, not re-settlement).
3. **Re-enable live** — setter `true`; the next admission is advisory again
   (`call-advisory-3`, `Presented`, continuation consumed).

Model request projection counting: the final foreground request carries exactly one
`role:"tool"` result per ask call id (`call-advisory-1`, `call-blocking-2`,
`call-advisory-3`). This proves the sampled history contains one result per call; the test
does not separately read the persisted Timeline to prove its event count.

## Isolation

Tempdir `GROW_HOME` + workdir (config written into the temp home; setter also writes only
there), `GROW_API_KEY=test-key-for-ci`, loopback mock provider only — no real API, no real
user configuration writes. Explicit provider configuration points at the loopback server;
this test does not independently audit every startup resource-discovery path. Single
`#[test]` for the env-mutation safety argument
(same as `test_registry_churn`).

## Command, counts, exit

```
RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 PATH=~/.cargo/bin \
  cargo +1.93.1 test --locked --offline -p shell --test test_advisory_question_admission
```

Final run: `test result: ok. 1 passed; 0 failed; 0 ignored` — exit 0, zero warnings
(log `/tmp/grow-ask-runtime-admission.log`; earlier iterations logged two compile fixes —
trait import and acp error constructor — plus removal of unneeded `mut`, no assertion
changes). Only this integration target was built/run; no workspace-wide build, no fmt.

## Parent acceptance

父阅读实际测试入口、三个场景的因果barrier及最终日志（末轮1 passed、exit0、无warning），接受Shell/Tools/ACP协议对端的runtime准入结果。明确上文count来自下一模型请求，不能独立称canonical日志事件证明；Pager真实展示、迟答及其持久去重仍未验收。

## Limits / pending

- Client fixture replies `Presented`/`Accepted` per plan; the real Pager card, late-answer
  as new user input (`question-reply:` identity), Settings UI registry, failure/unknown-ACK
  recovery, and child/StoppedGoal boundaries remain on the milestone plan's later items.
- No production code was changed in this round.
