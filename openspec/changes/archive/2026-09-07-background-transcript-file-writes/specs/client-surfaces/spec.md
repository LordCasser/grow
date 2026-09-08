## ADDED Requirements

### Requirement: Explicit transcript file writes run off the UI thread
交互式 /copy 和 /export 的显式文件输出 SHALL 在快照内容与目标后，通过后台任务执行文件 I/O，不在 UI dispatcher 中执行写入或同步。

#### Scenario: Slow file write
- **WHEN** 显式文件输出的底层写入尚未完成
- **THEN** dispatcher 已返回，成功反馈等待实际提交结果。

### Requirement: Transcript file jobs preserve order and origin
同一 Pager 应用的显式会话文件任务 SHALL 串行按提交顺序执行，待处理请求数量 SHALL 有限；结果反馈 SHALL 绑定原 agent/session。

#### Scenario: Repeated destination
- **WHEN** 前一文件任务未完成时再次提交同一路径
- **THEN** 后一任务等待前一任务完成，最终内容按提交顺序确定。

#### Scenario: Origin no longer matches
- **WHEN** 完成时原视图已移除或绑定其他会话
- **THEN** 不给新会话发布旧任务反馈，但仍推进待处理队列。

#### Scenario: Failure or saturated queue
- **WHEN** 写入失败、后台任务异常或待处理队列已满
- **THEN** 明确报告对应失败；任务异常不阻塞后续已接纳请求，满载时不静默丢弃旧任务。
