## ADDED Requirements

### Requirement: Clipboard AppleScript paths are data arguments
macOS 剪贴板图片脚本 SHALL 通过 osascript 参数接收路径，不将路径插入 AppleScript 源码。

#### Scenario: Special characters in a path
- **WHEN** 图片或临时目录路径包含空格、双引号、反斜杠、换行或 Unicode
- **THEN** 路径作为一个参数保留，字符不改变脚本结构。

#### Scenario: Read and write image scripts
- **WHEN** 调用图片读取、附件读取或图片写入脚本
- **THEN** 统一使用参数边界传递路径，保持既有类型优先和临时文件生命周期。
