# Bind session LSP construction to its plugin snapshot

## Why

Backlog AC-03a：Shell bootstrap已按本次cwd、trust、session pluginDirs构造 `session_plugin_registry`，用于Agent定义及runtime；LSP构造段仍另外读取进程shared latest，可能漏掉本会话插件，或把A目录允许的插件命令投入B会话。当前源码在 `agent/mvp_agent/agent_ops.rs:1660` 和1741保留该分歧。现有项目LSP trust gate只约束Project来源，不能修已经误标允许Plugin的上游快照。

## What changes

LSP仅在本次bootstrap需要新建backend时消费同一个已构造的session plugin registry。保留active_plugins过滤、file/inline来源合并、project trust和user/project/plugin优先级。已继承的父LSP backend不被覆盖；本项不增加运行时LSP全量reload，不改Skills API（AC-03b另项）或插件reload语义。

## Impact

生产落点是Shell session bootstrap的既有LSP construction分支，不新增公共接口或配置实体。用实际MvpAgent新会话与隔离stubLSP验证不同cwd/同名server，以及同cwd不同session pluginDirs的file和inline来源。受控进程不得访问真实用户项目/配置。实施等待当前问答里程碑构建窗口，不在其固定source期间改Shell依赖。
