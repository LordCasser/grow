## 1. Live MCP catalog refresh

- [x] 1.1 在 Pager in-process config consumer 接入 `McpCatalogChanged` 的既有 Shell reload 路径。
- [x] 1.2 按 session cwd 和 `mcp_recursive_config_watch` 接入当前及动态 session 项目目录监听，并核对取消清理。
- [x] 1.2a 修复目录别名导致迟建 `.grow` 事件与 session reload 范围无法匹配的问题，并验证符号链接 cwd。
- [x] 1.3 验证 global/project 新增、延迟目录、不同 cwd、幂等、disabled、客户端覆盖及临时 HTTP MCP initialize/list/call。
- [x] 1.4 更新开发者说明并完成 OpenSpec 严格校验。
