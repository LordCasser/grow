## 1. Baseline and boundaries

- [x] 1.1 复核 rmcp 2.2 的结果字段、主 use_tool→截断→Shell→Timeline/ACP→Pager 链路、direct extension 和现有图片预算；记录与取消 change 的 `servers.rs` 写入协调。
- [x] 1.2 明确主路径 typed 投影/文本 JSON 去重/图片限额/direct 响应字节上限的实现常量和测试依据，不添加新的可变全局配置。

## 2. Implementation

- [x] 2.1 成功与业务错误共用 MCP 内容投影，保留结构化 JSON、可见资源描述、图片及 unsupported block 提示；按语义去重。
- [x] 2.2 将 typed 图片与 `MCPOutput` 的文本截断分离，在 Shell 结果入口经既有正规化和 follow-up 提交；确认默认 ACP raw_output/普通 prompt 不包含 raw base64，显式 opt-in 行为有界且有测试。
- [x] 2.3 direct `grow/mcp/call` 返回完整有界 `CallToolResult`，超限明确失败；不写入主模型 Timeline。

## 3. Verification

- [x] 3.1 单测 structured-only、摘要+相同/不同 JSON、业务错误、五类 block、无效/超预算图片、资源链接及 direct 序列化上限。
- [x] 3.2 端到端回归大图越过文本截断、图片正规化/省略、顺序、Timeline 恢复/portable 投影、TUI 显示和非 MCP 工具不变。
- [x] 3.3 更新开发者说明，在 `verification.md` 记录实际命令、退出码、上限和未验证限制。
- [x] 3.4 逐项核对 delta、执行全量 strict OpenSpec、归档本 change，再执行全量和 archived 校验；未验证项不勾选。
