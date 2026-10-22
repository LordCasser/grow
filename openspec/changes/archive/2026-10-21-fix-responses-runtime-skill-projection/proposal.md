## Why

真实会话 01a15c48-ab5b-7872-a263-15c28b8d8e69 的 Responses thinking 请求在用户加载技能后连续被 reasoning_text 400 拒绝。Runtime 用户技能投影生成没有模型 reasoning 的 function_call/output；已有 portable reasoning 恢复无法为这类宿主事实恢复并不存在的思考。

## What Changes

- Responses 将用户触发的 SkillLoad 直接编码为独立 user 上下文，完整保留现有结构化加载结果，不生成模拟模型调用或思考。
- 保持真实模型技能调用、工具批次排序、原始用户输入和 Timeline 不变。
- 覆盖初次、portable/已学习路由、native continuation 与工具批次边界。

## Capabilities

### Modified Capabilities
- `model-sampling`: 明确 Responses 用户技能事实的非模型调用投影。

## Impact

仅 sampling-types 的 Responses wire 转换与回归及开发说明。复用现有结果序列化、排序和预算估算路径；不加依赖、重试机制或持久化迁移。Chat/Messages 不在本次修复范围。
