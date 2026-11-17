## Why

从已结算子任务在分页 resume 后显示为 Running 的回归出发，检查同一类事实已持久化、执行 owner 已知，但 Pager 在恢复时遗漏当前状态或历史关联的问题。

## What Changes

本 change 交付代码路径、定向反例和独立 backlog 项。现有行为契约继续作为验收依据，因此使用 `skip_specs: true`。
