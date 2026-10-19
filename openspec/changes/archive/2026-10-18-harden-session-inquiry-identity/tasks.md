## 1. Durable inquiry identity and recovery

- [x] 1.1 将 runtime、actor、Timeline 与 UI correlation 对齐到 `(source_session_id, inquiry_id)`，保留 peer/incarnation 的认证与归因用途。
- [x] 1.2 实现 target canonical Timeline receipt 去重、open receipt 防重入及冷恢复 `TargetRestarted` 终态复用。
- [x] 1.3 持久化 source 首次 target/question 绑定；实现完成结果复用、未完成 `SourceUnavailable` 及冲突重试拒绝。
- [x] 1.4 校验 progress/outcome/cancel response identity，并覆盖预取消、terminal remote cancel 与 source/target scope。
- [x] 1.5 完成对抗性回归、更新开发者说明并通过严格 OpenSpec 校验。
