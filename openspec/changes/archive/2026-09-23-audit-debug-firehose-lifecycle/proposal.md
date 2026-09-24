# Debug firehose 生命周期审计

## Why

以可控测试记录 per-session firehose 的 worker/guard 数量、可观测日志字节增长、retention 触发与协作/非协作 writer 的删除边界，并收窄 backlog 中仍未解决的资源上限问题。

## What Changes

仅审计与测试现有行为，不改变运行时策略，因此不新增 delta spec。日志保留周期、总字节预算及 worker 上限仍需后续独立设计。
