## Why

一次成功的压缩 Sideband 从冻结 `Surface` 选中了 `Input::Consumed` 与带模型输入的 `Notification::Consumed` 坐标。两类事件本来都会生成 canonical `SurfaceId`，但 Sideband 的 parent validator 只承认 `Messages`、`ImageProjection` 和 `Control`。会话继续在 resident actor 中工作，却在后续严格实体校验时被判为损坏；peer 或 parent-child 询问因而在接收事实写入前返回不可重试的 `audit_failure`。前台是否输出、是否等待子 Agent 不是接纳条件，只是与故障暴露同时出现。

## What Changes

- 由 Timeline 提供 Sideband parent validation 使用的 canonical Surface coordinate 判定，覆盖所有实际产生 Surface 坐标的事件。
- 让已消费用户输入和已消费通知生成的坐标可被压缩及其他 Sideband ledger 严格重载，不放宽对非 Surface 事件、越界 item 或伪造坐标的拒绝。
- 增加 chat-state、存储严格重载及 busy/idle coordination 回归，证明后台子 Agent 和前台状态不影响合法询问交付。
- 既有合法 ledger 通过修正后的校验直接恢复，无迁移、重写或跳过审计。

## Capabilities

### Modified Capabilities

- `session-timeline`: Sideband parent validation 与 Timeline 实际 Surface 生产规则使用同一 canonical coordinate 语义。
- `local-coordination`: 已完成压缩且后台仍有子 Agent 的会话继续接收 peer/parent-child inquiry，不因合法 Surface 坐标被误判而 audit-fail。

## Impact

主要修改 `crates/codegen/chat-state/src/timeline.rs`、`sideband.rs` 及相关测试；Shell 只增加跨层回归，不新增协议字段、队列、状态机、持久化实体或兼容分支。不改变 Sideband source/input range、compaction 选区、foreground 调度或 subagent 生命周期。
