## Why
交互式 /export 的相对路径补全以会话 cwd 为基准，但实际 std::fs::write 以进程 cwd 为基准。两者不一致时文件写入用户未选择的位置。

## What Changes
TUI 导出在展开 ~ 后，将相对路径锚定到活动会话 cwd。绝对路径保持原样。

## Capabilities
### Modified Capabilities
- client-surfaces: 会话导出目录基准。

## Impact
仅 Pager dispatch_export_conversation 的文件路径解析。不改变 CLI export 的进程 cwd 语义或文件覆盖策略。
