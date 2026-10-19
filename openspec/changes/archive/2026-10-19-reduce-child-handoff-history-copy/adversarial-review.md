# 对抗性审查

- 重放入口仍将完整 event vector 交给 `Timeline::from_events`；非单调/重复序号等损坏历史仍在读取 child facts 前拒绝。
- Timeline 拒绝重复 child spawn 与 terminal/session 不匹配；对应回归分别构造越过普通 `Timeline::record` 的损坏物理序列，确保 handoff 读路径依然拒绝。
- `from_validated_timeline` 只用于 `load_light` 的冷加载结果；JSONL `load_light_data` 先调用 `opened.validated_timeline()`。该入口仍执行 CanonicalChildFacts 的 spawn/terminal 关系扫描，没有新增缓存或全局旁路。
- `suspend_session_tree` 仍在 child retire 后重读 `latest_events` 并使用它确认终态；没有将旧快照复用于最终 barrier。查询仍位于原 deadline 范围内。
- ignored 本地 probe 对比旧 borrowed clone/replay 路径与 owned candidate，并断言两者产出的 spawn/ended facts 相同。它是隔离测量，不设通过时间阈值，也不代表端到端挂起延迟。待主 agent 运行后记录实际数据。
