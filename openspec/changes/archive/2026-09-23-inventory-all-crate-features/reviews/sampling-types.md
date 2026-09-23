# sampling-types 逐包核查（进行中）

已读取manifest、lib.rs、serde_helpers.rs和error.rs；error首次输出中段截断后补读448–710。其余4文件未读，保持pending。无Cargo feature；依赖tools/compaction以及provider类型，不能仅凭“pure data”描述推断无传递依赖或无日志输出。

lib重导出conversation/types、doom-loop及error若干公共类型，rs指向async-openai Responses类型。MAX_REQUEST_BODY_BYTES为50MiB常量，实际最终字节执行上限仍须核对transport，类型包不执行发送。

empty_string_as_none仅把空字符串变None，不trim空白；double_option须配serde(default)：缺失None、null Some(None)、值Some(Some)，辅助函数本身被调用总返回外层Some。

error提供Auth含SentCredential、InvalidConfiguration、Persistence、Http、Serialization、Api(status/message/model metadata/retry hints)、EventStream、IdleTimeout、EmptyResponse上下文及DoomLoop triggers/chunk。EmptyReason蛇形reasoning_only/no_visible_content，分类算法待conversation。SentCredential默认Unknown，反序列化未知string退Unknown但非string仍报错；from_sent_fragment只看Option是否Some，空字符串Some也Sent，不校验实际wire。

from_stream_error先把类型非ASCII alnum换underscore并转小写，按顺序判authentication/invalid_api_key/精确unauthorized→Auth Unknown；billing/payment/quota→402；permission/forbidden/denied/sandbox或network+policy/blocked/not_allowed→403；not_found→404；request/payload too large→413；rate_limit→429；overload/service unavailable→529；invalid/unsupported/bad_request→400；其它500。保留原type:message，metadata和hint为空。这是启发式顺序映射，不保证未知provider标签准确。

is_auth仅Auth或Api401，不含403；rate/payload只Api429/413。is_retryable对Auth/config/persistence/serialization/idle false；Api仅429/500/502/503/504/520/529；EventStream/Empty/DoomLoop true；Http按reqwest flags：timeout/connect优先true，status为任意5xx或429，request/body true，其它false。因此Http状态与Api白名单不完全相同。is_retryable不自身应用veto，调用方仍须结合should_retry=false、context-length启发式及413；true header不强制其它错误可重试。

is_overloaded仅Api529或5xx message包含overloaded/service_unavailable_error；4xx message提到overload不变容量错误。context-length仅Api message，匹配固定小写片段或current message与exceeds budget同时存在，不泛化其它budget。metadata/retry_after/should_retry只Api取值，本层不解析HTTP headers或执行重试预算。

Serialization重建保留variant，用共享display prefix移除一次避免重复；serde From记录debug，stream error解析记录warn含原消息。JSON错误解析接受nested error或flat error字符串，nested优先code再type，缺失message给unknown error。structured用户消息trim后最多280 Unicode字符再省略号，未知/server_error不加type；非法UTF8/非结构JSON返回upstream error或按status文案，绝不直接展示HTML/plain body。完整原stream错误不走280截断。

所有error测试已静态读取，包括状态分类、veto、serialization roundtrip、idle、nested/flat错误、用户文案和凭据serde；尚未动态执行，不能据此证明各调用方统一应用重试策略。

## 本批读取证据

- `crates/codegen/sampling-types/Cargo.toml` SHA256 `15b7e515573701b1175327b04cdfe2bf7e33b8f724f185b9d10df428ca28a56b`
- `crates/codegen/sampling-types/src/lib.rs` SHA256 `f22e4622d3bcd29a4a59b773594ce0ee6b4119a6482bab3a9abfb6546d1496fa`
- `crates/codegen/sampling-types/src/serde_helpers.rs` SHA256 `b25f5630f26de22dfd613d4ed931e6ae89d5681be1f994eba1b9ea3036d089b1`
- `crates/codegen/sampling-types/src/error.rs` SHA256 `7dd4be9b979e5a6bf04619378f1292dc756028d670c6c5951105f2b8c401d43a`

## doom_loop.rs 完整读取

527行含全部16项测试读取。header/event常量分别x-grow-doom-loop-check与response.doom_loop_check，两个sample字符串保存累计trigger样本；本模块定义解析和策略判定，不发送header、不执行重采样，也不验证样本来自实时服务端。

Policy默认threshold8/retries2，serde缺字段逐项默认且忽略额外字段。clamp helper分别2..64及0..5，但struct公开字段及反序列化不自动clamp。is_confident仅channel精确thinking且TailRepetition阈值<=max_threshold；不检查max_retries，也不检查阈值下限，0可被解析且判confidence。confident_triggers保留输入顺序和重复raw，不做累计集合去重。

Signal parse按第一个@拆head/channel，head再第一个冒号，精确tail_repetition+u32或无冒号low_logprob，其它Unknown保留head，raw全留；不trim、大小写不归一，channel可空或含额外@。tightest跨所有channel取最低tail阈值，同阈值保留首个，无tail退首raw，空迭代None；不是confidence过滤器。derive Deserialize可直接构造kind/channel/raw互不一致对象，不重新parse raw。

