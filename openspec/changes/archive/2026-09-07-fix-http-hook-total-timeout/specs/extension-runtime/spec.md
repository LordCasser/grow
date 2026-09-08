## ADDED Requirements

### Requirement: HTTP hook phases share one timeout budget
HTTP Hook 的 URL 校验、请求和正文异步等待 SHALL 共享一次 timeout_ms 预算，阶段切换不得重置总预算。超时 SHALL 返回 TimedOut 并保留已经取得的 URL/状态码信息。同步代码不提供抢占式超时。

#### Scenario: 校验与正文分别耗时
- **WHEN** URL 校验与正文读取各自短于预算但合计超过预算
- **THEN** 总预算耗尽时返回 TimedOut；若已收到响应头则保留其状态码。
