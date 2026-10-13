## Context

本项依赖前两项的 typed observation 和收尾责任，补齐重发与清理的恢复边界。

| 入口 | 现状与复用点 |
| --- | --- |
| `crates/codegen/pager/src/app/root/dispatch/prompt.rs` | 即时发送生成新 prompt_id；失败后恢复 composer 或需要 review 的 pending draft。 |
| `crates/codegen/pager/src/app/session/mod.rs::InFlightPrompt` | 保存内容和图片等展示信息，没有完整的持久接纳关联。 |
| `crates/codegen/shell/src/session/input_inbox.rs::InputPayload` | 已有不可变 payload、prompt_id、内容、origin、turn_kind 和 queue 信息，可作为请求比较基础。 |
| `crates/codegen/shell/src/session/actor/input_admission.rs::run_human_input_admission_hook` | 每次随机生成 input_id；先提交 Submitted，再执行接纳 Hook。 |
| `crates/codegen/chat-state/src/commands.rs::SubmitInputDurably` | 同 input_id/intent/payload 幂等，冲突拒绝；尚未形成贯穿客户端重发的关联。 |
| `crates/codegen/shell/src/session/storage/jsonl/mod.rs::delete_if_still_stale` | 已取得 candidate writer lease 并复查 activity/identity，但未检查未闭合 subagent 链。 |
| `crates/codegen/workspace/src/worktree/mod.rs::run_auto_gc_best_effort/gc_worktrees_mgmt` | 独立调用 fast-worktree 清理；不能假定这些入口拥有 Shell Timeline 权威。 |
| `crates/codegen/fast-worktree/src/db/mod.rs::WorktreeKind/WorktreeRecord` | 已有 Subagent kind、session_id 与工作树记录，可用于保守识别需要上层资格判断的资源。 |

既有 Durable input recovery 仅约束已接纳输入的恢复；本项补充 RPC 提交结果不明。既有图片引用清理、TTL 安全计算、activity recheck 和本地草稿大小/单源约束继续适用。

## Goals / Non-Goals

- 同一发送意图的重试关联同一持久输入，不重复接纳、Hook 或排队。
- 自动维护只删除已证明不再被恢复链需要的资源；清理与引用发布之间有所有权边界。
- 限制存储故障后的新增工作量。不会自动删除用户工作成果，也不建立独立恢复数据库。

## Decisions

### 1. 使用既有 prompt identity 建立确定的 input identity

客户端首次提交前固定 `(session_id, prompt_id)` 和完整请求快照，重试沿用；Shell 在该 session 内由 prompt_id 确定 input_id，复用 SubmitInputDurably。比较不可变 intent/payload，包含原请求的 route/steer target 等影响执行的语义；尚未进入 InputPayload 的原始路由信息在既有 payload 中补齐，不能在重试时按当前环境重建。不增加另一份可变映射表。

同 identity 的精确重放返回原 Submitted/AdmissionResolved/消费状态；相同文字的新显式发送仍用新 prompt_id。不同 payload 复用旧 identity 在 Hook 前拒绝。queue、steer、followup 入口必须同样在第一次派发前持有身份；内部调用也在源入口只分配一次。

已有记录可以从已验证 InputPayload 的 prompt_id 建立只读关联；缺失、重复或冲突关联返回证据不足，不能按文本相似性猜测。这里不保留旧的随机重试语义，也不迁移或回写历史身份。

### 2. 区分请求接收、接纳决定和执行

输入状态包括未确认、已 Submitted 但 admission 未决、已允许排队/执行、已拒绝/中断，以及明确未接纳。读快照暂未发现 input 不能单独证明未接纳：原请求可能还在排队或写入。只有原提交操作已明确未写入，或在旧实例完整 drain 后由新 writer 验证不存在且没有在途 admission，才能返回明确未接纳。

重发遇到同 identity 的活跃接纳工作时附着现有 owner，不再运行 Hook。Submitted 已提交但进程在 Hook 中退出，恢复使用既有 ProcessInterrupted 拒绝边界，禁止猜测 Hook 是否发生外部副作用并自动重跑。已完成的接纳决定和排队/消费状态按原事实返回。

### 3. 草稿保全与操作语义

现有 draft payload 增加提交身份、原请求摘要及未确认 disposition，仍只保存一个 prompt source，沿用大小和图片引用预算。发送前 checkpoint 绑定身份与快照；checkpoint 失败时保留内存内容并显示本地未保存限制，不能声称重启后仍可恢复。

未知 ACK 的草稿可查看和编辑，但自动队列不会作为新输入派发。未编辑重试使用原 identity；若原输入已接纳，合并到该项。编辑将单一草稿内容转为新意图，原提交只保留 identity、摘要及仍有效的快照引用作核对；旧快照不可得时只允许状态核对或显式新发送，不能用编辑内容重构旧请求。显示旧输入仍未确认/已接纳，由用户显式发送后才使用新 identity；不能把新内容重放到旧 identity，也不能静默删除新编辑内容。迟到 ACK 只更新其原绑定和原提交项。

