# end_turn 来源复核

## 确认的区分

用户的怀疑有实际代码依据：`shell/src/session/actor/turn/admission.rs` 的 `AdmittedTurnSuccess::Model(TurnOutcome::Completed)` 分支自行将 `TurnTerminal.stop_reason` 写成 `end_turn`（refusal 除外）；控制边界等分支也使用这个标签。它表达 Grow 的 Turn 分类，并不是 provider 原始 stop_reason。

因此，单凭 `Turn::Ended.terminal.stop_reason=end_turn` 不能认定端点发出了同名字段。Responses 本身通常发出 `response.completed`，与 Grow 的 `end_turn` 标签不同。`response.completed` 也可同时带工具调用，并不自动代表整个用户任务完成。

## 第一个会话：结束帧存在于原始 body

会话 `01a08906-7afd-70e2-af4e-5ff9ea4db84c`，ScriptOS。重新读取三份原始 response artifact，长度均与 observation 一致，`truncated_by_evidence_limit=false`。

| observation seq | 字节数 | artifact hash |
| --- | --- | --- |
| 49584 | 199470 | a7b56c1b7fd95f5f827cdee51c542d410bdd168a6738a3e5aec2c3722322ffd4 |
| 49617 | 46603 | a6aacbc0c068fb08e692699e411a7bcd8ff497df29e3e3670b06fda81e98aa6e |
| 49660 | 110936 | 092b34a777af31e4bb0ba87c3dcc71356b6ccbb59859d18b61aa49817ddab7f8 |

三份 body 都在末尾中文冒号的 text_delta 之后携带 content_block_stop、如下 message_delta（usage 省略）以及 message_stop：

```text
event:message_delta
data:{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":...}

event:message_stop
data:{"type":"message_stop"}
```

`sampler/src/client.rs::create_message_stream` 的链路是 `response.bytes_stream()` → `audit.response(&bytes)` → BOM 处理 → `.eventsource()` → JSON 解析。旧版 `155780e2` 也在相同的解析前位置采集。`audit.rs::response` 只 `extend_from_slice`，`finish` 仅把捕获的 body 交给 sink；Shell sink 按内容保存不可变 artifact，没有拼入 SSE 终态。

所以这三份结束帧不是 Grow 本地补出来的。这仅证明所连接 HTTP 端点返回了这些字节，不能证明该端点内部是否有代理/兼容转换，也不能据此把行动预告认定为任务完成。

## 第二个会话：已定位片段与边界

会话 `01a08b02-acf9-7c33-8b0b-cf474d521d09`，konboot；request evidence 记录 Responses 路由 `https://api.deepseek.com/v1/responses`。只读已持久化前缀至 seq 7624，没有执行其中的终端命令或更改会话。

在该前缀内找出所有 assistant 正文末尾为中文冒号的片段：

| 时间（上海） | assistant seq | 正文末尾 | 实际后续 |
| --- | --- | --- | --- |
| 19:14:11 | 263 | 再次需要管理员授权…只读挂载）： | 带一条工具调用；工具成功完成于 19:14:17，Step 273 为 continued，下一请求 276。 |
| 20:39:01 | 6323 | 趁此分析内存里那第二处代码副本： | 带一条工具调用；141 ms 后成功完成，Step 6333 为 continued，下一请求 6336。 |

对应原始响应 seq 261 / 6320 分别为 138327 / 321890 字节。两者的 `response.completed` 内都包含 `message.phase=commentary` 和完整的 `function_call`，工具 ID 与 Timeline 一致。

该前缀内已完成的正常 Turn 对应原始 response seq 2022、2596、3940、4636、5555；均有 `response.completed`、status completed 和 `message.phase=final_answer`，正文为较长报告。没有找到“无工具、中文冒号结尾、Turn completed”的第二份精确样本。已请求用户补充具体句子或时刻；这不排除其他时刻、界面状态或其他会话的停顿。

## 缺终态分支

旧版 `155780e2` 与当前三个流解析器都不会仅凭 EOF 生成完整候选：Chat 缺 finish_reason、Messages 缺 stop_reason/message_stop 或未闭合 block、Responses 缺 terminal response 都走错误。当前版本把这类缺失归为可重试 incomplete stream，恢复仍受统一预算和输出接纳约束；旧版主要为 protocol failure。

明确但未知的 stop 字符串仍可归一化为 Stop；原始值另存。这和“字段缺失时补终态”不同，也未出现在第一份会话的三次已定位响应。

## 后续候选及未解决项

Responses 的有效 native fragment 会 clone 原 message，保留 phase；中性 AssistantItem 没有 phase，portable 重建时为 None。不能因此声称它解释 Qwen Messages 的三次结束，也不能仅凭此修改全部 Turn 行为。[OpenAI phase 文档](https://developers.openai.com/api/docs/guides/latest-model?model=gpt-5.5) 明确要求手动重放时保留该字段；这是值得单独以实际输入/输出验证的候选。

整体“行动预告后任务提前结束”仍未关闭。前次 1034 项测试证明请求结构和既有生命周期场景通过，不是模型语义完成率测试；其中故意冒号结尾的 mock 只证明未增加标点重试，不能作为真实任务完成的证据。
