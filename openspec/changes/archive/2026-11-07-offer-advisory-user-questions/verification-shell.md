# Task 2 (Shell admission) verification — 2026-11-06

Command env: `RUST_MIN_STACK=16777216 JOBS=2 PATH=~/.cargo/bin cargo +1.93.1 test -p shell --lib --locked --offline`.
Logs: `/tmp/grow-ask-shell-target.log` (compile), `/tmp/grow-ask-shell-regress.log` (test filters). Compile exit 0.

## Implemented (Tier 2 evidence below)

- `crates/codegen/shell/src/tools/config.rs`: `AskUserQuestionToolConfig.allow_non_blocking: Option<bool>`.
- `crates/codegen/shell/src/util/config/resolve/toolset.rs`: `resolve_ask_user_question_allow_non_blocking_from_disk() -> Result<bool, String>` — env `GROW_ASK_USER_QUESTION_ALLOW_NON_BLOCKING` > effective global TOML (`load_effective_config`, `$GROW_HOME/config.toml` only, no cwd tier) > `DEFAULT_ASK_USER_QUESTION_ALLOW_NON_BLOCKING` (true). Disk read/parse failure → `Err` (fail-closed), never defaults the gate open.
- `crates/codegen/shell/src/util/config/settings_writes.rs`: pub `set_ask_user_question_allow_non_blocking(bool) -> Result<()>` via existing `update_config`/SAVE_LOCK raw write.
- `crates/codegen/shell/src/util/config/persist.rs`: `merge_ask_user_question_section` early-return guard extended to all three `None` fields (was timeout-only; writing just `allow_non_blocking` would have been skipped).
- `crates/codegen/shell/src/session/agent_rebuild.rs` (Factory, initial/rebuild/fork single path): injects `UserQuestionPolicyResolver` next to `UserQuestionSender`. Base = finalized `Params<AskUserQuestionParams>` read back from the built agent via `ToolBridge::read_resource` (per parent review 2026-11-06; not a re-parse of the raw JSON map; `unwrap_or_default` only when the params resource is absent). Closure re-resolves only `allow_non_blocking` per call; timeout fields stay the build snapshot.
- `crates/codegen/shell/src/session/actor/spawn.rs` (worker): advisory branch (`request.blocking == false`) before the blocking path — after serial-queue dequeue checks receiver-closed and deadline (expired-in-queue sends no card, returns `TransportError`); bounded `select!` over shutdown / `result_tx.closed` / deadline / `gateway.ext_method`; only `Presented` accepted, other user responses → `MalformedResponse`; no elicitation hook, no `PendingInteractionGuard`, no durable Opened/Resolved, no second tool result, no `CANCEL_TEXT`. Blocking path: client `Presented` rewritten to `Err(MalformedResponse)` so durable resolution is `Failed` (previously recorded `Answered`).

## Verification results (all exit 0)

| Filter (in-process or isolated child) | Result |
| --- | --- |
| `allow_non_blocking` (3: TOML tier; disk tiers env>TOML>default + parse-fail Err in isolated child w/ temp GROW_HOME) | ok |
| `ask_policy_resolver` (Factory: disk default true over base flag; settings write → next call false w/o rebuild; disk timeout_secs change NOT live; resolver timeout == finalized resource incl. legacy-shaped map; child process) | ok |
| `ask_allow_non_blocking_write` (raw write preserves sibling `timeout_secs`, clearing removes only the key) | ok |
| `advisory_question` (3: Presented roundtrip, wire `blocking=false`, no durable events, next request served; wrong ACK → MalformedResponse, transport drop → TransportError, no durable events; expired-in-queue → no ext card + TransportError) | ok |
| `question_worker_resolves_user_outcomes` (existing blocking loop + new `presented-to-blocking-question` → `Failed` durable + `Err(MalformedResponse)`) | ok |
| `question_deadline_requires_closed_receiver` (existing blocking regression) | ok |
| `ask_user_question_merge_writes` (existing merge guard regression, updated guard) | ok |

## Parent verification on final Shell source

父核实际分支和Factory资源读取后，重新运行相关模块（Rust1.93.1、locked/offline、CARGO_BUILD_JOBS=2）：`reverse_request_session_id_tests` **17 passed / 0 failed**，包含原14项和新增3项；`allow_non_blocking` **3 passed / 0 failed**（其中包含存储测试），两个命令及独立存储filter均exit0。日志分别 `/tmp/grow-ask-shell-parent-worker.log`、`/tmp/grow-ask-shell-parent-policy.log`、`/tmp/grow-ask-shell-parent-write.log`。计数存在filter重叠，不相加。最后修正Factory后留下的 `/tmp/grow-ask-shell-regress.log` 实际仅保留Factory一项final重跑，不能将它称为全部filter的完整日志。

父确认partial rawmap不能finalize的反证；生产实现仍从finalized Params读取快照，避免与后续preset合并路径各自猜测。模块已接受，Pager/Settings及真实mockprovider里程碑尚未据此通过。

## Notes / limits

- A truly partial params map (e.g. missing `timeout_enabled`) cannot finalize at build (registry `params_missing_field`, observed), so the reviewed partial-map-reset-timeout scenario is structurally unreachable; pinned by test comment.
- Not covered here by design: Pager ext handling/cards, Settings UI registry (next tasks), integration milestone (mock-provider advisory tool call through a real session), full-workspace build (Pager compiles independently per handoff).