同一 session 的多客户端发送可共享已接纳状态；不同 session 或绑定的草稿不串用。权限仍由 Shell 判定，客户端提供 identity 不授予额外访问权。

### 4. 从已有持久关系判断保留

恢复根包括：未闭合 Spawned、匹配 Seed/child result、缺失 parent Ended 的结果、尚需交付的 receipt，以及相关输入/图片/结果 artifact 和尚未取得有效 snapshot 的 worktree。按 surface_completion 和实际 owner 区分无需通知的内部 child；已经 Consumed/Dismissed 的通知不继续占用交付保留根。终态/引用损坏时保守保留并报告原因。

不新增永久 pin ledger。复用 Timeline 引用、SubagentSeed 的 parent_timeline_id/parent_spawn_seq、worktree 的 kind/session_id 推导资格；索引或扫描缓存仅作候选筛选。进程重启后保护仍能由这些事实建立。

Session TTL 删除先枚举候选，再对该 candidate 及关联生命周期 owner 尝试取得既有 writer lease，按稳定顺序非阻塞获取、失败即释放并跳过，避免和活跃写者死锁。持有相关 lease 后重读 identity/activity/关系，再决定删除；scan 时的“没有引用”不构成删除许可。相同关系的新发布或修复必须经过相应 owner 的 writer 边界，不能跨越正在删除的实体。

artifacts 继续使用既有 publication/cleanup gate，根集合以经过校验的 Timeline 为准。锁、预算或读取失败都意味着本轮不删除。跨实体清理不宣称原子事务；删除中断后，剩余证据无法证明安全的项保留供后续检查。

### 5. 覆盖所有 worktree 清理入口而不反转 crate 依赖

正常 child 收尾、启动失败回滚、session TTL、显式 GC 的普通模式、自动 GC 和 orphan snapshot 清理分别核对。fast-worktree/workspace 不依赖 Shell：任务关联的 Subagent worktree/snapshot 需要调用方在实际删除期间持有已核验的资格；缺少该能力的独立自动 GC 保守跳过这些项。资格是当次操作持有的 guard/回调结果，不是可过期复用的持久 allow 位。

Shell 可以在引用核对和锁保护下执行既有删除入口；只有原工作树已被可验证持久 snapshot 覆盖、必要引用已确认且没有仍需保留的修改时才清理。未写 spawn 且从未交给执行器的 pristine 新工作树可按明确回滚证据删除。含用户修改、ACK 不明或身份不匹配的路径不走这种例外。用户明确销毁资源的路径仍遵循现有授权边界，自动 GC 不获得这种权限。

### 6. 存储故障准入与资源预算

单 session writer 的完整性或权限错误只关闭相关 owner；根据故障来源路径和目录能力确定共享 Grow 存储域出现 ENOSPC/配额故障时，由既有运行管理 owner 关闭其管理范围内的新 prompt promotion、child spawn 和 provider/tool admission，防止更多工作无法持久化。不能仅凭相同 errno 推广到无关存储域，也不宣称该进程内门槛已经控制其他独立进程。只读诊断、取消及合法收尾/恢复仍可进入。磁盘剩余字节变化只是提示；重开需确认有效 writer 的持久屏障及既有 Goal/预算准入，不靠盲目重试新业务探测。

不建立全机存储健康服务。域外 writer 不受无关故障影响；执行中的工具沿其原所有权停止或收尾，不能假装已经撤回副作用。日志、toast 等可选展示不能阻塞收尾或无限增长。

待恢复索引分批扫描、有总字节/实体和 worker 上限；超过预算保留候选并返回未完成扫描，不将未扫描部分当作无引用。正文只在前一项定义的内存预算内尽力保留。永久不可写且进程退出时，尚未持久化数据可能丢失；不通过预留盘文件或第二份 journal 宣称零损失。

## Risks / Trade-offs

- 保守保留可能积累磁盘占用 → 展示具体阻塞原因、资源归属和已有检查/清理入口，存储准入阻止继续无界增长。
- 独立 workspace GC 无法校验 Timeline → 跳过任务资源；完整回收由持有上层资格的 Shell 执行。
- 请求 payload 内含可变路径 → 重试使用第一次固定的内容/附件快照，资源丢失时明确失败，不重新读取为另一内容。
- Hook 副作用无法跨故障做到通用 exactly-once → 中断接纳保留原事实并拒绝自动重跑，用户新的显式提交是独立意图。

## Migration Plan

前两项归档后先接入稳定输入 identity 和 draft 核对，再接入清理资格及存储准入门槛。历史未闭合来源以可验证证据保护，不自动修复为成功。实现时更新开发说明中的输入未确认、收尾受阻与清理诊断入口。完成关联故障矩阵后才归档，不提前把 delta 写入主规范。
