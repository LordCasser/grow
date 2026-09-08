## ADDED Requirements

### Requirement: Typed invocation context
TypedExtensions SHALL 按 Rust TypeId 保存 Arc 值，同类型插入覆盖，读取和删除返回可选 Arc；clone 复制映射并共享已有值，merge_defaults 只填补缺失类型。ToolCallContext 默认生成 UUID v7，显式构造保留 call_id。

#### Scenario: 克隆与默认合并
- **WHEN** 克隆 context 后删除某个 extension，再合并 defaults
- **THEN** 原 context 的映射不受删除影响；现有同类型值不被 defaults 覆盖。

#### Scenario: 环境载体
- **WHEN** 读取未安装的 Cwd、SessionContext 或 Cancellation
- **THEN** 得到 None，本层不推导进程默认目录或执行取消；WorkspaceViewerContext 的 stream_tool_progress 缺省 false。

证据：`crates/common/tool-runtime/src/context.rs` — `TypedExtensions`；`crates/common/tool-runtime/src/context.rs` — `ToolCallContext`；`crates/common/tool-runtime/src/context.rs` — `WorkspaceViewerContext`。

### Requirement: Typed tool execution stream
Tool SHALL 以 execute 为入口，默认 await run 后返回一个 Terminal；未实现两者时返回 NotImplemented。terminal_only 构造单终值流，with_progress 先排空 progress 再 await terminal future。

#### Scenario: 空进度
- **WHEN** with_progress 的 progress 流为空
- **THEN** 仍输出一个 Terminal；手写 ToolStream 的唯一终值约束由实现者遵守，类型别名不自动校验。

#### Scenario: 默认描述
- **WHEN** 工具未覆盖 should_list 或 has_dynamic_description
- **THEN** 分别为 true 和 false；description 接收与 should_list 相同的 ListToolsContext。

证据：`crates/common/tool-runtime/src/tool.rs` — `Tool`；`crates/common/tool-runtime/src/tool.rs` — `terminal_only`；`crates/common/tool-runtime/src/tool.rs` — `with_progress`。

### Requirement: JSON tool erasure
ToolDyn blanket 与 ErasedTool SHALL 反序列化 Args、调用 typed execute、透传 Progress 与错误终值；成功输出打包 tool_id、JSON value、非空自定义或自动提取的 model_output 及可选 chat_completion_output。

#### Scenario: 非法参数
- **WHEN** JSON 不满足 Args 的反序列化要求
- **THEN** 返回 InvalidArguments 终值，不调用 typed execute。

#### Scenario: 输出编码失败
- **WHEN** typed Output 无法序列化
- **THEN** ToolDyn 返回带 source 的 Execution；ErasedTool 返回 Custom，code 为 output_encoding。

#### Scenario: 现成 JSON
- **WHEN** TypedToolOutput::from_value 构造结果
- **THEN** 自动提取模型内容，chat_completion_output 为 None，可通过 with_chat_completion_output 设置。

证据：`crates/common/tool-runtime/src/tool.rs` — `ToolDyn`；`crates/common/tool-runtime/src/tool.rs` — `TypedToolOutput`；`crates/common/tool-runtime/src/registry.rs` — `ErasedTool`。

### Requirement: Terminal dispatch adapter
ToolDispatch::call_terminal SHALL 丢弃 Progress 并返回遇到的第一个 Terminal；流结束且没有 Terminal 时返回 Custom 错误 stream_no_terminal。

#### Scenario: 无终值流
- **WHEN** call 返回空流或只有进度
- **THEN** call_terminal 返回 stream_no_terminal；它不校验第一个 Terminal 之后是否还有重复终值。

证据：`crates/common/tool-runtime/src/dispatch.rs` — `call_terminal`。

### Requirement: Local registration and aliases
LocalRegistry SHALL 共享有序工具表和 extractor 表，支持 typed、Arc、dynamic 注册、查找、别名和注销；同 ID 注册返回被替换 handle，list_tools 使用 should_list 过滤并按上下文生成 description。

#### Scenario: 别名
- **WHEN** 为存在的 target 注册 alias
- **THEN** alias 保存当前 target handle，并在存在时复制 extractor；target 缺失则返回 false，后续替换 target 不自动重定向 alias。

#### Scenario: 注销和提取
- **WHEN** 注销 ID 或用已登记 extractor 提取 JSON
- **THEN** 注销同时移除该 ID 的 handle 和 extractor；extractor_for 类型解析失败返回 None，自定义空 blocks 时自动提取。

证据：`crates/common/tool-runtime/src/registry.rs` — `LocalRegistry`；`crates/common/tool-runtime/src/render.rs` — `extractor_for`。

### Requirement: Tool family variant interface
ToolFamily SHALL 提供同一 ID 的 Default 或不透明命名 Variant 查询与枚举接口，default_variant_name 默认 None；具体路由由 family 实现。

#### Scenario: 命名 default
- **WHEN** 比较 ToolVariant::Default 与 Variant("default")
- **THEN** 两者不同；接口不自动添加 fallback 或构造缺失 variant。

证据：`crates/common/tool-runtime/src/tool.rs` — `ToolFamily`；`crates/common/tool-runtime/src/tool.rs` — `ToolVariant`。

### Requirement: Model content extraction precedence
extract_content_blocks SHALL 按单 block、含 block 形状的数组、ToolRunResult 的 prompt_text、MCP content、混合对象、普通文本的顺序提取；字符串不加 JSON 引号，其他普通值使用 JSON 文本。

#### Scenario: MCP 结构内容
- **WHEN** 非空 content 至少有一个已知 block type，且 structuredContent 非 null
- **THEN** 先输出 structuredContent JSON 文本，再按原序输出 content 元素；非 block 元素转文本。

#### Scenario: 工具运行结果
- **WHEN** 对象有字符串 prompt_text 且存在 output 和 effective_tool_name 键
- **THEN** 模型只看到 prompt_text。

#### Scenario: 混合对象
- **WHEN** 字段中有合法 block 或全部合法 blocks 的非空数组
- **THEN** 先输出剩余字段 JSON，再输出提取的 blocks；混合数组保留在剩余字段，避免丢失普通数据。

#### Scenario: 空或畸形输入
- **WHEN** 没有可提取的合法 block
- **THEN** 返回至少一个文本 block；已知 type 的畸形对象不能绕过 ContentBlock 的字段校验。

证据：`crates/common/tool-runtime/src/render.rs` — `extract_content_blocks`；`crates/common/tool-runtime/src/render.rs` — `classify_field`；`crates/common/tool-runtime/src/render.rs` — `value_to_block`。

### Requirement: Rich progress and client frames
ToolProgress SHALL 以 kind=text/content/custom 序列化，Custom 用独立 subkind 和任意 JSON payload；ContentBlock 以 type 区分 text/image/resource 并拒绝未知字段。

#### Scenario: 图像与资源
- **WHEN** 构造 image 或 resource 内容
- **THEN** image 保存 mime_type、base64 data 和可选 media_id/filename/path/metadata；resource 保存 uri 与可选 mime_type/text，本层不读取资源或验证 base64。

#### Scenario: 前端输出
- **WHEN** ToolOutput 提供 chat_completion_output
- **THEN** 保留 result 与 stream_error，结果含 message/tag/card/code execution 和 flattened extra；sender 的字符串默认空，并不强制 assistant，执行结果缺省空 stdout/stderr、exit_code=0、command_timed_out=false。

证据：`crates/common/tool-runtime/src/tool.rs` — `ToolProgress`；`crates/common/tool-runtime/src/tool.rs` — `ContentBlock`；`crates/common/tool-runtime/src/render.rs` — `ToolChatCompletionResponse`；`crates/common/tool-runtime/src/render.rs` — `ToolCodeExecutionResult`。

### Requirement: Monotonic progress chunking
stream_chunk SHALL 以 total 与 last_total 的差值从 tail 取新字节，默认 cap 16 KiB，可由 StreamingSpec 覆盖；按实际消费推进游标，丢失的上游中间字节用 gap 标记，truncated 原样透传。

#### Scenario: 无新输出
- **WHEN** total 不大于 last_total
- **THEN** 返回 None 且不推进游标。

#### Scenario: 超出尾缓冲
- **WHEN** 未消费字节数大于 tail 长度
- **THEN** 从存活 tail 输出，并将已丢失部分计入推进量，gap=true；单帧未消费部分可在后续调用继续输出。

#### Scenario: UTF-8 边界
- **WHEN** 有完整前缀且 cap 截到未完成字符
- **THEN** 先输出完整前缀并保留未消费字节；cut 为零且还有字节时兜底至少取至多四字节起步，可能超出 cap，采用 from_utf8_lossy。仅有未完成字符时可能消费为替换字符，当前实现不保证跨 tick 无损。

#### Scenario: payload 校验
- **WHEN** 反序列化 PartialResultPayload
- **THEN** delta 和 total_bytes 必需，gap/truncated 缺省 false，未知字段拒绝。

证据：`crates/common/tool-runtime/src/streaming.rs` — `stream_chunk`；`crates/common/tool-runtime/src/streaming.rs` — `PartialResultPayload`。

### Requirement: Typed error representation
ToolError SHALL 区分未实现、参数、未找到、权限、认证、超时、取消、限流、服务不可用、网络、执行、渲染限制、终端与 Custom 错误，保存可选结构 details 和不参与序列化的 source。

#### Scenario: 显示和传输
- **WHEN** Display、Debug 或 serde 序列化错误
- **THEN** Display 只输出 detail，Debug 可含 source chain；kind serde 使用 Rust variant 名，as_str 提供 snake_case 名，两者不可混用。

#### Scenario: 附加信息
- **WHEN** 使用 custom 或带工具 ID 的构造器
- **THEN** custom 将 code 放入 details；not_found/timeout/cancelled/execution/terminal_error 附加 tool_id，serde_json 错误转 InvalidArguments。

证据：`crates/common/tool-runtime/src/error.rs` — `ToolError`；`crates/common/tool-runtime/src/error.rs` — `ToolErrorKind`。

### Requirement: Best effort notification channel
ToolNotificationHandle SHALL 使用 futures 无界 MPSC，支持 channel/new/from_sender、克隆、noop 及每个通知 variant 的 send helper；发送为 best effort，接收端关闭时静默丢弃。

#### Scenario: 通知种类
- **WHEN** 序列化 ToolNotification
- **THEN** type 使用 PascalCase，覆盖 BashOutputChunk/Complete/Timeout/Backgrounded/Failed、FileWritten、TaskCompleted、UserQuestionAsked、LSP Starting/Ready/Crashed/Retrying/Failed、ScheduledTask Fired/Removed/Created 和 MonitorEvent 共 17 类。

#### Scenario: 读取通知
- **WHEN** 使用公开 FileRead 结构
- **THEN** 它没有对应 ToolNotification variant 或 send helper，不能据此声称文件读取会发出通知。

证据：`crates/common/tool-runtime/src/notification.rs` — `ToolNotification`；`crates/common/tool-runtime/src/notification.rs` — `ToolNotificationHandle`；`crates/common/tool-runtime/src/notification.rs` — `FileRead`。

### Requirement: Notification payload and task snapshots
通知 payload SHALL 保留 Bash 调用 ID、命令、原始输出字节、总字节、截断和 cwd，按事件额外携带退出/信号、超时、后台 task/output_file 或启动失败信息；FileWritten 携带写入前后内容及新建标记。

#### Scenario: 后台时长
- **WHEN** TaskSnapshot 调用 duration_secs
- **THEN** 用 end_time 或当前时间减 start_time，逆向时间返回 0；kind 缺省 Bash，另支持 Monitor，display_command 可选且区别于实际 command。

#### Scenario: 其他事件载荷
- **WHEN** 构造用户问题、LSP 重试、调度或 Monitor 通知
- **THEN** 保留任意 questions_json、重试计数与 backoff、调度 prompt/human_schedule/可选 next_fire_at 或原始文本/XML event_text；本层只传输，不执行调度、进程重启或 XML 包装。

#### Scenario: 输出字节
- **WHEN** BashNotificationBase 输出非法 UTF-8 或检查 was_signaled
- **THEN** output_lossy 使用替换字符；was_signaled 仅判断 signal 是否存在，不额外校验 exit_code 的一致性。

证据：`crates/common/tool-runtime/src/notification.rs` — `BashNotificationBase`；`crates/common/tool-runtime/src/notification.rs` — `TaskSnapshot`；`crates/common/tool-runtime/src/notification.rs` — `MonitorEvent`。

### Requirement: Search snapshot interface
ToolSearchIndex SHALL 提供 Send+Sync 的 search_snapshot(query,limit) 和 server summaries 接口，结果携带工具/服务名、描述、分数、参数顺序、完整 input_schema，以及同一快照的 hidden 数与 ready 标记。

#### Scenario: 接口边界
- **WHEN** 使用 ToolIndex 或 ServerSummary
- **THEN** ToolIndex 克隆共享 Arc；tool_count 返回 tool_names 长度，搜索排序、字母序、限额和快照一致性由具体后端实现，本包不提供搜索算法或自动排序。

证据：`crates/common/tool-runtime/src/search.rs` — `ToolSearchIndex`；`crates/common/tool-runtime/src/search.rs` — `ToolIndex`；`crates/common/tool-runtime/src/search.rs` — `ServerSummary`。

### Requirement: Tool descriptions preserve raw schema
ToolDescription SHALL 保存 name、description、可选 namespace/title/kind、原始 arguments_schema 和不序列化的 extra；to_input_schema 原样返回原始 schema，缺失时返回空 object schema。

#### Scenario: 显式验证
- **WHEN** 调用 validate
- **THEN** 收集 name 与 namespace 的所有标识符错误，要求非空且仅 ASCII 字母数字/下划线/连字符；构造和 serde 不自动调用 validate，不校验 schema 内容。

#### Scenario: 显示与派生
- **WHEN** 调用 Display 或 to_arguments_lossy
- **THEN** Display 使用 namespace.name 与非空 description，不使用 title，也不是规范 ID；参数视图有损，未附 schema 则为空。

证据：`crates/common/tool-types/src/types.rs` — `ToolDescription`；`crates/common/tool-types/src/types.rs` — `validate_identifier`。

### Requirement: Runtime description extensions
Extensions SHALL 按 TypeId 保存可克隆的 Box 值，支持 set/get/get_mut/remove/contains；克隆时调用值自身 Clone，Debug 只显示长度。

#### Scenario: 比较和传输
- **WHEN** 比较不同 Extensions 或序列化 ToolDescription
- **THEN** Extensions 恒等比较 true，ToolDescription 的相等性忽略这些元数据，serde 跳过 extra；不能据此判断实际 runtime metadata 相同。

证据：`crates/common/tool-types/src/ext.rs` — `Extensions`；`crates/common/tool-types/src/ext.rs` — `clone_any`。

### Requirement: Argument type and constraint metadata
ToolArgument SHALL 保存类型、required、default、allowed_values、可选原始 schema 和四种数值边界；新建时类型 String、required=true，设置 default 不改变 required。

#### Scenario: 类型表示
- **WHEN** 使用 SchemaType 的 Single 或 Multiple
- **THEN** 支持 string/integer/number/boolean/array/object/null；primary_type 取首个非 null，缺失则 Null；nullable 按是否包含 Null，primitive 要求所有成员 primitive，composite/numeric 只需任一成员满足。

#### Scenario: 宽松解析与 serde
- **WHEN** SchemaType::from_value 遇到未知值或数组内非法元素
- **THEN** 过滤非法成员，无有效成员回退 String，单个成员归一为 Single；直接 serde 对未知类型返回错误，不等同于 from_value。

#### Scenario: 约束字段
- **WHEN** 设置 required=false 或 numeric bounds
- **THEN** required=false 写入 JSON，true 省略；边界仅为元数据，本层不验证参数值满足限制。

证据：`crates/common/tool-types/src/types.rs` — `ToolArgument`；`crates/common/tool-types/src/types.rs` — `SchemaType`；`crates/common/tool-types/src/types.rs` — `ArgumentType`。

### Requirement: Lossy schema property projection
parse_arguments_from_schema_lossy SHALL 只遍历顶层 properties，提取 description、required、default、allowed_values 优先于 enum、type 和四种数值边界；不存在对象 properties 返回空。

#### Scenario: 有限 schema 支持
- **WHEN** 输入含复杂 schema
- **THEN** 不进行完整 JSON Schema 校验；array/object 保留属性原文，普通属性的 pattern/format/组合与条件限制不会成为独立 ToolArgument 约束。

#### Scenario: 枚举引用
- **WHEN** 直接 ref 或 anyOf 分支引用本地 #/$defs 枚举
- **THEN** 提取非空 enum 或 oneOf const，类型由首个值推导，首个枚举值成为 resolved default 并优先于属性 default；anyOf 中的 null 分支可能不保留，结果不能代替原 schema。

#### Scenario: 无 ref 的联合
- **WHEN** anyOf 有可识别 type 且属性无 type
- **THEN** 组合可识别类型并保留属性原始 schema；不解析远程 ref 或任意递归引用。

证据：`crates/common/tool-types/src/schema_utils.rs` — `parse_arguments_from_schema_lossy`；`crates/common/tool-types/src/schema_utils.rs` — `resolve_ref_type`；`crates/common/tool-types/src/schema_utils.rs` — `extract_enum_from_def`。

### Requirement: Lenient argument conversion helpers
布尔转换辅助函数 SHALL 接受 bool、整数 0/1、trim 后不区分大小写的 true/false/yes/no/1/0，null 转 false；不识别形式返回 None 或 serde 错误。

#### Scenario: 可选布尔
- **WHEN** optional 布尔字段缺失或显式 null
- **THEN** 配合 serde default，缺失为 None，显式 null 为 Some(false)；1.0、其他数字、空字符串和对象拒绝。

#### Scenario: 字符串列表
- **WHEN** 调用 lenient_string_list_from_json
- **THEN** 字符串或数字转单元素列表，数组成员只接受字符串/数字，null 为空；bool、对象、嵌套数组拒绝。这些辅助函数不会自动应用到所有 task 输入。

证据：`crates/common/tool-types/src/serde_lenient.rs` — `lenient_bool_from_json`；`crates/common/tool-types/src/serde_lenient.rs` — `deserialize_lenient_option_bool`；`crates/common/tool-types/src/serde_lenient.rs` — `lenient_string_list_from_json`。

### Requirement: Task spawn input and optional sentinels
TaskToolInput SHALL 要求 prompt 与 description，缺省 subagent_type=general-purpose、run_in_background=true，后者使用宽松布尔解析；保留 capability_mode/isolation/resume_from/cwd/model/task_id 可选字段。

#### Scenario: Schema 与运行时边界
- **WHEN** 构造输入或生成 schema
- **THEN** task_id 被 schemars 隐藏但 serde 可接收；目录存在性、resume 归属和状态、model 选择及 worktree 互斥由启动实现核验，本结构不执行。

#### Scenario: 可选参数清理
- **WHEN** 调用 sanitize_optional_arg
- **THEN** trim 并删除空白、null/none/undefined 的不区分大小写哨兵；serde 不自动调用此函数。

证据：`crates/common/tool-types/src/task.rs` — `TaskToolInput`；`crates/common/tool-types/src/task.rs` — `sanitize_optional_arg`。

### Requirement: Task output query normalization
TaskOutputToolInput SHALL 严格接收 task_ids 字符串数组与可选 u64 timeout_ms，拒绝未知字段；resolved_task_ids trim、去空并按首次出现顺序去重。

#### Scenario: 查询输入
- **WHEN** 使用单数 task_id、字符串代替数组或数字 ID 成员
- **THEN** serde 拒绝；MAX_MULTI_WAIT_IDS=20 为共享常量，本结构本身不限制数组长度。

#### Scenario: 等待判定
- **WHEN** timeout_ms 缺失、0 或正数
- **THEN** 前两者 waits=false，正数 true；raw JSON 辅助函数只认可非负整数，不把数字字符串当等待。

#### Scenario: 等待上限
- **WHEN** 读取 max_wait_block_ms
- **THEN** 每次从 GROW_MAX_WAIT_BLOCK_MS 解析 u64，失败回退 600000，合法 0 也接受；format_wait_cap_ms 秒/分钟向下取整，description 保留 max_wait_ms 占位符由下游解析，本包不执行阻塞或 cap。

证据：`crates/common/tool-types/src/task.rs` — `TaskOutputToolInput`；`crates/common/tool-types/src/task.rs` — `resolve_task_ids`；`crates/common/tool-types/src/task.rs` — `max_wait_block_ms`；`crates/common/tool-types/src/task.rs` — `task_output_waits_from_json`。

### Requirement: Task result state and progress signature
TaskOutputResult SHALL 保留状态、时间、输出、文件、截断提示及 raw_output_bytes；is_terminal 只将 completed/failed/cancelled 视为终态。

#### Scenario: 外层变体
- **WHEN** 检查 TaskOutputOutput::is_terminal
- **THEN** Result 委托内部状态，TaskNotFound 为 false，MultiResult 总为 true，不逐项检查结果。

#### Scenario: 进度指纹
- **WHEN** 调用 progress_signature
- **THEN** 仅哈希 status、exit_code、ended 是否存在和 raw_output_bytes；不使用显示文本长度、duration 或结束时间具体值，不承诺跨版本稳定哈希。

#### Scenario: 终止结果
- **WHEN** 检查 KillTaskOutput::was_killed
- **THEN** 仅 Result.outcome==killed 返回 true，already_exited 和 TaskNotFound 为 false；本包不杀进程。

证据：`crates/common/tool-types/src/task.rs` — `TaskOutputResult`；`crates/common/tool-types/src/task.rs` — `TaskOutputOutput`；`crates/common/tool-types/src/task.rs` — `KillTaskOutput`。

### Requirement: Subagent model result formatting
SubagentCompletedOutput SHALL 保留 output、子 Agent ID/type、调用/turn/耗时统计及可选 worktree_path；to_model_text 输出回答、subagent_meta 和含 resume_from 提示的 subagent_result footer。

#### Scenario: 后台启动与完成
- **WHEN** 调用后台提示或完成格式化 helper
- **THEN** 后台提示含 ID/type/description 和传入的查询工具名；完成格式化不自动把 worktree_path 加入模型文本，字符串字段直接插值，本层不校验或转义。

证据：`crates/common/tool-types/src/task.rs` — `SubagentCompletedOutput`；`crates/common/tool-types/src/task.rs` — `format_subagent_started_background`；`crates/common/tool-types/src/task.rs` — `format_subagent_completed`。

### Requirement: Builtin subagent prompt catalog
BUILTIN_SUBAGENTS SHALL 按 general-purpose、explore 顺序提供描述、工具片段和提示模板；按名称精确匹配，未知或用户自定义类型返回 None。

