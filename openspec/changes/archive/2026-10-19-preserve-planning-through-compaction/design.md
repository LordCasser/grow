# 设计

移除 Pager 同步 compaction 完成通知对根 `AgentView.todo` 的清空副作用。异步完成本来不清空；child compaction handler 也不清空。让所有 compaction 完成类型仅更新压缩行与 token 使用量，由已有显式 Plan/Planning 更新继续维护 TODO 投影。通过完整通知路由测试 root/child 的 sync/async 组合，并验证显式 Plan 更新仍覆盖已有列表。
