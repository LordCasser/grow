## Why
LSP 执行端已在合并前排除未信任项目，但 inspect 仍只显示项目优先的同名定义并标记 untrusted。用户看不到保留下来的用户或插件来源，诊断结果容易被误读成该服务器配置完全不可用。
## What Changes
inspect 使用报告已解析的项目信任结果构造允许来源视图，并另列未信任项目定义。保留现有 source/untrusted 字段，同名允许来源排在被禁用项目项之前。
## Capabilities
### New Capabilities
无。
### Modified Capabilities
- `client-surfaces`: LSP 配置诊断展示允许来源及被禁用的项目定义。
## Impact
仅 Shell inspect 及其测试、说明与 backlog 状态。不启动 LSP，不改执行授权和插件启用策略。