#### Scenario: 工具片段
- **WHEN** render_tools 使用 SubagentToolNaming
- **THEN** 替换 execute/read/edit/list/search/plan 对应名称，未知占位符回退末段 kind，未闭合占位符原样保留；to_descriptor 保留名称描述并设置工具片段。

#### Scenario: 提示渲染开关
- **WHEN** 启用 prompt-render feature 调用 render_prompt
- **THEN** 使用 MiniJinja 自定义分隔符与 tools.by_kind，上下文缺失 kind 留空且条件段可隐藏，失败返回 None；general-purpose 提示限定任务范围和工作区，explore 提示只读，这些文字不替代运行时权限。

证据：`crates/common/tool-types/src/task.rs` — `BUILTIN_SUBAGENTS`；`crates/common/tool-types/src/task.rs` — `BuiltinSubagent`；`crates/common/tool-types/src/task.rs` — `substitute_tool_placeholders`。

### Requirement: Task lifecycle description builders
任务描述 builders SHALL 按调用者提供的工具名、参数名和可用工具生成 task/get_task_output/kill_task 文本，保留传入模板占位符供下游解析。

#### Scenario: 启动描述
- **WHEN** build_task_description 接收动态子 Agent 列表
- **THEN** 按列表顺序输出名称/描述/可选工具片段，并说明后台、resume 和 isolation；文案要求指定类型，但输入 serde 仍存在 general-purpose 默认。

#### Scenario: 终止描述
- **WHEN** 选择 Windows 或 POSIX 且配置 bash/monitor/subagent 可用性
- **THEN** 按配置输出 Job Object 或 SIGTERM/SIGKILL、Cancel+Shutdown 文案，不在本包执行这些动作。

#### Scenario: 输出描述
- **WHEN** bash 与 subagent 后台参数同名或 read 工具缺失
- **THEN** 合并同名来源表述，缺 read 时不添加读文件提示，等待上限保持占位符，monitor 提示使用单数 ID 参数名。

证据：`crates/common/tool-types/src/task.rs` — `build_task_description`；`crates/common/tool-types/src/task.rs` — `build_kill_task_description`；`crates/common/tool-types/src/task.rs` — `build_task_output_description`。



### Requirement: Shell per step tool definition visibility
prepare_tool_definitions_timed SHALL 在Blocking且MCP未初始化时等待，Progressive不等待；返回的mcp_wait_ms只覆盖等待阶段，不包含后续构造。构造时先处理子agent MCP eligibility变动，再取builtins-only定义（底层实际按client_name不含双下划线筛选）；无已提交compaction prompt索引时移除ContextRecall，有delegated GoalContext时移除GoalLifecycleUpdate。活动turn用捕获的turn_behavior，否则用当前behavior；Workflow同时要求捕获和当前均Workflow，PlanControl仅Plan可见，最后Plan按Workflow工具名再过滤。此清单过滤不替代dispatch授权。turn_base_tool_specs仅克隆转换，不在这里添加structured output工具。

#### Scenario: Workflow changes during a turn
- **WHEN** 捕获的turn为Workflow但当前已切换其他behavior
- **THEN** 后续工具定义不再显示Workflow launcher。

源码证据：
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `pub(super) async fn prepare_tool_definitions_timed`。
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `pub(in crate::session::actor) async fn prepare_tool_definitions_inner`。
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `pub(crate) fn turn_base_tool_specs`。
- `crates/codegen/shell/src/session/actor/session_mode.rs` — `pub(super) fn filter_cursor_tools_by_plan_mode`。
- `crates/codegen/tools/src/registry/types.rs` — `pub fn tool_definitions_builtins_only`。

- `crates/codegen/shell/src/session/actor/tests/rewind_cross_compaction_tests.rs` — `async fn context_recall_visibility_follows_completed_compaction_on_selected_branch`。
### Requirement: Tools crates/codegen/tools/build.rs bundled search binary build pipeline contract
build.rs SHALL use the pinned helper-binary versions/checksums and bundle the declared fd/rg/ugrep/bfs assets into the generated output; checksum/decompression/path failures remain build errors according to the explicit Result paths.

#### Scenario: Pinned helper asset
- **WHEN** a bundled search binary is prepared
- **THEN** the declared release/version and SHA-256 data identify the asset before it is embedded.

#### Scenario: Archive failure
- **WHEN** download, checksum, or decompression validation fails
- **THEN** the build script returns an error rather than publishing an unverified binary.

证据：`crates/codegen/tools/build.rs` — `RG_VER`；`crates/codegen/tools/build.rs` — `BFS_VER`；`crates/codegen/tools/build.rs` — `UGREP_VER`；`crates/codegen/tools/build.rs` — `FD_VER`；`crates/codegen/tools/build.rs` — `FD_VER_MACOS_X64`；`crates/codegen/tools/build.rs` — `FD_TARBALL_SHA256`；`crates/codegen/tools/build.rs` — `main`；`crates/codegen/tools/build.rs` — `bundle_fd`；`crates/codegen/tools/build.rs` — `hex_encode`；`crates/codegen/tools/build.rs` — `release`；`crates/codegen/tools/build.rs` — `bundle_search_tool`；`crates/codegen/tools/build.rs` — `bundle_rg`。

### Requirement: Tools bridge descriptor, authorization, rendering, and baseline contract
ToolBridge SHALL register and resolve tool descriptors by ToolKind/client name, apply authored and native access ceilings, preflight batch requests, expose builtins-only/native descriptor projections, and render tool prompts/metadata through the shared bridge context. Batch preflight SHALL return per-tool admission/error information without bypassing downstream dispatch; skill-baseline updates remain a separate asynchronous bridge operation.

#### Scenario: Unknown tool kind
- **WHEN** a caller resolves a tool name or kind absent from the registered descriptor set
- **THEN** the bridge returns its explicit missing/invalid result and does not synthesize an executable tool.

#### Scenario: Builtins-only projection
- **WHEN** a caller requests the builtins-only descriptor set
- **THEN** only descriptors passing the bridge’s builtins/client-name filter are returned; this list is not itself authorization.

#### Scenario: Batch admission
- **WHEN** a batch contains a tool that fails preflight
- **THEN** the bridge reports the per-item failure while preserving the explicit batch result contract.

证据：`crates/codegen/tools/src/bridge.rs` — `ToolBridgeResult`；`crates/codegen/tools/src/bridge.rs` — `from`；`crates/codegen/tools/src/bridge.rs` — `ToolBridge`；`crates/codegen/tools/src/bridge.rs` — `get_builder`；`crates/codegen/tools/src/bridge.rs` — `finalize_builder`；`crates/codegen/tools/src/bridge.rs` — `tool_definitions`；`crates/codegen/tools/src/bridge.rs` — `tool_for_kind`；`crates/codegen/tools/src/bridge.rs` — `tool_kind`；`crates/codegen/tools/src/bridge.rs` — `isolates_batch_preflight`；`crates/codegen/tools/src/bridge.rs` — `max_access`；`crates/codegen/tools/src/bridge.rs` — `native_tool_descriptors`；`crates/codegen/tools/src/bridge.rs` — `authored_native_tool_names`；`crates/codegen/tools/src/bridge.rs` — `tool_definitions_builtins_only`；`crates/codegen/tools/src/bridge.rs` — `render_prompt`；`crates/codegen/tools/src/bridge.rs` — `template_renderer_snapshot`；`crates/codegen/tools/src/bridge.rs` — `register_mcp_tools`；`crates/codegen/tools/src/bridge.rs` — `unregister_tools_by_prefix`；`crates/codegen/tools/src/bridge.rs` — `unregister_tool_by_name`；`crates/codegen/tools/src/bridge.rs` — `toolset`；`crates/codegen/tools/src/bridge.rs` — `call`；`crates/codegen/tools/src/bridge.rs` — `try_parse`；`crates/codegen/tools/src/bridge.rs` — `seed_agents_md`；`crates/codegen/tools/src/bridge.rs` — `restore_announced_skill_names`；`crates/codegen/tools/src/bridge.rs` — `get_announced_skill_names`；`crates/codegen/tools/src/bridge.rs` — `skill_listing_snapshot`；`crates/codegen/tools/src/bridge.rs` — `seed_skill_discovery`；`crates/codegen/tools/src/bridge.rs` — `set_context_window_tokens`；`crates/codegen/tools/src/bridge.rs` — `seed_gitignore_filter`。

### Requirement: Tools crates/codegen/tools/src/computer/local/cgroup.rs local computer, shell, filesystem, and terminal runtime contract
crates/codegen/tools/src/computer/local/cgroup.rs SHALL implement the local computer, shell, filesystem, and terminal runtime boundary through resolve local shell/filesystem/terminal capabilities, apply process/resource limits, and preserve cleanup state. Its source symbols PROCESS_OOM_EXIT_CODE, MemoryHighEvent, CgroupMemoryConfig, memory_max, inotify_init1, inotify_add_watch, libc, IN_MODIFY, Inotify, new, add_watch, wait_and_drain, read_self_cgroup, parse_memory_events_high, CgroupHandle, create, add_process, memory_current (additional symbols omitted from the title but included in source evidence) follow explicit markers filesystem or durable persistence、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、platform/feature conditional; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/computer/local/cgroup.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/computer/local/cgroup.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/computer/local/cgroup.rs` — `PROCESS_OOM_EXIT_CODE`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `MemoryHighEvent`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `CgroupMemoryConfig`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `memory_max`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `inotify_init1`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `inotify_add_watch`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `libc`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `IN_MODIFY`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `Inotify`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `new`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `add_watch`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `wait_and_drain`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `read_self_cgroup`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `parse_memory_events_high`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `CgroupHandle`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `create`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `add_process`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `memory_current`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `path`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `drop`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `MemoryHighMonitor`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `start`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `try_recv`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `monitor_loop`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `read_high_counter`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `CgroupGuard`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `try_create`；`crates/codegen/tools/src/computer/local/cgroup.rs` — `noop`。

### Requirement: Tools crates/codegen/tools/src/computer/local/embedded_search_tools.rs local computer, shell, filesystem, and terminal runtime contract
crates/codegen/tools/src/computer/local/embedded_search_tools.rs SHALL implement the local computer, shell, filesystem, and terminal runtime boundary through resolve local shell/filesystem/terminal capabilities, apply process/resource limits, and preserve cleanup state. Its source symbols UGREP_DEFAULT_ARGS, BFS_BYTES, UGREP_BYTES, search_injection, build_injection, restore_command, ResolvedTools, resolved_tools, TOOLS, extract_bundled, bundled_bfs, bundled_ugrep, resolve_tool, resolve_tool_from, bash_safe_quote, shell_function, both_tools, shell_function_shape (additional symbols omitted from the title but included in source evidence) follow explicit markers filesystem or durable persistence、explicit error classification、child process execution、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/computer/local/embedded_search_tools.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/computer/local/embedded_search_tools.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `UGREP_DEFAULT_ARGS`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `BFS_BYTES`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `UGREP_BYTES`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `search_injection`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `build_injection`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `restore_command`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `ResolvedTools`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `resolved_tools`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `TOOLS`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `extract_bundled`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `bundled_bfs`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `bundled_ugrep`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `resolve_tool`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `resolve_tool_from`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `bash_safe_quote`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `shell_function`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `both_tools`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `shell_function_shape`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `shell_function_unresolved_uses_empty_hint`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `grep_prepends_ugrep_defaults`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `bash_safe_quote_escapes_metacharacters`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `restore_command_is_marker_gated`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `config_default_is_both_on`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `build_injection_off_emits_marker_gated_restore_not_function`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `build_injection_on_shadows_resolved_tools`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `build_injection_enabled_unresolved_still_self_heals`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `injection_always_nonempty_and_trailing_sep`；`crates/codegen/tools/src/computer/local/embedded_search_tools.rs` — `env_override_accepts_regular_file_without_exec_bit`。

### Requirement: Tools crates/codegen/tools/src/computer/local/file_system.rs local computer, shell, filesystem, and terminal runtime contract
crates/codegen/tools/src/computer/local/file_system.rs SHALL implement the local computer, shell, filesystem, and terminal runtime boundary through resolve local shell/filesystem/terminal capabilities, apply process/resource limits, and preserve cleanup state. Its source symbols LocalFs, WRITE_RETRY_DELAYS, WINDOWS_ERROR_SHARING_VIOLATION, WINDOWS_ERROR_LOCK_VIOLATION, is_permission_error, is_windows_transient_write_lock_raw_os_error, is_transient_write_lock_error, is_test_transient_write_lock_error, write_file_with_transient_lock_retries, write_file_with_retry_hooks, read_file, write_file, delete_file, classifies_windows_transient_write_lock_errors, classifies_windows_transient_write_lock_io_errors, non_windows_does_not_retry_windows_raw_error_numbers, first_attempt_success_does_not_fire_retry_callbacks, transient_lock_errors_are_retried_until_success (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、timeout, budget, or rate limit、platform/feature conditional、sandbox, trust, or allow/deny policy、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/computer/local/file_system.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

#### Scenario: Policy boundary
- **WHEN** input crosses an allow/deny or sandbox check
- **THEN** the explicit policy branch controls admission before execution.

证据：`crates/codegen/tools/src/computer/local/file_system.rs` — `LocalFs`；`crates/codegen/tools/src/computer/local/file_system.rs` — `WRITE_RETRY_DELAYS`；`crates/codegen/tools/src/computer/local/file_system.rs` — `WINDOWS_ERROR_SHARING_VIOLATION`；`crates/codegen/tools/src/computer/local/file_system.rs` — `WINDOWS_ERROR_LOCK_VIOLATION`；`crates/codegen/tools/src/computer/local/file_system.rs` — `is_permission_error`；`crates/codegen/tools/src/computer/local/file_system.rs` — `is_windows_transient_write_lock_raw_os_error`；`crates/codegen/tools/src/computer/local/file_system.rs` — `is_transient_write_lock_error`；`crates/codegen/tools/src/computer/local/file_system.rs` — `is_test_transient_write_lock_error`；`crates/codegen/tools/src/computer/local/file_system.rs` — `write_file_with_transient_lock_retries`；`crates/codegen/tools/src/computer/local/file_system.rs` — `write_file_with_retry_hooks`；`crates/codegen/tools/src/computer/local/file_system.rs` — `read_file`；`crates/codegen/tools/src/computer/local/file_system.rs` — `write_file`；`crates/codegen/tools/src/computer/local/file_system.rs` — `delete_file`；`crates/codegen/tools/src/computer/local/file_system.rs` — `classifies_windows_transient_write_lock_errors`；`crates/codegen/tools/src/computer/local/file_system.rs` — `classifies_windows_transient_write_lock_io_errors`；`crates/codegen/tools/src/computer/local/file_system.rs` — `non_windows_does_not_retry_windows_raw_error_numbers`；`crates/codegen/tools/src/computer/local/file_system.rs` — `first_attempt_success_does_not_fire_retry_callbacks`；`crates/codegen/tools/src/computer/local/file_system.rs` — `transient_lock_errors_are_retried_until_success`；`crates/codegen/tools/src/computer/local/file_system.rs` — `non_transient_errors_are_not_retried`；`crates/codegen/tools/src/computer/local/file_system.rs` — `persistent_transient_lock_exhausts_retry_budget`。

### Requirement: Tools crates/codegen/tools/src/computer/local/mock_fs.rs local computer, shell, filesystem, and terminal runtime contract
crates/codegen/tools/src/computer/local/mock_fs.rs SHALL implement the local computer, shell, filesystem, and terminal runtime boundary through resolve local shell/filesystem/terminal capabilities, apply process/resource limits, and preserve cleanup state. Its source symbols MockFs, default, new, set_file, get_file, exists, list_files, read_file, write_file, delete_file, test_mock_fs_read_write, test_mock_fs_delete, test_mock_fs_set_file follow explicit markers explicit error classification、platform/feature conditional; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/computer/local/mock_fs.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/computer/local/mock_fs.rs` — `MockFs`；`crates/codegen/tools/src/computer/local/mock_fs.rs` — `default`；`crates/codegen/tools/src/computer/local/mock_fs.rs` — `new`；`crates/codegen/tools/src/computer/local/mock_fs.rs` — `set_file`；`crates/codegen/tools/src/computer/local/mock_fs.rs` — `get_file`；`crates/codegen/tools/src/computer/local/mock_fs.rs` — `exists`；`crates/codegen/tools/src/computer/local/mock_fs.rs` — `list_files`；`crates/codegen/tools/src/computer/local/mock_fs.rs` — `read_file`；`crates/codegen/tools/src/computer/local/mock_fs.rs` — `write_file`；`crates/codegen/tools/src/computer/local/mock_fs.rs` — `delete_file`；`crates/codegen/tools/src/computer/local/mock_fs.rs` — `test_mock_fs_read_write`；`crates/codegen/tools/src/computer/local/mock_fs.rs` — `test_mock_fs_delete`；`crates/codegen/tools/src/computer/local/mock_fs.rs` — `test_mock_fs_set_file`。

### Requirement: Tools crates/codegen/tools/src/computer/local/mod.rs local computer, shell, filesystem, and terminal runtime contract
crates/codegen/tools/src/computer/local/mod.rs SHALL implement the local computer, shell, filesystem, and terminal runtime boundary through resolve local shell/filesystem/terminal capabilities, apply process/resource limits, and preserve cleanup state. Its source symbols SearchShadowConfig, default follow explicit markers platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/computer/local/mod.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/computer/local/mod.rs` — `SearchShadowConfig`；`crates/codegen/tools/src/computer/local/mod.rs` — `default`。

### Requirement: Tools crates/codegen/tools/src/computer/local/shell_state.rs local computer, shell, filesystem, and terminal runtime contract
crates/codegen/tools/src/computer/local/shell_state.rs SHALL implement the local computer, shell, filesystem, and terminal runtime boundary through resolve local shell/filesystem/terminal capabilities, apply process/resource limits, and preserve cleanup state. Its source symbols BASH_STATE_START_MARKER, BASH_STATE_END_MARKER, ZSH_STATE_START_MARKER, ZSH_STATE_END_MARKER, INIT_STATE_MARKER, INIT_TIMEOUT, DUMP_READ_TIMEOUT, shell_env_overrides, sudo_alias_injection, strings, DUMP_BASH_STATE_SCRIPT, DUMP_ZSH_STATE_SCRIPT, ShellKind, detect, str, binary_path, rc_file_name, dump_script (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、async task and cancellation lifecycle、child process execution、timeout, budget, or rate limit、platform/feature conditional; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/computer/local/shell_state.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/computer/local/shell_state.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/computer/local/shell_state.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/computer/local/shell_state.rs` — `BASH_STATE_START_MARKER`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `BASH_STATE_END_MARKER`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `ZSH_STATE_START_MARKER`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `ZSH_STATE_END_MARKER`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `INIT_STATE_MARKER`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `INIT_TIMEOUT`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `DUMP_READ_TIMEOUT`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `shell_env_overrides`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `sudo_alias_injection`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `strings`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `DUMP_BASH_STATE_SCRIPT`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `DUMP_ZSH_STATE_SCRIPT`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `ShellKind`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `detect`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `str`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `binary_path`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `rc_file_name`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `dump_script`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `dump_function_name`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `start_marker`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `end_marker`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `ShellState`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `init`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `prepare_command`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `update_from_dump`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `PreparedCommand`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `os_pipe`；`crates/codegen/tools/src/computer/local/shell_state.rs` — `set_cloexec`。

### Requirement: Tools crates/codegen/tools/src/computer/local/static_shell.rs local computer, shell, filesystem, and terminal runtime contract
crates/codegen/tools/src/computer/local/static_shell.rs SHALL implement the local computer, shell, filesystem, and terminal runtime boundary through resolve local shell/filesystem/terminal capabilities, apply process/resource limits, and preserve cleanup state. Its source symbols INIT_MARKER, INIT_TIMEOUT, StaticShellSnapshot, shell_binary, str, rc_file_name, sudo_alias_injection, init, shell, prepare_command, PreparedStaticCommand, write_snapshot_to_pipe, os_pipe, set_cloexec, bash_available, run_static, replays_aliases_and_functions, user_command_exit_code_propagates_past_bad_snapshot (additional symbols omitted from the title but included in source evidence) follow explicit markers filesystem or durable persistence、explicit error classification、async task and cancellation lifecycle、child process execution、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/computer/local/static_shell.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/computer/local/static_shell.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/computer/local/static_shell.rs` — `INIT_MARKER`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `INIT_TIMEOUT`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `StaticShellSnapshot`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `shell_binary`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `str`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `rc_file_name`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `sudo_alias_injection`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `init`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `shell`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `prepare_command`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `PreparedStaticCommand`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `write_snapshot_to_pipe`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `os_pipe`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `set_cloexec`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `bash_available`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `run_static`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `replays_aliases_and_functions`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `user_command_exit_code_propagates_past_bad_snapshot`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `empty_snapshot_runs_plain`；`crates/codegen/tools/src/computer/local/static_shell.rs` — `sourced_script_does_not_inherit_wrapper_positional_args`。

### Requirement: Tools crates/codegen/tools/src/computer/local/terminal.rs local computer, shell, filesystem, and terminal runtime contract
crates/codegen/tools/src/computer/local/terminal.rs SHALL implement the local computer, shell, filesystem, and terminal runtime boundary through resolve local shell/filesystem/terminal capabilities, apply process/resource limits, and preserve cleanup state. Its source symbols SpawnResult, READ_BUFFER_SIZE, DEFAULT_NOTIFICATION_INTERVAL_MS, COMMAND_CHANNEL_SIZE, COMPLETED_TASK_TTL, SIGTERM_GRACE, BACKGROUND_MAX_RUNTIME, FOREGROUND_BLOCK_BUDGET, foreground_block_budget_from_env, MAX_OUTPUT_FILE_BYTES, output_file_cap_from_env, DRAIN_TIMEOUT, MAX_RETAINED_OUTPUT_FILE_BYTES, MAX_COMPLETED_TASK_SNAPSHOTS, notification_interval, ExitStatus, TerminalCommand, BackgroundStatus (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/computer/local/terminal.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/computer/local/terminal.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/computer/local/terminal.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/computer/local/terminal.rs` — `SpawnResult`；`crates/codegen/tools/src/computer/local/terminal.rs` — `READ_BUFFER_SIZE`；`crates/codegen/tools/src/computer/local/terminal.rs` — `DEFAULT_NOTIFICATION_INTERVAL_MS`；`crates/codegen/tools/src/computer/local/terminal.rs` — `COMMAND_CHANNEL_SIZE`；`crates/codegen/tools/src/computer/local/terminal.rs` — `COMPLETED_TASK_TTL`；`crates/codegen/tools/src/computer/local/terminal.rs` — `SIGTERM_GRACE`；`crates/codegen/tools/src/computer/local/terminal.rs` — `BACKGROUND_MAX_RUNTIME`；`crates/codegen/tools/src/computer/local/terminal.rs` — `FOREGROUND_BLOCK_BUDGET`；`crates/codegen/tools/src/computer/local/terminal.rs` — `foreground_block_budget_from_env`；`crates/codegen/tools/src/computer/local/terminal.rs` — `MAX_OUTPUT_FILE_BYTES`；`crates/codegen/tools/src/computer/local/terminal.rs` — `output_file_cap_from_env`；`crates/codegen/tools/src/computer/local/terminal.rs` — `DRAIN_TIMEOUT`；`crates/codegen/tools/src/computer/local/terminal.rs` — `MAX_RETAINED_OUTPUT_FILE_BYTES`；`crates/codegen/tools/src/computer/local/terminal.rs` — `MAX_COMPLETED_TASK_SNAPSHOTS`；`crates/codegen/tools/src/computer/local/terminal.rs` — `notification_interval`；`crates/codegen/tools/src/computer/local/terminal.rs` — `ExitStatus`；`crates/codegen/tools/src/computer/local/terminal.rs` — `TerminalCommand`；`crates/codegen/tools/src/computer/local/terminal.rs` — `BackgroundStatus`；`crates/codegen/tools/src/computer/local/terminal.rs` — `BackgroundReason`；`crates/codegen/tools/src/computer/local/terminal.rs` — `is_backgrounded`；`crates/codegen/tools/src/computer/local/terminal.rs` — `as_signal`；`crates/codegen/tools/src/computer/local/terminal.rs` — `str`；`crates/codegen/tools/src/computer/local/terminal.rs` — `ProcessState`；`crates/codegen/tools/src/computer/local/terminal.rs` — `to_result`；`crates/codegen/tools/src/computer/local/terminal.rs` — `notify_waiters`；`crates/codegen/tools/src/computer/local/terminal.rs` — `maybe_truncate`；`crates/codegen/tools/src/computer/local/terminal.rs` — `flush_and_truncate_output_file`；`crates/codegen/tools/src/computer/local/terminal.rs` — `is_timed_out`。

