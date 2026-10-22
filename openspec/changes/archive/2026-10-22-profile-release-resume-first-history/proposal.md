## Why

R5/R6 后现代 2048 轮 debug 样本首条历史仍约 6.69 秒。需要核对串行准备和异步交付的真实边界，并用固定工具链的 release 数据决定下一轮算法优先级。

## What Changes

- 保留逐阶段原始时间线，补充首条历史前的归因证据。
- 在现有现代 admission/projection 长会话夹具上测量 release 的 RPC 与真实 TUI 边界，并单独验证修复和顺序。
- 在现有 JSONL、Timeline、Runtime 内给出后续独立优化切口，不引入 SQLite，不提前实施新存储格式或快照。

## Impact

仅基准工具、必要诊断计时和分析文档；契约保持不变，不添加 delta specs。

## 最终状态

本 change 的测量与分析已完成；C1/C2 已独立归档，C3 已完成实施、最终 correctness/release 配对验证及构建清理，见 [C3 验证](../2026-10-22-unify-timeline-history-storage/verification.md)。本分析只记录测量与取舍，生产格式和行为变化由独立 C3 承担。收尾按 OpenSpec 流程归档；本轮不包含 SQLite 或就绪/历史分页分离。
