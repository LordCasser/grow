## 1. Incident evidence

- [x] 1.1 对齐用户确认的 Ctrl+D 操作、parent/child Timeline、统一日志和 memtrace，记录取消时刻与恢复时刻及归因限制。
- [x] 1.2 核对当前退出与恢复所有者，更新开发者排查说明，不改已有终态或契约。

## 2. Verification

- [x] 2.1 使用当前安装的 grow 执行现有递归树 Ctrl+D/冷恢复 PTY 回归，核对实际执行数量与结果。
- [x] 2.2 完成证据对抗性自查、文档 diff 检查和全量 strict OpenSpec 校验，记录验证范围后归档并校验 archive。
