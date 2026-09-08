## ADDED Requirements

### Requirement: Shared threshold boundaries
压缩阈值辅助函数 SHALL 使用饱和整数运算和大于等于比较；context_window 为零时返回 false，headroom 在缩放阈值中扣除并饱和到零。

#### Scenario: 精确阈值
- **WHEN** window=1000、threshold=85
- **THEN** used=850 触发，849 不触发。

#### Scenario: headroom 超过阈值
- **WHEN** 非零窗口且 headroom 大于阈值所对应 token
- **THEN** 阈值饱和到零，used=0 也触发；零窗口仍不触发。

证据：`crates/codegen/token-estimation/src/lib.rs` — `exceeds_threshold_with_headroom`。

### Requirement: Pure tool result text pruning
prune_tool_result_content SHALL 将 token 预算饱和乘 4 转为字节上限，已满足预算则返回 None；超限时给头部一半、尾部四分之一，剩余给 marker，各切点保持 UTF-8 字符边界。

#### Scenario: marker 超限
- **WHEN** marker 大于剩余空间
- **THEN** 截短 marker，不侵占头尾份额，结果字节数不超预算；marker 较短时不重新分配空余预算。

#### Scenario: 零预算
- **WHEN** 非空文本使用 0 token 预算
- **THEN** 返回 Some 空字符串；空文本返回 None。

证据：`crates/common/compaction/src/prune.rs` — `prune_tool_result_content`。

### Requirement: Oldest first pruning plan
plan_tool_result_pruning SHALL 用 host ItemTokenCounter 一次计算各项 token，按升序选择超出单项预算的 tool result，累计保守节省估计直至总量不大于目标；合计和减法使用饱和计算。

#### Scenario: 跳过与停止
- **WHEN** 存在用户消息、预算内工具结果或已达目标
- **THEN** 不裁剪非工具结果和预算内项，达到目标后不再增加候选。

#### Scenario: 无候选与零预算
- **WHEN** 输入空、任一预算为零或没有可裁剪项
- **THEN** 返回空计划；即使返回非空计划也不保证可达到目标，不执行实际内容替换或持久化。

证据：`crates/common/compaction/src/prune.rs` — `plan_tool_result_pruning`；`crates/common/compaction/src/token.rs` — `ItemTokenCounter`；`crates/common/compaction/src/prune.rs` — `PrunePlan`。

### Requirement: Range summary host boundary
generate_summary SHALL 对 host 选择的非空 turns 构造空 system 与摘要 user prompt，按 SummaryConfig 调用 sampler，返回未经清洗的获胜 summary 与 attempts；空 turns 返回 NothingToCompact。

#### Scenario: 默认配置
- **WHEN** 使用 SummaryConfig::default
- **THEN** 总尝试数 3、重试间隔 3 秒、每次传给 sampler 的 timeout 120 秒；自动压缩阈值共享常量为 80%，触发、选区、缩小输入、持久化与提交由 host 负责。

#### Scenario: 超时职责
- **WHEN** 调用 CompactionSampler
- **THEN** timeout 参数传给实现，本循环不额外套 tokio timeout；LlmCompactionOutput 只保存 response 字符串，不能由注释推导出 thinking 输出。

证据：`crates/common/compaction/src/code_compaction/compact.rs` — `generate_summary`；`crates/common/compaction/src/code_compaction/config.rs` — `SummaryConfig`；`crates/common/compaction/src/sampler.rs` — `CompactionSampler`。

### Requirement: Bounded summary retry outcomes
sample_summary_with_retries SHALL 至少尝试一次，正常非空且清洗后不少于 500 字符的摘要立即成功；空白或退化摘要按瞬时失败重试，只有确实继续时等待 retry_delay。

#### Scenario: 确定性失败
- **WHEN** sampler 返回 Deterministic 或错误文本匹配上下文超限
- **THEN** 立即返回 Failure 并标记 deterministic，超限另外标记 context_overflow，不重复相同输入。

#### Scenario: 重试耗尽
- **WHEN** 最后一次返回空/退化内容或 sampler 错误
- **THEN** 前者返回 Empty，后者返回 Failure，结果携带尝试数；Timeout/Transient/EmptyResponse 错误默认非确定性，但文本超限规则仍可覆盖。

