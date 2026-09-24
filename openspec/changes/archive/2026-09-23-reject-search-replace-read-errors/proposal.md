## Why

`search_replace` 的空 `old_string` 路径用于创建文件或更新空文件。当前读取返回任何错误都会被解释为文件不存在，后续无条件写入；权限、I/O 或远端文件系统错误可能因此覆盖无法核对的既有文件。

## What Changes

- 只有明确的 NotFound 才进入新文件创建路径；其它读取错误返回失败且不尝试写入或发布 FileWritten。
- 保留已存在空文件的更新、非空文件拒绝和真正缺失路径的现有行为。
- 跨进程创建/替换的版本提交仍属于独立债务，本 change 不把 read→write 宣称为 CAS。

## Capabilities

### Modified Capabilities

- `tool-authorization`：空匹配编辑的读取错误不能隐式授予创建资格。

## Impact

影响 `search_replace` 及共用核心的 concise 变体；更新开发者入口说明与 backlog 状态。