### Requirement: Tools crates/codegen/tools/src/computer/mod.rs tools crate module boundary contract
crates/codegen/tools/src/computer/mod.rs SHALL implement the tools crate module boundary boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols mod follow explicit markers typed inputs and outputs; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/computer/mod.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/computer/mod.rs` — `mod`。

### Requirement: Tools crates/codegen/tools/src/computer/types.rs tools crate module boundary contract
crates/codegen/tools/src/computer/types.rs SHALL implement the tools crate module boundary boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols ComputerError, io, io_with_kind, io_error_kind, from, AsyncFileSystem, read_file, write_file, delete_file, TerminalRunRequest, TaskKind, TerminalRunResult, BackgroundHandle, TaskSnapshot, duration_secs, is_outstanding, is_outstanding_background, KillOutcome (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、child process execution、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/computer/types.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/computer/types.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/computer/types.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/computer/types.rs` — `ComputerError`；`crates/codegen/tools/src/computer/types.rs` — `io`；`crates/codegen/tools/src/computer/types.rs` — `io_with_kind`；`crates/codegen/tools/src/computer/types.rs` — `io_error_kind`；`crates/codegen/tools/src/computer/types.rs` — `from`；`crates/codegen/tools/src/computer/types.rs` — `AsyncFileSystem`；`crates/codegen/tools/src/computer/types.rs` — `read_file`；`crates/codegen/tools/src/computer/types.rs` — `write_file`；`crates/codegen/tools/src/computer/types.rs` — `delete_file`；`crates/codegen/tools/src/computer/types.rs` — `TerminalRunRequest`；`crates/codegen/tools/src/computer/types.rs` — `TaskKind`；`crates/codegen/tools/src/computer/types.rs` — `TerminalRunResult`；`crates/codegen/tools/src/computer/types.rs` — `BackgroundHandle`；`crates/codegen/tools/src/computer/types.rs` — `TaskSnapshot`；`crates/codegen/tools/src/computer/types.rs` — `duration_secs`；`crates/codegen/tools/src/computer/types.rs` — `is_outstanding`；`crates/codegen/tools/src/computer/types.rs` — `is_outstanding_background`；`crates/codegen/tools/src/computer/types.rs` — `KillOutcome`；`crates/codegen/tools/src/computer/types.rs` — `TerminalBackend`；`crates/codegen/tools/src/computer/types.rs` — `run`；`crates/codegen/tools/src/computer/types.rs` — `run_background`；`crates/codegen/tools/src/computer/types.rs` — `get_task`；`crates/codegen/tools/src/computer/types.rs` — `kill_task`；`crates/codegen/tools/src/computer/types.rs` — `kill_foreground_commands`；`crates/codegen/tools/src/computer/types.rs` — `kill_foreground_commands_by_owner`；`crates/codegen/tools/src/computer/types.rs` — `kill_all_background_tasks`；`crates/codegen/tools/src/computer/types.rs` — `kill_all_background_tasks_by_owner`；`crates/codegen/tools/src/computer/types.rs` — `warm_shell`。

