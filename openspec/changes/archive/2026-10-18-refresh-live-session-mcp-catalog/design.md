## Context

Pager 的 in-process agent 在 `spawn.rs` 启动配置 watcher，但 consumer 目前只处理 `ModelsChanged`。Shell 已有 `McpCatalogChanged` 路径，可按 project root 调用 `grow/internal/reload_mcp_catalog`，复用配置合并、信任过滤与 `UpdateMcpServers`。Pager 启动路径还需给现存及动态 session 注册其真实 cwd，才能发现项目配置变化。

## Decisions

- 仅在 Pager in-process MCP consumer 接收 `McpCatalogChanged` 并调用既有 Shell reload extension；不扩展其他配置 update 类型。
- 将启动 cwd 传给 watcher，并在 agent 接收 session 前安装既有 cwd 注册通道；动态 session 通过该通道添加实际 cwd，复用 `mcp_recursive_config_watch` 开关。目录尚不存在时依赖 `watch_path` 的延迟创建能力。
- watcher 登记与取消登记使用目录规范路径，避免平台目录别名或符号链接导致迟建 `.grow` 事件无法匹配；catalog reload 按规范目录做组件级范围匹配，保留无法解析时的原路径。
- 刷新沿用 Shell 现有目录合并、信任过滤、客户端配置优先级及 session MCP diff/init/tool/snapshot 路径；同一目录合并键的配置冲突仍由客户端配置优先，不新增并行 catalog owner。
- 监听和 consumer 受进程取消生命周期约束；关闭 watcher 不遗留后台任务。

## Validation

验证 global/project 新增、项目目录后创建、动态 cwd、重复相同配置、监听关闭、客户端覆盖及 watcher 取消。用临时 HTTP MCP server 完成真实 initialize/list/call，确认已有 session 可见并调用新工具。测试运行与验收由实现阶段完成。
