## ADDED Requirements

### Requirement: Shared approximate token arithmetic
Token 估算 SHALL 使用 UTF-8 字节数整除 4，而非 Unicode 字符数；反向预算采用饱和乘 4，每张图片采用 765 的近似 token 成本。

#### Scenario: 不足四字节
- **WHEN** 字符串字节数小于 4
- **THEN** estimate_tokens 返回 0，不把估算当 Provider 实际用量。

#### Scenario: 预算溢出
- **WHEN** estimate_chars 或 estimate_image_tokens 的乘法溢出
- **THEN** 结果饱和到 u64 上限。

证据：`crates/codegen/token-estimation/src/lib.rs` — `estimate_tokens`。

### Requirement: Context percentage display arithmetic
上下文占比 SHALL 在 total 为零时返回零、最高为 100%；分别提供浮点占比、四舍五入整数和截断整数，剩余 token 使用饱和减法。

#### Scenario: 半值边界
- **WHEN** used=85 且 total=200
- **THEN** rounded 返回 43，truncated 返回 42；used 超过 total 时 free_tokens 为 0。

证据：`crates/codegen/token-estimation/src/lib.rs` — `usage_percentage_truncated_u8`。

### Requirement: Prompt rewind marker authority
会话截断位置 SHALL 优先使用显式 prompt_index；首 marker 与 legacy 前缀计数连续时允许前缀 legacy 计数，否则仅使用 marker。首 marker 之后未标记 User 不开启新 turn。

#### Scenario: 压缩重建前缀
- **WHEN** 首 marker 为 11 且前缀 legacy 计数不为 11，目标为 12
- **THEN** 返回首个 marker >= 12 的位置；不存在则返回会话长度。

#### Scenario: 无 marker
- **WHEN** 整个会话没有 prompt_index
- **THEN** 首个非 synthetic User 作为 preamble；后续普通 User 和 starts_prompt_turn synthetic 计数，返回目标起点位置。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `conversation_truncate_for_prompt`。

### Requirement: Conversation cwd substring projection
CWD 转换 SHALL 对 System、User Text、Assistant 正文及工具参数、ToolResult 正文和 Reasoning 文本执行原始子串替换，不解析路径边界或重新编码 JSON。

#### Scenario: 同前缀路径
- **WHEN** source=/a/proj，文本包含 /a/proj-extra
- **THEN** 替换同时影响 /a/proj-extra；不承诺只匹配目录边界。

#### Scenario: 图片与元数据
- **WHEN** 图片 URL、工具结果 images 或元数据包含 source
- **THEN** 这些字段不在该转换函数的改写范围内。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `transform_conversation_cwd`。

### Requirement: Adjacent tool result repair
工具调用修复 SHALL 扫描所有带调用的 Assistant，仅认其后紧邻的连续 ToolResult 段；缺失调用按原调用顺序在该段末尾补 synthetic result，返回新增数量。

#### Scenario: 中间隔开
- **WHEN** 同 ID 结果与 Assistant 之间存在 User 或其他 item
- **THEN** 该远处结果不算已回答，邻接段后仍补结果。

#### Scenario: 再次修复
- **WHEN** 已补结果后再次调用修复
- **THEN** 现有邻接结果满足调用，不再补同一调用。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `repair_dangling_tool_calls`。

### Requirement: Dangling tool result cause text
修复结果文案 SHALL 由调用方传入的 UserCancelled、ProcessInterrupted 或 HarnessHalted(class) 决定；本层不探测工具实际执行状态。

#### Scenario: 进程中断
- **WHEN** 原因是 ProcessInterrupted
- **THEN** 文案说明结果记录前中断且工具可能尚未开始。

#### Scenario: 内部终止
- **WHEN** 原因是 HarnessHalted 并带 class
- **THEN** 文案包含 class 和工具名；不自动推断为用户取消。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `synthetic_dangling_result_text`。

### Requirement: Adjacent tool result deduplication
结果去重 SHALL 仅处理带调用 Assistant 后的连续结果段，按 tool_call_id 保留最后出现项并返回移除数，不检查最后项是否真实执行结果。

#### Scenario: 重复结果
- **WHEN** 同段中旧结果与新结果使用同 ID
- **THEN** 删除前项，保留最后项；不同 Assistant 段独立处理。

#### Scenario: 孤立结果
- **WHEN** 没有紧邻带调用 Assistant 的结果项
- **THEN** 本函数不处理该孤立段。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `dedup_duplicate_tool_results`。

### Requirement: Messages tool call identifier projection
Messages 投影 SHALL 将工具调用和结果 ID 中非 Unicode 字母数字且非 _/- 的字符替换为 _，不检测替换后的 ID 碰撞。

#### Scenario: 标点与中文
- **WHEN** ID 同时包含冒号和中文字母
- **THEN** 冒号替换为 _，is_alphanumeric 接受的 Unicode 字符保留。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `build_messages_request`。

### Requirement: Messages user image source parsing
Messages 用户图片投影 SHALL 按 data:、小写 http(s) 和其他格式分支处理；data: 以首逗号切分，不能剥离 ;base64 的 header 使用 image/png，不校验编码内容。

#### Scenario: 缺少逗号
- **WHEN** data: URL 没有逗号
- **THEN** 生成 invalid image 文本块。

#### Scenario: 未知 scheme
- **WHEN** 图片 URL 不以 data: 或小写 http(s) 开始
- **THEN** 生成 image 文本占位，而非 ImageSource。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `build_messages_request`。

### Requirement: Messages tool result image source parsing
Messages 工具结果有 images 时 SHALL 构造正文 Text 加 Image 块；仅 data: 后含 ;base64, 的 URL 转 Base64，其余原样作为 URL，images 中 Text 不进入该直接投影。

#### Scenario: 混合 images
- **WHEN** images 包含 Text 占位和一张 Image
- **THEN** 正文保留，输出图片，跳过占位 Text。

#### Scenario: 不完整 data URL
- **WHEN** 工具图片以 data: 开始但没有 ;base64,
- **THEN** 保留为 URL source，不采用用户图片的文本回退。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `build_messages_request`。

### Requirement: Messages pending block chronology
Messages 构建 SHALL 累积连续助手块和连续工具结果块，分别输出 Assistant 与 User；User/System 边界触发 flush，System 汇入独立 system，durable Reasoning 不重建 Thinking。

#### Scenario: 相邻用户输入
- **WHEN** 连续两个 User item
- **THEN** 分别产生 User 消息，不执行通用同角色合并。

#### Scenario: 原生 Messages 片段
- **WHEN** 有效 native segment 提供非空块
- **THEN** 先 flush pending，再单独输出原生 Assistant 块。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `build_messages_request`。

### Requirement: Messages explicit cache breakpoint placement
Messages 缓存标记 SHALL 写入最后 system block、从末尾找到的可标记 message 最后非 thinking 块，以及 tip 前最后 Assistant 之前的最后 User；不清除已有 native cache 标记。

#### Scenario: 尾部为图片
- **WHEN** 可标记消息最后块是 Image
- **THEN** 标记该 Image 为 ephemeral。

#### Scenario: 只有 thinking 的消息
- **WHEN** 反向扫描到仅 Thinking 或 RedactedThinking 的消息
- **THEN** 继续向前寻找可标记消息；纯文本消息提升为 Text block。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `apply_cache_breakpoints`。

### Requirement: Messages request defaults and output configuration
Messages 请求 SHALL 在 model 缺失时使用空字符串、max_output_tokens 缺失时使用 0；JSON Object 映射 object schema，有可映射 effort 时启用 Adaptive 和 Summarized，JSON 输出独立于 thinking。

#### Scenario: 无工具但指定选择
- **WHEN** tools 为空且 tool_choice 已设置
- **THEN** tools=None，仍映射 choice；None choice 映射 Auto。

#### Scenario: 无有效 effort
- **WHEN** effort 缺失或为 None/Minimal，且无 JSON 输出
- **THEN** thinking 和 output_config 都为空；不在本层校验请求可被服务端接受。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `build_messages_request`。

### Requirement: Messages response visible fact projection
单项 MessagesResponse 转换 SHALL 聚合 Text 和 ToolUse 为一个 Assistant，文本按非空累计正文插入 LF，工具 input 编码 JSON，保留 model，丢弃其他块及 fingerprint/effort。

#### Scenario: 推理块
- **WHEN** 响应包含 Thinking 或 RedactedThinking
- **THEN** 该单项转换不保留推理；流式 sibling 路径属于其他消费者。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `impl From<crate::messages::MessagesResponse>`。

### Requirement: Credential provenance classification
SentCredential SHALL 区分 Sent、Missing、Unknown；默认 Unknown，未知字符串反序列化为 Unknown，from_sent_fragment 仅按 Option 是否有值判断。

#### Scenario: 空凭据片段
- **WHEN** 传入 Some 空字符串
- **THEN** 仍为 Sent，不在本层验证请求实际发送凭据。

#### Scenario: 错误 JSON 类型
- **WHEN** 凭据来源字段为非字符串
- **THEN** 反序列化失败，不作为 Unknown 接受。

证据：`crates/codegen/sampling-types/src/error.rs` — `SentCredential`。

### Requirement: Sampling failure taxonomy
SamplingError SHALL 分离认证、配置、持久化、HTTP、序列化、API、事件流、空响应、闲置超时和 doom loop；API 错误可携带模型元数据及重试提示。

#### Scenario: 非 API 错误
- **WHEN** 查询非 API 错误的 metadata/retry_after/should_retry_header
- **THEN** 返回 None，不从错误文本生成这些提示。

证据：`crates/codegen/sampling-types/src/error.rs` — `SamplingError`。

### Requirement: Stream error classification precedence
流式错误类型 SHALL 将非 ASCII 字母数字替换为下划线并小写，按认证、计费、权限、未找到、体积、限流、过载、非法请求、默认服务端错误的顺序启发式分类。

#### Scenario: 认证标签
- **WHEN** 类型命中 authentication 或 invalid_api_key 或精确 unauthorized
- **THEN** 返回 Auth，来源 Unknown。

#### Scenario: 未知标签
- **WHEN** 类型不命中已知规则
- **THEN** 返回 API 500，保留原类型与消息，不保证未知 provider 语义分类准确。

证据：`crates/codegen/sampling-types/src/error.rs` — `from_stream_error`。

### Requirement: Sampling status category predicates
错误类别谓词 SHALL 仅将 Auth 或 API 401 判为认证错误，将 API 429 判为限流、API 413 判为体积超限；403 不自动变为认证错误。

#### Scenario: 权限错误
- **WHEN** 错误是 API 403
- **THEN** is_auth_error 为 false。

#### Scenario: HTTP 封装状态
- **WHEN** 错误以 Http variant 携带响应状态
- **THEN** 这些 API 专用状态谓词不据此分类为限流或体积超限。

证据：`crates/codegen/sampling-types/src/error.rs` — `is_auth_error`。

### Requirement: Sampling retry eligibility
is_retryable SHALL 对 API 仅接受 429/500/502/503/504/520/529，对 EventStream、EmptyResponse、DoomLoop 返回 true，对 Auth/配置/持久化/序列化/IdleTimeout 返回 false；HTTP 另用 reqwest 判定。

#### Scenario: API 状态不在名单
- **WHEN** API 状态为 501
- **THEN** 返回 false；不泛化所有 API 5xx。

#### Scenario: 存在禁止提示
- **WHEN** 可重试状态带 should_retry=false
- **THEN** is_retryable 本身不应用 veto，调用方必须另查 veto。

证据：`crates/codegen/sampling-types/src/error.rs` — `is_retryable`。

### Requirement: Reqwest retry eligibility
reqwest 错误重试判定 SHALL 优先接受 timeout/connect；有状态时仅 5xx 或 429，其他 request/body 错误接受，其余拒绝。

#### Scenario: HTTP 501
- **WHEN** reqwest 错误有 501 状态且无更早分类命中
- **THEN** 可重试，与 API variant 白名单不同。

证据：`crates/codegen/sampling-types/src/error.rs` — `is_retryable_reqwest`。

### Requirement: Sampling retry veto separation
is_retry_vetoed SHALL 独立判断 should_retry=false、上下文长度错误启发式或 API 413；should_retry=true 不将不可重试错误强制变为可重试。

#### Scenario: 过大请求
- **WHEN** 错误为 API 413
- **THEN** veto=true。

#### Scenario: 上下文超限
- **WHEN** API 消息命中固定上下文长度特征
- **THEN** veto=true，不依赖具体 API 状态为 400。

证据：`crates/codegen/sampling-types/src/error.rs` — `is_retry_vetoed`。

### Requirement: Sampling overload predicate
is_overloaded SHALL 仅接受 API 529，或 API 5xx 消息包含 overloaded/service_unavailable_error；不把所有可重试错误当作过载。

#### Scenario: 4xx 文本提过载
- **WHEN** API 400 消息包含 overloaded
- **THEN** 返回 false。

证据：`crates/codegen/sampling-types/src/error.rs` — `is_overloaded`。

### Requirement: User facing upstream error rendering
用户错误文案 SHALL 优先提取结构化错误，trim 后最多保留 280 个 Unicode 字符并追加省略号；未解析的 HTML/plain body 不直接展示，使用状态对应或通用上游文案。

#### Scenario: 非结构响应
- **WHEN** 错误响应正文为 HTML 或非法 UTF-8
- **THEN** 不原样展示正文。

#### Scenario: 流式原错误
- **WHEN** 调用流式错误解析入口
- **THEN** 该入口保留原始消息，不承诺同样的 280 字符截断。

证据：`crates/codegen/sampling-types/src/error.rs` — `user_facing_api_error_message`。

### Requirement: Sampling serialization error reconstruction
序列化错误重建 SHALL 保留 Serialization variant，通过共享 display 前缀移除一次避免重复前缀；不将持久化过的错误字符串改判为 HTTP 错误。

#### Scenario: 已有前缀
- **WHEN** 从已经展示过的 serialization 文案重建
- **THEN** 去掉一次共享前缀后重建。

证据：`crates/codegen/sampling-types/src/error.rs` — `serialization_from_rendered`。

### Requirement: Doom loop policy defaults and explicit clamping
DoomLoopRecoveryPolicy SHALL 默认阈值 8、重试次数 2；缺失字段逐项默认，clamp 辅助分别限制到 2..64 与 0..5，公开字段赋值和 serde 不自动 clamp。

#### Scenario: 直接输入越界策略
- **WHEN** 反序列化超过 clamp 范围的 u32 值
- **THEN** 保留该值，只有显式调用 clamp 才收敛范围。

证据：`crates/codegen/sampling-types/src/doom_loop.rs` — `DoomLoopRecoveryPolicy`。

### Requirement: Doom loop confidence filter
confidence SHALL 仅接受 channel 精确 thinking、kind 为 TailRepetition 且 threshold <= max_threshold 的信号；不检查重试预算，也不检查阈值下限。

#### Scenario: 零阈值
- **WHEN** thinking 信号 threshold=0
- **THEN** 在通常 max_threshold 下仍为 confident。

#### Scenario: 累计重复
- **WHEN** 输入含重复的 confident raw 标签
- **THEN** confident_triggers 保留顺序和重复，不自行去重。

证据：`crates/codegen/sampling-types/src/doom_loop.rs` — `is_confident`。

### Requirement: Doom loop trigger grammar
触发标签解析 SHALL 按首个 @ 切 channel、首个冒号切 kind 参数；接受 tail_repetition 的 u32 和无冒号 low_logprob，其他语法保存 Unknown 与原 raw，不 trim 或归一大小写。

#### Scenario: 多个分隔符
- **WHEN** raw 包含多个 @
- **THEN** 首个 @ 后完整余串作为 channel。