peek先data字面包含doom_loop_check才JSON解析，非法JSON返回None。顶层type精确check即CheckEvent，即使triggers缺失或错误也空Vec；否则只要/response/doom_loop_check/triggers路径存在就ResponseField，不限定completed/incomplete event type。路径值非数组返回空，数组跳过非string但保留空字符串/重复/未知标签。该函数不接受event_name，命名但坏JSON需调用方另用is_check_event判断吞掉。

is_check_event先精确event名即可true而不管data合法性，否则要求data字面包含完整type文本再解析顶层type；只在正文引用type不会误判。字面substring快路径意味着全部用JSON Unicode escape编码关键字的等价payload可能跳过，不能宣称所有等价JSON编码均识别。返回值只是分类，吞掉/转发由调用方落实。

16测试静态读取：标签种类/坏语法/serde、policy默认缺字段、sample精确及累计、坏trigger与非string跳过、terminal字段、confidence三个条件、tightest、event_name/type确认与普通/非法输入。本轮未运行测试。

读取证据：`crates/codegen/sampling-types/src/doom_loop.rs` SHA256 `0256f5bd477b606bb9ee64a16707287eb2a0bfec3fa8b43f79602fac4d542087`。

累计4/7份Rust文件完整读取；尚余messages、types、conversation。

## messages.rs 完整读取

503行含6测试完整读取。MessagesRequest含必需model/messages/max_tokens及可选system/tools/tool_choice/temperature/top_p/top_k/stream/stop_sequences/thinking/output_config/metadata；derive Default不等于serde字段缺失默认，三个必需字段缺失仍失败，程序构造Default可产生空model/零max_tokens，本层不校验合法请求。

MessageRole仅user/assistant；content untagged为string或ContentBlock列表，system为string或TextBlock列表。TextBlock.type和CacheControl.type都是任意String，注释text/ephemeral不是验证，ephemeral构造器才固定值。块支持Text/Image/ToolUse/ToolResult/Thinking/RedactedThinking；前四可带cache_control，Thinking须thinking和signature，redacted仅opaque data。ToolResultContent递归允许任意ContentBlock，未提供is_error字段；未知字段可被忽略，不保证可往返保留。

ImageSource支持base64(media_type/data)及url，均不验证编码/MIME/URL。ToolParam要求name/input_schema，可选description；schema任意JSON。ToolChoice只auto/any/tool name，无none变体。ThinkingConfig enabled带u32 budget、adaptive可选display(omitted/summarized)、disabled；不根据model判兼容或约束budget。OutputConfig effort任意string，可选json_schema format仅schema，无name/strict；Metadata仅user_id。

非流式MessagesResponse必需id/type/role/content/model/usage，type和role是任意String；stop_reason可缺。StopReason七种已知标签加Unknown(String)捕获未知字符串并原样序列化，非字符串不保证接受。MessagesUsage input/output必需，cache creation/read默认0；derive Default不使input/output缺失合法。

MessageStreamEvent仅列message_start/delta/stop、block start/delta/stop、ping/error，无未知event兜底；StreamDelta仅text/input_json/thinking/signature四类。block index为u32但不校验序列关系；partial_json只保存string，不在类型层组装/验证JSON。StreamError.type/message均必需string。

MessageDeltaBody保留stop_reason、可选stop_sequence/stop_details；不检查stop_sequence是否只配StopSequence。StopDetails三个Option字段可缺且未知键忽略，但已知字段错误类型仍反序列化失败，注释“unknown shape never fails”不能泛化。MessageDeltaUsage output_tokens必需，input/cache为可选u32，未设置skip_none会序列化null；与MessagesUsage缺失默认0语义不同。

6测试静态读取：所有stop标签与未知roundtrip、refusal、stop_details未知键、matched stop_sequence、redacted thinking、json schema形态。未运行动态测试，实际流累积与会话投影待其它模块核对。

读取证据：`crates/codegen/sampling-types/src/messages.rs` SHA256 `7ebf18ba542891cc4bd31924ffb4295d9c39fcd7e4050a0279530af4e3ccbb70`。

累计5/7份Rust文件完整读取；尚余types和conversation。

## types.rs 完整读取

1320行含全部测试读取。ChatCompletionRequest构造new设Some model、from_messages不设model，其余选项None；builder赋值不检查temperature/top_p/token范围或工具一致性。此request结构无stream字段，transport如何补入待调用方。message Role仅system/user/assistant/tool；content必需string或text/image_url blocks，null不接受。image URL任意string，不带detail。tool_calls缺失默认空但null未用null-default，区别于流delta。

MessageContent.is_empty仅检查空string/空Vec，不检查Vec内所有text为空；blocks将string复制为一个text block。text_content忽略图片并以LF连接文本；set_text覆盖所有block；append对string直接拼接，对非空Vec追加text，对空content改为string。assistant_tool_call构造空文本和单工具；tool构造tool_call_id。chat_truncate_for_prompt按user计数保留目标prompt及其后直到下一user前的项，超范围保留全部；target+1普通usize加法有极值溢出边界，不自动删除历史。

ToolDefinition/FunctionTool/ModelImageInputKey重导出tools权威定义。ToolChoice string preset接受任意值，便捷auto/none/required；function种类只有Function。请求工具id可缺，响应tool kind任意String且id必需；arguments始终string，from_json只序列化，不做schema验证。

