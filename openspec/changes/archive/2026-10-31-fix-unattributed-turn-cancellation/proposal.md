## Why

用户反馈压缩完成后显示“Turn cancelled by user”，但没有现场 session 或日志。代码确认 Pager 将所有 cancelled 终态都归因用户，实际 Shell 的内部准入关闭也可产生该终态；截图不能证明压缩触发了取消。

## What Changes

- 取消标记改为中性的“Turn cancelled”，不从 stop reason 推断用户操作。
- 用真实 sampler/turn-loop 夹具补充 Plan 执行阶段压缩后续跑验证，并核对已有异步、取消和失败回归。
- 记录验证边界：本次修正文案，不宣称修复未复现的自动取消原因。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `client-surfaces`：取消标记不虚构操作者。

## Impact

Pager 共用 session event renderer、现有 PTY 文案断言、Shell 压缩回归及开发者说明。不增加终态协议字段，不改取消、压缩或准入状态机。
