## MODIFIED Requirements

### Requirement: Cold context pressure follows portable replay

When cold restoration starts a new native continuation lane, ChatState SHALL initialize context pressure from the provider-neutral portable history that this lane can replay, without counting omitted historical reasoning. It SHALL retain effective input and request-envelope estimates in the same coordinate. The first real request SHALL add its current request envelope through the existing projection path, and later provider observations SHALL remain authoritative anchors. Initialization SHALL NOT delete history, start provider work, fabricate usage, or clamp genuine overflow to the model window.

#### Scenario: Resume retains reasoning that is omitted from replay

- **WHEN** a restored Timeline contains enough historical reasoning to make its raw estimate exceed the window, while portable message content fits
- **THEN** context pressure reflects the portable message estimate before the first prompt, and the raw reasoning remains available in Timeline.

#### Scenario: A new message follows restoration

- **WHEN** a committed Surface append and a normal request build follow cold initialization
- **THEN** the new content is counted once, and request projection replaces the prior envelope adjustment without reintroducing omitted reasoning or subtracting it twice.

#### Scenario: Portable content is genuinely too large

- **WHEN** portable replay itself exceeds the configured model window
- **THEN** the restored context count remains above the window rather than being clipped for display.

#### Scenario: Restored active Goal is visible before the first request
- **WHEN** 冷恢复的控制授权为 Goal 模式且目标状态 Active，尚未发出模型请求
- **THEN** 首次上下文快照保留匹配该 id/revision 的最新指令，shadow 其他指令；不通过历史最后 tag 猜授权，不启动 provider；已完成/暂停/缺失目标不保留活动指令。

#### Scenario: Recovery reconciliation pauses the Goal
- **WHEN** 初始持久 Goal 为 Active，但恢复协调因执行条件不可用将其暂停
- **THEN** 会话暴露和首次请求前的最终压力按暂停后的授权重新同步，不残留活动 scope。

### Requirement: Context mutations use effective request estimates
ChatState SHALL compute context pressure from the effective current request input estimate with the current route's provider calibration. Raw Surface content omitted from replay SHALL NOT be subtracted from provider-visible pressure. Surface mutation, request assembly, native reset and learned reasoning changes SHALL preserve this coordinate; repeated request projection SHALL be idempotent. Route replacement and rewind SHALL reset calibration; other replacements SHALL retain valid same-route calibration. Failed durable writes SHALL NOT publish changed pressure. Lifetime usage SHALL remain independent.

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

### Requirement: Known input overflow cannot bypass provider admission
Before ordinary provider dispatch the Session SHALL check the current calibrated input pressure plus the actual explicit output limit against its current configured window independently of auto-compaction suppression and task output budgeting. Model-switch compaction errors SHALL propagate through existing cancellation and failure boundaries rather than authorize sampling. Automatic recovery MAY attempt existing bounded compaction; a still oversized request SHALL fail without provider dispatch.

#### Scenario: Smaller model and failed compaction
- **WHEN** 600K context on a 1M model switches to a 500K model and compaction fails or leaves a known overflow
- **THEN** the turn fails without sending the ordinary oversized request, even if compaction suppression is active.

#### Scenario: Suppression or child budget disables automatic compaction
- **WHEN** a fully projected ordinary request exceeds its configured window and automatic compaction is unavailable
- **THEN** provider dispatch is rejected while existing history and usage remain intact.
#### Scenario: Explicit output limit consumes the same window for ordinary and skill requests
- **WHEN** 普通或带技能请求的校准输入符合窗口，但加上 task clamp 后实际显式输出上限会超过总窗口
- **THEN** 两者均在 provider 派发前拒绝；相等边界可发送；不添加未声明的固定输出预留或额外 256。

#### Scenario: Provider output limit is implicit
- **WHEN** 请求未设置显式输出上限且路由未提供该上限
- **THEN** 最终准入检查已知输入，不虚构某后端的默认输出值；本地无法据此保证未知输出预算。
