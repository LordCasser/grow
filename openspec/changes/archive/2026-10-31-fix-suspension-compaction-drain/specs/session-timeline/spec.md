## ADDED Requirements

### Requirement: Suspension drains compaction before retiring the foreground
挂起 SHALL 在活动压缩的持久化与结算 owner 退出后收回并 join foreground，不等待已经为 runtime handoff 停驻的 foreground 自行退出。挂起 SHALL 沿用同一 deadline 和 operation 身份；压缩尚未排空时不得提前 abort 或确认 Suspended。

#### Scenario: Active compaction settles into a parked foreground
- **WHEN** 挂起关闭准入并取消活动压缩，压缩完成结算并释放 lease，foreground 随后停驻等待 handoff
- **THEN** 挂起 owner 收回并 join foreground，在原 deadline 内完成 checkpoint，不生成 Turn cancelled/error/completed。

#### Scenario: Compaction acknowledgement exceeds the suspension deadline
- **WHEN** 压缩持久确认仍在途且共享 deadline 已到
- **THEN** 返回未确认并保留压缩任务，不提交 Suspended；ACK 结算后同 operation 重试可以收回 foreground 并完成挂起。

#### Scenario: An admitted manual worker reaches lease acquisition after suspension
- **WHEN** 手动压缩已获得会话活动许可但尚未取得 publication lease，挂起先关闭执行准入
- **THEN** 该 worker 不能取得新压缩 lease；原活动 lease 释放后仍保持关闭，只有恢复执行才重新开放。