证据：`crates/common/compaction/src/code_compaction/sample.rs` — `sample_summary_with_retries`；`crates/common/compaction/src/code_compaction/summary.rs` — `is_degenerate_summary`；`crates/common/compaction/src/sampler.rs` — `CompactionSampleError`。

### Requirement: Compaction error classification rules
HTTP 分类 SHALL 将除 408/429 外的 4xx 视为确定性，任何状态的已知 context length 文本也确定性，其余 transient；stream error 额外识别 code 或 message 中的 invalid_request_error。

#### Scenario: 字符串识别边界
- **WHEN** 消息涉及 prompt/context 超限
- **THEN** 匹配 ASCII 小写化后的 too long for this model、prompt is too long、maximum prompt length、maximum context length、context_length_exceeded，或同时有 current message 与 exceeds budget；普通附件或索引预算错误不自动认定上下文超限。

#### Scenario: 流错误未知值
- **WHEN** code 无法解析 u16 且没有确定性标记
- **THEN** 默认 transient；invalid_request_error 标记匹配区分大小写，数值 408/429 同样可重试。

证据：`crates/common/compaction/src/code_compaction/failure.rs` — `classify_http_status`；`crates/common/compaction/src/code_compaction/failure.rs` — `classify_stream_event_error`；`crates/common/compaction/src/code_compaction/failure.rs` — `is_context_length_error`。

### Requirement: Summary observer emission
摘要循环 SHALL 为每个实际 attempt 发出 Success/EmptyResponse/Degenerate/Failure 回调，携带 1 起始序号、原始摘要或错误与是否将重试。generate_summary 最终成功或失败再发对应终结回调。

#### Scenario: 终结回调边界
- **WHEN** 摘要成功、失败或输入为空
- **THEN** 成功报告原始摘要字符数、次数和耗时；失败报告次数；空 turns 提前返回，不产生 attempt 或终结回调。observer 默认方法和 () 实现不执行副作用。

证据：`crates/common/compaction/src/code_compaction/observer.rs` — `SummaryObserver`；`crates/common/compaction/src/code_compaction/compact.rs` — `generate_summary`；`crates/common/compaction/src/code_compaction/sample.rs` — `sample_summary_with_retries`。

### Requirement: Canonical summary prompt content
build_summary_prompt SHALL 加载仓库模板并替换 user_context_section；Some context 原样插入上下文段，None 不生成该段。模板要求单一 summary 块、九个编号章节和继续工作的关键信息。

#### Scenario: 摘要主题
- **WHEN** 构建默认提示
- **THEN** 包含请求意图、技术概念、文件代码、错误修复、问题处理、用户消息、待办、当前工作与下一步，要求不输出独立 analysis 和不调用工具；这些是发送给模型的提示，不是结构化输出校验器。

证据：`crates/common/compaction/src/code_compaction/prompt.rs` — `build_summary_prompt`；`crates/common/compaction/src/code_compaction/templates/summary_prompt.txt` — `Primary Request and Intent`。

### Requirement: Summary cleanup and continuation carrier
format_compact_summary SHALL 移除位于 summary 前或紧随开头的 analysis 草稿块，将有效外层 summary 转为 Summary 标题，保留外部文本，并压缩连续三个以上换行及首尾空白。

#### Scenario: 正文中的标签
- **WHEN** 摘要编号章节中引用控制标签
- **THEN** 不把正文中 analysis 当作开头草稿删除；剩余 analysis/summary/summary_request 开闭标签在小于号后插入零宽空格，避免保留原控制标签。

#### Scenario: 不完整结构
- **WHEN** analysis 未闭合或 summary 开闭顺序错误
- **THEN** 前置 analysis 丢至下一 summary 或末尾；无有效 summary 配对不强制截正文，剩余标签中和。内层非编号草稿可按最后 analysis close 去掉前缀，编号章节跳过该剥离。

#### Scenario: carrier
- **WHEN** 调用 format_compact_summary_content 或 wrap_user_query
- **THEN** 前者清洗后加继续会话前言，后者仅原样包 user_query；退化判定按清洗结果 Unicode 字符数少于 500，不按 UTF-8 字节数。