FinishReason五个标准值加Unknown保留原string，未知不丢整chunk但非string仍失败。ChatResponseMessage content/reasoning可缺，tool_calls缺省空；ChatCompletionResponse基本id/object/created/model/choices必需，usage/citations可选。Usage三个token计数必需u32，details字段各自默认0，cost ticks可选i64且本层不归一0/负数，不校验total等于两项和。

ChatCompletionChunk fingerprint空string转None（不trim），choice delta必需。ToolCallDelta index必需u32，其它可选；function delta可部分name/arguments，无累积逻辑。ChatChunkDelta tool_calls缺失/null均空，reasoning_content None序列化null（无skip），未知字段忽略。与非流式tool_calls null行为不同。

CompactionAtTokens untagged bool/u64：false None、true context_window*percent/100、Fixed原值；不饱和或clamp percent，可能乘法溢出。CompactionsRemaining bool/u8：dynamic true无summary1/有summary0，false None，fixed不随summary变。

ReasoningEffort七级none/minimal/low/medium/high/xhigh/max，默认medium。Responses双向逐项映射，Messages none/minimal省略，其余传规范string。FromStr转小写但不trim，serde enum只规范小写；parse_canonical_effort_token沿用FromStr，名称canonical不代表只接受小写。meta.reasoningEffort类型/值错误warn后None。

ReasoningEffortOption裸string沿FromStr归一id/首字符大写label，full对象value走严格serde，id/label缺失才默认，显式空值保留。description可空，default默认false。reasoningEfforts meta需整Vec解析成功且非空，否则整体None（无部分挽救）；没有唯一id、唯一default或重复canonical值校验，“coherent menu”注释不能代替这些约束。序列化失败fallback空数组。

ApiBackend三种默认Chat，supports_native_schema只Chat/Responses true，此为内部策略标志不证明服务端能力。SamplingConfig保存base_url/wire model、输出/采样设置、backend、三份有序header/query/env映射、NonZeroU64 context window、可选reasoning/stream_tool_calls。NonZero阻止零窗口，未自动验证URL/headers或执行env解析；API key未独立字段但extra_headers本身仍可含值。

model_image_input_key以wire model和backend标签加endpoint摘要构造，BLAKE3输入base_url原字节及按key排序的query，每项NUL+key+=+value；不含headers/env/context/settings，不规范化URL。hash不暴露原值但低熵值可猜；未使用长度编码，任意key/value包含分隔符时存在编码歧义，不将其宣称为任意输入的无碰撞路由身份。CreateResponseWrapper仅inner和From/new/Default，无自定义序列化或行为。

全部types测试静态读取：finish扩展、effort serde/menu/meta、remaining配置、content形态及null tool delta；本轮未构建或执行。

读取证据：`crates/codegen/sampling-types/src/types.rs` SHA256 `37155fd9618b80cb761d03596995e596d789e3531b4e3a6ece9e3a5c9da957e6`。

累计6/7份Rust文件完整读取；剩余conversation.rs（10099行）。

## conversation.rs 1–920行阶段读取

本文件尚未读完，不登记全文件完成hash。ConversationItem tagged六类System/User/Assistant/ToolResult/BackendToolCall/Reasoning。VisibleReasoning新写仅text，custom decode优先string text，否则从旧summary/content数组提text并LF拼接，任意坏形态也可空文本，不保留opaque字段。BackendToolCall新写kind+summary，旧native对象取tool_type；未知kind降Other，code interpreter preview取100字符，MCP固定概述。Serde宽松读取不证明所有durable元数据均经过强验证。

SyntheticReason列14项，starts_prompt_turn仅TaskCompleted/SubagentCompleted/NotificationDrain true，其余false；未知标签拒绝。PriorTurnInterrupt三个fatal用户中断标签无unknown。PermissionEvidence tagged DirectUser/Interjection及text，deny_unknown_fields；UserItem可同时携带synthetic、permission evidence、goal tag、cwd generation、prior interrupt、prompt_index，本struct未校验这些字段间注释不变量。

project_conversation_for_goal_scope在传入Vec找最后一个与active goal_id/revision完全相等的User，其余所有有goal tag项将content改占位文本；无active全遮罩，保留item位置与其它metadata，不检查synthetic_reason，不修改调用方另持有的原历史。Assistant携带文本/tool calls/model/fingerprint/effort，旧reasoning字段忽略；fingerprint仅空string归None。ToolResult images可含ContentPart text/image，字段名不强制全是图片。

is_unconditional_image_input_unsupported须image_count>0且HTTP400；先排除格式/损坏/尺寸/透明度/安全政策关键词，再匹配text-only或不支持图片的终结式断言（后缀仅空白/ASCII标点），或image类型名称与unknown variant/expected text等组合。纯字符串启发式，不是provider capability声明解析。

conversation_image_groups每个含图片User/ToolResult一组，保留图片顺序；fingerprint含来源标签及各URL长度LE+字节，不含源text或item index。遍历只关联此前Assistant的同id工具，重复id后者覆盖。User source_text拼text并strip image_files envelope（helper后续待读），tool source_text固定隐藏路径，group保留tool item index/id。

replace_item_images_with_text对User首张图位置插单replacement、其余图删除，所有text剥envelope；即使无图也处理text，permission evidence未改。ToolResult仅removed>0时删除图片、保留并清理images中的text，普通content非空时追加Projected image description；其它item返回0。函数修改传入item，是否作用副本由调用方负责。

