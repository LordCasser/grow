# 功能入口核对

本轮为静态入口审计，不代表下表全部功能已完成故障/取消/持久化验证。路径均相对仓库；默认值指没有任何覆盖项。

| 功能 | 默认与执行证据 | 后续范围 |
| --- | --- | --- |
| LSP 工具 | `shell/src/agent/config.rs::resolve_lsp_tools` 默认 false；`mvp_agent/agent_ops.rs` 构建工具；`config-types/src/flags.rs::BoolFlag::env` 证明默认 false | Workspace 自身 LSP 与工具开关独立，不能推导全部 LSP 进程禁用；已归档来源/inspect 修复 |
| web_fetch | `resolve_web_fetch` 默认 false；`prepare_web_fetch_config` 返回 Enabled/Disabled；`agent/src/builder.rs` 条件注入 | 已归档缓存/类型/请求边界修复，不声称全部 HTTP 平台路径验证 |
| ask_user_question | `resolve_ask_user_question` 默认 true；`mvp_agent/agent_ops.rs` 和 `subagent_coordinator.rs` 使用，允许启动元数据覆盖 | 后续检查等待、取消与子 Agent 交互完整生命周期 |
| write_file | `resolve_write_file` 默认 true；主 Agent 构建和子 Agent coordinator 都传入 | 不以开关存在推断可删除写入能力 |
| session recap | `resolve_session_recap` 默认 true；`extensions/recap.rs` 检查 `is_session_recap_enabled`；ACP 宣告能力 | 后续核对 recap 异步任务与模型切换/取消交错 |
| cancel rewind | `resolve_cancel_rewind` 默认 true；ACP 元数据到 Pager `acp/mod.rs`，最终 `app/root/dispatch/turn.rs` 检查 | 确认有 UI 执行消费者，非仅展示字段 |
| MCP liveness | `agent/config.rs` canonical resolver 默认 true；`session/actor/run_loop.rs` 非子 Agent 且启用时装 channel/dispatcher | 下一轮检查生命周期与退出 |
| MCP auto restart | 默认 true；同一 run_loop 在 liveness 区域内条件创建 RestartActions | 实际依赖 liveness dispatcher；下一轮检查失败重试与关闭交错 |
| MCP recursive config watch | 默认 true；`agent/app.rs` 在创建 cwd channel 前检查；名字虽含 recursive，注释与实现描述为窄范围非递归监听 | 下一轮核对事件合并、重载与资源清理，不因命名误判为未实现 |
| 内嵌搜索替换 | `resolve/toolset.rs::resolve_search_tools_enabled` 默认 true；`session/actor/spawn.rs` 消费 | 禁用环境变量优先；继续检查命令实际路由 |

上述 shell/agent/tools 路径均位于 `crates/codegen/`，Pager 同理。使用 rg 核对定义、调用及源码条件，未读取或改写用户配置，也未启动外部 MCP 服务器。本轮没有新增已确认 bug，不虚构修复；已有 R1/R2/R3 删除候选保留在根目录临时文件，未执行删除。

下一项选择 MCP restart/dispatcher，因为已确认生产入口及两个开关的依赖关系。下一轮必须读取相关规范和生命周期实现后立项，不能仅根据此表直接修改。
