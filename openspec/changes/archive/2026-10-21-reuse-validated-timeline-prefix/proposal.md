## Why

冷恢复已完整读取并 fold Timeline，第一次 writer append 仍因 prefix 未初始化再次读取、解码、fold。现有 adapter 从 writer load 延续到 persistence actor，已经具备 lease、LedgerPrefix 和失效回退机制，无需新增证明服务。

## What Changes

- 初次 writer load 对同一固定完整前缀同时解码、校验并计算现有 BLAKE3 prefix/hasher。
- 全部恢复校验成功后，把结果写入同一 adapter 的现有 `timeline_prefixes`，供首次 append 复用；不经公共 RPC 传递。
- 保留 append 锁下的身份/stamp、长度、hash 回退、tail seq/exact payload 和 file/directory sync。

## Impact

主要影响 Shell JSONL writer load、prefix 初始化与直接调用方；不改磁盘格式或恢复契约，因此 skip_specs。推荐在 codec 优化之后、删除 updates 之前落地并独立测量。

不扩展到只读观察、resident writer 接管、跨 adapter/跨 lease 缓存，不跳过事件或证据校验，不改 TUI。当前仅提案。