redact_projected_image_tool_call找首个指定id，保留id/name，把arguments替换固定JSON，同时整个assistant content变占位；不是仅局部路径改写。tool_result_references需结果id属于给定ids且source为Assistant，从所有匹配call的JSON参数收集source-shaped string，去空/排序去重，替换content及images中text；收集/替换helper尚待读。response_carrier把Reasoning/BackendToolCall整个替换为assistant占位，其它不动。

response_carrier_compaction_references先serialize source收集所有string，加入全string及带URL/斜杠等token，排序去重后委托compaction替换；并非仅opaque字段。image compaction helper从source提reference再委托，具体适用summary和token匹配边界需后续读取，未提前宣称全面脱敏。

下一段从921行继续，函数/测试总体验证未完成。

## conversation.rs 921–1450行阶段读取

compaction引用替换只作用synthetic_reason=CompactionMeta的User文本，references空则不动；逐string全局replace，没有token边界/最长优先，排序后的短引用可先改写长引用，也可能替换普通文本子串。tool source参数收集递归对象时按当前key是否包含path/file/image/url/source重新判断，数组继承flag，嵌套对象不继承上层true；无JSON参数则跳过。carrier收集包括所有string值，不能视为语义精确路径解析。

projected_image_reference_tokens对User收image URL及所有已闭合image_files信封中的数字序号“. ”行路径，ToolResult只image URL，trim空后排序去重。image_files_envelope_paths循环多信封，未闭合停止；strip_image_files_envelope仅去首个闭合信封并trim后文CR/LF，不循环、无XML解析。因此“移除全部envelope”不成立。

assistant_response_carrier_indices须索引指Assistant，只收其前面连续Reasoning/BackendToolCall且保留顺序，遇任意其它角色停止。ToolSpec从ToolDefinition搬name/description/parameters，ToolCall仅保存id/name/arguments，不验证JSON或唯一id。

JsonOutputFormat portable_schema_for_backend对Chat选JsonObject（丢传入schema于该enum），Responses/Messages选JsonSchema。compile_output_schema先序列化检查<=256KiB，再jsonschema compiler配置拒绝外部retrieval、regex size256KiB/DFA2MiB；validate_output_value返回首个验证错误string。编译上限是schema编码大小与regex限制，不证明对所有输入的CPU/内存全面有界。

NativeContinuationFragment三backend枚举不derive serde，token估算序列化字节/4、序列化失败0；Chat empty同时要求content empty、reasoning空和tool_calls空，Responses/Messages仅Vec空。signature只取Messages首个非空Thinking签名，不是所有签名合并。Span start/end与projection portable_prefix_len公开，类型本身不校验区间、顺序、backend或route身份。

ConversationRequest持source_projection审计、items/tools/choice、采样/model/schema/cache key及native projection，无derive serde。image_count按image_groups求和，仅从durable items数，不含native fragment；has_native_continuation只看spans非空，不检查有效性。

project_portable_history开始部分已读：Reasoning删除，System/User clone；无工具Assistant移除model/fingerprint/effort，若trim_start以历史工具模板头开头则整个跳过。含工具Assistant只收紧随的连续ToolResult，以id BTreeMap（重复结果覆盖），缺结果call跳过。其余投影构造尚未读完，从1451行继续，不能提前定义完整portable行为。

## conversation.rs 1451–2560行阶段读取

portable历史完成核对：每个已配对本地工具按call顺序写Tool/Arguments/Result文本，result.images文本追加正文、图片汇入合成User；使用历史工具exchange头标记不可信，但user构造器synthetic_reason与permission evidence均None。Assistant非空正文先保留为无tool/模型元数据项，模板echo前缀则丢正文。连续结果未匹配的丢弃、孤立ToolResult丢弃、BackendToolCall变assistant summary；原工具id不写模板，但arguments/result内容不做脱敏。未知历史语义不能由User角色推导授权。

request_segments无native直接clone原items（不执行portable）；portable_prefix越界或span倒序/重叠/空区间/end越界/backend错则整个历史portable。合法时prefix portable、span用native替代、区间gap和tail原样items。只校验backend与索引，不校验model/endpoint；空spans仍可portable prefix，与has_native_continuation=false同时成立。

统一StopReason含Stop/Length/ToolCalls/ContentFilter/ModelContextWindowExceeded/PauseTurn；Chat未知finish归Stop，工具存在升级逻辑不在该From。TokenUsage Chat转换直接copy总数、reasoning/cached details缺失0、cache_creation0；record_on_span只四项，不记录total或cache_creation。

ConversationResponse公开字段允许任意item序列，assistant()/mut反向找最后Assistant而非验证末项；reasoning_items扫描全部位置，backend同理。empty_reason无Assistant立即NoVisibleContent，即便有reasoning；有Assistant只检查精确空文本和本地tool_calls，空白文本非空，backend工具不单独豁免；空assistant+任意非空reasoning判ReasoningOnly，不查stop_reason/usage。fallback_text仅message_chunks_emitted=0且最后assistant文本非空时返回，不判断已发文本是否完整。reported_cost_ticks仅保留>0。

