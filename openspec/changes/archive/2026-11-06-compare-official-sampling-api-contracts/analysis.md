# 跨协议模型切换：官方契约与 Grow 历史投影对照

本次于2026-11-06采集OpenAI、Anthropic及DeepSeek的官方资料，复核Grow主线8ab486a6。用户明确本题主要是同一会话在不同协议、不同模型之间切换，因此分析主线是“源模型响应保存→route替换→portable历史重建→目标API接纳”，单次流解码仅作后面的关联证据。官方wire、官方SDK容错、Grow锁定SDK和现场代理是不同依据。本次未调用真实推理API，未修改生产代码或发布内容。

## 切换链路的当前结论

当前没有新证据表明Grow把旧协议的原生签名直接泄漏到新协议。已核且本轮重跑的回归证明route替换清除旧native span与已学习reasoning规则，工具调用/结果按目标协议配对，A→B→A不复活旧native。实际风险集中在目标API对历史语义的额外要求：DeepSeek thinking Chat要求完整reasoning回传，而它的Chat又限制客户端合成工具往返插入中段。这两项不能靠给新流事件补几个缺省字段解决。

### 目标协议接收的历史不是同一种结构

| 目标 | 完整工具往返的目标表达 | reasoning/native状态边界 |
| --- | --- | --- |
| Chat Completions | assistant.tool_calls的id/name/arguments字符串；role:tool的tool_call_id结果 | 标准OpenAIChat不等于全部兼容服务；DeepSeek thinking+tools另要求历史reasoning_content完整回传 |
| Responses | function_call的call_id/name/arguments；function_call_output用同call_id关联 | 官方reasoning item、encrypted_content与可见summary不同；兼容family的原生复用有供应方边界 |
| Messages | assistant tool_use的id/name/input对象；user tool_result的tool_use_id关联 | 同tool-use turn要保留完整未修改的thinking/signature；foreign签名不能从可见文字重建 |

相同模型名经不同endpoint/backend服务，也属于不同路由；相同协议换模型仍可能改变原生reasoning可读性。OpenAI当前文档按model family限制重用，Anthropic同生态可让服务按目标model过滤不兼容block。因此“每次换model都必须丢所有reasoning”不是普遍官网规则；Grow对route替换统一清native是现有保守隔离选择，本轮不将可兼容模型的复用优化冒充缺陷，也不把同供应方允许复用推广到跨供应方。

### 已核实的所有者与不变量

- Shell模型选择发布目标路由；ChatState `ContinuationLane::replace_route`创建新epoch并清spans/portable_reasoning_backend。request_segments遇backend不匹配也降为portable，不能把同route原生片段直接用在新协议。
- `project_portable_history_with_reasoning`保留正文及完整唯一工具配对，过滤孤立结果、重复ID和非法参数；MessagesID编码保持一一对应。转换历史本身不请求执行旧工具。
- 默认portable不发旧签名、encrypted_content、provider itemID/status或reasoning。目标明确400要求Chat reasoning_content或Responses reasoning_text时，Shell按typed事实请求ChatState ACK，在当前route内启用一次；相同route reset保留该学习，真正route替换清除，原logicalattempt预算不重置。
- Chat在全部runtime消息展开后再次给缺reasoning字段的assistant补空字符串（conversation.rs:4010），所以不能因helper内部continue就断言agent/skill合成assistant最终漏key。空字段是否被目标认为满足“完整回传”是另一问题。

### 风险一：可见 summary 无法保证满足目标的完整 reasoning 要求

[DeepSeek官方Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)明确：thinking请求携带tools时，所有之前assistant回合的reasoning_content要完整回传，包括没有调用工具的回合。Grow持久化的VisibleReasoningItem只有text，来源Responses的summary/content会合为可见文字；Anthropic当前官方也说明thinking可见文本是summary，display omitted还可能为空。route替换后旧native不复活，现有恢复只能回放已有可见文字或空字符串。

因此，“切换后补上reasoning_content键”与“满足目标所需完整原生reasoning”不能等同。举例：A为Responses并只返回summary，B为DeepSeek thinking Chat且带tools；B的首次请求默认移除历史reasoning，若明确400触发学习，第二次只能回放summary。若源未提供完整reasoning，系统不存在可无损重建它的数据。来源摘要与完整正文也没有类型层区分，不能声称请求已恢复原模型的完整思考。

