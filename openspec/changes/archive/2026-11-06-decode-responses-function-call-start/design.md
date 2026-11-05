# Responses initial function-call decoding boundary

## Evidence and owner

父从指定会话 Timeline 中的 sampling_evidence/response 引用拼接原始 SSE，仅检查事件结构。两次失败请求 `9e5097d7-5eaf-440b-9d74-25adc4705faf` 和 `0d140b6f-c7a3-4170-92d6-b57e70c768e0` 均在没有 arguments 的 in_progress function_call added 事件后失败；`c4353ef2-62e6-47e3-a4ad-79b8a3a48c09` 的初始项包含空字符串，并完成参数分片、done 与 response.completed。原会话持续写入期间的后来未结束请求不计为失败或通过。

`sampler::client::decode_response_event` 把原始事件交给锁定 SDK；`sampler::stream::responses` 随后以 output_index 和 item/call 身份积累参数，比较 done 与最终快照，最终由 response_to_conversation_items 验证可执行参数。SDK 必填字段造成的初始事件解码差异归 client 所有；不能通过全局修改 SDK FunctionToolCall 默认值让完成项也静默缺失参数。

## Decision and invariants

在现 wire 解码规范化边界处理一个明确形状：顶层 type 为 response.output_item.added，item.type 为 function_call，item.status 为 in_progress，且 arguments 键不存在；只在供 typed decode 的副本中补空字符串。已有非空前缀原样保留。null、数值、对象不是缺省，仍报错；done、terminal、其他状态/类型以及缺少 name/call_id 等身份字段仍按既有严格解码失败。未知顶层扩展事件规则、usage 明细规范化与原 evidence 不变。

不在 L2 放宽前缀或最终参数校验，不把缺失 final arguments 合成 `{}`，不把序列化错误改成自动重试。局部 helper 或现 helper 合并由 coder 选择，不增加公共抽象。现有 terminal snapshot、identity、completed invalid JSON 恢复与预算控制提供后续保证。

验收反例已证实：先解为 Value 再编码会吞掉重复 name/call_id/sequence_number，甚至 item 后面的重复字段；原 SDK 的拒收被初始候选改成成功。仅依赖原错误为 missing arguments 也不能保留尾部重复字段。因此此精确规范化入口先以两个局部 Deserialize 结构检查 event/function-call 的 SDK 已知字段唯一性，值用 IgnoredAny 且保留字段 presence，缺省不复制 SDK 必填或类型规则；通过后才补空前缀，再由 SDK 验证类型/身份。包含 null-first 重复的控制，不新建通用 JSON parser、不改变 usage 规范化路径。

## Coder task and review

单一功能的写范围为 sampler client wire 解码及必要测试，优先实际 HTTP/SSE → client → L2 验证可完成调用，不能只测 normalize helper。Tier 2：先以最小原形状重现 red，再 green；覆盖省略初始参数后合法分片/终态、保留非空初始前缀、缺失 completed/done 参数和 null/错误类型仍失败、身份冲突或 EOF 无完成不能产出 Completed，以及相邻 usage/unknown-event 回归。采用无副作用合成工具；不执行或重放原会话工具。

父核实际 diff 与 red/green、最小错误事件和调用链。不新增独立 review（沿用用户停止新审查边界）。初始 Responses 实现采用独立 worktree 和构建目录，避免改变 LSP 正在验证的源码；LSP 交付后父仅整合 client 补丁，后续在主工作区独占窗口完成收尾与验证，不合入隔离快照的其他差异。验证后同步开发说明、strict 校验并归档。最终候选默认/发行编译及官方精确提交门禁覆盖两项集成。