证据：`crates/common/compaction/src/code_compaction/summary.rs` — `format_compact_summary`；`crates/common/compaction/src/code_compaction/summary.rs` — `format_compact_summary_content`；`crates/common/compaction/src/code_compaction/summary.rs` — `wrap_user_query`。

### Requirement: Post compaction active state reminders
提醒格式化 SHALL 按后台任务、TODO、子 Agent 顺序组合借用的 host 状态，跳过空段；TODO 只展开 Pending/InProgress，Completed/Cancelled 合计放尾注，无行动项时省略整个 TODO 段。

#### Scenario: 子 Agent 工具名缺失
- **WHEN** format_active_agent_sections 没有 subagent_tools
- **THEN** 仅省略子 Agent 段；已知工具名、任务 ID、描述和状态直接插值，不重新查询存活状态或校验名称。

#### Scenario: 包装与附加
- **WHEN** wrap_system_reminder 或 append_reminder_block 收到空白段
- **THEN** 忽略纯空白，非空段用空行连接并包 system-reminder；附加 reminder=None/空白时 summary 不变，其他情况空行追加。

#### Scenario: 状态空判定
- **WHEN** ActiveAgentReminderState 只有已完成 TODO 或子 Agent 无可用工具名
- **THEN** 只有已完成 TODO 时 is_empty=true；存在子 Agent 时 is_empty=false，即使工具名缺失使渲染结果为空。

证据：`crates/common/compaction/src/reminder.rs` — `format_active_agent_sections`；`crates/common/compaction/src/reminder.rs` — `ActiveAgentReminderState`；`crates/common/compaction/src/reminder.rs` — `append_reminder_block`。


### Requirement: Shell compaction threshold tier resolution

自动压缩阈值解析 SHALL 按环境、精确model_id的本地模型、session、本次传入ModelInfo、remote全局、共享默认80的顺序返回。环境按未trim的i64解析，仅0..100有效；其他Option<u8>层在此不校验0..100，允许101..255原样返回。测试名default_85仍比较共享常量，不能认定默认85。此函数只选阈值，不触发压缩。

#### Scenario: Out of range typed local value
- **WHEN** 环境无有效值，本地模型层为Some(150)
- **THEN** 解析器返回150，不在此clamp。

证据：`crates/codegen/shell/src/util/config/resolve/compaction.rs` — `pub fn resolve_auto_compact_threshold_percent`；`crates/codegen/shell/src/util/config/resolve/compaction.rs` — `pub fn resolve_auto_compact_threshold_percent_from_tiers`。

### Requirement: Shell compaction pre prune configuration resolution

pre_prune SHALL 按trim后bool环境值、本地、remote、默认true解析，数字1不是有效bool。token budget按trim后u64环境、本地、remote顺序跳过零，均缺失返回None；无效或零环境不阻断下一层。该函数不计算注释所述context 5%预算，派生由消费方负责。

#### Scenario: Zero env with valid config
- **WHEN** env为0，本地budget为100
- **THEN** 返回Some(100)，不直接采用默认派生预算。

证据：`crates/codegen/shell/src/util/config/resolve/compaction.rs` — `pub fn resolve_compaction_pre_prune_from`；`crates/codegen/shell/src/util/config/resolve/compaction.rs` — `pub fn resolve_compaction_pre_prune_token_budget_from`；`crates/codegen/shell/src/util/config/resolve/compaction.rs` — `fn budget_zero_and_garbage_fall_through`。

### Requirement: Shell compaction wall clock budget selection

压缩wall clock预算解析 SHALL 按trim后u64环境GROW_COMPACTION_WALL_CLOCK_SECS、remote全局、默认300秒返回；0保留用于禁用，1..119仅warn不clamp，120及以上不触发该低预算warning。此resolver不执行超时或取消。现有default_global_disable_and_no_clamp测试依赖环境未设置，不自行隔离。

#### Scenario: Small remote budget
- **WHEN** 环境无有效值且remote为5
- **THEN** 返回5并warning，不替换为300或最低120。

证据：`crates/codegen/shell/src/util/config/resolve/compaction.rs` — `pub fn resolve_compaction_wall_clock_budget_secs`；`crates/codegen/shell/src/util/config/resolve/compaction.rs` — `fn default_global_disable_and_no_clamp`。
### Requirement: Pager confirmed context usage and model-window refresh

