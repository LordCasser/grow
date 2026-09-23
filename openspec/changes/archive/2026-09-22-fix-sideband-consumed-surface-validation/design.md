## Context

故障会话的压缩 Sideband attempt 记录 813 个 `selected_surface_ids`。其中 17 个来自 `Input::Consumed` 或 `Notification::Consumed { input: Some(_) }`。`Timeline::apply_validated_event` 会把这两类 payload 追加到 Surface，`materialize_timeline` 因而正确返回这些坐标；`sideband::surface_id_exists` 却只枚举 `Messages`、`ImageProjection` 与 `Control`，导致 `SidebandTimeline::validate_parent` 在严格实体加载时返回 `InvalidSurfaceSelection`。

同一目标在压缩前能够完成直接子 Agent inquiry；压缩后六次不同内容的 peer inquiry 都返回相同、不可重试的 `audit_failure`。第一次失败时前台 turn 已结束约 54 秒。现有进程级测试也证明 foreground provider 阻塞时 inquiry 可以交付。因此不应给 coordination 增加“等待 Surface 收敛”、等待 foreground idle 或绕开 durable audit 的分支。

## Goals / Non-Goals

**Goals:**

- 让 Sideband parent validation 精确承认 Timeline 已定义的全部 canonical Surface coordinate producer。
- 保持 source/input range、item bounds 与 parent event identity 的 fail-closed 校验。
- 用真实严格重载和 coordination 路径覆盖本次故障形状。

**Non-Goals:**

- 不修改 compaction 选区、Sideband manifest 或协调队列。
- 不把非 Surface audit/lifecycle 事件视为 Surface coordinate。
- 不为故障 ledger 增加跳过、修复写回或版本兼容逻辑。
- 不处理与本缺陷无关的 coordination UI、消息内容或模型回答质量。

## Decisions

### 1. Canonical coordinate ownership belongs to Timeline

在 `Timeline` 上增加 crate-private coordinate query。它根据已验证 parent event 的 seq、event kind 与 item cardinality 判断一个历史 `SurfaceId` 是否可由 Timeline 产生；Sideband 不再维护自己的 event-kind 白名单。

合法 producer 为：

- `Messages` 的每个 item，包括 append 与 replace；
- `Input::Consumed` 的单个 item；
- `Notification::Consumed { input: Some(_) }` 的单个 item；
- `Control` 的每个 model context；
- `ImageProjection` 的每个 shadow。

其余 Input/Notification lifecycle、Observation、Hook、Turn、Step、Request、Tool、Workflow、Compaction、Recovery、SessionTitle、Sideband 与 Subagent 事件不产生可选 Surface item。

只在 Sideband validator 增加两个 `match` arm 虽然更短，但会继续把 canonical ownership 留在消费方；本次将 query 放回 Timeline，同时保持实现为一个小型只读方法，不引入 registry 或新状态。

### 2. Existing ledgers recover by corrected validation

故障 ledger 的 event seq、item index 与 frozen input range 都有效，错误仅来自 producer 白名单不完整。修正 validator 后直接通过既有严格读取；不迁移、不重写用户数据，也不放宽 malformed ledger 的拒绝。

### 3. Coordination remains independent of foreground/subagent activity

询问仍由现有 SessionActor FIFO 和 tool-free Sideband 执行。回归在 target foreground busy/idle 且保留后台子 Agent 的形状下验证交付，但生产修复只在 canonical validation 层；不读取 `activity` 或 `active_subagents` 作为 admission gate。

## Risks / Trade-offs

- 新增 Surface producer 时仍需同步其 apply 与 canonical query。该 query 位于 Timeline owner，测试枚举 producer/非 producer，失败位置比 Sideband 私有白名单更直接。
- 接受历史 consumed 坐标会让先前被拒的合法 ledger 可加载。source refs、item bounds、event seq 与 purpose-specific materialization 校验保持不变，不会接受任意历史事件。
