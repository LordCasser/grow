## Why

官方2.6.16 Core原PTY显示后台命令已durable结算且正文有失败终态，但分页冷恢复的Tasks历史为空。分页分支只捕获正文cut，绕过了旧replay中对AgentSession.bg_tasks的填充，违反现有Resume displays and restores suspended work契约。

## What Changes

Shell在同一已验证cut投影TaskBackgrounded/TaskCompleted轻生命周期，包括CommandsSettled合成终态，通过现有grow/session/update发送仅任务状态的replay。Pager只恢复Tasks中心状态，正文仍由HistoryRow分页安装。两侧以isReplay=true和grow/taskStateOnly=true区分此路径，不写回Timeline，不触发工具或通知，不推进正文cursor。

## Scope

修复本轮分页接入，不扩大后台执行/挂起能力，不读取全量响应正文，不为Tasks另建分页。既有派生索引补全任务物理引用（不复制正文），使已完成任务可独立点读；该必要性来自indexed hydrate只保留未结算start的实际证据。原Windows cfg与死代码清理独立归档。正式后继版本在候选所有修复和验证完成后确定。
