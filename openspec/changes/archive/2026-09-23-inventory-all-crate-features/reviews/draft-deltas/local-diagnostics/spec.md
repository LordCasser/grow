## ADDED Requirements

### Requirement: Diagnostic agent identity cache
标识 SHALL 首次从 grow_home/agent_id 读取 trim 非空值并缓存；缺失时按机器信息与 UUIDv5 生成，机器信息失败使用随机输入。

#### Scenario: 行为边界
- **WHEN** 已有非UUID缓存或写入失败
- **THEN** 非空缓存不校验UUID；Unix权限best-effort收紧0600，原子缓存写入失败不阻止返回且不保证跨进程稳定。

证据：`crates/codegen/diagnostics/src/id.rs` — `agent_id`。

### Requirement: Diagnostic TLS and Git helpers
TLS helper SHALL 尝试安装ring默认provider并忽略已有provider错误；Git context仅记录当前路径能否discover仓库。

#### Scenario: 行为边界
- **WHEN** 重复安装或Git发现失败
- **THEN** 不替换已安装provider；Git失败只得到false，不提供branch/dirty或错误分类。

证据：`crates/codegen/diagnostics/src/tls.rs` — `install_ring_provider_once`；`crates/codegen/diagnostics/src/context.rs` — `collect_git_context`。

### Requirement: Diagnostic shared typed modes
共享枚举 SHALL 提供MCP Blocking默认与精确progressive字符串转换，以及权限Ask默认和PR创建来源序列化。

#### Scenario: 行为边界
- **WHEN** MCP策略字符串大小写不同或有空白
- **THEN** 不归一化，回退Blocking；这些类型不执行权限判定。

证据：`crates/codegen/diagnostics/src/enums.rs` — `McpInitStrategy`。

### Requirement: Diagnostic session scope and prompt identity
with_session_ctx SHALL 同时建立task-local context和含session_id的span，clone共享prompt索引及ID；begin_prompt_id只在当前scope更新UUIDv4。

#### Scenario: 行为边界
- **WHEN** 无scope或索引锁忙时emit
- **THEN** 无scope输出空session/prompt ID与缺失turn；索引try_lock失败不等待，成功值cast u32。

证据：`crates/codegen/diagnostics/src/session_ctx.rs` — `with_session_ctx`。

### Requirement: Diagnostic structured emission
log_event与log_session_event SHALL 同样使用DiagnosticEvent::NAME，经emit_event写target diagnostics及JSON payload。

#### Scenario: 行为边界
- **WHEN** payload序列化失败或payload含session字段
- **THEN** 失败降为null；上下文写外层tracing字段，不覆盖payload，不直接写文件或保证subscriber落盘。

证据：`crates/codegen/diagnostics/src/session_ctx.rs` — `emit_event`。

### Requirement: Diagnostic event payload registry
事件 SHALL 通过DiagnosticEvent为91种payload绑定稳定名称，涵盖会话、权限、压缩、工具、MCP、插件、memory和客户端诊断；字段目录以本包events及关联payload源码为准。

#### Scenario: 行为边界
- **WHEN** 调用方提供字符串分类或统计值
- **THEN** 本层序列化不验证注释范围、不执行业务操作；多数Option None省略，DisplayRefreshProbe.hz None为null，TerminalDiagnostic可flatten。

证据：`crates/codegen/diagnostics/src/events.rs` — `diagnostics_event`；`crates/codegen/diagnostics/src/memory_events.rs` — `MemorySessionSummary`；`crates/codegen/diagnostics/src/session_metrics.rs` — `DoomLoopRecovery`。

### Requirement: Diagnostic project picker projection
ProjectPickerOutcome SHALL 仅将RecentProject和CustomPath投影为picked_project=true。

#### Scenario: 行为边界
- **WHEN** 构造ProjectPickerSelected或ClipboardCopy
- **THEN** 公开派生字段仍由调用方赋值，本层不强制outcome与bool或delivery与reported_success一致。

证据：`crates/codegen/diagnostics/src/events.rs` — `picked_project`。

