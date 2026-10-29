## MODIFIED Requirements

### Requirement: Cancellation preserves and drains the compaction generation owner
When ordinary authority invalidation cancels a background compaction generation, the Session SHALL signal cancellation without joining an unfinished worker from inside the root command handler, and SHALL retain the worker in its existing owner slot. An explicit suspension or teardown barrier SHALL keep the root accounting inbox able to process the worker's Goal settlement receipt while it joins the worker; it SHALL publish the cancellation terminal only after that drain. A runtime freeze SHALL use the existing shared deadline for this drain and SHALL fail without claiming a settled checkpoint when the worker has not joined by that deadline. Cancelled or late generation output SHALL NOT be committed to the visible Surface.

#### Scenario: Goal usage invalidation and Sideband settlement share the root inbox
- **WHEN** a promoted summary is still running and `RecordGoalUsageIncomplete` is processed by the root accounting inbox
- **THEN** invalidation signals cancellation and returns from the current inbox command, allowing the same inbox to process the worker's `SettleGoalUsageAttempt` before a later explicit drain records cancellation.

#### Scenario: Promoted generation is cancelled after incomplete Goal usage
- **WHEN** a promoted summary is still running as the Goal usage window becomes incomplete
- **THEN** the same worker is cancelled and joined, its Sideband attempt reaches a terminal settlement, the Goal settlement is not detached, and the generated summary is not committed.

#### Scenario: Suspension deadline expires during generation drain
- **WHEN** runtime suspension cancels a background summary but its owner has not joined by the shared deadline
- **THEN** suspension returns an unconfirmed result and retains the worker owner for retry; it does not publish a completed frozen checkpoint.

#### Scenario: Teardown drains with the accounting inbox active
- **WHEN** graceful or fatal teardown has an unfinished cancelled compaction worker
- **THEN** its existing drain loop continues dispatching Goal usage and settlement commands until the compaction worker and non-Workflow children finish, or teardown fails without publishing a final persistence frontier.

#### Scenario: Manual compaction waits for the existing generation owner
- **WHEN** a manual compaction starts while an asynchronous summary still owns the compaction lease
- **THEN** manual compaction signals cancellation, leaves the root Goal inbox available while it joins that worker, and acquires the lease only after the original worker has settled.

#### Scenario: Ordinary stop precedes a late background summary
- **WHEN** 未提升的后台摘要仍在等待 provider，普通取消已返回，然后 provider 返回有效摘要
- **THEN** 后续发布边界丢弃迟到结果，不提交 replacement；排空仍由现有显式 drain 边界负责。

#### Scenario: Promotion waiter is dropped and the same generation is awaited again
- **WHEN** 后台 generation 已经 durable 提升，但等待方 future 被丢弃，随后前台再次等待同一 worker
- **THEN** 复用已有 promotion 事实和 provider attempt，保留原截止时间，并且最多提交一次 replacement；generation token 真正取消时不得发布迟到结果。

#### Scenario: Rejoined promotion still waits for the original durable acknowledgement
- **WHEN** 原 Promoted 写入尚未确认，等待方退出后另一等待方提交同一 promotion
- **THEN** 后者等待原 durable 边界；原 ACK 确认后复用同一事件，原写入失败或超时则拒绝后续写入，不能因事件已保留而提前报告成功。

### Requirement: Async compaction completion reports a materialized request projection

异步压缩的用户可见完成通知 SHALL 等下一次普通模型请求完成 request projection 后再发布，并以该投影作为 `tokens_after`。Surface replacement 后、请求重新物化前的中间估算 SHALL NOT 作为异步压缩最终值展示。每次成功提交最多发布一个完成通知；同步提升、同步自动压缩和手动压缩保留各自的即时完成语义。

#### Scenario: Between-step request materializes after async commit

- **WHEN** 异步压缩在闭合 Step 边界成功提交，随后普通后继请求完成组装
- **THEN** 提交与请求组装之间没有异步完成通知；组装后发布的 `tokens_after` 等于该请求已应用的 ChatState projection。

#### Scenario: Completed turn has no next request yet

- **WHEN** 异步压缩在已结束 turn 的边界提交，尚无后继普通请求
- **THEN** 不展示 Surface-only 中间值，完成通知保持待发布，且其间不启动另一轮后台压缩。

#### Scenario: A later turn materializes the next request

- **WHEN** 待发布通知存在，后续 turn 的首个普通请求完成 projection
- **THEN** 使用该 projection 发布恰好一次异步完成通知；后续请求构建或边界检查不重复发布。

#### Scenario: Request construction fails before projection

- **WHEN** 下一普通请求未能完成构建和 request projection
- **THEN** 不发布带猜测 `tokens_after` 的完成通知，也不消费待发布记录。

#### Scenario: A later replacement supersedes the pending display

- **WHEN** 待发布期间同步压缩、手动压缩或 rewind 在下一普通请求前替换上下文
- **THEN** 丢弃旧异步压缩的待发布通知，不把新 replacement 的 token 数归因给旧异步事务。

#### Scenario: Route changes while an async completion awaits projection
- **WHEN** 异步 replacement 已提交且完成通知待发布，其后仅切换模型路由并成功物化下一普通请求
- **THEN** 不提前发布；通知采用该新请求投影并且只发布一次；若切换触发新的 replacement，旧通知按既有 supersession 规则丢弃。

## ADDED Requirements

### Requirement: Compaction persistence uncertainty preserves committed boundaries
压缩在 replacement、终态或必要控制修复的 durable ACK 不明时 SHALL 返回错误并阻断依赖采样。已确认 replacement SHALL 保持其 identity；冷恢复 SHALL 依据实际持久事实闭合事务，不重复替换，也不把未确认的写入作为 live 成功依据。

#### Scenario: Replacement or terminal acknowledgement is lost
- **WHEN** replacement 或 Completed 记录的 durable ACK 丢失且既有精确重试仍无法重新确认，记录可能已经持久化
- **THEN** live 调用不能报告整体执行成功；恢复按实际记录保留原历史或确认后的 replacement，且每个事务最多有一次有效替换及一个终态。

#### Scenario: Required control repair fails after replacement
- **WHEN** replacement 已确认但其后的必要控制上下文重投影未获得持久确认
- **THEN** 保留已提交 replacement，当前执行失败并阻断下一请求；事务按已提交事实闭合，不能被改判为未提交失败。

#### Scenario: Lost acknowledgement is recovered by an exact retry
- **WHEN** 记录实际已持久化但第一次 ACK 丢失，既有写入循环对同一事件重试并获得确认
- **THEN** 复用同一 seq 和 payload，只应用一次逻辑事实；不重发模型请求、不重复替换或计费。
