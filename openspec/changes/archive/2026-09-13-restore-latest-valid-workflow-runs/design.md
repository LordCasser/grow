## Decisions
从已验证 Timeline 的 Spawned 候选逆序检查。沿现有 O(1) workflow_lifecycle 查询及文件校验路径，仅成功构造 RestoredWorkflowRun 后计数，达到 128 停止，最后 reverse。不创建第二份 lifecycle 投影或扫描缓存。

## Risks / Trade-offs
大量无效候选会增加 sidecar 读取；保留每文件大小限制与目录能力校验。整体扫描不超出已验证 Timeline 的有限候选数，返回对象始终最多 128。不能声称恢复读取量仍固定 128 个候选。