### Requirement: Diagnostic compaction event scope
CompactionScope SHALL 在begin生成UUID并emit triggered，窗口0占比0，否则饱和计算且最高100；显式complete消耗scope并emit耗时和after值。

#### Scenario: 行为边界
- **WHEN** scope在未调用complete时丢弃
- **THEN** 没有Drop补发；公开ID/model/tokens字段可变，不保证异常路径成对或不可篡改关联。

证据：`crates/codegen/diagnostics/src/events.rs` — `CompactionScope`。

### Requirement: Diagnostic prompt latency accounting
PromptTiming SHALL 记录Instant、工具准备与MCP等待，emit时计算总耗时及饱和pre_model。

#### Scenario: 行为边界
- **WHEN** 重复record_tool_prep或子耗时大于总耗时
- **THEN** 准备值被覆盖，减法饱和0；调用方提供model_call时间，不由本层观测模型请求。

证据：`crates/codegen/diagnostics/src/prompt_timing.rs` — `PromptTiming`。

### Requirement: Diagnostic shared appender lifetime
共享appender SHALL append打开文件并保留每个非阻塞writer的WorkerGuard；flush清空全部guard。

#### Scenario: 行为边界
- **WHEN** mutex poison或重复创建writer
- **THEN** poison恢复，guard累积直到flush；队列写入不等于持久化确认，本层无大小或数量上限。

证据：`crates/codegen/diagnostics/src/appender.rs` — `non_blocking_file_writer`。

### Requirement: Diagnostic sampling file opt in
sampling layer SHALL 仅在GROW_LOG_SAMPLING精确1/true/on启用，写logs/sampling.jsonl的JSON，过滤sampling_log目标。

#### Scenario: 行为边界
- **WHEN** 其他值或目录/打开错误
- **THEN** 返回NoOp；只初始化时检查5MiB并尝试trim，单guard slot重复构造替换旧guard。

证据：`crates/codegen/diagnostics/src/sampling_log.rs` — `layer`。

### Requirement: Diagnostic hooks file configuration
hooks日志 SHALL opt-in，trim后的1/true/on/yes使用logs/hooks.log，空/0/false/off/no禁用，其余作为路径。

#### Scenario: 行为边界
- **WHEN** 开启hooks日志
- **THEN** append文本、uptime/thread id，EnvFilter hooks=debug和agent::plugins=debug，失败None；无轮转。

证据：`crates/codegen/diagnostics/src/hooks_log.rs` — `resolve_log_path`。

### Requirement: Diagnostic memory feature logging
memory-log feature SHALL 才导出memory layer，TARGET为grow_memory；启用feature后env缺失默认logs/memory.log。

#### Scenario: 行为边界
- **WHEN** 设置GROW_MEMORY_LOG或未编译feature
- **THEN** 已编译时按trim开关或路径配置，grow_memory=trace；未编译没有layer，不以debug_assertions控制。

证据：`crates/codegen/diagnostics/src/memory_log.rs` — `TARGET`。

### Requirement: Diagnostic instrumentation mode cache
Instrumentation SHALL 首次缓存trim且ASCII lowercase的env模式，默认Disabled，log/json系列为Log，chrome/trace系列为Chrome，未知非空值为Log。

#### Scenario: 行为边界
- **WHEN** 首次读取后修改env
- **THEN** 模式不刷新；输出路径可由GROW_INSTRUMENTATION_LOG非空值覆盖。

证据：`crates/codegen/diagnostics/src/instrumentation.rs` — `current_mode`。

### Requirement: Diagnostic instrumentation output lifecycle
Log模式 SHALL append JSON线程/时间信息，Chrome模式truncate并写Async trace含args；打开失败降sink或NoOp。

#### Scenario: 行为边界
- **WHEN** finalize或finalizer Drop
- **THEN** take单独Log/Chrome guard，poison跳过；不刷新其他辅助日志guard。

证据：`crates/codegen/diagnostics/src/instrumentation.rs` — `finalize`。

### Requirement: Diagnostic target layer filtering
TargetFilterLayer SHALL 在enabled精确匹配target并调用inner.enabled，在new_span/event再过滤，其余生命周期回调转发。

