# 最终状态

本 change 的测量与分析已完成，三个实施 changes 均有独立验证：

- C1 `streamline-resume-codec-and-projections` 已归档。2048 轮 RPC 943.225→765.529 ms、首条历史550.976→424.361 ms；repair512 与普通512 PTY 有回退，未概括为所有路径加速。
- C2 `reuse-validated-timeline-prefix` 已归档。2048 轮 RPC 815.158→743.678 ms；重复prefix读取消除，但首条历史463.450→474.457 ms，没有首屏收益。
- C3 `unify-timeline-history-storage` 已完成实施和最终验证，见 [verification](../2026-10-22-unify-timeline-history-storage/verification.md) 与 [配对报告](../2026-10-22-unify-timeline-history-storage/paired-analysis.md)。2048轮 RPC747.1→416.7 ms、首条ACP历史438.3→232.3 ms、目录约减少49.5%；TUI末屏/慢writer没有一致改善，内存下降没有确证。

各阶段使用各自独立配对基线，百分比不能直接相加。完整来源与反例见 [方案入口](timeline-reorganization.md)。C3 的46条wire fixture、32个typed Grow variants、7类ACP、实际工具/冷ACP与主子孙挂起恢复均有对应验证记录；不把DTO golden等同于真实producer穷举。

专用release目录在全部运行完成后清理11.077GiB（含保留before binaries）；SHA、原始数据及日志保留，共享target和真实会话保留。最后全量strict在归档前为18/18；本 change 与 C3 已于2026-10-22独立归档，再做全量/归档校验。本轮没有SQLite、检查点或ready与历史分页分离。
