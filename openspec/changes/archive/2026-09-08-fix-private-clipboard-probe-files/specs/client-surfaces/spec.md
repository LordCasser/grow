## ADDED Requirements

### Requirement: Clipboard image fallback owns isolated private files
macOS AppleScript 图片回退 SHALL 为每次调用分配独立私有临时目录，并持有到脚本执行和图片读取完成。

#### Scenario: Concurrent fallback requests
- **WHEN** 两个调用分别读取图片
- **THEN** 使用不同目录与文件路径，不能覆盖或清理另一调用的产物；Unix 目录权限为 0700。

#### Scenario: Fallback completes or fails
- **WHEN** 调用正常返回、无图片、读取失败或脚本返回错误
- **THEN** 释放该调用的目录所有者并尝试清理全部残留文件，不依赖仅成功分支删除选中文件。
