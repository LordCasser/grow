## ADDED Requirements

### Requirement: Cross-route continuation hands off completed history at an admitted step
同一 turn 在闭合 Step 边界实际切换模型路由后，获准开始的后继 Step SHALL 在保留历史之后获得恰好一个持久化 synthetic `AutoContinue` user 边界，说明新路由接手原已授权任务及已完成工具事实。既有异步压缩续接若已提供该边界 SHALL 被复用。该交接 SHALL NOT 成为真实用户输入、权限证据、新 turn 或额外采样的理由；工具 ID、结果和执行次数保持不变。只改变同路由采样参数 SHALL NOT 添加交接。

#### Scenario: Model changes after a completed tool exchange
- **WHEN** 当前 Step 已完成工具结果，排队的模型路由切换已生效且同一 owner 获准下一 Step
- **THEN** 下一请求在原工具调用/结果后含一次交接，旧 native reasoning 不恢复；后续 Step 不重复追加，不为兼容错误重新执行历史工具。

#### Scenario: Same route changes only reasoning effort
- **WHEN** 控制只改变当前路由的 reasoning effort 等采样参数
- **THEN** 不追加模型交接，保留既有 native continuation 语义。

#### Scenario: No subsequent step is admitted
- **WHEN** 控制终止当前 turn、取消/手动压缩接管，或模型选择发生在带真实新输入的首 Step 前
- **THEN** 不为路由切换追加续接或启动额外请求。

#### Scenario: Compaction already supplies a continuation
- **WHEN** 同一边界发生路由切换且异步压缩已提交、下一 Step 使用既有压缩续接
- **THEN** 复用该 user 边界，不追加第二个路由交接。

#### Scenario: Handoff persistence fails
- **WHEN** 交接无法取得 durable append 确认
- **THEN** 沿既有持久化失败路径终止，不派发缺少交接的下一请求。

#### Scenario: Actual request still exceeds its configured budget
- **WHEN** 路由交接后当前校准输入加实际显式输出预留仍超过窗口
- **THEN** 继续执行既有有界压缩或拒绝，不因交接而放宽总窗口准入。
