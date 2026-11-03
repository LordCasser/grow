显式永久删除承认放弃独立 session 的本地恢复责任。已有 writer lease 阻止删除另一个 writer 正在推进的实体，canonical Spawn/Seed 双向检查保护其他 session，原 quarantine rename/revalidation/remove 是唯一 destructive commit。既然目标全部历史会被删除，无需在即将销毁的 ledger 里追加假的 ExecutionReleased，或新增另一个 abandonment 协议/账本。这与前期“可能需要单独 abandon fact”的候选方案不同：本轮将已确认的永久删除与 quarantine commit 作为清晰的放弃边界；不存在幸存的目标 ledger 声称工作已恢复/完成。

不支持隐式级联：存在真正跨 session 结果链仍拒绝并给出关联 identity。删除不宣称停止 Grow 无法控制的外部副作用，不以 checkpoint 保护为由要求用户先执行恢复。自动维护继续完整保护所有未消费 runtime/交付事实。

Tier 3：原先拒绝未消费 checkpoint 的场景改为明确授权的独立永久删除；覆盖 Suspending/Suspended/Restoring/Released，持有另一个 writer 时仍拒绝且原字节不变，释放后显式删除成功、重复删除幂等。自动 TTL 对前三阶段保持保留，既有跨 session 与完整存储回归保持。
