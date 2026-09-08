## Why
HookRegistry 反序列化信任事件索引且未运行 HookSpec.validate；索引与内部 event 不一致时，dispatcher 会按错误事件执行。

## What Changes
恢复入口拒绝索引不一致和已有 validate 约束不合法的规格。

## Capabilities
### Modified Capabilities
- extension-runtime: registry 恢复验证事件身份与已有策略约束。

## Impact
仅 hooks registry 反序列化；workspace wire 现有错误传播保持。