#### Scenario: 行为边界
- **WHEN** 与其他subscriber层组合
- **THEN** 该enabled属于Layer钩子，不保证只影响自身输出；NoOp使用默认方法。

证据：`crates/codegen/diagnostics/src/instrumentation.rs` — `TargetFilterLayer`。

### Requirement: Diagnostic panic hook chaining
panic hook SHALL 提取字符串payload或unknown panic，记录error与internal_error span，再调用此前hook。

#### Scenario: 行为边界
- **WHEN** 重复安装或payload含敏感字符串
- **THEN** 每次包裹此前hook，没有once门禁，本函数没有内容或路径脱敏。

证据：`crates/codegen/diagnostics/src/instrumentation.rs` — `install_panic_hook`。

### Requirement: Diagnostic timing and Chrome conversion
计时器 SHALL 在Log Drop输出timing，重复附加key以后值覆盖；Chrome只释放传入span，Disabled不输出。转换器读取匹配target/timing且时长正数的JSONL生成X事件。

#### Scenario: 行为边界
- **WHEN** 坏行或没有有效事件
- **THEN** 坏行跳过，无事件返回错误；输入全量收集无上限，us优先于ms，输出覆盖非原子，线程id不可解析为0，普通timer在Chrome不自行创建span。

证据：`crates/codegen/diagnostics/src/instrumentation.rs` — `generate_chrome_trace`；`crates/codegen/diagnostics/src/instrumentation.rs` — `InstrumentationTimer`。

### Requirement: Diagnostic debug destination precedence
debug目标 SHALL 优先非空GROW_LOG_FILE，否则GROW_DEBUG_LOG按小写布尔或路径选择；非UTF8路径保留。

#### Scenario: 行为边界
- **WHEN** GROW_LOG_FILE=0或GROW_DEBUG_LOG为未知值
- **THEN** 前者仍为路径0；后者为单文件路径；只有truthy debug值按session路由。

证据：`crates/codegen/diagnostics/src/debug_log.rs` — `resolve_debug_target_inner`。

### Requirement: Diagnostic debug filters and installation
GROW_LOG_FILE SHALL 尊重RUST_LOG且默认DEBUG，sampling_log关闭；GROW_DEBUG_LOG使用固定first-party debug和依赖info过滤。

#### Scenario: 行为边界
- **WHEN** 安装全局subscriber或打开失败
- **THEN** init非幂等；单文件打开失败安装原registry后warning，按session模式安装后仅sweep一次。

证据：`crates/codegen/diagnostics/src/debug_log.rs` — `install_firehose`。

### Requirement: Diagnostic debug session routing
路由 SHALL 在span创建时捕获session_id并sanitize，从最近祖先路由到session文件，无关联写role-PID文件。

#### Scenario: 行为边界
- **WHEN** 事件只有自身session字段或span后来record
- **THEN** 不据此更新路由；sanitize可碰撞且无长度限制，role未经sanitize。

证据：`crates/codegen/diagnostics/src/debug_log.rs` — `RoutingLayer`。

### Requirement: Diagnostic debug sink and latest lifetime
session sink SHALL 首写惰性打开且在进程内保留，Unix首开更新相对latest.txt链接，非Unix不操作链接。

#### Scenario: 行为边界
- **WHEN** 并发首写或已打开文件被删
- **THEN** 可创建额外worker且guard保留，旧sink不重开；无FD/worker上限，latest仅首开更新且同session临时名可冲突。

证据：`crates/codegen/diagnostics/src/debug_log.rs` — `write_session`。

### Requirement: Diagnostic debug retention
清理 SHALL 删除一级目录mtime超过7天的txt和latest临时文件，保留latest.txt和不匹配名称。

#### Scenario: 行为边界
- **WHEN** 仍打开但长期无写入的旧文件
- **THEN** 仍可能被unlink；没有FD检查、周期清理或总大小上限，最近mtime文件不按数量删除。

证据：`crates/codegen/diagnostics/src/debug_log.rs` — `prune_old_logs`。

### Requirement: Diagnostic unified wire identity
统一日志 SHALL 使用严格字段的LogEntry/ClientLogEntry/notification，pid/ver必需，source只接受shell/grow-pager，级别固定四种。