#### Scenario: 非法阈值
- **WHEN** tail_repetition 参数无法解析为 u32
- **THEN** 返回 Unknown，不抛解析错误。

证据：`crates/codegen/sampling-types/src/doom_loop.rs` — `pub fn parse`。

### Requirement: Doom loop tightest diagnostic label
tightest SHALL 在所有 channel 中选择最小 TailRepetition 阈值，平局保留首项，没有 tail 信号时返回首 raw，空输入返回 None。

#### Scenario: 低阈值但非 thinking
- **WHEN** 最低 tail 阈值属于 response channel
- **THEN** 仍选它；该辅助不是 confidence 过滤器。

证据：`crates/codegen/sampling-types/src/doom_loop.rs` — `tightest`。

### Requirement: Doom loop payload classification
peek_doom_loop SHALL 先执行字面 doom_loop_check 预筛，再解析 JSON；顶层 type 精确命中返回 CheckEvent，否则仅按 response 下 triggers 路径存在返回 ResponseField。

#### Scenario: 坏 JSON
- **WHEN** 含关键字但 JSON 无效
- **THEN** 返回 None。

#### Scenario: 非终止事件带字段
- **WHEN** 任意事件包含 /response/doom_loop_check/triggers
- **THEN** 返回 ResponseField，不要求 completed 或 incomplete 类型。

#### Scenario: 转义关键字
- **WHEN** 等价 JSON 将关键字全部 Unicode 转义，字面预筛不命中
- **THEN** 返回 None，不承诺所有等价编码均可识别。

证据：`crates/codegen/sampling-types/src/doom_loop.rs` — `peek_doom_loop`。

### Requirement: Doom loop tolerant trigger arrays
triggers 解析 SHALL 对缺失或非数组返回空列表，数组仅解析字符串，保留空字符串、未知标签与重复。

#### Scenario: 类型已确认但触发器损坏
- **WHEN** CheckEvent 的 triggers 为非数组
- **THEN** 仍返回 CheckEvent 空列表；吞掉事件由调用方执行。

证据：`crates/codegen/sampling-types/src/doom_loop.rs` — `parse_triggers`。

### Requirement: Doom loop named event recognition
is_check_event SHALL 对精确 event 名直接返回 true；否则要求正文预筛命中且合法 JSON 顶层 type 精确匹配。

#### Scenario: 命名坏事件
- **WHEN** event 名精确匹配而正文非法
- **THEN** 仍返回 true。

#### Scenario: 正文引用事件名
- **WHEN** 普通正文只引用该字符串但顶层 type 不符
- **THEN** 返回 false，不因引用文字吞掉普通事件。

证据：`crates/codegen/sampling-types/src/doom_loop.rs` — `is_check_event`。

### Requirement: Empty string optional normalization
empty_string_as_none SHALL 将空字符串或 null 解析为 None，不 trim 非空字符串。

#### Scenario: 空白字符串
- **WHEN** 值只含空格
- **THEN** 保留 Some 空格，不归一为空。

证据：`crates/codegen/sampling-types/src/serde_helpers.rs` — `empty_string_as_none`。

### Requirement: Three state optional update decoding
double_option SHALL 在被调用时返回外层 Some；调用字段配合 serde default 才可区分缺失、null 和具体值。

#### Scenario: 字段缺失
- **WHEN** 字段配置 default 和该 deserialize_with
- **THEN** 得到 None，表示未设置。

#### Scenario: 显式清除
- **WHEN** 字段值为 null
- **THEN** 得到 Some(None)；合法具体值为 Some(Some(value))。

证据：`crates/codegen/sampling-types/src/serde_helpers.rs` — `double_option`。

### Requirement: Encoded sampling request size constant
sampling-types SHALL 暴露 MAX_REQUEST_BODY_BYTES=50*1024*1024；该常量本身不编码请求或执行发送大小检查。

#### Scenario: 仅构造类型
- **WHEN** 内存请求超过该常量表示的长度
- **THEN** 类型构造不自动拒绝；最终字节执行检查属于 transport。

证据：`crates/codegen/sampling-types/src/lib.rs` — `MAX_REQUEST_BODY_BYTES`。

### Requirement: Messages request required wire fields
MessagesRequest 反序列化 SHALL 要求 model/messages/max_tokens；Default 构造能力不等同于 wire 缺字段默认，也不校验模型、温度或 token 参数合法性。

#### Scenario: 缺必需字段
- **WHEN** JSON 省略 model 或 messages 或 max_tokens
- **THEN** 反序列化失败。

#### Scenario: 程序默认值
- **WHEN** 调用 Default
- **THEN** 可得到空模型和零 max_tokens，不在类型层保证服务端接受。

证据：`crates/codegen/sampling-types/src/messages.rs` — `MessagesRequest`。

### Requirement: Messages content and system shapes
Messages Message SHALL 仅支持 user/assistant role，content 为字符串或块列表，system 为字符串或 TextBlock 列表；TextBlock.type 和 CacheControl.type 是字符串字段。

#### Scenario: 自定义 type
- **WHEN** TextBlock 或 CacheControl 的 type 是非预期字符串
- **THEN** 字段本身不拒绝；ephemeral 构造器才固定缓存类型。

证据：`crates/codegen/sampling-types/src/messages.rs` — `SystemParam`。

### Requirement: Messages typed content blocks
Messages ContentBlock SHALL 表达 Text/Image/ToolUse/ToolResult/Thinking/RedactedThinking；前四种可带缓存标记，Thinking 携带签名，RedactedThinking 携带 opaque data。

#### Scenario: 工具结果递归内容
- **WHEN** ToolResult content 为块列表
- **THEN** 可递归保存 ContentBlock；该类型没有 is_error 字段。

#### Scenario: 未知字段
- **WHEN** 输入包含类型未定义字段
- **THEN** 不承诺往返保留这些字段。

证据：`crates/codegen/sampling-types/src/messages.rs` — `ContentBlock`。

### Requirement: Messages image and tool definitions
ImageSource SHALL 支持 Base64(media_type/data) 与 Url；ToolParam 要求 name/input_schema，可选 description，schema 为任意 JSON；这些类型不校验 URL、编码或 schema 可执行性。

#### Scenario: 无效编码
- **WHEN** Base64 data 不是有效 base64 文本
- **THEN** 字符串字段仍可构造，不在此验证解码。

证据：`crates/codegen/sampling-types/src/messages.rs` — `ImageSource`。

### Requirement: Messages tool choice type boundary
ToolChoiceParam SHALL 支持 auto、any、tool(name)，不提供 none 变体；工具名称不与请求工具表自动交叉验证。

#### Scenario: 不存在的工具名
- **WHEN** 指定 tool choice name 不在 tools 中
- **THEN** 类型构造不拒绝，由调用方或服务端处理。

证据：`crates/codegen/sampling-types/src/messages.rs` — `ToolChoiceParam`。

### Requirement: Messages thinking and structured output types
ThinkingConfig SHALL 表达 Enabled(u32 budget)、Adaptive(可选 display)、Disabled；OutputConfig 支持任意 effort 字符串和仅携带 schema 的 JSON Schema 格式。

#### Scenario: 模型不支持配置
- **WHEN** 构造任意模型不支持的 thinking 或 effort
- **THEN** 类型层不检查模型能力。

#### Scenario: 结构化输出
- **WHEN** 构造 OutputFormat::JsonSchema
- **THEN** 该结构无 name/strict 字段，不补充 Chat 的 schema 包装。

证据：`crates/codegen/sampling-types/src/messages.rs` — `ThinkingConfig`。

### Requirement: Messages response stop reason preservation
MessagesResponse SHALL 保存 id/type/role/content/model/usage，stop_reason 可缺；StopReason 保存七种已知值及未知字符串的原值。

#### Scenario: 未来 stop 标签
- **WHEN** 反序列化未知字符串 stop_reason
- **THEN** 保存 Unknown 并原样序列化。

#### Scenario: 错误值类型
- **WHEN** stop_reason 不是字符串
- **THEN** 不能作为未知字符串接受。

证据：`crates/codegen/sampling-types/src/messages.rs` — `StopReason`。

### Requirement: Messages usage field defaults
MessagesUsage SHALL 要求 input_tokens/output_tokens，cache creation/read 缺失默认零；MessageDeltaUsage 要求 output_tokens，其余计数为可选。

#### Scenario: 完整 usage 缺 input
- **WHEN** 反序列化没有 input_tokens 的 MessagesUsage
- **THEN** 失败，不因 Default 派生补零。

#### Scenario: delta 无缓存计数
- **WHEN** 可选计数字段为 None
- **THEN** 序列化为 null，与完整 usage 缺失默认零不同。

证据：`crates/codegen/sampling-types/src/messages.rs` — `MessageDeltaUsage`。

### Requirement: Messages stream event vocabulary
MessageStreamEvent SHALL 支持 message start/delta/stop、content block start/delta/stop、ping/error；StreamDelta 支持 text/input_json/thinking/signature，不提供未知事件兜底。

#### Scenario: 部分工具 JSON
- **WHEN** input_json delta 携带 partial_json
- **THEN** 只保存字符串，不在该类型中拼接或验证完整 JSON。

#### Scenario: 乱序块索引
- **WHEN** 事件带 u32 index
- **THEN** 不在数据结构中验证事件先后顺序。

证据：`crates/codegen/sampling-types/src/messages.rs` — `MessageStreamEvent`。

### Requirement: Messages optional stop details
MessageDeltaBody SHALL 保留可选 stop_reason/stop_sequence/stop_details；StopDetails 允许未知字段被忽略，但已知字段类型错误仍失败。

#### Scenario: 未知 details 键
- **WHEN** stop_details 仅包含未知字段
- **THEN** 可按已知字段为空解析。

#### Scenario: 类型错误
- **WHEN** 已知可选字段提供错误 JSON 类型
- **THEN** 反序列化失败，不将任意形状都当作容错成功。

证据：`crates/codegen/sampling-types/src/messages.rs` — `StopDetails`。

### Requirement: Chat request builder field semantics
ChatCompletionRequest builder SHALL 直接设置模型、消息、工具、选择、max_tokens、temperature 和 top_p；该结构不含 stream 字段，不验证参数范围。

#### Scenario: 边界行为
- **WHEN** 使用 from_messages 而非 new
- **THEN** 模型保持 None；new 使用 Some(model)。

证据：`crates/codegen/sampling-types/src/types.rs` — `ChatCompletionRequest`。

### Requirement: Chat message content wire semantics
ChatRequestMessage SHALL 使用四种角色及必需字符串或 Text/ImageUrl 列表 content；tool_calls 缺失默认空，但 null 不视为空。

#### Scenario: 边界行为
- **WHEN** content 为 null
- **THEN** 反序列化失败；ImageUrl 本身仅存字符串，不校验 URL。

证据：`crates/codegen/sampling-types/src/types.rs` — `ChatRequestMessage`。

### Requirement: Chat content extraction and mutation
Chat 文本提取 SHALL 忽略图片、以 LF 连接 Text；set_text_content 替换全部内容，append 在字符串上直接拼接、非空列表中追加 Text，空内容改为字符串。

#### Scenario: 边界行为
- **WHEN** 列表中所有 Text 都为空但列表非空
- **THEN** MessageContent::is_empty 仍为 false，只检查容器是否为空。

证据：`crates/codegen/sampling-types/src/types.rs` — `append_text_content`。

### Requirement: Chat prompt truncation boundary
chat_truncate_for_prompt SHALL 按 User 计数保留目标 prompt 及其后到下一个 User 前的消息，超出范围保留全部。

#### Scenario: 边界行为
- **WHEN** 请求极大 usize target
- **THEN** 实现使用普通 target+1，不承诺饱和处理；与 durable prompt marker 截断不同。

证据：`crates/codegen/sampling-types/src/types.rs` — `chat_truncate_for_prompt`。

### Requirement: Chat tool call argument representation
Chat 工具调用 SHALL 将 arguments 保存为字符串，请求 ID 可缺，响应 ID 必需；ToolChoice preset 可为任意字符串，from_json 只编码 JSON 不校验工具 schema。

#### Scenario: 边界行为
- **WHEN** 输入合法 JSON 标量作为工具参数
- **THEN** from_json 可编码该标量，不强制 object。

证据：`crates/codegen/sampling-types/src/types.rs` — `ToolCallFunction`。

### Requirement: Chat finish reason unknown preservation
FinishReason SHALL 保存五种标准标签和未知字符串，unknown_value 提供未知原值。

#### Scenario: 边界行为
- **WHEN** 未来字符串标签到达
- **THEN** 保留并可序列化，不丢整项；非字符串仍不接受。

证据：`crates/codegen/sampling-types/src/types.rs` — `FinishReason`。

### Requirement: Chat usage arithmetic boundary
Usage SHALL 保存必需 prompt_tokens/completion_tokens/total_tokens 计数、可选 details 与有符号 cost ticks，不校验计数和，不归一非正 cost。

#### Scenario: 边界行为
- **WHEN** total 不等于输入输出之和或 cost 为负
- **THEN** 数据类型仍可保存；后续响应归一由其他入口执行。

证据：`crates/codegen/sampling-types/src/types.rs` — `Usage`。

### Requirement: Chat stream delta partial fields
ChatChunkDelta SHALL 接受缺失或 null tool_calls 为空列表；ToolCallDelta 要求 index，其余字段可部分提供，不在类型层累积参数。

#### Scenario: 边界行为
- **WHEN** reasoning_content=None 或空 fingerprint
- **THEN** reasoning_content 序列化 null；fingerprint 仅空字符串变 None，不 trim。

证据：`crates/codegen/sampling-types/src/types.rs` — `ChatChunkDelta`。

### Requirement: Compaction header configuration resolution
CompactionAtTokens SHALL 接受 bool 或 u64，false=None，true=context_window*percent/100，固定值原样返回；不 clamp 百分比或饱和乘法。

#### Scenario: 边界行为
- **WHEN** 计算乘法溢出
- **THEN** 该类型不提供饱和保证。

证据：`crates/codegen/sampling-types/src/types.rs` — `CompactionAtTokens`。

### Requirement: Remaining compaction configuration resolution
CompactionsRemaining SHALL 接受 bool 或 u8；false=None，true 在无 summary 时为 1、有 summary 时为 0，固定值不随 summary 改变。

#### Scenario: 边界行为
- **WHEN** 固定 remaining=3 且已有 summary
- **THEN** 仍返回 Some(3)。

证据：`crates/codegen/sampling-types/src/types.rs` — `CompactionsRemaining`。

### Requirement: Reasoning effort parsing and backend projection
ReasoningEffort SHALL 支持 none/minimal/low/medium/high/xhigh/max，默认 medium；FromStr 忽略大小写但不 trim，serde 仅接受规范小写。

#### Scenario: 边界行为
- **WHEN** 投影 None 或 Minimal 到 Messages
- **THEN** 返回 None；Responses 七值逐项映射。

证据：`crates/codegen/sampling-types/src/types.rs` — `ReasoningEffort`。

### Requirement: Reasoning effort menu parsing
ReasoningEffortOption SHALL 接受裸字符串或完整对象，裸字符串按宽松 FromStr 归一，完整对象 value 走严格 serde；缺失 id/label 才默认，显式空值保留。

#### Scenario: 边界行为
- **WHEN** 菜单有非法项或为空
- **THEN** parse_reasoning_efforts_meta 整体返回 None，不保留部分有效项；不校验唯一 ID 或唯一 default。

证据：`crates/codegen/sampling-types/src/types.rs` — `ReasoningEffortOption`。

### Requirement: Sampling endpoint configuration boundary
SamplingConfig SHALL 保存 base_url、wire model、采样设置、backend、有序 header/query/env 映射和 NonZero context window，不执行 URL/header/env 解析验证。

