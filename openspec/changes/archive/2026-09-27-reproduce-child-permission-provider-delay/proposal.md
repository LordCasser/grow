## Why

截图只证明子 Agent Auto 裁决到达总期限，不能区分主会话阻塞、Sideband 准备和模型响应慢。需要在真实 PermissionManager → 主 SessionActor → mock provider 路径上构造可控延迟，核实实际等待点。

## What Changes

- 增加一个不改变行为的集成回归：先证明子 Agent 请求在主会话独立发出模型调用，再让 provider 在旧首次尝试窗口后返回；另测 provider 一直不返回时的最终拒绝。
- 超时时记录分类请求所在阶段及排队/总耗时，方便把未来故障归因到通道排队、Sideband 准备、模型响应或结算；记录可证结论和截图无法单独证明的部分。

## Capabilities

不修改行为契约；`skip_specs: true`，现有 `tool-authorization` 的总期限和 fail-closed 场景已经覆盖预期行为。

## Impact

权限分类 worker 的诊断日志、集成测试和本 change 验证记录；不改变权限裁决。
