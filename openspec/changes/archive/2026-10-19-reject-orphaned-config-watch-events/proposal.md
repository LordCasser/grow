## Why

候选 fb1beab8 的 Core run 39275706329 在 config_watch_late_directory_and_recreation 失败（shell 4284 passed, 1 failed, 7 ignored），尚未执行 tools suite。源码中取消注册与回调由同一锁串行化，但 debouncer 已排队的 config.toml 事件不检查项目是否仍注册，因此可在 unwatch 后发布旧事件。现有 .grow 补挂分支已有所有权检查。

## What Changes

项目配置事件在实际处理时检查仍存活的目录所有者，保留共享祖先；全局和显式额外配置目录是独立永久所有者，不因项目 unwatch 一并移除。提取已有逐路径处理为同一私有方法，生产 callback 和确定性迟到事件回归共用；保留真实目录重建用例并验证全局/额外配置继续更新。不改 debouncer 时间、不以增加 sleep 或跳过测试发布，不合入 MCP SDK/策略分支。