构造器逐项已读：User/UserWithParts没有权限证据；synthetic对应reason独立，interjection显式permission_text可与model content不同；goal构造接受任意SyntheticReason；所有prompt_index初始None，TaskCompleted等constructor不自行赋turn index。project instruction“不可替换”只注释不由构造器强制。set_prior_turn_interrupt/set_prompt_index/set_permission_evidence对任意User赋值，不检查是否真实用户/合成类别；非User no-op。

text_content对ToolResult只返回普通content，不包含images内text；User拼text LF；Backend/Reasoning返回可见文本。compaction trait仅is_tool_result。reasoning_text_from_response_item先summary后content拼LF。inject_streaming_reasoning_fallback空输入不动，任意现有reasoning非空则全不动，否则覆盖首个空reasoning；无reasoning插最后Assistant前、无Assistant追加末尾，不重建native签名。

ChatRequestMessage→ConversationItem：System仅文本丢图片；User保留text/image但metadata全默认；Assistant丢reasoning_content并取text和tool calls，缺id为空，模型字段空；Tool仅text，缺tool_call_id为空且不保留图片。转换不是无损双向桥。

truncate_bytes向前退UTF8字符边界，不保字素。sanitize_tool_arguments以IgnoredAny验证完整JSON，非法记录前200byte安全字符前缀并返回{}；合法任意JSON值通过，不要求object/schema。未在此检查配对result确实是parse error，注释不能代替调用条件；函数只有返回值，不自行改持久化历史。单项chat转换函数开始已读，具体match剩余从2561继续。

## conversation.rs 2561–3160行阶段读取

单项Chat转换User无图时text按LF折叠为string，有图保留block顺序；Assistant每个tool args sanitize，不传durable reasoning/model metadata；ToolResult无images用string，有images先普通text再Image，images中的Text被丢弃。Backend摘要作assistant；公开单项函数收到Reasoning仍unreachable panic，只有batch入口先filter保证。ChatResponseMessage→Assistant不看传入role，丢reasoning/citations/tool_call_id等，保留content和tool calls。

response_to_conversation_items先遍历全部FunctionCall验证：response必须Completed或Incomplete；call.status Completed可接受，缺status仅response Completed接受；其它status或非法JSON args拒绝整response为Serialization。无FunctionCall时该前置不拒绝其它response status。随后聚合所有Message OutputText到末Assistant（非空已有内容时加LF），拒绝/其它message content忽略；FunctionCall用call_id而非item id；非空可见Reasoning保留兄弟项，CodeInterpreter/McpCall保留摘要，其它OutputItem忽略。必追加一个Assistant含model、非空metadata fingerprint及echo effort。不保持Message和tool在输出数组中的完整交错顺序，而是兄弟reasoning/backend+末聚合Assistant。

responses_native_fragment保留typed Message、FunctionCall、Reasoning及File/WebSearch、Computer、ImageGeneration、CodeInterpreter、LocalShell、McpList/Approval/Call、CustomToolCall。只FunctionCall和Reasoning清status，其它clone。Function/Computer outputs、Compaction、Shell及ApplyPatch call/output、CustomToolCallOutput、ToolSearch call/output跳过；该函数不执行前述completion/JSON校验。附带单测已读，覆盖7种response/item status组合，但尚未执行。

ConversationRequest→Chat应用request_segments，合法native Chat整message原样插入（不走durable args sanitize）。tools空则None并清tool_choice；非空映射全部ToolSpec。JSON schema直接请求可写structured_output name+strict true；portable helper才选择JsonObject。采样字段转移，frequency/presence/user None，不传prompt_cache_key/source_projection。

Responses CreateResponse同样segment投影；tool_choice即便tools空也保留，区别于Chat。json format在text里，schema name固定strict true。reasoning始终Some且summary Concise，即便effort None；prompt_cache_key保留。instructions、stream、store、previous_response_id等None，不能从这里声称已发stream或启用持久化。build_responses_input总返回Items，native原样Vec接入。

patch_reasoning_text_types只处理顶层input数组里type=reasoning项的content对象，缺type才插reasoning_text，已有错误type也不纠正；不递归任意嵌套，也不改summary。Responses逐item转换已读System/User入口，其余从3161继续。

### 续读 conversation.rs:3161–4330

本轮逐行读取以上范围（此前 3161–3740 的截断输出不计入阅读），生产实现已读至结束，测试继续从 4331 行读取。未将整个 crate 标记完成，尚未运行本 crate 测试。

