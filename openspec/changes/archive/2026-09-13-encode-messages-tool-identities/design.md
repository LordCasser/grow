## Decisions
request_segments 只生成一次。先预留所有原生 ID 与短合法中性 ID；对其余中性 ID 使用现有 blake3 的稳定摘要生成短 ASCII 候选，并以请求内 reserved set 检查和消解碰撞。调用和结果查同一个映射，映射仅活在请求构造期间。

中性直通 ID 的本地编码预算定为 64 字节，仅允许 ASCII 字母、数字、下划线和连字符；这是 Grow 的输出约束，不宣称 provider 的最大长度。生成候选及碰撞后缀保持该预算。原生块不重写，包含签名 thinking；与原生调用配对的中性结果继续使用原生 ID。

## Risks / Trade-offs
摘要不能独自构成无碰撞保证，必须预留合法/native ID 并检查生成 ID。测试人工制造生成候选与现有 ID 重合。保持 portable 过滤和边界逻辑，不重新引入被隔离的歧义历史。


## 外部契约核对
核对 [Messages request 官方文档](https://platform.claude.com/docs/en/api/messages/create) 和 [官方 SDK 的 ToolUseBlockParam](https://github.com/anthropics/anthropic-sdk-python/blob/main/src/anthropic/types/tool_use_block_param.py)：保留调用/结果关联及原生 thinking 的原样续接。公开类型没有给出可据以断言的 client tool ID 最大长度；本 change 的 64 字节是本地输出预算，不将 server-tool ID 的专用 pattern 套到 client tool ID。未发起真实 provider 请求。
