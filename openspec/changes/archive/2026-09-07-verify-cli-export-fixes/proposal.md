## Why
剪贴板结果、会话相对路径、字符统计、原子导出和历史加载检查已完成模块回归，但工作树 CLI 尚未包含这些修改。需要更新可执行产物并验证命令入口。

## What Changes
构建 main CLI，检查 version/help/export help 并记录二进制摘要及磁盘状态。无行为契约变更，skip_specs=true。
