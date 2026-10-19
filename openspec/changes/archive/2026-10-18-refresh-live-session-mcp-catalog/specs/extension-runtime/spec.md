# extension-runtime Specification Delta

## ADDED Requirements

### Requirement: Live ACP sessions refresh their MCP catalog from configuration changes
Pager 进程内 ACP session SHALL 接收适用的 `McpCatalogChanged` 并通过既有 Shell catalog reload 路径刷新其 MCP server 集合。刷新 SHALL 使用 session 实际 cwd 和现有 `mcp_recursive_config_watch` 配置控制项目监听；配置合并、信任过滤与客户端 MCP 配置优先级保持现有语义。重复相同配置 SHALL 幂等；关闭项目监听 SHALL 不登记项目路径，进程取消 SHALL 结束该运行时的配置监听任务。

#### Scenario: Global MCP server is added after session startup
- **WHEN** 活动 ACP session 启动后新增全局 MCP server 配置
- **THEN** session 收到 catalog 刷新并可 initialize/list 及调用该 server 的工具，无需重启 session。

#### Scenario: Project MCP configuration changes for an active session
- **WHEN** 已启用项目监听的 session cwd 下新增或修改项目 MCP 配置
- **THEN** 仅适用该 session 的 catalog 合并并刷新，已有目录信任过滤继续生效。

#### Scenario: Project configuration directory is created later
- **WHEN** session cwd 对应的 `.grow` 配置目录在 watcher 启动时不存在，之后创建并加入 MCP 配置
- **THEN** 延迟目录监听发现变化并刷新该 session 的 catalog。

#### Scenario: A session starts with a different cwd
- **WHEN** Pager 动态建立 cwd 不同的 session
- **THEN** watcher 为该实际 cwd 注册项目配置监听，刷新范围按该 cwd 解析。

#### Scenario: Session cwd and filesystem event use directory aliases
- **WHEN** session cwd 经过符号链接或平台目录别名，而 filesystem event 使用同一目录的规范路径
- **THEN** 延迟项目目录监听及 reload 范围匹配仍识别该 session，且不匹配其他目录。

#### Scenario: Identical catalog update is repeated
- **WHEN** 配置重载重复交付同一份有效 MCP catalog
- **THEN** session 保持同一有效 catalog，不重复注册或破坏已初始化的等价 server。

#### Scenario: Recursive project watching is disabled
- **WHEN** `mcp_recursive_config_watch` 解析为 disabled
- **THEN** 不注册项目 cwd watcher，且不产生未消费的动态路径登记；适用的 global reload 仍可工作。

#### Scenario: Client MCP configuration wins for the same merge key
- **WHEN** 刷新 catalog 时目录配置与 ACP client 配置使用相同目录合并键
- **THEN** 继续采用既有客户端配置优先级并保留其连接与工具。

#### Scenario: In-process watcher is cancelled
- **WHEN** Pager 进程取消或退出
- **THEN** 配置 watcher、动态 cwd drain 与 reload consumer 均结束，不遗留后台任务。