- Responses Assistant 先输出非空文本，再逐项输出 FunctionCall；arguments 经过 sanitize，id/status/namespace 为空。Reasoning 不输出，BackendToolCall 转 assistant 摘要。
- Responses ToolResult 无 images 时输出字符串；有 images 时以正文文本块开头，仅保留 Image 项，忽略 images 中 Text，图片 detail Auto、file_id 空。用户恰好一个 Text 使用字符串，其余（包括空数组）使用 ContentList；工具参数 Some，strict/defer_loading 空。
- Request builder 仅赋值，无温度/token/schema 校验。
- prompt 截断：无 marker 使用 legacy 计数（首个非 synthetic User 当 preamble）；首 marker 与 legacy 前缀计数不连续时只按 marker；连续时首 marker 前 legacy、之后只数 marked User。返回首个有效 index >= target 的位置，否则 len，未排序或校验 marker 单调性。
- CWD 转换对 System/User Text/Assistant 文本及工具参数/ToolResult 正文/Reasoning 执行原始 substring replace；忽略图片、ToolResult images 中 Text、BackendToolCall 和元数据。没有路径边界或 JSON 重新编码保护，不能将注释中的 safe 当作任意输入保证。
- dangling 检测和修复仅认每个带 tool_calls 的 Assistant 紧邻连续 ToolResult 段；缺失调用按原调用顺序在该段末尾补结果，逆序 splice 保持位置。相同缺失 ID 的重复调用可各补一项；不跨其他 item 查找。三个原因分别使用 user cancelled、process interrupted、harness halted(class) 文案，未实际判断工具是否执行。
- 结果去重只处理上述相邻段，按 tool_call_id 留最后出现项（不验证确为真实结果，也不要求 ID 属于该 Assistant），其他段不动；返回移除数。
- Messages cache：最后 system block ephemeral；从尾找可标记 message 的最后一个非 thinking/redacted block；纯文本提升为 Text block。再从 tip 前寻找最后 Assistant 之前最后 User 标记。没有全局清除已有 native cache 标记，不能保证任意输入总数最多三个。
- Messages ID 清洗实际用 char::is_alphanumeric，保留 Unicode 字母数字及 _/-，其余换 _；没有碰撞检测。用户图片 data: 按第一个逗号切分，header 不以 ;base64 结尾默认 image/png；缺逗号降文本；小写 http(s) 转 URL；其他降文本，不验证 base64。工具结果图片只有 data: 且含 ;base64, 才转 Base64，其余原样 URL；忽略 images 中 Text。
- Messages 连续 Assistant 累积块，ToolResult 累积为 User 消息；遇 User/System flush，不做通用相邻同角色合并。System 收集到独立 system；durable Reasoning 丢弃，native Messages 块原样独立 Assistant。工具 args 任意合法 JSON 接受、无效降 {}。
- Messages 无 tools 时 tools=None 但 tool_choice 仍映射；None choice 映射 Auto。有可映射 effort 时 Adaptive+Summarized；JSON Object 转 object schema，JSON schema 原样传递。缺 model 空字符串、缺 max_tokens=0；stream/top_k/stop_sequences/metadata 空，不在此校验请求有效性。
- MessagesResponse 转单 Assistant：Text 用 LF 聚合，ToolUse input JSON 编码；thinking/redacted/image/ToolResult 忽略，model 保留，fingerprint/effort 空。不检查 role/stop_reason。
- 已读测试开头：compaction bridge、外部 schema ref 拒绝、goal latest directive/shadow metadata 保留、interrupt 三变体 serde roundtrip（下一测试待续）。这些是静态阅读，不是测试执行结论。

磁盘检查：本工作区 target 不存在，可用约 69 GiB；未触碰其他工作区构建目录。

### 续读 conversation.rs:4331–6880（测试证据）

本轮完整读取以上连续范围，下次从 6881 继续；尚未执行测试，不增加 reviewed crate 数。

- interrupt 未知字符串拒绝，None 字段省略，setter 在非 User 无操作；文本及工具调用 Chat roundtrip、模型参数与三协议 JSON Schema/Object 映射均有具体断言。
- Responses model/effort 持久化有断言；原生 reasoning 的 encrypted_content、native id 只在 native fragment 留存，durable 只存可见摘要，纯加密 reasoning 不生成 durable Reasoning。native Reasoning status 被清除，Message status 保留。
- 旧 native timeline 反序列化为可见事实后，序列化丢弃 opaque id/container/status/output/extension；这证明迁移读取行为，不意味着继续持久化旧协议。
- portable 三协议测试验证历史工具对转为 User 文本及图片，保留结果中的 Text 占位；丢弃 reasoning、源模型/fingerprint、调用 ID 和工具协议结构。旧 assistant 历史模板 echo 被丢弃；不完整调用仅保留助手可见文本。
- native Responses span 携带加密推理有单独正例；名为 only_encrypted_reasoning_included_in_request 的测试实际上断言没有 native span 的 durable reasoning 不进入请求，不按过时测试名扩张语义。
- Chat 有 tools 时四种 choice 保留，无 tools 时清除 choice；Responses choice 测试不要求 tools。多图顺序、特殊字符和复杂 JSON 原串保留；非法工具 JSON 在 Chat/Responses 投影为 {}，不证明修改 durable 原事实。
- effort 七值 Chat 顶层/Responses nested 映射；Messages Low/Medium/High/Xhigh/Max 配对 adaptive，None/Minimal 不产生 effort/thinking（无 JSON 输出时）。unset effort 省略；summarized display 有具体断言。
- cache 测试证明普通生成请求三个断点、跳过尾部连续 User 找前轮 tip、图片尾块可标记、native thinking 不标记而 Text 标记；未覆盖任意 native 预存 cache_control 总数，不能用三个断点测试证明全局上限。
- btw 系列在测试内手写准备函数（过滤 Reasoning、循环移除尾部带调用 Assistant/ToolResult、追加问题），检验三协议无 call_2、保留 call_1 对、无 reasoning 和默认温度。fixture 的所谓 with thinking Assistant 本身不含 Reasoning；这些测试不是 shell handle_side_question 运行证据，后续 shell crate 必须核对真实实现。
- UTF-8 截断 CJK/emoji/零长度边界和非法非 ASCII 参数日志预览回退已有断言；最后一个有效非 ASCII 测试断言在 6880 行尚未结束，下一轮接读。

