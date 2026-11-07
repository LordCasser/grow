# Separate MCP catalog names from provider function names

## Why

2.6.2第四项上游候选已核实为两个不同边界。MCP `into_registration`目前对完整`server__tool`执行provider的64字符正则，导致较长但无歧义的目录身份在search/use_tool之前被丢弃。普通Shell采样用builtins-only，不直接暴露MCP目录；但公开`Agent::tool_definitions`承诺供采样API使用，放宽目录后仍须保直接provider名约束。详细官方来源与既有消费者核对见[固定来源评审](../complete-nondeferred-backlog/upstream-2-6-2-review.md#name-boundary-evidence)。

## What changes

接纳候选中的Grow qualified目录上限256；保现字符集、首字符及唯一分隔规则，原identity通过search/schema/use_tool往返。先使Agent公开采样投影只返回现64字符跨provider共同格式内的名称，再放宽MCP registration长度。完整ToolBridge目录保持，不截断、不alias、不新建catalog。256是Grow聚合目录政策，不冒充官方MCP原始tool name限制。

## Impact

Agent projection和MCP registration各负责一个可独立验收结果，按依赖顺序交付。Agent不增加MCP高层依赖或新公共validator；MCP现validator只服务目录语义，不再将目录等同provider函数。Shell既有完整snapshot/search/use_tool、权限和app-only可见性规则继续消费同一个目录；用真实注册到内部调用的链路验收，不能只改正则后计完成。
