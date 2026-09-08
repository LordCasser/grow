## Why
最近两个已验证修复尚未纳入 CLI 产物，需要验证实际组合根构建和入口启动。

## What Changes
构建 main 当前工作树 grow，执行只读版本和帮助入口冒烟，记录二进制哈希、尺寸和验证边界。不改变运行时契约，故跳过 delta specs。

## Impact
覆盖 background-transcript-file-writes 与 fix-private-pager-transcripts 的组合构建；不替换用户已安装二进制。

## Audit observation
构建等待期间核对外部分页器调用：Command::status 的错误和退出码均被忽略。登记 backlog，留待独立行为修复；本项不混入源码改动。