#### Scenario: 行为边界
- **WHEN** ingest客户端来源Shell或空批次
- **THEN** 直接拒绝；Pager条目保持客户端时间/PID/version和payload，本层不认证或限制批次大小。

证据：`crates/codegen/diagnostics/src/unified_log.rs` — `ingest_client_entries`。

### Requirement: Diagnostic unified synchronous writer
统一emit SHALL 生成Shell来源与当前PID/版本/UTC时间，在进程mutex下同步append JSONL；版本只接受首次设置。

#### Scenario: 行为边界
- **WHEN** 初始打开失败或写入失败
- **THEN** None writer不由后续write自动重试；写失败warning，poison丢弃，无调用方落盘确认。

证据：`crates/codegen/diagnostics/src/unified_log.rs` — `write_lines`。

### Requirement: Diagnostic unified writer maintenance
writer SHALL 写入触发且距上次至少2秒时检查路径身份与真实大小，身份变化重开原保存路径。

#### Scenario: 行为边界
- **WHEN** 重开失败或非Unix替换
- **THEN** detached丢弃直到后续维护成功；非Unix仅检测存在性，2秒窗口和大批次可超过5MiB。

证据：`crates/codegen/diagnostics/src/unified_log.rs` — `maintain`。

### Requirement: Diagnostic unified in place trimming
trim SHALL try_lock排斥其他trimmer，读取文件并保留后半段首换行后的尾部，原地重写截断保inode。

#### Scenario: 行为边界
- **WHEN** 后半段无换行或并发追加
- **THEN** 无换行不动，唯一换行在EOF可清空；append不取同锁且可能丢失，读取无上限、无crash原子性，set_len/flush错误忽略。

证据：`crates/codegen/diagnostics/src/unified_log.rs` — `trim_file`。

### Requirement: Diagnostic unified snapshots and test isolation
快照 SHALL 尝试flush后释放锁全量读取，session快照仅按JSON sid匹配；测试重定向同时改变writer和snapshot路径。

#### Scenario: 行为边界
- **WHEN** 空/坏文件或测试目录冲突
- **THEN** 快照空/错误None，session跳过坏行；私有PID+nanos目录非递归创建，Unix0700，冲突panic而不采用已有目录。

证据：`crates/codegen/diagnostics/src/unified_log.rs` — `snapshot_session_log`；`crates/codegen/diagnostics/src/unified_log.rs` — `redirect_to_temp_for_tests`。


### Requirement: Shell instrumentation exit and macro delegation

shell::instrumentation SHALL 复用diagnostics接口；finalize_and_exit先写process_exit事件，130标为SIGINT、143标为SIGTERM、其他标为other，忽略finalize返回错误，再flush debug log并process::exit，不进行栈析构。instrumentation_timer宏只接受literal，Chrome模式建立并进入目标info span后构造timer，其他模式委托普通new；不单独实现计时后端。

#### Scenario: Other exit code
- **WHEN** finalize_and_exit(1)被调用
- **THEN** 事件signal标记other，flush后进程退出1。

证据：`crates/codegen/shell/src/instrumentation.rs` — `pub fn finalize_and_exit`；`crates/codegen/shell/src/instrumentation.rs` — `macro_rules! instrumentation_timer`。

### Requirement: Shell heap profiling hook installation boundary

heap_profile SHALL 通过进程级OnceLock安装四个函数指针，首次安装生效，后续静默忽略。无hook时stats为None、set_prof_active/prof_available为false、dump报no heap profile hooks；有hook时直接委托并传播返回值，不自动根据prof_available阻止dump/set。LG_PROF_SAMPLE=19是建议值，此模块不安装allocator或修改MALLOC_CONF。

#### Scenario: Second installation
- **WHEN** 已安装hook后再次install
- **THEN** 保留首次hook，没有替换、卸载或错误返回。

证据：`crates/codegen/shell/src/heap_profile/mod.rs` — `pub fn install`；`crates/codegen/shell/src/heap_profile/mod.rs` — `pub fn dump_to_path`；`crates/codegen/shell/src/heap_profile/mod.rs` — `pub const LG_PROF_SAMPLE`。


