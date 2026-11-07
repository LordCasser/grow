# Design

## Facts

- `drain_subagent_usage_for_prompt_bounded`（`turn/mod.rs`）只计算一次 `deadline = now + max_wait`，随后循环：await `outstanding_reply_for_prompt` → 匹配 reply → 只有前台 live 分支检查 deadline。await 本身无界（`settlement.rs` 的 `rx.await`），排队或永不回复的请求使预算失效。
- 调用方：`admission.rs:1580/1651` 走 120 秒 freeze；`admission.rs:1279` 在 fatal terminal 路径用 `Duration::ZERO`；取消路径（`tasks_cancel.rs`）已用 250ms `tokio::time::timeout` 包同一查询——即既有 bounded observation 约定。
- `error_path_usage_fallback`（`turn/mod.rs`）从 actor loop 上的 completion 处理发起同一查询，无界等待。
- `UsageDrainOutcome` 既有策略：`fail_closed` 才通过 `finalize_usage_from_outcome` 标记双账本；sticky/background 仅 report 级。查询失败（`None`）即 fail-closed。

## Decision

- 在 `settlement.rs` 增加 `outstanding_reply_for_prompt_bounded(prompt_id, deadline)`：`tokio::time::timeout_at(deadline.into(), outstanding_reply_for_prompt)` 直接表达原绝对期限，不做每轮窗口刷新；到期映射为 `None`（与 send 失败同形）。
- drain 循环每轮改用 bounded 变体；`None` 分支保持 fail-closed（report + 双账本），日志同时说明查询失败与错过 freeze deadline 两种形态。既有 clean/background/sticky/live-at-deadline 分支不变。
- 每轮 poll 之间的 sleep clamp 到原 deadline：`sleep_until(min(now+POLL, deadline))`，live 回复后的固定 50ms sleep 不再可能把下一轮 poll 推过原期限。
- `error_path_usage_fallback` 用同一 bounded 帮助函数加一个与取消路径一致的短固定观察窗（250ms），到期即 `None` → `may_undercount`。
- 零剩余预算：即时 ready 的观察可能被消费（如无 subagent channel 时内部查询立即返回 `Some(Default::default())`），也可能按现语义 fail-closed；不把其中单一行为写成必然。两态都落在 delta 允许的"即时 ready 或 fail-closed"语义内。
- 取消路径不动（已有 250ms 界），Coordinator、child 生命周期不动。

## Alternatives rejected

- 每轮给新窗口（per-round refresh）：违反绝对 deadline 要求。
- 到期杀 child 或记零用量：违反用量结算所有权；迟到 fold 必须仍能落账。
- 通用 RPC timeout 框架：超出本项；仅 freeze 与 error-path 查询沿既有约定收敛。

## Tests

新增回归：responder 收到 Outstanding 后持有 `respond_to` 不回复；小预算 freeze 必须结束且 report 与双账本 incomplete（测试内加外层守卫超时，使旧行为表现为失败而非挂死）。既有矩阵（空集合、前台 live 超时、background-only、sticky-only、mid-drain fold、取消策略、归属/迟到落账）保持原断言不动并通过。
