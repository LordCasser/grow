# Preserve final code-index removal events

## Why

WS-01的真实调用链是Shell watcher或Git refresh向IndexManagerHandle提交文件事件，codebase-graph actor归约后更新共享符号索引。当前CoalescedEvents把Created/Modified之后的Removed直接从map删掉，已索引文件因而可能一直保留definitions、references与file metadata，导航继续返回已删除文件。当前两项coalescer测试反而固定了这一错误空map预期。

## What Changes

- codebase-graph事件归约保留路径的最终Removed，即使该批次曾含Created、Modified或rename目的地。
- 保留现有Removed→Created/Modified替换行为、独立路径和共享快照隔离；删除未知路径沿既有幂等入口处理。
- 用真实IndexManagerHandle::send_events与同步snapshot查询验证最终删除及rename/重建对照，先在原实现取得有界失败证据。

## Impact

只涉及codebase-graph的归约及必要回归，归档时补client-surfaces的导航索引一致性契约和开发说明。不改变Shell事件协议、watcher、缓存文件格式或CodeNavigation准入，不新增初始存在性状态、事件队列或依赖。原始审计证据见archive/2026-11-05-audit-grow-architecture-across-versions/reviews/workspace-storage.md的WS-01；本变更按统一审计修复授权推进。
