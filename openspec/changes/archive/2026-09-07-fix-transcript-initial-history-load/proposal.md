## Why
首次恢复会话时，transcript 在 loading_replay 期间读取部分正文：普通模式立即输出部分 Markdown，minimal 固定不完整的 ID 列表。已存在的 export 加载保护没有覆盖这个入口，用户可能误以为历史丢失。

## What Changes
首次加载期间拒绝打开 transcript 并提示完成后重试；普通模式加载中同样拒绝。minimal 的重连窗口继续按已归档规则等待并自动重建。

## Impact
两个 transcript 生产入口，不改 copy、export 或历史回放协议。
