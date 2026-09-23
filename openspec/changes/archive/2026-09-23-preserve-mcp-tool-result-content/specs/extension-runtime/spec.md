## ADDED Requirements

### Requirement: MCP tool results retain structured and typed content

Grow SHALL 对 MCP `CallToolResult` 的成功与 `isError` 结果保留 `structuredContent` 及按原序可解释的 content block，不把只有结构化内容的结果视为空。主模型路径 SHALL 将 text/结构化 JSON 作为受现有文本截断约束的正文，将 image block 和 image MIME 的 embedded resource 作为独立、受图片限额与正规化约束的附件；unsupported block SHALL 有可辨认的类型占位，资源链接不得被隐式抓取。direct `grow/mcp/call` SHALL 返回有界的完整原生结果，而非丢字段的 `{type,text}` 投影。

#### Scenario: Structured-only and nonduplicate JSON
- **WHEN** 工具只返回 `structuredContent`，或同时返回摘要 text 与不等价的结构化 JSON
- **THEN** 主模型结果包含完整结构化语义（超长时遵守现有预览/文件保存），摘要及不同 JSON 均可见；与结构化 JSON 语义相等的 text 不重复展示。

#### Scenario: Business error with nontext content
- **WHEN** `isError` 为 true 且含结构化内容、图片或资源信息
- **THEN** 工具仍标记失败，但上述内容遵守各自投影规则，不被仅保留 text 的错误分支丢弃。

#### Scenario: Image survives text truncation
- **WHEN** 前置正文超过 MCP 文本限额，后面跟有效 image block 或 image resource
- **THEN** 正文按既有规则截断/保存，图片不被切成半截 data URI，仍在图片限额内形成视觉附件并按原结果顺序交付。

#### Scenario: Invalid or excessive image and unsupported blocks
- **WHEN** 图片 MIME/base64/解码无效或超出单件、数量预算，或结果包含当前模型不能呈现的 audio/未知 block
- **THEN** 有明确的省略/不可呈现说明，不向模型发送损坏的图片或未受控的原始字节，不静默丢 block。

#### Scenario: Complete direct result
- **WHEN** `grow/mcp/call` 收到 text、image、audio、resource、resource link 混合结果及 `structuredContent`/`isError`/`_meta`
- **THEN** 有界响应保留原始 block 顺序、类型和字段；不会把 image 丢弃或 resource 伪装成 text。

#### Scenario: Direct response exceeds its budget
- **WHEN** 完整 direct 结果的编码大小超出明确响应上限
- **THEN** extension 返回可识别的超限错误，不发送不完整却声称成功的结果。
