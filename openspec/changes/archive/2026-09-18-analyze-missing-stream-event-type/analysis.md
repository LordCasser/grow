# 会话 missing type 根因与恢复边界

结论：这次确实需要更新错误解析和供应商错误码分类，但不应把 `Serialization` 全部设为可重试，也不需要重写共享 retry 调度。上游已经报告输出内容检查拒绝；Grow 把这条拒绝误报为事件反序列化错误。修复后的这次请求仍应停止自动重试，只是向用户准确说明原因。

## 真实请求与失败链

会话目录：`~/.grow/sessions/%2FUsers%2Flordcasser%2FDocuments%2FCelaeno/01a0b3af-3159-73c0-9ae8-f1220fa9d6c7/`。模型选择为 `qwen/deepseek-v4.1-flash`；实际消息模型为 `deepseek-v4.1-flash`，backend 为 Messages，路由为 `https://token-plan.cn-beijing.maas.aliyuncs.com/apps/anthropic/v1/messages`。这不是前一次 Chat reasoning_content 回传问题。

| 证据 | 事实 |
| --- | --- |
| `timeline.jsonl:762` / seq 761 | request `6f9b8fde-6fd2-4cd1-965a-a4672a22d480` 开始，Step 19，24 个工具定义 |
| `timeline.jsonl:763` / seq 762 | Messages 路由、attempt 1 |
| `timeline.jsonl:764` / seq 763 | HTTP 200；流式输出已经出现；无 provider terminal；retractable；剩余 14 次；恢复预算允许 |
| `timeline.jsonl:765` / seq 764 | attempt 用量结算为 unknown/null，未按零消费处理 |
| `timeline.jsonl:766` / seq 765 | `Fatal(Serialization(...missing field type...column 138))`；本地分类阻止重试，配置总上限 15 未耗尽 |
| `timeline.jsonl:767–769` | request、Step、Turn 以 error 结束 |

响应原件：`artifacts/sampling/6022d665108a04014ce68cfadefb0dc3272c5f8249d51b415aaec1ba06b95ea0.bin`，58,485 bytes。末尾原始 SSE 第 1243–1244 行是以下错误；为避免复制远端 request id，展示使用占位符：

```text
event:error
data:{"request_id":"<redacted>","code":"InvalidParameter","message":"Output data may contain inappropriate content."}
```

原 JSON 恰好 138 bytes，顶层仅 `request_id/code/message`。此前有 message_start、两个 tool_use block 和 409 个 content_block_delta，之后没有 message_delta/message_stop。故不是 JSON 语法截断，也不是工具参数 JSON 缺 type。

阿里云[官方错误说明](https://help.aliyun.com/zh/model-studio/error-code)把这段 message 对应到输出内容检查拦截；实际响应使用的 code 是 `InvalidParameter`，必须保留这一原始事实，不能将其改写成文档中的 `DataInspectionFailed`。证据无法确定具体命中内容，也不能证明服务端判断正确。

## Grow 为什么丢失了真正错误

1. `crates/codegen/sampler/src/client.rs:1712` 先调用 `try_parse_stream_error(data)`，只传 SSE data，没有传 `event:error` 的事件名。
2. `crates/codegen/sampling-types/src/error.rs:499` 的解析只支持 `error:{...}` 和 `code/error:string`。此次 `code/message` 格式不匹配，返回 None。
3. `client.rs:1716` 回退至 `decode_tagged_event<MessageStreamEvent>`；内部 `client.rs:69` 按 JSON 顶层 type 选择事件，遇到 138-byte data 末尾仍找不到该字段，生成 Serialization。
4. `sampling-types/src/error.rs` 明确把 Serialization 设为不可重试；`sampler/src/retry.rs::classify_error` 返回 Fatal。这个“协议错误不可盲目重发”的规则本身合理，问题在于上游业务错误被错误归入这一类。

现有框架把解析、分类、恢复准入分开是合适的。本次应修错误事实的入口，让现有机制拿到正确原因，而不是从 `missing field type` 文本猜测可重试性。

## 工具与消费边界

失败响应包含两个 `write`：index 0 收到 content_block_stop，index 1 未闭合。它们都没有进入 Timeline assistant admission、tool execution 或工具结果；整个候选没有合法终止。截图的 Creating 预览不能证明本次工具已执行，完整 sibling 也不能越过整个响应的接纳边界单独执行。

message_start 有 `input_tokens=99595/output_tokens=0`，但后者只是开始时快照；已有大量输出，不能把它当作最终输出消费。没有最终 usage，所以 attempt_settled 为 unknown 是正确边界。重试即使技术上满足可撤回输出、工具未执行、预算充足，也仍需通过错误语义判断；本次输出检查拒绝不属于瞬态恢复。

## 建议的最小改动

**错误解析：需要。** 在已有三 backend 共用的错误解析入口识别 named `event:error` 的 `code/message` 形状，保留 provider code、message 和 request id 供诊断。限定 error 事件及完整字段，不能给任意缺 type 的正常事件补 type，也不能静默跳过该帧；HTTP 200 是传输状态，协议内的错误分类另行保留。

**错误码分类：需要与格式兼容一起更新。** 当前 `InvalidParameter` 可正确映射到不可重试的请求失败，但 `DataInspectionFailed` / `data_inspection_failed` 会落入默认 500，`Throttling` 也会落入 500。只扩大解析格式会暴露错误的自动重试行为。应为已确认的内容检查、限流和服务故障建立明确映射；未知 code 不能仅凭默认 500 被视为已证明的瞬态故障。复用现有错误结构和 classifier，避免新建通用 provider 框架。

**调度和预算：本次无需重写。** 保留统一 attempt 上限和绝对期限、取消、输出撤回能力、用量结算 ACK、owner 准入与工具接纳屏障。限流继续用限流预算和 Retry-After/退避；已知过载与断流继续走现有有界恢复。不要用立即重发、自动换 provider 或改动用户输入来掩盖这次策略拒绝。

| 失败语义 | 自动处理 |
| --- | --- |
| 本次 InvalidParameter + 输出检查拒绝 | 准确报告原因，终止自动重试 |
| 已知 DataInspectionFailed | 同上，不能变成 500 重采样 |
| Throttling / 已确认的 429 | 有界退避，复用限流预算 |
| 已知服务过载 / 5xx | 满足安全及结算边界后重试 |
| 无合法终止的 EOF / idle timeout | 沿现有 incomplete-stream 准入规则恢复 |
| 缺少必要字段的普通事件、身份冲突 | 保持协议失败；不把全部 Serialization 放行 |

后续实现验收应包含：三个 backend 的真实 HTTP/SSE 入口、精确 code/message 错误帧、未知和缺字段反例、内容拒绝不重试、429/过载的分类差异、部分工具候选不执行、用量 unknown 和共享预算不重置。修复建议登记在 backlog，本次仅分析。
