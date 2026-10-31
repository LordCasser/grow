## ADDED Requirements

### Requirement: Provider admission preserves storage failure after compaction
在正常前台仍拥有后继采样权时，provider 准入 SHALL 保留现有 storage gate 的失败原因并按错误结束该回合，不把存储失败折叠为 `Cancelled`，也不得继续派发 provider 请求。已经确认的压缩 replacement 与 Completed SHALL 保留。手动压缩交接和实际前台控制停止仍按原规则结算。

#### Scenario: Shared storage closes during a summary request
- **WHEN** 摘要已发出后另一 owner 的共享存储故障关闭准入，当前摘要 replacement 和 Completed 已确认
- **THEN** 下一普通 provider 请求不发出，当前回合携带存储失败原因结束，压缩保持 Completed，不能记为取消。

#### Scenario: Storage failure belongs to another session
- **WHEN** 非共享存储故障只属于另一 session，当前前台和存储 gate 仍允许后继采样
- **THEN** 压缩后在同一 Turn 继续普通 provider 请求，不因其他 session 的单独故障停止。
