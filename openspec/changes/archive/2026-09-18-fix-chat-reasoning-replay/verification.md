# Verification

## 实际故障证据

只读核对会话 `01a0af36-023e-73b0-839e-8a750b3d3258`，目录为 `~/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fjarde/01a0af36-023e-73b0-839e-8a750b3d3258/`。

- `timeline.jsonl:18665`（seq 18664）：从 `deepseek-v4-flash` / Responses 切到 `opencode/deepseek-flash` / Chat Completions，effort 为 max。
- `timeline.jsonl:18668`（seq 18667）：实际请求共有 723 条 messages、345 条 assistant；其中 342 条包含非空 tool_calls，共 345 个工具调用。全部 345 条 assistant 都缺少 `reasoning_content`。投影证据为 `replay_portable_responses_reasoning:false`、`native_spans:[]`。
- `timeline.jsonl:18669` / `:18671`：明确的 reasoning_content 回传 400 被判定 Fatal，未进入 reasoning 修复。
- 证据由会话内 22 个 sampling request artifact 分块重组核对；未复制用户原文、凭据或完整请求进入仓库，也未修改用户数据。

这与 [DeepSeek thinking mode 文档](https://api-docs.deepseek.com/guides/thinking_mode/) 的要求一致：带 tools 的请求应回传历史 reasoning_content，包括没有工具调用的 assistant 轮次。空字符串是本地对“历史没有可见文本”的编码，不声称恢复了原模型完整思考；实际代理接受性未作线上调用验证。

## 先失败再修复

`learned_chat_route_replays_assistant_reasoning` 在旧实现上失败：期望 `first thought\nsecond thought`，实际 wire 字段为 Null。修复后通过。第一次测试命令误加不匹配的 exact filter，仅运行 0 项；随后使用名称筛选确认 1 项实际失败，未把空跑作为验证。

开发中一次核心构建因新测试使用错误的 estimator 名称失败，修正为现有 `estimate_request_input_tokens` 后全量重跑通过。

## 场景与验证

| 场景 | 证据 |
| --- | --- |
| Chat 历史多段 reasoning、普通正文、无 reasoning、工具配对 | sampling-types `learned_chat_route_replays_assistant_reasoning` |
| User/System/孤立结果边界、native 内容保留与缺字段补空、Timeline 不变 | `learned_chat_reasoning_does_not_cross_boundaries_or_replace_native_content` |
| Responses 原有窄化规则与三协议隔离 | `learned_responses_route_replays_only_visible_portable_reasoning` 和完整 sampling-types 回归 |
| 明确 400、代理嵌套文本、近似错误与歧义字段拒绝 | `recognizes_only_explicit_backend_reasoning_replay_rejections` |
| backend 事实经 DTO 往返 | sampler `explicit_reasoning_replay_requirement_survives_dto_round_trip` |
| ACK 幂等、错误 backend、同路由参数更新/reset、真实路由替换、token 估算 | ChatState `reasoning_replay_is_route_local_and_acknowledged` |
| 空历史拒绝启用、Chat 无 reasoning 可启用、Messages 不启用 | `chat_reasoning_replay_requires_an_assistant_but_not_invented_reasoning` |
| Responses/Chat 首次修复、Chat 空 reasoning、同错不循环 | Shell `explicit_reasoning_replay_rejection_updates_once_then_stops` |
| 切换队列与 Step 边界 | Shell `session::actor::model_switch::tests`，包括 `busy_model_and_effort_switch_applies_after_the_active_step` |
| attempt/deadline 不重置 | sampler `repairs_share_count_and_cannot_extend_deadline`、`shared_recovery_budget_blocks_second_real_http_attempt`；Shell turn 的恢复分支仍复用传给 `run_turn_via_sampler` 的同一 RecoveryBudget |
| recap caller 保持原有 Responses 投影 | Shell `session::helpers::session_recap::tests` |

## 命令结果

- `RUST_MIN_STACK=16777216 cargo test --locked --lib -p sampling-types -p chat-state -p sampler -j 2 -- --test-threads=4`：sampling-types 280 passed；chat-state 499 passed / 1 ignored；sampler 241 passed；0 failed。
- `RUST_MIN_STACK=16777216 cargo test --locked --lib -p shell session::actor::turn::sampling::image_input_rejection_tests -j 2 -- --test-threads=4`：7 passed。
- `RUST_MIN_STACK=16777216 cargo test --locked --lib -p shell session::actor::model_switch::tests -j 2 -- --test-threads=4`：25 passed。
- `RUST_MIN_STACK=16777216 cargo test --locked --lib -p shell session::helpers::session_recap::tests -j 2 -- --test-threads=4`：34 passed。
- 合计 1,086 passed / 1 ignored / 0 failed。Shell 链接保留已有大型 debug 二进制 `__eh_frame` 警告，不影响测试结果。
- `openspec validate --all --strict --no-interactive`：归档前 20 passed / 0 failed。
- `git diff --check`：通过。
- `CARGO_BUILD_JOBS=2 cargo build --locked -p cli --bin grow`：通过，产物为 `target/debug/grow`；仅既有 `__eh_frame` 链接警告。
- 已归档为 `2026-09-18-fix-chat-reasoning-replay`；归档后的全量 strict 校验 19 passed / 0 failed，archive strict 校验 346 passed / 0 failed。首次 archive 校验仅因最后的归档任务尚未勾选失败（345 passed / 1 incomplete）；完成该任务后重新校验通过。

未进行真实 provider 重发、用户会话重启或进程替换；未提交 Git。工作树原有 recovery-stop evidence 修复保持。
