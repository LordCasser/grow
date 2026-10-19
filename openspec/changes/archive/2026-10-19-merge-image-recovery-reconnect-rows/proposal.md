# 合并重连中的图片恢复行

## Why
cursor 命中的 session reload 保留旧 transcript 并附加重连期间的 live tail。图片恢复事件在 fresh tail 中重新创建行，导致同一 operation 在最终 scrollback 中出现两行，旧 Started 行还可能在前台 prompt 活跃时继续显示运行。

## What Changes
cursor tail 合并按 operation identity 更新既有图片恢复行，保留原 EntryId 和终态语义。

## Non-goals
不改变图片恢复后端、完整 transcript replay、其他 scrollback 行的合并策略。
