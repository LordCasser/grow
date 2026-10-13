## Why

审阅新增 MCP 多轮交互、保留计划及上游评审记录，核对 Grow 现有 owner、持久化和调度边界，补全会改变实现方向的遗漏场景。

## What Changes

- 核对当前规范、工作树源码和 MCP 官方协议，记录静态证据与架构判断。
- 在两个尚未实现的行为 change 内补充 design、delta、tasks 和 acceptance，保持单一会话/工具 owner 和最小闭环。
- 不改产品代码、已归档主规范或上游跟踪范围；原行为 change 继续 active。

## Impact

本 change 仅审计和完善未来提案，不改变当前运行契约，因此 `skip_specs: true`，不建立重复 delta。新增行为仍由各自原 change 承载，文档校验不能代表产品验收通过。
