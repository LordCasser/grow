## Design
在 input_log 模块增加小型写入函数，接收日志目录、时间和已序列化 JSON。Builder 使用 input-debug-<timestamp>- 前缀及 .json 后缀，Unix 默认 0600，write_all + sync_all 后 keep。dispatcher 保留序列化与反馈，改用返回的真实路径。测试私有目录内连续相同时间导出及注入部分写入失败，不修改 GROW_HOME。

## Boundaries
输出仍同步，最多200条记录，序列化/慢文件系统响应性后续测量。不承诺目录 fsync、断电恢复或自动历史清理。
