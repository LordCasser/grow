## Why

用户报告 Solaris 工作区主会话 `01a1a419-081c-7d92-81e0-bb3f97c017f1` 连续输入即时返回 unknown session id，连 session-info 也失败，页面仍显示子任务运行。需先找出真实 first failure，不能从错误字符串猜 leader 重连，也不能自动重建/删改正常会话掩盖因果错误。

## What Changes

源码/现场只读调查及两个隔离prefix已复现加载后退出：新通知Turn启动后，post-load child reconciler调用无范围整会话恢复，先结束新Turn再导致合法通知消费被拒绝。小纯fold控制排除缺child、payload及provider依赖。修复后置恢复的权限：只结算actor出生前冻结的历史Workflow执行(run_id, execution_epoch)，保留新actor自己的Turn/Step/request/hooks/input。

ChatState提供精确Workflow-only持久结算；Shell从加载的Timeline在spawn前捕获目标，通过现child reconciliation传递，替换post-load整会话恢复。bootstrap在执行开始前的本地恢复保持原职责。用真实reconciler与新通知Turn并存回归验收，而不放宽因果校验或修改writer deadline。原11:07首次失败仍缺第一错误，保持证据限制，不把加载缺陷修复当成原首次原因已证。

## Impact

正常运行会话及其配置、进程、关联工作区内容不改写，不重启/安装，不发真实 provider/MCP/hooks 请求。诊断材料仅放本 change，临时 probe/fixture 放 `/tmp` 或已批准的窄测试入口。源码与进行中的 ChatState runtime_context 增量按写入所有权先后交接，保留相邻脏差分。
