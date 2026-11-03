## ADDED Requirements

### Requirement: Idle memory flush waits for inactivity and checkpoints only success

自动 memory flush SHALL 在用户活动停止达到正数配置时长，且 session 前台与 session activities 均空闲后才启动。用户活动及前台忙转闲 SHALL 重新开始 idle interval。零秒 timeout SHALL 禁用自动 flush。只有 flush 成功 checkpoint（写入记忆或确认无需写入）才推进最近成功历史水位；失败和 flush lock 冲突 SHALL 保留同一历史的后续重试资格。

#### Scenario: Recent activity delays idle flush
- **WHEN** 用户提交输入、输入仍在 FIFO 等待、前台 turn 正在运行或手动压缩待执行时 idle timeout 到期
- **THEN** 不启动 memory flush；用户活动与 turn 完成分别使 idle interval 从最近活动或忙转闲重新计时

#### Scenario: Successful flush checkpoints history
- **WHEN** 正数 idle interval 到期且 foreground/session activity 空闲，并且 flush 成功写入或确认无内容
- **THEN** 发起一次 flush 并推进成功历史水位；未出现新历史时后续 interval 不重复 flush

#### Scenario: Failed flush retains retry eligibility
- **WHEN** idle flush 因 provider/snapshot 错误或 flush lock 冲突未成功
- **THEN** 不推进成功水位；相同历史在下一次完整 idle interval 再次具备 flush 资格

#### Scenario: Zero timeout disables idle flush
- **WHEN** memory flush idle timeout 配置为零秒
- **THEN** run loop 不启动周期性 idle flush timer
