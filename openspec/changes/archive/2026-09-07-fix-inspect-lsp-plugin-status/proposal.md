## Why
inspect 的 LSP 列表只按 discovered plugin 的 trusted 筛选，不读取 registry.enabled。已信任但禁用插件的声明没有禁用标记，且可遮蔽已启用插件的同名配置，诊断与执行配置来源产生差异。
## What Changes
允许来源使用既有 PluginRegistry.active_plugins；其余插件声明作为诊断项单列，并分别标记 disabled 和 untrusted。复用插件 LSP 解析逻辑，保持文件优先于 inline 的规则。
## Capabilities
### New Capabilities
无。
### Modified Capabilities
- `client-surfaces`: LSP 诊断表达插件启用和信任状态，区分允许来源与被禁用声明。
## Impact
Shell inspect、tools 的插件 LSP 解析提取及回归；不修改插件启用状态、不启动进程、不删除禁用插件定义。
