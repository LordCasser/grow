## Context
append/dedup 按 spec.event 建索引并 validate；serde 直接恢复 map。dispatcher 按 map 键选 handler，validate 只看内部 event。

## Decisions
在现有 deserialize_registry_hooks 遍历中先检查 map 键等于 spec.event，再调用 validate，失败返回 serde 错误。拒绝整个矛盾快照，不猜测应迁移到哪个事件、不静默删除 handler；合法快照顺序与匹配器重建不变。

## Risks
以前可解析的损坏快照现在明确失败；不扫描或修改用户数据。
