# tool-runtime 逐包核查

包路径：`crates/common/tool-runtime`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；已阅读 10 个 src 模块及 8 个集成测试文件；已执行 cargo test --locked -p tool-runtime：114 项单元/集成测试和 2 项 doctest 通过，1 项文档示例忽略。

## 模块与开关

- `crates/common/tool-runtime/Cargo.toml`
- `crates/common/tool-runtime/src/context.rs`
- `crates/common/tool-runtime/src/dispatch.rs`
- `crates/common/tool-runtime/src/error.rs`
- `crates/common/tool-runtime/src/lib.rs`
- `crates/common/tool-runtime/src/notification.rs`
- `crates/common/tool-runtime/src/registry.rs`
- `crates/common/tool-runtime/src/render.rs`
- `crates/common/tool-runtime/src/search.rs`
- `crates/common/tool-runtime/src/streaming.rs`
- `crates/common/tool-runtime/src/tool.rs`
- `crates/common/tool-runtime/tests/context_extensions.rs`
- `crates/common/tool-runtime/tests/notification_serde.rs`
- `crates/common/tool-runtime/tests/search.rs`
- `crates/common/tool-runtime/tests/should_list.rs`
- `crates/common/tool-runtime/tests/tool_blocking.rs`
- `crates/common/tool-runtime/tests/tool_dyn.rs`
- `crates/common/tool-runtime/tests/tool_streaming.rs`
- `crates/common/tool-runtime/tests/trait_object_safety.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Typed invocation context](../specs/tool-runtime/spec.md#requirement-typed-invocation-context)：TypedExtensions SHALL 按 Rust TypeId 保存 Arc 值，同类型插入覆盖，读取和删除返回可选 Arc；clone 复制映射并共享已有值，merge_defaults 只填补缺失类型。ToolCallContext 默认生成 UUID v7，显式构造保留 call_id。
- [Typed tool execution stream](../specs/tool-runtime/spec.md#requirement-typed-tool-execution-stream)：Tool SHALL 以 execute 为入口，默认 await run 后返回一个 Terminal；未实现两者时返回 NotImplemented。terminal_only 构造单终值流，with_progress 先排空 progress 再 await terminal future。
- [JSON tool erasure](../specs/tool-runtime/spec.md#requirement-json-tool-erasure)：ToolDyn blanket 与 ErasedTool SHALL 反序列化 Args、调用 typed execute、透传 Progress 与错误终值；成功输出打包 tool_id、JSON value、非空自定义或自动提取的 model_output 及可选 chat_completion_output。
- [Terminal dispatch adapter](../specs/tool-runtime/spec.md#requirement-terminal-dispatch-adapter)：ToolDispatch::call_terminal SHALL 丢弃 Progress 并返回遇到的第一个 Terminal；流结束且没有 Terminal 时返回 Custom 错误 stream_no_terminal。
- [Local registration and aliases](../specs/tool-runtime/spec.md#requirement-local-registration-and-aliases)：LocalRegistry SHALL 共享有序工具表和 extractor 表，支持 typed、Arc、dynamic 注册、查找、别名和注销；同 ID 注册返回被替换 handle，list_tools 使用 should_list 过滤并按上下文生成 description。
- [Tool family variant interface](../specs/tool-runtime/spec.md#requirement-tool-family-variant-interface)：ToolFamily SHALL 提供同一 ID 的 Default 或不透明命名 Variant 查询与枚举接口，default_variant_name 默认 None；具体路由由 family 实现。
- [Model content extraction precedence](../specs/tool-runtime/spec.md#requirement-model-content-extraction-precedence)：extract_content_blocks SHALL 按单 block、含 block 形状的数组、ToolRunResult 的 prompt_text、MCP content、混合对象、普通文本的顺序提取；字符串不加 JSON 引号，其他普通值使用 JSON 文本。
- [Rich progress and client frames](../specs/tool-runtime/spec.md#requirement-rich-progress-and-client-frames)：ToolProgress SHALL 以 kind=text/content/custom 序列化，Custom 用独立 subkind 和任意 JSON payload；ContentBlock 以 type 区分 text/image/resource 并拒绝未知字段。
- [Monotonic progress chunking](../specs/tool-runtime/spec.md#requirement-monotonic-progress-chunking)：stream_chunk SHALL 以 total 与 last_total 的差值从 tail 取新字节，默认 cap 16 KiB，可由 StreamingSpec 覆盖；按实际消费推进游标，丢失的上游中间字节用 gap 标记，truncated 原样透传。
- [Typed error representation](../specs/tool-runtime/spec.md#requirement-typed-error-representation)：ToolError SHALL 区分未实现、参数、未找到、权限、认证、超时、取消、限流、服务不可用、网络、执行、渲染限制、终端与 Custom 错误，保存可选结构 details 和不参与序列化的 source。
- [Best effort notification channel](../specs/tool-runtime/spec.md#requirement-best-effort-notification-channel)：ToolNotificationHandle SHALL 使用 futures 无界 MPSC，支持 channel/new/from_sender、克隆、noop 及每个通知 variant 的 send helper；发送为 best effort，接收端关闭时静默丢弃。
- [Notification payload and task snapshots](../specs/tool-runtime/spec.md#requirement-notification-payload-and-task-snapshots)：通知 payload SHALL 保留 Bash 调用 ID、命令、原始输出字节、总字节、截断和 cwd，按事件额外携带退出/信号、超时、后台 task/output_file 或启动失败信息；FileWritten 携带写入前后内容及新建标记。
- [Search snapshot interface](../specs/tool-runtime/spec.md#requirement-search-snapshot-interface)：ToolSearchIndex SHALL 提供 Send+Sync 的 search_snapshot(query,limit) 和 server summaries 接口，结果携带工具/服务名、描述、分数、参数顺序、完整 input_schema，以及同一快照的 hidden 数与 ready 标记。

## 边界

- 原 context 的映射不受删除影响；现有同类型值不被 defaults 覆盖。
- 得到 None，本层不推导进程默认目录或执行取消；WorkspaceViewerContext 的 stream_tool_progress 缺省 false。
- 仍输出一个 Terminal；手写 ToolStream 的唯一终值约束由实现者遵守，类型别名不自动校验。
- 分别为 true 和 false；description 接收与 should_list 相同的 ListToolsContext。
- 返回 InvalidArguments 终值，不调用 typed execute。
- ToolDyn 返回带 source 的 Execution；ErasedTool 返回 Custom，code 为 output_encoding。
- 自动提取模型内容，chat_completion_output 为 None，可通过 with_chat_completion_output 设置。
- call_terminal 返回 stream_no_terminal；它不校验第一个 Terminal 之后是否还有重复终值。
- alias 保存当前 target handle，并在存在时复制 extractor；target 缺失则返回 false，后续替换 target 不自动重定向 alias。
- 注销同时移除该 ID 的 handle 和 extractor；extractor_for 类型解析失败返回 None，自定义空 blocks 时自动提取。
- 两者不同；接口不自动添加 fallback 或构造缺失 variant。
- 先输出 structuredContent JSON 文本，再按原序输出 content 元素；非 block 元素转文本。
- 模型只看到 prompt_text。
- 先输出剩余字段 JSON，再输出提取的 blocks；混合数组保留在剩余字段，避免丢失普通数据。
- 返回至少一个文本 block；已知 type 的畸形对象不能绕过 ContentBlock 的字段校验。
- image 保存 mime_type、base64 data 和可选 media_id/filename/path/metadata；resource 保存 uri 与可选 mime_type/text，本层不读取资源或验证 base64。
- 保留 result 与 stream_error，结果含 message/tag/card/code execution 和 flattened extra；sender 的字符串默认空，并不强制 assistant，执行结果缺省空 stdout/stderr、exit_code=0、command_timed_out=false。
- 返回 None 且不推进游标。
- 从存活 tail 输出，并将已丢失部分计入推进量，gap=true；单帧未消费部分可在后续调用继续输出。
- 先输出完整前缀并保留未消费字节；cut 为零且还有字节时兜底至少取至多四字节起步，可能超出 cap，采用 from_utf8_lossy。仅有未完成字符时可能消费为替换字符，当前实现不保证跨 tick 无损。
- delta 和 total_bytes 必需，gap/truncated 缺省 false，未知字段拒绝。
- Display 只输出 detail，Debug 可含 source chain；kind serde 使用 Rust variant 名，as_str 提供 snake_case 名，两者不可混用。
- custom 将 code 放入 details；not_found/timeout/cancelled/execution/terminal_error 附加 tool_id，serde_json 错误转 InvalidArguments。
- type 使用 PascalCase，覆盖 BashOutputChunk/Complete/Timeout/Backgrounded/Failed、FileWritten、TaskCompleted、UserQuestionAsked、LSP Starting/Ready/Crashed/Retrying/Failed、ScheduledTask Fired/Removed/Created 和 MonitorEvent 共 17 类。
- 它没有对应 ToolNotification variant 或 send helper，不能据此声称文件读取会发出通知。
- 用 end_time 或当前时间减 start_time，逆向时间返回 0；kind 缺省 Bash，另支持 Monitor，display_command 可选且区别于实际 command。
- 保留任意 questions_json、重试计数与 backoff、调度 prompt/human_schedule/可选 next_fire_at 或原始文本/XML event_text；本层只传输，不执行调度、进程重启或 XML 包装。
- output_lossy 使用替换字符；was_signaled 仅判断 signal 是否存在，不额外校验 exit_code 的一致性。
- ToolIndex 克隆共享 Arc；tool_count 返回 tool_names 长度，搜索排序、字母序、限额和快照一致性由具体后端实现，本包不提供搜索算法或自动排序。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 已复现的实现边界

使用本分支 cargo test 生成的 tool_runtime/tool_protocol rlib 编译独立调用程序，调用真实 stream_chunk（未复制算法、未修改 crate）：

```rust
let spec = StreamingSpec { subkind: "probe".into(), max_delta_bytes: None };
let mut consumed = 0;
stream_chunk(&spec, &[0xc3], 1, &mut consumed, false);
stream_chunk(&spec, &[0xc3, 0xa9], 2, &mut consumed, false);
```

两次 delta 分别为 `�` 和 `�`，游标先为 1 后为 2；拼接并非原字符 `é`。两次 gap/truncated 均为 false。源码承诺的无损边界与实际不符；现有测试通过不能排除此问题。债务已登记，不在文档迁移中修复。
