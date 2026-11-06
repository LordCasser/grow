# Design

## Evidence and existing owners

两个直属child为01a1ae75-f38d-72c1-88be-87f7444464d8和01a1ae75-89fe-7551-8179-31db03e2cf15，request分别eaa1b3cf-ab7d-4a8e-a260-af54476ad675与f40f3e24-f573-4563-a895-dcab95bc89a1。其raw流先有完整created/in_progress，同response ID与有效created_at；结尾在590704与116023字节处含相同稀疏failed形状：top{type,response}，response{error,object,status}，error{code,message}，object=response/status=failed/code=server_error/message指明unexpected EOF。没有seq/id/created_at/usage。相邻同route成功对照含完整completed。父通过有界脚本亲核两条原流及对照，结果在 `/tmp/grow-created-at-parent-proof.json`，调查资料在 `/tmp/grow-created-at-investigation-01a1a9c0/`。没有重放provider或任务。

当前client先对SDKResponseStreamEvent解码；失败包的SDKResponse字段必填使created_at先报错。stream/responses.rs的完整ResponseFailed分支本来会校验failedstatus、保留terminal/usage并调用SamplingError::from_stream_error；actor沿现有限恢复规则处理该API错误。共享try_parse_stream_error不识别response.error嵌套，不应为了单一Responses形状扩散到Chat/Messages。

## Exact error fallback

唯一落点是Responses client已有decode_response_event错误分支。先保留常规SDK及已有usage/初始part正常路径；对SDK仍无法解码的已知failed形状尝试私有有类型稀疏error envelope。所有key集合按上述原形状严格限制，可直接局部serde structs+deny_unknown_fields，SDK继续处理完整包。三层非Option字段保留类型/缺失/重复key错误，code/message需trim非空但传递原值；精确检查type/object/status。额外seq/id/usage/其它字段不授予这个例外，原known-event错误保留。不要用Value再编码吞重复key，不生成任何SDKResponse、不补created_at/id/model/output/seq。

该fallback返回SamplingError::from_stream_error(code,message)而非成功typed事件，完整typed失败仍由原L2负责。AttemptEvidence::observe_terminal记录已实际收到的ProviderTerminal::Responses{event:response.failed,status:failed,incomplete_reason:None}，只有fallback已成功识别后才记录，不构造响应或用缺失id做归属。归属是本HTTP流/attempt；raw响应证据原字节保留。该诊断用于重建实际服务终态，不等于成功接纳。

from_stream_error已负责API/auth/request/rate错误分类。server_error继续走已有有限transport恢复预算；不改retry预算、ReplayUnsafe或candidate admission。中途工具参数只能是候选，失败时不能Completed或执行调用。上游连接实际断开仍会失败，修复保证原因/分类准确并允许既有恢复处理，不声称修复外部服务。

## Alternatives and limits

补0/now或从created缓存拼完整failed会制造不存在的序号、ID、usage与输出快照，且新增跨事件状态；忽略failed会把明确错误变成无终态EOF。宽泛接受任意嵌套error又会隐藏已知畸形字段。因此采用精确错误包fallback，不为未证实的其它缺省字段预建框架。

## Coder task and review

一个client owner实施此单项错误传播修复，test放同client。先在旧decoder保存真实HTTP/SSE（完整created→部分输出→稀疏failed）的有效red，预期API500/server_error而实际Serialization。修后断言原code/message、is_retryable与现classify_error有限预算、恰好一个Failed且无Completed/工具接纳、AttemptEvidence原bytes和provider_terminal。显式auth/rate/invalid-request codes按共享分类作控制；完整typedfailed保留，completed/incomplete缺字段仍失败；错误/空code/message、错status/object/type、三层重复key/null-first、额外字段不修，EOF无终态仍错误。无需改actor或共享error类型。

父核最终diff、原red/green与负向控制、Sampler库/集成和固定1.93.1check/fmt零warning，合入后进行必要定向验证；文档/strict/archive完成后独立提交下一版本修复。无新独立review，不移动任何公开tag或发布新版本。
