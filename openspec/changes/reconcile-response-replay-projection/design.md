## Context

`push_response_durably_with_identity` 已保证 canonical assistant response 先进入 Timeline，Shell 才发布 Accepted 或执行工具。Streaming text/thought 仍作为 `{samplingRequestId, samplingAttempt}` candidate 暂存在 persistence actor；`SamplingAttempt::Accepted` 经独立 event queue 到达后，`flush_sampling_candidate` 才逐条写 `updates.jsonl`，错误仅告警并丢弃条目。停止/channel close 又会有意丢弃未收到 Accepted 的 candidate。

普通 session/load 原样回放 `updates.jsonl`；子任务视图和导出也有直接 replay reader。它们不与 Timeline response admission join。因此 Timeline ACK 后停止，或 accepted candidate write 失败，会永久形成模型/UI split-brain。

## Goals / Non-Goals

**Goals:**

- 让每个新的 identity-bearing response admission 具有恰好一次、可校验且可冷重建的 replay projection。
- 将 projection durability 加入现有 side-effect gate，禁止缺失 projection 时继续采样、执行工具或发布成功 lifecycle。
- 让 candidate stream 只承担 provisional live preview；accepted history 由同一个纯 projector 生成，不再把 provider chunk timing 当作 cache authority。
- 在普通 load、cursor reconnect、子任务 replay 和导出中复用同一 record 解析与缺失重建规则。

**Non-Goals:**

- 不从 replay reconciliation 启动 provider、恢复 native continuation、执行工具或继续 truncation/pause-turn。
- 不重建没有 `response_admission` identity 的历史 response，也不对旧 cache 做文本/位置启发式迁移。
- 不把全部 tool progress/result 改成 Timeline projection；tool runtime 继续拥有完整 ACP ToolCall/ToolCallUpdate 历史。
- 不顺带修复 duplicate live sampler RequestId、historical BackendToolCall replay 或其他 architecture review finding。

## Decisions

### 1. Timeline 暴露已验证的 branch-local admitted response view

ChatState 在 validated Timeline fold 上提供只读 admitted-response view：Timeline event sequence、response identity、canonical items、deterministic quarantine result，以及该 event 是否仍属于 rewind 选中的 branch。该 view 不修改 Surface、不安装 continuation，也不执行 integrity repair。

Compaction/content replacement 不能抹去原始 branch response；rewind 已切走的 response 不能被重建。实现应复用 `branch_transcript_with_ids`/现有 provenance fold，而不是把“当前 Surface 中可见”等同于“属于当前历史 branch”。Legacy `response_admission=None` 不进入新 view。

新增该 query 是必要的：storage/replay 需要由 canonical owner 提供经过 Timeline 校验的响应集合，不能在 Shell 重新实现 branch/rewind 规则。

### 2. Shell 持有唯一的 deterministic response replay projector

在 session/acp conversion 边界增加纯 projector，以 admitted-response view 生成 versioned `ResponseReplayProjection`：

- key 为 session + `{request_id, attempt}` + Timeline event sequence；
- digest 覆盖 canonical response items、quarantine result 和 projection schema version；
- healthy response 生成确定性的 ACP AgentThoughtChunk/AgentMessageChunk，assistant text 同时覆盖“provider 未流式输出、只能 fallback”的情况；
- tool-call identity 纳入 digest/order validation，但完整 ACP ToolCall/ToolCallUpdate 仍由 tool runtime projection 持有，不能由 reconciliation 触发 dispatch；
- quarantined response 生成显式 `Discarded` disposition 和零个 raw candidate updates，不能把 malformed tool preview 复活为 accepted/executable history。

projector 不读取 transient candidate buffer，不调用 provider/tool，不分配 authority。live commit 和 cold synthesis 必须使用同一函数，避免两套 response→UI 语义。

**Alternative: 持久化原 candidate chunks。** 拒绝。Chunk 边界、fallback 和 provider delta 是 provisional delivery 细节；部分写入无法证明完整，quarantine 也不能把 raw candidate 当作 accepted history。

**Alternative: 只持久化 SamplingAttempt::Accepted marker。** 拒绝。Marker 不能表达 projection version、payload digest、quarantine disposition 或 fallback content，并会混淆 public preview lifecycle 与 storage receipt。

### 3. 一条 storage-only projection record 是 cache 的提交单位

`updates.jsonl` 增加独立内部 record，一条 JSONL 包含 projection key、digest、schema version、disposition 和确定性的 projected ACP notifications。它不作为 Grow public notification 转发；所有 replay readers 在边界处展开为普通 ACP updates。

单条 record 避免“部分 projected chunks 已写、terminal marker 未写”的第二个事务。append 以 key + digest 幂等：

