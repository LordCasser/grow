# Known Limitations

本文档记录已确认但**不计划在当前任务内修复**的已知限制，以及修复所需的决策点。
每个条目包含：机制、理由（为什么不能随手修）、影响面与后续任务建议。

## 1. prompter 无超时：leader 丢包时 shell 侧永久挂起

**状态**：已确认的已知限制，需协议级设计决策后另立任务修复。本次不做行为改动。

### 机制

子代理（或顶层会话）请求 bash 等工具权限时，shell 侧的 `AcpPrompter::request`
（`workspace/src/permission/prompter.rs`）构造 `request_permission` 后直接
`self.gateway.request_permission(req).await`，**没有任何超时**。`acp` 的
`GatewaySender::request_permission` 也只是 `forward(args).await`（`gateway.rs:391-396`），
同样无超时。

正常情况下消息链为：

```text
shell AcpPrompter ──request_permission──▶ gateway ──▶ leader 转发 ──▶ pager 弹窗 ──▶ 用户选择
shell AcpPrompter ◀──Selected/Rejected──── gateway ◀─── leader ◀─── pager 应答
```

当 **leader 丢包**（leader 进程重启、gateway 连接中断、消息在转发层丢失、pager 崩溃等）
时，应答永远不会回来：`request_permission` future 永远不 resolve，`execute_tool_calls`
永久阻塞在该工具调用上，整个 session 的后续轮次全部挂起。shell 侧没有其他机制能感知
"这个请求已经不可能被应答"（pager 对未知会话会回 `Cancelled`，但丢包场景下 pager 根本
收不到请求，或应答在回程上丢失）。

### 为什么不能随手修（需要协议级设计决策）

1. **超时后怎么办没有现成答案**：直接返回 `PromptOutcome::Error` 会把权限请求变成
   工具失败，用户可能在 pager 里已经点了 Allow——此时工具没执行，用户看到的却是
   "权限错误"；而如果用户没有点，超时后静默拒绝又可能打断合法流程。需要一个明确的
   语义：`Cancelled`（shell 自取消）vs `Error`（告知用户重试）vs 自动降级策略。
2. **超时时长与用户体验耦合**：交互式权限弹窗的合理等待时间（秒级）与无头/CI 场景
   不同；配置化超时又会引入新的配置面（键名、默认值、与 `[ui]` 其他选项的关系）。
3. **影响面跨三端**：shell（prompter 超时 + 取消语义）、leader（丢包检测/重试/转发
   确认）、pager（取消通知回传）。任何一端单独改都会破坏现有契约（例如 pager 侧的
   `unregistered_child_permission_is_cancelled` 契约就依赖"未知会话必须被应答"）。
4. **与现有兜底的关系**：pager 已经对**未知会话**回 `Cancelled`（防止注册竞态造成
   永久挂起）；本限制针对的是**请求本身在传输中丢失**，与注册竞态是两条独立路径，
   不能靠 pager 兜底解决。

### 影响面

- 触发条件：leader 进程重启 / 网络中断 / 消息丢失，且恰逢权限弹窗未应答。
- 后果：该会话的所有后续工具调用永久挂起，只能手动 Ctrl+C / 重启 session。
- 频率：低（依赖 leader 故障），但后果严重（整会话不可用）。

### 后续任务建议

另立任务，包含：

- 在 `AcpPrompter::request` 增加 `tokio::time::timeout`（时长可配置，默认值建议
  交互 60s / 非交互 10s，需产品确认）。
- 定义超时结果语义：倾向 `PromptOutcome::Cancelled`（与 pager 对未知会话的
  `Cancelled` 语义一致，上层已能处理 `PermissionReject`），并在事件日志中标记
  `timed_out`。
- 评估 leader 侧转发层是否应增加请求-应答确认/重试，避免把丢包责任全部压给 shell。

## 2. 其他已知限制占位

暂无其他已确认的限制。
