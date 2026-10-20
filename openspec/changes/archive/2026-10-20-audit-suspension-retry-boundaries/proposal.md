## Why

用户要求深入分析退出挂起失败的同类问题。此前 TaskCompleted source version 修正只覆盖类型不匹配，尚未验证正常通知与挂起通知的正文一致性以及挂起失败后的等待状态。

## What Changes

- 审计生产通知构造、Timeline 接纳、挂起重试及人工等待恢复。
- 保存可复现证据、已排除路径与测试覆盖限制。
- 本 change 仅保存审计资料，不修改产品行为或契约；`skip_specs: true`。临时测试接线运行后移除，复现源留在 change 中。

## Capabilities

无契约变更。

## Impact

审计范围为 shell SessionActor、通知桥接、terminal 留存结算、chat-state 和退出/恢复入口。真实用户会话不参与故障注入。
