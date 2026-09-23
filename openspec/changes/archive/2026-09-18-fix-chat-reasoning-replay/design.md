## Context

`ReplaceSamplingRoute` 正确撤销 native continuation。`request_segments` 对旧历史保留完整工具往返，但默认删除 reasoning。现有 Responses 专用 bool 从 SamplingError 经 DTO、SessionActor、ChatState ACK 到投影，只覆盖 `reasoning_text`。Chat 的 `conversation_to_chat_messages` 则无条件删除 Reasoning sibling。

## Goals / Non-Goals

目标：让明确要求 `reasoning_content` 的当前路由重建合法 assistant 历史，保留可见事实、工具配对和原有恢复预算。非目标：全局开启 reasoning、按供应商名称猜能力、恢复 opaque native 状态或改变历史存储。

## Decisions

1. 复用 `ApiBackend` 的可选值替代 Responses 专用状态。错误仅在 400、thinking mode 与 pass-back 语义齐全且字段无歧义时给出目标 backend。ChatState 核对当前 backend，防止跨协议启用。
2. 状态仍由 ContinuationLane 持有，同路由 reset 保留、replace 清除。请求投影携带 backend，wire 转换仅在目标一致时启用；不增加第二条恢复链。
3. Chat portable projection 保留紧邻 assistant 的已有 visible reasoning（包括普通正文），按顺序合并为 `reasoning_content`；没有文本时显式编码空字符串。User/System/ToolResult 等边界清空待绑定 reasoning，不能把前一响应的思考借给后一条。已有 native reasoning 保持原样，缺字段时仅补空字符串。Responses 继续只保留完整工具往返前的 reasoning，Messages 与未学习路由仍删除 portable reasoning。
4. Chat 有可输出 assistant 或缺 reasoning 字段的 native assistant 时可确认变化；Responses 仍要求完整工具往返前有非空 reasoning。重复启用返回 false，Shell 不重新开启恢复；重建继续消耗原 logical sampling 预算。token 估算与 wire 使用同一 portable 规则。

## Risks / Trade-offs

- 首次拒绝消耗一次 attempt；明确协议错误触发避免对未声明要求的路由改变历史请求。
- 空字符串仅表达历史没有可回传文本，不伪造思考。离线测试可证明 wire/恢复边界，不能代替真实代理验收。
- 现有工作树有独立 recovery-stop 修复；本次不覆盖其文件改动，开发说明仅追加相关段落。
