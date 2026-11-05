# Decode Responses function-call starts without arguments

## Why

会话 `01a1ac9e-561d-7963-ac01-2239a244934c` 的两次已结束请求在 Responses SSE 解码处报告 `missing field arguments`。父读取原始 response evidence：`response.output_item.added` 的 function_call 有 name、call_id、id 和 `status: in_progress`，没有 arguments。锁定 async-openai 的 FunctionToolCall 同时用于初始项与最终项，要求 arguments 为 String，因此流在后续参数分片到达前终止。同会话另一次带 `arguments: ""` 的调用成功完成，构成区分证据。

这是已观察路由的初始项编码差异。OpenAI 官方示例会在初始项给出空 arguments，本变更不宣称省略字段符合官方 schema。当前严格拒收已完成坏参数的契约保持。

## What Changes

在 Sampler Responses wire 解码 owner 内，仅为 `response.output_item.added` 中显式 in_progress 的 function_call 补缺省空 arguments 前缀，继续接收参数 delta、done 和最终快照。保留原始 evidence；不补 null/非字符串、不修复 done/terminal、不修改通用 serialization 重试与 Goal 暂停规则。

## Impact

只涉及 Sampler 的 Responses 解码及此行为的回归，沿用 SDK 和 L2 的身份、前缀、终态及 JSON 校验。没有公共接口、依赖或持久格式变化。此项与 LSP 修复分别验收，最终 2.6.1 候选包含两项；不操作源会话、不向原远端服务重放请求、不重启其 Goal。
