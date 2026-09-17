## Context

Sampler 成功路径已经把稳定 `request_id` 与 `InferenceLatencyStats::attempts` 返回给 Shell。Request `Started/Completed` 也先进入 Timeline；但 `push_response_durably` 当前只提交 `items` 与瞬态 native continuation，assistant `MessageEvent` 没有原提交 identity。

ChatState actor 的顺序是“持久化并 accept response event → 完成必要的 deterministic quarantine repair → command handler 发送 oneshot reply”。actor 在 commit 后、reply 前终止时，Shell 得到 `TimelineWriteError::AcknowledgementLost`。Shell 目前把所有接纳错误格式化为普通 ACP error，completion-requirement wrapper 因而可能追加 AutoRecovery、开始新 Step 并再次调用 provider。

Timeline 是唯一 canonical response owner；Shell 持有 policy 和 provider-admission gate；sampler 不应获得历史修改能力。

## Goals / Non-Goals

**Goals:**

- 给每个新 response admission 一个可持久化、可冷恢复核对的原提交 identity。
- 让 ChatState 对 exact local admission retry 幂等，对 identity conflict fail closed。
- 最终确认不明时停止 completion recovery 和 provider activity，同时保持 Accepted/tool gate 不变。
- 复用现有 Timeline、ChatState actor 和 Shell turn 边界，不建立第二份接纳账本。

**Non-Goals:**

- 不自动恢复执行进程终止前尚未执行的工具；cold recovery 继续按 interrupted turn 和 Surface integrity 规则收尾。
- 不把 provider-native continuation 变成持久化协议，也不从历史 response 重建它。
- 不处理 `updates.jsonl` projection 裂缝、duplicate live sampler `RequestId` 或其他 review findings。
- 不改变 provider retry 分类、completion requirement 本身或客户端 attempt lifecycle 协议。

## Decisions

### 1. Identity 与 canonical response 放在同一个 MessageEvent

新增小型 `ResponseAdmissionIdentity { request_id, attempt }`，并在 `MessageEvent` 增加 optional `response_admission` metadata；metadata 同时记录由 canonical candidate conversation 确定的 `quarantined_tool_exchanges`，使 exact retry 能返回原 admission result，而无需从修复后的 Surface 猜测。只有 `MessageCause::Assistant + SurfaceOp::Append` 可以携带该字段；identity 必须引用当前 Timeline 中已完成的同一 sampler request/attempt，quarantine 数量必须由既有 Surface 与 event items 可重复计算，且一个 identity 只能对应一个 assistant response。

新字段使用 `serde(default, skip_serializing_if = "Option::is_none")`：已有 current-schema response 保持可读，但 `None` 不能满足任何新的不明 admission。此 additive 变化不提升 `TIMELINE_SCHEMA_VERSION`，避免把全部 schema-v25 会话误判为不可读；旧 binary 会因 `deny_unknown_fields` 拒绝新事件，因此不承诺 downgrade 读取。

选择 request ID + final attempt，而不是 outer prompt ID：一个 prompt 可含多个合法模型 Step；sampler request 与最终 attempt 精确指向产生该 candidate 的 provider execution。identity 直接位于 response event，避免“response 已提交、后置 receipt 未提交”的第二个不明窗口。

**Alternative:** 只增加 fatal error marker。它能阻止立即重采样，却无法在冷恢复区分 Missing、exact duplicate 与 conflict，不满足原 identity 核对契约。

**Alternative:** response 后再追加独立 receipt。两个 durable events 之间会产生新的 partial-commit protocol，故拒绝。

### 2. ChatState admission 采用现有 input/notification 的 at-least-once 模式

`PushResponseDurably` 接受 identity。actor 先按 identity 查找既有 assistant response：

- 不存在：先在 candidate conversation 上计算 quarantine 结果，再按现有顺序 commit 带 identity/result metadata 的 raw response，必要时 commit deterministic quarantine repair；仅新提交的 live response 可以安装 native continuation。
- 存在且 `items` 完全相同：不追加 response；从 immutable response metadata 取得原 quarantined 结果，若 raw response 的 deterministic repair 尚未闭合则先补齐，再返回原结果。
- 存在但 `items` 不同：返回 typed identity conflict，Timeline/Surface 不变。

实现可以像 `submitted_input_event` 一样对当前 Timeline 做线性查找；本 change 不新增常驻索引。response 数量与会话事件数同阶，且 admission 不是高频扫描入口。

canonical equality 只比较持久化 `items`。`NativeContinuationFragment` 明确不可序列化、不可 cold restore：exact duplicate 不覆盖或重新安装 continuation；若原 actor 已成功处理 response，它已安装原 fragment，若 actor 已丢失则恢复保持 provider-neutral。

### 3. Shell fail-closed admission and cold reconciliation

Shell 从 `response_request_id` 和 `latency.attempts` 构造 identity，并将其交给当前 ChatState owner。当前 actor 的 acknowledgement loss/owner failure 直接以 `fatal_turn_boundary_error` 分类为 response-admission boundary failure；Shell 不通过同一失效 owner 盲目重试。completion-requirement wrapper 已对该 marker fail closed，因此不会发布 `RetryState::Retrying`、追加 AutoRecovery 或开始新 provider Step。

在 cold/replacement actor 已从 canonical Timeline 恢复且 owner 可用时，后续 exact identity + payload reissue 由 ChatState admission 幂等核对；该核对不经过 sampler、不消耗新的 provider attempt，也不重新安装 native continuation。

`finish_sampling_preview`、response context anchor、Accepted、semantic handling 和工具执行仍严格位于 confirmed admission 之后。identity conflict 与 unresolved admission 都不能跨越该 gate。

### 4. Cold recovery 保留响应但不自动执行副作用

Timeline replay 从 MessageEvent 直接恢复 response identity。若 commit 已完成而进程在 reply 前终止，response 作为 canonical Surface 历史保留；`recover_interrupted` 关闭未完成 Step/Turn，`recover_surface_integrity` 对 dangling/malformed tools 做既有 quarantine/repair。恢复不自动重新调用 provider，也不自动执行该 response 的工具。

后续显式核对同 identity 时可返回 exact existing admission；历史 `response_admission=None` 不能通过位置、文本相似或“最后一条 assistant”启发式匹配。

## Risks / Trade-offs

- **Risk：MessageEvent wire shape 增加字段，旧 binary 无法读取新 event。** → 保留 schema-v25 历史读取，新 writer 不承诺 downgrade；开发说明记录该边界。
- **Risk：exact retry 落在 raw response 与 quarantine repair 之间。** → raw response metadata 固化预先计算的 quarantined 结果；duplicate path 在返回确认前补齐缺失的 deterministic repair。
- **Risk：线性查找随 Timeline 增长。** → response admission 每模型 Step 一次；先复用已有简单模式，只有证据表明热点后才增加索引。
- **Risk：当前 actor acknowledgement loss 后无法继续确认。** → 返回 typed fatal boundary，停止 provider 和工具；cold/replacement owner 通过 canonical Timeline 提供后续 exact reconciliation，不把 unavailable 猜成 Missing 或 Committed。
- **Risk：attempt 计数为零或与 Request Completed 不一致。** → Timeline fold 拒绝无匹配 completed request/attempt 的 identity；Shell 从 sampler 成功 metrics 传递，不自行递增。

## Migration Plan

无需离线迁移。已有 response 读取为 `response_admission=None`；新 response 开始携带 identity。回滚到旧 binary 不能读取新字段，需同时回滚会话数据或保持新 binary，因此本 change 不承诺 downgrade。实施完成后更新开发者说明并归档 delta。
