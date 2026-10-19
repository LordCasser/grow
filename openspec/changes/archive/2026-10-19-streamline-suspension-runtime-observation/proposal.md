# Proposal

## Why

性能优化限定于已确认的退出热路径：挂起最开始只需要 requested runtime 的 root 身份，却通过 runtime_snapshot_before 复制完整 Timeline 后立即丢弃；已退役节点检查也只需要 runtime。

## What Changes

改为轻量 runtime 查询，保留需要历史的所有验证。增加现有日志的节点/整树阶段耗时，帮助定位真实排空等待。

不新增行为、持久化或公共接口契约，skip_specs；不放宽历史校验、deadline 或 ownership，不做泛化缓存/索引重构。后续性能方向须根据测量另行立项。
