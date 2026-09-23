## Context and evidence

`mcp/src/servers.rs::McpErasedTool::run` 把 `CallToolResult` 拍平到 `MCPOutputDetails::{OkayOutput,Error}(String)`；`tools/src/util/mcp_truncate.rs` 对此 string 先截断，`shell/src/session/actor/tool/result.rs` 再从 prompt text 提取 data URI，正规化成图片 follow-up。当前文本图片上限来自 `base64_images`（每件编码 10 MiB、每结果最多 5 件）及 `image_normalize`；新 typed 路径不能绕开这些上限。`shell/src/extensions/mcp.rs::handle_call` 是不进入主模型 Timeline 的 direct ACP extension，现有 `{type,text}` 无法表达所有 rmcp block。`session-timeline` 和 `model-sampling` 已要求图片预算和工具往返投影，本 change 不另造模型协议。

## Decisions

图片的最小生产调用链（每个 `value` 箭头都只序列化脱敏后的正文，旁路的 `model_output` Image 负责还原）：

```text
McpErasedTool::run (MCPOutput + runtime images)
  → LocalRegistry::ErasedTool::execute (TypedToolOutput: value + model_output)
  → FinalizedToolset::call_raw (第 1 次 value 解码 + Image 还原)
  → InnerDispatchForToolset → use_tool::dispatch_local_mcp (第 2 次 value 解码 + Image 还原)
  → UseTool::run 文本截断 → LocalRegistry::ErasedTool::execute
  → FinalizedToolset::finalize_output (第 3 次 value 解码 + Image 还原)
  → SessionActor::handle_bridge_tool_success (Timeline tool result + image follow-up)
```

1. **MCP 边界保留结构和顺序。** 用 rmcp 的 `CallToolResult` 构造主路径中立的 typed 投影：text 按原序；`structuredContent` 用稳定 JSON 表达，在已有 text 与其 JSON 语义相等时不追加第二份；摘要与不同 JSON 同时保留。`isError` 只决定成功/失败状态，不删内容。text resource 与 resource link 附上类型、URI、MIME/描述等有界文本，不主动抓取远程资源；audio 或未知 block 给出类型明确的不可呈现提示，不能悄悄空结果。
2. **图片不经过文本截断。** image block 和 `image/*` embedded blob 作为独立的、有顺序的 typed image 存在工具运行结果中；每个 text/资源文本块的 inline data URI 在 MCP 投影边界即时抽取，按 block 原序与 typed image 合并，并共享一次五图预算。Shell 对 MCP 正文不再二次优先抽取图片，以免把末尾 text URI 排到较早的 typed image 前面。text/JSON 可由现有 `mcp_truncate` 截断并存文件，而 typed image 限额、MIME/base64/解码验证和现有 `image_normalize` 必须在资源受控的路径完成。`use_tool` 路径经内层 `ToolDyn`、`FinalizedToolset::call_raw`、`InnerDispatchForToolset` 与外层 `FinalizedToolset::finalize_output` 三轮 JSON value 往返；runtime-only 附件不序列化到 `ToolOutput` JSON，而是经现有 `TypedToolOutput.model_output` 的 Image block 穿过这些内部边界并还原到 MCP output。Shell 仍由既有工具结果结算入口写入 Timeline tool result，再发有序 image follow-up；成功、业务错误和混合内容都不因错误分支丢图片。拒绝的图片以明确、可见的占位/通知表示，不将半截 data URI 当普通文本。不得把 typed 图片原始 base64 放进 TUI `raw_output` 或普通 prompt text。
3. **保留 opt-in raw bytes 的既有界限。** `expose_image_base64` 已允许显式暴露原始字节供路径转发；实现需说明其与文本上限及保存文件的关系，不因 typed image 默认路径无意泄露或悄悄取消 opt-in。无论选项如何，视觉附件独立于文本截断和 opt-in 文本。
4. **direct wire 采用原生结果而非第二个拍平模型。** 将 `grow/mcp/call` 的响应序列化为完整 `CallToolResult`，保留五类 rmcp content block 的顺序、`structuredContent`、`isError` 和 `_meta`。扩展入口在序列化/发送前设定明确的响应字节上限；超限返回可分类错误，不输出被截断的假完整 JSON。该路径不追加 Timeline、不调用主模型 image projection。

## Risks / trade-offs

- rmcp 已在接收时完成 JSON/base64 字符串分配；本 change 的有界投影限制后续复制、显示和上下文，不宣称限制网络层完整响应大小。direct wire 的大小判断应尽早进行，避免额外放大。
- 图片 follow-up 是当前 Grow 的独立用户样式附件，必须在恢复、模型切换和 portable 投影中保持图片顺序，不把 MIME/base64 文本送入 token 截断器。
- structuredContent 可能很大，仍受现有文本 20 KB 默认预览和完整输出保存规则；不能以“去重”为名删除不等价的业务字段。
- 资源链接不主动读取，避免扩展成网络抓取或新的授权能力；audio 暂无模型投影时明确说明不可呈现。

## Verification and rollout

以 MCP 结果构造单测覆盖各 block、dedupe、错误和预算；以 `use_tool`→Shell 结果回归确认大图经过截断后仍成视觉附件且持久 Surface 可重放。对 direct extension 使用混合结果和超额响应验证 wire 字段顺序与失败边界。检查现有 TUI raw_output 不含默认 base64，更新开发者说明和验证记录后归档。