这是已确认的表示/保证边界，不是本轮已证B必然400或产生质量退化。DeepSeek文档描述其自身历史回传，没有给跨vendor/无reasoning历史一个统一迁移保证；实际目标可能仅检查字段，也可能有更强要求，需要原始target请求/响应验证。已有2026-10-29真实六方向矩阵都未触发reasoning400，不能拿它证明新first-party规则下“400→学习→成功”已经通过。

### 风险二：目标 Chat 限制合成工具历史，当前投影确实会生成

[DeepSeek官方Tool Calls](https://api-docs.deepseek.com/guides/tool_calls/#inserting-tool-calls-mid-conversation)明确区别模型生成的工具历史与客户端动态插入的非模型工具调用：它的Chat接口不支持后者插在会话中段，Responses/Anthropic格式支持。Grow的AgentMessage及用户来源SkillLoad持久事实会在Chat投影中展开为synthetic assistant tool_call与匹配role:tool结果；模型来源SkillLoad已有真实调用，走skip而不重复合成。旧历史经portable切换仍保留这些runtime事实。

这给出一个可定位的兼容冲突前提：历史包含位于中段的agent消息或用户技能事实，随后切到遵守该限制的DeepSeek Chat。当前生成形状属于官网限定的client合成工具往返，而不只是一般真实工具历史换格式。即使reasoning字段已补空，这个结构限制仍存在。目标Chat适配与共享runtime事实表达是需要共同确认的契约边界；不能按“缺一个reasoning字段”一直重试，也不能把runtime内容伪装成DirectUser授权、删除已接纳消息或重新执行历史工具。

本轮确认本地投影和官方限制的静态冲突，尚未发真实DeepSeek请求，不能宣称具体会话已因此400，不能扩大为所有Chat厂商、所有合成尾部消息或所有foreign模型工具历史均不支持。后续最小验证应比较同一历史中的真实模型工具往返/合成runtime往返，目标DeepSeek Chat/Responses/Messages分别接受与拒绝的证据；保留同一内容、权限来源和只执行一次的业务事实。

### 验证强度与下一步顺序

本轮固定Rust1.93.1、--locked --offline，重跑sampling-types的portable、learned、agent_message_projection用例及ChatState endpoint_switch_matrix/reasoning_replay：24次执行通过，去重22项。六个跨backend方向与A→B→A在matrix单测内部覆盖。它们证明本地wire和route状态规则，不证明目标API接纳。没有为本轮修改源码或新增同义测试。

当前优先级应是：先给“含runtime合成往返切到DeepSeekChat”建立目标接纳证据并确定适配政策；再核“来源只有summary/没有reasoning→目标要求完整reasoning”的受支持迁移边界。不得为了消除400伪造签名/fullreasoning、把中性事实升级为用户权限、默默删历史、复活旧native或扩大恢复预算。单次流usage补全和标准error映射是关联的独立缺口，不能替代上述切换链验证。

## 官方流解码与 SDK 的关联依据

## 1. OpenAI Responses：空前缀有依据，省略字段没有统一依据

父直接下载官方 `openai/openai-openapi@9752a7f1fc51a0729e8fe544b315ec5018f78ada` 的5,134,456字节openapi.json，SHA256为cea6c5f4380b25256390c565dff487586c9332add7fbd345f257d54733bfc90c；同时取得官方Python SDK c6fc264689d384d5caec150fafa1a9567c4a3f56及Node SDK a523d74b7c82fc4379259eb2352a13ee400d8b4b的必要源码。固定来源见[来源索引](sources.md)。

| 对象/事件 | 官方schema与SDK类型 | 对Grow结论的影响 |
| --- | --- | --- |
| FunctionToolCall | required为type/call_id/name/arguments；arguments是string。官方指南初始示例为arguments:"" | 现场added省略arguments是兼容路由差异，不能宣称OpenAI标准普遍允许省略；已验收的in_progress窄例外仍有现场依据 |
| OutputTextContent | required含type/text/annotations/logprobs；官方初始part示例明确text:""、annotations:[]、logprobs:[] | 省略annotations同样不属于官网通用保证；现空part窄例外不应扩大到非空文本/done/terminal |
| OutputMessage | required含id/type/role/content/status，初始added例子content:[]、status:in_progress | 上轮把缺content/status列为候选是拒收路径描述，不能据此判定SDK过严或直接补空/完成状态 |
| ReasoningItem | required为id/summary/type；status和content不在required中 | summary缺省不是当前官方允许的形状；保持缺失/空数组的区别 |
| ResponseFailedEvent | required为type/response/sequence_number，response引用完整Response；其required包含id/created_at/model/output等 | 现场仅response.error/object/status的失败包不是官方完整failed形状。记录真实错误可以有局部兼容，不能补0/now或缓存快照伪造其他字段 |
| ResponseErrorEvent | sequence_number为required；code/param为required但可null，message必填 | named平铺错误包与typedResponses错误是不同入口。未命名且缺序号的包不能自动推定为标准OpenAI错误 |
| ResponseUsage | 两个明细object及input/output/total为required；cached_tokens、cache_write_tokens、reasoning_tokens有明确计数类型 | 缺usage明细的支持是Grow兼容策略；补全不能改变其它字段的身份/类型/冲突校验 |

需要保留官方资料自身的差异：OpenAPI的OutputTextContent把logprobs列为required，官方Python的ResponseOutputText却将其建模为Optional默认None；ResponseErrorEvent的code/param在schema要求key存在但允许null，SDK也能容忍缺key。因此不应把当前schema机械映射成所有入口的新增硬性校验。官网函数调用指南的简化片段还省略sequence_number等字段；wire判定以正式schema、对应事件示例和实际SDK路径共同校验，不能只复制示例。

## 2. 官方 SDK 容错不等于允许任意字段缺省

OpenAI Python客户端的 `_strict_response_validation` 默认false。`_base_client.py::_process_response_data` 默认走construct_type而不是validate_type；`_models.py`明确写着无验证递归构造。README说明缺字段与显式null都可能表现为None，需用model_fields_set区分。Node底层reader则JSON.parse后交出对象，并没有逐事件运行完整schema验证。

这些事实能解释同一不完整包在Python/TypeScript中未立即触发反序列化错误、在Rust serde中却先报missing field；它们不能证明包符合官方wire schema，也不能保证后续累积器不会失败。父亲读的官方Python Responses累积器仍校验response和output identity，仍使用 `output.arguments += delta`、`content.text += delta` 等状态；松解析之后还可能在访问或累积阶段失败。本次没有安装/执行这些官方SDK，结论来自固定源码。

Grow使用的是社区async-openai0.41.3，且通过Cargo patch锁定LordCasser fork的a2bae9966f435bc343aa095065d96700bfb81aed；它不是OpenAI官方Rust SDK。不能把该库的serde必填性直接当作官方协议，也不能直接用Python默认构造的宽松程度替代Grow已承诺的候选接纳保证。

## 3. 上一轮两项发现的官方依据修正

**usage补全丢重复字段的实验结论保持不变。** 同一个重复id/name完成帧，只因是否缺usage明细而出现拒收/完成差异，已经在当前真实client→L2链路复现；这是Grow的归一化一致性问题。官方SDK先使用json.loads/JSON.parse，不能作为“所有官方SDK都拒绝重复键”的依据。[RFC8259 §4](https://www.rfc-editor.org/rfc/rfc8259.html#section-4)规定名字SHOULD唯一，并明确不同实现可能取末值或报错，并非所有解析器MUST拒收。后续修复要维持Grow自身一致的拒收与raw证据，不以虚构外部保证立项。

**error抢先识别需要收窄为Grow内部契约与重试分类问题。** 官方OpenAI Python `_streaming.py:94-107,214-227` 和Node `core/streaming.ts:138-157` 同样会在普通typed处理之前对顶层error抛APIError。故“先识别error”本身不是与OpenAI官方SDK相悖的行为。上一轮合成known普通帧仍证明Grow当前实现和现有malformedordinary不得取得retry资格的规范不一致；应先明确混合报文优先级以及服务错误分类，再决定窄修复。官方SDK抛出流迭代错误也不等于把HTTP200自动改记500后重放；Grow额外的typed映射、重试资格与预算仍需独立审查。

## 4. Anthropic Messages：主流式分工已覆盖，标准错误类型需补核

官方资料和固定SDK证据见sources.md；以下结论由父核原文后形成。官方streaming文档明确message_start含空content，text/thinking/signature初值为空字符串，tool_use.input为{}占位，随后input_json_delta.partial_json作为字符串累积。空对象和增量字符串分工是设计契约，未给出“任意省略key”的通用保证。必须在block关闭/完整终态处处理JSON有效性和max_tokens等实际停止原因，不能把初始{}视为最终工具参数。

message_delta中的usage是累计值，覆盖而不是相加；未提供的可选字段应保留message_start已有值。官方说明新event types可新增且应graceful处理；这不等于已知事件里的畸形必要字段均可吞掉。Grow现Messages路径已区分initial_input和partial_json，覆盖累计usage与未知顶层事件处理，这些官方差异本身不构成新缺陷。

新的具体分类缺口位于 `sampling-types/error.rs::from_stream_error`：官网明确500/api_error是服务内部错误并建议指数退避；timeout_error列为504，也可能在开始响应前对应500。官方Python/TypeScript错误类型同样列出这两个字符串。Grow当前known_server_error只有server_error/internal_server_error/network_error/transient，因此两者在流内都落入通用500并附should_retry=false；timeout身份还被折叠。已有overloaded_error→529和未知code不重试边界应保持。

要区分政策和事实：官网明确HTTP200后SSE错误不遵循通常HTTP错误机制；官方SDK迭代器此时抛错交调用方，未在该迭代路径重新派发。api_error有明确backoff建议，timeout段主要定义taxonomy，不能用它推出无条件自动重试。后续独立验证应让标准错误通过真实Messages SSE→helper/L2/recovery，保留code/message与requestID诊断、unknownusage、候选不执行及sharedbudget/replaysafety。实际用户route的anthropic-version/betaheaders和是否收到这些标准流错误本轮未知，不读会话配置推断。

## 5. OpenAI Chat 与 DeepSeek：同名兼容接口仍有尾包差异

OpenAI官方Chat stream_options.include_usage定义额外usage-only尾帧，choices=[]，其它usage值为null；流中断时可能收不到最终usage。DeepSeek当前官方说明不同：最终统计附在最后一个含单个choice及非null finish_reason的chunk，明确不另发usage-only帧。Grow现Chat L2既处理同帧finish+usage，也处理choices=[]尾帧，并保留丢usage时的未知结算；这是应保留的两种已支持形状，不需因官网差异再添分支。

Chat明细字段与Responses不能共用必填规则。官方OpenAPI CompletionUsage只要求prompt/completion/total，明细和各项计数可缺；计数写integer/default0，未写null分支。官方Python模型则允许Optional[int]=None，Grow既有契约也支持现场nullable细项。应区分“schema允许缺省”和“SDK/兼容路由允许null”，保留rawpresence和unknown语义，不能把所有null变0，也不能将Responses明细要求套到Chat。

DeepSeek官网直接推荐OpenAI/Anthropic兼容SDK，本次未将第三方DeepSeek封装称为官方SDK。thinking_mode文档按是否携带tools区分reasoning_content回传，不能把无工具示例推广到带工具调用链；本轮不改continuation实现。旧reasoning_model URL在不同抓取路径表现为PageNotFound或HTTP200首页fallback，不是可用旧文档，采用已亲取的thinking_mode正文。

## 6. 当前可采取的独立处理边界

| 事项 | 本轮判定 | 后续范围 |
| --- | --- | --- |
| Anthropic api_error/timeout_error分类 | 有官网与官方SDK支持的标准类型映射缺口；源码确定，未跑新API或Rust用例 | 在现stream-error分类owner补标准类型回归，再决定有限retry/timeout表达；保留未知codeveto和所有attempt门 |
| usage补全抹冲突 | 前轮真实链路实验仍成立；不是所有SDK统一拒收保证 | 在usage补全owner修复“无关字段校验随统计缺省而变化”的行为，保留正控/availability |
| 普通帧附error的优先级 | Grow现规范偏差；OpenAI官方SDK存在同类error优先策略 | 先用三reader组合red明确本地政策，再修资格/分类，不整体收紧HTTP错误包parser |
| 新缺summary/content/sequence等 | 正式schema有必填依据，未支持变体不能直接定性SDK过严 | 原始route证据+适用schema/版本到齐再决定精确例外；不批量加serde(default) |
| Chat尾包/nullable、Messages占位与累计usage | 已有实现/回归入口覆盖主要差异；本轮仅静态对照 | 维护代表性官方fixtures，不新造兼容层 |

本次结束于资料收集和架构分析。未新增公共类型、兼容注册表、默认值或重试循环，未改业务实现；每一项实施均按独立行为和真实调用链验收。官网当前资料与用户代理部署可能有版本差异，最终以被测route的原始事件和固定版本证据核对。
