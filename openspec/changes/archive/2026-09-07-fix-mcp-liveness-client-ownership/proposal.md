# Why
健康监听任务持有 McpClient 的强引用；配置 diff 从 owned_clients 移除连接后，监听任务仍拥有连接及其 transport。健康连接不会触发关闭事件，可能持续存活而无法释放。

# What Changes
监听任务只保存 Weak，每次 tick 升级为临时 Arc；连接消失时清理自身槽位并退出。

# Impact
只调整 watcher 客户端所有权，不改变事件身份、频率和恢复策略。
