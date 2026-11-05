## Why

MCP-D1：extension-runtime旧Late tool errors条款仍写替代服务执行一次重试，与同规范现有Uncertain MCP tool calls are never automatically replayed及当前实现冲突。副作用可能已发生，不能为满足旧文字恢复自动重发。

## What Changes

将旧错误场景明确为保留replacement服务并返回原调用失败，不新增握手或tools/call。这是现有已归档no-replay契约的一致性修正，不改代码或工具行为；采用真实MODIFIED delta消除契约内矛盾，不编造产品新能力。

## Capabilities

### Modified Capabilities

- extension-runtime：Late tool errors的原服务绑定与零自动重放保持一致。

## Impact

仅规范和开发说明，无Rust代码、依赖或接口变化；不运行provider、不重跑已经存在且未变化的实现测试。验证依据是生产发送链、现有定向回归断言及strict/archive校验，不能报为本轮MCP运行通过。
