## Why

主 Agent 和普通子 Agent 已共用 SessionActor 的请求投影、同步与异步压缩、工具结果预检查和 provider 超窗恢复。子 Agent 启动仍维护独立的配置解析和阈值快照，后续修改主 Agent 的策略容易遗漏这条入口。

## What Changes

子 Agent 用现有 Config 快照调用主 Agent 的压缩配置解析器，并在派生入口验证实际选择的模型阈值。此变更统一实现，现有阈值优先级、压缩策略与 Workflow 冻结路由契约保持。
