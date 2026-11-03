## Why

Dashboard 详情的确认关闭只移除本地 AgentView，服务端 actor 继续存在；最后一个本地页面还被无条件拒绝。关闭后的 roster 复活、退出所有权丢失和草稿边界因此不可靠。

## What Changes

- 确认关闭已绑定 session 时调用现有 grow/session/close，收到成功确认后才退休本地页面。
- 用现有 command state 表示关闭过程；失败保留身份与重试，不宣称已关闭。回执匹配 AgentId/session/binding epoch。
- 允许最后一个页面关闭并落到 Dashboard/Welcome，保存最新未发送草稿，不删除历史。
- 后台工作仍活跃时复用既有停止控制，不能只根据前台 Idle 就关闭页面。

## Capabilities

Modified: `client-surfaces`。

## Impact

Pager close effect/result、session dispatch、草稿捕获和 Dashboard 反馈；服务端现有 terminal close、writer drain 与历史保留语义不变。进程退出的权威 root 范围和多客户端 scope 另作下一项独立修复。
