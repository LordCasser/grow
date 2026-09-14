## Context
load_light_data 的 recover_sidebands 已区分 writer/observer；load_session 总是观察。两者却无条件调用会写 Summary 的 reconcile 方法。参见 jsonl/mod.rs 与 persistence::load_light 的 claim_writer 调用方。

## Decisions
复用已有模式边界，统一表达 repair_projections/写者恢复意图，不增加 actor 或存储实体。title/model 先按 canonical Timeline 验证、派生；只有 admitted writer 执行持久化更新。full load 明确只读。title 同 seq 或更高 seq 冲突仍失败，非法 model observation 仍失败。

## Risks / Trade-offs
观察可能返回比磁盘 Summary 更新的值，这是 Timeline 权威的直接结果。磁盘修复延后到持有独占 lease 的加载；观察不保证多个文件的跨文件原子快照。

