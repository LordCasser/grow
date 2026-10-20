# Verification

## 修复与对抗性核对
- F1：真实 `suspend_session_tree` 调用，传输边界分别注入初始化未完成导致期限到达、freeze ACK 丢失。每次接收 freeze 时核对真实 ChatState 已是 Suspending 且 operation 相同；重试能进入节点 checkpoint 阶段。测试刻意在节点 checkpoint 返回注入错误，不冒充整树 E2E 成功。
- F2：真实 PendingInteractionGuard 在节点意图前 durable Suspended；覆盖 Permission、Question、PlanApproval。首轮 drain 超时，随后同 operation checkpoint 成功，重建全新 ChatState/EventTracker/SessionActor，prepare + commit 恢复保留原等待且无伪造 ToolResult。该用例在 execution release 前检查恢复游标；真实问题/计划审批重新打开与回答由既有 reverse-request 回归独立覆盖。
- 历史边界：最近 ExecutionReleased 清除旧周期；真实 Answered/Allowed/Rejected/Cancelled/TimedOut/Failed 仍移除精确 attempt；旧 operation 查询失败。源码核对真实 ToolResult 在等待分类前剔除调用。
- 前移意图仍允许已接纳 owner 排空；Timeline 阻止新的 Turn/Step/Request/Tool/Spawn，但既有 child write-ahead Spawn 的初始化不需要再接纳新 Spawn。未放宽 coordinator 冲突保护、未改 writer retirement 或树 barrier。

## 执行
Rust/Cargo 1.93.1，`cargo test --locked -p shell --lib -- session::actor::suspension:: session::actor::goal::goal_admission_tests:: agent::mvp_agent::tests::suspension_admission_tests:: session::actor::tests::reverse_request_session_id_tests:: session::actor::tests::runtime_ session::actor::tool::preparation::restored_pre_tool_hook_tests:: session::pending_interaction:: --test-threads=1`。

结果：**55 passed，0 failed，0 ignored**。其中新增 tree freeze 两项和 pre-intent 等待矩阵一项全部通过，后者实际覆盖三类等待。其余包含上一轮失败重试、通知回执和冷恢复检查。

`openspec validate --all --strict --no-interactive`：归档前 18 passed，0 failed。`git diff --check` 通过。归档后全量与 archive 校验见完成记录。

## 范围
未执行全仓或跨平台发布矩阵，未替换安装的 Grow 二进制、未操作原故障 session。不能把局部传输注入测试称作真实进程崩溃恢复 E2E。

## 完成记录

已归档合入主规范。归档后 `openspec validate --all --strict --no-interactive`：16 passed、0 failed；`openspec validate --archived --strict --no-interactive`：654 passed、0 failed。最终 Goal 定向复跑：15 passed、0 failed、0 ignored（与联合测试中的 15 项重叠，不累计为独立覆盖数）。
