# Task 1 验证记录 — Tools advisory 契约（2026-11-06）

环境：Rust 1.93.1（locked, offline），`PATH=$HOME/.cargo/bin` 优先，`RUST_MIN_STACK=16777216`，`CARGO_BUILD_JOBS=2`。

## Tier 2：真实 `Tool::run` + mpsc channel（tools crate lib tests）

命令：`cargo test --lib ask_user_question` → **72 passed / 0 failed**（日志 `/tmp/grow-ask-contract-target.log`）。

新增 advisory 场景（`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` tests）：

- `advisory_presented_round_trip`：`blocking=false` + 合法推荐 → 请求 `blocking=false`（effective）；仅收到 `Presented` 后 `advisory=Some{tool_call_id, questions}`；message 含题目、推荐 label、"Agent recommendation"、"NOT answered"，不含 "User has answered"。
- `advisory_missing_recommendation_rejected_before_send`：缺推荐 → `invalid_arguments`，`rx.try_recv()` 为空（enqueue 前拒绝）。
- `invalid_recommended_label_rejected_in_both_modes`：显式无效推荐（含 "(Recommended)" 后缀）在 blocking/advisory 两种模式都 `invalid_arguments`，不解析后缀。
- `advisory_timeout_is_failure_not_cancel`（start_paused）：5 秒共享预算过期 → `execution` error，message 不含 `CANCEL_TEXT`。
- `advisory_receiving_human_answer_is_error`：advisory 收 `Accepted` → error（"expected presentation acknowledgement"），人答不变成 advisory 结果。
- `blocking_receiving_presented_is_error`：blocking 收 `Presented` → error。
- `preference_disabled_forces_blocking`：`allow_non_blocking=false` 时 `blocking=false` 请求被强制 blocking（request.blocking=true）；无人答走旧 CANCEL_TEXT 政策、`advisory=None`。
- `resolver_failure_is_admission_failure`：`UserQuestionPolicyResolver` 返回 Err → 准入失败（custom error "failed to resolve"），不 enqueue。
- `resolver_overrides_injected_params`：resolver 快照优先于注入 `Params`。
- `input_blocking_defaults_true` / `output_advisory_omitted_when_none`：serde 缺省与 wire 省略。

旧 blocking/Plan/Other/timeout/cancel 回归全部保留并通过（含 `blocking_round_trip_accepted`、`short_params_timeout_fires_with_cancel_text`、`timeout_disabled_waits_beyond_default_budget`、`transport_error_not_cancel` 等）。

## 输出类型回归

命令：`cargo test --lib "types::output"` → **56 passed / 0 failed**（日志 `/tmp/grow-ask-contract-output.log`）。`AskUserQuestionOutput` 新增 `advisory` 字段后 `to_prompt_format` 仍只返回 `message`。

## 跨 crate 机械适配编译

命令：`cargo check --offline -p shell -p pager -p pager-minimal -p agent --tests` → **exit=0**（日志 `/tmp/grow-ask-contract-regress3.log`）。

- shell：`session/actor/spawn.rs`（worker ext_req 透传 `request.blocking`，advisory 分支未实现，属 Shell Task2）、`util/config/resolve/toolset.rs`（resolver 字面量补 `allow_non_blocking: DEFAULT_ASK_USER_QUESTION_ALLOW_NON_BLOCKING`，TOML tier 解析留 Shell Task2）、`session/actor/tests/reverse_request_session_id_tests.rs`（ExtRequest/UserQuestionRequest 字面量补 `blocking: true`）。
- pager / pager-minimal / agent：仅 `Question`/`QuestionOption`/`AskUserQuestionExtRequest` 字面量补默认字段（`recommended_option: None` / `blocking: true`）：`peek.rs`、`overlay.rs`、`question_view_playground.rs`、`agent_view/interactions.rs`、`agent_view/paste.rs`、`app/root/mod.rs`、`dispatch/prompt.rs`、`dispatch/session/{fork,lifecycle}.rs`、`dispatch/tests/{mod,router}.rs`、`dispatch/tests/session/{fork,lifecycle,load}.rs`、`project_picker/mod.rs`、`views/question_view.rs`；`agent/src/builder.rs` 经 serde map 构造，无需改动。

## 未验证 / 边界

- 未实现 Shell worker advisory 分支、配置 writer/resolver tier、Pager 卡片/Settings（按分工留 Task2/3/4）。
- 真实 mock-provider 里程碑集成、cold restart 持久化验证未做（属父里程碑）。
- 未跑全 workspace 检查；未全局 fmt；未 commit。
