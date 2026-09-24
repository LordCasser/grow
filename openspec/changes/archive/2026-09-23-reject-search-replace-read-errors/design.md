## Context

`handle_new_file_creation` 通过 `AsyncFileSystem::read_file` 判断缺失、空文件和非空文件。该接口已保留 `ComputerError::io_error_kind`；本地、ACP adapter 和 MockFs 均通过这条边界。现有 `Err(_) => None` 抹掉错误分类，使读取失败落到写入分支。

## Decision

仅在 `io_error_kind() == Some(NotFound)` 时设置“缺失”。其它错误返回现有 `SearchReplaceOutput::InvalidInput`，包含目标路径和原始错误。函数在返回前不调用 `write_file`，也不发送 FileWritten。选择现有结果类型，避免为这一个错误增加输出实体。

## Verification

以实现 `AsyncFileSystem` 的故障夹具返回 PermissionDenied 与未知 kind，记录 write 调用次数；正常 NotFound 和空文件路径使用现有回归。测试从工具执行入口调用，验证结果和文件不变。范围之外的 read→write TOCTOU 需要独立定义条件创建/版本提交能力，不能由本次错误分类测试证明。