### Requirement: Tools crates/codegen/tools/src/gitignore.rs tools crate module boundary contract
crates/codegen/tools/src/gitignore.rs SHALL implement the tools crate module boundary boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols is_ignored, build_gitignore, is_ignored_matches_gitignored_paths, is_ignored_does_not_match_non_gitignored_paths, is_ignored_strips_git_root_prefix, is_ignored_returns_false_for_path_outside_git_root, regression_no_panic_on_absolute_path_without_git_root follow explicit markers explicit error classification、platform/feature conditional、session, prompt, goal, or subagent context、repository/worktree scope; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/gitignore.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/gitignore.rs` — `is_ignored`；`crates/codegen/tools/src/gitignore.rs` — `build_gitignore`；`crates/codegen/tools/src/gitignore.rs` — `is_ignored_matches_gitignored_paths`；`crates/codegen/tools/src/gitignore.rs` — `is_ignored_does_not_match_non_gitignored_paths`；`crates/codegen/tools/src/gitignore.rs` — `is_ignored_strips_git_root_prefix`；`crates/codegen/tools/src/gitignore.rs` — `is_ignored_returns_false_for_path_outside_git_root`；`crates/codegen/tools/src/gitignore.rs` — `regression_no_panic_on_absolute_path_without_git_root`。

### Requirement: Tools crates/codegen/tools/src/implementations/context_recall.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/context_recall.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols CONTEXT_RECALL_TOOL_NAME, MAX_CONTEXT_RECALL_QUERY_CHARS, ContextRecallInput, ContextRecallOutput, validate, ContextRecallBackend, recall, ContextRecallImpl, kind, tool_namespace, description_template, Args, Output, id, description, capabilities, run, recall_query_must_be_specific_and_bounded (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、async task and cancellation lifecycle、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/context_recall.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/context_recall.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Policy boundary
- **WHEN** input crosses an allow/deny or sandbox check
- **THEN** the explicit policy branch controls admission before execution.

证据：`crates/codegen/tools/src/implementations/context_recall.rs` — `CONTEXT_RECALL_TOOL_NAME`；`crates/codegen/tools/src/implementations/context_recall.rs` — `MAX_CONTEXT_RECALL_QUERY_CHARS`；`crates/codegen/tools/src/implementations/context_recall.rs` — `ContextRecallInput`；`crates/codegen/tools/src/implementations/context_recall.rs` — `ContextRecallOutput`；`crates/codegen/tools/src/implementations/context_recall.rs` — `validate`；`crates/codegen/tools/src/implementations/context_recall.rs` — `ContextRecallBackend`；`crates/codegen/tools/src/implementations/context_recall.rs` — `recall`；`crates/codegen/tools/src/implementations/context_recall.rs` — `ContextRecallImpl`；`crates/codegen/tools/src/implementations/context_recall.rs` — `kind`；`crates/codegen/tools/src/implementations/context_recall.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/context_recall.rs` — `description_template`；`crates/codegen/tools/src/implementations/context_recall.rs` — `Args`；`crates/codegen/tools/src/implementations/context_recall.rs` — `Output`；`crates/codegen/tools/src/implementations/context_recall.rs` — `id`；`crates/codegen/tools/src/implementations/context_recall.rs` — `description`；`crates/codegen/tools/src/implementations/context_recall.rs` — `capabilities`；`crates/codegen/tools/src/implementations/context_recall.rs` — `run`；`crates/codegen/tools/src/implementations/context_recall.rs` — `recall_query_must_be_specific_and_bounded`；`crates/codegen/tools/src/implementations/context_recall.rs` — `description_scopes_recall_to_missing_compacted_context`。

### Requirement: Tools crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols FileOperationLockManager, LockInner, QueuedWaiter, new, wait_for_lock, wait_for_exclusive_lock, default, LockKind, FileOperationLockGuard, drop, has_exclusive_waiter_ahead, process_queue, serializes_same_path, allows_different_paths_concurrently, exclusive_blocks_file_locks follow explicit markers serde/json wire or configuration、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `FileOperationLockManager`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `LockInner`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `QueuedWaiter`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `new`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `wait_for_lock`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `wait_for_exclusive_lock`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `default`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `LockKind`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `FileOperationLockGuard`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `drop`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `has_exclusive_waiter_ahead`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `process_queue`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `serializes_same_path`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `allows_different_paths_concurrently`；`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs` — `exclusive_blocks_file_locks`。

### Requirement: Tools crates/codegen/tools/src/implementations/editor_infra/mod.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/editor_infra/mod.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols DEFAULT_BLOCK_UNTIL_MS follow explicit markers tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/editor_infra/mod.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/editor_infra/mod.rs` — `DEFAULT_BLOCK_UNTIL_MS`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/bash/mod.rs bash command tool contract
crates/codegen/tools/src/implementations/grow_build/bash/mod.rs SHALL implement the bash command tool boundary through validate command parameters, enforce requirement/access metadata, execute through the terminal path, and project bounded output/notifications. Its source symbols BashError, default_true, MAX_PROGRESS_DELTA_BYTES, BASH_CAPABILITIES, bash_output_chunk_progress, terminal_notification_base, append_running_tasks_notice, BashParams, level, default, ID, str, validate_params_value, schema_default_timeout_ms, BashToolInput, BashToolOutput, chat_completion_output, from (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、platform/feature conditional; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/bash/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/bash/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/bash/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `BashError`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `default_true`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `MAX_PROGRESS_DELTA_BYTES`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `BASH_CAPABILITIES`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `bash_output_chunk_progress`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `terminal_notification_base`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `append_running_tasks_notice`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `BashParams`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `level`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `default`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `ID`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `validate_params_value`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `schema_default_timeout_ms`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `BashToolInput`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `BashToolOutput`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `chat_completion_output`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `from`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `KillReason`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `Err`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `from_str`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `fmt`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `annotations`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `format_default_prompt`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `DEFAULT_MAX_TIMEOUT_MS`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `ABSOLUTE_MAX_TIMEOUT_MS`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `DEFAULT_FOREGROUND_BLOCK_BUDGET_MS`；`crates/codegen/tools/src/implementations/grow_build/bash/mod.rs` — `has_trailing_background_operator`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/coordination.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/coordination.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols LIST_ACTIVE_SESSIONS_TOOL_NAME, ASK_SESSION_TOOL_NAME, GET_INQUIRY_TOOL_NAME, CoordinationErrorCode, CoordinationError, new, retry, fmt, from, ActiveSession, CoordinationInquiryResult, CoordinationInquiryState, CoordinationBackend, list_active_sessions, ask_session, get_inquiry, CoordinationBackendResource, ListActiveSessionsInput (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/coordination.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/coordination.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Policy boundary
- **WHEN** input crosses an allow/deny or sandbox check
- **THEN** the explicit policy branch controls admission before execution.

证据：`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `LIST_ACTIVE_SESSIONS_TOOL_NAME`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `ASK_SESSION_TOOL_NAME`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `GET_INQUIRY_TOOL_NAME`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `CoordinationErrorCode`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `CoordinationError`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `new`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `retry`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `fmt`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `from`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `ActiveSession`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `CoordinationInquiryResult`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `CoordinationInquiryState`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `CoordinationBackend`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `list_active_sessions`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `ask_session`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `get_inquiry`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `CoordinationBackendResource`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `ListActiveSessionsInput`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `ListActiveSessionsOutput`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `ListActiveSessionsTool`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/coordination.rs` — `capabilities`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/deploy_app_stub.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/deploy_app_stub.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols AppBuilderDeployerConfig, is_enabled, DEPLOY_APP_TOOL_NAME follow explicit markers tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/grow_build/deploy_app_stub.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/grow_build/deploy_app_stub.rs` — `AppBuilderDeployerConfig`；`crates/codegen/tools/src/implementations/grow_build/deploy_app_stub.rs` — `is_enabled`；`crates/codegen/tools/src/implementations/grow_build/deploy_app_stub.rs` — `DEPLOY_APP_TOOL_NAME`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/grep/mod.rs bounded grep search tool contract
crates/codegen/tools/src/implementations/grow_build/grep/mod.rs SHALL implement the bounded grep search tool boundary through parse pattern/path/output options, apply timeout/output/line budgets, and project bounded search matches. Its source symbols OutputMode, GrepSearchInput, to, GrepParams, CONTENT_LINE_LIMIT, CONTENT_LINE_DEFAULT, FILE_COUNT_LIMIT, FILE_COUNT_DEFAULT, DEFAULT_MAX_CHARS_PER_LINE, MAX_STDOUT_BYTES, EXACT_FIT_PROBE_TIMEOUT, GREP_TIMEOUT_DEFAULT_SECS, GREP_TIMEOUT_WSL_SECS, grep_timeout_secs, grep_timeout, resolve_effective_head_limit, max_head_limit, GREP_CAPABILITIES (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、async task and cancellation lifecycle、child process execution、timeout, budget, or rate limit、platform/feature conditional; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/grep/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/grep/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/grep/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `OutputMode`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `GrepSearchInput`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `to`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `GrepParams`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `CONTENT_LINE_LIMIT`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `CONTENT_LINE_DEFAULT`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `FILE_COUNT_LIMIT`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `FILE_COUNT_DEFAULT`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `DEFAULT_MAX_CHARS_PER_LINE`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `MAX_STDOUT_BYTES`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `EXACT_FIT_PROBE_TIMEOUT`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `GREP_TIMEOUT_DEFAULT_SECS`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `GREP_TIMEOUT_WSL_SECS`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `grep_timeout_secs`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `grep_timeout`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `resolve_effective_head_limit`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `max_head_limit`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `GREP_CAPABILITIES`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `GrepTool`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/grep/mod.rs` — `execute`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols KillTaskTool, not_found_response, kind, tool_namespace, description_template, DESC, finalized_definition, requires_expr, kill_task_description, Args, Output, id, description, capabilities, run, MockTerminal, run_background, kill_task (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、platform/feature conditional; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `KillTaskTool`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `not_found_response`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `DESC`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `finalized_definition`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `kill_task_description`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `MockTerminal`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `run_background`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `kill_task`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `get_task`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `wait_for_completion`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `list_tasks`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `resources_with_terminal`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `tool_name_and_description`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `fallback`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `description_context_aware_for_tool_subsets`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `description_tracks_renamed_task_id`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `kill_verb_matches_platform`；`crates/codegen/tools/src/implementations/grow_build/kill_task/mod.rs` — `kill_task_killed`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols kill_terminal_command_requires_expr, KillTerminalCommandTool, kind, tool_namespace, description_template, requires_expr, Args, Output, id, description, capabilities, run, MockTerminal, run_background, kill_task, get_task, wait_for_completion, list_tasks (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `kill_terminal_command_requires_expr`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `KillTerminalCommandTool`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `MockTerminal`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `run_background`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `kill_task`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `get_task`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `wait_for_completion`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `list_tasks`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `resources_with_terminal`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `tool_name_and_description_are_subagent_free`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `description_template_tracks_renamed_task_id`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `delegates_kill_killed`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `delegates_kill_already_exited`；`crates/codegen/tools/src/implementations/grow_build/kill_task/terminal_command.rs` — `delegates_kill_not_found`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs directory listing tool contract
crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs SHALL implement the directory listing tool boundary through resolve directory scope, apply hidden/gitignore and result budgets, and format truncation notices. Its source symbols ListDirInput, ListDirParams, compute_display_path, ListDirTool, DEFAULT_MAX_OUTPUT_CHARS, TOP_K_EXTENSIONS, ROOT_TRUNCATION_NOTICE_TEMPLATE, ROOT_TRUNCATION_NOTICE_FALLBACK, root_truncation_notice, MAX_GLOBAL_ITEMS, MAX_SEED_ITEMS, _, DirAccum, add_ext, to_summary, ext_key_from_path, DirNode, new (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、child process execution、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `ListDirInput`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `ListDirParams`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `compute_display_path`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `ListDirTool`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `DEFAULT_MAX_OUTPUT_CHARS`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `TOP_K_EXTENSIONS`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `ROOT_TRUNCATION_NOTICE_TEMPLATE`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `ROOT_TRUNCATION_NOTICE_FALLBACK`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `root_truncation_notice`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `MAX_GLOBAL_ITEMS`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `MAX_SEED_ITEMS`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `_`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `DirAccum`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `add_ext`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `to_summary`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `ext_key_from_path`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `DirNode`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `new`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `add_item`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `sort_recursive`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `all_subitems_sorted`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `subitem_line`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `summary_str`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `summary_char_cost`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `render_expanded`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `render_subtree`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `list_dir_walk_builder`；`crates/codegen/tools/src/implementations/grow_build/list_dir/mod.rs` — `seed_depth1_children`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols impl, LspToolOutput, from, LspTool, kind, tool_namespace, description_template, info, emitted_notifications, str, Args, Output, id, description, capabilities, run follow explicit markers serde/json wire or configuration、explicit error classification、tool definition, schema, or registry projection、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `impl`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `LspToolOutput`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `from`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `LspTool`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `info`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `emitted_notifications`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/lsp/mod.rs` — `run`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/mod.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/mod.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols mod follow explicit markers tool definition, schema, or registry projection、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle、scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/grow_build/mod.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/grow_build/mod.rs` — `mod`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols PlanControlAction, PlanControlInput, validate, str, PlanControlTool, kind, tool_namespace, description_template, isolates_batch_preflight, Args, Output, id, description, capabilities, run, metadata_uses_plan_control_kind, input_requires_action, complete_requires_only_a_non_empty_report (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Policy boundary
- **WHEN** input crosses an allow/deny or sandbox check
- **THEN** the explicit policy branch controls admission before execution.

证据：`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `PlanControlAction`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `PlanControlInput`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `validate`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `PlanControlTool`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `isolates_batch_preflight`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `metadata_uses_plan_control_kind`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `input_requires_action`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `complete_requires_only_a_non_empty_report`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `each_action_rejects_fields_owned_by_another_action`；`crates/codegen/tools/src/implementations/grow_build/plan_control/mod.rs` — `complete_returns_the_final_report_verbatim_after_trimming`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/plan_control/types.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/plan_control/types.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols PlanApprovalExtRequest, PlanApprovalOutcome, PlanApprovalExtResponse, request_uses_acp_camel_case, response_rejects_unknown_outcome, response_preserves_all_wire_outcomes follow explicit markers serde/json wire or configuration、explicit error classification、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/plan_control/types.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/plan_control/types.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Policy boundary
- **WHEN** input crosses an allow/deny or sandbox check
- **THEN** the explicit policy branch controls admission before execution.

证据：`crates/codegen/tools/src/implementations/grow_build/plan_control/types.rs` — `PlanApprovalExtRequest`；`crates/codegen/tools/src/implementations/grow_build/plan_control/types.rs` — `PlanApprovalOutcome`；`crates/codegen/tools/src/implementations/grow_build/plan_control/types.rs` — `PlanApprovalExtResponse`；`crates/codegen/tools/src/implementations/grow_build/plan_control/types.rs` — `request_uses_acp_camel_case`；`crates/codegen/tools/src/implementations/grow_build/plan_control/types.rs` — `response_rejects_unknown_outcome`；`crates/codegen/tools/src/implementations/grow_build/plan_control/types.rs` — `response_preserves_all_wire_outcomes`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs file read and media extraction tool contract
crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs SHALL implement the file read and media extraction tool boundary through resolve offset/line/token budgets, read text or media, and select image/PDF/document extraction paths. Its source symbols MAX_NUM_TOKENS, MAX_LINES_READ, STREAM_DELTA_TARGET_BYTES, READ_FILE_CAPABILITIES, DESCRIPTION_FULL, schema_default_offset, ReadFileInput, resolve_read_start_line, stored_read_offset, is_skill_markdown, ExtractedContent, extract_file_content_lines, strip, run_read_file, ReadFileTool, kind, tool_namespace, description_template (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、repository/worktree scope; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `MAX_NUM_TOKENS`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `MAX_LINES_READ`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `STREAM_DELTA_TARGET_BYTES`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `READ_FILE_CAPABILITIES`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `DESCRIPTION_FULL`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `schema_default_offset`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `ReadFileInput`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `resolve_read_start_line`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `stored_read_offset`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `is_skill_markdown`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `ExtractedContent`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `extract_file_content_lines`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `strip`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `run_read_file`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `ReadFileTool`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `execute`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `read_with_streamability`；`crates/codegen/tools/src/implementations/grow_build/read_file/mod.rs` — `test_resources`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs search and replace editing tool contract
crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs SHALL implement the search and replace editing tool boundary through normalize matches, validate path/edit limits, serialize file operation access, and return nearest-match diagnostics. Its source symbols render_snippet, LineRange, compute_line_range, replace_using_positions, build_edit_details, NormalizedMatch, NormalizedMatchResult, find_normalized_match_positions, replace_normalized_matches, render_snippet_middle_of_file_contexts, render_snippet_start_of_file_contexts, render_snippet_end_of_file_contexts, render_snippet_multiline_insertion_middle, test_replace_using_positions, unwrap_matches, normalized_match_smart_quotes, normalized_match_em_dash, normalized_match_nbsp (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `render_snippet`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `LineRange`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `compute_line_range`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `replace_using_positions`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `build_edit_details`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `NormalizedMatch`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `NormalizedMatchResult`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `find_normalized_match_positions`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `replace_normalized_matches`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `render_snippet_middle_of_file_contexts`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `render_snippet_start_of_file_contexts`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `render_snippet_end_of_file_contexts`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `render_snippet_multiline_insertion_middle`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `test_replace_using_positions`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `unwrap_matches`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `normalized_match_smart_quotes`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `normalized_match_em_dash`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `normalized_match_nbsp`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `normalized_match_ellipsis`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `normalized_match_returns_no_match_for_unrelated`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `normalized_match_pure_ascii_still_works`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `normalized_match_multiple_occurrences`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `normalized_match_empty_pattern_returns_no_match`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `partial_expansion_dash_inside_em_dash_rejected`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `partial_expansion_dot_inside_ellipsis_rejected`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `partial_expansion_double_dot_inside_ellipsis_rejected`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `full_expansion_em_dash_accepted`；`crates/codegen/tools/src/implementations/grow_build/search_replace/helpers.rs` — `full_expansion_ellipsis_accepted`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs search and replace editing tool contract
crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs SHALL implement the search and replace editing tool boundary through normalize matches, validate path/edit limits, serialize file operation access, and return nearest-match diagnostics. Its source symbols CONTEXT_LINES, DESCRIPTION_FULL, SearchReplaceInput, default_true, SearchReplaceParams, default, SearchReplaceTool, run_search_replace, NAME_MAX, validate_path_length, handle_new_file_creation, build_nearest_match_hint, build_confusable_hint, MAX_LISTED_LINES, handle_replacement, kind, tool_namespace, description_template (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、channel, fanout, or acknowledgement flow、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `CONTEXT_LINES`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `DESCRIPTION_FULL`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `SearchReplaceInput`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `default_true`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `SearchReplaceParams`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `default`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `SearchReplaceTool`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `run_search_replace`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `NAME_MAX`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `validate_path_length`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `handle_new_file_creation`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `build_nearest_match_hint`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `build_confusable_hint`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `MAX_LISTED_LINES`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `handle_replacement`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `emitted_notifications`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/search_replace/mod.rs` — `test_resources`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/storage.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/storage.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols IMAGE_MAX_BYTES, VIDEO_MAX_BYTES, DEFAULT_MAX_BYTES, budget_for, SessionFileWriter, str, new, without, save, scan_dir_stats, save_creates_numbered_files, save_resumes_counter_from_existing_files, save_uses_ext_override, scan_dir_stats_empty_dir, scan_dir_stats_finds_max_and_bytes, scan_dir_stats_cleans_orphan_tmp, scan_dir_stats_missing_dir follow explicit markers filesystem or durable persistence、explicit error classification、platform/feature conditional、session, prompt, goal, or subagent context、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/storage.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/storage.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `IMAGE_MAX_BYTES`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `VIDEO_MAX_BYTES`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `DEFAULT_MAX_BYTES`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `budget_for`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `SessionFileWriter`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `new`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `without`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `save`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `scan_dir_stats`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `save_creates_numbered_files`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `save_resumes_counter_from_existing_files`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `save_uses_ext_override`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `scan_dir_stats_empty_dir`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `scan_dir_stats_finds_max_and_bytes`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `scan_dir_stats_cleans_orphan_tmp`；`crates/codegen/tools/src/implementations/grow_build/storage.rs` — `scan_dir_stats_missing_dir`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols DEFAULT_WAIT_TIMEOUT, max_wait_block, capped_wait_timeout, requested_wait_timeout, WaitHint, WaitSubject, noun, str, still_running_wait_hint, format_waited_duration, with_still_running_wait_hint, apply_running_wait_hint, background_bash_requires_exprs, task_output_requires_expr, TaskOutputTool, run_single_task, run_multi_tasks, is_terminal_status (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、platform/feature conditional; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `DEFAULT_WAIT_TIMEOUT`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `max_wait_block`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `capped_wait_timeout`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `requested_wait_timeout`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `WaitHint`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `WaitSubject`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `noun`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `still_running_wait_hint`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `format_waited_duration`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `with_still_running_wait_hint`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `apply_running_wait_hint`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `background_bash_requires_exprs`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `task_output_requires_expr`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `TaskOutputTool`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `run_single_task`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `run_multi_tasks`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `is_terminal_status`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `not_found_result`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `ResolveResult`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `resolve_tasks`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `AbortWaitsOnDrop`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `drop`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `WaitOutcome`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `hint`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `finalize_wait_outcome`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `wait_all_event_driven`；`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs` — `format_subagent_snapshot`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols terminal_command_output_requires_expr, GetTerminalCommandOutputTool, kind, tool_namespace, description_template, requires_expr, Args, Output, id, description, capabilities, run, tool_name_and_description_are_subagent_free, output_wait_is_observation_only, delegates_to_task_output_for_running_task, delegates_to_task_output_for_completed_task follow explicit markers explicit error classification、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `terminal_command_output_requires_expr`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `GetTerminalCommandOutputTool`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `tool_name_and_description_are_subagent_free`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `output_wait_is_observation_only`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `delegates_to_task_output_for_running_task`；`crates/codegen/tools/src/implementations/grow_build/task_output/terminal_command.rs` — `delegates_to_task_output_for_completed_task`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/todo/mod.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/todo/mod.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols TodoError, validate_no_duplicate_ids, apply_replace, apply_merge, summarize_todo_state, TodoId, TodoPriority, TodoStatus, tag, fn, TodoItem, TodoState, push, clear, update, todo_items, todo_items_with_ids, is_empty (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/todo/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/todo/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/todo/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `TodoError`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `validate_no_duplicate_ids`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `apply_replace`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `apply_merge`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `summarize_todo_state`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `TodoId`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `TodoPriority`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `TodoStatus`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `tag`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `fn`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `TodoItem`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `TodoState`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `push`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `clear`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `update`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `todo_items`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `todo_items_with_ids`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `is_empty`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `has_id`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `TodoUpdate`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `has_no_content`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `default_merge`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `TodoWriteInput`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `TodoWriteTool`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/todo/mod.rs` — `requires_expr`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols CREATE_GOAL_TOOL_NAME, GET_GOAL_TOOL_NAME, GetGoalInput, CreateGoalInput, GoalUpdateStatus, UpdateGoalInput, GoalView, GoalContextSnapshot, GoalContextSnapshotResource, GoalDelegationSnapshotResource, GoalMutationAuthority, GoalMutationAuthorityResource, GoalCommand, GoalRuntimeHandle, fmt, UpdateGoalOutput, CreateGoalOutput, runtime_sender (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、channel, fanout, or acknowledgement flow、child process execution、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `CREATE_GOAL_TOOL_NAME`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `GET_GOAL_TOOL_NAME`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `GetGoalInput`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `CreateGoalInput`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `GoalUpdateStatus`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `UpdateGoalInput`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `GoalView`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `GoalContextSnapshot`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `GoalContextSnapshotResource`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `GoalDelegationSnapshotResource`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `GoalMutationAuthority`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `GoalMutationAuthorityResource`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `GoalCommand`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `GoalRuntimeHandle`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `fmt`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `UpdateGoalOutput`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `CreateGoalOutput`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `runtime_sender`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `mutation_runtime`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `channel_error`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `goal_metadata`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `isolates_batch_preflight`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `command_output`；`crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs` — `str`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols WorkflowDefinitionId, new, fmt, WorkflowScope, as_str, str, WorkflowRunControl, WorkflowDraftSource, WorkflowToolInput, MAX_AGENT_BUDGET, DEFAULT_MAX_CONCURRENCY, MAX_CONCURRENCY, validate, action_label, validate_definition_id, validate_budget, WorkflowRequest, WorkflowAck (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、channel, fanout, or acknowledgement flow、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `WorkflowDefinitionId`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `new`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `fmt`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `WorkflowScope`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `as_str`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `WorkflowRunControl`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `WorkflowDraftSource`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `WorkflowToolInput`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `MAX_AGENT_BUDGET`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `DEFAULT_MAX_CONCURRENCY`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `MAX_CONCURRENCY`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `validate`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `action_label`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `validate_definition_id`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `validate_budget`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `WorkflowRequest`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `WorkflowAck`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `WorkflowEnvelope`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `WorkflowHandle`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `WorkflowDefinitionSummary`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `WorkflowDiagnostic`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `WorkflowToolOutput`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `message`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `WorkflowTool`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs` — `description_template`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/write/mod.rs Grow tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build/write/mod.rs SHALL implement the Grow tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols DESCRIPTION, WriteInput, WriteTool, WriteOutput, kind, tool_namespace, description_template, emitted_notifications, str, requires_expr, Args, Output, id, description, capabilities, run, test_resources, write_new_file_creates_with_correct_content (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/write/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build/write/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build/write/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `DESCRIPTION`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `WriteInput`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `WriteTool`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `WriteOutput`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `emitted_notifications`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `test_resources`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `write_new_file_creates_with_correct_content`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `overwrite_existing_file`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `creates_parent_directories`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `tool_metadata`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `serde_roundtrip`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `empty_content_write`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `overwrite_preserves_path_in_output`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `relative_path_resolution`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `missing_filesystem_resource`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `missing_cwd_resource`；`crates/codegen/tools/src/implementations/grow_build/write/mod.rs` — `notification_fields`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_concise/bash.rs concise Grow tool implementation contract
crates/codegen/tools/src/implementations/grow_build_concise/bash.rs SHALL implement the concise Grow tool implementation boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols annotations, format_concise_foreground_prompt, format_concise_background_prompt, BashConciseTool, kind, tool_namespace, description_template, emitted_notifications, str, requires_expr, Args, Output, id, description, capabilities, run, make_bash, concise_prompt_starts_with_exit_code (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build_concise/bash.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `annotations`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `format_concise_foreground_prompt`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `format_concise_background_prompt`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `BashConciseTool`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `emitted_notifications`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `make_bash`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `concise_prompt_starts_with_exit_code`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `concise_prompt_no_double_header`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `concise_timeout_annotations`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `concise_killed_reasons`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `concise_backgrounded`；`crates/codegen/tools/src/implementations/grow_build_concise/bash.rs` — `no_double_header_after_default_prebake`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_concise/mod.rs concise Grow tool implementation contract
crates/codegen/tools/src/implementations/grow_build_concise/mod.rs SHALL implement the concise Grow tool implementation boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols mod follow explicit markers tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/grow_build_concise/mod.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/grow_build_concise/mod.rs` — `mod`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs concise Grow tool implementation contract
crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs SHALL implement the concise Grow tool implementation boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols DESCRIPTION_CONCISE, ReadFileConciseTool, kind, tool_namespace, description_template, requires_expr, Args, Output, id, description, capabilities, run, test_resources, description_template_selects_concise, description_template_tracks_renamed_offset_limit, concise_mode_uses_concise_content follow explicit markers filesystem or durable persistence、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `DESCRIPTION_CONCISE`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `ReadFileConciseTool`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `test_resources`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `description_template_selects_concise`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `description_template_tracks_renamed_offset_limit`；`crates/codegen/tools/src/implementations/grow_build_concise/read_file.rs` — `concise_mode_uses_concise_content`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs concise Grow tool implementation contract
crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs SHALL implement the concise Grow tool implementation boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols DESCRIPTION_CONCISE, SearchReplaceConciseTool, kind, tool_namespace, description_template, emitted_notifications, str, requires_expr, Args, Output, id, description, capabilities, run, test_resources, make_input, concise_tool_uses_canonical_edit_path, concise_replace_all_output follow explicit markers filesystem or durable persistence、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `DESCRIPTION_CONCISE`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `SearchReplaceConciseTool`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `emitted_notifications`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `test_resources`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `make_input`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `concise_tool_uses_canonical_edit_path`；`crates/codegen/tools/src/implementations/grow_build_concise/search_replace.rs` — `concise_replace_all_output`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols split_lines, generate_for_content, validate_against_content, find_shifted_in_content, split_lines_basic, split_lines_trailing_newline, split_lines_empty, split_lines_single_newline, generate_for_content_roundtrip, validate_against_content_valid, validate_against_content_stale, find_shifted_in_content_found follow explicit markers platform/feature conditional、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs` — `split_lines`；`crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs` — `generate_for_content`；`crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs` — `validate_against_content`；`crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs` — `find_shifted_in_content`；`crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs` — `split_lines_basic`；`crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs` — `split_lines_trailing_newline`；`crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs` — `split_lines_empty`；`crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs` — `split_lines_single_newline`；`crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs` — `generate_for_content_roundtrip`；`crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs` — `validate_against_content_valid`；`crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs` — `validate_against_content_stale`；`crates/codegen/tools/src/implementations/grow_build_hashline/anchor.rs` — `find_shifted_in_content_found`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols main, BenchmarkConfig, default, SchemeMetrics, new, stale_precision, stale_recall, collision_rate, avg_validation_us, avg_read_amp_lines, trace_survival_rate, BenchmarkReport, fmt, run_benchmark, build_scheme_configs, standard_mutations, str, estimate_read_amp_lines (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `main`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `BenchmarkConfig`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `default`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `SchemeMetrics`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `new`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `stale_precision`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `stale_recall`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `collision_rate`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `avg_validation_us`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `avg_read_amp_lines`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `trace_survival_rate`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `BenchmarkReport`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `fmt`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `run_benchmark`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `build_scheme_configs`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `standard_mutations`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `estimate_read_amp_lines`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `run_phase1_for_file`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `TraceStep`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `standard_traces`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `run_phase2_for_file`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `SMALL_FILE`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `MEDIUM_FILE`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `process`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `format_counts`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `REPETITIVE_FILE`；`crates/codegen/tools/src/implementations/grow_build_hashline/benchmark.rs` — `Config`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_hashline/config.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build_hashline/config.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols ExampleAnchors, HashlineSchemeParams, default_scheme_name, default_hash_len, default_chunk_size, ID, str, validate_params_value, default, validate, example_anchors, main, render_description, build_tool_definition, build_scheme, default_builds_chunk_scheme, content_only_builds, custom_chunk_params (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build_hashline/config.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build_hashline/config.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Policy boundary
- **WHEN** input crosses an allow/deny or sandbox check
- **THEN** the explicit policy branch controls admission before execution.

证据：`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `ExampleAnchors`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `HashlineSchemeParams`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `default_scheme_name`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `default_hash_len`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `default_chunk_size`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `ID`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `validate_params_value`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `default`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `validate`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `example_anchors`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `main`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `render_description`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `build_tool_definition`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `build_scheme`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `default_builds_chunk_scheme`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `content_only_builds`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `custom_chunk_params`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `unknown_scheme_rejected`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `hash_len_zero_rejected`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `hash_len_five_rejected`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `chunk_size_zero_rejected`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `content_only_ignores_chunk_size`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `deserializes_from_json`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `render_description_chunk_3`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `render_description_content_only_2`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `render_grep_examples_chunk`；`crates/codegen/tools/src/implementations/grow_build_hashline/config.rs` — `render_grep_examples_content_only`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols SNIPPET_CONTEXT, anchor_format_hint, str, anchor_suffix, detect_anchor_prefix_in_content, anchor_content_error, render_anchored_line, ResolvedOp, ApplyResult, EditRegionDetail, apply_edits, MAX_CONTIGUOUS_SNIPPET, build_snippet, resolve_op, recover_anchor_by_suffix, validate_anchor, check_overlaps, overlap_error (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `SNIPPET_CONTEXT`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `anchor_format_hint`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `str`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `anchor_suffix`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `detect_anchor_prefix_in_content`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `anchor_content_error`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `render_anchored_line`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `ResolvedOp`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `ApplyResult`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `EditRegionDetail`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `apply_edits`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `MAX_CONTIGUOUS_SNIPPET`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `build_snippet`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `resolve_op`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `recover_anchor_by_suffix`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `validate_anchor`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `check_overlaps`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `overlap_error`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `build_write_result`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `test_path`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `SAMPLE`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `main`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `test_scheme`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `anchors_for`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `point_replace`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `delete_via_empty_content`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `range_replace`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/apply.rs` — `insert_after_bof`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols DESCRIPTION, HashlineEditTool, file_not_found, to_search_replace, kind, tool_namespace, description_template, finalized_definition, requires_expr, Args, Output, id, description, capabilities, run, test_resources, test_resources_with_hints, description_template_tracks_renamed_edits (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `DESCRIPTION`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `HashlineEditTool`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `file_not_found`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `to_search_replace`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `finalized_definition`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `test_resources`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `test_resources_with_hints`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `description_template_tracks_renamed_edits`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `anchors_for`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `missing_file_with_hints_returns_enriched_message`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `disk_preserves_same_anchor_insert_order`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `disk_eof_append_no_extra_blank`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `output_is_tool_output_search_replace`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `test_scheme`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `multi_edit_details_are_per_edit_not_positional`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `single_edit_detail_has_correct_content`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `main`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/mod.rs` — `scattered_edits_details_total_size_bounded`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_hashline/edit/range_policy.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build_hashline/edit/range_policy.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols SMALL_MAX, MEDIUM_MAX, RangeSize, classify, range_warning, classification, small_range_no_warning, medium_range_warns, large_range_warns, empty_range_no_warning follow explicit markers platform/feature conditional、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/grow_build_hashline/edit/range_policy.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/grow_build_hashline/edit/range_policy.rs` — `SMALL_MAX`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/range_policy.rs` — `MEDIUM_MAX`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/range_policy.rs` — `RangeSize`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/range_policy.rs` — `classify`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/range_policy.rs` — `range_warning`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/range_policy.rs` — `classification`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/range_policy.rs` — `small_range_no_warning`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/range_policy.rs` — `medium_range_warns`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/range_policy.rs` — `large_range_warns`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/range_policy.rs` — `empty_range_no_warning`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols HashlineEditInput, deserialize_edits, HashlineOp, HashlineEditOutput, HashlineEditsApplied, HashlineEditError, HashlineEditErrorKind, deserialize_edits_from_array, deserialize_edits_from_double_encoded_string, deserialize_edits_rejects_non_array_string, deserialize_edits_from_double_encoded_empty_array, deserialize_edits_from_bare_object, deserialize_edits_rejects_number follow explicit markers serde/json wire or configuration、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs` — `HashlineEditInput`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs` — `deserialize_edits`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs` — `HashlineOp`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs` — `HashlineEditOutput`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs` — `HashlineEditsApplied`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs` — `HashlineEditError`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs` — `HashlineEditErrorKind`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs` — `deserialize_edits_from_array`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs` — `deserialize_edits_from_double_encoded_string`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs` — `deserialize_edits_rejects_non_array_string`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs` — `deserialize_edits_from_double_encoded_empty_array`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs` — `deserialize_edits_from_bare_object`；`crates/codegen/tools/src/implementations/grow_build_hashline/edit/types.rs` — `deserialize_edits_rejects_number`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols DEFAULT_ANCHOR_TIMEOUT_SECS, get_or_generate, inject_anchors, parse_rg_line, DESCRIPTION, HashlineGrepTool, kind, tool_namespace, description_template, finalized_definition, requires_expr, Args, Output, id, description, capabilities, run, test_scheme (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `DEFAULT_ANCHOR_TIMEOUT_SECS`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `get_or_generate`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `inject_anchors`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `parse_rg_line`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `DESCRIPTION`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `HashlineGrepTool`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `finalized_definition`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `test_scheme`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `parse_rg_line_match`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `parse_rg_line_context`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `parse_rg_line_no_digits`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `parse_rg_line_empty`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `tool_metadata`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `description_mentions_anchors_and_edit`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `inject_anchors_content_mode`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `main`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `inject_anchors_preserves_headers`；`crates/codegen/tools/src/implementations/grow_build_hashline/grep.rs` — `inject_anchors_context_lines`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_hashline/mod.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build_hashline/mod.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols and follow explicit markers tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/grow_build_hashline/mod.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/grow_build_hashline/mod.rs` — `and`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols Mutation, LineOutcome, MutationResult, apply_mutation, gen_insert_above, gen_delete, gen_token_edit, gen_reindent, gen_range_rewrite, gen_boilerplate_insert, sample_lines, main, insert_lines_above, insert_at_end, delete_lines, delete_past_end_clamped, edit_line, reindent_line (additional symbols omitted from the title but included in source evidence) follow explicit markers platform/feature conditional、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `Mutation`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `LineOutcome`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `MutationResult`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `apply_mutation`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `gen_insert_above`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `gen_delete`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `gen_token_edit`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `gen_reindent`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `gen_range_rewrite`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `gen_boilerplate_insert`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `sample_lines`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `main`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `insert_lines_above`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `insert_at_end`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `delete_lines`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `delete_past_end_clamped`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `edit_line`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `reindent_line`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `range_rewrite`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `boilerplate_insert`；`crates/codegen/tools/src/implementations/grow_build_hashline/mutate.rs` — `range_rewrite_expand`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols format_hashline_content, DESCRIPTION, HashlineReadTool, kind, tool_namespace, description_template, finalized_definition, requires_expr, Args, Output, id, description, capabilities, run, test_resources, format_basic_file, format_includes_anchor_with_context, main (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `format_hashline_content`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `DESCRIPTION`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `HashlineReadTool`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `kind`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `description_template`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `finalized_definition`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `requires_expr`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `Args`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `Output`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `id`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `description`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `capabilities`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `run`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `test_resources`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `format_basic_file`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `format_includes_anchor_with_context`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `main`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `format_with_offset_and_limit`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `format_empty_file`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `format_keeps_long_lines_whole`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `format_deterministic`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `tool_metadata`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `description_differs_from_standard`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `description_template_tracks_renamed_offset_limit`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `read_basic_file`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `read_long_line_unclipped_by_default`；`crates/codegen/tools/src/implementations/grow_build_hashline/read_file.rs` — `extracted_images_cleared_after_hashline_overwrite`。

### Requirement: Tools crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols AnchorScheme, name, hash_len, generate_anchors, validate, validation_window_lines, find_shifted, Anchor, render, fmt, ParsedAnchor, parse, ValidationResult, ShiftResult, DEFAULT_SEARCH_RADIUS, ContentOnly, new, with_hash_len (additional symbols omitted from the title but included in source evidence) follow explicit markers platform/feature conditional、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `AnchorScheme`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `name`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `hash_len`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `generate_anchors`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `validate`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `validation_window_lines`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `find_shifted`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `Anchor`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `render`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `fmt`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `ParsedAnchor`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `parse`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `ValidationResult`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `ShiftResult`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `DEFAULT_SEARCH_RADIUS`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `ContentOnly`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `new`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `with_hash_len`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `default`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `DEFAULT_CHUNK_SIZE`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `ChunkFingerprint`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `with_params`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `chunk_fingerprint`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `DEFAULT_CHECKPOINT_INTERVAL`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `CheckpointChain`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `checkpoint_fingerprint`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `find_shifted_generic`；`crates/codegen/tools/src/implementations/grow_build_hashline/scheme.rs` — `sample_lines`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/capabilities.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/capabilities.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols SavePolicy, ServerPolicy, from_capabilities, full_replacement_range, did_save, wants_incremental_sync, save_policy, uri, position, policy, a_bare_incremental_kind_means_incremental, roslyns_shape_is_incremental_with_no_save, include_text_is_the_only_way_to_get_the_text, a_server_with_no_sync_capability_keeps_the_historical_behaviour, only_incremental_servers_get_a_range follow explicit markers platform/feature conditional、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/lsp/capabilities.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `SavePolicy`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `ServerPolicy`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `from_capabilities`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `full_replacement_range`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `did_save`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `wants_incremental_sync`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `save_policy`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `uri`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `position`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `policy`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `a_bare_incremental_kind_means_incremental`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `roslyns_shape_is_incremental_with_no_save`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `include_text_is_the_only_way_to_get_the_text`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `a_server_with_no_sync_capability_keeps_the_historical_behaviour`；`crates/codegen/tools/src/implementations/lsp/capabilities.rs` — `only_incremental_servers_get_a_range`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/client.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/client.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols LspClient, fmt, drop, LspMainLoopAndServer, create_client_main_loop, TransportHandles, spawn_transport, build_initialize_params, initialize_with_timeout, send_initial_configuration, abort_transport, start, enroll, start_stdio, start_socket, close_all_documents, shutdown, reap_children (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、async task and cancellation lifecycle、child process execution、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/lsp/client.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/lsp/client.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/lsp/client.rs` — `LspClient`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `fmt`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `drop`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `LspMainLoopAndServer`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `create_client_main_loop`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `TransportHandles`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `spawn_transport`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `build_initialize_params`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `initialize_with_timeout`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `send_initial_configuration`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `abort_transport`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `start`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `enroll`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `start_stdio`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `start_socket`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `close_all_documents`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `shutdown`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `reap_children`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `client_capabilities`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `provider`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `tracked_documents`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `server_name`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `notify_file_change`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `get_diagnostics`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `goto_definition`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `goto_implementation`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `goto_references`；`crates/codegen/tools/src/implementations/lsp/client.rs` — `hover`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/config.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/config.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols DEFAULT_STARTUP_TIMEOUT_MS, DEFAULT_SHUTDOWN_TIMEOUT_MS, REQUEST_TIMEOUT, load_servers_with_plugins_sourced, filter_project_lsp_when_untrusted, load_servers, load_file, resolve_server, LspTransport, WorkspaceOpen, LspServerConfig, startup_timeout_ms, shutdown_timeout_ms, restart_on_crash, max_restarts, effective_root, sourced, untrusted_drops_only_project_keeps_user_and_plugin (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、timeout, budget, or rate limit、platform/feature conditional、sandbox, trust, or allow/deny policy、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/lsp/config.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/lsp/config.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/lsp/config.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/lsp/config.rs` — `DEFAULT_STARTUP_TIMEOUT_MS`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `DEFAULT_SHUTDOWN_TIMEOUT_MS`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `REQUEST_TIMEOUT`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `load_servers_with_plugins_sourced`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `filter_project_lsp_when_untrusted`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `load_servers`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `load_file`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `resolve_server`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `LspTransport`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `WorkspaceOpen`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `LspServerConfig`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `startup_timeout_ms`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `shutdown_timeout_ms`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `restart_on_crash`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `max_restarts`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `effective_root`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `sourced`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `untrusted_drops_only_project_keeps_user_and_plugin`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `trusted_keeps_all_including_project`；`crates/codegen/tools/src/implementations/lsp/config.rs` — `server_config_requires_canonical_snake_case_fields`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/diagnostics.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/diagnostics.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols Answer, new, NO_VERSION, DiagnosticsStore, Inner, error, server_publishes, install, install_if, record_push, confirm_unchanged, covers, answered_for, answer, items, forget, read, write (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、platform/feature conditional、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/lsp/diagnostics.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `Answer`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `new`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `NO_VERSION`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `DiagnosticsStore`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `Inner`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `error`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `server_publishes`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `install`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `install_if`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `record_push`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `confirm_unchanged`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `covers`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `answered_for`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `answer`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `items`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `forget`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `read`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `write`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `A`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `diagnostic`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `reported`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `an_unanswered_document_has_no_verdict`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `a_clean_answer_is_still_an_answer`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `an_answer_from_before_the_edit_does_not_count_as_a_reply_to_it`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `a_late_answer_does_not_overwrite_a_newer_one`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `an_overtaken_empty_answer_does_not_erase_what_we_know`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `an_answer_the_store_refused_leaves_no_result_id_behind`；`crates/codegen/tools/src/implementations/lsp/diagnostics.rs` — `unchanged_refreshes_the_verdict_without_losing_the_diagnostics`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/dispatch.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/dispatch.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols to, StartupState, StartupCoordinator, LspBackendAdapter, new, spawn_bootstrap_task, ensure_started_inner, ensure_started_with_state, bootstrap_lsp, drop, ensure_started_background, ensure_ready, is_ready, dispatch, drain_diagnostics, notify_file_changed, read_diagnostics, DispatchSockets (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、async task and cancellation lifecycle、child process execution、timeout, budget, or rate limit、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/lsp/dispatch.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/lsp/dispatch.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `to`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `StartupState`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `StartupCoordinator`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `LspBackendAdapter`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `new`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `spawn_bootstrap_task`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `ensure_started_inner`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `ensure_started_with_state`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `bootstrap_lsp`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `drop`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `ensure_started_background`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `ensure_ready`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `is_ready`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `dispatch`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `drain_diagnostics`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `notify_file_changed`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `read_diagnostics`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `DispatchSockets`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `err_result`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `ok_result`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `require_position`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `timed_request`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `DispatchError`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `from`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `dispatch_on_sockets`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `dispatch_goto`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `dispatch_references`；`crates/codegen/tools/src/implementations/lsp/dispatch.rs` — `dispatch_hover`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/documents.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/documents.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols FIRST_VERSION, _, Tracked, Update, version, Documents, new, plan, commit, contains, tracked, uris, versions, take_all, read, write, end_position, A (additional symbols omitted from the title but included in source evidence) follow explicit markers platform/feature conditional、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/lsp/documents.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/lsp/documents.rs` — `FIRST_VERSION`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `_`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `Tracked`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `Update`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `version`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `Documents`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `new`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `plan`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `commit`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `contains`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `tracked`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `uris`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `versions`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `take_all`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `read`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `write`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `end_position`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `A`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `an_unknown_document_is_opened_above_the_no_version_marker`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `a_committed_document_is_changed_from_where_it_ended`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `planning_alone_does_not_move_the_document`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `position`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `end_position_counts_the_trailing_newline_as_a_new_line`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `x`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `end_position_measures_in_utf16_code_units`；`crates/codegen/tools/src/implementations/lsp/documents.rs` — `take_all_empties_the_map`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/format.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/format.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols markup_string_to_text, flatten_document_symbols, format_locations_labeled, format_symbols follow explicit markers child process execution、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/lsp/format.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/lsp/format.rs` — `markup_string_to_text`；`crates/codegen/tools/src/implementations/lsp/format.rs` — `flatten_document_symbols`；`crates/codegen/tools/src/implementations/lsp/format.rs` — `format_locations_labeled`；`crates/codegen/tools/src/implementations/lsp/format.rs` — `format_symbols`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/manager.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/manager.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols DiagnosticsSummary, MAX_PER_FILE, MAX_PER_SUMMARY, CollectedDiagnostics, Reopened, append_file, trimmed_note, LspManager, default, new, with_process_scope, is_initialized, alloc_lifecycle_id, mark_uri_pending_diagnostics, ensure_initialized, shutdown, tools_enabled, restartable_servers (additional symbols omitted from the title but included in source evidence) follow explicit markers filesystem or durable persistence、explicit error classification、child process execution、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/lsp/manager.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/lsp/manager.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/lsp/manager.rs` — `DiagnosticsSummary`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `MAX_PER_FILE`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `MAX_PER_SUMMARY`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `CollectedDiagnostics`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `Reopened`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `append_file`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `trimmed_note`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `LspManager`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `default`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `new`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `with_process_scope`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `is_initialized`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `alloc_lifecycle_id`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `mark_uri_pending_diagnostics`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `ensure_initialized`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `shutdown`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `tools_enabled`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `restartable_servers`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `notify_file_changed`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `has_pending_diagnostics`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `worth_blocking_for_diagnostics`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `reopen_refreshed_questions`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `pending_file_count`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `pending_count`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `is_uri_pending`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `take_answered_diagnostics`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `socket_for_file`；`crates/codegen/tools/src/implementations/lsp/manager.rs` — `all_sockets`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/mod.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/mod.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols DIAGNOSTICS_DRAIN_TIMEOUT, LspError, DiagnosticsNotify, LspMainLoop, file_uri, text_document_position follow explicit markers explicit error classification、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/lsp/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/lsp/mod.rs` — `DIAGNOSTICS_DRAIN_TIMEOUT`；`crates/codegen/tools/src/implementations/lsp/mod.rs` — `LspError`；`crates/codegen/tools/src/implementations/lsp/mod.rs` — `DiagnosticsNotify`；`crates/codegen/tools/src/implementations/lsp/mod.rs` — `LspMainLoop`；`crates/codegen/tools/src/implementations/lsp/mod.rs` — `file_uri`；`crates/codegen/tools/src/implementations/lsp/mod.rs` — `text_document_position`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/pending.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/pending.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols VERDICT_TTL, SERVER_PATIENCE, PendingPolicy, default, PendingEdit, PendingEdits, new, mark, note_server_spoke, clear, lifecycle_id, is_empty, len, contains, take_answered, worth_blocking, SERVER, A (additional symbols omitted from the title but included in source evidence) follow explicit markers timeout, budget, or rate limit、platform/feature conditional、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/lsp/pending.rs` — `VERDICT_TTL`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `SERVER_PATIENCE`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `PendingPolicy`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `default`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `PendingEdit`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `PendingEdits`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `new`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `mark`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `note_server_spoke`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `clear`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `lifecycle_id`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `is_empty`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `len`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `contains`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `take_answered`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `worth_blocking`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `SERVER`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `A`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `B`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `diagnostic`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `a_verdict_on_the_edit_settles_it`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `a_clean_verdict_settles_it_too`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `a_verdict_on_the_previous_revision_does_not_settle_the_new_one`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `a_file_nobody_answers_for_is_eventually_let_go`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `a_productive_server_still_lets_go_of_a_file_it_never_answers_for`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `a_silent_server_stops_costing_every_turn_its_budget`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `a_server_that_starts_answering_is_waited_on_again`；`crates/codegen/tools/src/implementations/lsp/pending.rs` — `one_file_nobody_answers_for_does_not_make_a_busy_server_look_dead`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/pull.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/pull.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols REQUEST_TIMEOUT, CONFIRM_DELAY, _, PullSupport, SupportFlag, new, get, write_off, PullOutcome, PullDiagnostics, InFlight, support, will_answer, worth_asking, refresh_all, resolve, request, note_answered (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、async task and cancellation lifecycle、timeout, budget, or rate limit、platform/feature conditional、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/lsp/pull.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/lsp/pull.rs` — `REQUEST_TIMEOUT`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `CONFIRM_DELAY`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `_`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `PullSupport`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `SupportFlag`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `new`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `get`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `write_off`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `PullOutcome`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `PullDiagnostics`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `InFlight`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `support`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `will_answer`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `worth_asking`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `refresh_all`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `resolve`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `request`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `note_answered`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `disowned`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `commit`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `begin`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `finish`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `fmt`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `a_server_is_asked_until_it_says_no`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `detached_pull`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `an_answer_from_before_a_refresh_is_not_written_down`；`crates/codegen/tools/src/implementations/lsp/pull.rs` — `a_document_with_a_pull_running_is_queued_rather_than_raced`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/refresh.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/refresh.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols ProjectInitializationComplete, Params, METHOD, str, RefreshTarget, new, publish, refresh_all, take_invalidated, a_refresh_before_the_handshake_is_a_no_op, the_invalidation_is_reported_once follow explicit markers serde/json wire or configuration、explicit error classification、platform/feature conditional、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/lsp/refresh.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/lsp/refresh.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/implementations/lsp/refresh.rs` — `ProjectInitializationComplete`；`crates/codegen/tools/src/implementations/lsp/refresh.rs` — `Params`；`crates/codegen/tools/src/implementations/lsp/refresh.rs` — `METHOD`；`crates/codegen/tools/src/implementations/lsp/refresh.rs` — `str`；`crates/codegen/tools/src/implementations/lsp/refresh.rs` — `RefreshTarget`；`crates/codegen/tools/src/implementations/lsp/refresh.rs` — `new`；`crates/codegen/tools/src/implementations/lsp/refresh.rs` — `publish`；`crates/codegen/tools/src/implementations/lsp/refresh.rs` — `refresh_all`；`crates/codegen/tools/src/implementations/lsp/refresh.rs` — `take_invalidated`；`crates/codegen/tools/src/implementations/lsp/refresh.rs` — `a_refresh_before_the_handshake_is_a_no_op`；`crates/codegen/tools/src/implementations/lsp/refresh.rs` — `the_invalidation_is_reported_once`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/restart.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/restart.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols wait_for_crashed_lifecycle, replay_tracked_documents, take_crashed_client_if_current, discard_crashed_client_if_current, install_restarted_client, RestartContext, RestartOutcome, send_failed_notification, alloc_lifecycle_id_if_running, restart_lsp_with_retries, restart_monitor, MAX_BACKOFF, POLL_INTERVAL follow explicit markers filesystem or durable persistence、explicit error classification、child process execution、timeout, budget, or rate limit、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/lsp/restart.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/lsp/restart.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/lsp/restart.rs` — `wait_for_crashed_lifecycle`；`crates/codegen/tools/src/implementations/lsp/restart.rs` — `replay_tracked_documents`；`crates/codegen/tools/src/implementations/lsp/restart.rs` — `take_crashed_client_if_current`；`crates/codegen/tools/src/implementations/lsp/restart.rs` — `discard_crashed_client_if_current`；`crates/codegen/tools/src/implementations/lsp/restart.rs` — `install_restarted_client`；`crates/codegen/tools/src/implementations/lsp/restart.rs` — `RestartContext`；`crates/codegen/tools/src/implementations/lsp/restart.rs` — `RestartOutcome`；`crates/codegen/tools/src/implementations/lsp/restart.rs` — `send_failed_notification`；`crates/codegen/tools/src/implementations/lsp/restart.rs` — `alloc_lifecycle_id_if_running`；`crates/codegen/tools/src/implementations/lsp/restart.rs` — `restart_lsp_with_retries`；`crates/codegen/tools/src/implementations/lsp/restart.rs` — `restart_monitor`；`crates/codegen/tools/src/implementations/lsp/restart.rs` — `MAX_BACKOFF`；`crates/codegen/tools/src/implementations/lsp/restart.rs` — `POLL_INTERVAL`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/tests.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/tests.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols WAIT_TIMEOUT, wait_until, BRIEF_VERDICT_TTL, brief_policy, drain_until_reported, mock_server_config, start_mock_client, poll_diagnostics, single_server_manager, wait_for_server, did_change_carries_range_for_incremental_servers, x, wait_for_message, start_client_with, pull_diagnostics_populate_the_map, pull_diagnostics_send_previous_result_id, a_premature_empty_answer_does_not_erase_known_diagnostics, an_answer_about_the_previous_revision_does_not_settle_the_new_one (additional symbols omitted from the title but included in source evidence) follow explicit markers filesystem or durable persistence、explicit error classification、async task and cancellation lifecycle、channel, fanout, or acknowledgement flow、child process execution、timeout, budget, or rate limit、platform/feature conditional; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/lsp/tests.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/lsp/tests.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/lsp/tests.rs` — `WAIT_TIMEOUT`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `wait_until`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `BRIEF_VERDICT_TTL`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `brief_policy`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `drain_until_reported`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `mock_server_config`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `start_mock_client`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `poll_diagnostics`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `single_server_manager`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `wait_for_server`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `did_change_carries_range_for_incremental_servers`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `x`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `wait_for_message`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `start_client_with`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `pull_diagnostics_populate_the_map`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `pull_diagnostics_send_previous_result_id`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `a_premature_empty_answer_does_not_erase_known_diagnostics`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `an_answer_about_the_previous_revision_does_not_settle_the_new_one`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `an_answer_the_store_refused_leaves_no_result_id_behind`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `pull_diagnostics_are_not_retried_on_a_server_that_says_it_has_none`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `did_save_includes_text_when_the_server_asks_for_it`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `did_change_stays_rangeless_for_full_sync_servers`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `e2e_did_open_publishes_diagnostics`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `e2e_goto_definition`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `e2e_lsp_manager_full_lifecycle`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `e2e_did_change_updates_diagnostics`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `e2e_spawn_failure_is_graceful`；`crates/codegen/tools/src/implementations/lsp/tests.rs` — `e2e_multi_server_routing`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/types.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/types.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols LspConfig, is_enabled, LspToolResult, LspBackend, ensure_started_background, ensure_ready, is_ready, dispatch, drain_diagnostics, notify_file_changed, read_diagnostics, DiagnosticEntry, errors, DiagnosticSeverityLevel, FileDiagnosticEntry, LspOperation, fmt, LspToolInput follow explicit markers serde/json wire or configuration、explicit error classification、timeout, budget, or rate limit、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/lsp/types.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/lsp/types.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/lsp/types.rs` — `LspConfig`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `is_enabled`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `LspToolResult`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `LspBackend`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `ensure_started_background`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `ensure_ready`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `is_ready`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `dispatch`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `drain_diagnostics`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `notify_file_changed`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `read_diagnostics`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `DiagnosticEntry`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `errors`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `DiagnosticSeverityLevel`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `FileDiagnosticEntry`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `LspOperation`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `fmt`；`crates/codegen/tools/src/implementations/lsp/types.rs` — `LspToolInput`。

### Requirement: Tools crates/codegen/tools/src/implementations/lsp/workspace_open.rs LSP client, diagnostics, and document lifecycle contract
crates/codegen/tools/src/implementations/lsp/workspace_open.rs SHALL implement the LSP client, diagnostics, and document lifecycle boundary through manage client/workspace/document state, dispatch requests, collect diagnostics, and recover or restart pending work. Its source symbols SolutionOpen, Params, METHOD, str, ProjectOpen, send, resolve, relative_paths_resolve_against_the_workspace_root, absolute_paths_are_left_alone, a_missing_path_still_resolves follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、platform/feature conditional、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/lsp/workspace_open.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/lsp/workspace_open.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/lsp/workspace_open.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/lsp/workspace_open.rs` — `SolutionOpen`；`crates/codegen/tools/src/implementations/lsp/workspace_open.rs` — `Params`；`crates/codegen/tools/src/implementations/lsp/workspace_open.rs` — `METHOD`；`crates/codegen/tools/src/implementations/lsp/workspace_open.rs` — `str`；`crates/codegen/tools/src/implementations/lsp/workspace_open.rs` — `ProjectOpen`；`crates/codegen/tools/src/implementations/lsp/workspace_open.rs` — `send`；`crates/codegen/tools/src/implementations/lsp/workspace_open.rs` — `resolve`；`crates/codegen/tools/src/implementations/lsp/workspace_open.rs` — `relative_paths_resolve_against_the_workspace_root`；`crates/codegen/tools/src/implementations/lsp/workspace_open.rs` — `absolute_paths_are_left_alone`；`crates/codegen/tools/src/implementations/lsp/workspace_open.rs` — `a_missing_path_still_resolves`。

### Requirement: Tools crates/codegen/tools/src/implementations/mod.rs tool implementation and protocol contract
crates/codegen/tools/src/implementations/mod.rs SHALL implement the tool implementation and protocol boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols mod follow explicit markers explicit error classification、tool definition, schema, or registry projection、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/implementations/mod.rs` — `mod`。

### Requirement: Tools crates/codegen/tools/src/implementations/read_file/document.rs file read tool contract
crates/codegen/tools/src/implementations/read_file/document.rs SHALL implement the file read tool boundary through resolve offset/line/token budgets, read text or media, and select image/PDF/document extraction paths. Its source symbols MAX_DOCUMENT_BYTES, DOCUMENT_PROCESS_TIMEOUT, detect_format, format_label, str, run_bounded_document_task, convert_to_markdown, detection_prefers_content_over_extension, csv_uses_extension_fallback, converts_rtf_to_markdown follow explicit markers explicit error classification、timeout, budget, or rate limit、platform/feature conditional、session, prompt, goal, or subagent context、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/read_file/document.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/read_file/document.rs` — `MAX_DOCUMENT_BYTES`；`crates/codegen/tools/src/implementations/read_file/document.rs` — `DOCUMENT_PROCESS_TIMEOUT`；`crates/codegen/tools/src/implementations/read_file/document.rs` — `detect_format`；`crates/codegen/tools/src/implementations/read_file/document.rs` — `format_label`；`crates/codegen/tools/src/implementations/read_file/document.rs` — `str`；`crates/codegen/tools/src/implementations/read_file/document.rs` — `run_bounded_document_task`；`crates/codegen/tools/src/implementations/read_file/document.rs` — `convert_to_markdown`；`crates/codegen/tools/src/implementations/read_file/document.rs` — `detection_prefers_content_over_extension`；`crates/codegen/tools/src/implementations/read_file/document.rs` — `csv_uses_extension_fallback`；`crates/codegen/tools/src/implementations/read_file/document.rs` — `converts_rtf_to_markdown`。

### Requirement: Tools crates/codegen/tools/src/implementations/read_file/image.rs file read tool contract
crates/codegen/tools/src/implementations/read_file/image.rs SHALL implement the file read tool boundary through resolve offset/line/token budgets, read text or media, and select image/PDF/document extraction paths. Its source symbols CompressImageError, MAX_IMAGE_PAYLOAD_BYTES, MAX_IMAGE_RAW_BYTES, MAX_IMAGE_PIXELS, MAX_IMAGE_DIMENSION, MIN_IMAGE_DIMENSION, READFILE_QUALITY_STEPS, MAX_DECODE_PIXELS, compress_image_for_conversation, image_read_output, compress_image_for_conversation_with_caps, make_noisy_png, make_small_png, compress_small_image_returns_unchanged, compress_truncated_small_jpeg_re_encodes_to_valid_bytes, compress_small_gif_becomes_png, compress_large_noisy_image_picks_jpeg, compress_large_dimensions_small_bytes_downscales (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/read_file/image.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/implementations/read_file/image.rs` — `CompressImageError`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `MAX_IMAGE_PAYLOAD_BYTES`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `MAX_IMAGE_RAW_BYTES`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `MAX_IMAGE_PIXELS`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `MAX_IMAGE_DIMENSION`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `MIN_IMAGE_DIMENSION`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `READFILE_QUALITY_STEPS`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `MAX_DECODE_PIXELS`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_image_for_conversation`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `image_read_output`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_image_for_conversation_with_caps`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `make_noisy_png`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `make_small_png`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_small_image_returns_unchanged`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_truncated_small_jpeg_re_encodes_to_valid_bytes`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_small_gif_becomes_png`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_large_noisy_image_picks_jpeg`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_large_dimensions_small_bytes_downscales`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_wide_image_under_area_budget_passes_through`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_screenshot_respects_area_budget`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_camera_sized_photo_succeeds`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_above_api_ceiling_returns_pixel_limit_exceeded`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_small_undecodable_fails_closed`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_barely_over_limit_succeeds`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_oversized_zero_bytes_returns_format_detection_failed`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `compress_oversized_corrupt_png_returns_decode_failed`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `payload_cap_exceeded_display_string_pinned`；`crates/codegen/tools/src/implementations/read_file/image.rs` — `payload_cap_exceeded_reached_through_production_path`。

### Requirement: Tools crates/codegen/tools/src/implementations/read_file/metadata.rs file read tool contract
crates/codegen/tools/src/implementations/read_file/metadata.rs SHALL implement the file read tool boundary through resolve offset/line/token budgets, read text or media, and select image/PDF/document extraction paths. Its source symbols FileMetadata, is_image, bytes_to_metadata, from, file_metadata_identifies_images follow explicit markers explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/read_file/metadata.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/implementations/read_file/metadata.rs` — `FileMetadata`；`crates/codegen/tools/src/implementations/read_file/metadata.rs` — `is_image`；`crates/codegen/tools/src/implementations/read_file/metadata.rs` — `bytes_to_metadata`；`crates/codegen/tools/src/implementations/read_file/metadata.rs` — `from`；`crates/codegen/tools/src/implementations/read_file/metadata.rs` — `file_metadata_identifies_images`。

### Requirement: Tools crates/codegen/tools/src/implementations/read_file/mod.rs file read tool contract
crates/codegen/tools/src/implementations/read_file/mod.rs SHALL implement the file read tool boundary through resolve offset/line/token budgets, read text or media, and select image/PDF/document extraction paths. Its source symbols mod follow explicit markers explicit error classification、tool definition, schema, or registry projection、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/read_file/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/implementations/read_file/mod.rs` — `mod`。

### Requirement: Tools crates/codegen/tools/src/implementations/read_file/pdf.rs file read tool contract
crates/codegen/tools/src/implementations/read_file/pdf.rs SHALL implement the file read tool boundary through resolve offset/line/token budgets, read text or media, and select image/PDF/document extraction paths. Its source symbols PDF_MAX_PAGES_PER_READ, MAX_IMAGES_PER_PAGE, MAX_EXTRACTED_IMAGES, PDF_AUTO_IMAGE_PAGE_LIMIT, MIN_IMAGE_PIXELS, MAX_FORM_DEPTH, PdfExtraction, extract_pdf, parse_page_range, extract_pdf_inner, extract_page_images, collect_images, encode_image, indexed_to_image, cmyk_to_rgb, make_test_pdf, make_scanned_pdf, make_mixed_pdf (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/read_file/pdf.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `PDF_MAX_PAGES_PER_READ`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `MAX_IMAGES_PER_PAGE`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `MAX_EXTRACTED_IMAGES`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `PDF_AUTO_IMAGE_PAGE_LIMIT`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `MIN_IMAGE_PIXELS`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `MAX_FORM_DEPTH`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `PdfExtraction`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `extract_pdf`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `parse_page_range`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `extract_pdf_inner`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `extract_page_images`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `collect_images`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `encode_image`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `indexed_to_image`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `cmyk_to_rgb`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `make_test_pdf`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `make_scanned_pdf`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `make_mixed_pdf`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `parses_page_ranges`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `text_pdf_uses_markdown_without_images`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `scanned_pdf_extracts_raster_for_vision`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `mixed_pdf_keeps_text_and_extracts_visual_page`；`crates/codegen/tools/src/implementations/read_file/pdf.rs` — `page_selection_limits_both_text_and_visual_extraction`。

### Requirement: Tools crates/codegen/tools/src/implementations/search_tool/mod.rs search tool contract
crates/codegen/tools/src/implementations/search_tool/mod.rs SHALL implement the search tool boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols MAX_MCP_DESCRIPTION_LENGTH, TRUNCATION_SUFFIX, truncate_description, ServerFingerprint, hash_value, Fnv1aHasher, OFFSET_BASIS, PRIME, finish, write, fingerprint_servers, build_server_reminder, build_delta_reminder, format_server_line, format_compaction_server_line, format_server_line_inner, sanitize_description, SearchTool (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/search_tool/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/search_tool/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `MAX_MCP_DESCRIPTION_LENGTH`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `TRUNCATION_SUFFIX`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `truncate_description`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `ServerFingerprint`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `hash_value`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `Fnv1aHasher`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `OFFSET_BASIS`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `PRIME`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `finish`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `write`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `fingerprint_servers`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `build_server_reminder`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `build_delta_reminder`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `format_server_line`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `format_compaction_server_line`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `format_server_line_inner`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `sanitize_description`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `SearchTool`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `Args`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `Output`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `id`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `description`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `capabilities`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `run`；`crates/codegen/tools/src/implementations/search_tool/mod.rs` — `StaticToolIndex`。

### Requirement: Tools crates/codegen/tools/src/implementations/search_tool/types.rs search tool contract
crates/codegen/tools/src/implementations/search_tool/types.rs SHALL implement the search tool boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols SearchToolInput, default_limit follow explicit markers serde/json wire or configuration、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/search_tool/types.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/implementations/search_tool/types.rs` — `SearchToolInput`；`crates/codegen/tools/src/implementations/search_tool/types.rs` — `default_limit`。

### Requirement: Tools crates/codegen/tools/src/implementations/task_output/mod.rs task output and wait projection contract
crates/codegen/tools/src/implementations/task_output/mod.rs SHALL implement the task output and wait projection boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols mod follow explicit markers tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/task_output/mod.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/task_output/mod.rs` — `mod`。

### Requirement: Tools crates/codegen/tools/src/implementations/task_output/tool.rs task output and wait projection contract
crates/codegen/tools/src/implementations/task_output/tool.rs SHALL implement the task output and wait projection boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols snapshot_to_result, make_test_snapshot, test_snapshot_to_result_running, test_snapshot_to_result_completed_success, test_snapshot_to_result_completed_failed, test_snapshot_to_result_killed_is_cancelled_not_failed, test_snapshot_to_result_timeout_kill_is_timed_out, test_snapshot_to_result_explicit_kill_beats_timeout_signal, test_no_output_wording_tracks_task_state, test_snapshot_to_result_truncates_large_output, test_snapshot_to_result_uses_resolved_tool_name, test_snapshot_to_result_wraps_long_lines, test_snapshot_to_result_preserves_short_output, test_snapshot_to_result_respects_custom_max_output_bytes, test_snapshot_to_result_prefers_display_command, test_snapshot_to_result_falls_back_to_command_when_no_display follow explicit markers explicit error classification、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/task_output/tool.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/implementations/task_output/tool.rs` — `snapshot_to_result`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `make_test_snapshot`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_snapshot_to_result_running`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_snapshot_to_result_completed_success`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_snapshot_to_result_completed_failed`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_snapshot_to_result_killed_is_cancelled_not_failed`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_snapshot_to_result_timeout_kill_is_timed_out`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_snapshot_to_result_explicit_kill_beats_timeout_signal`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_no_output_wording_tracks_task_state`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_snapshot_to_result_truncates_large_output`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_snapshot_to_result_uses_resolved_tool_name`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_snapshot_to_result_wraps_long_lines`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_snapshot_to_result_preserves_short_output`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_snapshot_to_result_respects_custom_max_output_bytes`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_snapshot_to_result_prefers_display_command`；`crates/codegen/tools/src/implementations/task_output/tool.rs` — `test_snapshot_to_result_falls_back_to_command_when_no_display`。

### Requirement: Tools crates/codegen/tools/src/implementations/use_tool/mod.rs dynamic tool invocation contract
crates/codegen/tools/src/implementations/use_tool/mod.rs SHALL implement the dynamic tool invocation boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols UseToolInput, object_value_schema, UseToolParams, default_true, default, UseTool, dispatch_local_mcp, normalize_mcp_arguments, dispatch_mcp_tool, kind, tool_namespace, description_template, Args, Output, id, description, capabilities, run (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/use_tool/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/use_tool/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/use_tool/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `UseToolInput`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `object_value_schema`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `UseToolParams`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `default_true`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `default`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `UseTool`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `dispatch_local_mcp`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `normalize_mcp_arguments`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `dispatch_mcp_tool`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `kind`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `tool_namespace`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `description_template`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `Args`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `Output`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `id`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `description`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `capabilities`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `run`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `MockToolDispatch`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `call`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `SharedArgs`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `CapturingDispatch`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `ctx_capturing`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `ErrorToolDispatch`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `new_ctx`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `ctx_with_dispatch`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `rejects_builtin_tool_names`；`crates/codegen/tools/src/implementations/use_tool/mod.rs` — `errors_when_inner_dispatch_not_set`。

### Requirement: Tools crates/codegen/tools/src/lib.rs tools crate module boundary contract
crates/codegen/tools/src/lib.rs SHALL implement the tools crate module boundary boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols DEFAULT_TOOL_OUTPUT_BYTES, DEFAULT_TOOL_OUTPUT_CHARS follow explicit markers tool definition, schema, or registry projection、repository/worktree scope; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/lib.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/lib.rs` — `DEFAULT_TOOL_OUTPUT_BYTES`；`crates/codegen/tools/src/lib.rs` — `DEFAULT_TOOL_OUTPUT_CHARS`。

### Requirement: Tools crates/codegen/tools/src/registry/mod.rs tool registry, definitions, and indexing contract
crates/codegen/tools/src/registry/mod.rs SHALL implement the tool registry, definitions, and indexing boundary through register and resolve tool kinds/definitions, normalize metadata and schemas, and expose searchable indexes. Its source symbols mod follow explicit markers platform/feature conditional、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/registry/mod.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/registry/mod.rs` — `mod`。

### Requirement: Tools crates/codegen/tools/src/registry/tool_kind_contract.rs tool registry, definitions, and indexing contract
crates/codegen/tools/src/registry/tool_kind_contract.rs SHALL implement the tool registry, definitions, and indexing boundary through register and resolve tool kinds/definitions, normalize metadata and schemas, and expose searchable indexes. Its source symbols entry, expected_builtin_tool_kinds, registry_registers_exactly_the_builtin_allowlist_with_expected_kinds, kindless_mcp_custom_exceptions_are_explicit follow explicit markers sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、repository/worktree scope、LSP/diagnostic lifecycle、scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Policy boundary
- **WHEN** input crosses an allow/deny or sandbox check
- **THEN** the explicit policy branch controls admission before execution.

证据：`crates/codegen/tools/src/registry/tool_kind_contract.rs` — `entry`；`crates/codegen/tools/src/registry/tool_kind_contract.rs` — `expected_builtin_tool_kinds`；`crates/codegen/tools/src/registry/tool_kind_contract.rs` — `registry_registers_exactly_the_builtin_allowlist_with_expected_kinds`；`crates/codegen/tools/src/registry/tool_kind_contract.rs` — `kindless_mcp_custom_exceptions_are_explicit`。

### Requirement: Tools registry definitions, metadata, and searchable index contract
The registry SHALL register the declared builtin tool packs, normalize ToolConfig metadata and parameter schemas, resolve canonical/client names and aliases, expose tool definitions and access/requirements metadata, and maintain searchable tool/server summaries. MCP/custom kind exceptions remain explicit in the registry contract; registry visibility does not replace dispatch authorization.

#### Scenario: Builtin allowlist
- **WHEN** the registry is initialized
- **THEN** the declared builtin packs are registered with their expected kinds and metadata.

#### Scenario: Name resolution
- **WHEN** a canonical or client-facing tool name is looked up
- **THEN** the registry resolves the matching definition/alias or returns the explicit missing result.

#### Scenario: Custom MCP kind
- **WHEN** a kindless MCP/custom descriptor is registered
- **THEN** the explicit MCP exception is retained without changing builtin kind semantics.

证据：`crates/codegen/tools/src/registry/types.rs` — `ToolPack`；`crates/codegen/tools/src/registry/types.rs` — `TOOL_PACKS`；`crates/codegen/tools/src/registry/types.rs` — `tool_packs`；`crates/codegen/tools/src/registry/types.rs` — `Mutex`；`crates/codegen/tools/src/registry/types.rs` — `register_tool_pack`；`crates/codegen/tools/src/registry/types.rs` — `ToolConfig`；`crates/codegen/tools/src/registry/types.rs` — `via`；`crates/codegen/tools/src/registry/types.rs` — `for_tool`；`crates/codegen/tools/src/registry/types.rs` — `from_id`；`crates/codegen/tools/src/registry/types.rs` — `with_name`；`crates/codegen/tools/src/registry/types.rs` — `with_description`；`crates/codegen/tools/src/registry/types.rs` — `with_param`；`crates/codegen/tools/src/registry/types.rs` — `with_param_rename`；`crates/codegen/tools/src/registry/types.rs` — `resolve_client_name`；`crates/codegen/tools/src/registry/types.rs` — `from`；`crates/codegen/tools/src/registry/types.rs` — `ToolServerConfig`；`crates/codegen/tools/src/registry/types.rs` — `SubagentSessionResources`；`crates/codegen/tools/src/registry/types.rs` — `SessionContext`；`crates/codegen/tools/src/registry/types.rs` — `str`；`crates/codegen/tools/src/registry/types.rs` — `DefaultToolMetadata`；`crates/codegen/tools/src/registry/types.rs` — `kind`；`crates/codegen/tools/src/registry/types.rs` — `tool_namespace`；`crates/codegen/tools/src/registry/types.rs` — `description_template`；`crates/codegen/tools/src/registry/types.rs` — `drain_value_stream`；`crates/codegen/tools/src/registry/types.rs` — `stream_no_terminal_error`；`crates/codegen/tools/src/registry/types.rs` — `OutputConverter`；`crates/codegen/tools/src/registry/types.rs` — `DispatchParts`；`crates/codegen/tools/src/registry/types.rs` — `at`。

### Requirement: Tools crates/codegen/tools/src/tool_taxonomy.rs tools crate module boundary contract
crates/codegen/tools/src/tool_taxonomy.rs SHALL implement the tools crate module boundary boundary through compose the tools crate public/module boundary and its explicit input/output/error paths. Its source symbols definitions, PATH, OFFSET, LIMIT, COMMAND, DESCRIPTION, CWD, DIRECTORY, PATTERN, TOOL_META_KEY, TOOL_META_VERSION, presentation_name, str, ToolIdentity, CanonicalToolMeta, stays, new, merge_into (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/tool_taxonomy.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/tool_taxonomy.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/tool_taxonomy.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/tool_taxonomy.rs` — `definitions`；`crates/codegen/tools/src/tool_taxonomy.rs` — `PATH`；`crates/codegen/tools/src/tool_taxonomy.rs` — `OFFSET`；`crates/codegen/tools/src/tool_taxonomy.rs` — `LIMIT`；`crates/codegen/tools/src/tool_taxonomy.rs` — `COMMAND`；`crates/codegen/tools/src/tool_taxonomy.rs` — `DESCRIPTION`；`crates/codegen/tools/src/tool_taxonomy.rs` — `CWD`；`crates/codegen/tools/src/tool_taxonomy.rs` — `DIRECTORY`；`crates/codegen/tools/src/tool_taxonomy.rs` — `PATTERN`；`crates/codegen/tools/src/tool_taxonomy.rs` — `TOOL_META_KEY`；`crates/codegen/tools/src/tool_taxonomy.rs` — `TOOL_META_VERSION`；`crates/codegen/tools/src/tool_taxonomy.rs` — `presentation_name`；`crates/codegen/tools/src/tool_taxonomy.rs` — `str`；`crates/codegen/tools/src/tool_taxonomy.rs` — `ToolIdentity`；`crates/codegen/tools/src/tool_taxonomy.rs` — `CanonicalToolMeta`；`crates/codegen/tools/src/tool_taxonomy.rs` — `stays`；`crates/codegen/tools/src/tool_taxonomy.rs` — `new`；`crates/codegen/tools/src/tool_taxonomy.rs` — `merge_into`；`crates/codegen/tools/src/tool_taxonomy.rs` — `by`；`crates/codegen/tools/src/tool_taxonomy.rs` — `tool_meta_json_schema_str`；`crates/codegen/tools/src/tool_taxonomy.rs` — `identity`；`crates/codegen/tools/src/tool_taxonomy.rs` — `namespace_round_trips_canonical_wire_values`；`crates/codegen/tools/src/tool_taxonomy.rs` — `wire`；`crates/codegen/tools/src/tool_taxonomy.rs` — `unknown_kind_is_rejected`；`crates/codegen/tools/src/tool_taxonomy.rs` — `kind_and_namespace_schemas_are_closed`；`crates/codegen/tools/src/tool_taxonomy.rs` — `canonical_meta_wire_shape_round_trips`；`crates/codegen/tools/src/tool_taxonomy.rs` — `tool_meta_schema_is_up_to_date`；`crates/codegen/tools/src/tool_taxonomy.rs` — `merge_into_nests_under_one_key_and_preserves_existing`。

### Requirement: Tools crates/codegen/tools/src/types/api_key_provider.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/api_key_provider.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols ApiKeyProvider, providers, current_api_key, current_api_key_async, SharedApiKeyProvider follow explicit markers tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/types/api_key_provider.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/types/api_key_provider.rs` — `ApiKeyProvider`；`crates/codegen/tools/src/types/api_key_provider.rs` — `providers`；`crates/codegen/tools/src/types/api_key_provider.rs` — `current_api_key`；`crates/codegen/tools/src/types/api_key_provider.rs` — `current_api_key_async`；`crates/codegen/tools/src/types/api_key_provider.rs` — `SharedApiKeyProvider`。

### Requirement: Tools crates/codegen/tools/src/types/config_source.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/config_source.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols ConfigSource, display_short, display_label, plugin_name, path follow explicit markers serde/json wire or configuration; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/config_source.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/types/config_source.rs` — `ConfigSource`；`crates/codegen/tools/src/types/config_source.rs` — `display_short`；`crates/codegen/tools/src/types/config_source.rs` — `display_label`；`crates/codegen/tools/src/types/config_source.rs` — `plugin_name`；`crates/codegen/tools/src/types/config_source.rs` — `path`。

### Requirement: Tools crates/codegen/tools/src/types/context.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/context.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols MAX_LINES_READ_DEFAULT, TruncationConfig, max_lines_read, max_output_bytes_for, mcp_max_output_bytes_for, interpolate_description, apply_to_schema, max_lines_read_default_and_override, interpolate_description_resolves_max_wait_ms, apply_to_schema_resolves_and_pins_the_wait_property, apply_to_schema_tracks_a_raised_ceiling_and_a_renamed_property, apply_to_schema_bounds_the_real_optional_u64_property, apply_to_schema_tolerates_schemas_without_properties, mcp_max_output_bytes_for_lookup_order, mcp_override_does_not_bleed_into_non_mcp_lookup follow explicit markers serde/json wire or configuration、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/context.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/types/context.rs` — `MAX_LINES_READ_DEFAULT`；`crates/codegen/tools/src/types/context.rs` — `TruncationConfig`；`crates/codegen/tools/src/types/context.rs` — `max_lines_read`；`crates/codegen/tools/src/types/context.rs` — `max_output_bytes_for`；`crates/codegen/tools/src/types/context.rs` — `mcp_max_output_bytes_for`；`crates/codegen/tools/src/types/context.rs` — `interpolate_description`；`crates/codegen/tools/src/types/context.rs` — `apply_to_schema`；`crates/codegen/tools/src/types/context.rs` — `max_lines_read_default_and_override`；`crates/codegen/tools/src/types/context.rs` — `interpolate_description_resolves_max_wait_ms`；`crates/codegen/tools/src/types/context.rs` — `apply_to_schema_resolves_and_pins_the_wait_property`；`crates/codegen/tools/src/types/context.rs` — `apply_to_schema_tracks_a_raised_ceiling_and_a_renamed_property`；`crates/codegen/tools/src/types/context.rs` — `apply_to_schema_bounds_the_real_optional_u64_property`；`crates/codegen/tools/src/types/context.rs` — `apply_to_schema_tolerates_schemas_without_properties`；`crates/codegen/tools/src/types/context.rs` — `mcp_max_output_bytes_for_lookup_order`；`crates/codegen/tools/src/types/context.rs` — `mcp_override_does_not_bleed_into_non_mcp_lookup`。

### Requirement: Tools crates/codegen/tools/src/types/definition.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/definition.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols ToolType, ToolDefinition, function, FunctionTool follow explicit markers serde/json wire or configuration、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/definition.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/types/definition.rs` — `ToolType`；`crates/codegen/tools/src/types/definition.rs` — `ToolDefinition`；`crates/codegen/tools/src/types/definition.rs` — `function`；`crates/codegen/tools/src/types/definition.rs` — `FunctionTool`。

### Requirement: Tools crates/codegen/tools/src/types/description.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/description.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols DescriptionContext, make_desc_env, resolve_description, render_with_model_slugs, ctx_with_tools, variable_substitution, conditional_section_enabled, conditional_section_disabled, literal_braces_pass_through, fallback_to_raw_template_on_render_failure, no_template_markers_returns_unchanged, render_with_model_slugs_sorts_dedups_and_loops, multiple_tools_in_one_description, param_name_substitution, mixed_conditionals_and_substitutions, empty_template_returns_empty, canonical_names_as_default_when_no_override follow explicit markers serde/json wire or configuration、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/types/description.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/description.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/types/description.rs` — `DescriptionContext`；`crates/codegen/tools/src/types/description.rs` — `make_desc_env`；`crates/codegen/tools/src/types/description.rs` — `resolve_description`；`crates/codegen/tools/src/types/description.rs` — `render_with_model_slugs`；`crates/codegen/tools/src/types/description.rs` — `ctx_with_tools`；`crates/codegen/tools/src/types/description.rs` — `variable_substitution`；`crates/codegen/tools/src/types/description.rs` — `conditional_section_enabled`；`crates/codegen/tools/src/types/description.rs` — `conditional_section_disabled`；`crates/codegen/tools/src/types/description.rs` — `literal_braces_pass_through`；`crates/codegen/tools/src/types/description.rs` — `fallback_to_raw_template_on_render_failure`；`crates/codegen/tools/src/types/description.rs` — `no_template_markers_returns_unchanged`；`crates/codegen/tools/src/types/description.rs` — `render_with_model_slugs_sorts_dedups_and_loops`；`crates/codegen/tools/src/types/description.rs` — `multiple_tools_in_one_description`；`crates/codegen/tools/src/types/description.rs` — `param_name_substitution`；`crates/codegen/tools/src/types/description.rs` — `mixed_conditionals_and_substitutions`；`crates/codegen/tools/src/types/description.rs` — `empty_template_returns_empty`；`crates/codegen/tools/src/types/description.rs` — `canonical_names_as_default_when_no_override`。

### Requirement: Tools crates/codegen/tools/src/types/error.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/error.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols for, SearchReplaceError, impl_runtime_error_from, from, tool follow explicit markers explicit error classification、tool definition, schema, or registry projection、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/types/error.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/types/error.rs` — `for`；`crates/codegen/tools/src/types/error.rs` — `SearchReplaceError`；`crates/codegen/tools/src/types/error.rs` — `impl_runtime_error_from`；`crates/codegen/tools/src/types/error.rs` — `from`；`crates/codegen/tools/src/types/error.rs` — `tool`。

### Requirement: Tools crates/codegen/tools/src/types/memory_backend.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/memory_backend.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols for, MEMORY_LOG_TARGET, STALE_NOTE_DAYS, VERY_STALE_DAYS, format_staleness_note, format_age, MemorySearchResult, MemoryBackend, search, get, total_chunks, default_search_max_results, default_search_min_score, now_secs, staleness_global_is_evergreen, staleness_workspace_is_evergreen, staleness_none_created_at_is_empty, staleness_fresh_is_empty (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/types/memory_backend.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/types/memory_backend.rs` — `for`；`crates/codegen/tools/src/types/memory_backend.rs` — `MEMORY_LOG_TARGET`；`crates/codegen/tools/src/types/memory_backend.rs` — `STALE_NOTE_DAYS`；`crates/codegen/tools/src/types/memory_backend.rs` — `VERY_STALE_DAYS`；`crates/codegen/tools/src/types/memory_backend.rs` — `format_staleness_note`；`crates/codegen/tools/src/types/memory_backend.rs` — `format_age`；`crates/codegen/tools/src/types/memory_backend.rs` — `MemorySearchResult`；`crates/codegen/tools/src/types/memory_backend.rs` — `MemoryBackend`；`crates/codegen/tools/src/types/memory_backend.rs` — `search`；`crates/codegen/tools/src/types/memory_backend.rs` — `get`；`crates/codegen/tools/src/types/memory_backend.rs` — `total_chunks`；`crates/codegen/tools/src/types/memory_backend.rs` — `default_search_max_results`；`crates/codegen/tools/src/types/memory_backend.rs` — `default_search_min_score`；`crates/codegen/tools/src/types/memory_backend.rs` — `now_secs`；`crates/codegen/tools/src/types/memory_backend.rs` — `staleness_global_is_evergreen`；`crates/codegen/tools/src/types/memory_backend.rs` — `staleness_workspace_is_evergreen`；`crates/codegen/tools/src/types/memory_backend.rs` — `staleness_none_created_at_is_empty`；`crates/codegen/tools/src/types/memory_backend.rs` — `staleness_fresh_is_empty`；`crates/codegen/tools/src/types/memory_backend.rs` — `staleness_note_for_moderately_old`；`crates/codegen/tools/src/types/memory_backend.rs` — `staleness_warning_for_very_old`；`crates/codegen/tools/src/types/memory_backend.rs` — `format_age_hours`；`crates/codegen/tools/src/types/memory_backend.rs` — `format_age_one_day`；`crates/codegen/tools/src/types/memory_backend.rs` — `format_age_several_days`；`crates/codegen/tools/src/types/memory_backend.rs` — `format_age_one_week`；`crates/codegen/tools/src/types/memory_backend.rs` — `format_age_weeks`；`crates/codegen/tools/src/types/memory_backend.rs` — `staleness_note_at_exact_one_day`；`crates/codegen/tools/src/types/memory_backend.rs` — `staleness_fresh_just_under_one_day`；`crates/codegen/tools/src/types/memory_backend.rs` — `staleness_stale_at_exact_seven_days`。

### Requirement: Tools crates/codegen/tools/src/types/mod.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/mod.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols mod follow explicit markers explicit error classification、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/types/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

证据：`crates/codegen/tools/src/types/mod.rs` — `mod`。

### Requirement: Tools crates/codegen/tools/src/types/output.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/output.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols line_diff, TextOutput, from, DynamicOutput, ToolRunResult, into_typed_tool_output, typed_tool_output_preserving_cco, ListDirContent, ListDirOutput, GrepLineMatch, GrepFileMatch, GrepSearchOutput, FileContent, so, requires, happens, ImageContent, of (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、child process execution、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/types/output.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/output.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/types/output.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/types/output.rs` — `line_diff`；`crates/codegen/tools/src/types/output.rs` — `TextOutput`；`crates/codegen/tools/src/types/output.rs` — `from`；`crates/codegen/tools/src/types/output.rs` — `DynamicOutput`；`crates/codegen/tools/src/types/output.rs` — `ToolRunResult`；`crates/codegen/tools/src/types/output.rs` — `into_typed_tool_output`；`crates/codegen/tools/src/types/output.rs` — `typed_tool_output_preserving_cco`；`crates/codegen/tools/src/types/output.rs` — `ListDirContent`；`crates/codegen/tools/src/types/output.rs` — `ListDirOutput`；`crates/codegen/tools/src/types/output.rs` — `GrepLineMatch`；`crates/codegen/tools/src/types/output.rs` — `GrepFileMatch`；`crates/codegen/tools/src/types/output.rs` — `GrepSearchOutput`；`crates/codegen/tools/src/types/output.rs` — `FileContent`；`crates/codegen/tools/src/types/output.rs` — `so`；`crates/codegen/tools/src/types/output.rs` — `requires`；`crates/codegen/tools/src/types/output.rs` — `happens`；`crates/codegen/tools/src/types/output.rs` — `ImageContent`；`crates/codegen/tools/src/types/output.rs` — `of`；`crates/codegen/tools/src/types/output.rs` — `ReadFileOutput`；`crates/codegen/tools/src/types/output.rs` — `SearchReplaceEditsApplied`；`crates/codegen/tools/src/types/output.rs` — `SearchReplaceEditContextInformation`；`crates/codegen/tools/src/types/output.rs` — `SearchReplaceEditDetail`；`crates/codegen/tools/src/types/output.rs` — `NoMatchesFoundError`；`crates/codegen/tools/src/types/output.rs` — `for`；`crates/codegen/tools/src/types/output.rs` — `SearchReplaceOutput`；`crates/codegen/tools/src/types/output.rs` — `BashOutput`；`crates/codegen/tools/src/types/output.rs` — `make_output_for_prompt`；`crates/codegen/tools/src/types/output.rs` — `BackgroundTaskStarted`。

### Requirement: Tools crates/codegen/tools/src/types/params_validation.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/params_validation.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols ParamValidationError, new, with_field_path, with_expected, with_bad_value, validate_params_json, normalize_serde_path, value_at_path, classify_serde_error, str, extract_expected follow explicit markers serde/json wire or configuration、explicit error classification; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/types/params_validation.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/params_validation.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/types/params_validation.rs` — `ParamValidationError`；`crates/codegen/tools/src/types/params_validation.rs` — `new`；`crates/codegen/tools/src/types/params_validation.rs` — `with_field_path`；`crates/codegen/tools/src/types/params_validation.rs` — `with_expected`；`crates/codegen/tools/src/types/params_validation.rs` — `with_bad_value`；`crates/codegen/tools/src/types/params_validation.rs` — `validate_params_json`；`crates/codegen/tools/src/types/params_validation.rs` — `normalize_serde_path`；`crates/codegen/tools/src/types/params_validation.rs` — `value_at_path`；`crates/codegen/tools/src/types/params_validation.rs` — `classify_serde_error`；`crates/codegen/tools/src/types/params_validation.rs` — `str`；`crates/codegen/tools/src/types/params_validation.rs` — `extract_expected`。

### Requirement: Tools crates/codegen/tools/src/types/process_manager.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/process_manager.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols has, format_system_time_rfc3339 follow explicit markers typed inputs and outputs; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/types/process_manager.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/types/process_manager.rs` — `has`；`crates/codegen/tools/src/types/process_manager.rs` — `format_system_time_rfc3339`。

### Requirement: Tools crates/codegen/tools/src/types/requirements.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/requirements.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols Expr, from, and, eval, ToolParamsRequirement, new, check, ProposedTool, metadata, EvalContext, ToolRequirement, if_params, tool_kind, tool_kind_with_params, input_param, tool, tool_with_params follow explicit markers serde/json wire or configuration、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/requirements.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/types/requirements.rs` — `Expr`；`crates/codegen/tools/src/types/requirements.rs` — `from`；`crates/codegen/tools/src/types/requirements.rs` — `and`；`crates/codegen/tools/src/types/requirements.rs` — `eval`；`crates/codegen/tools/src/types/requirements.rs` — `ToolParamsRequirement`；`crates/codegen/tools/src/types/requirements.rs` — `new`；`crates/codegen/tools/src/types/requirements.rs` — `check`；`crates/codegen/tools/src/types/requirements.rs` — `ProposedTool`；`crates/codegen/tools/src/types/requirements.rs` — `metadata`；`crates/codegen/tools/src/types/requirements.rs` — `EvalContext`；`crates/codegen/tools/src/types/requirements.rs` — `ToolRequirement`；`crates/codegen/tools/src/types/requirements.rs` — `if_params`；`crates/codegen/tools/src/types/requirements.rs` — `tool_kind`；`crates/codegen/tools/src/types/requirements.rs` — `tool_kind_with_params`；`crates/codegen/tools/src/types/requirements.rs` — `input_param`；`crates/codegen/tools/src/types/requirements.rs` — `tool`；`crates/codegen/tools/src/types/requirements.rs` — `tool_with_params`。

### Requirement: Tools crates/codegen/tools/src/types/resources.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/resources.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols for, ResourceType, ID, str, validate_params_value, Params, with, register_resource, default, Target, deref, deref_mut, serialize, deserialize, State, ResourceCategory, as_str, SerializeFn (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、child process execution、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/types/resources.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/resources.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/types/resources.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/types/resources.rs` — `for`；`crates/codegen/tools/src/types/resources.rs` — `ResourceType`；`crates/codegen/tools/src/types/resources.rs` — `ID`；`crates/codegen/tools/src/types/resources.rs` — `str`；`crates/codegen/tools/src/types/resources.rs` — `validate_params_value`；`crates/codegen/tools/src/types/resources.rs` — `Params`；`crates/codegen/tools/src/types/resources.rs` — `with`；`crates/codegen/tools/src/types/resources.rs` — `register_resource`；`crates/codegen/tools/src/types/resources.rs` — `default`；`crates/codegen/tools/src/types/resources.rs` — `Target`；`crates/codegen/tools/src/types/resources.rs` — `deref`；`crates/codegen/tools/src/types/resources.rs` — `deref_mut`；`crates/codegen/tools/src/types/resources.rs` — `serialize`；`crates/codegen/tools/src/types/resources.rs` — `deserialize`；`crates/codegen/tools/src/types/resources.rs` — `State`；`crates/codegen/tools/src/types/resources.rs` — `ResourceCategory`；`crates/codegen/tools/src/types/resources.rs` — `as_str`；`crates/codegen/tools/src/types/resources.rs` — `SerializeFn`；`crates/codegen/tools/src/types/resources.rs` — `DeserializeFn`；`crates/codegen/tools/src/types/resources.rs` — `ResourceEntry`；`crates/codegen/tools/src/types/resources.rs` — `Resources`；`crates/codegen/tools/src/types/resources.rs` — `SharedResources`；`crates/codegen/tools/src/types/resources.rs` — `new`；`crates/codegen/tools/src/types/resources.rs` — `into_shared`；`crates/codegen/tools/src/types/resources.rs` — `reconfigure_from`；`crates/codegen/tools/src/types/resources.rs` — `get`；`crates/codegen/tools/src/types/resources.rs` — `name`；`crates/codegen/tools/src/types/resources.rs` — `require`。

### Requirement: Tools crates/codegen/tools/src/types/schema.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/schema.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols GrowIntegerSchema, schema_name, json_schema, F64_EXACT_INTEGER_LIMIT, parse_string_to_f64, parse_lenient_whole_f64, parse_lenient_u64_value, deserialize_lenient_option_u64, deserialize_lenient_u32, deserialize_lenient_u64, deserialize_lenient_usize, parse_lenient_i64_value, deserialize_lenient_i64, deserialize_u32, Wrapper, deserialize_u64, deserialize_usize, u32_accepts_whole_float (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/types/schema.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/schema.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/types/schema.rs` — `GrowIntegerSchema`；`crates/codegen/tools/src/types/schema.rs` — `schema_name`；`crates/codegen/tools/src/types/schema.rs` — `json_schema`；`crates/codegen/tools/src/types/schema.rs` — `F64_EXACT_INTEGER_LIMIT`；`crates/codegen/tools/src/types/schema.rs` — `parse_string_to_f64`；`crates/codegen/tools/src/types/schema.rs` — `parse_lenient_whole_f64`；`crates/codegen/tools/src/types/schema.rs` — `parse_lenient_u64_value`；`crates/codegen/tools/src/types/schema.rs` — `deserialize_lenient_option_u64`；`crates/codegen/tools/src/types/schema.rs` — `deserialize_lenient_u32`；`crates/codegen/tools/src/types/schema.rs` — `deserialize_lenient_u64`；`crates/codegen/tools/src/types/schema.rs` — `deserialize_lenient_usize`；`crates/codegen/tools/src/types/schema.rs` — `parse_lenient_i64_value`；`crates/codegen/tools/src/types/schema.rs` — `deserialize_lenient_i64`；`crates/codegen/tools/src/types/schema.rs` — `deserialize_u32`；`crates/codegen/tools/src/types/schema.rs` — `Wrapper`；`crates/codegen/tools/src/types/schema.rs` — `deserialize_u64`；`crates/codegen/tools/src/types/schema.rs` — `deserialize_usize`；`crates/codegen/tools/src/types/schema.rs` — `u32_accepts_whole_float`；`crates/codegen/tools/src/types/schema.rs` — `u32_accepts_integer`；`crates/codegen/tools/src/types/schema.rs` — `u32_null_is_none`；`crates/codegen/tools/src/types/schema.rs` — `u32_missing_is_none`；`crates/codegen/tools/src/types/schema.rs` — `u32_rejects_fractional_float`；`crates/codegen/tools/src/types/schema.rs` — `u32_rejects_negative`；`crates/codegen/tools/src/types/schema.rs` — `u32_rejects_above_max`；`crates/codegen/tools/src/types/schema.rs` — `u64_accepts_whole_float`；`crates/codegen/tools/src/types/schema.rs` — `u64_accepts_integer`；`crates/codegen/tools/src/types/schema.rs` — `u64_null_is_none`；`crates/codegen/tools/src/types/schema.rs` — `u64_missing_is_none`。

### Requirement: Tools crates/codegen/tools/src/types/skill_discovery_tracker/conditional.rs skill discovery tracker and listing budgets contract
crates/codegen/tools/src/types/skill_discovery_tracker/conditional.rs SHALL implement the skill discovery tracker and listing budgets boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols ConditionalSkills, is_empty, is_pending, take_unconditional, hold_dynamic, activate_for_paths, rehide, skill_matches_any follow explicit markers tool definition, schema, or registry projection、session, prompt, goal, or subagent context、repository/worktree scope; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/types/skill_discovery_tracker/conditional.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/types/skill_discovery_tracker/conditional.rs` — `ConditionalSkills`；`crates/codegen/tools/src/types/skill_discovery_tracker/conditional.rs` — `is_empty`；`crates/codegen/tools/src/types/skill_discovery_tracker/conditional.rs` — `is_pending`；`crates/codegen/tools/src/types/skill_discovery_tracker/conditional.rs` — `take_unconditional`；`crates/codegen/tools/src/types/skill_discovery_tracker/conditional.rs` — `hold_dynamic`；`crates/codegen/tools/src/types/skill_discovery_tracker/conditional.rs` — `activate_for_paths`；`crates/codegen/tools/src/types/skill_discovery_tracker/conditional.rs` — `rehide`；`crates/codegen/tools/src/types/skill_discovery_tracker/conditional.rs` — `skill_matches_any`。

### Requirement: Tools crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs skill discovery tracker and listing budgets contract
crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs SHALL implement the skill discovery tracker and listing budgets boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols SKILL_BUDGET_CONTEXT_PERCENT, DEFAULT_CHAR_BUDGET, MAX_LISTING_COMBINED_BYTES, MIN_DESC_LENGTH, TRIGGER_PREFIXES, DEFAULT_SKILL_TOOL_NAME, listing_header, is_listable, SkillEntry, func_desc, format, proportional_budgets, name_only, overhead, SkillListing, render, names_only, skill_source_dir (additional symbols omitted from the title but included in source evidence) follow explicit markers filesystem or durable persistence、child process execution、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `SKILL_BUDGET_CONTEXT_PERCENT`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `DEFAULT_CHAR_BUDGET`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `MAX_LISTING_COMBINED_BYTES`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `MIN_DESC_LENGTH`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `TRIGGER_PREFIXES`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `DEFAULT_SKILL_TOOL_NAME`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `listing_header`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `is_listable`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `SkillEntry`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `func_desc`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `format`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `proportional_budgets`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `name_only`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `overhead`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `SkillListing`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `render`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `names_only`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `skill_source_dir`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `collect_source_dirs`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `extract_trigger_suffix`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `strip_leading_trigger_prefix`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `build_skill_entry`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `format_announcement`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `format_compaction_skill_listing`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `skill`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `announce`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `no_skills_returns_none`；`crates/codegen/tools/src/types/skill_discovery_tracker/listing.rs` — `use_when_label_not_duplicated_for_embedded_triggers`。

### Requirement: Tools crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs skill discovery tracker and listing budgets contract
crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs SHALL implement the skill discovery tracker and listing budgets boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols SkillUpdateKind, SkillListingSnapshot, carries, SkillUpdateEffects, SkillManager, canonical_path, PendingKind, dedup_by_canonical_path, dedupe_by_canonical_path_and_name, ListingRenderParams, render_listing, new, set_skill_tool_name, restore_announced_names, announced_names, seed, set_context_window_tokens, update_startup_baseline (additional symbols omitted from the title but included in source evidence) follow explicit markers filesystem or durable persistence、child process execution、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、repository/worktree scope; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `SkillUpdateKind`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `SkillListingSnapshot`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `carries`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `SkillUpdateEffects`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `SkillManager`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `canonical_path`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `PendingKind`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `dedup_by_canonical_path`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `dedupe_by_canonical_path_and_name`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `ListingRenderParams`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `render_listing`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `new`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `set_skill_tool_name`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `restore_announced_names`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `announced_names`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `seed`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `set_context_window_tokens`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `update_startup_baseline`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `add_discovered`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `activate_conditional_skills_for_paths`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `take_pending`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `take_pending_reconciliation`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `slash_skills`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `listing_snapshot`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `render_params`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `discovered_skills`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `startup_skills`；`crates/codegen/tools/src/types/skill_discovery_tracker/mod.rs` — `on_compaction`。

### Requirement: Tools crates/codegen/tools/src/types/template_renderer.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/template_renderer.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols ToolsContext, TemplateContext, render_with_env, TemplateRenderError, template, fmt, source, strip_markers_on_render_failure, strip_template_markers, TemplateRenderer, new, with_system_reminders_enabled, render, param_names, render_schema_descriptions, tool_for_kind, param_for_kind, resolve (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/types/template_renderer.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/template_renderer.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/types/template_renderer.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/types/template_renderer.rs` — `ToolsContext`；`crates/codegen/tools/src/types/template_renderer.rs` — `TemplateContext`；`crates/codegen/tools/src/types/template_renderer.rs` — `render_with_env`；`crates/codegen/tools/src/types/template_renderer.rs` — `TemplateRenderError`；`crates/codegen/tools/src/types/template_renderer.rs` — `template`；`crates/codegen/tools/src/types/template_renderer.rs` — `fmt`；`crates/codegen/tools/src/types/template_renderer.rs` — `source`；`crates/codegen/tools/src/types/template_renderer.rs` — `strip_markers_on_render_failure`；`crates/codegen/tools/src/types/template_renderer.rs` — `strip_template_markers`；`crates/codegen/tools/src/types/template_renderer.rs` — `TemplateRenderer`；`crates/codegen/tools/src/types/template_renderer.rs` — `new`；`crates/codegen/tools/src/types/template_renderer.rs` — `with_system_reminders_enabled`；`crates/codegen/tools/src/types/template_renderer.rs` — `render`；`crates/codegen/tools/src/types/template_renderer.rs` — `param_names`；`crates/codegen/tools/src/types/template_renderer.rs` — `render_schema_descriptions`；`crates/codegen/tools/src/types/template_renderer.rs` — `tool_for_kind`；`crates/codegen/tools/src/types/template_renderer.rs` — `param_for_kind`；`crates/codegen/tools/src/types/template_renderer.rs` — `resolve`；`crates/codegen/tools/src/types/template_renderer.rs` — `resolve_tool_name`；`crates/codegen/tools/src/types/template_renderer.rs` — `render_with_extra`；`crates/codegen/tools/src/types/template_renderer.rs` — `strip_template_markers_removes_interpolations_and_tags`；`crates/codegen/tools/src/types/template_renderer.rs` — `strip_template_markers_keeps_plain_text_and_literal_dollar_brace`；`crates/codegen/tools/src/types/template_renderer.rs` — `make_renderer`；`crates/codegen/tools/src/types/template_renderer.rs` — `render_tool_name`；`crates/codegen/tools/src/types/template_renderer.rs` — `render_tool_name_with_override`；`crates/codegen/tools/src/types/template_renderer.rs` — `render_param_name`；`crates/codegen/tools/src/types/template_renderer.rs` — `param_for_kind_presence_aware`；`crates/codegen/tools/src/types/template_renderer.rs` — `render_schema_descriptions_resolves_param_refs`。

### Requirement: Tools crates/codegen/tools/src/types/tool.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/tool.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols ToolNamespace, ToolKind, as_key, str, Reminder, requires_expr, collect_reminders follow explicit markers serde/json wire or configuration、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、LSP/diagnostic lifecycle; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/tool.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/types/tool.rs` — `ToolNamespace`；`crates/codegen/tools/src/types/tool.rs` — `ToolKind`；`crates/codegen/tools/src/types/tool.rs` — `as_key`；`crates/codegen/tools/src/types/tool.rs` — `str`；`crates/codegen/tools/src/types/tool.rs` — `Reminder`；`crates/codegen/tools/src/types/tool.rs` — `requires_expr`；`crates/codegen/tools/src/types/tool.rs` — `collect_reminders`。

### Requirement: Tools crates/codegen/tools/src/types/tool_index.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/tool_index.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols for, ToolSearchResult, SearchSnapshot, ServerSummary, ToolSearchIndex, search_snapshot, list_server_summaries, ToolIndex, fmt follow explicit markers serde/json wire or configuration、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/tool_index.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/types/tool_index.rs` — `for`；`crates/codegen/tools/src/types/tool_index.rs` — `ToolSearchResult`；`crates/codegen/tools/src/types/tool_index.rs` — `SearchSnapshot`；`crates/codegen/tools/src/types/tool_index.rs` — `ServerSummary`；`crates/codegen/tools/src/types/tool_index.rs` — `ToolSearchIndex`；`crates/codegen/tools/src/types/tool_index.rs` — `search_snapshot`；`crates/codegen/tools/src/types/tool_index.rs` — `list_server_summaries`；`crates/codegen/tools/src/types/tool_index.rs` — `ToolIndex`；`crates/codegen/tools/src/types/tool_index.rs` — `fmt`。

### Requirement: Tools crates/codegen/tools/src/types/tool_io.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/tool_io.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols MCPToolInput, ToolInput, dispatch_target_name, try_into_input_succeeds_for_matching_variant, dispatch_target_name_resolves_meta_dispatch_tools, try_into_input_fails_for_mismatched_variant, try_into_all_input_variants, dynamic_input_holds_arbitrary_json follow explicit markers serde/json wire or configuration、explicit error classification、child process execution、timeout, budget, or rate limit、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/types/tool_io.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/tool_io.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/types/tool_io.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/types/tool_io.rs` — `MCPToolInput`；`crates/codegen/tools/src/types/tool_io.rs` — `ToolInput`；`crates/codegen/tools/src/types/tool_io.rs` — `dispatch_target_name`；`crates/codegen/tools/src/types/tool_io.rs` — `try_into_input_succeeds_for_matching_variant`；`crates/codegen/tools/src/types/tool_io.rs` — `dispatch_target_name_resolves_meta_dispatch_tools`；`crates/codegen/tools/src/types/tool_io.rs` — `try_into_input_fails_for_mismatched_variant`；`crates/codegen/tools/src/types/tool_io.rs` — `try_into_all_input_variants`；`crates/codegen/tools/src/types/tool_io.rs` — `dynamic_input_holds_arbitrary_json`。

### Requirement: Tools crates/codegen/tools/src/types/tool_metadata.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/tool_metadata.rs SHALL implement the tool type, schema, context, and output contract boundary through define typed tool inputs, outputs, resources, requirements, schema validation, and serialization boundaries. Its source symbols implements, ToolMetadata, kind, tool_namespace, description_template, emitted_notifications, str, isolates_batch_preflight, requires_expr, sanitized_description_template, finalized_definition, shared_resources, resolve_cwd, test_ctx, test_ctx_with_call_id, invoking_param_names follow explicit markers serde/json wire or configuration、explicit error classification、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/types/tool_metadata.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/types/tool_metadata.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/types/tool_metadata.rs` — `implements`；`crates/codegen/tools/src/types/tool_metadata.rs` — `ToolMetadata`；`crates/codegen/tools/src/types/tool_metadata.rs` — `kind`；`crates/codegen/tools/src/types/tool_metadata.rs` — `tool_namespace`；`crates/codegen/tools/src/types/tool_metadata.rs` — `description_template`；`crates/codegen/tools/src/types/tool_metadata.rs` — `emitted_notifications`；`crates/codegen/tools/src/types/tool_metadata.rs` — `str`；`crates/codegen/tools/src/types/tool_metadata.rs` — `isolates_batch_preflight`；`crates/codegen/tools/src/types/tool_metadata.rs` — `requires_expr`；`crates/codegen/tools/src/types/tool_metadata.rs` — `sanitized_description_template`；`crates/codegen/tools/src/types/tool_metadata.rs` — `finalized_definition`；`crates/codegen/tools/src/types/tool_metadata.rs` — `shared_resources`；`crates/codegen/tools/src/types/tool_metadata.rs` — `resolve_cwd`；`crates/codegen/tools/src/types/tool_metadata.rs` — `test_ctx`；`crates/codegen/tools/src/types/tool_metadata.rs` — `test_ctx_with_call_id`；`crates/codegen/tools/src/types/tool_metadata.rs` — `invoking_param_names`。

### Requirement: Tools crates/codegen/tools/src/util/binary.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/binary.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols BINARY_EXTENSIONS, SAMPLE_SIZE, NON_PRINTABLE_THRESHOLD, is_binary, detects_known_binary_extensions, pdf_not_in_binary_extensions, pptx_not_in_binary_extensions, allows_text_files, main, empty_content_is_not_binary, detects_null_bytes, null_byte_at_sample_boundary, null_byte_beyond_sample_not_detected, threshold_boundary_not_binary, threshold_boundary_is_binary, tabs_and_newlines_are_printable, large_text_file_is_not_binary, extensions_are_sorted follow explicit markers platform/feature conditional、session, prompt, goal, or subagent context、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/util/binary.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/util/binary.rs` — `BINARY_EXTENSIONS`；`crates/codegen/tools/src/util/binary.rs` — `SAMPLE_SIZE`；`crates/codegen/tools/src/util/binary.rs` — `NON_PRINTABLE_THRESHOLD`；`crates/codegen/tools/src/util/binary.rs` — `is_binary`；`crates/codegen/tools/src/util/binary.rs` — `detects_known_binary_extensions`；`crates/codegen/tools/src/util/binary.rs` — `pdf_not_in_binary_extensions`；`crates/codegen/tools/src/util/binary.rs` — `pptx_not_in_binary_extensions`；`crates/codegen/tools/src/util/binary.rs` — `allows_text_files`；`crates/codegen/tools/src/util/binary.rs` — `main`；`crates/codegen/tools/src/util/binary.rs` — `empty_content_is_not_binary`；`crates/codegen/tools/src/util/binary.rs` — `detects_null_bytes`；`crates/codegen/tools/src/util/binary.rs` — `null_byte_at_sample_boundary`；`crates/codegen/tools/src/util/binary.rs` — `null_byte_beyond_sample_not_detected`；`crates/codegen/tools/src/util/binary.rs` — `threshold_boundary_not_binary`；`crates/codegen/tools/src/util/binary.rs` — `threshold_boundary_is_binary`；`crates/codegen/tools/src/util/binary.rs` — `tabs_and_newlines_are_printable`；`crates/codegen/tools/src/util/binary.rs` — `large_text_file_is_not_binary`；`crates/codegen/tools/src/util/binary.rs` — `extensions_are_sorted`。

### Requirement: Tools crates/codegen/tools/src/util/command_display.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/command_display.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols strip_redundant_session_cd, paths_equal_for_display, is_absolute_shaped_path_token, is_windows_shaped_str, path_segments, trim_wrapping_parens, peel_cd_prefix, take_shell_word, take_path_token, unquote_path_token, cwd, expect_peel, expect_no_peel, matrix_happy_path_unix, matrix_windows_shaped_peel_on_any_host, matrix_no_peel_fail_closed, matrix_adversarial_and_model_noise, paths_equal_slash_and_trailing follow explicit markers child process execution、platform/feature conditional、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/util/command_display.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/util/command_display.rs` — `strip_redundant_session_cd`；`crates/codegen/tools/src/util/command_display.rs` — `paths_equal_for_display`；`crates/codegen/tools/src/util/command_display.rs` — `is_absolute_shaped_path_token`；`crates/codegen/tools/src/util/command_display.rs` — `is_windows_shaped_str`；`crates/codegen/tools/src/util/command_display.rs` — `path_segments`；`crates/codegen/tools/src/util/command_display.rs` — `trim_wrapping_parens`；`crates/codegen/tools/src/util/command_display.rs` — `peel_cd_prefix`；`crates/codegen/tools/src/util/command_display.rs` — `take_shell_word`；`crates/codegen/tools/src/util/command_display.rs` — `take_path_token`；`crates/codegen/tools/src/util/command_display.rs` — `unquote_path_token`；`crates/codegen/tools/src/util/command_display.rs` — `cwd`；`crates/codegen/tools/src/util/command_display.rs` — `expect_peel`；`crates/codegen/tools/src/util/command_display.rs` — `expect_no_peel`；`crates/codegen/tools/src/util/command_display.rs` — `matrix_happy_path_unix`；`crates/codegen/tools/src/util/command_display.rs` — `matrix_windows_shaped_peel_on_any_host`；`crates/codegen/tools/src/util/command_display.rs` — `matrix_no_peel_fail_closed`；`crates/codegen/tools/src/util/command_display.rs` — `matrix_adversarial_and_model_noise`；`crates/codegen/tools/src/util/command_display.rs` — `paths_equal_slash_and_trailing`。

### Requirement: Tools crates/codegen/tools/src/util/grow_home.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/grow_home.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols grow_home follow explicit markers session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/util/grow_home.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/util/grow_home.rs` — `grow_home`。

### Requirement: Tools crates/codegen/tools/src/util/hash.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/hash.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols FNV_OFFSET, FNV_PRIME, fnv1a_32, line_hash, encode_hash, DEFAULT_HASH_LEN, fnv1a_32_empty, fnv1a_32_deterministic, fnv1a_32_different_inputs_differ, line_hash_deterministic, line_hash_whitespace_normalization_indentation, line_hash_whitespace_normalization_trailing, line_hash_whitespace_normalization_internal_collapse, line_hash_preserves_token_boundaries, line_hash_empty_line, line_hash_content_changes_differ, encode_hash_length, encode_hash_lowercase_letters (additional symbols omitted from the title but included in source evidence) follow explicit markers platform/feature conditional、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/util/hash.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/util/hash.rs` — `FNV_OFFSET`；`crates/codegen/tools/src/util/hash.rs` — `FNV_PRIME`；`crates/codegen/tools/src/util/hash.rs` — `fnv1a_32`；`crates/codegen/tools/src/util/hash.rs` — `line_hash`；`crates/codegen/tools/src/util/hash.rs` — `encode_hash`；`crates/codegen/tools/src/util/hash.rs` — `DEFAULT_HASH_LEN`；`crates/codegen/tools/src/util/hash.rs` — `fnv1a_32_empty`；`crates/codegen/tools/src/util/hash.rs` — `fnv1a_32_deterministic`；`crates/codegen/tools/src/util/hash.rs` — `fnv1a_32_different_inputs_differ`；`crates/codegen/tools/src/util/hash.rs` — `line_hash_deterministic`；`crates/codegen/tools/src/util/hash.rs` — `line_hash_whitespace_normalization_indentation`；`crates/codegen/tools/src/util/hash.rs` — `line_hash_whitespace_normalization_trailing`；`crates/codegen/tools/src/util/hash.rs` — `line_hash_whitespace_normalization_internal_collapse`；`crates/codegen/tools/src/util/hash.rs` — `line_hash_preserves_token_boundaries`；`crates/codegen/tools/src/util/hash.rs` — `line_hash_empty_line`；`crates/codegen/tools/src/util/hash.rs` — `line_hash_content_changes_differ`；`crates/codegen/tools/src/util/hash.rs` — `encode_hash_length`；`crates/codegen/tools/src/util/hash.rs` — `encode_hash_lowercase_letters`；`crates/codegen/tools/src/util/hash.rs` — `encode_hash_deterministic`；`crates/codegen/tools/src/util/hash.rs` — `encode_hash_zero_len_panics`；`crates/codegen/tools/src/util/hash.rs` — `encode_hash_five_len_panics`；`crates/codegen/tools/src/util/hash.rs` — `encode_hash_different_hashes_differ`。

### Requirement: Tools crates/codegen/tools/src/util/mcp_truncate.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/mcp_truncate.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols MCP_MAX_OUTPUT_BYTES, ENV_MAX_MCP_OUTPUT_BYTES, ENV_GROW_MAX_MCP_OUTPUT_BYTES, EFFECTIVE_MCP_MAX_OUTPUT_BYTES, set_mcp_max_output_bytes, parse_positive_bytes_env, mcp_max_output_bytes_from_env, mcp_max_output_bytes, LONG_LINE_BYTES, McpDumpKind, classify, extension, str, steer, McpTruncateContext, from_tool_ctx, sanitized_stem, truncate_mcp_text (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/util/mcp_truncate.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/util/mcp_truncate.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/util/mcp_truncate.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/util/mcp_truncate.rs` — `MCP_MAX_OUTPUT_BYTES`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `ENV_MAX_MCP_OUTPUT_BYTES`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `ENV_GROW_MAX_MCP_OUTPUT_BYTES`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `EFFECTIVE_MCP_MAX_OUTPUT_BYTES`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `set_mcp_max_output_bytes`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `parse_positive_bytes_env`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `mcp_max_output_bytes_from_env`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `mcp_max_output_bytes`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `LONG_LINE_BYTES`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `McpDumpKind`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `classify`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `extension`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `str`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `steer`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `McpTruncateContext`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `from_tool_ctx`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `sanitized_stem`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `truncate_mcp_text`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `truncate_tool_output`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `cfg_with_folder`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `with_mcp_limit_lock`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `LOCK`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `host_set_overrides_env_fallback`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `env_parser_rejects_zero_and_junk`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `text_over_limit_truncates_and_dumps_full_payload`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `boundary_exact_limit_untouched_one_over_truncates`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `traversal_in_call_id_cannot_escape_session_dir`；`crates/codegen/tools/src/util/mcp_truncate.rs` — `non_text_variant_passes_through`。

### Requirement: Tools crates/codegen/tools/src/util/mod.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/mod.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols mod follow explicit markers serde/json wire or configuration、explicit error classification、timeout, budget, or rate limit、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、repository/worktree scope、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/util/mod.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/util/mod.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/util/mod.rs` — `mod`。

### Requirement: Tools crates/codegen/tools/src/util/query_tools.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/query_tools.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols QueryTools, str, detect, DETECTED, json_tools, text_tools, edit_tools, wrap, examples_clause, all, examples_clause_formats_lists, tool_sets_membership_and_order, edit_tools_exclude_cut follow explicit markers platform/feature conditional、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/util/query_tools.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/util/query_tools.rs` — `QueryTools`；`crates/codegen/tools/src/util/query_tools.rs` — `str`；`crates/codegen/tools/src/util/query_tools.rs` — `detect`；`crates/codegen/tools/src/util/query_tools.rs` — `DETECTED`；`crates/codegen/tools/src/util/query_tools.rs` — `json_tools`；`crates/codegen/tools/src/util/query_tools.rs` — `text_tools`；`crates/codegen/tools/src/util/query_tools.rs` — `edit_tools`；`crates/codegen/tools/src/util/query_tools.rs` — `wrap`；`crates/codegen/tools/src/util/query_tools.rs` — `examples_clause`；`crates/codegen/tools/src/util/query_tools.rs` — `all`；`crates/codegen/tools/src/util/query_tools.rs` — `examples_clause_formats_lists`；`crates/codegen/tools/src/util/query_tools.rs` — `tool_sets_membership_and_order`；`crates/codegen/tools/src/util/query_tools.rs` — `edit_tools_exclude_cut`。

### Requirement: Tools crates/codegen/tools/src/util/remap.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/remap.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols remap_json_keys, reverse_map, remap_schema_properties, remap_json_keys_basic, remap_json_keys_unmapped_passthrough, remap_json_keys_empty_map, remap_json_keys_non_object_passthrough, reverse_map_basic, remap_schema_properties_basic, remap_schema_properties_empty_map follow explicit markers serde/json wire or configuration、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/util/remap.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/util/remap.rs` — `remap_json_keys`；`crates/codegen/tools/src/util/remap.rs` — `reverse_map`；`crates/codegen/tools/src/util/remap.rs` — `remap_schema_properties`；`crates/codegen/tools/src/util/remap.rs` — `remap_json_keys_basic`；`crates/codegen/tools/src/util/remap.rs` — `remap_json_keys_unmapped_passthrough`；`crates/codegen/tools/src/util/remap.rs` — `remap_json_keys_empty_map`；`crates/codegen/tools/src/util/remap.rs` — `remap_json_keys_non_object_passthrough`；`crates/codegen/tools/src/util/remap.rs` — `reverse_map_basic`；`crates/codegen/tools/src/util/remap.rs` — `remap_schema_properties_basic`；`crates/codegen/tools/src/util/remap.rs` — `remap_schema_properties_empty_map`。

### Requirement: Tools crates/codegen/tools/src/util/ripgrep.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/ripgrep.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols RG_BYTES, resolve_bundled_rg, rg_path, RG_EXEC, rg_available, bundled_rg_is_runnable_without_path_lookup follow explicit markers filesystem or durable persistence、explicit error classification、child process execution、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、repository/worktree scope; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/util/ripgrep.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/util/ripgrep.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/util/ripgrep.rs` — `RG_BYTES`；`crates/codegen/tools/src/util/ripgrep.rs` — `resolve_bundled_rg`；`crates/codegen/tools/src/util/ripgrep.rs` — `rg_path`；`crates/codegen/tools/src/util/ripgrep.rs` — `RG_EXEC`；`crates/codegen/tools/src/util/ripgrep.rs` — `rg_available`；`crates/codegen/tools/src/util/ripgrep.rs` — `bundled_rg_is_runnable_without_path_lookup`。

### Requirement: Tools crates/codegen/tools/src/util/serde_base64.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/serde_base64.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols serialize, deserialize, Wrapper, vec_round_trips_binary_bytes, vec_serializes_to_string_not_array, vec_rejects_integer_array, vec_rejects_malformed_base64, vec_rejects_unexpected_type, vec_empty_round_trips, vec_round_trips_large_binary_buffer follow explicit markers serde/json wire or configuration、explicit error classification、platform/feature conditional; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/util/serde_base64.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/util/serde_base64.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/util/serde_base64.rs` — `serialize`；`crates/codegen/tools/src/util/serde_base64.rs` — `deserialize`；`crates/codegen/tools/src/util/serde_base64.rs` — `Wrapper`；`crates/codegen/tools/src/util/serde_base64.rs` — `vec_round_trips_binary_bytes`；`crates/codegen/tools/src/util/serde_base64.rs` — `vec_serializes_to_string_not_array`；`crates/codegen/tools/src/util/serde_base64.rs` — `vec_rejects_integer_array`；`crates/codegen/tools/src/util/serde_base64.rs` — `vec_rejects_malformed_base64`；`crates/codegen/tools/src/util/serde_base64.rs` — `vec_rejects_unexpected_type`；`crates/codegen/tools/src/util/serde_base64.rs` — `vec_empty_round_trips`；`crates/codegen/tools/src/util/serde_base64.rs` — `vec_round_trips_large_binary_buffer`。

### Requirement: Tools crates/codegen/tools/src/util/truncate.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/truncate.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols DEFAULT_SOFT_WRAP_WIDTH, PREVIEW_SIZE, TRUNCATION_MARKER, truncate_line, soft_wrap_line, truncate_str, truncate_with_preview, truncate_str_with_marker, floor_char_boundary, ceil_char_boundary, used, estimate_tokens, estimate_chars, format_bytes, soft_wrap_lines, truncate_front_and_back, truncate_middle, MARKER (additional symbols omitted from the title but included in source evidence) follow explicit markers platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、image/PDF/media processing; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/util/truncate.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/util/truncate.rs` — `DEFAULT_SOFT_WRAP_WIDTH`；`crates/codegen/tools/src/util/truncate.rs` — `PREVIEW_SIZE`；`crates/codegen/tools/src/util/truncate.rs` — `TRUNCATION_MARKER`；`crates/codegen/tools/src/util/truncate.rs` — `truncate_line`；`crates/codegen/tools/src/util/truncate.rs` — `soft_wrap_line`；`crates/codegen/tools/src/util/truncate.rs` — `truncate_str`；`crates/codegen/tools/src/util/truncate.rs` — `truncate_with_preview`；`crates/codegen/tools/src/util/truncate.rs` — `truncate_str_with_marker`；`crates/codegen/tools/src/util/truncate.rs` — `floor_char_boundary`；`crates/codegen/tools/src/util/truncate.rs` — `ceil_char_boundary`；`crates/codegen/tools/src/util/truncate.rs` — `used`；`crates/codegen/tools/src/util/truncate.rs` — `estimate_tokens`；`crates/codegen/tools/src/util/truncate.rs` — `estimate_chars`；`crates/codegen/tools/src/util/truncate.rs` — `format_bytes`；`crates/codegen/tools/src/util/truncate.rs` — `soft_wrap_lines`；`crates/codegen/tools/src/util/truncate.rs` — `truncate_front_and_back`；`crates/codegen/tools/src/util/truncate.rs` — `truncate_middle`；`crates/codegen/tools/src/util/truncate.rs` — `MARKER`；`crates/codegen/tools/src/util/truncate.rs` — `MARKER_LEN`；`crates/codegen/tools/src/util/truncate.rs` — `truncate_lines_to_char_budget`；`crates/codegen/tools/src/util/truncate.rs` — `estimate_tokens_empty`；`crates/codegen/tools/src/util/truncate.rs` — `estimate_tokens_four_bytes`；`crates/codegen/tools/src/util/truncate.rs` — `estimate_tokens_rounds_down`；`crates/codegen/tools/src/util/truncate.rs` — `estimate_tokens_large`；`crates/codegen/tools/src/util/truncate.rs` — `truncate_short_line_borrowed`；`crates/codegen/tools/src/util/truncate.rs` — `truncate_exact_limit_not_truncated`；`crates/codegen/tools/src/util/truncate.rs` — `truncate_over_limit`；`crates/codegen/tools/src/util/truncate.rs` — `truncate_utf8_safe`。

### Requirement: Tools crates/codegen/tools/src/util/unicode_confusables.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/unicode_confusables.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols CONFUSABLE_MAP, ConfusableHit, str, lookup, has_confusables, normalize_confusables, detect_confusables, build_offset_map, normalize_left_double_quote, normalize_right_double_quote, normalize_left_single_quote, normalize_right_single_quote, normalize_em_dash, normalize_en_dash, normalize_ellipsis, normalize_nbsp, normalize_pure_ascii_is_identity, normalize_empty_string (additional symbols omitted from the title but included in source evidence) follow explicit markers platform/feature conditional、session, prompt, goal, or subagent context、scheduler generation/journal state; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/util/unicode_confusables.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/util/unicode_confusables.rs` — `CONFUSABLE_MAP`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `ConfusableHit`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `str`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `lookup`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `has_confusables`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `normalize_confusables`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `detect_confusables`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `build_offset_map`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `normalize_left_double_quote`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `normalize_right_double_quote`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `normalize_left_single_quote`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `normalize_right_single_quote`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `normalize_em_dash`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `normalize_en_dash`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `normalize_ellipsis`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `normalize_nbsp`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `normalize_pure_ascii_is_identity`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `normalize_empty_string`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `normalize_preserves_non_confusable_unicode`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `has_confusables_false_for_ascii`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `has_confusables_false_for_non_confusable_unicode`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `has_confusables_true_for_smart_quotes`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `has_confusables_true_for_nbsp`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `has_confusables_true_for_em_dash`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `detect_returns_empty_for_ascii`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `detect_single_smart_quote`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `detect_confusables_tracks_line_numbers`；`crates/codegen/tools/src/util/unicode_confusables.rs` — `detect_consecutive_confusables`。
