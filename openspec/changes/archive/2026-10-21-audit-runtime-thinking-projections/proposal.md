## Why

用户要求分析 Responses 用户技能修复后是否仍存在同类问题。限定审计宿主事实被投影成模型调用、thinking 字段覆盖与 native/portable 工具批次边界。

## What Changes

只保存审计证据、可复现离线探针及独立 backlog。不修改产品实现或行为契约；skip_specs=true，不虚构 delta。

## Impact

复用 sampling-types 公共转换 API 运行离线结构探针。探针保存在本 change 的 harness，不纳入产品回归，不调用真实 provider，不读取更多用户会话正文。