#### Scenario: 边界行为
- **WHEN** context window 为零
- **THEN** NonZero 类型拒绝零；其他字段合法性不由该容器统一检查。

证据：`crates/codegen/sampling-types/src/types.rs` — `SamplingConfig`。

### Requirement: Backend native schema policy flag
ApiBackend SHALL 提供 ChatCompletions/Responses/Messages，默认 Chat；supports_native_schema 仅对前两者为 true。

#### Scenario: 边界行为
- **WHEN** Messages 服务端支持 schema
- **THEN** 本标志仍为 false，它表示内部策略而非远端能力探测。

证据：`crates/codegen/sampling-types/src/types.rs` — `supports_native_schema`。

### Requirement: Model image input endpoint identity
model_image_input_key SHALL 使用 wire model/backend 及 base_url 原字节与排序 query 的 BLAKE3 摘要；query 每项以 NUL 和等号分隔，不包含 headers/env。

#### Scenario: 边界行为
- **WHEN** 只改变 header 或 URL 等价写法
- **THEN** 只改变 header 不改 key；URL 不规范化，等价写法可产生不同 key。任意分隔符输入不保证无编码歧义。

证据：`crates/codegen/sampling-types/src/types.rs` — `model_image_input_key_from_parts`。

### Requirement: Responses request wrapper delegation
CreateResponseWrapper SHALL 仅包装 CreateResponse，提供构造与转换入口，不额外执行网络请求或补丁序列化。

#### Scenario: 边界行为
- **WHEN** 通过 wrapper 构造请求
- **THEN** 保持 inner 数据，协议修补须调用其他显式入口。

证据：`crates/codegen/sampling-types/src/types.rs` — `CreateResponseWrapper`。

### Requirement: Durable conversation item taxonomy
ConversationItem SHALL 区分 System/User/Assistant/ToolResult/BackendToolCall/Reasoning 六类；Assistant 保存文本、调用和模型元数据，ToolResult images 可同时含 Text/Image。

#### Scenario: 边界行为
- **WHEN** 从旧 Assistant JSON 读取 reasoning 字段
- **THEN** 该旧字段不构成新的持久化推理载体。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `ConversationItem`。

### Requirement: Visible reasoning one way decoding
VisibleReasoningItem SHALL 新写仅 text；读取优先字符串 text，否则从旧 summary/content 数组提取文本并 LF 拼接，丢弃 opaque native 字段。

#### Scenario: 边界行为
- **WHEN** 旧形状无法提取文本
- **THEN** 可退为空文本，不保证拒绝所有坏形状。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `VisibleReasoningItem`。

### Requirement: Backend tool visible summary decoding
BackendToolCallItem SHALL 持久化 kind 与 summary，兼容读取旧 native kind 对象并丢弃 opaque 字段；未知种类降 Other，code interpreter 预览最多 100 字符。

#### Scenario: 边界行为
- **WHEN** 旧 payload 包含 container/id/output
- **THEN** 这些原生字段不在新摘要结构中往返保留。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `BackendToolCallItem`。

### Requirement: Synthetic prompt origin classification
SyntheticReason SHALL 区分十四种注入来源，仅 TaskCompleted/SubagentCompleted/NotificationDrain 开启 prompt turn，其他 synthetic 不开启。

#### Scenario: 边界行为
- **WHEN** 反序列化未知或已删除标签
- **THEN** 失败，不归一为普通用户输入。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `SyntheticReason`。

### Requirement: Permission evidence typed origin
PermissionEvidence SHALL 区分 DirectUser 与 Interjection，保存独立授权文本并拒绝未知字段；UserItem 容器不自动校验授权来源与其他元数据组合。

#### Scenario: 边界行为
- **WHEN** 为 synthetic User 显式设置 evidence
- **THEN** 公开 setter 不因 synthetic 标签自动拒绝，调用方负责来源正确。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `PermissionEvidence`。

### Requirement: Goal directive current scope projection
目标投影 SHALL 仅保留最后一个匹配 active goal_id/revision 的 User directive 正文，其余带 goal tag 的正文替换占位，保留位置和其他元数据。

#### Scenario: 边界行为
- **WHEN** active goal 为空
- **THEN** 全部带 tag 的正文遮罩，普通输入不变。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `project_conversation_for_goal_scope`。

### Requirement: Conversation image group identity
图片分组 SHALL 按 User 或 ToolResult 项收集有序 URL，指纹包含来源和 URL 长度/内容；工具结果关联最近的先前同 ID 调用。

#### Scenario: 边界行为
- **WHEN** 重复 tool ID 或 source text 含私有路径
- **THEN** 关联最近调用；工具组 source_text 使用固定隐藏说明，用户组去掉图片 envelope。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `conversation_image_groups`。

### Requirement: Unconditional image rejection heuristic
图片不支持判定 SHALL 要求状态 400 且请求含图片；排除格式、体积和策略类错误后再匹配不支持图片的声明或期待文本的特征。

#### Scenario: 边界行为
- **WHEN** 没有图片或状态非 400
- **THEN** 返回 false，不因消息提到 image 就推断模型不支持。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `is_unconditional_image_input_unsupported`。

### Requirement: Image replacement preserves permission evidence
图片替换 SHALL 在 User 首图片位置放替代文本并删除全部图片，清除文本中首个完整 image_files envelope；ToolResult 保留已有 Text 占位并将描述追加正文。

#### Scenario: 边界行为
- **WHEN** 图片描述包含授权指令
- **THEN** 不修改 User 已有 permission_evidence，不把转录自动当授权来源。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `replace_item_images_with_text`。

### Requirement: Projected image tool call redaction
图片调用脱敏 SHALL 对匹配 ID 的工具参数写固定占位，并替换整个所属 Assistant 可见正文。

#### Scenario: 边界行为
- **WHEN** Assistant 同时包含其他文本
- **THEN** 正文整体替换，不保证只删图片路径片段。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `redact_projected_image_tool_call`。

### Requirement: Projected image reference redaction boundary
图片来源引用脱敏 SHALL 从来源调用的路径相关键递归提取字符串，对目标文本执行子串替换；不使用路径 token 边界匹配。

#### Scenario: 边界行为
- **WHEN** 任意文本恰好包含该路径子串
- **THEN** 匹配部分也会被替换；未知未提取来源不保证删除。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `redact_projected_image_tool_result_references`。

### Requirement: Image response carrier selection and replacement
图片响应 carrier SHALL 仅选择 Assistant 紧邻之前连续 Reasoning/BackendToolCall，按顺序返回；显式脱敏将其变为通用 Assistant 文本。

#### Scenario: 边界行为
- **WHEN** 中间存在 User 或普通 Assistant
- **THEN** carrier 搜索在边界停止，不扫描更早项。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `assistant_response_carrier_indices`。

### Requirement: Image compaction reference scope
压缩引用脱敏 SHALL 仅改写标记 CompactionMeta 的 User 文本中的已提取来源引用，不把所有 User 输入当压缩资料。

#### Scenario: 边界行为
- **WHEN** 普通 User 含相同来源字符串
- **THEN** 该 compaction 专用入口不改写普通 User。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `replace_compaction_reference_tokens`。

### Requirement: Conversation constructors and metadata setters
会话构造器 SHALL 按入口写入角色与 synthetic 标签；普通 User 默认无授权 evidence，interjection 使用单独 permission_text；marker/interrupt/evidence setter 仅作用 User。

#### Scenario: 边界行为
- **WHEN** 在非 User 设置 prompt_index
- **THEN** 不修改该项，不自动创建 User。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `set_prompt_index`。

### Requirement: Streaming reasoning fallback insertion
推理 fallback SHALL 在已有任意非空 Reasoning 时不操作，否则填首个空 Reasoning，或插入最后 Assistant 之前，没有 Assistant 时追加。

#### Scenario: 边界行为
- **WHEN** 已有非空推理但位于其他位置
- **THEN** 仍不注入重复 fallback，不要求紧邻最后 Assistant。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `inject_streaming_reasoning_fallback`。

### Requirement: Output schema local validation limits
schema 编译 SHALL 限制编码大小为 256KiB、拒绝外部引用获取，并设置 regex 256KiB 和 DFA 2MiB 限制；验证值不匹配时返回说明字符串。

#### Scenario: 边界行为
- **WHEN** 复杂 schema 验证
- **THEN** 这些限制不是总 CPU 时间或全部内存的硬上限。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `compile_output_schema`。

### Requirement: Native continuation fragment boundary
NativeContinuationFragment SHALL 区分 Chat/Responses/Messages typed fragment，不派生持久化 serde；估算采用 JSON 字节数整除 4，序列化失败按零估算。

#### Scenario: 边界行为
- **WHEN** 查询签名
- **THEN** 只从 Messages 中首个非空 Thinking signature 获取，不从 durable Reasoning 重建。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `NativeContinuationFragment`。

### Requirement: Native continuation span validation
request_segments SHALL 校验 prefix 范围、span 顺序与不重叠、非空范围、end 上界及 backend；任一不符则整体 portable 投影。

#### Scenario: 边界行为
- **WHEN** span backend 正确但路由或模型不同
- **THEN** 本入口没有路由/模型验证字段，不能据 backend 相同推导可安全跨路由复用。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `request_segments`。

### Requirement: Native and neutral request segment assembly
有效 native projection SHALL 将 prefix portable 化、span 替换成 native fragment，间隙和尾部保留 neutral items；没有 projection 时原样克隆 items。

#### Scenario: 边界行为
- **WHEN** projection 不存在
- **THEN** 不自动调用 project_portable_history；普通历史工具协议仍可进入直接投影。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `request_segments`。

### Requirement: Portable historical tool exchange projection
portable history SHALL 删除 durable Reasoning，保留 System/User，清除 Assistant 模型元数据，将有紧邻匹配结果的工具调用转为 User 历史工具文本及图片。

#### Scenario: 边界行为
- **WHEN** 调用缺结果或结果无匹配调用
- **THEN** 不完整工具协议被丢弃；非空 Assistant 正文仍可保留。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `project_portable_history`。

### Requirement: Portable historical duplicate and echo handling
portable 工具结果映射 SHALL 对同 ID 使用邻接段最后结果；无调用 Assistant 若正文以历史工具模板头开始则整项丢弃。

#### Scenario: 边界行为
- **WHEN** 旧模板 echo 同时夹带其他正文
- **THEN** 整个匹配前缀的 Assistant 项丢弃，不局部解析模板。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `project_portable_history`。

### Requirement: Response assistant and empty result selection
ConversationResponse SHALL 从后向前寻找最后 Assistant；其正文或本地工具调用非空即非空，否则按是否存在非空 Reasoning 区分 ReasoningOnly/NoVisibleContent。

#### Scenario: 边界行为
- **WHEN** 只含后台工具或无 Assistant
- **THEN** 判 NoVisibleContent，不把后台工具当本地工具调用。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `empty_reason`。

### Requirement: Response fallback text and reported cost
fallback_text SHALL 仅在 message_chunks_emitted=0 且助手正文非空时返回正文；reported_cost_ticks 仅保留正数。

#### Scenario: 边界行为
- **WHEN** 只有工具调用或 cost 为零/负数
- **THEN** 无 fallback 正文；非正 cost 归 None。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `fallback_text`；`crates/codegen/sampling-types/src/conversation.rs` — `reported_cost_ticks`。

### Requirement: Unified usage and stop reason conversion
TokenUsage SHALL 从 Chat usage 复制计数，缺少 reasoning/cached details 用零，cache creation 为零；FinishReason 未知值映射 Stop，FunctionCall 映射 ToolCalls。

#### Scenario: 边界行为
- **WHEN** 底层包含未知 finish 标签
- **THEN** 统一 StopReason 不保留该字符串；原始值需其他字段携带。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `TokenUsage`。

### Requirement: Chat direct conversation projection
Chat 直接投影 SHALL 保留 User 文本图片、Assistant 文本与 sanitized 工具参数，BackendToolCall 转助手摘要；batch 删除 Reasoning，单项 Reasoning 转换 panic。

#### Scenario: 边界行为
- **WHEN** ToolResult images 同时有 Text/Image
- **THEN** 直接投影保留正文和 Image，跳过 images 中 Text。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `conversation_to_chat_messages`。

### Requirement: Tool argument wire sanitization
工具参数 wire sanitization SHALL 接受任意合法 JSON 原串，无效 JSON 改为 {}，日志预览最多 200 字节并退到 UTF-8 边界。

#### Scenario: 边界行为
- **WHEN** 合法 JSON 标量
- **THEN** 不强制参数必须对象，不执行工具 schema 校验。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `sanitize_tool_arguments`。

### Requirement: Responses function completion validation
Responses 输出转事实 SHALL 在转换前校验所有 FunctionCall：response Completed/Incomplete 且 call Completed，或 response Completed 且 call status 缺失；参数必须有效 JSON。

#### Scenario: 边界行为
- **WHEN** 任何调用不满足完成证据
- **THEN** 整个转换返回 Serialization 错误；没有函数调用时不由这条规则验证 response status。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `response_to_conversation_items`。

### Requirement: Responses output visible fact aggregation
Responses 输出转换 SHALL 聚合 OutputText 和 FunctionCall 到尾部单个 Assistant，可见 Reasoning 与支持的后台工具另成 siblings，保留模型/fingerprint/effort。

#### Scenario: 边界行为
- **WHEN** 输出含 refusal 或其他未处理 item
- **THEN** 不自动转成可见正文；不同 Message 的原始交错顺序不保证逐项保留。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `response_to_conversation_items`。

### Requirement: Responses native fragment allowlist
responses_native_fragment SHALL 仅保留实现列出的 typed native 项，清除 FunctionCall/Reasoning status；不在本入口验证完成状态。

#### Scenario: 边界行为
- **WHEN** 输出为 Shell/ApplyPatch 调用或结果、Compaction、ToolSearch
- **THEN** 不纳入该 native fragment；Message status 仍保留。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `responses_native_fragment`。

### Requirement: Responses input text and tool output projection
Responses 直接投影 SHALL 先输出 Assistant 非空文本再输出函数调用；ToolResult 无 images 用字符串，有 images 用正文及 Image 块，忽略 images 中 Text。

#### Scenario: 边界行为
- **WHEN** User 恰好一个 Text
- **THEN** 使用字符串；其他内容形状包括空列表使用 ContentList。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `conversation_item_to_input_items`。

### Requirement: Responses reasoning discriminator patch
patch_reasoning_text_types SHALL 仅为 input 顶层 reasoning.content 数组中的对象补缺失 type=reasoning_text，不修改已有 type。

#### Scenario: 边界行为
- **WHEN** 已有错误或未来 type 值
- **THEN** 原值保留，不把补丁当 schema 验证器。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `patch_reasoning_text_types`。

### Requirement: Chat request reverse durable conversion
ChatRequestMessage 转 durable SHALL 按 role 构造单项，User 保留文本图片但默认无 synthetic/evidence；Assistant 丢 reasoning 与模型元数据，缺调用 ID 用空串，Tool 只留文本。

#### Scenario: 边界行为
- **WHEN** 原 Tool message 含图片或缺 tool_call_id
- **THEN** 图片不进入 ToolResult images，缺失 ID 使用空串；转换不验证调用配对。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `impl From<ChatRequestMessage>`。

### Requirement: Chat response single item conversion
ChatResponseMessage 转 durable SHALL 聚合为单个 Assistant，保留 content 与工具调用，不保留 reasoning/citations/tool_call_id，也不按输入 role 改变输出类别。

#### Scenario: 边界行为
- **WHEN** 响应只有 reasoning 无正文
- **THEN** 单项转换不会生成 Reasoning sibling；流式保存属于消费者。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `impl From<ChatResponseMessage>`。

