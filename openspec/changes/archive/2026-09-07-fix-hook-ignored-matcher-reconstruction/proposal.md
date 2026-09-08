## Why
registry 重建 matcher 未遵从 EventTraits.matcher，覆盖 parser 对 Ignored 事件的 None，非法模式导致这些事件错误跳过。

## What Changes
重建时优先遵从事件策略，Ignored 保留配置但清除缓存。

## Capabilities
### Modified Capabilities
- extension-runtime: 匹配器重建遵从事件策略。

## Impact
修正上一轮 registry 重建回归，不改变 Tested 事件 fail-closed 行为。
