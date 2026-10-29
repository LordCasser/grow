# 对抗审查

- HTTP 展示文案 280 字符截断可能丢失要求；SSE 路径无同样截断。typed 分类须在展示前产生，不能因路径不同产生不同结论。
- 字段精确匹配，reason_text 不自动视为 reasoning_text；两个 reasoning 字段或 param/message 冲突不得选择一个。
- 不支持字段、字段值非法、泛化 missing、非400、HTML/非JSON 都不能误启用。明确缺失参数与已知 pass-back 语义是恢复依据。
- DTO 往返丢弃 in-process source 后仍保留 typed requirement，不能退回截短文字分类。
- Responses 普通回答有文本才绑定 reasoning；空 assistant、孤立 reasoning、User/System/ToolResult 边界及 native span 不生成幽灵请求项。
- History 无 reasoning 不能虚构；Chat 空字符串是既有编码，Responses 空 item 目标接受性未经证明，不先实现。
- 当前 VisibleReasoningItem 只保留 text，可见摘要/full content 合并；此次回放不宣称恢复原生完整思考，真实接受性必须验证。
- route 切换清已学习状态，native reset 保留同route要求；不按模型名猜，不跨route污染。A→B→A 不复活旧native。
- 既有工具调用和结果保持配对；转换历史不触发执行。真实夹具对计数/随机事实检验续跑与无重复执行。
- 重复拒绝、不匹配backend、无实际wire变化或预算耗尽都不得无限重试；不自动切换模型/降低thinking/删历史。
- 通用 native 400 reset 仍是既有行为；本阶段只改可证实 reasoning 恢复，进一步分类独立处理。

## 独立复核结果

Luna 只读审查发现 code=missing_required_parameter 与正文否定 reasoning 缺失时仍可误分类。修复后 generic Field required 按完整 token 序列判断，标准 Missing required parameter 句与实际 param 路径精确比对；not missing、not required、not supported 冲突反例保留。37 项 error 测试通过，独立复核确认 typed HTTP→DTO→rich error 往返不依赖展示文案，未发现新的可复现问题。

## 实施后补查

- named SSE 的 request_id 属于诊断，不可混入协议分类；已把原始 message 与展示字符串分开，增加另一 backend 字段出现在 request_id 的反例。非字符串 param 只阻止 reasoning 分类，不丢失错误正文。
- 验证提取器原先对空/缺阶段记录的 all() 会误报全部执行一次，现精确比较阶段 1–9 的计数；并按 ACP toolCallId 核对 completed 状态及工具输出 token。
- 首轮 Messages 403 与后继模型补做失败请求均保留为失败；后继模型能继续，不等于原目标路由成功。真实端点均未触发 reasoning 400，不能以切换成功替代该恢复路径的真实接受性证据。
- 原始 reasoning、provider body、诊断和凭据不归档；保留请求摘要、配对计数、业务检查与哈希。