### Requirement: Chat request top level projection
ConversationRequest 转 Chat SHALL 传递模型、temperature/max_tokens/top_p/effort，无工具时清除 tool_choice；schema 输出使用固定 name 与 strict=true。

#### Scenario: 边界行为
- **WHEN** 设置 prompt_cache_key
- **THEN** 该 Chat 请求结构不携带此键；frequency/presence penalty 与 user 为空。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `impl From<ConversationRequest>`。

### Requirement: Responses request top level projection
ConversationRequest 转 Responses SHALL 传递 prompt_cache_key，reasoning 始终 Some 且 summary=Concise，effort 可缺；schema 使用固定 name、strict=true，instructions/stream/store/previous_response_id 为空。

#### Scenario: 边界行为
- **WHEN** tools 为空但 choice 已设置
- **THEN** tools=None，choice 仍映射；不在转换中验证服务端接受性。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `impl From<&ConversationRequest>`。

### Requirement: Portable structured output mode selection
portable_schema_for_backend SHALL 对 ChatCompletions 返回 JsonObject，对 Responses/Messages 返回携带原 schema 的 JsonSchema；直接设置 JsonSchema 不受该辅助限制。

#### Scenario: 边界行为
- **WHEN** 直接使用 with_json_schema
- **THEN** 保存提供的 schema，builder 不自动运行 compile_output_schema。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `portable_schema_for_backend`。

### Requirement: UTF8 byte prefix truncation
truncate_bytes SHALL 返回不超过给定字节数的前缀，切点退到合法 UTF-8 字符边界，不按 grapheme cluster 截断。

#### Scenario: 边界行为
- **WHEN** 上限落在四字节 emoji 内
- **THEN** 退到 emoji 之前，不 panic；上限大于原长度则返回原串。

证据：`crates/codegen/sampling-types/src/conversation.rs` — `truncate_bytes`。

### Requirement: Sampler configuration and auth independence
采样层 SHALL 独立保存 backend 与 Bearer/XApiKey auth scheme；默认空模型、空 URL、零 context_window 不构成有效服务配置保证。

#### Scenario: 实现边界
- **WHEN** 默认构造 SamplerConfig
- **THEN** auth 默认为 Bearer；context_window 不自行执行请求长度约束。

证据：`crates/codegen/sampler/src/config.rs` — `SamplerConfig`。

### Requirement: Sampler configuration hook serialization
采样层 SHALL 在配置 serde 往返时跳过 attribution_callback 和 bearer_resolver，保留可序列化 api_key 字段。

#### Scenario: 实现边界
- **WHEN** 序列化再反序列化含运行时 hook 的配置
- **THEN** 调用方须重新安装 hook；此配置容器不能当作已脱敏日志对象。

证据：`crates/codegen/sampler/src/config.rs` — `SamplerConfig`。

### Requirement: Sampler shared HTTP client lifetime
采样层 SHALL 分别缓存普通与 HTTP1 客户端；共享开关首次读取锁定，仅精确 0 或大小写无关 false 禁用。

#### Scenario: 实现边界
- **WHEN** 禁用共享后反复构造客户端
- **THEN** 每次重新构建；构建失败不写入缓存，竞争不保证只构建一次。

证据：`crates/codegen/sampler/src/shared_http.rs` — `GROW_SAMPLER_SHARED_CLIENT`。

### Requirement: Sampler HTTP transport defaults
采样层 SHALL 默认每 host 池容量2、idle90秒、connect10秒，开启TCP_NODELAY及HTTP2 ping；解析环境整数失败采用默认值。

#### Scenario: 实现边界
- **WHEN** 使用 HTTP1 fallback
- **THEN** 使用 http1_only、零池容量与零idle，不保证连接池复用；本层未配置总请求时限。

证据：`crates/codegen/sampler/src/shared_http.rs` — `http1_only`。

### Requirement: Sampler endpoint query merging
采样层 SHALL 在需要 query 处理且 URL 可解析时保留未覆盖的 base query，并用配置键替换全部原同名键及编码值。

#### Scenario: 实现边界
- **WHEN** URL 解析失败
- **THEN** 警告后退回字符串拼接，配置 query 不被应用；fragment 未被独立移除。

证据：`crates/codegen/sampler/src/client.rs` — `EndpointTemplate`。

### Requirement: Sampler header construction precedence
采样层 SHALL 按 Content-Type、静态 key、extra headers、环境 headers、可构造 UA 的顺序构建头。

#### Scenario: 实现边界
- **WHEN** 静态 key 或 extra header 无法构成合法头
- **THEN** 分别返回认证或配置错误；非法/空环境值则跳过，环境合法值覆盖已有同名头。

证据：`crates/codegen/sampler/src/client.rs` — `SamplingClient`。

### Requirement: Sampler fresh resolver authority
采样层 SHALL 每次 post 使用同步 resolver 当前值，先删除 Authorization 和 x-api-key 再写选定 scheme。

#### Scenario: 实现边界
- **WHEN** resolver 返回 None 或非法新值
- **THEN** 无该凭据发送，不回退到静态 key；配置静态非法值仍可在构建阶段先失败。

证据：`crates/codegen/sampler/src/client.rs` — `post`。

### Requirement: Sampler credential attribution capture
采样层 SHALL 将发送时捕获的 credential 片段用于401归因，诊断读取与实际发送读取分别进行。

#### Scenario: 实现边界
- **WHEN** 发送后 resolver 发生轮换
- **THEN** 401 回调仍得到发送时捕获值；Bearer 片段识别要求精确 Bearer 前缀。

证据：`crates/codegen/sampler/src/client.rs` — `post`。

### Requirement: Sampler credential fragment boundary
采样层 SHALL 以最多尾12个Unicode字符形成凭据片段，保留短字符串原值。

#### Scenario: 实现边界
- **WHEN** 凭据不超过12字符
- **THEN** 片段等于完整输入，不能承诺该函数永不输出完整短 key。

证据：`crates/codegen/sampler/src/attribution.rs` — `SENT_BEARER_PREFIX_LEN`。

### Requirement: Sampler request size barrier
采样层 SHALL 对可见字节 body 在发送前检查50MiB上限，超出产生 API413 和 should_retry=false。

#### Scenario: 实现边界
- **WHEN** body 恰好50MiB或 as_bytes 不可见
- **THEN** 恰好上限通过；不可见流 body 按零计，不构成通用流大小限制。

证据：`crates/codegen/sampler/src/client.rs` — `check_request_body_size`。

### Requirement: Sampler request evidence admission
采样层 SHALL 六个协议及流式组合的发送入口均在 execute 前等待请求证据ACK，大小检查先于证据写入。

#### Scenario: 实现边界
- **WHEN** 证据写入失败
- **THEN** 返回 Persistence，不执行该请求；mark_dispatched 表示客户端开始派发，不证明服务端接收。

证据：`crates/codegen/sampler/src/client.rs` — `check_and_record_request`。

### Requirement: Sampler response header hints
采样层 SHALL 解析 Retry-After 为u64秒且最多120秒，解析大小写无关的布尔重试头，独立提取模型容量和ETag。

#### Scenario: 实现边界
- **WHEN** Retry-After 为HTTP日期或重试布尔包含空白
- **THEN** 不采用这些值；合法零容量和空ETag仍可保留。

证据：`crates/codegen/sampler/src/client.rs` — `extract_model_metadata`。

### Requirement: Sampler Chat request defaults
采样层 SHALL 只补 Chat 请求缺失的模型、输出上限、温度和 top_p；流式请求强制 stream 与 include_usage。

#### Scenario: 实现边界
- **WHEN** 请求显式给空模型或配置 reasoning_effort
- **THEN** 空模型不作为缺失处理；客户端默认值不自动注入 reasoning_effort。

证据：`crates/codegen/sampler/src/client.rs` — `StreamingChatRequest`。

### Requirement: Sampler Responses request defaults
采样层 SHALL 补缺失模型及采样参数，store缺失设false，确保include包含加密reasoning。

#### Scenario: 实现边界
- **WHEN** 请求显式 store=true
- **THEN** 保留true；非流式请求仍进行reasoning type修补，但不执行SSE终态context/cost覆盖。

证据：`crates/codegen/sampler/src/client.rs` — `ReasoningEncryptedContent`。

### Requirement: Sampler Messages request defaults
采样层 SHALL 为空模型补默认值，max_tokens为0时使用配置上限或128000，补缺失温度及top_p。

#### Scenario: 实现边界
- **WHEN** 配置 output_limit=Some(0)
- **THEN** 补值仍可为0，不把默认填充描述为完整请求有效性验证。

证据：`crates/codegen/sampler/src/client.rs` — `128_000`。

### Requirement: Sampler stream byte and extension handling
采样层 SHALL 先记录原始响应字节，再仅剥首个字节chunk的完整UTF8 BOM；精确[DONE]结束。

#### Scenario: 实现边界
- **WHEN** BOM跨chunk或首chunk为空
- **THEN** 不保证BOM被修复；未知非空顶层事件可忽略，已知损坏payload仍报错。

证据：`crates/codegen/sampler/src/client.rs` — `decode_tagged_event`。

### Requirement: Sampler Chat extension discrimination
采样层 SHALL 仅在无choices/error键且非标准chunk对象等条件满足时忽略扩展事件。

#### Scenario: 实现边界
- **WHEN** 扩展样式payload仍含choices
- **THEN** 保留解析失败，不能借扩展标签绕过已知chunk校验。

证据：`crates/codegen/sampler/src/client.rs` — `decode_chat_chunk`。

### Requirement: Sampler Responses terminal context and cost
采样层 SHALL 仅对completed/incomplete事件注入正i64成本，并在context input/output均可转u32时饱和相加覆盖total_tokens。

#### Scenario: 实现边界
- **WHEN** context字段缺失或越界
- **THEN** 保留原total_tokens；累计input/output/cache/reasoning不受覆盖。

证据：`crates/codegen/sampler/src/client.rs` — `COST_USD_TICKS_METADATA_KEY`。

### Requirement: Sampler SSE error termination boundary
采样层 SHALL 在transport/eventsource错误后只发一次错误并终止底层扫描；语义或反序列化错误不在L1设置同一终止标记。

#### Scenario: 实现边界
- **WHEN** L2消费遇到语义错误
- **THEN** 由L2自己的失败分支停止；不能将L1单错误终止规则推广到所有错误。

证据：`crates/codegen/sampler/src/client.rs` — `EventStreamError`。

### Requirement: Sampler direct conversation collection
采样层 SHALL 按配置backend选择L2并传idle timeout，收首终态后丢弃metrics并重建错误。

#### Scenario: 实现边界
- **WHEN** 调用conversation_collect
- **THEN** 不运行actor重试循环；显式协议便捷入口不因配置backend不同而拒绝。

证据：`crates/codegen/sampler/src/client.rs` — `conversation_collect`。

### Requirement: Sampler response buffering limit boundary
采样层 SHALL 无审计时将HTTP响应全量缓冲，有审计时先逐chunk记录再缓冲。

#### Scenario: 实现边界
- **WHEN** 响应过大且没有审计sink
- **THEN** read_response_bytes本身没有大小上限或独立idle限制。

证据：`crates/codegen/sampler/src/client.rs` — `read_response_bytes`。

### Requirement: Sampler retry budget resolution
采样层 SHALL 优先解析GROW_MAX_RETRIES，再用调用方值，最后默认15；actor配置显式0绕过环境覆盖。

#### Scenario: 实现边界
- **WHEN** 环境值非法或有无法解析的空白
- **THEN** 退回调用方预算，不钳制合法u32极值。

证据：`crates/codegen/sampler/src/retry.rs` — `resolve_max_retries`。

### Requirement: Sampler retry classifier precedence
采样层 SHALL 先将认证错误交session，再处理零预算和服务端否决，随后区分限流及普通可重试错误。

#### Scenario: 实现边界
- **WHEN** 认证错误同时预算为0
- **THEN** 仍EmitToSession；413及should_retry=false不进入普通重试。

证据：`crates/codegen/sampler/src/retry.rs` — `classify_error`。

### Requirement: Sampler retry attempt limits
采样层 SHALL 以retry_count+1达到预算即耗尽，429使用普通预算和限流阈值的较小者。

#### Scenario: 实现边界
- **WHEN** 采用默认限流阈值2
- **THEN** 允许一次429重试，不能解释为额外两次重试。

证据：`crates/codegen/sampler/src/retry.rs` — `classify_error`。

### Requirement: Sampler initial retry client rebuild
采样层 SHALL 普通可重试错误的首次重试选择HTTP1重建，之后按普通重试处理。

#### Scenario: 实现边界
- **WHEN** 首次错误是空响应或EventStreamError
- **THEN** 也会选择重建，不仅限于原始HTTP连接错误。

证据：`crates/codegen/sampler/src/retry.rs` — `RetryWithClientRebuild`。

### Requirement: Sampler retry delay arithmetic
采样层 SHALL 普通指数基值上限30000毫秒后施加正负20%抖动，doom抖动0到250毫秒。

#### Scenario: 实现边界
- **WHEN** 普通backoff达到基值上限
- **THEN** 实际延迟仍可到36000毫秒；直接传入classifier的Retry-After不在此再限120秒。

证据：`crates/codegen/sampler/src/retry.rs` — `doom_loop_backoff`。

### Requirement: Sampler error clone fidelity
采样层 SHALL 复制API重试提示、认证归因、空响应与doom信息，无法clone的Http降为EventStreamError。

#### Scenario: 实现边界
- **WHEN** 原Http含结构化状态或不可重试分类
- **THEN** 副本不保证结构及重试语义等价；Serialization保留类别与显示文本。

证据：`crates/codegen/sampler/src/retry.rs` — `clone_error`。

### Requirement: Sampler request identifier representation
采样层 SHALL 允许任意String作为RequestId，并提供UUIDv4随机生成、原样Display与字符串serde。

#### Scenario: 实现边界
- **WHEN** 调用From传空字符串
- **THEN** 不执行UUID或非空校验。

证据：`crates/codegen/sampler/src/types.rs` — `RequestId`。

### Requirement: Sampler event error representation
采样层 SHALL 将错误转换为公开SamplingErrorInfo字段，is_retryable来自错误分类而非独立服务端否决。

#### Scenario: 实现边界
- **WHEN** 将API500且should_retry=false转换再重建
- **THEN** 独立否决信息不能保证保留；usage不属于重建SamplingError的字段。

证据：`crates/codegen/sampler/src/events.rs` — `sampling_error_from_info`。

### Requirement: Sampler event transport shape
采样层 SHALL 提供开始、首token、通道、工具delta、响应开始、思考完成、metadata、重试及终态事件。

#### Scenario: 实现边界
- **WHEN** 需要序列化SamplingEvent
- **THEN** 类型本身仅Clone/Debug；SamplingChannel和ErrorKind的serde变体名不等同as_str标签。

证据：`crates/codegen/sampler/src/events.rs` — `SamplingEvent`。

### Requirement: Sampler collection terminal contract
采样层 SHALL 返回遇到的首Completed或Failed，忽略中间事件，无独立timeout及ID一致性校验。

#### Scenario: 实现边界
- **WHEN** 流EOF但无终态
- **THEN** 返回不可重试的合成错误，不读取已收到终态之后的数据。

证据：`crates/codegen/sampler/src/stream/collect.rs` — `collect_response`。

### Requirement: Sampler latency statistics
采样层 SHALL 以text内容chunk时间计算TTFB和相邻ITL，TTLB取流结束，p50取上中位、p99取ceil百分位、mean整数截断。

#### Scenario: 实现边界
- **WHEN** 零或一个内容chunk
- **THEN** 无ITL统计；attempts初始0由actor填写，不把FirstToken事件当作该计时输入。

证据：`crates/codegen/sampler/src/metrics.rs` — `InferenceLatencyStats`。