- 缺失：durably append；
- 同 key、同 event/digest/version：视为 exact success，不追加第二份；
- 同 key 但内容冲突：typed conflict，保留原 cache 和 Timeline，停止 writer epoch 的该边界。

record 的 replay anchor 使用其 projected updates 的有序 event IDs；cursor 只能在完整 record 边界继续。若 cursor 位于 record 内部且不能证明其余 updates 已应用，回退 full replay，不能跳过半个 response。

### 4. Projection commit 经过 session event FIFO，并成为 live side-effect gate

Shell 在 Timeline admission ACK 后，从 admitted response/result 构造 projection。Fallback-only text 在 preview identity 仍有效时进入同一 event FIFO。随后 enqueue 一个带 oneshot 的 projection barrier；run loop 只有在此前 candidate/independent notifications 已处理后才把 commit 命令交给 persistence actor。

persistence actor 在该 serialized boundary：

1. 取得 exact `{request_id, attempt}` candidate window；
2. 丢弃 candidate ACP rows，保留并按原序 durable 写入交织的非 candidate rows；
3. 幂等 durable append 单条 projection record；
4. 只在 record 已确认或 exact reconcile 成功后 ACK。

ACK 后 Shell 才发布 public Accepted；quarantine 发布 Discarded。随后才允许 usage-followup UI、truncation/pause-turn continuation、tool prepare/dispatch 或 Turn terminal。普通 Accepted 到达 persistence actor 时只闭合 transient window，不能再次 flush candidate。

projection failure/acknowledgement loss 映射为 typed `response-projection` fatal turn-boundary error。它停止 completion recovery、provider resampling、continuation 和工具；可恢复错误进行有界 exact retry/reconcile，永久错误 fail closed。后续写入不能绕过未闭合 projection boundary。

### 5. Reconciliation 在 replay snapshot/cursor cutoff 之前完成

storage 提供共享 reconciliation core：读取 validated Timeline admitted-response view，扫描 projection records，校验 key/digest/version/disposition，并返回完整 replay stream。

- replacement writer/cold session load 在读取 replay snapshot 前补写缺失 record；repair 不执行 provider/tool/continuation。
- read-only/direct replay（子任务视图、导出）不能获取 writer lease；它在内存中合成同一 record 并展开。
- resident load 先走现有 actor flush barrier；新 live gate 保证任何已确认 admission 的 projection 已闭合，随后仍使用同一校验器拒绝 conflict。
- missing projection 的合成发生在 load response、leader cutoff 和 buffered-live release 之前；不能先回放 later tool/result 再补 response。

新 gate 保证 projection gap 后不存在更晚的可持久副作用。若扫描发现同一 branch 上缺失 response 之后已有相冲突的后继 projection、无法确定插入顺序，load 必须返回 typed corruption/reconciliation error，而不是在末尾猜测追加。

### 6. Rewind、discard 和 legacy 使用明确边界

- 只重建 rewind 选中 branch 上的 identity-bearing response；已切走 response 的旧 cache 继续由现有 rewind filter 处理。
- earlier discarded attempt 的 transient chunks 没有 Timeline response identity，不生成 record；later admitted attempt 只能匹配 exact attempt。
- legacy identity-less response 保持现状，不能借助文本、最后一条 assistant 或 request proximity 猜测。
- quarantined admission 的 record 证明“该 response 已处理但 raw preview 不可接纳”，防止每次 load 重复尝试复活 candidate。

## Risks / Trade-offs

- **内部 record 扩展多个 ACP updates，cursor 不能假设一行等于一条 client event。** → record 保存有序 event IDs；完整边界可增量继续，内部 cursor 回退 full replay。
- **cache record 复制部分 canonical display payload。** → Timeline + digest/version 仍是 authority；record 可删除并从 Timeline projector 重建，不参与模型请求。
- **projection gate 增加 response 完成延迟。** → 只增加一次本地 durable cache append；这是在工具/下一 provider admission 前闭合用户可见因果所需成本。
- **tool-only response 的 record 可能没有 text/thought payload。** → record 仍提供 admission/digest/order receipt；完整工具行继续由 tool runtime 持久化，并必须位于 response projection boundary 之后。
- **永久 cache I/O 失败会停止当前 turn。** → 不允许模型状态继续领先用户历史；session 保留 canonical Timeline，可在 storage 恢复后 exact reconcile。

## Migration Plan

无需离线迁移。只有带 `response_admission` 的新 response 受 projection gate 约束；旧 identity-less response 不自动重建。实现、delta specs、开发说明和回归验证完成后归档 change。回滚到不认识内部 record 的旧 binary 不受支持。
