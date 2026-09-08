## Decision
复用 tempfile 依赖和本仓 local_drafts 的 persist 方式，但不使用配置专用 helper（其没有导出链接语义）。export_cmd 内部共享 write_export_file；小型 write_export_file_with 接收写入闭包，生产执行 write_all，测试注入部分写入错误。

先检查目标：现存 symlink canonicalize 后针对其目标提交；悬空链接返回错误，不替换链接。普通缺失路径允许创建父目录。只允许普通文件目标，拒绝目录及其他特殊节点。保留已有文件 Permissions，拒绝 readonly；新文件采用 NamedTempFile 默认私有权限。先写临时文件并 sync_all，再 persist 原子替换，任一前置错误保留旧文件。

## Limits
保证普通写入失败不截断旧目标，不宣称父目录掉电持久化或对抗恶意并发路径替换。原子替换更换 inode，不保留硬链接联动或平台扩展 ACL/所有者元数据。同步 I/O 的 UI 阻塞独立处理。

## Verification
注入部分写入后错误，核对旧内容及临时文件清理；验证正常替换、新文件、现有符号链接、悬空链接及目录拒绝。Unix 检查权限。