### Requirement: Sampler doom signal collector
采样层 SHALL clone共享收集状态，按原始标签去重保序；disarm只关闭abort判断，不停止收集。

#### Scenario: 实现边界
- **WHEN** 调用take后再次收到相同标签
- **THEN** 可重新记录；take不重置disarm或坏payload仅告警一次的状态。

证据：`crates/codegen/sampler/src/doom_loop.rs` — `DoomLoopSignalCollector`。

### Requirement: Sampler doom collector poison and checks
采样层 SHALL 命名check事件即使payload损坏仍吞，ResponseField记录通常继续转发。

#### Scenario: 实现边界
- **WHEN** collector锁poison
- **THEN** 查询退空或None、写入跳过，不以panic恢复；本层无signal容量上限。

证据：`crates/codegen/sampler/src/doom_loop.rs` — `absorb`。

### Requirement: Sampler attempt evidence response cap
采样层 SHALL 为每次attempt保存最多64MiB原始响应前缀，溢出设置sticky状态并返回错误。

#### Scenario: 实现边界
- **WHEN** 响应恰好达到上限
- **THEN** 允许；超过后即使finish成功写入仍返回overflow错误。

证据：`crates/codegen/sampler/src/audit.rs` — `AttemptEvidence`。

### Requirement: Sampler evidence acknowledgement lifecycle
采样层 SHALL 等待request sink ACK并保留request failure；finish先检查request failure，否则将响应快照交sink。

#### Scenario: 实现边界
- **WHEN** response sink ACK失败
- **THEN** 保留body；ACK成功才清body，不重置其他状态，重复finish不保证幂等。

证据：`crates/codegen/sampler/src/audit.rs` — `finish`。

### Requirement: Sampler task scoped audit boundary
采样层 SHALL 通过task-local scope启用AttemptEvidence，sink决定具体持久化实现。

#### Scenario: 实现边界
- **WHEN** 其他任务未显式进入scope
- **THEN** 不自动继承证据；sink等待无本层timeout，锁poison可panic。

证据：`crates/codegen/sampler/src/audit.rs` — `scope`。

### Requirement: Sampler handle admission and close
采样层 SHALL 共享accepting标志及无界命令队列，close首次切换false并发送Shutdown。

#### Scenario: 实现边界
- **WHEN** close与submit并发
- **THEN** 检查与send不是原子步骤，是否处理取决于队列顺序；close本身不等待join。

证据：`crates/codegen/sampler/src/handle.rs` — `close`。

### Requirement: Sampler handle query failure defaults
采样层 SHALL 通过oneshot查询active集合，无回复或发送失败返回false或0。

#### Scenario: 实现边界
- **WHEN** actor已关闭
- **THEN** 默认查询值不区分关停与空闲；查询没有独立timeout。

证据：`crates/codegen/sampler/src/handle.rs` — `active_count`。

### Requirement: Sampler collect future cancellation guard
采样层 SHALL collect请求成功入队后安装CancelOnDrop，并通过oneshot等待结果。

#### Scenario: 实现边界
- **WHEN** collect future被丢弃或正常返回
- **THEN** guard均发送Cancel；入队成功不等于已执行，丢失completion返回AuthUnknown关停类错误。

证据：`crates/codegen/sampler/src/handle.rs` — `CancelOnDrop`。

### Requirement: Sampler actor request isolation
采样层 SHALL 逐条处理命令并为每个请求spawn独立任务，复制当时effective config与retry policy。

#### Scenario: 实现边界
- **WHEN** 更新默认config后提交无override请求
- **THEN** 仅后续请求使用新config，已有任务保留快照。

证据：`crates/codegen/sampler/src/actor/mod.rs` — `handle_command`。

### Requirement: Sampler active registry semantics
采样层 SHALL 以RequestId登记token，Cancel先移除再取消，正常任务回收按返回ID删除登记。

#### Scenario: 实现边界
- **WHEN** 复用同一ID覆盖在途任务
- **THEN** 旧token被取消；回收无generation校验，旧任务返回可误删新登记，已记录独立债务。

证据：`crates/codegen/sampler/src/actor/state.rs` — `register`。

### Requirement: Sampler actor shutdown ownership
采样层 SHALL 提供owned显式join与spawn返回handle两种入口，Shutdown或队列EOF取消登记token后shutdown JoinSet。

#### Scenario: 实现边界
- **WHEN** 使用普通spawn
- **THEN** JoinHandle被丢弃；不承诺owner析构自动join，也不承诺强制abort完成逐请求持久化结算。

证据：`crates/codegen/sampler/src/actor/mod.rs` — `spawn_owned`。

### Requirement: Sampler bounded owner shutdown
采样层 SHALL 关闭admission并在给定时限内等待actor，超时abort后仍await观察结束。

#### Scenario: 实现边界
- **WHEN** 超时或actor panic
- **THEN** 返回错误且取走task；重复无task的关停返回成功。

证据：`crates/codegen/sampler/src/actor/mod.rs` — `shutdown_bounded`。

### Requirement: Sampler provider scope admission
采样层 SHALL 每attempt先await scope capture，再进入对应provider开流；slot区分从未准入与已准入无Goal。

#### Scenario: 实现边界
- **WHEN** 取消发生在scope捕获完成后开流返回前
- **THEN** 保留捕获scope用于结算；捕获尚未完成则不登记provider_started。

证据：`crates/codegen/sampler/src/actor/request_task.rs` — `run_one_attempt`。

### Requirement: Sampler attempt usage settlement
采样层 SHALL 从终态取usage；有证据且尚未dispatched或严格无条件图片能力拒绝时可补精确零。

#### Scenario: 实现边界
- **WHEN** 已准入attempt缺可信usage
- **THEN** 向配置的usage sink提交Incomplete；没有evidence时不凭init failure普遍推断零。

证据：`crates/codegen/sampler/src/actor/request_task.rs` — `run_request_task`。

### Requirement: Sampler persistence before terminal and retry
采样层 SHALL 先await evidence.finish再await usage结算，检查持久化结果后才检查cancel和分派outcome。

#### Scenario: 实现边界
- **WHEN** 持久化失败
- **THEN** 先发送Failed(Persistence)再完成oneshot，不重试；finish/usage等待不能由本层cancel select直接打断。

证据：`crates/codegen/sampler/src/actor/request_task.rs` — `finish_attempt_persistence_failure`。

### Requirement: Sampler retry evidence gate
采样层 SHALL 在可重试决策执行前等待retry evidence ACK，随后增加计数、发Retrying并可取消地sleep。

#### Scenario: 实现边界
- **WHEN** 取消先于retry记录完成
- **THEN** 终止且不增加该次普通retry计数；记录失败返回Persistence。

证据：`crates/codegen/sampler/src/actor/request_task.rs` — `apply_retry_decision`。

### Requirement: Sampler output observed replay policy
采样层 SHALL 在retry_only_before_output启用且观察到输出后将普通retry预算设0，输出标志跨attempt保留。

#### Scenario: 实现边界
- **WHEN** 错误本身仍被分类为retryable
- **THEN** 本轮actor仍可直接终止；不改写错误类别以伪装不可重试。

证据：`crates/codegen/sampler/src/actor/request_task.rs` — `response_has_observed_output`。

### Requirement: Sampler independent doom resampling budget
采样层 SHALL 在启用策略及非零总预算时对confident doom用独立计数重采样，预算花完下轮disarm。

#### Scenario: 实现边界
- **WHEN** 已观察普通输出但收到doom失败
- **THEN** 仍可走独立doom恢复；最终metrics attempts为普通与doom计数加1。

证据：`crates/codegen/sampler/src/actor/request_task.rs` — `doom_retry_count`。

### Requirement: Sampler L2 outcome precedence
采样层 SHALL 处理Completed时先标输出，再检查doom，再分类Length/context超限/PauseTurn，最后检测空响应。

#### Scenario: 实现边界
- **WHEN** ContentFilter终态没有正文
- **THEN** 不按EmptyResponse重试；截断类部分响应作为Completed交上层。

证据：`crates/codegen/sampler/src/actor/request_task.rs` — `drive_l2`。

### Requirement: Sampler terminal cancellation usage priority
采样层 SHALL drive_l2优先poll现成终态保留usage，非terminal转发一个后即检查取消。

#### Scenario: 实现边界
- **WHEN** terminal与cancel同时ready
- **THEN** 可先获得终态usage结算，但外层再次检查cancel后仍可能返回取消。

证据：`crates/codegen/sampler/src/actor/request_task.rs` — `drive_l2`。

### Requirement: Sampler raw error tee and event forwarding
采样层 SHALL 捕获原始流首错误的clone供重试分类，L2失败时优先使用它，否则从Info重建。

#### Scenario: 实现边界
- **WHEN** 收到普通中间事件
- **THEN** 忽略发送失败且retag实际原样返回；L2无终态EOF合成EventStreamError。

证据：`crates/codegen/sampler/src/actor/request_task.rs` — `tee_errors`。

### Requirement: Sampler cancel terminal representations
采样层 SHALL 对取消发送Api kind、无status、不可重试事件，同时oneshot返回AuthUnknown。

#### Scenario: 实现边界
- **WHEN** 调用方比较两通道错误类型
- **THEN** 不能假设二者类别完全相同；completion发送最多一次，接收方丢弃被忽略。

证据：`crates/codegen/sampler/src/actor/request_task.rs` — `handle_cancellation`。

### Requirement: Sampler wire shared connection verification
采样层 SHALL 在默认池配置下允许不同SamplingClient共享底层连接，同时每配置请求头独立。

#### Scenario: 实现边界
- **WHEN** forceHTTP1或共享kill switch关闭
- **THEN** 本地wire测试观察到每次新accept；该测试不证明真实TLS或HTTP2故障恢复。

证据：`crates/codegen/sampler/tests/shared_http_wire.rs` — `shared_client_keeps_per_config_headers_isolated`。

### Requirement: Sampler user facing edge errors
采样层 SHALL 对非JSON代理错误使用状态文案，对结构化JSON错误保留message。

#### Scenario: 实现边界
- **WHEN** Chat流式返回524 HTML
- **THEN** 用户错误Display不包含HTML；原始证据与诊断日志不由此保证抹除。

证据：`crates/codegen/sampler/tests/cf_edge_error_message.rs` — `stream_524_html_uses_status_copy`。

### Requirement: Sampler Chat candidate and response identity
Chat 流转换 SHALL 仅接受单个index0候选，后续chunk id须与首chunk一致。

#### Scenario: 协议边界
- **WHEN** 首chunk模型与后续模型不同
- **THEN** 保留首模型，不在本层校验模型一致；空id本身未被拒绝。

证据：`crates/codegen/sampler/src/stream/chat_completions.rs` — `stream_chat_completions`。

### Requirement: Sampler Chat finish evidence and bounded tail
Chat 流转换 SHALL 要求choice finish_reason作为成功证据，并从首次finish启动固定2秒usage尾期限。

#### Scenario: 协议边界
- **WHEN** 已有finish后EOF或尾timeout
- **THEN** 可完成而不虚构usage；尾transport错误仍失败，重复finish不延长固定期限。

证据：`crates/codegen/sampler/src/stream/chat_completions.rs` — `stream_chat_completions`。

### Requirement: Sampler Chat terminal conflict checks
Chat 流转换 SHALL 拒绝空或冲突finish，拒绝首finish之后的新正文、思考或工具delta。

#### Scenario: 协议边界
- **WHEN** 正文与首次finish在同一frame
- **THEN** 允许处理；后续同finish无输出可接受，raw finish仍保留原值。

证据：`crates/codegen/sampler/src/stream/chat_completions.rs` — `stream_chat_completions`。

### Requirement: Sampler Chat content and latency
Chat 流转换 SHALL 非空text/reasoning首次发FirstToken并共享chunk index，仅text计message chunks与latency。

#### Scenario: 协议边界
- **WHEN** 只收到tool delta
- **THEN** 不发FirstToken、不计text latency，但有效工具进展可续idle。

证据：`crates/codegen/sampler/src/stream/chat_completions.rs` — `stream_chat_completions`。

### Requirement: Sampler Chat tool identity assembly
Chat 流转换 SHALL 按tool index聚合，允许args先于id/name，忽略trim空identity并拒绝已定identity冲突。

#### Scenario: 协议边界
- **WHEN** 重复相同id/name且无新args
- **THEN** 不发delta、不刷新内容idle；可缺kind，有kind必须精确function。

证据：`crates/codegen/sampler/src/stream/chat_completions.rs` — `stream_chat_completions`。

### Requirement: Sampler Chat tool JSON completion boundary
Chat 流转换 SHALL 终态验证各工具arguments为完整JSON，任一损坏则整轮Failed并附最后usage。

#### Scenario: 协议边界
- **WHEN** arguments是合法数组或null但id/name缺失
- **THEN** 此L2未实施object、最终identity非空或跨index唯一检查，不能宣称已完成这些语义验证。

证据：`crates/codegen/sampler/src/stream/chat_completions.rs` — `stream_chat_completions`。

### Requirement: Sampler Chat tool ordering and stop override
Chat 流转换 SHALL 按index升序输出工具，有工具则typed stop改ToolCalls，保留raw finish。

#### Scenario: 协议边界
- **WHEN** 完整工具伴随length终态
- **THEN** 保留工具并改typed stop，raw仍length。

证据：`crates/codegen/sampler/src/stream/chat_completions.rs` — `stream_chat_completions`。

### Requirement: Sampler Chat native and reasoning response
Chat 流转换 SHALL 构造Assistant及可选synthesized reasoning sibling，同时native Chat消息保存reasoning_content。

#### Scenario: 协议边界
- **WHEN** wire含response id
- **THEN** response.message_id仍None；模型及fingerprint来自首chunk，effort未回显。

证据：`crates/codegen/sampler/src/stream/chat_completions.rs` — `stream_chat_completions`。

### Requirement: Sampler Chat usage and cost updates
Chat 流转换 SHALL 以最后usage覆盖累计用量，正成本上报覆盖旧成本，缺失或未报告成本不抹旧值。

#### Scenario: 协议边界
- **WHEN** usage-only帧持续到达
- **THEN** 不据此刷新内容idle；完成后usage可仍None。

证据：`crates/codegen/sampler/src/stream/chat_completions.rs` — `stream_chat_completions`。

### Requirement: Sampler Chat content idle completion boundary
Chat 流转换 SHALL 同时限制每次next与无实际内容的累计等待，重复identity、role和usage不算内容。

#### Scenario: 协议边界
- **WHEN** finish前只有keepalive
- **THEN** IdleTimeout失败；finish后内容idle到期可结束尾部。

证据：`crates/codegen/sampler/src/stream/chat_completions.rs` — `stream_chat_completions`。

### Requirement: Sampler Messages start and block lifecycle
Messages 流转换 SHALL 要求唯一合法message_start，id/model非空、type和role正确、初始content空且无stop_reason。

#### Scenario: 协议边界
- **WHEN** block在start前、重复index、孤儿delta/stop或类型不匹配
- **THEN** 立即协议失败；Ping及Error可在start前处理。

证据：`crates/codegen/sampler/src/stream/messages.rs` — `stream_messages`。

### Requirement: Sampler Messages message delta phase
Messages 流转换 SHALL 首message_delta后禁止内容块事件，未闭合块记录sticky协议错误并继续等终态usage。

#### Scenario: 协议边界
- **WHEN** 未闭合thinking或tool伴随max_tokens结束
- **THEN** 整轮失败而非静默丢弃后自动继续；可信终态usage仍附错误。

证据：`crates/codegen/sampler/src/stream/messages.rs` — `stream_messages`。