Context refresh SHALL apply used tokens against the current model context window or zero. The confirmed path additionally records used for pending compaction messaging. It does not clamp used to total or distinguish unknown from a real zero window.

#### Scenario: Known window
- **WHEN** the model exposes its context window
- **THEN** used and total refresh the view.

#### Scenario: Confirmed
- **WHEN** accepted meta.totalTokens is processed
- **THEN** used is also recorded on the session.

#### Scenario: Unknown window
- **WHEN** no window resolves
- **THEN** total zero is applied.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `refresh_context_used`、`confirm_context_used`。
### Requirement: Pager compaction lifecycle prompt hold feedback and deferred completion

Compaction start SHALL hold the first in-flight prompt, clear active prompt and show AutoCompacting progress. Async completion appends an informational notice via early return. Synchronous completion clears activity/feedback/held prompt and either appends manual/replay completion or defers live automatic completion. Failure clears activity/feedback and appends failure without clearing held prompt; cancellation clears all three. ImageDropped/Projected join notes into notices.

#### Scenario: Start
- **WHEN** compaction begins
- **THEN** prompt is held and progress becomes live.

#### Scenario: Synchronous completion
- **WHEN** compaction completes
- **THEN** state clears and completion is immediate for manual/replay, deferred otherwise.

#### Scenario: Failure
- **WHEN** compaction fails
- **THEN** failure renders while held prompt remains unchanged.

#### Scenario: Cancel
- **WHEN** compaction cancels
- **THEN** activity, feedback and held prompt clear.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `apply_session_event`。

### Requirement: Pager recent compaction failure trailing-block scan

Detection SHALL scan backward through trailing SessionEvent and Notice blocks, returning true for CompactionFailed, skipping notices and other session events, and stopping at the first other block.

#### Scenario: Recent
- **WHEN** failure exists in the trailing session/notice run
- **THEN** true is returned.

#### Scenario: Boundary
- **WHEN** ordinary content appears before reaching failure
- **THEN** scanning stops with false.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `scrollback_has_recent_compaction_failed`。

### Requirement: Pager compaction prompt history and recap effects

Explicit compaction SHALL call grow/compact_conversation with session and user context and deserialize its status into CompactComplete while preserving foreground tracking. Prompt-history lookup SHALL pass cwd and optional session_id and accept prompts either under result.prompts or top-level prompts; transport or malformed response SHALL yield an empty history. Recap SHALL call grow/recap with the auto flag and report any ACP failure as an optional error without discarding session correlation. This file does not prove the compaction algorithm, token reduction, prompt-history persistence, recap contents or UI application of stale results.

#### Scenario: Compact response invalid
- **WHEN** the compact response JSON or status cannot deserialize
- **THEN** CompactComplete carries an internal ACP error describing the invalid response.

#### Scenario: History unavailable
- **WHEN** history transport fails or prompts are missing/malformed
- **THEN** PromptHistoryLoaded contains an empty vector.

#### Scenario: Recap rejected
- **WHEN** grow/recap fails
- **THEN** RecapRequested preserves session and auto and includes a request-failed message.

