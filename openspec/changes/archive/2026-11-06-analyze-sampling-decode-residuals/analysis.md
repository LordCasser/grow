# 采样解码同类问题分析

当前仍可能出现同类问题。本次在包含下一版本修复的main `8ab486a6e45870561b91dba872934a6e24a795f1` 上，确认一项真实链路可复现的解析缺陷和一项源码可确定的错误分类偏差；另有数个需要真实路由响应才能决定是否兼容的输入边界。没有修改生产代码或重新发布。公开2.6.1源码仍为deaca4899，尚不包含此前单独提交的annotations和稀疏failed修复。

2026-11-06官方资料补查：用户明确重点是跨协议、跨模型切换，当前结论应连同[历史投影与官方契约对照](../2026-11-06-compare-official-sampling-api-contracts/analysis.md)阅读。本文usage实验事实不变；第2项是Grow自身malformedordinary/重试契约偏差，官方OpenAI SDK也有顶层error优先处理，不能称其违反OpenAI协议。summary/content/序号等字段在官方schema也有必填依据，不直接认定SDK过严或统一补默认值。切换的主要目标约束是历史reasoning完整性与目标Chat对客户端合成工具往返的许可，本文单次解码分析不替代该链路验证。

## 1. usage明细补全会擦掉其他字段冲突：已实跑，优先修复

所有者为 `crates/codegen/sampler/src/client.rs:256` 的 `normalize_response_usage_details`。SDK不能解码时，该函数把整个完成事件读成serde_json::Value，补齐缺失的usage明细，再序列化。Value只保留同名字段的最后一个值；该helper没有新arguments/annotations helper的重复字段保护。因此补一项usage字段的例外会同时改变response身份、状态、输出或工具身份的拒收结果。

以下是实验使用的一类完整输入，末尾加SSE空行即可；重复name是原始JSON字节的一部分，不能先用Value构造它：

```json
{"type":"response.completed","sequence_number":7,"response":{"id":"resp-direct","object":"response","created_at":0,"model":"test-model","status":"completed","output":[{"type":"function_call","id":"item-0","call_id":"call-direct","name":"other_tool","name":"test_tool","arguments":"{\"a\":1}","status":"completed"}],"usage":{"input_tokens":100,"output_tokens":5,"total_tokens":105}}}
```

| 对照输入 | 锁定SDK直接解码 | 当前client实际结果 |
| --- | --- | --- |
| 无重复字段，usage明细完整 | 接受 | 完成，tool=test_tool |
| 无重复字段，仅省略usage明细 | 缺input_tokens_details | 经既有例外完成，cache read保留未知 |
| 重复response.id或function.name，usage明细完整 | 拒绝重复字段 | Serialization拒收 |
| 同样的重复字段，仅省略usage明细 | 仍拒绝重复字段 | HTTP/SSE→conversation_collect→L2完成，返回test_tool候选 |

实际SDK/decoder矩阵还核了重复status/output/sequence_number/type/call_id，以及null-first response.id：直接SDK均拒收，仅触发usage补全时现decoder均转为ResponseCompleted。完整真实链路对照只跑response.id与function.name两个代表，不把其余decoder结果夸大成全部端到端验证。

隔离worktree仅增加三个临时探针，生产helper与main完全相同；固定Rust1.93.1、锁定async-openai0.41.3/rev a2bae9966f435bc343aa095065d96700bfb81aed，--locked --offline运行3/3通过（断言现有差异，并非修复通过）。父读完整探针与原始日志，再独立复跑同三项，exit0。证据在 `/tmp/grow-similar-usage-Ep8Z1S/`，包含原日志probes.log、父日志parent-probes.log、临时测试probes.patch及六条SSE输入。没有真实provider出现这种重复字段的证据，也没有执行候选工具；不能据此宣称权限绕过或实际工具误执行。

下一项独立修复应由usage解码补全owner负责：只允许补全契约接受的usage缺省，保留原始已知字段的类型、唯一性与冲突拒收。验收用上述成对输入证明补全不会改变无关身份/输出，保留raw evidence、缓存presence、终态和L2接纳规则。不能全局忽略重复键，也不能取消usage缺省支持。若采用复用解析能力，应由实际不变量证明必要性，不预建通用provider框架。

## 2. 流错误预解析会抢先覆盖普通事件校验：源码确定，未实跑

所有者为 `crates/codegen/sampling-types/src/error.rs:822` 的 `try_parse_stream_error`。三种reader都在各自typed decoder之前调用它：Chat `client.rs:1343`、Responses `:1702`、Messages `:1999`。该helper先无条件使用HTTP错误包parser；嵌套/平铺ErrorResponse没有排除已明确声明为普通事件的payload。仅后备平铺code/message分支限制SSE事件名为error。