### 完成 conversation.rs:6881–10099 连续阅读

至此 manifest 和 7 个 Rust 文件均已逐行阅读；规范合同尚待整理登记，crate 状态继续 pending。

- 100KB 工具结果投影不截断有断言。prompt rewind 覆盖空/无 User、legacy preamble、非 turn synthetic 跳过、task/notification 起始、compaction 高绝对 marker、连续 legacy→marker 升级以及 marker 后无编号 phantom 跳过。早期测试注释把首 User 写作 prompt 0，实际首 User 被当 preamble，规范依实现计数。
- CWD 文本/参数/Reasoning 正反转换、多项替换与无匹配均测试；partial-match 测试明确断言 myproject-extra 也改写。名为 empty_source_noop 的测试实际使用相同非空 source/target，不能证明空 source 无操作。
- Response 可见文本或工具调用非空、无 Assistant/no content/ReasoningOnly、fallback chunks=0 且正文非空、仅工具无 fallback、六个 stop reason 映射及 serde。早期 reasoning-only fixture 没有 Reasoning sibling；后续 empty_reason_reasoning_only 才实际覆盖该形状。
- dangling 全扫描、部分完成、多个并行、历史多处、插入在邻接段后/下一 User 前、harness class 文案以及重复调用零修改有测试。dedup 保留最后、跨多 Assistant 独立处理、空/无调用/重复执行无操作。
- image_count 用户+工具图片；工具图片分组隐藏源文本、关联 call index；替换保留 images 中 Text、正文其他内容与调用 ID；显式调用后续 redaction 才删除参数和正文已知路径。carrier 连续 Reasoning/Backend 变通用 Assistant，不会自动消除工具参数路径。用户转录文本不修改已有 PermissionEvidence，首图片 envelope 从显示正文去掉。
- 三协议工具图片 text+image 结构及无图字符串、持久化 images 空省略/有值往返；synthetic 已移除 doom_loop_warning/未知值拒绝，cwd generation 与 prompt marker 缺失默认 None；各 synthetic 构造器标记和内容、session_rules/memory_context/truncation_continue 非新 turn。
- Responses metadata system_fingerprint 保留；durable Reasoning 在多轮/尾部/被 User 隔开均从普通 wire 丢弃。patch 只补缺少 type，不覆盖未来 discriminator。
- prefix 测试比较 JSON Value 数组前缀；serialization_determinism 额外验证重复串相同及 type<role<content 插入顺序。reasoning_sibling helper 显式忽略 id/encrypted 参数，相关测试并未测试加密 native 前缀稳定；部分注释仍声称 reasoning inline，实际断言为零 reasoning，规范以断言和生产实现为准。

下一步：运行该包现有测试，整理能力合同与来源映射，然后才能标记 reviewed。

### 动态测试结果

`cargo test --locked --offline -p sampling-types --all-features` 成功：264 passed、0 failed、0 ignored；doc-tests 0。完整日志 `/tmp/grow-sampling-types-tests.log`。此结果证明当前包现有测试通过，不将静态审阅发现但未覆盖的边界说成已动态测试。测试完成后立即执行本工作区 `cargo clean`，不清理其他工作区。

### 合同整理第一批

新增 `sampling-types-features.draft.json`，当前 12 条合同，覆盖 prompt marker 截断、CWD 子串转换、邻接工具结果修复/原因/去重，以及 Messages ID、两种图片解析、消息排列、缓存断点、默认请求和响应投影。每条均含场景和源码符号，已检查来源路径及符号存在。该文件是待汇总草稿，不计入 feature-map 或 reviewed 数；其余类型、错误、doom-loop、图片脱敏、native lane 和 Chat/Responses 合同继续整理。

### 合同整理第二批

草稿增至 22 条，本批新增 10 条：凭据来源、错误分类、流式分类优先级、状态谓词、API/reqwest 两套重试条件、独立 veto、过载、用户文案、序列化错误重建。重读 error.rs:330–420、583–609 核对 retry/veto 的真实实现；所有草稿来源符号存在、条目名称唯一，diff check 通过。尚未计入正式映射，继续补齐其余功能。

### 合同整理第三批

草稿累计 32 条。本批加入 doom-loop 默认/clamp、confidence、标签语法、tightest、payload/event 分类及容错数组，另有空字符串归一、三态 optional 和 50MiB 常量边界。重读 doom_loop.rs:150–277、完整 serde_helpers/lib，来源符号及名称唯一性检查通过。该批未构建，无新增 target；仍需完成 messages/types 结构与其余 conversation 能力映射。

### 合同整理第四批

草稿累计 42 条。本批补充 Messages 的必需请求字段、content/system、六类块、图片/工具定义、tool choice、thinking/schema、stop reason、usage 默认、流式事件、stop details。来源符号及名称唯一性检查通过，diff check 通过；未将结构类型能力误写为 HTTP 请求或流式执行保证。仍待 types 与 conversation 剩余条目，crate 不标记完成。

### 合同整理第五批

