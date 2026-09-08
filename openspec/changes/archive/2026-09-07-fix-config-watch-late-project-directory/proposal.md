## Why
ConfigFileWatcher 启动时 .grow 不存在会吞掉 watch ENOENT；父目录虽被监听但 callback 只处理 config.toml，后来创建 .grow 不触发补挂。watch_path 对同cwd直接去重也不重试。

## What Changes
让目录创建/重建维护 watch 与配置重读，保持非递归范围，不依赖 MCP 内容变更通知补挂。

## Capabilities
### Modified Capabilities
- configuration-rules: 项目配置目录晚创建及重建热加载。

## Impact
ConfigFileWatcher 与 start_config_reload 的所有权/维护接点；需同时覆盖 leader 和 in-process pager，不能仅修 agent/app。
