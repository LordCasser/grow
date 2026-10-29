## ADDED Requirements

### Requirement: Queued manual compaction yields at a closed execution boundary
普通 turn SHALL 在已排队手动压缩需要取得前台 owner 时，于闭合 Step 边界结束当前执行并交给既有 idle arbitration；被跳过的自动压缩 SHALL NOT 被当作恢复进展而无限重建请求。

#### Scenario: Manual request competes with automatic pressure recovery
- **WHEN** 手动压缩已排队且普通请求投影超过自动压缩阈值
- **THEN** 普通 turn 有界让出 owner，手动请求仍保留并可运行，不反复生成空 Step 或向超窗 provider 发请求。

### Requirement: Summary source fitting uses the transmitted history projection
摘要来源预算 SHALL 使用将实际发送的历史投影，同时保持原始 Surface 的完整选区边界及 target identity。不会发送的 reasoning 和回忆派生内容 SHALL NOT 挤占来源预算；重试缩小选区也使用同一投影。

#### Scenario: Omitted reasoning dominates raw history
- **WHEN** 原历史包含大量在当前摘要协议中不发送的 reasoning，而完整来源投影可装入预算
- **THEN** 接纳该合法来源，不因原始 reasoning 体积拒绝压缩；replacement 仍只覆盖选定完整范围。

### Requirement: Only completed summary responses may replace history
压缩 SHALL 仅接纳成功结束的摘要响应作为历史 replacement。非成功终态即使有足量正文，也 SHALL 按现有失败/有界恢复路径处理，不提交部分摘要。

#### Scenario: Provider returns partial or refused summary text
- **WHEN** 摘要流以输出截断、上下文超窗、拒绝或暂停终止，并附带非空正文
- **THEN** 不以该正文替换历史；Sideband 和压缩事务按实际结果结算。

#### Scenario: Provider stream ends without a completion terminal
- **WHEN** provider stream emits summary text but reaches EOF without a typed successful completion terminal
- **THEN** the response is rejected through the bounded failure path and its text cannot replace history.

### Requirement: Summary cleanup preserves quoted control tokens as evidence
摘要清理 SHALL 只剥离明确的前导 scratchpad，保留正文引用控制标签之前和之后的事实，不以是否数字开头判断正文可删除。

#### Scenario: Prose summary quotes an analysis closing tag
- **WHEN** 摘要正文以自然语言开始并在后文引用 analysis 结束标签
- **THEN** 标签可被中和，但正文前面的证据保持完整。

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