### Requirement: Sampler Messages initial content preview
Messages 流转换 SHALL Text或Thinking start即使文本空也可发FirstToken，初始文本仅累积不发ChannelToken。

#### Scenario: 协议边界
- **WHEN** 初始块已经带非空文本但没有delta
- **THEN** 最终内容可存在，message chunk计数及text latency仍不因初始文本增加。

证据：`crates/codegen/sampler/src/stream/messages.rs` — `stream_messages`。

### Requirement: Sampler Messages delta channels and signatures
Messages 流转换 SHALL 非空text/thinking发相应通道，signature delta替换旧signature并于block stop发ReasoningCompleted。

#### Scenario: 协议边界
- **WHEN** 多段signature delta
- **THEN** 保留最后一段，不拼接；thinking不计text latency。

证据：`crates/codegen/sampler/src/stream/messages.rs` — `stream_messages`。

### Requirement: Sampler Messages tool JSON object admission
Messages 流转换 SHALL 工具初始input须object；出现任何参数delta则初始对象须空，以delta原文验证完整object。

#### Scenario: 协议边界
- **WHEN** 空参数delta或非空初始对象再跟delta
- **THEN** 整轮失败，即使健康兄弟工具已闭合也不输出Completed；预览事件不回撤。

证据：`crates/codegen/sampler/src/stream/messages.rs` — `stream_messages`。

### Requirement: Sampler Messages visible and native ordering
Messages 流转换 SHALL visible正文、思考、工具按block关闭顺序累计，正文块用换行连接，native按block index升序。

#### Scenario: 协议边界
- **WHEN** block交错且关闭次序不同于index
- **THEN** 两种顺序可不同；native Text/ToolUse重建cache_control=None。

证据：`crates/codegen/sampler/src/stream/messages.rs` — `stream_messages`。

### Requirement: Sampler Messages opaque reasoning separation
Messages 流转换 SHALL closed thinking的签名和redacted data保留在native，visible只存非空思考文本。

#### Scenario: 协议边界
- **WHEN** 收到RedactedThinking
- **THEN** 不发对应实时事件，不在visible items暴露data；不代表native未保存该内容。

证据：`crates/codegen/sampler/src/stream/messages.rs` — `stream_messages`。

### Requirement: Sampler Messages stop reason preservation
Messages 流转换 SHALL 映射已知stop reason并保留wire原值，未知非空reason警告后按Stop处理。

#### Scenario: 协议边界
- **WHEN** 闭合工具与refusal或length同轮存在
- **THEN** 有效tool优先将内部stop设ToolCalls，wire原停止原因不丢失。

证据：`crates/codegen/sampler/src/stream/messages.rs` — `stream_messages`。

### Requirement: Sampler Messages repeated terminal delta updates
Messages 流转换 SHALL 允许多个message_delta，拒绝冲突reason/sequence，缺省reason保留之前值。

#### Scenario: 协议边界
- **WHEN** usage下降或optional计数Some0
- **THEN** 按最新值覆盖，不强制单调；stop_details解释可被后续None解释清除。

证据：`crates/codegen/sampler/src/stream/messages.rs` — `stream_messages`。

### Requirement: Sampler Messages terminal usage accounting
Messages 流转换 SHALL 仅有message_stop及stop_reason时视usage可信，prompt饱和合并uncached、cache read和write。

#### Scenario: 协议边界
- **WHEN** 合法全零用量或协议失败但可信终态已到
- **THEN** 仍提供Some usage；reasoning计数0，cache读写分别保留。

证据：`crates/codegen/sampler/src/stream/messages.rs` — `stream_messages`。

### Requirement: Sampler Messages terminal boundary and EOF
Messages 流转换 SHALL message_stop立即停止消费，成功还要求delta、reason和所有块关闭。

#### Scenario: 协议边界
- **WHEN** EOF缺终态或协议即时失败
- **THEN** 不把非终态计数冒充最终usage，不输出可执行Completed。

证据：`crates/codegen/sampler/src/stream/messages.rs` — `stream_messages`。

### Requirement: Sampler Messages content aware idle
Messages 流转换 SHALL Ping和空delta不续内容deadline，结构事件和非空signature计进展。

#### Scenario: 协议边界
- **WHEN** 重复相同message_delta只改变stop details
- **THEN** 不续内容deadline；transport next timeout与内容时钟均可失败。

证据：`crates/codegen/sampler/src/stream/messages.rs` — `stream_messages`。

### Requirement: Sampler Responses progress and output tracking
Responses 流转换 SHALL 在处理前将有意义事件标记为可能输出，包括未转发的refusal及hosted tool进展。

#### Scenario: 协议边界
- **WHEN** created、queued、in_progress或ResponseError
- **THEN** 这些不标输出；其他未知兜底事件可计进展，不等同已转发正文。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler Responses doom abort timing
Responses 流转换 SHALL 在raw Ok事件到达后、处理事件前检查collector，armed confident直接失败。

#### Scenario: 协议边界
- **WHEN** confident信号与terminal同帧
- **THEN** terminal usage未在该分支保留；只有被吞check且无后续Ok事件不保证立即唤醒abort。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler Responses response lifecycle identity
Responses 流转换 SHALL 记录created/inprogress/queued的id并比较成功/不完整终态，允许省略这些前置事件。

#### Scenario: 协议边界
- **WHEN** 只有合法terminal snapshot
- **THEN** 可完成；不校验sequence_number或强制非空response id。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler Responses terminal status correspondence
Responses 流转换 SHALL completed/incomplete/failed事件分别要求对应status，provider error映射Failed且保留终态usage。

#### Scenario: 协议边界
- **WHEN** failed事件id与前置id不同
- **THEN** 该直接失败路径不再执行成功终态的id交叉校验。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler Responses preview and final content
Responses 流转换 SHALL text及reasoning delta用于实时事件，最终正文取terminal snapshot。

#### Scenario: 协议边界
- **WHEN** 预览text与terminal正文不同
- **THEN** 不在本层交叉校验文本一致；仅reasoning text累计作fallback，summary delta不累计该fallback。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler Responses function output registration
Responses 流转换 SHALL Added index不可重复或已关闭，function按到达顺序分配tool-only index并发call_id/name。

#### Scenario: 协议边界
- **WHEN** function初始arguments非空
- **THEN** 保存为已见前缀但不作为初始arguments_delta发出。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler Responses function argument lifecycle
Responses 流转换 SHALL delta/done须对应已登记function，禁止已done后的参数，存在call.id时比item_id。

#### Scenario: 协议边界
- **WHEN** argsdone包含已有前缀的补全内容
- **THEN** 允许补全并锁定完整arguments，不另发补全delta；可选name须一致。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler Responses item done evidence
Responses 流转换 SHALL 允许无added的itemdone，拒绝重复done或已观察function与非function类型互换。

#### Scenario: 协议边界
- **WHEN** terminal省略此前已见function
- **THEN** 协议失败；非function项不执行同等完整的终态交叉校验。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler Responses terminal tool consistency
Responses 流转换 SHALL 按output vector索引比对function identity及args，未argsdone允许前缀，done则要求全等。

#### Scenario: 协议边界
- **WHEN** 终态工具显式Incomplete或InProgress
- **THEN** 匹配done也不得覆盖该状态；仅缺省status可用匹配done补Completed，再交转换器校验。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler Responses conversion errors and usage
Responses 流转换 SHALL 在终态到达后保留usage用于id、工具一致性或类型转换失败。

#### Scenario: 协议边界
- **WHEN** snapshot自带provider error
- **THEN** 优先按provider错误失败，不执行后续成功转换。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler Responses incomplete control semantics
Responses 流转换 SHALL 仅max_output_tokens映射Length，content_filter映射ContentFilter，其余不完整detail映射Stop。

#### Scenario: 协议边界
- **WHEN** 终态包含可执行assistant工具
- **THEN** ToolCalls优先，raw记录status及原detail。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler Responses native fragment and fallback
Responses 流转换 SHALL 在注入流式reasoning fallback之前生成native fragment。

#### Scenario: 协议边界
- **WHEN** fallback补充visible reasoning
- **THEN** 不因此回写native；response.message_id、stop_message和stop_sequence仍None。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler Responses terminal consumption and idle
Responses 流转换 SHALL 正常terminal不poll尾数据，但先执行内容idle检查再break。

#### Scenario: 协议边界
- **WHEN** 空terminal到达且内容时钟已过期
- **THEN** 仍可能IdleTimeout且不保终态usage；EOF无terminal是Serialization失败。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler Responses terminal cost and signals
Responses 流转换 SHALL 移除内部metadata成本键并解析i64，成功后take collector信号附响应。

#### Scenario: 协议边界
- **WHEN** 直接调用L2并提供负i64成本metadata
- **THEN** 本层不再次限制正数；失败不drain signals，成功可附disarmed信号。

证据：`crates/codegen/sampler/src/stream/responses.rs` — `stream_responses`。

### Requirement: Sampler diagnostic disclosure boundary
采样层 SHALL 请求诊断保留base URL，部分日志包含凭据片段与原始SSE数据，不能把采样日志视为自动脱敏输出。

#### Scenario: 诊断边界
- **WHEN** 非法静态key或短key
- **THEN** 现有诊断可出现完整短值；债务独立登记，不在文档迁移中修改runtime。

证据：`crates/codegen/sampler/src/client.rs` — `post`。

### Requirement: Sampler sampling span fields
采样层 SHALL 为请求创建sampling span并记录模型、backend、URL、auth诊断及可选reasoning effort，成功后记录输出和reasoning用量。

#### Scenario: 诊断边界
- **WHEN** 未配置独立日志文件
- **THEN** 该模块仍可向tracing subscriber发事件，不在本模块决定文件sink或启用开关。

证据：`crates/codegen/sampler/src/sampling_log.rs` — `request_span`。

### Requirement: Sampler user agent composition
采样层 SHALL 生成grow-shell版本及平台UA，可加入不同origin，arm64归一aarch64。

#### Scenario: 诊断边界
- **WHEN** origin令最终HeaderValue非法
- **THEN** 不覆盖已有UA，不因该字段单独返回构建错误。

证据：`crates/codegen/sampler/src/client.rs` — `HeaderValue`。

### Requirement: Sampler error display reconstruction
采样层 SHALL 按错误类型提供文案，并从Info重建Idle秒数、API状态及可选空响应/doom上下文。

#### Scenario: 诊断边界
- **WHEN** Info无合法状态或Idle秒数字段文本
- **THEN** API状态回退500，Idle回退0；缺空响应上下文降EventStreamError，不承诺无损往返。

证据：`crates/codegen/sampler/src/events.rs` — `sampling_error_from_info`。



### Requirement: Shell provider catalog hierarchy and parse failure boundaries
provider catalog SHALL 要求root/provider/各provider/models/options及各model为table，缺provider返回空；provider层仅接受api_backend/options/models，其他键导致整体Err。顶层api_backend覆盖options内同名值。options单字段类型解析失败使整份options回退默认并warning，未知options字段warning且忽略。模型catalog key为provider_id/model_id，缺model字段时补model_id作为wire名，再交模型override解析。inline auth以provider:前缀合成name写入auth映射，冲突warning并覆盖，即使显式auth_provider最终遮蔽inline auth也仍插入。

#### Scenario: Malformed provider option
- **WHEN** options存在但其中字段类型不可解析
- **THEN** 整份options回退默认，继续模型解析；不等同单字段回退。

源码证据：
- `crates/codegen/shell/src/agent/provider_catalog.rs` — `pub(crate) fn parse_provider_catalog`。
- `crates/codegen/shell/src/agent/provider_catalog.rs` — `fn parse_provider_options`。
- `crates/codegen/shell/src/agent/provider_catalog.rs` — `pub(crate) fn provider_auth_name`。


### Requirement: Shell provider defaults and model credential group inheritance
模型继承 SHALL 对base_url/api_backend/context_window仅None回退provider；extra_headers/query_params/env_http_headers仅模型map为空时整份继承，不逐key合并。模型有非空api_key、EnvKeys.primary存在或auth_provider为Some任一即视为自己配置认证，阻止继承provider整个认证组；否则一并继承api_key/env_key/auth_provider，显式provider auth_provider优先于inline auth合成引用。是否存在模型env变量不在继承阶段判断；空map无法表示主动清除provider map。

#### Scenario: Partial model authentication
- **WHEN** 模型只设置env_key名称但宿主该变量未设置
- **THEN** 仍阻止继承provider认证组，不因运行时env缺失恢复provider key/helper。

源码证据：
- `crates/codegen/shell/src/agent/provider_catalog.rs` — `fn with_provider_defaults`。


### Requirement: Shell LLM catalog admission validation
validate_llm_configuration SHALL 拒绝空catalog、缺失或trim为空的global default、trim后不在config_models中的default以及任一resolved模型trim为空的base_url；不在此验证URL格式、连通性或API key存在。reasoning_efforts逐模型拒绝trim及ASCII小写归一后空/重复id、重复value和多个default，允许零default。find_model_by_catalog_id仅精确map.get，不trim也不按wire model名匹配。

#### Scenario: Keyless model admission
- **WHEN** 配置catalog/default/base_url与reasoning菜单有效但没有key
- **THEN** 此校验可通过，不代表网络或辅助模型调用可用。

源码证据（测试本轮未执行）：
- `crates/codegen/shell/src/agent/config.rs` — `pub fn validate_llm_configuration`。
- `crates/codegen/shell/src/agent/config.rs` — `pub fn find_model_by_catalog_id`。
- `crates/codegen/shell/src/agent/provider_catalog.rs` — `fn missing_provider_catalog_is_rejected_before_connect`。
- `crates/codegen/shell/src/agent/provider_catalog.rs` — `fn reasoning_menu_rejects_multiple_defaults`。
- `crates/codegen/shell/src/agent/provider_catalog.rs` — `fn global_default_must_be_an_exact_provider_model_id`。
- `crates/codegen/shell/src/agent/provider_catalog.rs` — `fn product_credentials_never_become_inference_credentials`。


### Requirement: Shell model sampler projection and URL derived headers
sampling_config_for_model SHALL 从model.info复制wire model、采样参数、backend、headers/query/env headers、context、默认reasoning、retry及compaction字段，从ResolvedCredentials取key/base_url/auth_scheme；stream_tool_calls缺省false，force_http1=false，idle_timeout/origin_client/attribution/bearer_resolver/doom_loop_recovery均None。构造本身不附加auth_provider动态resolver。URL识别为cli chat proxy时按大小写敏感IndexMap entry补X-Grow-Token-Auth、x-authenticateresponse和CLIENT_MODE_HEADER，精确同名已有值保留；不同大小写键不在这里去重。alpha_test_key当前未使用，不生成额外access header。

#### Scenario: Provider model projection
- **WHEN** 输入model带auth_provider但调用此构造器
- **THEN** SamplerConfig的bearer_resolver仍None，动态凭据需上层另行附加。

源码证据：
- `crates/codegen/shell/src/agent/config.rs` — `pub fn sampling_config_for_model`。
- `crates/codegen/shell/src/agent/config.rs` — `pub fn inject_url_derived_headers`。


### Requirement: Shell ACP model catalog metadata projection
to_acp_model_info SHALL 保持catalog map身份作为ModelId，显示名优先info.name的Some值否则wire model，复制description；meta总含totalContextTokens与agentType，reasoning列表非空才附菜单并在存在默认effort时附默认值。该投影不筛选模型、不检查凭据、不将catalog id替换成wire name；因为两个基础meta字段总插入，当前meta不会为空。

#### Scenario: Catalog and wire names differ
- **WHEN** catalog key与provider-facing model字段不同
- **THEN** ACP ModelId采用catalog key，缺name时显示wire model。

源码证据：
- `crates/codegen/shell/src/agent/config.rs` — `pub fn to_acp_model_info`。