草稿累计 58 条，新增 types 的 Chat 请求/内容/工具/finish/usage/stream、两种 compaction header 解析、effort 与 menu、endpoint 配置和 image key、native schema 策略标志及 wrapper。已逐项检查来源符号和重复名称，diff check 通过。剩余工作集中在 conversation 核心事实、图片投影、native lane 与 Chat/Responses 转换；尚未正式登记 reviewed。

### 合同整理第六批

草稿累计 73 条，补充六类 durable item、旧可见推理/后台工具读取、synthetic/permission 元数据、goal scope、图片分组/拒绝启发式/替换/调用和引用脱敏/carrier/compaction 范围、构造器及推理 fallback。所有来源符号检查通过，diff check 通过；原生续接和 Chat/Responses 投影仍待整理，保持 pending。

### 合同整理第七批

草稿累计 89 条，加入 schema 限制、native fragment 与 span 校验/拼装、portable 历史、response 空结果/usage/cost、Chat 直接投影、参数 sanitize、Responses 完成证据/输出聚合/native allowlist/输入及 discriminator patch。重读 schema 编译和 request_segments 实现，来源符号、唯一性及 diff check 通过。下一步对照完整 ledger 查漏（尤其请求顶层配置和反向转换），然后汇总正式 delta，不能仅因条目数足够就标记完成。

### 合同整理第八批

草稿累计 95 条，补齐 Chat 反向请求/单响应转换、Chat/Responses 顶层配置、portable schema 策略与 UTF-8 前缀。重读实际转换和 schema 策略，明确 Chat portable 选 JsonObject、Responses/Messages 选 JsonSchema；修正 Usage 字段为源码准确名称，补 cost 独立来源。下一轮进行完整草稿对照与正式汇总登记；尚未修改 reviewed 状态。


### 正式登记 sampling-types

全部 7 个 Rust 文件（13615 行）及 Cargo.toml 已逐行读取。95 条合同已汇入 feature-map 与 model-sampling delta，包含类型、错误、doom-loop、图片、native/portable 投影和三个协议转换；crate-inventory 保存全部文件 SHA256。此前阶段记录的未读/未测试状态由后续完成记录覆盖。现有测试 264 通过、0 失败、0 忽略，doc-tests 0；测试后 cargo clean 清理 2.9GiB。运行时风险仅记录边界，不修改产品代码。

- Prompt rewind marker authority
- Conversation cwd substring projection
- Adjacent tool result repair
- Dangling tool result cause text
- Adjacent tool result deduplication
- Messages tool call identifier projection
- Messages user image source parsing
- Messages tool result image source parsing
- Messages pending block chronology
- Messages explicit cache breakpoint placement
- Messages request defaults and output configuration
- Messages response visible fact projection
- Credential provenance classification
- Sampling failure taxonomy
- Stream error classification precedence
- Sampling status category predicates
- Sampling retry eligibility
- Reqwest retry eligibility
- Sampling retry veto separation
- Sampling overload predicate
- User facing upstream error rendering
- Sampling serialization error reconstruction
- Doom loop policy defaults and explicit clamping
- Doom loop confidence filter
- Doom loop trigger grammar
- Doom loop tightest diagnostic label
- Doom loop payload classification
- Doom loop tolerant trigger arrays
- Doom loop named event recognition
- Empty string optional normalization
- Three state optional update decoding
- Encoded sampling request size constant
- Messages request required wire fields
- Messages content and system shapes
- Messages typed content blocks
- Messages image and tool definitions
- Messages tool choice type boundary
- Messages thinking and structured output types
- Messages response stop reason preservation
- Messages usage field defaults
- Messages stream event vocabulary
- Messages optional stop details
- Chat request builder field semantics
- Chat message content wire semantics
- Chat content extraction and mutation
- Chat prompt truncation boundary
- Chat tool call argument representation
- Chat finish reason unknown preservation
- Chat usage arithmetic boundary
- Chat stream delta partial fields
- Compaction header configuration resolution
- Remaining compaction configuration resolution
- Reasoning effort parsing and backend projection
- Reasoning effort menu parsing
- Sampling endpoint configuration boundary
- Backend native schema policy flag
- Model image input endpoint identity
- Responses request wrapper delegation
- Durable conversation item taxonomy
- Visible reasoning one way decoding
- Backend tool visible summary decoding
- Synthetic prompt origin classification
- Permission evidence typed origin
- Goal directive current scope projection
- Conversation image group identity
- Unconditional image rejection heuristic
- Image replacement preserves permission evidence
- Projected image tool call redaction
- Projected image reference redaction boundary
- Image response carrier selection and replacement
- Image compaction reference scope
- Conversation constructors and metadata setters
- Streaming reasoning fallback insertion
- Output schema local validation limits
- Native continuation fragment boundary
- Native continuation span validation
- Native and neutral request segment assembly
- Portable historical tool exchange projection
- Portable historical duplicate and echo handling
- Response assistant and empty result selection
- Response fallback text and reported cost
- Unified usage and stop reason conversion
- Chat direct conversation projection
- Tool argument wire sanitization
- Responses function completion validation
- Responses output visible fact aggregation
- Responses native fragment allowlist
- Responses input text and tool output projection
- Responses reasoning discriminator patch
- Chat request reverse durable conversion
- Chat response single item conversion
- Chat request top level projection
- Responses request top level projection
- Portable structured output mode selection
- UTF8 byte prefix truncation
