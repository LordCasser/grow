## Why
配置 disabled 将技能标为 enabled=false，但 agent skills 声明的预加载只匹配名称，依然读取并注入禁用技能正文，违背配置说明。

## What Changes
在既有名称解析选定技能后检查 enabled，禁用条目不进入预加载结果，不回退到低优先级同名技能。

## Capabilities
### Modified Capabilities
- configuration-rules: agent 技能预加载尊重禁用状态。

## Impact
仅 agent 预加载解析；不扩大到用户 slash 调用或修改 disable_model_invocation 的显式声明语义。
