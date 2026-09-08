## Why
CLI 和 TUI 会话导出使用直接 write，会先截断已有文件，后续写入失败可能丢失旧导出。需要将两入口的文件提交统一为先完成新内容、再替换目标。

## What Changes
共用导出专用写入函数，采用同目录 NamedTempFile、完整写入及 sync_all 后 persist。失败清理临时文件；现存符号链接解析到目标，保留链接本身。Unix 保留已有权限，新文件使用私有临时文件默认权限。

## Capabilities
### Modified Capabilities
- client-surfaces: 导出文件原子提交。

## Impact
CLI/TUI 文件分支。路径锚定规则保持现状；剪贴板/stdout 不变。不改通用配置写入框架。
