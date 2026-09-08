## Why
配置保存与 MCP 原子写入使用普通临时文件创建，随后才设置旧权限且忽略错误。新文件权限受 umask 影响，可能比私有配置所需更宽；写入失败残留由手工路径管理，异步保存 rename 失败也不清理。

## What Changes
复用已有 tempfile 依赖，为两条路径创建同目录独占临时文件；Unix 新配置默认私有，既有权限在写内容前恢复且错误传播。使用打开句柄写入与 persist，失败由 RAII 清理。

## Capabilities
### Modified Capabilities
- configuration-rules: 配置临时文件权限及生命周期。

## Impact
Shell 设置保存和 atomic_write_string。同步路径原先已有 rename 失败清理，保留并扩展到所有退出路径。不引入跨进程锁或 fsync 保证。
