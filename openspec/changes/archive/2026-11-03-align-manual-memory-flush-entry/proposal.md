# 统一手动 flush 的忙闲准入

## Why

Pager `/flush` 在 foreground 忙时被通用 slash gate 拒绝，而 ACP `grow/memory/flush` 已允许独立 Sideband 执行。让显式 `/flush` 也使用现有会话活动 owner 和 flush 原子锁，在 foreground 运行期间发起独立 flush；不把命令写成模型输入。

## What Changes

同时把 btw、flush 和手动 compact 的 session lookup 收敛到已有 session/load 屏障，修复 reconnect/load 并发时错误的 session not found；关闭 command channel 则明确返回接纳失败。
