## Context

GoalTracker 保存目标用量；共享窗口保存带稳定 owner 的 attempt；Timeline Control 保存可恢复快照。结算先提交 Control，成功后才 finish_attempt。失败时恢复 Goal 快照并返回错误。

## Goals / Non-Goals

验证已知用量和未知用量都能在提交失败后保留待结算记录，恢复持久化后只应用一次。不给产品增加新 API、容错路径或状态。不模拟真实磁盘部分写入和进程崩溃。

## Decisions

沿用 build_actor 与 ChatStateHandle::noop，交换 handle 制造可控的提交失败，保留原 handle 供重试。断言内存回滚、attempt 内容、最终 Timeline 和重复结算结果。若测试揭示运行时偏差，先另建行为 change 再修复，不通过弱化断言隐藏问题。

## Risks / Trade-offs

noop 只证明提交不可用时的逻辑路径，不证明 fsync 或崩溃窗口；本次不声称覆盖所有持久化故障。
