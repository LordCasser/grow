## Context
inspect_report 已构造与运行时同型的 PluginRegistry，允许来源无需再按 discovered.trusted 推断。sourced loader 同时处理用户、项目和插件；为单独列出禁用插件声明，需要复用其中插件文件/inline 的解析及覆盖规则。
## Goals / Non-Goals
展示准确的配置启用状态，并保留被禁用定义的诊断价值。不改变插件生命周期、信任判断和 LSP 协议，不将条目视为运行中进程。
## Decisions
list_lsp_servers 接收已有 registry。active_plugins 参与允许来源合并，其余插件按插件单独解析后追加；新增 disabled 布尔沿用 MCP 诊断字段习惯，untrusted 扩展到插件来源。提取 load_plugin_servers_sourced 并由执行合并和诊断共用，不复制 JSON 解析。允许同名项排在受限诊断项之前。
## Risks / Trade-offs
同名不同来源可有多行，必须依据 source、disabled、untrusted 阅读。禁用与未信任独立表达。项目已信任时仍不重复显示已启用的低优先级同名项，但禁用插件声明保留供诊断。
