## Evidence and boundary

真实输入 seq3031 的 350 个 wire messages 中 12 条 assistant 正文为占位句；seq3032 原始 SSE 返回同句，seq3035 将其作为真实 provider 回复接纳。此前 seq2995 是成功 description，相关 seq2978 assistant 正文原为空。故生成侧与展示侧均需处理。

## Model-facing projection

现有图片工具 shadow 继续移除完整 arguments、关联 assistant 正文和 native response boundaries，以保留源信息不可重新注入的安全边界；替代采用空正文和空 JSON 对象，不制造内部自然语言。Reasoning/BackendToolCall carrier 用现有空 assistant 槽保留 Surface 坐标，wire 派生阶段省略无正文/无调用的槽。

两条历史技术文本作为有限保留值，只有整个 assistant 正文精确相等时才清空；包含该文本的正常解释不删，User/ToolResult 不处理。清理发生在 RequestSegment 的 Items 构造、直接 Chat 压缩转换入口与 child background 归一化，不重写 Timeline。若当前 native span 的中性镜像含此类正文，沿既有 portable fallback 重建，不能修改签名/native 片段或留下第二条载体。此次不放宽关联 assistant 正文的既有整段源清理策略。

## TUI

AgentMessageBlock 是 normal/minimal/child 的共用入口。完整文本构造直接判断两条保留值；流式只暂存可能命中的消息起始前缀，长度不超过最长标记。分叉立即原样释放，完整标记在 finish 丢弃，未完成前缀在 finish 恢复。只精确匹配整条消息，不做子串或行过滤。子 Agent finalize 不一定清除 tracker 当前块；完整占位已隐藏后若晚到非空后缀，恢复原始全文，避免吞掉实际解释或出现显示/复制不一致。复制/搜索使用同一派生正文，raw canonical 记录及 response_projection digest/updates 不改变。

## Validation

先以新回归证明旧生成侧及 UI 行为不满足约束，再实现；覆盖三协议的调用/result 配对、成功 description/OCR/removal、原图证据与 replay 一致性、历史 marker 与真实解释/用户/工具文本、child 背景，以及所有字节切分位置和 deferred UI 通道。仅验证受影响路径，不执行/改写用户运行会话，不涉及并行审计或 MCP。
