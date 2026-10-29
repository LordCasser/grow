## MODIFIED Requirements

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

#### Scenario: Writer process dies after a durable replacement but before acknowledgement
- **WHEN** replacement 已经通过真实文件与目录同步，返回原 actor 的 ACK 未确认时 writer 进程被强制终止
- **THEN** 新进程取得独占 writer 后保留同一 replacement 与未选中的 Surface 身份，恢复必要 Control 上下文并且只闭合一个 Completed，不重跑摘要生成。

#### Scenario: Recovery process dies after committing its terminal
- **WHEN** 恢复进程写入 Completed 后、其 ACK 返回前再次中断
- **THEN** 下一独立恢复进程承认已有终态，保持原日志 prefix，不追加重复 replacement、terminal 或 Control 重投影。

#### Scenario: Failed summary-only recovery is loaded again
- **WHEN** 摘要事实已持久化但 replacement 未提交，恢复以 Failed 闭合该事务，随后再次启动
- **THEN** 校验接受该合法失败事务，保留原历史及摘要来源证据，不要求失败事务存在 replacement。

#### Scenario: A failed range is summarized by a later transaction
- **WHEN** 含 Summary 的失败事务未替换历史，后续新事务重新压缩相同 SurfaceRange 并提交不同摘要
- **THEN** 每个 Summary 只与自身事务中的 replacement 对账，不借用另一事务的 replacement；原失败事实仍可重复加载。
