## MODIFIED Requirements

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

#### Scenario: Low provider calibration survives multiple compactions and regrowth
- **WHEN** 低值provider锚点后执行多次合法压缩、增长和重复请求构建
- **THEN** 各次变化使用最新滚动参考，追加计入完整本地增量，正的保留footprint不被负差分算成零，重复构建和失败写入不重复调整，累计消费不变。

#### Scenario: Restored Goal crosses route and rewind in either order
- **WHEN** 冷恢复的当前Goal授权不变，模型切换与用户rewind先后请求，历史含其他Goal标签
- **THEN** 用户入口按既有Goal约束拒绝rewind并保留历史/压力；新lane清旧校准但按当前授权投影。独立的内部durable历史重放不能从恢复的旧标签激活旧Goal。

#### Scenario: Clearing a restored Goal updates pressure before route or rewind
- **WHEN** 冷恢复后 `/goal clear` 已持久化并清除运行时授权，随后切模型或用户 rewind
- **THEN** 在下一次请求构建或命令状态公布之前，ChatState 作用域同步为 None；恢复到历史 Goal 标签不重新激活授权，累计消费保持不变；clear 的持久化失败不清除原授权和投影。

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
- **THEN** 两者均按同一总预算准入，无法恢复时在 provider 派发前拒绝；相等边界可发送；不添加未声明的固定输出预留或额外 256。

#### Scenario: Provider output limit is implicit
- **WHEN** 请求未设置显式输出上限且路由未提供该上限
- **THEN** 最终准入检查已知输入，不虚构某后端的默认输出值；本地无法据此保证未知输出预算。

#### Scenario: Explicit output overflow enters existing automatic precompaction
- **WHEN** 已投影输入低于普通压缩阈值但加实际显式输出预留超过窗口，输出预留小于窗口且既有自动压缩准入允许
- **THEN** 先跨闭合Step边界执行一次有界预压缩，prune和图片重投影按相同总预算复核；重新投影后只有总需求符合窗口才发送普通请求。

#### Scenario: One compaction cannot fit the reserved output
- **WHEN** 为输出预留触发的压缩或后台提升已完成，但总需求仍超窗
- **THEN** 保留已提交压缩及其Completed，结束本轮且不派发普通请求，不反复重发摘要；自动恢复被抑制或受子任务输出规则禁用时仍由最终准入拒绝。
