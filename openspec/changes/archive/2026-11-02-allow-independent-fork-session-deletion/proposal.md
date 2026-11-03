## Why

普通 fork 的 summary.parent_session_id 是来源展示信息，当前单条删除把它当成 canonical parent/child 结果链，导致独立历史不可删除。另一方面只扫描其他 session 的 Seed，漏掉已提交 Spawn、child Seed 尚未写入时的 incoming 引用。

## What Changes

只按 canonical Spawn/Seed 的双向引用保护单条删除；普通 fork 的 summary 来源不单独授予恢复依赖。保留 writer lease、runtime handoff、quarantine 和真实结果链拒绝，不递归删除其他历史。本轮不更改显式删除的观察预算或 runtime/UI 删除事务，分别留在后续增量。

## Capabilities

Modified: `session-timeline`。

## Impact

JsonlStorageAdapter 单条删除资格及隔离存储回归；无格式、接口或复制逻辑变化。
