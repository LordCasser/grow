## Why

Grow 的 MCP server 已收到 rmcp 2.2 `CallToolResult`，但主工具路径丢弃 `structuredContent`、audio、resource link，业务错误只保留 text。image/image resource 被先转成 data URI 文本；`use_tool` 默认先截断到 20 KB，Shell 随后才提取图片，因此大图会被截坏或丢失。独立的 `grow/mcp/call` direct 路径还额外丢 image、把 resource 压成 text。上游 grok-build 只解决结构化文本追加，未解决 Grow 的截断顺序与多模态路径。

## What Changes

- 在 MCP 工具边界将 text、structuredContent、image/resource 及不支持的 block 做一次明确、有界投影；成功与业务错误均保留结构化信息，语义重复的 JSON 不二次展示。
- 主模型路径让 text/structured JSON 继续走现有截断与文件保存，图片以独立 typed payload 越过文本截断，经现有图片校验/正规化后进入有序的多模态 follow-up；不可呈现内容显示明确占位而非静默丢弃。
- direct `grow/mcp/call` 保留 rmcp `CallToolResult` 的原始 content block 顺序、`structuredContent`、`isError` 和元数据，不再压成 `{type,text}`；对序列化响应设置明确的体积上限。
- 覆盖结构化-only、业务错误、长文本+大图、无效/超预算图片、混合 block 和 direct 响应过大。

## Capabilities

### Modified Capabilities

- `extension-runtime`: MCP 结果的接纳与两类消费者投影不得静默丢失已支持的结构化和多模态内容。
- `session-timeline`: 主工具结果的图片附件在文本截断之后仍按原顺序进入可恢复的模型 Surface，不将 base64 文本伪装成模型可读图片。

## Impact

- 入口：`crates/codegen/mcp/src/servers.rs`、`crates/codegen/tools/src/types/output.rs` 与 `util/mcp_truncate.rs`、`crates/codegen/shell/src/session/actor/tool/result.rs`、`shell/src/extensions/mcp.rs`、ACP 转换/展示测试。
- 不更改 MCP 授权、transport 初始化、调用重试、非 MCP 工具输出、模型图片预算/正规化的既有责任。`/transcript` 与客户端显示只投影元数据/占位，不把原始 base64 倒灌到文本。
- 与 `cancel-abandoned-mcp-tool-requests` 共用 `servers.rs`，先串行协调这两个 change 的实现与验证；保留并行工作树的其他改动。