### Requirement: Shell image description route admission
图像描述路由 SHALL 要求配置image_description_model且resolve_aux_sampler_config成功；重建当前session配置并复制session local sampler字段后，按model/backend/base_url/query_params形成辅助runtime key。辅助key等于被拒绝key或已在ModelImageInputState标记unsupported时返回None，不尝试该辅助模型；SamplingClient构造失败同样None。当前模型unsupported查询基于chat-state采样配置形成key，资源缺失视未标记；标记入口委托bridge mark-and-flush并返回IO结果。报告described_images=0不产生恢复通知，非零通知描述永久替代及原图Timeline保留。此路由准入本身不证明描述生成或ImageShadow持久化成功。

#### Scenario: Auxiliary aliases rejected runtime
- **WHEN** 辅助catalog名称不同但解析后的runtime key与被拒绝runtime相同
- **THEN** 拒绝使用辅助描述路由，不能以别名绕过该检查。

源码证据：
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `async fn resolve_image_description_route`。
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `async fn model_image_input_is_unsupported`。
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `pub(in crate::session::actor) async fn record_unsupported_model_image_input`。
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `fn image_recovery_notification`。


### Requirement: Shell deferred sampling failure notification ownership
send_grow_notification SHALL 对存在current_prompt_id的RetryState特殊处理：Failed或Exhausted覆盖pending_sampling_failure为该prompt与retry并立即返回，Retrying清空pending后携带promptId发送；未取得当前prompt（包括锁失败）则直接走普通发送。finish_sampling_failure_notification仅在pending owner等于传入prompt_id时take，owner不同保留；匹配后仅failed=true才发送缓存RetryState和promptId，failed=false消费但不发送。该方法根据传入布尔值决策，不自行判定turn成败，不持久化pending缓存，不在此确认发送成功。

#### Scenario: Wrong completion owner
- **WHEN** pending属于prompt A而finish传prompt B
- **THEN** 不消费A的pending，也不发送。

#### Scenario: Recovered matching turn
- **WHEN** finish匹配pending且failed=false
- **THEN** 清除缓存并不发送失败通知。

#### Scenario: Later failure replaces pending
- **WHEN** 已有缓存时收到当前prompt Failed或Exhausted
- **THEN** 覆盖旧缓存并推迟发送。

源码证据：
- `crates/codegen/shell/src/session/actor/updates.rs` — `pub(super) async fn send_grow_notification(`。
- `crates/codegen/shell/src/session/actor/updates.rs` — `pub(super) async fn finish_sampling_failure_notification`。

- `crates/codegen/shell/src/session/actor/updates.rs` — `async fn sampling_failure_notice_waits_for_exact_turn_outcome`（测试源码已读，未运行）。
### Requirement: Pager effort slash current model validation and patch routing

EffortCommand SHALL 标记session_scoped且offered_when_session_less=true，接受可选level。suggest_args忽略query，从ModelState::reasoning_effort_options取得当前模型选项；空列表返回None，否则用共享build_effort_arg_items按option id插入，并按当前canonical effort标记active。run在检查参数前先要求models.current，缺失时即使参数为空也返回No active model；有current且trim参数为空时打开effort picker。非空时以当前model ID和原trim token调用resolve_effort_for_model，成功返回PatchEffort{model_id,current resolved canonical effort}，错误直接使用resolver message；因此none/minimal只在模型菜单提供时可用，remapped id可产生不同canonical value，非reasoning模型拒绝。命令不检查session、不会重新选择model；Action携带的model ID用于本地验证/展示，Shell与最新目标合成的并发语义由下游负责。

#### Scenario: Remapped effort ID
- **WHEN** 菜单项id为deep、canonical value为xhigh且运行/effort deep
- **THEN** 产生model ID不变、effort为Xhigh的PatchEffort。

#### Scenario: No current model with bare command
- **WHEN** models.current为None且参数为空
- **THEN** 先返回No active model，不打开空picker。

证据：`crates/codegen/pager/src/slash/commands/effort.rs` — `EffortCommand::session_scoped / offered_when_session_less / suggest_args / run`。

### Requirement: Pager model slash catalog rows and chained effort suggestions

ModelCommand SHALL 标记session_scoped且offered_when_session_less=true，接受可选model与effort；模型为空时suggest_args返回None。模型阶段按ModelState.available迭代顺序为每个catalog ID生成一行，display/match/insert均不使用友好display name，当前项只在display追加(current)，description缺失时为空；存在可解析reasoningEfforts meta的模型insert_text追加空格，其他模型不追加。effort阶段仅在query以某个reasoning模型catalog ID忽略ASCII大小写开头且其后至少存在一个whitespace字符时进入，空白之后可以没有effort文本；候选按ID长度降序避免共享前缀抢占，部分模型ID且未跟空白仍留在模型阶段。effort行来自该模型的reasoning菜单，insert_text补成`<catalog-id> <option-id>`；仅当该模型是current时按current canonical effort标记active。空菜单可返回空Vec而不是None；suggestion只构造候选，不切换会话。

#### Scenario: Shared catalog prefix
- **WHEN** reasoning模型ID包含另一个reasoning模型ID且query匹配较长ID后跟空格
- **THEN** 按最长ID进入该模型的effort阶段，短ID不能抢占。

#### Scenario: Friendly display name query
- **WHEN** 用户输入模型的友好display name而不是catalog ID
- **THEN** 不进入该模型的effort阶段，模型候选仍只显示和匹配catalog ID。

源码证据：
- `crates/codegen/pager/src/slash/commands/model.rs` — `ModelCommand::suggest_args / supports_reasoning_effort / detect_effort_phase / build_model_items / build_effort_items`。

### Requirement: Pager model slash exact catalog resolution and optional effort switch

ModelCommand::run SHALL trim全部参数；空值打开model参数picker。非空值先把整个字符串按catalog ID忽略ASCII大小写精确解析，命中即返回SwitchModel且effort=None，因此含空白的完整ID若存在也优先于末token拆分。仅在全串未命中时，才按最终单个whitespace字符拆成trim_end后的非空模型前缀与非空末token；前缀必须按catalog ID忽略ASCII大小写命中，且模型reasoningEfforts meta可解析，随后以该模型菜单解析effort ID/canonical token。成功返回一次SwitchModel并携带canonical effort；菜单拒绝的token直接返回列出有效菜单的effort错误，不能降级为Unknown model。显示名没有解析回退；非reasoning模型加effort、未知模型以及不能拆分的输入返回Unknown model。此命令只产生当前会话的Action，不持久化默认模型；dashboard暂存行为由外层dispatch承担。

#### Scenario: Full catalog ID wins
- **WHEN** catalog同时含reasoning模型grow和完整ID grow-4.5，输入grow-4.5
- **THEN** 选择完整ID且effort=None，不把4.5解释为grow的effort。

#### Scenario: Unoffered effort
- **WHEN** reasoning模型存在但末token不在其菜单中
- **THEN** 返回unknown effort及有效选项，不误报整个输入为Unknown model。

#### Scenario: Display name rejected
- **WHEN** 输入只匹配ModelInfo友好名称
- **THEN** 返回Unknown model，不通过名称切换。

源码证据：
- `crates/codegen/pager/src/slash/commands/model.rs` — `ModelCommand::run / resolve_model / split_trailing_token`。
### Requirement: Pager authoritative model catalog publication

A valid `grow/models/update` SHALL deserialize the shell SessionModelState, convert it to pager ModelState, replace the process-owned `app.models` template even when the old default remains available, recursively update concrete sessions, retry deferred authoritative controls and resynchronize subagent control projections, then return true. Malformed notification parameters warn and return false. The process catalog's current model is a fallback for sessions whose own current model is absent from the new catalog, not an unconditional overwrite of every live selection.

#### Scenario: Valid publication
- **WHEN** SessionModelState parses
- **THEN** the app template and concrete session catalogs are reconciled and true returned.

#### Scenario: Changed default
- **WHEN** the shell publishes a different current model while the old one remains available
- **THEN** the future-session app template adopts the shell current.

#### Scenario: Live selection
- **WHEN** a session's current model remains in the new catalog
- **THEN** catalog update preserves that concrete selection.

#### Scenario: Malformed
- **WHEN** notification parsing fails
- **THEN** a warning is logged and false returned.

证据：`crates/codegen/pager/src/app/acp_handler/settings.rs` — `handle_models_update`、`apply_models_state_update`。

### Requirement: Pager recursive model catalog update with workflow route freeze

Concrete model-catalog reconciliation SHALL update the current AgentView, warning when its selected model is absent and passing the process fallback into `update_catalog`. It recursively visits child views except a child whose parent `subagent_sessions` record has a workflow_run_id; that workflow child and all descendants beneath it keep their frozen runtime route. A child view without matching metadata is treated as non-workflow and updated.

#### Scenario: Current view
- **WHEN** catalog reconciliation visits an AgentView
- **THEN** its catalog is replaced and an unavailable current selection can fall back.

#### Scenario: Ordinary child
- **WHEN** a child has no workflow_run_id
- **THEN** it and its descendants are recursively reconciled.

#### Scenario: Workflow child
- **WHEN** the child's metadata has workflow_run_id
- **THEN** that child subtree is skipped.

#### Scenario: Missing metadata
- **WHEN** a child view has no subagent_sessions record
- **THEN** it is updated as an ordinary child.

证据：`crates/codegen/pager/src/app/acp_handler/settings.rs` — `update_model_catalog_recursively`。

### Requirement: Pager recursive deferred authoritative control retry

After each catalog publication, deferred authoritative controls SHALL be retried for every AgentView that has a session id and recursively for every child view. Results are OR-folded so all children are visited even after a prior change. Workflow-run subtrees are not skipped by this retry helper, although their catalog update may have been skipped; views without session ids only recurse into children. The caller does not use the returned changed flag to decide its final true result.

#### Scenario: Root with id
- **WHEN** the root session id exists
- **THEN** deferred controls are applied against that id.

#### Scenario: Root without id
- **WHEN** the root id is absent
- **THEN** its own retry is skipped but child recursion continues.

#### Scenario: Multiple children
- **WHEN** one child reports a change
- **THEN** remaining children are still visited and the aggregate remains true.

#### Scenario: Workflow subtree
- **WHEN** a workflow child exists
- **THEN** this helper still enters it.

证据：`crates/codegen/pager/src/app/acp_handler/settings.rs` — `retry_authoritative_controls_recursively`。
### Requirement: Pager model auto-switch diagnostics and immutable timeline event

ModelAutoSwitched SHALL warn through tracing and unified logging with previous/new ids, catalog count and up to ten available keys, then append ModelUnavailable. It does not itself change current selection or suppress replay output.

#### Scenario: Notification
- **WHEN** auto-switch arrives
- **THEN** diagnostics and one immutable timeline event are produced.

#### Scenario: Selection
- **WHEN** the event names a new model
- **THEN** this arm does not apply it.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `ModelAutoSwitched`。

### Requirement: Pager authoritative model change local-intent and catalog gates

ModelChanged SHALL defer behind a nonmatching local sampling intent and also defer a model missing from the local catalog. Otherwise it clears deferred state, applies model and parsed optional effort, records user_model_preference and reports whether effective selection changed. Invalid effort text becomes absent.

#### Scenario: Intent mismatch
- **WHEN** pending local sampling does not resolve
- **THEN** the authoritative update is deferred.

#### Scenario: Catalog lag
- **WHEN** the model is absent locally
- **THEN** it remains deferred.

#### Scenario: Apply
- **WHEN** both gates pass
- **THEN** current model, effort and user preference update.

#### Scenario: Invalid effort
- **WHEN** effort parsing fails
- **THEN** effort is treated as absent.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `apply_model_changed`。

### Requirement: Pager deferred model and Agent authoritative control drain

Deferred authoritative model and Agent values SHALL be taken and applied independently. A model can re-defer on catalog lag while Agent applies. Live AgentChanged resolves a matching pending control, defers a mismatch, or applies directly when no control is pending.

#### Scenario: Independent
- **WHEN** both domains are deferred
- **THEN** each is attempted without one blocking the other.

#### Scenario: Agent mismatch
- **WHEN** a pending local Agent intent does not resolve
- **THEN** the authoritative name is deferred.

#### Scenario: Aggregate
- **WHEN** either domain changes
- **THEN** true is returned.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `apply_deferred_authoritative_controls`、`AgentChanged`。

### Requirement: Pager child model and Agent control projection into parent metadata

Child control projection SHALL copy current model and any present Agent name into matching parent SubagentInfo, reporting model or name difference. Missing view/metadata returns false; absent Agent name preserves prior subagent_type.

#### Scenario: Model
- **WHEN** child model differs
- **THEN** parent metadata changes.

#### Scenario: Agent
- **WHEN** a present child Agent name differs
- **THEN** subagent_type changes.

#### Scenario: Missing
- **WHEN** view or metadata is absent
- **THEN** false is returned.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `sync_child_control_projection`。

### Requirement: Pager control state receipt validation projection and terminal notice

ControlStateUpdate SHALL validate receipt-only domain/desired/intent/terminal phase or delegate non-receipt state application. Accepted state and reconnect-intent resolution combine. Superseded, nonterminal, missing-message and uncorrelated replay terminals stay silent; Applied produces Success and Rejected Error with recovery detail, using event id or synthetic epoch/domain/revision identity.

#### Scenario: Valid receipt
- **WHEN** terminal receipt identity is consistent
- **THEN** it is accepted without direct mutation.

#### Scenario: Invalid receipt
- **WHEN** validation fails
- **THEN** false is returned.

#### Scenario: Replay
- **WHEN** a replayed terminal resolves no local intent
- **THEN** no new notice is added.

#### Scenario: Terminal
- **WHEN** Applied or Rejected has a message
- **THEN** a deduplicated success/error Control notice is pushed.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `apply_control_state_update`。
### Requirement: Pager retry terminal projection rate-limit diagnostic and context suppression

Retrying SHALL clear retry activity and retained prompt without a visible row. Exhausted marks model failure, optionally logs RateLimitHit, and appends sanitized rate-limit or attempts/reason RetryFailed. Failed context_length appends ContextTooLarge unless a recent CompactionFailed exists; other failures append typed RetryFailed. Every variant clears in_flight_prompt.

#### Scenario: Retrying
- **WHEN** recovery continues
- **THEN** activity and in-flight prompt clear without a retained error.

#### Scenario: Rate limit
- **WHEN** flagged rate-limited attempts exhaust
- **THEN** a diagnostic and sanitized failure are emitted.

#### Scenario: Context
- **WHEN** context_length follows recent compact failure
- **THEN** duplicate ContextTooLarge is suppressed.

#### Scenario: Other failure
- **WHEN** another error type fails
- **THEN** RetryFailed retains type and message.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `apply_retry_state`、`scrollback_has_recent_compaction_failed`。

### Requirement: Pager local session control intent correlation and reconnect handoff

The pager SHALL correlate Sampling, Agent and Behavior control intents independently with one process client id, stable user-intent generation, monotonically increasing sequence and transport-local dispatch generation. A new intent SHALL replace only the newest local correlation in its domain; deferred and reconnect-restored intents SHALL dispatch in global sequence order, preserve semantic identity and receive a new transport generation. Completion SHALL drain only an exact token, while reconnect terminal projections SHALL additionally match domain, intent and the applied, rejected or superseded target without guessing from current-state equality. New Sampling intents SHALL compose against the newest pending or deferred target without committing it, Behavior SHALL remain optimistic only until its authoritative matching outcome, and parked authoritative model/Agent projections SHALL release independently after their domain drains. Screen-mode handoff SHALL serialize only established sessions with pending controls and restore their original correlation. This file does not prove shell-side admission, persistence of the handoff, process relaunch transfer, RPC ordering, model catalog correctness, or CurrentMode/ModelChanged/AgentChanged handler behavior outside this file.

