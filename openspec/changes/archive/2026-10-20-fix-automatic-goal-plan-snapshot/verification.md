# Verification

真实 `initialize_goal_runtime` 与 Goal 定义重激活路径新增两条回归：保留未批准候选、无 completed_plan，检查最新 durable Control 与 live BehaviorCoordinator 完全一致；从最新 Control 重建协调器，再提交 Plan，断言 submission id 大于持久计数。已有 completed_plan、持久失败恢复与 Goal admission 测试一并执行。

Rust/Cargo 1.93.1：与挂起边界联合执行 Shell 定向回归，**55 passed，0 failed，0 ignored**，日志确认包含两条新增 Goal 测试。最终格式化后另跑 goal_admission_tests，结果在完成记录中记录。

对抗性核对：采用与 live 相同的 select_behavior 语义；同一 Behavior 选择仍保持原状态；既有 durable ACK 在 live 选择之前的顺序不变；不扩展自动审批，也不修改手动 completed-plan dismissal 规则。

OpenSpec 首次检查发现 MODIFIED delta 改名遗漏原 scenario 标题；保留原场景名后，`openspec validate --all --strict --no-interactive` **18 passed，0 failed**。`git diff --check` 通过。

冷恢复测试覆盖 Control 解码和协调器恢复，未启动新的操作系统进程；未安装或发布二进制。

## 完成记录

已归档合入主规范。归档后 `openspec validate --all --strict --no-interactive`：16 passed、0 failed；`openspec validate --archived --strict --no-interactive`：654 passed、0 failed。最终 Goal 定向复跑：15 passed、0 failed、0 ignored（与联合测试中的 15 项重叠，不累计为独立覆盖数）。
