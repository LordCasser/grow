## Why

Replay 把标题、播放、历史时间、Behavior/Goal、任务数量和两份操作提示集中在五行底栏。用户需要正常会话的阅读现场：顶部状态、subagent/task 栏及正文保持熟悉的结构，只把输入框槽位换成播放控制。

## What Changes

- 复用普通 Agent 的被动布局、状态行、任务列表和快捷键组件；不创建 live AgentView 或 SessionActor。
- 历史任务和状态来自截至当前播放位置已交付事件；运行、完成、Goal 状态与播放器状态各自独立。
- 原 composer 槽位改成紧凑 Replay 控制框，完整诊断信息移到帮助；快捷键只在正常底部栏展示一次。
- 任务列表保留查看、搜索、折叠及进入历史 child 的能力，执行/停止/管理动作不出现。

## Capabilities

### Modified Capabilities
- `client-surfaces`：调整 Replay 页面层次，恢复正常任务和状态展示。

## Impact

`pager::replay_cmd`、`transcript_projection` 和纯展示组件 `AgentViewLayout`、`AgentStatusBar`、`TasksPane`、`ShortcutsBar`。不改变播放时钟、IDLE 策略、源数据或执行协议。