### Requirement: Shell effective resource limit logging
资源日志 SHALL 发出startup.effective_limits，包含nofile/nproc软硬上限与available_parallelism；Unix读取失败和无限上限表示null，非Unix两项null。Linux读取/proc/self/cgroup首个0::路径并从/sys/fs/cgroup拼接读取四项pids/memory字符串，单项失败为null，未找到路径或proc读失败整个cgroup为null；不读取祖先更严上限，不修改系统限制。

#### Scenario: Partial cgroup reads
- **WHEN** 已找到v2路径但memory.max不可读
- **THEN** 仍返回cgroup对象且该字段null，其余字段各自读取。

源码证据：
- `crates/codegen/shell/src/util/limits.rs` — `pub fn log_effective_limits`。
- `crates/codegen/shell/src/util/limits.rs` — `fn gather`。
- `crates/codegen/shell/src/util/limits.rs` — `fn cgroup_v2_limits`。
### Requirement: Pager debug slash profile visibility and overlay routes

DebugCommand SHALL 在所有build中保留run实现，但visible精确返回编译期cfg!(debug_assertions)，所以release默认不列入completion；建议固定scroll、fps、log三项且忽略query。run对trim后参数做大小写敏感精确匹配：空值返回ShowDebugStatus，scroll返回与隐藏scroll-debug别名相同的ToggleScrollDebugHud，fps返回ToggleFpsHud，log返回ToggleScrollLog，其他值在错误中回显并列出三项合法值。命令不要求session、不读取当前toggle状态；实际HUD、JSONL路径和日志生命周期由下游负责。

#### Scenario: Release direct invocation
- **WHEN** release build中visible为false但调用方精确解析并运行/debug fps
- **THEN** 命令实现仍产生ToggleFpsHud。

#### Scenario: Unknown case variant
- **WHEN** 参数为FPS而非fps
- **THEN** 返回Unknown option错误，不进行ASCII小写化。

证据：`crates/codegen/pager/src/slash/commands/debug.rs` — `LISTED_IN_COMPLETIONS / DebugCommand::visible / suggest_args / run`。

### Requirement: Pager doctor slash live report and fix request parsing

DoctorCommand::report_for_terminal SHALL 以LiveTmuxProbe、terminal context、screen_mode.is_fullscreen、全局kitty flags及已检测XTVERSION构建TUI snapshot，再把notification method/protocol/condition与workspace runtime findings以及agent-definition findings依次合并进view report。slash command接受参数且session_scoped，但run不检查session；无token返回Doctor Report，单独小写fix返回ListFixes，`fix <value>`仅在恰好两token时经resolve_fix_id返回Fix，未知ID附带resolver错误和usage，其他token数或大小写返回usage。suggest_args对空query返回None；非`fix`前缀返回单个fix项，精确`fix`或`fix `前缀按handle contains或canonical ID starts_with过滤automatic choices；已解析有效完整ID时关闭建议。建议匹配区分大小写且不运行probe或fix。

#### Scenario: Canonical fix already complete
- **WHEN** query为`fix terminal.ssh-wrap`且resolve成功
- **THEN** suggest_args返回None，让已完成参数保持关闭。

#### Scenario: Extra fix token
- **WHEN** 运行`fix ssh-wrap extra`
- **THEN** 返回usage错误，不忽略尾随参数。

证据：`crates/codegen/pager/src/slash/commands/doctor.rs` — `DoctorCommand::report_for_terminal / suggest_args / session_scoped / run`。

### Requirement: Pager scroll flight recorder enablement lazy sink and failure isolation

ScrollLogRecorder::from_env_at SHALL 只读取一次GROW_SCROLL_LOG：unset或trim后`0`返回None；trim后空或`1`选择`grow_home()/logs/scroll-log-<UTC YYYYMMDD-HHMMSS>.jsonl`，其他trim值直接作为PathBuf。构造只保存Pending path与base Instant，不创建文件；首条record才创建非空parent目录并以File::create打开BufWriter，因此enabled但无scroll不留文件，显式或默认同名旧文件会被截断。open失败记录一次warn并永久切换Disabled；write/writeln或Finalize flush失败同样warn并Disabled，后续record仅分支返回且不panic、不写stderr。普通record只进入BufWriter，Finalize record额外flush使gesture boundary对tail可见；drop/failure不提供显式最终flush保证。recorder为mouse state的可选观察者，不向scroll决策反馈。