#### Scenario: Latest intent per domain
- **WHEN** multiple local controls target the same domain
- **THEN** the newest target replaces local correlation while other domains remain independent and stale completions cannot drain it.

#### Scenario: Reconnect rearm
- **WHEN** the ACP transport is replaced or a screen-mode handoff is restored
- **THEN** pending semantic tokens keep client, generation and sequence identity, gain the current dispatch generation and are claimed once in sequence order.

#### Scenario: Exact terminal correlation
- **WHEN** a reconnect projection or authoritative Behavior/Agent/Sampling outcome arrives
- **THEN** the control drains only if intent, domain and outcome target match the pending request.

#### Scenario: Uncommitted composition
- **WHEN** a new effort or Behavior UI choice is interpreted during an outstanding control
- **THEN** it composes with the newest desired target while the committed model and behavior remain server-authoritative.

证据：`crates/codegen/pager/src/app/session/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/agent/models/resolution.rs agent bootstrap, model, and session control contract

crates/codegen/shell/src/agent/models/resolution.rs SHALL 维护 agent bootstrap, model, and session control 的入口 selectable_catalog_key_for_persisted, ModelFallbackNotice, resolve_new_session_model_id, is_campaign_only_flip, resolve_default_model, available_models, ModelGlobSet, compile, matches, resolve_model_catalog, model_offers_reasoning_effort, allowlist_matches_nothing, validate_selectable, entry, catalog, available_for, persisted_exact_key_match_returns_the_key, persisted_exact_key_match_not_selectable_returns_none (plus 6 additional private symbols)。实现显示该边界包含 explicit error/result paths、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** selectable_catalog_key_for_persisted, ModelFallbackNotice, resolve_new_session_model_id, is_campaign_only_flip, resolve_default_model, available_models, ModelGlobSet, compile, matches, resolve_model_catalog, model_offers_reasoning_effort, allowlist_matches_nothing, validate_selectable, entry, catalog, available_for, persisted_exact_key_match_returns_the_key, persisted_exact_key_match_not_selectable_returns_none (plus 6 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/agent/models/resolution.rs`。

### Requirement: Shell crates/codegen/shell/src/agent/models/tests.rs agent bootstrap, model, and session control contract

crates/codegen/shell/src/agent/models/tests.rs SHALL 维护 agent bootstrap, model, and session control 的入口 config, two_model_config, from_config_uses_only_explicit_provider_models, apply_config_preserves_an_existing_session_selection, apply_config_reselects_when_current_model_disappears, explicit_selection_bumps_model_switch_generation_once, configured_filters_fail_closed, task_model_error_lists_provider_qualified_ids, task_selectable_projection_matches_task_model_validation, sampling_config_uses_selected_provider_credentials, invalid_reload_leaves_the_live_snapshot_unchanged, live_sampling_lookup_uses_reloaded_provider_route, reasoning_efforts_come_from_the_selected_catalog_entry。实现显示该边界包含 explicit error/result paths、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** config, two_model_config, from_config_uses_only_explicit_provider_models, apply_config_preserves_an_existing_session_selection, apply_config_reselects_when_current_model_disappears, explicit_selection_bumps_model_switch_generation_once, configured_filters_fail_closed, task_model_error_lists_provider_qualified_ids, task_selectable_projection_matches_task_model_validation, sampling_config_uses_selected_provider_credentials, invalid_reload_leaves_the_live_snapshot_unchanged, live_sampling_lookup_uses_reloaded_provider_route, reasoning_efforts_come_from_the_selected_catalog_entry 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/agent/models/tests.rs`。

### Requirement: Shell crates/codegen/shell/src/agent/models.rs agent bootstrap, model, and session control contract

crates/codegen/shell/src/agent/models.rs SHALL 维护 agent bootstrap, model, and session control 的入口 ModelId, new, str, from, fmt, ModelInfo, description, meta, SessionModelState, from_config_options, task_model_error_for_catalog, task_model_is_selectable, task_selectable_catalog, ModelsManager, CatalogState, PublishedModelCatalog, PublishedSessionRoute, models (plus 32 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** ModelId, new, str, from, fmt, ModelInfo, description, meta, SessionModelState, from_config_options, task_model_error_for_catalog, task_model_is_selectable, task_selectable_catalog, ModelsManager, CatalogState, PublishedModelCatalog, PublishedSessionRoute, models (plus 32 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** ModelId, new, str, from, fmt, ModelInfo, description, meta, SessionModelState, from_config_options, task_model_error_for_catalog, task_model_is_selectable, task_selectable_catalog, ModelsManager, CatalogState, PublishedModelCatalog, PublishedSessionRoute, models (plus 32 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/agent/models.rs`。

### Requirement: Shell crates/codegen/shell/src/sampling/conversation.rs sampling conversation and error model contract

crates/codegen/shell/src/sampling/conversation.rs SHALL 维护 sampling conversation and error model 的入口 the file module entrypoint。实现显示该边界包含 模块内函数、类型和常量组合；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 the file module entrypoint 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/sampling/conversation.rs`。

### Requirement: Shell crates/codegen/shell/src/sampling/error.rs sampling conversation and error model contract

crates/codegen/shell/src/sampling/error.rs SHALL 维护 sampling conversation and error model 的入口 RATE_LIMITED_ERROR_CODE, RATE_LIMITED_USER_MESSAGE, format_rate_limited_user_message, strip_sampling_api_error_prefix, PREFIX, SEP, pushes_consumer_subscription_upsell, OVERLOADED_USER_MESSAGE, map_sampling_err_to_acp, error_data_with_status, terminal_error_data, context_window_exceeded_for_turn_error, error_message_from_data, error_detail_from_data, http_status_from_error, PROMPT_USAGE_DATA_KEY, attach_prompt_usage, prompt_usage_from_error (plus 21 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、hook dispatch or hook source boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** RATE_LIMITED_ERROR_CODE, RATE_LIMITED_USER_MESSAGE, format_rate_limited_user_message, strip_sampling_api_error_prefix, PREFIX, SEP, pushes_consumer_subscription_upsell, OVERLOADED_USER_MESSAGE, map_sampling_err_to_acp, error_data_with_status, terminal_error_data, context_window_exceeded_for_turn_error, error_message_from_data, error_detail_from_data, http_status_from_error, PROMPT_USAGE_DATA_KEY, attach_prompt_usage, prompt_usage_from_error (plus 21 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/sampling/error.rs`。

### Requirement: Shell crates/codegen/shell/src/sampling/mod.rs sampling conversation and error model contract

crates/codegen/shell/src/sampling/mod.rs SHALL 维护 sampling conversation and error model 的入口 the file module entrypoint。实现显示该边界包含 explicit error/result paths；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** the file module entrypoint 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

证据：`crates/codegen/shell/src/sampling/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/sampling/types.rs sampling conversation and error model contract

crates/codegen/shell/src/sampling/types.rs SHALL 维护 sampling conversation and error model 的入口 get_image_content_url, get_image_content_url_returns_uri_when_present, get_image_content_url_builds_data_uri_when_no_uri。实现显示该边界包含 platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/sampling/types.rs`。
### Requirement: Tools crates/codegen/tools/src/normalization.rs tools crate module boundary contract
crates/codegen/tools/src/normalization.rs SHALL implement the tools crate module boundary boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols tool_identity_of, merge_tool_meta, norm_offset_i64, canonical_input, req, opt, obj, str, parse, canonical_omits_absent_options_not_null follow explicit markers serde/json wire or configuration、child process execution、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、image/PDF/media processing、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/normalization.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/normalization.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/normalization.rs` — `tool_identity_of`；`crates/codegen/tools/src/normalization.rs` — `merge_tool_meta`；`crates/codegen/tools/src/normalization.rs` — `norm_offset_i64`；`crates/codegen/tools/src/normalization.rs` — `canonical_input`；`crates/codegen/tools/src/normalization.rs` — `req`；`crates/codegen/tools/src/normalization.rs` — `opt`；`crates/codegen/tools/src/normalization.rs` — `obj`；`crates/codegen/tools/src/normalization.rs` — `str`；`crates/codegen/tools/src/normalization.rs` — `parse`；`crates/codegen/tools/src/normalization.rs` — `canonical_omits_absent_options_not_null`。

### Requirement: Tools crates/codegen/tools/src/retry.rs tools crate module boundary contract
crates/codegen/tools/src/retry.rs SHALL implement the tools crate module boundary boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols BackoffConfig, default, new, calculate_delay, execute_with_backoff, to, ExecuteFn, Error, Fut, call, test_backoff_config_default, test_calculate_delay, test_calculate_delay_custom_config, test_backoff_config_serde_roundtrip follow explicit markers serde/json wire or configuration、explicit error classification、timeout, budget, or rate limit、platform/feature conditional、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/retry.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/retry.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/retry.rs` — `BackoffConfig`；`crates/codegen/tools/src/retry.rs` — `default`；`crates/codegen/tools/src/retry.rs` — `new`；`crates/codegen/tools/src/retry.rs` — `calculate_delay`；`crates/codegen/tools/src/retry.rs` — `execute_with_backoff`；`crates/codegen/tools/src/retry.rs` — `to`；`crates/codegen/tools/src/retry.rs` — `ExecuteFn`；`crates/codegen/tools/src/retry.rs` — `Error`；`crates/codegen/tools/src/retry.rs` — `Fut`；`crates/codegen/tools/src/retry.rs` — `call`；`crates/codegen/tools/src/retry.rs` — `test_backoff_config_default`；`crates/codegen/tools/src/retry.rs` — `test_calculate_delay`；`crates/codegen/tools/src/retry.rs` — `test_calculate_delay_custom_config`；`crates/codegen/tools/src/retry.rs` — `test_backoff_config_serde_roundtrip`。
### Requirement: Pager task-result test: switch_model_rpc_waits_for_authoritative_projection_without_local_notice
A successful model-switch transport acknowledgement SHALL leave the Shell-owned switch pending until authoritative ModelChanged projection; it SHALL not persist defaults or duplicate terminal feedback.

#### Scenario: Authoritative model projection
- **WHEN** RPC acknowledgement precedes ModelChanged
- **THEN** the committed model changes only on the notification and no persistence effect is emitted.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `switch_model_rpc_waits_for_authoritative_projection_without_local_notice`。

### Requirement: Pager task-result test: authoritative_switch_projections_update_the_exact_subagent_view
Authoritative model and Agent projections SHALL resolve against the exact child session view identified by session id, leaving the root view unchanged.

#### Scenario: Subagent control projection
- **WHEN** controls target a child session
- **THEN** only the child commits model and Agent state and clears its pending controls.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `authoritative_switch_projections_update_the_exact_subagent_view`。

### Requirement: Pager task-result test: switch_model_complete_noop_waits_for_shell_notice_and_skips_persist
A no-op model switch SHALL still wait for ModelChanged terminal projection and SHALL not persist a preferred model or emit duplicate feedback.

#### Scenario: No-op model switch
- **WHEN** RPC acknowledgement and matching notification arrive for the current model/effort
- **THEN** pending clears on notification and no persistence effect is emitted.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `switch_model_complete_noop_waits_for_shell_notice_and_skips_persist`。

### Requirement: Pager task-result test: switch_model_complete_resolves_effort_from_catalog_meta_session_only
When no effort is requested, model metadata SHALL determine the authoritative session reasoning effort after ModelChanged, while the switch remains session-scoped and does not write config defaults.

#### Scenario: Catalog effort resolution
- **WHEN** the selected catalog model advertises xhigh metadata
- **THEN** the session commits Xhigh on notification and persistence remains empty.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `switch_model_complete_resolves_effort_from_catalog_meta_session_only`。

### Requirement: Pager task-result test: switch_to_non_reasoning_model_clears_session_effort
An authoritative switch to a model without a reasoning-effort menu SHALL clear the session effort, while transport completion alone leaves the prior effort unchanged.

#### Scenario: Non-reasoning model
- **WHEN** ModelChanged reports no reasoning effort
- **THEN** the committed session effort becomes None without persisting defaults.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `switch_to_non_reasoning_model_clears_session_effort`。

### Requirement: Pager task-result test: switch_model_complete_failure_pushes_error_and_clears_pending
A local model-switch failure SHALL clear the pending control, preserve the prior committed model, and append one error row.

#### Scenario: Model switch failure
- **WHEN** the model RPC returns a local failure
- **THEN** pending is false, current model is unchanged, and scrollback grows by one.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `switch_model_complete_failure_pushes_error_and_clears_pending`。

### Requirement: Pager task-result test: failed_model_switch_preserves_local_selection
A failed model switch SHALL not roll back an already projected local selection when no speculative rollback is owned by the control correlation.

#### Scenario: Projected model failure
- **WHEN** the selected model is already locally current when the RPC fails
- **THEN** the current model remains selected.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `failed_model_switch_preserves_local_selection`。

### Requirement: Pager task-result test: model_switch_succeeds_without_changing_agent
A successful model projection SHALL change sampling state without changing Agent identity, opening a question, or persisting the default model.

#### Scenario: Model-only switch
- **WHEN** a different model is selected and ModelChanged arrives
- **THEN** the model changes, Agent identity remains stable, and no persistence effect is emitted.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `model_switch_succeeds_without_changing_agent`。

### Requirement: Pager task-result test: switch_model_stays_pending_until_authoritative_projection
Model switch pending state SHALL span dispatch and accepted RPC transport and clear only after ModelChanged projection.

#### Scenario: Model pending lifecycle
- **WHEN** the action dispatch, acknowledgement, and authoritative notification occur in order
- **THEN** pending is true through acknowledgement and false after notification.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `switch_model_stays_pending_until_authoritative_projection`。

### Requirement: Pager task-result test: sampling_completion_order_keeps_the_latest_model_as_effort_base
Model switch completion order SHALL not regress the effort base: both acknowledgement-first and notification-first orders use the latest model for a subsequent effort change.

#### Scenario: Sampling completion order
- **WHEN** model acknowledgement and ModelChanged arrive in either order
- **THEN** the next low-effort control targets the latest model.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `sampling_completion_order_keeps_the_latest_model_as_effort_base`。

### Requirement: Pager task-result test: reconnect_terminal_superseded_drains_local_sampling_desired
A terminal Superseded result for the reconnect retry SHALL drain the local desired sampling control without committing the superseded target.

#### Scenario: Reconnect supersession
- **WHEN** a reconnect retry receives Superseded
- **THEN** sampling control is no longer pending and the target is not committed.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `reconnect_terminal_superseded_drains_local_sampling_desired`。

### Requirement: Pager task-result test: no_deferred_switch_means_no_extra_effect
SessionCreated SHALL not emit a SwitchModel effect when no deferred model switch exists.

#### Scenario: Session creation without deferred switch
- **WHEN** a new session is created with no deferred target
- **THEN** no switch effect and no pending model control are produced.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `no_deferred_switch_means_no_extra_effect`。
### Requirement: Pager agent input test: ctrl_x_m_opens_configured_model_picker
Ctrl-X then M SHALL open the model command picker against the configured model catalog.

#### Scenario: Leader model picker
- **WHEN** the leader sequence is entered with a configured current model
- **THEN** OpenCommandPicker targets model.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `ctrl_x_m_opens_configured_model_picker`。

### Requirement: Pager agent input test: ctrl_x_e_and_p_open_their_shared_selectors
Ctrl-X then E and Ctrl-X then P SHALL open the effort and permission selectors respectively.

#### Scenario: Leader selector routing
- **WHEN** each leader continuation is entered
- **THEN** the matching command picker action is returned.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `ctrl_x_e_and_p_open_their_shared_selectors`。