最小反例是完整SSE帧，省略event行时SSE名称默认为message：

```text
data: {"type":"response.output_text.delta","error":{"code":"server_error","message":"fixture"}}

```

这条已知普通delta缺少sequence_number/item_id/indices/delta，直接decoder会报Serialization。但组合reader先返回server_error，构成API500且没有retry veto。Messages的 `{"type":"message_start","error":{...}}` 和Chat的 `{"object":"chat.completion.chunk","choices":"invalid","error":{...}}` 有同一调用顺序问题。平铺error字符串也会命中gateway包，缺code时默认server_error。

这与主规范model-sampling的“Named provider stream errors retain their error facts”中畸形普通帧不能取得自动重试资格，以及typed attempt facts区分协议错误/服务失败的要求冲突。原typed错误通过SamplingErrorInfo.source和actor tee_errors保留，L2失败后不会重新校验该普通帧。未知code仍会生成should_retry=false并被veto，不能概括为全部500均自动重试；实际再次派发仍受副作用安全、交付撤回、结算ACK、准入及共享预算保护。

本项由父读完整共享parser、三个reader、L2错误传播、error source及retry规则确定；未执行实际Rust组合链，未取得该混合形状的现场路由证据。子报告为 `/tmp/grow-similar-errors-aeaf-preparse/report.md`。既有malformed-known单元测试直接调用decoder，未覆盖预解析+decoder组合，因此局部通过不能证明此反例被拒收。

后续先补三reader最小red，再在现有共享流错误识别owner明确普通事件身份与错误包资格。必须保留既有bare legacy nested/gateway包（归档2026-09-23-fix-named-sse-errors明确支持与SSE名称无关）、named error及合法typed error。不能整体收紧HTTP错误parser，不能简单要求所有错误都叫event:error。该独立行为与第1项不同，不捆绑实施。

## 3. 会再次出现missing field的支持边界：需现场证据

| 输入变化 | 当前拒收原因 | 如何判断是否应修 |
| --- | --- | --- |
| 初始reasoning item省略summary | SDK ReasoningItem.summary是必填Vec | 取得真实added帧、同项后续summary/done和成功对照；确认允许空前缀后才决定局部例外 |
| 初始message省略content/status | SDK OutputMessage要求这些字段 | 区分未完成前缀与身份/状态不完整；不能统一替换成空/完成状态 |
| 已知事件省略sequence_number；尤其unnamed type:error/code/message | SDK事件要求序号，named error helper仅为SSE event:error提供平铺例外 | 原始code/message可能仍被missing sequence遮住，但现契约未承诺此unnamed形状；有实际路由证据再决定支持 |
| 稀疏response.failed多出request_id/sequence_number/usage等 | 已验收修复严格限制三层字段集合 | 这是明示的兼容范围；必须核字段类型、归属和真实usage，不能为容纳一字段重新伪造完整Response |
| output_text added缺text，或非空text缺annotations | 现annotations例外仅显式text为空且annotations键缺失 | 当前拒收符合契约；先确认真实供应方语义，不扩大空文本例外到任意内容 |
| 未支持的嵌套content/status/annotation变体 | known顶层事件里的未知嵌套值仍会typed失败 | 只有unknown顶层扩展按现规则跳过，不能吞掉已知事件的未知内部结构 |

这些是源码能定位的拒收路径，不是已经证明存在的额外provider缺陷。Response本体的id/model/output/status/created_at仍被created、in_progress、completed等SDK事件复用；目前的几个精确例外没有把整个流协议变成允许任意字段缺省。

## 4. 处理顺序与停止点

先处理第1项可复现的数据一致性问题，再为第2项补组合red并修正服务错误识别归属。第3组先补真实路由证据，不提前加默认值/兼容注册表，不将通用Serialization设为可重试。每项实现各建独立change及owner，不把本次分析自动变成修复授权。

已验收的初始arguments/空part annotations/精确稀疏failed修复仍有效，EOF不产生完成、身份冲突与非法工具JSON不接纳、候选接纳和工具权限仍沿原所有者。effort缺失的原现场是2.6.0进程旧声明，正式2.6.1actualschema已核，不能仅凭旧进程再认定新参数缺陷。

本次只新增分析记录并登记两个独立后续事项，不扩大到LSP、Goal、用户会话或全仓审计，不改生产代码、公开tag/资产或用户运行环境。实验和静态覆盖无法证明所有provider变体都兼容，后续需以具体原始事件和被测版本为准。
