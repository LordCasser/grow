## Why

MCP 配置重载已经能在 Shell 合并目录配置、信任过滤并向 session 发送 `UpdateMcpServers`，但 Pager 启动的既有 ACP session 只消费 `ModelsChanged`，忽略 `McpCatalogChanged`。因此会话建立后新增的 MCP server 不会进入该 session 的工具集合，用户需重启 session 才能使用。项目目录监听及 reload 范围匹配还使用路径字面量，目录别名会使迟建配置目录事件与 session 错失匹配。

## What Changes

- 在 Pager 进程内配置重载 consumer 中接入 `McpCatalogChanged`，沿用 Shell 已有的 `grow/internal/reload_mcp_catalog` 路径更新活动 session。
- 为活动 session 注册必要的项目配置监听，使用 session 实际 cwd 和现有 `mcp_recursive_config_watch` 开关；保留目录合并、信任过滤与客户端配置优先级。
- 在 watcher 登记、取消登记及 reload 范围匹配时统一目录身份，使平台目录别名和符号链接 cwd 能正常刷新。
- 对刷新幂等、全局/项目新增、项目配置目录后创建、动态 session cwd、关闭监听开关、客户端覆盖及 watcher 取消进行验证。

不扩展为其他 UI 或 memory 配置热更新，也不改变 MCP server 连接和工具调用语义。

## Capabilities

### Modified Capabilities

- `extension-runtime`：活动 ACP session 可按配置变更刷新 MCP catalog，并正确管理项目监听生命周期。

## Impact

影响 Pager 的进程内配置重载消费与 session MCP catalog 刷新接线，以及 extension-runtime 契约和开发者说明。验证使用本机临时配置和临时 HTTP MCP server，不改用户配置、不连接真实 MCP 服务。
