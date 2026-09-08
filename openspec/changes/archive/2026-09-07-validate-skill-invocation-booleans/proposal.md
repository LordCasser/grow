## Why
技能调用布尔开关把所有非 true 值当成 false，disable-model-invocation 的错误配置会因此开放调用。需要区分显式合法 false 与错误类型。

## What Changes
两个调用开关严格接受布尔值或 true/false 字符串；其他显式值拒绝加载，缺省默认不变。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能调用开关类型。

## Impact
tools frontmatter；不修改权限系统或已加载技能调用机制。
