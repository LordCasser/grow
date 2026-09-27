## Why

子 Agent 的 Auto 精确调用会使用主会话模型作临时权限裁决。当前默认 30 秒被两次 provider 尝试平分，较慢但仍可在总期限内完成的首次尝试会被提前取消；通道排队及 Sideband 准备/结算又不受同一期限约束，造成不可预测的等待和截图所示超时失败。主会话 Auto 分类复用这条通道，受同一问题影响。

## What Changes

- 将主会话和子 Agent Auto 模型裁决的期限从提交到分类通道时起算，覆盖排队、准备、模型调用和结算。
- 默认总期限调整为 60 秒；首次模型尝试使用总剩余时间的主要部分，失败后的第二次尝试只使用剩余期限。
- 期限到达或调用方消失后不采纳迟到的允许结果；子 Agent 仅失败当前工具，主会话维持既有 Auto fallback 语义。
- 在开发者说明中区分 Auto 模型裁决期限与人工权限提示期限，并记录尚未处理的权限 actor 排队问题。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `tool-authorization`: Auto 模型裁决的端到端期限、尝试预算与超时后的安全结果。

## Impact

`workspace` 分类通道请求、`shell` 权限 Sideband worker、默认配置及权限说明；不改变子 Agent 的独立 permission mode、能力上限、显式 deny、人工提示或一次性 permit。
