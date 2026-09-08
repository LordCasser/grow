## Approach
使用 locked、offline、无增量、无 debug info、两个构建 job 的现有配置。构建期间不修改 Rust 文件，检查 target 和剩余磁盘。版本与帮助在交互运行时初始化前返回，可用于无会话启动烟测。

## Limits
链接和帮助入口成功不替代真实交互会话、网络提供商或恢复已有用户会话的验证。
