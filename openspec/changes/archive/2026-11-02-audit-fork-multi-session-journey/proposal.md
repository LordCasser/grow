## Why

用户报告 fork 后的多 session 管理存在无法删除、无法退出的问题，要求全面审查用户旅程。

## What Changes

- 核对 fork、新建、切换、Dashboard 打开/关闭、停止、删除、返回首页、进程退出和恢复。
- 以当前规范、实现、测试及可用的本机只读证据区分确认缺陷、现有保护和未验证风险。
- 保存审计与验证记录；问题拆分登记，不在审查中混入产品修复。

## Capabilities

无契约变更。`skip_specs: true`：本 change 仅保存审查证据，不改变行为、接口、状态、安全或持久化，不虚构 delta specs。

## Impact

审查 Pager 多 AgentView、Dashboard、ACP session 管理、Shell actor 生命周期与本地 session 存储。保留工作区已有 child 历史恢复改动。不操作用户运行中的 session，不删除用户数据。
