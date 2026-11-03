## Why

显式删除复用只读/TTL 的 16 MiB、50k events、4096 references 预算，且按每个 sampling chunk 的 64 KiB 上限预留，导致正常长历史甚至很小但累积 chunk 较多的独立 fork 无法删除。删除不应依赖 provider body、prompt blob、展示 notice 等执行/展示内容完整可恢复。

## What Changes

- 显式删除读取有原 per-record 校验的 canonical journal，验证 Timeline、展示结构绑定与 receipt owner，并按其 Spawn/Seed 与 runtime lifecycle 决定资格。
- 目标读一次并复用到双向引用核对；关联扫描也按 canonical facts 验证，不使用维护的 aggregate 预算或 directory discovery cap。
- 不为删除读取 provider/展示 artifact，不扩大全局阈值；普通恢复、只读观察、TTL 和真实 cross-session/runtime 护栏保持各自验证。

## Capabilities

Modified: `session-timeline`。

## Impact

JsonlStorageAdapter 显式删除验证路径与回归；无持久化格式或协议扩张。不处理 runtime/UI teardown 顺序或挂起放弃语义。
