## Why
默认关闭的 LSP 工具有实际注册和运行入口。审计其配置路径发现项目配置先覆盖用户和插件同名项，随后不可信项目项被过滤，连可信来源的同名服务器也丢失，违背保留用户与插件配置的既有意图。
## What Changes
- 执行路径在加载合并前决定是否接受项目 LSP 来源，不可信项目不能参与名称覆盖。
- 保留执行前的信任过滤作为复核；inspect 可展示全部来源并标记项目项不可用。
- 验证不可信项目与可信插件同名、可信项目覆盖及无冲突来源保留。
## Capabilities
### New Capabilities
无。
### Modified Capabilities
- `configuration-rules`: LSP 合并仅允许已获准项目来源参与覆盖。
## Impact
tools LSP 配置合并、workspace 初始化、shell session 构建与 inspect 调用参数；不变更信任存储、LSP 协议或工具默认开关。
