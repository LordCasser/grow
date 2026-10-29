## Why
统一工具与 Thought 聚合的完整 Pager 回归出现 11 个 ACP initialize 超时，需要对照改动前后的代码判断归因。

## What Changes
只读追踪初始化路径，运行当前版本与 HEAD 基线的相同测试，记录复现条件、证据与结论。本审计不修改运行时、测试期限或行为契约，因此不新增 delta spec。
