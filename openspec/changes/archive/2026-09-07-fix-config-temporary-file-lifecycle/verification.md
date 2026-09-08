## Reproduction
旧 config_writers_create_private_files 失败，实际模式的 group/other 位为 0o044，而非零。测试未修改进程 umask。

## Validation
- shell util::config::persist：62 passed。
- shell util::config::：231 passed（包含上述 62 项，不相加）。
- 新回归检查同步/异步新配置私有权限、既有 0600/0640 保留、正文写入前临时文件权限、成功后无残留、同步替换目录失败后原目录保持且无残留。
- NamedTempFile 管理所有失败路径生命周期；异步通过打开句柄写入并 flush 后 persist，不按路径重开。

## Limits
在 macOS 执行，未执行 Windows 验证；异步取消清理由句柄及 RAII 路径核对支持，未注入时序测试。未模拟磁盘写满、chmod 失败或断电，不声称 fsync 保证。既有 linker __eh_frame 警告未影响结果。真实用户配置未修改。
