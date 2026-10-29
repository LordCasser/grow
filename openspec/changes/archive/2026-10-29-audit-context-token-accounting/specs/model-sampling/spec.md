## MODIFIED Requirements

### Requirement: Context mutations use effective request estimates
ChatState SHALL compute context pressure from the effective current request input estimate plus the current route's provider calibration overhead. Raw Surface content omitted from replay SHALL NOT be subtracted from provider-visible pressure. Surface mutation, request assembly, native reset and learned reasoning changes SHALL preserve this coordinate; repeated request projection SHALL be idempotent. Route replacement and rewind SHALL reset calibration; other replacements SHALL retain valid same-route overhead. Failed durable writes SHALL NOT publish changed pressure. Lifetime usage SHALL remain independent.

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
