## Context

普通 `list_sessions` 当前将 InvalidData 候选作为无效条目跳过，这是会话列表和 TTL cleanup 的既有边界；搜索 bootstrap 再用返回的 ID 集合裁剪全文索引。`reindex_all` 已能传播摘要枚举和 reader admission 错误，但 JoinSet 内的读取、超时、upsert 错误以及任务 panic 只记日志并继续 prune/写标记。详见 proposal.md 与 delta spec。

## Goals / Non-Goals

**Goals:**

- 搜索枚举能够区分完整快照与已打开目录内缺少摘要、摘要损坏或身份无法核对的部分快照；只在打开候选目录之前确认其已消失时继续跳过。
- 任一必要索引任务失败都能传回 bootstrap 所有者，阻止裁剪和完成标记。
- 从 claim 成功开始清除旧完成标记，使失败或中断后的 Recheck 能识别未完成状态。

**Non-Goals:**

- 不改变普通会话枚举或 TTL cleanup 的 InvalidData 跳过策略，也不定义 cleanup Once 的新产品语义。
- 不要求失败前已成功 upsert 的其他会话回滚；SQLite 索引是可重建派生数据，下一轮完整 bootstrap 会收敛。
- 不改变 oversized Timeline 的既有 title-only 策略；仅当该必要占位写入自身失败时整轮失败。

## Decisions

- 在 `StorageAdapter` 增加明确的搜索索引枚举入口，JSONL 将已打开会话目录中的 summary NotFound/InvalidData、物理身份无法核验及当前格式验证失败作为错误；目录在打开前消失仍可跳过，普通 `list_sessions` 保持原策略。相比全局改成严格扫描，这不会改变 cleanup 对无效实体的既有处理。当前只有 JSONL 实现该 trait，新增方法作为必需契约，避免未来 adapter 静默继承宽松默认值。
- JoinSet 索引任务返回 `io::Result<()>`。Timeline fold、timeout、upsert、title-only upsert 错误及任务 panic 统一计为 incomplete；Drain 结束后若有任一失败即返回错误，且不调用 orphan prune 或完成发布。成功的 oversized title-only 路径仍是成功的有界索引策略。
- claim 和旧标记处理在 SQLite Immediate transaction 中完成。首次 Launch 即使旧标记存在也必须重建，所以同事务清除它；非强制 Recheck/等待者若标记在前置探测后、claim 前出现，则同事务释放刚取得的 claim 并 adopt 完成标记，避免无意义的第二轮全量扫描。真正开始重建后标记已清除，因此后续失败或进程中断可由 Recheck 发现。
- 回归通过临时 root 下真实 `JsonlStorageAdapter` 和真实 SQLite 搜索索引验证：损坏 summary 保护旧行；Timeline 故障不写完成标记，修复后 `RecheckBootstrap` 产生可查询行。测试不依赖全局 manager。

## Risks / Trade-offs

- **某个有意无效的目录会阻止搜索 bootstrap →** 仅搜索全量索引路径采用严格枚举，错误可见且不损失旧结果；普通列表和 cleanup 继续沿用 InvalidData skip。
- **失败轮次已 upsert 的无关行可能先被新内容替换 →** 不执行删除或完成发布，修复后全量 recheck 收敛所有行；不引入跨 SQLite 多事务 rollback 层。
- **重建开始前的标记探测存在竞争 →** claim 与 marker 查询、adopt/清除在同一 Immediate 事务中决定；peer 刚完成时的非强制调用保留并 adopt marker，不会额外触发全量重建。
- **旧完成标记在真正重建开始时被删除 →** 仅新 claim owner 能在事务中执行；之后失败或中断会留下缺失标记，Recheck 可重试。

## Migration Plan

无数据库 schema 迁移。实现搜索严格枚举、claim-fenced marker 删除和任务错误聚合后，先运行真实临时 root 的定向测试与 OpenSpec 校验，再归档契约。失败时既有搜索行保留，完成标记缺失可触发下一次重建。
