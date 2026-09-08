## Why
技能 reset 与 config 方法把参数反序列化错误回退成默认 cwd，导致无效 reset 请求仍执行配置清空。它们虽未发现仓内 UI 调用，但通过 ACP 前缀路由实际可达，必须在操作前验证参数。

## What Changes
两处可选 cwd 参数改用已有 parse_params，类型错误返回 invalid_params；空对象仍表示默认 cwd。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能 reset/config 参数校验。

## Impact
shell 技能扩展；不改变有效 reset 的范围或权限。
