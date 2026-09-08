## Decision
共享 config_temp_file(path) 负责父目录、目标权限读取、NamedTempFile 独占创建及 Unix 权限设置。异步保存从临时文件克隆打开句柄并转换为 tokio File，write_all 后完成 flush，再 persist；不按临时路径重新打开，避免取消时重建已清理路径。同步 writer 使用 Write::write_all 后 persist。所有错误直接传播，未成功 persist 的 NamedTempFile 自动清理。

## Scope
没有新增依赖。Unix 使用 tempfile 的私有初始模式，已有模式在写正文前恢复。Windows 使用 tempfile 默认创建语义，不声称实现 Unix mode。flush 仅确保异步写完成，不承诺断电持久性。
