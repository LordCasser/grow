## Approach
沿用 locked/offline、无增量、无 debug info、两个 job；构建期间不修改 Rust。帮助与版本在会话初始化前返回。

## Limits
烟测不替代真实交互、提供商与历史会话恢复验证。
