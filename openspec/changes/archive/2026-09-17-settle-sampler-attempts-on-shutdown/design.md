## Context

`SamplerActor` owns the `JoinSet<RequestId>` for all live request tasks. Each request task owns provider polling, attempt evidence completion and `AttemptUsageSink` settlement; those final two stages contain independent `.await` and deliberately outlive provider cancellation. Session teardown already closes sampler admission through `SamplerOwner::shutdown_bounded(10s)`，then waits for the sampler event drainer before crossing later persistence barriers.

当前 actor 收到 `Shutdown` 后取消 active token，却立即调用 `JoinSet::shutdown()`。该 Tokio API abort 剩余 task，因此会切断正在等待 evidence/usage ACK 的 request task。外层十秒 deadline 没有机会区分正常 drain 与 actor 内部的立即 abort。

## Goals / Non-Goals

**Goals:**

- 正常 actor shutdown 保持“停止准入 → 取消 provider → 完成 attempt settlement → actor/event sender 结束”的所有权顺序。
- 继续由现有 `SamplerOwner::shutdown_bounded` 提供唯一强制 abort 边界，并保留 shell 对失败的传播。
- 用可控 ACK 屏障证明 request task 在正常关闭时没有被 abort。

**Non-Goals:**

- 不改变十秒/五秒 teardown deadline、普通请求取消、重试或响应接纳逻辑。
- 不为 shutdown 新增 actor、队列、持久化格式、公共错误类型或后台恢复器。
- 不在本 change 处理 duplicate live `RequestId`、response admission 确认不明或 `updates.jsonl` 对账。

## Decisions

### 1. Actor 在取消后协作 join，而不是调用 aborting shutdown

收到关闭命令后，actor 不再处理新的 mailbox 命令。它遍历当前 active request，触发各自 `CancellationToken`，随后持续 `join_next()`，直到 actor-owned `JoinSet` 为空。request task 因而可以从 provider poll/backoff 收敛到既有 evidence finish、usage sink 和 terminal event 路径。

不把 task 移交给 detached worker，也不并行建立第二个 drain owner；`SamplerActor` 继续是唯一 join owner。task panic/JoinError 沿用现有诊断策略，本 change 只移除正常关闭主动 abort 的行为。

**Alternative:** 保留 `JoinSet::shutdown()`，只在调用前 sleep/yield。该方案仍存在任意 ACK 窗口，不能形成可验证边界，因此拒绝。

### 2. 强制 abort 只由现有 owner deadline 触发

`SamplerOwner::shutdown_bounded` 先关闭 admission，再等待 actor。若十秒内 actor 仍未完成，它 abort actor 并返回明确错误；drop actor-owned `JoinSet` 此时才终止剩余 task。`shell::shutdown_sampler` 已收集该错误，正常 teardown 会在进入后续成功 frontier 前返回失败，因此无需增加另一套超时或状态。

**Alternative:** 在 actor 内再加 settlement timeout。双重 deadline 会分散关闭策略，并可能让 actor 在外层仍认为正常时主动丢结算，故拒绝。

### 3. 回归在 sampler owner 边界固定 ACK 竞争

测试使用真实 `SamplerActor::spawn_owned` 和 accounted submission，使 request 到达 evidence 或 usage sink 后阻塞 ACK。测试随后发起 owner shutdown，先断言 shutdown 未完成，再释放 ACK 并断言 shutdown 完成且 sink 恰好调用一次。现有 bounded-owner timeout 测试继续证明 forced path 返回失败并观察 actor terminal。

Shell 现有 `shutdown_sampler_joins_drainer_and_breaks_session_cycle` 保留跨模块顺序证据：sampler actor 完成并关闭 event channel 后，event drainer 才能退出。

## Risks / Trade-offs

- **Risk：持久化 owner 永久不返回会让 graceful join 挂起。** → 继续由现有十秒 `shutdown_bounded` 强制终止并返回失败；不把失败伪装为正常关闭。
- **Risk：取消后 request task 仍会发送 terminal event。** → event sender 由 actor/task 持有，shell 已在 owner 结束后再 join drainer，保持 FIFO drain。
- **Risk：协作 join 延长正常 session 关闭时间。** → 这是等待已发生 provider attempt 的必要结算；上限仍由既有十秒 deadline 控制，不新增无限用户等待。

## Migration Plan

无需数据迁移。实现可直接替换 actor 的正常关闭分支；回滚会恢复原有 abort 行为，但会重新引入结算丢失窗口。
