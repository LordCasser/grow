## Context

本项以 [第一项](../2026-10-08-observe-session-runtime-health/design.md) 的 typed observation 为前置。现有实现已具备 exact append、完整 writer drain、通知 inbox 和用量幂等原语；缺口在这些原语之间的责任交接。

| 入口 | 已核对的事实 |
| --- | --- |
| `crates/codegen/shell/src/agent/subagent/handle_request.rs` 收尾 | 依次写 output、child result、父用量及 parent Ended；未提交时改写返回结果为失败，部分路径保留 worktree。 |
| `crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs::finish_child/finish_panicked_child` | 先移出 active/pending，未提交结果也进入 completed；完成缓存会淘汰旧项。 |
| `crates/codegen/shell/src/agent/subagent/mod.rs::admit_notification_receipt` | 三次尝试中的每次 ACK 等待没有期限。 |
| 同文件 `result_from_inspection/ensure_recovered_child_result_with_opened/reconcile_orphaned_subagents_with_backend` | `None` 可生成 cancelled fallback；缺结果正文会阻止部分终态展示与修复。 |
| `crates/codegen/chat-state/src/actor/mod.rs::persist_pending_timeline_event` | 持有同一 pending event；永久错误 poison writer，其他错误重试原事件，ACK 等待可能长期阻塞。 |
| `crates/codegen/shell/src/session/storage/jsonl/mod.rs`、`actor/spawn.rs::SessionThread` | 已有序号/payload exact append、file/directory sync 与 actor+persistence owner drain。 |

行为约束来自 session-timeline 的 durable authority、完整 writer drain、exact replay、resident reconnect，以及 model-sampling 的取消/关闭结算契约。现有代码偏差不能通过把未知用量填零、把未提交成功改成取消来消除。

## Goals / Non-Goals

- 任一收尾阶段失败后，owner、已确认前缀和下一步动作仍可解释。
- caller 等待有界，底层未结束操作的所有权连续；只由持久事实确认终态。
- 复用既有 Coordinator/Timeline/inbox；不新增通用恢复服务或独立 journal。
- provider/tool 是否可重新执行继续由已有采样/工具安全规则决定。本项只恢复本地事实与交付。

## Decisions

### 1. 保留收尾责任与四维状态

扩展既有 child record 的阶段，runner 返回执行观察及结构化收尾错误，而非覆盖 `success/cancelled`。区分执行结束、child result 是否确认、parent Ended/用量是否确认、receipt 是否确认；对外复用第一项的观察形状。

Coordinator 在 normal/error/panic/cancel 后持有同一 child identity 的收尾记录。每个 child 同时只运行一个推进操作，重连、手动恢复和定时重试合并到该 owner。真正的执行退出才释放执行额度；待收尾项不能被普通 completed LRU 淘汰，不能授予源实例 resume 权限。

收尾元数据槽位在任务准入时预留，容量与既有任务并发策略绑定；池满时反压新任务。正文优先保留 immutable artifact 引用，尚未持久化正文的暂存采用独立的全局字节上限，超限显示内容保全失败，不能无限持有 Arc。具体常量在实现任务中固定并进行边界测试，不增加用户配置框架。

### 2. 同一推进函数用于运行期与冷恢复

推进顺序是：确认已准入 provider/tool scope 和用量结算 → 确认 output artifact → child result → 父用量与 parent Ended → receipt → 消费/按 owner 规则 dismissed。snapshot/worktree 操作保持既有依赖，父 terminal 只能引用已经确认的结果与快照。

每次推进先核对 identity、seed、原事件及引用，跳过已确认前缀。跨父子文件没有全局事务；允许合法中间前缀长期存在。只有需要执行写操作时取得相应 writer 权限，禁止同时持有父写请求并等待子写请求造成相互等待。

可验证的 child result outcome 不受后续 artifact 读取、父提交或交付失败影响。若正文丢失，返回完成元数据加 OutputUnavailable，不能猜测最后一条 assistant 文本等于原结构化输出，也不能交付虚构成功正文。事实自身无法校验时进入 IntegrityBlocked，不声称已确认完成。

### 3. 按提交确定性选择重试

