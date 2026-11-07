# Verification (final, runtime-confirmed)

集成文件：`crates/codegen/mcp/src/servers.rs`（与 `/tmp/grow-mcp-cancel-candidate/servers.rs` 一致，SHA-256 `abe45045…4d341`；原基线 `dbb851cd…7b1a4`）
Diff：`/tmp/grow-mcp-cancel-candidate.patch`（等同 `git diff crates/codegen/mcp/src/servers.rs`，+186/−8）
命令：`RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 PATH=$HOME/.cargo/bin:$PATH cargo +1.93.1 --locked --offline test -p mcp --lib -- cancel`

## 生产改动（one file）

`CancelMcpCallOnDrop::drop` 改为 `runtime.spawn(send_cancel_notification(service, request_id, MCP_CANCEL_NOTIFY_BUDGET))`；私有常量 `MCP_CANCEL_NOTIFY_BUDGET = 3s`（用户 bot 建议、父确认）；私有 helper `send_cancel_notification` 用单一 `tokio::time::timeout` 包住 `notify_cancelled`，结果三类：成功 / debug `not delivered` / debug `unconfirmed within budget; SDK send may still be pending`。**不宣称 SDK wire 已撤回或 SDK 所有 transport 资源已释放**——rmcp 3.5.1 `service.rs:1537` 会把 `transport.send` 放进 SDK send_task_set，外层 timeout 只结束 Grow 等待并释放 Grow 持有的 service Arc。

不变量保持：原 service + requestId、最多一次、completed 不发、原 call 绝对 deadline 不延长（budget 不入 caller deadline，spawn 不阻塞返回）。无新 config/public API/framework。

## Old-red（生产 drop 还原为旧 unbounded，仅 tests+helper 集成）

`/tmp/grow-mcp-cancel-oldred.log`：16 项中 15 过、1 失败——
`blocked_cancel_dispatch_exits_after_budget_and_releases_service` FAILED：
"cancellation dispatch outlived its budget; service Arc never released"。
即旧行为在真实受阻 transport 下派发任务无界等待、service Arc 永不释放（Weak 观测）。
helper 前提测试 `blocked_cancel_notification_is_pending_until_budget_expires` 在该阶段已绿（前提证据，非 root red）。

## Green（bounded 生产 drop）

`/tmp/grow-mcp-cancel-green.log`：**16 passed / 0 failed**，含：

- 新机制测试（2 项，之前草稿误记 3）：
  - `blocked_cancel_notification_is_pending_until_budget_expires`：真实 rmcp service + 32B duplex（取消 JSON ~110B 必然放不进），`tools/call` 到 wire 后 server `std::future::pending()` 停读不关闭；helper 级证明 `notify_cancelled` 在真实 backpressure 下确实 pending（100ms 内不完成）。
  - `blocked_cancel_dispatch_exits_after_budget_and_releases_service`：因果 barrier 后观测；原 call 100ms deadline 及时 `Timeout`（<1s 返回，不被 3s budget 延长）；Weak 轮询证明派发 task 在 budget 后退出并释放 service Arc（elapsed ≥ budget 断言确实 blocked，上限 budget+2s）。
- 既有控制全绿：`completed_mcp_tool_call_sends_no_cancel`、`timed_out_..._exact_request_once`、`dropped_..._exact_request`、`abandoned_..._never_cancels_replacement_service`、`timeout_and_future_drop_race_sends_only_one_cancellation`，及 init/cancellation 相关 8 项。

## 候选阶段父指出问题的修复

1. spawn move 后 helper 复用 service → spawn 持 clone，test 保留 owner。
2. stuck helper `method` 借用与 `wire_tx.send(message)` move 冲突 → 先 `let is_tools_call = method == "tools/call";`。
3. 128B duplex 可容整个取消 JSON → 降至 32B，取消帧（~110B）必然溢出，backpressure 不再是假设。
4. 测试计数更正为 2。

## 边界与剩余

- SDK send_task 可能仍 pending：记录于 debug 与本文件，不作为失败判据，不关共享 service。
- 虚拟时钟未用：实时等待（机制测试约 3s）；因果 barrier 在前、elapsed 断言在下，已防 sleep 猜测。
- 未跑全量套件 / fmt / commit（按窗口约定）；tasks.md 项 2/3 已勾，项 4（父 review、strict validate、archive、开发说明）归父。