#### Scenario: Enabled idle session
- **WHEN** GROW_SCROLL_LOG=1但没有任何scroll transition
- **THEN** recorder存在但默认日志文件尚未创建。

#### Scenario: Custom path
- **WHEN** 环境值trim后既非0、空也非1
- **THEN** 以该值为目标路径并在首record创建需要的父目录。

#### Scenario: First write failure
- **WHEN** open或写入发生IO错误
- **THEN** 禁用后续记录且只经tracing warn报告，不影响scroll状态机。

源码证据：
- `crates/codegen/pager/src/input/scroll_log.rs` — `ScrollLogRecorder::from_env_at / new / record / write_line / open_writer / default_log_path`。

### Requirement: Pager scroll flight recorder transition schema and timing bookkeeping

scroll log JSONL SHALL 每条为flat object，evt序列化stream_start/flush/finalize，trigger序列化event/tick/promotion/finalize；state machine提供kind、events_total、可选avg interval、accel、desired、applied/flushed/backlog、carry、cap、可选dropped/config，recorder补ts_ms、events_since_flush和可选ms_since_prev_flush。config echo仅由producer在stream start提供并flatten为mode/ept/wheel_lpt/trackpad_lpt/invert/speed/viewport_height；None字段avg_interval_ms、ms_since_prev_flush、dropped省略。ts_ms用now相对base的saturating monotonic duration。StreamStart把events_at_last_flush重置0且events_since_flush=0，但仍可携带距上一flush-bearing record的ms；Flush/Finalize用events_total减上次值的saturating差，随后把last_flush_at与events_at_last_flush更新为当前。record在序列化和IO前更新bookkeeping；JSON序列化失败只丢该行，不自动Disabled。Finalize的dropped由producer解释为丢弃whole-line backlog，recorder不验证它等于backlog_after，也不自行省略zero-delta事件。

#### Scenario: First stream start
- **WHEN** recorder尚无flush记录并收到StreamStart
- **THEN** ts从base计算、events_since_flush为0且ms_since_prev_flush省略。

#### Scenario: Flush spacing
- **WHEN** 连续两个非StreamStart记录events_total递增
- **THEN** 第二条events_since_flush只计算自上一flush-bearing记录的增量，并带两次Instant间隔。

#### Scenario: Config flattening
- **WHEN** producer在stream start提供ScrollLogConfigEcho
- **THEN** 配置字段直接出现在同一JSON object，不嵌套config对象。

源码证据：
- `crates/codegen/pager/src/input/scroll_log.rs` — `ScrollLogEvt / ScrollLogTrigger / ScrollLogConfigEcho / ScrollLogEvent / ScrollLogRecord / ScrollLogRecorder::record`。

### Requirement: Pager mouse scroll read only debug snapshot and recorder integration
debug_snapshot SHALL observe &self using supplied config/time without clock reads or mutation, exposing live classification/pricing/backlog/deadline, last summary, carry and captured config. The recorder is write-only to decisions: start is recorded before accumulation, nonzero flush after delivery with its trigger, and finalize with post-flush accounting. toggle_scroll_log drops an active recorder or creates a fresh lazy recorder at a timestamped path; active reports recorder presence. Recorder failure cannot alter delivered lines.

#### Scenario: Repeated sample
- **WHEN** snapshot is called twice with identical inputs
- **THEN** both values are equal and accounting is unchanged.

#### Scenario: Promotion record
- **WHEN** promotion causes a nonzero flush
- **THEN** the post-flush record has trigger=promotion.

#### Scenario: Runtime toggle
- **WHEN** logging is off then toggled twice
- **THEN** first returns a jsonl path and activates; second returns None and deactivates.

源码证据：`crates/codegen/pager/src/input/mouse.rs`；`crates/codegen/pager/src/input/mouse/tests.rs`。