| 分类 | 下一步 |
| --- | --- |
| 明确未提交、可重试、writer 有效 | 等原操作结束后重试同一 immutable event。 |
| ACK 丢失或 caller deadline | 保留 exact identity/payload，核对原写入；读取命中后仍满足 file/directory sync 才确认。 |
| StorageFull/Quota/只读/权限等使 writer 失效 | 关闭该实例写准入，等待完整 drain；外部条件恢复后重新取得 lease 并校验。 |
| 证据冲突或损坏 | 停止自动写入，保留证据和失败阶段，等待明确修复。 |
| 查询不可用、旧 owner 状态不明 | 保持 pending；不补取消终态，不重新执行。 |

typed error 从持久化边界传到收尾策略；字符串只用于展示。不得把已尝试提交的成功事件改成失败事件后复用相同序号。

### 4. writer 替换与中断恢复

旧 SessionActor、persistence task 均退出且 lease 释放，才允许新 writer。超时、flush ACK、drop handle 或取消 future 均不足以证明这一步。未退出的 blocking I/O 继续拥有其原任务与资源许可；新请求可以结束等待，但不能并发重写。

运行期修复由 owner 显式发起，普通只读查询不执行修复。resident reconnect 保持 live turn 所有权。只有新 writer 校验后证明旧执行已结束且没有有效 terminal，才恢复未闭合 attempt/tool/turn 及用量，再形成 Recovery 来源的 interrupted child 结果。使用已有失败 outcome 加明确恢复来源，不能伪装为用户 Cancelled；已知用量保留下界，未知部分保持 incomplete。恢复间隔不算作实际工作耗时。

如果 child seed/实际会话尚未建立，保留原 spawn 的未完成事实和诊断，按可验证创建阶段恢复；不得捏造一个 child session 或结果。只有能验证完整身份与结算边界的来源才可获得 resume 资格。

### 5. caller deadline 与重试预算

单次收尾推进的 caller 默认总期限为 30 秒，取已有 shutdown/call deadline 中更早者；完成回执每次默认最多等待 5 秒，最多三次明确可重试尝试，整个交接总期限不超过 30 秒。收尾写入将本周期的绝对期限传到持久化重试边界，期限到达后不再派发新的 append；已经派发的 I/O 仍由原 owner 收束，必要时按完整 writer drain 路径退出。等待超时且原操作仍在执行时不继续排入第二次 mutation；只核对该操作及其 durable identity。所有 backoff 可取消，同一恢复周期不因重连或换调用入口重置额度。

预算耗尽后转为可观察的 pending/blocked，释放 caller，不伪造 canonical failure。owner 继续保存责任；只有实际存储条件变化、原操作结束、显式修复或合法的新 writer 恢复才开启下一周期，持续满盘不会高速轮询。进程退出后的工作清单从 durable 未闭合事实重建；纯内存正文不能保证恢复。

### 6. 等待者与父任务交付

任务等待超时可返回 FinalizationPending，说明 child identity、确认前缀与失败阶段。原 tool call 只提交一次 tool_result；迟到终态通过现有 `CompletionDeliveryTracker` 和持久 inbox 交接，按 receipt identity/version 去重。先确认交接责任，再释放 waiter；父 writer 不可用时不能声称 tool_result 或 receipt 已持久化。

原 Goal id/revision 已不再有效时，保留结果并沿 `notification_drain.rs` 的既有 dismissed 路径收敛通知，不自动恢复 Goal。Workflow 未确认调用保留原 child/call 关联，不能作为最终成功写入 journal 或重新 spawn；缺少可验证关联时停止自动恢复并报告证据不足。

## Risks / Trade-offs

- 长期存储故障 → 保留元数据并限制准入；第三项负责 GC 与磁盘资源保护，不能承诺自动恢复物理故障。
- 取消与自然完成竞争 → 尊重已经确认的 outcome；取消请求不覆盖既有成功，也不越过未结算 attempt。
- 结果正文失效 → 允许只读展示已验证终态；实际消费和 resume 仍分别经过各自证据校验。
- 后台命令可独立存活 → 继续依既有 reparent/owner 规则跟踪，child turn 返回不代表所有进程停止。

## Migration Plan

第一项归档后接入 typed errors 和 pending ownership；统一推进函数后迁移 live/cold reconciliation；最后更新 task/Goal/Workflow 调用方和故障测试。未来归档需验证第三项依赖边界已记录，不提前修改主规范。历史缺少精确证据时保留不确定性，不猜测重建完整输出或成功 outcome。
