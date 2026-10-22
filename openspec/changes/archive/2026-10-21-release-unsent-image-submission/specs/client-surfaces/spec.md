## ADDED Requirements

### Requirement: Failed local input checkpoints release their submitting placeholder

Pager SHALL 在发送前本地 checkpoint 失败时释放该请求仍占有的 submitting 占位与状态查询计时，保留原请求身份、可用图片及草稿，等待显式重试。SHALL NOT 自动派发或生成服务端完成事实，也不得关闭其他请求或已确认运行的前台。

#### Scenario: Image checkpoint fails before dispatch
- **WHEN** 图片提交的本地 checkpoint 失败且前台仍是该请求的 submitting 占位
- **THEN** 页面退出 Sending、停止该占位的 watchdog，恢复完整草稿；显式原样重试使用原身份及图片且可通过既有 RAM-only gate。

#### Scenario: User edits while checkpoint is pending
- **WHEN** checkpoint 失败前用户已输入新草稿
- **THEN** 保留新编辑，将原请求完整保留为需审阅队列项，不自动重发。

#### Scenario: Foreground ownership changes before failure
- **WHEN** checkpoint 失败到达时前台已运行或属于另一个 prompt
- **THEN** 不清除该前台身份、状态或观察计时。
