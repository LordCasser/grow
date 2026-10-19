## Why

v2.4.0 发布构建中的真实进程协调回归在多个平台失败。source 已持久化 `OutgoingStarted`，但尚未向 runtime 登记运行记录；此时并发 `get_inquiry` 把正在启动的询问误判为冷恢复遗留，返回 `SourceUnavailable`。

## What Changes

- 在 source durable admission 前登记现有 outgoing 记录，提交成功才派发；查询的持久化回退也与登记串行。
- 保留 durable ACK 前不派发、首次 payload 绑定、冷恢复不自动重跑和原有取消语义。
- 增加屏障控制的启动/查询回归、提交失败及取消检查，运行现有独立进程测试，不放宽其断言。
- 协调 CI 对相关代码直接推送 `main` 也执行，记录此次覆盖缺口与真实验证范围。

## Capabilities

### Modified Capabilities

- `local-coordination`：明确活 source 启动提交与并发查询的一致观察边界。

## Impact

修改 coordination runtime、SessionCoordinationBackend、限定回归和协调 workflow；更新开发说明。不增加持久化格式、状态存储、产品工具或依赖，不修改已发布版本的资产。
