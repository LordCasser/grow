## MODIFIED Requirements

### Requirement: Context mutations use effective request estimates
ChatState SHALL compute context pressure from the effective current request input estimate with the current route's provider calibration. Raw Surface content omitted from replay SHALL NOT be subtracted from provider-visible pressure. Surface mutation, request assembly, native reset and learned reasoning changes SHALL preserve this coordinate; repeated request projection SHALL be idempotent. Route replacement and rewind SHALL reset calibration; other replacements SHALL retain valid same-route calibration. Failed durable writes SHALL NOT publish changed pressure. Lifetime usage SHALL remain independent. An admitted response with a zero reported total SHALL leave the current context projection and its calibration intact. Its provider usage SHALL continue through the ordinary attempt ledger.

#### Scenario: Compaction removes mostly omitted reasoning
- **WHEN** raw removed history exceeds the current pressure while portable retained history is nonempty
- **THEN** pressure reflects retained effective content and overhead without an intermediate saturated subtraction, and a later request cannot recover phantom tokens.

#### Scenario: Explicit request projection changes
- **WHEN** an image route invalidates cached selection, native continuation resets, or portable reasoning replay changes
- **THEN** context pressure is recomputed immediately and a changed count is published before another request is built.

#### Scenario: Projection and mutation boundaries
- **WHEN** append, pruning, image projection, repair, schema changes or repeated builds alter the effective request
- **THEN** changed content is counted once, omitted history stays omitted, and valid provider overhead is preserved.

#### Scenario: Active Goal projection survives image cache invalidation
- **WHEN** 当前请求保留活动 Goal 的长指令，模型路由或图片缓存变化但 Goal 授权范围未变化
- **THEN** 重算压力仍保留当前 Goal 指令并 shadow 已过期指令；不能因图片缓存被清除而遗漏当前指令；随后构建相同作用域请求时计数保持一致。

#### Scenario: Complete provider context is lower than the heuristic
- **WHEN** 已接纳响应提供完整可信 context count，数值低于当前有效本地估值
- **THEN** 压力精确接纳该锚点，消费账本不因此改写；后续追加按本地增量计入，缩小 footprint 按当前校准参考比例重算，不以负差分饱和丢失保留内容；重复 build 不重复调整。

#### Scenario: A tool-call response reports zero usage

- **WHEN** an admitted response reports zero total tokens while the current request has context, including after a prior positive provider observation
- **THEN** the current context projection retains the existing calibration and local growth, the next positive observation can calibrate it again, and the attempt retains its reported usage.

#### Scenario: Low provider calibration survives multiple compactions and regrowth
- **WHEN** 低值provider锚点后执行多次合法压缩、增长和重复请求构建
- **THEN** 各次变化使用最新滚动参考，追加计入完整本地增量，正的保留footprint不被负差分算成零，重复构建和失败写入不重复调整，累计消费不变。

#### Scenario: Restored Goal crosses route and rewind in either order
- **WHEN** 冷恢复的当前Goal授权不变，模型切换与用户rewind先后请求，历史含其他Goal标签
- **THEN** 用户入口按既有Goal约束拒绝rewind并保留历史/压力；新lane清旧校准但按当前授权投影。独立的内部durable历史重放不能从恢复的旧标签激活旧Goal。

#### Scenario: Clearing a restored Goal updates pressure before route or rewind
- **WHEN** 冷恢复后 `/goal clear` 已持久化并清除运行时授权，随后切模型或用户 rewind
- **THEN** 在下一次请求构建或命令状态公布之前，ChatState 作用域同步为 None；恢复到历史 Goal 标签不重新激活授权，累计消费保持不变；clear 的持久化失败不清除原授权和投影。
