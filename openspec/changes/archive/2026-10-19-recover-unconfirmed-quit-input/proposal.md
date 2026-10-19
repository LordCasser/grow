# Proposal

## Why

用户第二个会话 01a15323-b21a-7403-906e-72048e8b7594 在退出未完成后仍能操作，但 Ctrl+D 不再响应。当前输入层把 Pending 与 Unconfirmed 都吞掉；提示又可能在 Welcome 或窄窗口不可见。

## What Changes

修正未确认后的键盘重试和提示可见性，normal/minimal 共用退出状态绘制，补齐实际页面及输入到异步结果的回归。不改 Ctrl+C，不将重试转为强退，不混入 MCP。