证据：`crates/codegen/pager/src/app/root/effects/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/compaction.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/compaction.rs SHALL 维护 session actor lifecycle and notifications 的入口 COMPACTION_RETAIN_PERCENT, MIN_COMPACTION_SOURCE_TOKENS, CompactionGeneration, PreparedCompaction, GeneratedCompaction, BackgroundCompaction, drop, pre_compact_threshold, compaction_write_error, str, compaction_range_is_current, sampling_error_compaction_window, background_tasks_owned_by, append_system_reminder_section, format_scheduled_tasks_compaction_reminder, context_recall_hint, AutoCompactTriggerInfo, auto_compact_trigger (plus 58 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** COMPACTION_RETAIN_PERCENT, MIN_COMPACTION_SOURCE_TOKENS, CompactionGeneration, PreparedCompaction, GeneratedCompaction, BackgroundCompaction, drop, pre_compact_threshold, compaction_write_error, str, compaction_range_is_current, sampling_error_compaction_window, background_tasks_owned_by, append_system_reminder_section, format_scheduled_tasks_compaction_reminder, context_recall_hint, AutoCompactTriggerInfo, auto_compact_trigger (plus 58 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** COMPACTION_RETAIN_PERCENT, MIN_COMPACTION_SOURCE_TOKENS, CompactionGeneration, PreparedCompaction, GeneratedCompaction, BackgroundCompaction, drop, pre_compact_threshold, compaction_write_error, str, compaction_range_is_current, sampling_error_compaction_window, background_tasks_owned_by, append_system_reminder_section, format_scheduled_tasks_compaction_reminder, context_recall_hint, AutoCompactTriggerInfo, auto_compact_trigger (plus 58 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** COMPACTION_RETAIN_PERCENT, MIN_COMPACTION_SOURCE_TOKENS, CompactionGeneration, PreparedCompaction, GeneratedCompaction, BackgroundCompaction, drop, pre_compact_threshold, compaction_write_error, str, compaction_range_is_current, sampling_error_compaction_window, background_tasks_owned_by, append_system_reminder_section, format_scheduled_tasks_compaction_reminder, context_recall_hint, AutoCompactTriggerInfo, auto_compact_trigger (plus 58 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

证据：`crates/codegen/shell/src/session/actor/compaction.rs`。

### Requirement: Shell crates/codegen/shell/src/session/helpers/summary_compaction.rs session timeline and state model contract

crates/codegen/shell/src/session/helpers/summary_compaction.rs SHALL 维护 session timeline and state model 的入口 ShellCompactionSampler, new, take_last_success, take_image_input_unsupported, take_infrastructure_error, sideband_error, Item, sample_compaction, compact_failure_to_sample_error, sideband_error_to_sample_error, sideband_budget_rejection_drives_the_compaction_input_ladder, acp_error_message, SummaryDiagnostic, ObserverState, ShellSummaryObserver, attempt_count, degenerate_seen, last_error_message (plus 2 additional private symbols)。实现显示该边界包含 explicit error/result paths、async task lifecycle and cancellation、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** ShellCompactionSampler, new, take_last_success, take_image_input_unsupported, take_infrastructure_error, sideband_error, Item, sample_compaction, compact_failure_to_sample_error, sideband_error_to_sample_error, sideband_budget_rejection_drives_the_compaction_input_ladder, acp_error_message, SummaryDiagnostic, ObserverState, ShellSummaryObserver, attempt_count, degenerate_seen, last_error_message (plus 2 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** ShellCompactionSampler, new, take_last_success, take_image_input_unsupported, take_infrastructure_error, sideband_error, Item, sample_compaction, compact_failure_to_sample_error, sideband_error_to_sample_error, sideband_budget_rejection_drives_the_compaction_input_ladder, acp_error_message, SummaryDiagnostic, ObserverState, ShellSummaryObserver, attempt_count, degenerate_seen, last_error_message (plus 2 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/helpers/summary_compaction.rs`。
### Requirement: Pager task-result test: compact_rpc_acknowledgement_does_not_append_another_completion
Compact RPC acknowledgement SHALL not append a second completion row, regardless of whether the request tracked the foreground.

#### Scenario: Compact acknowledgement
- **WHEN** CompactComplete returns Completed
- **THEN** no additional scrollback completion is emitted.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `compact_rpc_acknowledgement_does_not_append_another_completion`。

### Requirement: Pager task-result test: compact_cancel_rpc_does_not_echo_the_backend_terminal
When backend auto-compaction cancellation has already published its terminal notice, the CompactComplete error SHALL clear live state without echoing another terminal row.

#### Scenario: Compact cancellation
- **WHEN** a cancellation notification precedes the RPC terminal error
- **THEN** exactly the backend terminal entry remains and live status is cleared.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `compact_cancel_rpc_does_not_echo_the_backend_terminal`。
### Requirement: Pager agent input test: jump_picker_ctrl_c_cancels_compact
Ctrl-C SHALL dismiss `/jump` and cancel a running compact command instead of being swallowed by the picker.

#### Scenario: Jump compact cancellation
- **WHEN** jump is open while Compact is running
- **THEN** CancelTurn is returned and jump closes.

证据：`crates/codegen/pager/src/app/agent_view/input.rs` — `jump_picker_ctrl_c_cancels_compact`。
