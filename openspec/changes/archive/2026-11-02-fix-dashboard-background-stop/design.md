从与 RowState 相同的 AgentActivityProjection 识别前台、普通 child 和 Goal。前台或 child 存在时发送 cancelSubagents=true 的现有 CancelTurn，active Goal 同时请求 pauseGoal；仅 Goal 时使用既有 /goal pause 控制。普通 child 不包含 Workflow children，后者由自己的 Run 负责停止。

active 且 management_available 的 Workflow 通过 ExecuteSlashCommand /workflow-run stop <run_id> 停止，复用已有执行平面，不进入模型队列、不依赖当前 active_view。反馈和状态等待后端事实确认，当前次 stop 不 arm delete。不可管理/加载中的活动不得伪装成停止成功。

Tier 2：以 AppView Ctrl+X + dispatch 覆盖选中后台 root，与另一 active root、draft 和终态保持隔离；对子任务/Goal/Workflow 组合及不可管理 Workflow 验证。沿既有 ACP/control 回归核对 idle cancel 与 host command 的真实入口。
