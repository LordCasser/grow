# diagnostics 逐包审阅（进行中）

已读Cargo.toml、lib.rs、tls.rs、id.rs、enums.rs、context.rs、prompt_timing.rs全部；其他模块待完成，包保持pending。

- feature default空、default-bazel空、memory-log空声明；其实际cfg用途待读。公共模块context/debug_log/enums/events/hooks_log/id/instrumentation/memory_events/memory_log/prompt_timing/sampling_log/session_ctx/session_metrics/tls/unified_log，appender私有。
- TLS helper每次创建ring provider并尝试install_default，忽略已安装/失败；函数名once表达幂等安装结果，没有本层OnceLock，既有其他provider不替换。
- agent_id以OnceLock缓存首次结果，优先读取grow_home/agent_id trim非空字符串，不要求UUID合法；读到即best-effort chmod0600。读失败/空则计算，Linux非空HOSTNAME作为mid key的一部分，缺失或mid失败UUIDv4；非Linuxmid(agent_id)失败也UUIDv4，之后统一UUIDv5 NAMESPACE_OID。持久化失败仍返回本次ID，跨进程稳定性不保证。
- ID cache创建父目录、config fs_atomic write_atomically mode0600；加载已有路径使用read_to_string/set_permissions跟随路径，未在本层校验symlink或固定文件身份。三项Unix测试只临时文件验证新写/覆写/旧权限收紧，不调用真实machine ID。
- McpInitStrategy默认Blocking，仅精确progressive From字符串命中Progressive，不trim/大小写归一，serde Serialize snake_case；PrCreationSource Bash/Mcp snake_case双向serde；PermissionMode Ask/AlwaysApprove/Auto kebab-case，默认Ask，as_str与is方法直接判枚举，不执行授权。
- collect_git_context仅git2 Repository::discover(cwd).is_ok=>is_git_repo；不返回branch/remote/dirty，不区分错误类型。
- PromptTiming::start记录Instant，record_tool_prep覆盖mcp_wait、tool_collection=total_prep saturating_sub(wait)；emit消耗self，以elapsed总毫秒计算pre_model saturating_sub(model_call)，加turn/MCP数量/策略/model ID记录PromptLatency事件，输出写入行为待session_ctx核对。

## 会话与辅助日志模块（完整阅读）

本阶段完整读取 session_ctx.rs、appender.rs、sampling_log.rs、hooks_log.rs、memory_events.rs、memory_log.rs、session_metrics.rs、instrumentation.rs；debug_log/events/unified_log 仍待完成，包保持 pending。

- DiagnosticCtx clone 共享 prompt_index 与 prompt_id 的 Arc；with_session_ctx 同时建立 Tokio task-local scope 和含 session_id 的 info span。begin_prompt_id 在当前 context 写 UUIDv4，无 context 静默不操作。log_event 与 log_session_event 实现相同，均使用类型 NAME 调用 emit_event。
- emit_event 将序列化失败降为 JSON null；无 context 时 session_id/prompt_id 为空字符串、turn_number 为 None。prompt_index 使用 try_lock，锁忙时不等待而省略值，usize 转 u32 为 cast；prompt_id 使用同步 parking_lot 锁。输出 target diagnostics、事件名、关联字段和 JSON payload 到 tracing，本函数不直接写磁盘。session span 测试只证明字段存在，不证明端到端路由或子任务自动继承。
- appender 创建父目录 best-effort、append 打开文件，创建非阻塞 writer；进程级 Vec 累积所有 WorkerGuard，mutex poison 后恢复；flush_file_log_guards 清空 Vec，依赖 guard drop 刷新并结束 writer。此处无文件大小、权限收紧、symlink 限制或持久化成功确认。其他三个辅助日志仍各用单个 guard slot，重复构造会替换/drop 旧 guard，poison 时新 guard 不保存。
- sampling layer 只接受 GROW_LOG_SAMPLING 精确 1/true/on，不 trim；其他值 NoOp。写 grow_home/logs/sampling.jsonl，初始化发现 size >= unified MAX_SIZE 才 trim，后续大小管理待 unified 核对。目录/打开失败降级 NoOp。JSON RFC3339、无 ANSI/target/current_span，使用 TargetFilterLayer 对 sampling_log 目标过滤；本模块未定义退出刷新函数。
- hooks layer opt-in；GROW_HOOKS_LOG trim 后空/0/false/off/no 禁用，1/true/on/yes 默认 logs/hooks.log，其他值作为路径（大小写不折叠）。追加文本、target/thread id、相对 uptime 毫秒，EnvFilter hooks=debug,agent::plugins=debug；打开失败 None，不做轮转或大小限制。
- memory_log TARGET 实际为 grow_memory，注释中的 memory 不是常量值。只有 memory-log feature 导出 layer；启用 feature 后 env 缺失或非 Unicode 默认 logs/memory.log，显式开关/自定义路径规则与 hooks 相同。追加文本、uptime/thread id，EnvFilter grow_memory=trace。不能将“debug builds”注释当作 cfg(debug_assertions) 门禁。
- memory_events 定义 9 个 Serialize payload：初始化（watcher/decay/MMR/embedding/库存）、search 与 empty、flush start/complete、injection、reindex、watcher sync、session summary（含 dream 计数）。这些是调用方传值的数据结构，不执行搜索、计时、阈值检查或计数累加；事件名称绑定待 events 模块核对。
- session_metrics 定义 SessionStarted、Turn、TurnCompletedLifecycle、DoomLoopRecovery。后者包含 attempts/accepted_after_budget/top_trigger/model，top_trigger None 不序列化；现有测试固定事件名 doom_loop_recovery 和字段 shape，不证明调用方恢复算法。

## Instrumentation（619 行完整阅读）

- 模式在首次读取时 OnceLock 缓存 GROW_INSTRUMENTATION：trim+ASCII lowercase；1/true/on/enabled/log/json/jsonl 为 Log，chrome/trace/trace.json 为 Chrome，空/0/false/off/disabled/none 为 Disabled，未知非空值也为 Log；env 缺失 Disabled，后续修改 env 不刷新模式。
- GROW_INSTRUMENTATION_LOG 是 trim 非空路径覆盖，默认 logs/instrumentation.log 或 instrumentation.trace.json。Log 创建目录并 append 文件，错误 stderr 后 sink；JSON 包含 UTC 时间、线程 id/name、target，省略 current span。Chrome 创建目录并 truncate 文件，include_args、Async trace style，错误降级 NoOp；不提供自动路径隔离或轮转。
- TargetFilterLayer.enabled 对 target 精确匹配且调用 inner.enabled，on_new_span/on_event 再判目标，其余 record/follows/enter/exit/close 直接转发；这是 Layer enabled 实现，不能描述为只影响自身输出而不会影响组合 subscriber。组合影响需读注册调用方与 tracing 依赖才能作更强结论。NoOpLayer 使用默认方法。
- 单独 LOG_GUARD/CHROME_GUARD slot 保存 writer；finalize 在 Disabled 直接成功，否则 take 两种 guard，poison 时跳过；InstrumentationFinalizer Drop 调用 finalize 并忽略 Result，不负责其他日志 guard。
- install_panic_hook 每次 take 当前 hook 再包裹，没有 once 防重入安装。将 &str/String payload 或 unknown panic 与源码位置写 tracing error，另创建 internal_error span，再调用前一个 hook；本函数未实现注释提及的路径脱敏，不能承诺 panic.message 不含用户内容。
- generate_chrome_trace 输入优先 options.input、env path、默认 log；输出默认 input.with_extension(trace.json)。逐行跳过 I/O 错误、非法 JSON、非 grow_instrumentation、非 timing、缺 name/正时长/合法 RFC3339 的记录。elapsed_us 优先，其次 elapsed_ms saturating_mul(1000)；start=end.saturating_sub(dur_us as i64)，cast 超 i64 边界并未拒绝。thread_id/threadId 接受 i64 数字/可解析字符串，否则 0；thread_name/threadName 和额外 fields 原样放 args。输出 X 事件、pid1、displayTimeUnit ms，保留输入顺序，无排序/事件数/单行长度上限；全部收集内存后才 File::create 输出，无事件返回错误且不创建输出。无父目录创建或原子写入。
- InstrumentationTimer 捕获 Instant 与模式；with_field 仅 Log 保存键值，重复 key 在 drop 建 JSON map 时以后值覆盖。Disabled drop 不输出；Chrome drop 只释放传入 EnteredSpan，因此普通 timer() 在 Chrome 模式不会自行创建 span。Log drop 发 timing/name/elapsed_us，附加 fields 使用 Debug 格式写入 tracing；离线转换保留该值，不保证它是嵌套 JSON 对象。计时 cast 为 u64，本模块无执行中取消/显式 stop 接口。

## Unified log（1037 行完整阅读，含测试）

- 全局 VERSION 只接受首次 set_version，未设置时用 diagnostics 编译版本。LogLevel 固定 error/warn/info/debug，LogSource 固定 shell/grow-pager；LogEntry、ClientLogEntry、LogNotificationParams deny_unknown_fields，pid/ver 必需，sid/ctx 可省略；timestamp 只是 String，未做 RFC3339 输入验证。
- emit 生成当前 UTC 毫秒时间、Shell 来源、当前 PID/version，串行 JSON 加换行后写；四个级别 helper 仅委托。ingest_client_entries 拒绝 Shell 或空批次，其他条目保留客户端 timestamp/pid/version/sid/msg/ctx 并统一来源；先构造整批字节再取 writer mutex，无条数/字节/字段内容限制，也不认证这些客户端字段。
- WRITER 是 LazyLock<Mutex<Option<LogWriter>>>，首次打开失败保留 None；普通 write_lines 对 None 直接丢弃，不自动重试初始失败。打开创建父目录，文件 >=5MiB 则尝试 trim，再 append。写失败只 warning，无 retry/错误返回；poison 的 mutex 直接丢弃。与非阻塞 tracing appender 不同，此处同步执行文件操作。
- maintain 仅在写入触发且距上次 >=2 秒时检查，不是后台每2秒运行。Unix 用路径 metadata(dev,ino)，非 Unix 只用(0,0)表示存在，因此能发现删除而不能可靠区分替换；打开身份来自路径 stat 而非 descriptor metadata，不能承诺路径竞争下的身份原子性。
- 检测身份变化后 reopen 同一已保存路径，不重新读取 GROW_HOME。reopen 失败进入 detached 丢弃条目，节流期继续丢弃，下一维护时间重试；成功替换 writer。身份相同清 detached，并按真实路径文件大小 trim。2秒窗口及一次大批次可超过5MiB，阈值不是硬上限。
- trim_file 以 read/write 打开同一文件并 try_lock 排斥其他 trimmer，竞争直接返回；不重查大小。read_to_end 无内存上限，在后半段寻找首个换行，保留该换行之后尾部；没有换行则不动，若唯一换行在 EOF 则尾部为空并可清空文件。原地 rewind/write_all/set_len/flush 保持 inode；set_len/flush 错误忽略，没有 fsync 或原子替换。普通 append writer 不取同一 advisory lock，修剪期间并发追加仍可能丢失；不能保证跨进程一条记录的原子完整性或崩溃一致性。
- snapshot_log 先尝试 flush 当前 descriptor、释放锁后按当前 log_path 全量读取，空/错误 None；不维护 stale handle、不限制大小、不保证一致快照。snapshot_session_log 同样读取后逐行解析任意 JSON Value，仅要求 sid 字符串相等，保留原字节并追加换行；不验证 LogEntry 完整 schema，坏 JSON/空行跳过，无匹配 None。
- redirect_to_temp_for_tests 设置进程级 atomic 标记并尝试重开 writer，snapshot 也改用同一路径；test 目录由 OnceLock 保存，PID+nanos 命名、非递归 create 拒绝已有路径，Unix mode0700，创建失败 panic；不自动清理目录。unified 模块的 ctor 在本测试二进制 main 前调用，生产接口本身没有 cfg(test) 限制。
- 测试覆盖重定向读写、JSON身份字段/枚举、修剪保 inode和旧句柄可见、替换/删除恢复、他人写入造成超限、锁竞争、reopen失败后恢复、保留后半段/无换行/缺文件。Unix文件身份测试受 cfg 限制，模拟另一writer用同进程独立句柄，不能声称真实多进程交错穷举。ingest_rejects_shell_src 仅调用函数无写入结果断言；客户端转发完整性也不能由 roundtrip 序列化测试替代。

## Debug log（961 行完整阅读，含测试）

- GROW_LOG_FILE 非空优先于 GROW_DEBUG_LOG，前者即使值为 0 也作为路径；UTF8 trim，非UTF8原字节保留。后者 trim 后空/0/false/off/no 禁用，1/true/on/yes 按 session 路由，其余值为单文件路径，不大小写归一。默认目录 grow_home/debug。
- GROW_LOG_FILE 的 EnvFilter 默认 DEBUG、from_env_lossy 读取 RUST_LOG，再添加 sampling_log=off。GROW_DEBUG_LOG 单文件及按session共用固定 firehose：全局info，pager/shell/tools/diagnostics/agent/mcp/acp debug，sampling_log off，acp_update debug；不读取 RUST_LOG。常量 acp_update_payload 未显式加入debug指令，不能从注释推断所有该target的DEBUG payload一定收录；RMCP噪声target常量本模块只声明，未添加到这里的filter。
- install_firehose 使用全局 subscriber init（非 try_init），因此已有全局subscriber时不是可重复无害调用。单文件打开失败仍安装原 registry 后warning；路由分支先安装后启动一次 sweep，其他分支不启动清理。flush 清空共享appender所有guard，供退出调用；不是按session释放。
- span创建时 visitor用 Debug记录 session_id，不要求span名为session，也不读取事件自身session_id；extensions保存一次sanitize结果，后续span.record不更新。event_scope最近携带ID的祖先优先，缺则 role-PID.txt。生产的 %Display ID避免Debug字符串引号；直接字符串字段可能带引号再被替换成下划线。sanitize仅保留ASCII字母数字/连字符/下划线/点，其他每字符替换为下划线，空和纯点统一 _；不限制长度或保证无碰撞。role由调用方原样用于fallback路径，不经过sanitize。
- 每个session惰性打开一个NonBlocking writer，fallback同样惰性；map mutex poison恢复。文件打开/worker创建在map锁外，失败丢当次行且后续事件重新尝试。并发首写可各自创建writer并写入再entry合并，额外guard已在全局登记，直到flush才释放；不承诺写入顺序。map/worker/FD没有上限或eviction，已有文件descriptor没有unified log的替换重开机制。
- 路由格式为UTC微秒、level、target、Debug message及字段，不包含祖先span上下文字段；本代码不主动加ANSI，但也未对消息内容脱敏、去ANSI或限制字节。write_all错误忽略，使用非阻塞队列不等于无丢失的持久化承诺。
- Unix在首次打开session sink时更新latest.txt，相对目标，临时名.latest.<session filename>.tmp，先remove再symlink再rename；同一session跨进程仍共用临时名，不是全局唯一。rename失败清临时，非Unix完全no-op；已有sink后续写不刷新latest。latest可能与保留的用户同名session key冲突，本层未预留latest或fallback名称。
- sweep在目录第一层按mtime >7天删除 .txt（排除latest.txt）及.latest.*.tmp，best-effort。metadata不跟随symlink，旧dangling临时也可删；非UTF8名字跳过，未来时间跳过。无总大小/数量上限、不周期运行、不检查FD是否仍打开，因此“永不unlink仍打开日志”的注释仅对近期写入成立；长期闲置已打开文件仍可被删，旧sink随后不会重开。
- 测试涵盖文件创建/打开失败、env优先和非UTF8路径、filter解析及payload未显式加入、sanitize、session/fallback隔离、真实firehose filter下路由、多session累积、Unix latest更新/失败清理、按年龄清理与dangling orphan、25个近期文件不按数量删除。没有嵌套session、span后置record、同名冲突、同session并发首次打开或长期闲置FD的动态覆盖；测试中的active指近期mtime，不检查真实持有句柄。

## Events（1654 行完整阅读，含测试）

- DiagnosticEvent 仅要求 Serialize + Send + static 和 NAME，宏逐类型实现名称常量，无通用字段校验、事件自动触发或隐私过滤。文件头说关联字段由shell integration注入、CompactionScope留在shell，均与当前本包实现位置不符；session_ctx写外层tracing字段，不修改payload内部同名值。
- enum均按声明snake_case序列化，范围包括计划触发/阶段、提示类型和动作、权限触发/access/outcome、压缩触发、通用与工具结果、hook/client gate结果、MCP transport/error/strategy、memory flush触发、pager命令来源、插件安装和来源、extensions触发/输入/页签、项目选择结果。PermissionOutcome/McpErrorType/InstallKind as_str显式匹配稳定字符串；这些枚举不执行对应权限或恢复操作。
- payload覆盖计划/提示/自动补全、权限、自动压缩与降级、子agent、模型切换、插件生命周期和CTA、extensions modal、hook与skill、MCP、会话harness/load/new/end、prompt/turn/model response/latency、PR与多agent选择、repo统计、memory、终端/clipboard/notification/dashboard、rate limit/API/internal error。大多数可选字段None省略；DisplayRefreshProbe.hz没有skip属性，None输出null。TerminalDiagnostic以flatten嵌入若干终端事件，无额外terminal对象；字段中的分类String/static str和数值由调用方提供，不验证注释列出的范围或一致性。
- ProjectPickerOutcome.picked_project仅RecentProject/CustomPath为true；ProjectPickerSelected公开的picked_project字段仍可由调用方给不一致值。ClipboardCopy的delivery/reported_success/toast_kind也是独立公开字段，不在此推导；tests手动提供匹配值，不能证明业务归因正确。API/internal error payload没有message字段，但String类型本身没有内容过滤。
- CompactionScope::begin UUIDv4，context_window0时percentage0，否则tokens_used saturating_mul100 /window cap100，先emit triggered再记录Instant；complete消耗self，emit completed，model_id Some、elapsed毫秒cast u64。没有Drop实现，丢弃/错误/取消时不自动complete，所以不保证注释的“both events fire”。compaction_id/tokens_before/model_id公开可变，调用方也可改变相关性；本模块不承诺跨任务上下文稳定。
- tests固定clipboard字段类型及delivery、项目选择名称/shape/全部variant、插件CTA名称和可选error省略、compaction retry降级字段。没有全部事件逐一roundtrip，也没有CompactionScope异常退出成对事件测试。所有事件名称绑定如下，覆盖已读memory_events和session_metrics；AgentInfo是嵌套载荷，不单独绑定。

| Payload | Event name |
| --- | --- |
| `PlanModeToggled` | `plan_mode_toggled` |
| `ContextualTip` | `contextual_tip` |
| `PromptSuggestion` | `prompt_suggestion` |
| `PermissionModeChanged` | `permission_mode_changed` |
| `SlashCommandUsed` | `slash_command_used` |
| `PermissionPrompted` | `permission_prompted` |
| `PermissionDecisionPayload` | `permission_decision` |
| `AutoCompactFired` | `auto_compact_fired` |
| `AutoCompactPruned` | `auto_compact_pruned` |
| `CompactionTriggered` | `compaction_triggered` |
| `CompactionCompleted` | `compaction_completed` |
| `AutoCompactSuppressed` | `auto_compact_suppressed` |
| `CompactionRetryDegraded` | `compaction_retry_degraded` |
| `SubagentLaunched` | `subagent_launched` |
| `SubagentCompleted` | `subagent_completed` |
| `ModelSwitched` | `model_switched` |
| `PluginAdded` | `plugin_added` |
| `PluginRemoved` | `plugin_removed` |
| `PluginInstalled` | `plugin_installed` |
| `PluginUninstalled` | `plugin_uninstalled` |
| `PluginReloaded` | `plugin_reloaded` |
| `PluginUsed` | `plugin_used` |
| `PluginCtaImpression` | `plugin_cta_impression` |
| `PluginCtaConnectClicked` | `plugin_cta_connect_clicked` |
| `PluginCtaDismissed` | `plugin_cta_dismissed` |
| `PluginCtaInstalled` | `plugin_cta_installed` |
| `ExtensionsModalOpened` | `extensions_modal_opened` |
| `ExtensionsModalAction` | `extensions_modal_action` |
| `HookAdded` | `hook_added` |
| `HookRemoved` | `hook_removed` |
| `HookTrusted` | `hook_trusted` |
| `HookExecuted` | `hook_executed` |
| `HookBlocked` | `hook_blocked` |
| `ClientHookGate` | `client_hook_gate` |
| `SkillAdded` | `skill_added` |
| `SkillRemoved` | `skill_removed` |
| `SkillDispatched` | `skill_dispatched` |
| `McpServerConnected` | `mcp_server_connected` |
| `McpServerFailed` | `mcp_server_failed` |
| `McpInitCompleted` | `mcp_init_completed` |
| `McpToolCalled` | `mcp_tool_called` |
| `SessionHarness` | `session_harness` |
| `SessionLoad` | `session_load` |
| `SessionNew` | `session_new` |
| `PromptSubmitted` | `prompt_submitted` |
| `UserFeedback` | `user_feedback` |
| `PrCreated` | `pr_created` |
| `PrMerged` | `pr_merged` |
| `MultiAgentFollowup` | `multi_agent_followup` |
| `MultiAgentApply` | `multi_agent_apply` |
| `MultiAgentDiscard` | `multi_agent_discard` |
| `RepoChanges` | `repo_changes` |
| `NonGitDecisionEvent` | `non_git_decision` |
| `PromptLatency` | `prompt_latency` |
| `TurnCompleted` | `turn_completed` |
| `ShellTrueNoop` | `shell_true_noop` |
| `ActionStationarityStop` | `action_stationarity_stop` |
| `ToolCallCompleted` | `tool_call_completed` |
| `ModelResponseReceived` | `model_response_received` |
| `MemoryFlushed` | `memory_flushed` |
| `SessionEnded` | `session_ended` |
| `PagerSlashCommand` | `pager_slash_command` |
| `PlanSubmit` | `plan_submit` |
| `ProjectPickerSelected` | `project_picker_selected` |
| `TerminalDiagnostic` | `terminal_context` |
| `DisplayRefreshProbe` | `display_refresh_probe` |
| `BackspaceNoEffect` | `backspace_no_effect` |
| `ClipboardImagePaste` | `clipboard_image_paste` |
| `PasteKeyEmptyHostClipboard` | `paste_key_empty_host_clipboard` |
| `ClipboardCopy` | `clipboard_copy` |
| `NotificationEmitted` | `notification_emitted` |
| `DashboardOpened` | `dashboard_opened` |
| `DashboardClosed` | `dashboard_closed` |
| `DashboardAgentAttached` | `dashboard_agent_attached` |
| `DashboardAgentLaunched` | `dashboard_agent_launched` |
| `RateLimitHit` | `rate_limit_hit` |
| `ApiError` | `api_error` |
| `InternalError` | `internal_error` |
| `crate::session_metrics::SessionStarted` | `session_started` |
| `crate::session_metrics::Turn` | `turn` |
| `crate::session_metrics::TurnCompletedLifecycle` | `turn_completed_lifecycle` |
| `crate::session_metrics::DoomLoopRecovery` | `doom_loop_recovery` |
| `crate::memory_events::MemorySessionInit` | `memory_session_init` |
| `crate::memory_events::MemorySearch` | `memory_search` |
| `crate::memory_events::MemorySearchEmpty` | `memory_search_empty` |
| `crate::memory_events::MemoryFlushStart` | `memory_flush_start` |
| `crate::memory_events::MemoryFlushComplete` | `memory_flush_complete` |
| `crate::memory_events::MemoryInjection` | `memory_injection` |
| `crate::memory_events::MemoryReindex` | `memory_reindex` |
| `crate::memory_events::MemoryWatcherSync` | `memory_watcher_sync` |
| `crate::memory_events::MemorySessionSummary` | `memory_session_summary` |

## 收口

全包已读；macOS all-features串行测试54通过、0失败、0忽略，doc-tests0。

## 功能与规范映射

- [Diagnostic agent identity cache](../specs/local-diagnostics/spec.md#requirement-diagnostic-agent-identity-cache)：标识 SHALL 首次从 grow_home/agent_id 读取 trim 非空值并缓存；缺失时按机器信息与 UUIDv5 生成，机器信息失败使用随机输入。
- [Diagnostic TLS and Git helpers](../specs/local-diagnostics/spec.md#requirement-diagnostic-tls-and-git-helpers)：TLS helper SHALL 尝试安装ring默认provider并忽略已有provider错误；Git context仅记录当前路径能否discover仓库。
- [Diagnostic shared typed modes](../specs/local-diagnostics/spec.md#requirement-diagnostic-shared-typed-modes)：共享枚举 SHALL 提供MCP Blocking默认与精确progressive字符串转换，以及权限Ask默认和PR创建来源序列化。
- [Diagnostic session scope and prompt identity](../specs/local-diagnostics/spec.md#requirement-diagnostic-session-scope-and-prompt-identity)：with_session_ctx SHALL 同时建立task-local context和含session_id的span，clone共享prompt索引及ID；begin_prompt_id只在当前scope更新UUIDv4。
- [Diagnostic structured emission](../specs/local-diagnostics/spec.md#requirement-diagnostic-structured-emission)：log_event与log_session_event SHALL 同样使用DiagnosticEvent::NAME，经emit_event写target diagnostics及JSON payload。
- [Diagnostic event payload registry](../specs/local-diagnostics/spec.md#requirement-diagnostic-event-payload-registry)：事件 SHALL 通过DiagnosticEvent为91种payload绑定稳定名称，涵盖会话、权限、压缩、工具、MCP、插件、memory和客户端诊断；字段目录以本包events及关联payload源码为准。
- [Diagnostic project picker projection](../specs/local-diagnostics/spec.md#requirement-diagnostic-project-picker-projection)：ProjectPickerOutcome SHALL 仅将RecentProject和CustomPath投影为picked_project=true。
- [Diagnostic compaction event scope](../specs/local-diagnostics/spec.md#requirement-diagnostic-compaction-event-scope)：CompactionScope SHALL 在begin生成UUID并emit triggered，窗口0占比0，否则饱和计算且最高100；显式complete消耗scope并emit耗时和after值。
- [Diagnostic prompt latency accounting](../specs/local-diagnostics/spec.md#requirement-diagnostic-prompt-latency-accounting)：PromptTiming SHALL 记录Instant、工具准备与MCP等待，emit时计算总耗时及饱和pre_model。
- [Diagnostic shared appender lifetime](../specs/local-diagnostics/spec.md#requirement-diagnostic-shared-appender-lifetime)：共享appender SHALL append打开文件并保留每个非阻塞writer的WorkerGuard；flush清空全部guard。
- [Diagnostic sampling file opt in](../specs/local-diagnostics/spec.md#requirement-diagnostic-sampling-file-opt-in)：sampling layer SHALL 仅在GROW_LOG_SAMPLING精确1/true/on启用，写logs/sampling.jsonl的JSON，过滤sampling_log目标。
- [Diagnostic hooks file configuration](../specs/local-diagnostics/spec.md#requirement-diagnostic-hooks-file-configuration)：hooks日志 SHALL opt-in，trim后的1/true/on/yes使用logs/hooks.log，空/0/false/off/no禁用，其余作为路径。
- [Diagnostic memory feature logging](../specs/local-diagnostics/spec.md#requirement-diagnostic-memory-feature-logging)：memory-log feature SHALL 才导出memory layer，TARGET为grow_memory；启用feature后env缺失默认logs/memory.log。
- [Diagnostic instrumentation mode cache](../specs/local-diagnostics/spec.md#requirement-diagnostic-instrumentation-mode-cache)：Instrumentation SHALL 首次缓存trim且ASCII lowercase的env模式，默认Disabled，log/json系列为Log，chrome/trace系列为Chrome，未知非空值为Log。
- [Diagnostic instrumentation output lifecycle](../specs/local-diagnostics/spec.md#requirement-diagnostic-instrumentation-output-lifecycle)：Log模式 SHALL append JSON线程/时间信息，Chrome模式truncate并写Async trace含args；打开失败降sink或NoOp。
- [Diagnostic target layer filtering](../specs/local-diagnostics/spec.md#requirement-diagnostic-target-layer-filtering)：TargetFilterLayer SHALL 在enabled精确匹配target并调用inner.enabled，在new_span/event再过滤，其余生命周期回调转发。
- [Diagnostic panic hook chaining](../specs/local-diagnostics/spec.md#requirement-diagnostic-panic-hook-chaining)：panic hook SHALL 提取字符串payload或unknown panic，记录error与internal_error span，再调用此前hook。
- [Diagnostic timing and Chrome conversion](../specs/local-diagnostics/spec.md#requirement-diagnostic-timing-and-chrome-conversion)：计时器 SHALL 在Log Drop输出timing，重复附加key以后值覆盖；Chrome只释放传入span，Disabled不输出。转换器读取匹配target/timing且时长正数的JSONL生成X事件。
- [Diagnostic debug destination precedence](../specs/local-diagnostics/spec.md#requirement-diagnostic-debug-destination-precedence)：debug目标 SHALL 优先非空GROW_LOG_FILE，否则GROW_DEBUG_LOG按小写布尔或路径选择；非UTF8路径保留。
- [Diagnostic debug filters and installation](../specs/local-diagnostics/spec.md#requirement-diagnostic-debug-filters-and-installation)：GROW_LOG_FILE SHALL 尊重RUST_LOG且默认DEBUG，sampling_log关闭；GROW_DEBUG_LOG使用固定first-party debug和依赖info过滤。
- [Diagnostic debug session routing](../specs/local-diagnostics/spec.md#requirement-diagnostic-debug-session-routing)：路由 SHALL 在span创建时捕获session_id并sanitize，从最近祖先路由到session文件，无关联写role-PID文件。
- [Diagnostic debug sink and latest lifetime](../specs/local-diagnostics/spec.md#requirement-diagnostic-debug-sink-and-latest-lifetime)：session sink SHALL 首写惰性打开且在进程内保留，Unix首开更新相对latest.txt链接，非Unix不操作链接。
- [Diagnostic debug retention](../specs/local-diagnostics/spec.md#requirement-diagnostic-debug-retention)：清理 SHALL 删除一级目录mtime超过7天的txt和latest临时文件，保留latest.txt和不匹配名称。
- [Diagnostic unified wire identity](../specs/local-diagnostics/spec.md#requirement-diagnostic-unified-wire-identity)：统一日志 SHALL 使用严格字段的LogEntry/ClientLogEntry/notification，pid/ver必需，source只接受shell/grow-pager，级别固定四种。
- [Diagnostic unified synchronous writer](../specs/local-diagnostics/spec.md#requirement-diagnostic-unified-synchronous-writer)：统一emit SHALL 生成Shell来源与当前PID/版本/UTC时间，在进程mutex下同步append JSONL；版本只接受首次设置。
- [Diagnostic unified writer maintenance](../specs/local-diagnostics/spec.md#requirement-diagnostic-unified-writer-maintenance)：writer SHALL 写入触发且距上次至少2秒时检查路径身份与真实大小，身份变化重开原保存路径。
- [Diagnostic unified in place trimming](../specs/local-diagnostics/spec.md#requirement-diagnostic-unified-in-place-trimming)：trim SHALL try_lock排斥其他trimmer，读取文件并保留后半段首换行后的尾部，原地重写截断保inode。
- [Diagnostic unified snapshots and test isolation](../specs/local-diagnostics/spec.md#requirement-diagnostic-unified-snapshots-and-test-isolation)：快照 SHALL 尝试flush后释放锁全量读取，session快照仅按JSON sid匹配；测试重定向同时改变writer和snapshot路径。

## 边界

- 非空缓存不校验UUID；Unix权限best-effort收紧0600，原子缓存写入失败不阻止返回且不保证跨进程稳定。
- 不替换已安装provider；Git失败只得到false，不提供branch/dirty或错误分类。
- 不归一化，回退Blocking；这些类型不执行权限判定。
- 无scope输出空session/prompt ID与缺失turn；索引try_lock失败不等待，成功值cast u32。
- 失败降为null；上下文写外层tracing字段，不覆盖payload，不直接写文件或保证subscriber落盘。
- 本层序列化不验证注释范围、不执行业务操作；多数Option None省略，DisplayRefreshProbe.hz None为null，TerminalDiagnostic可flatten。
- 公开派生字段仍由调用方赋值，本层不强制outcome与bool或delivery与reported_success一致。
- 没有Drop补发；公开ID/model/tokens字段可变，不保证异常路径成对或不可篡改关联。
- 准备值被覆盖，减法饱和0；调用方提供model_call时间，不由本层观测模型请求。
- poison恢复，guard累积直到flush；队列写入不等于持久化确认，本层无大小或数量上限。
- 返回NoOp；只初始化时检查5MiB并尝试trim，单guard slot重复构造替换旧guard。
- append文本、uptime/thread id，EnvFilter hooks=debug和agent::plugins=debug，失败None；无轮转。
- 已编译时按trim开关或路径配置，grow_memory=trace；未编译没有layer，不以debug_assertions控制。
- 模式不刷新；输出路径可由GROW_INSTRUMENTATION_LOG非空值覆盖。
- take单独Log/Chrome guard，poison跳过；不刷新其他辅助日志guard。
- 该enabled属于Layer钩子，不保证只影响自身输出；NoOp使用默认方法。
- 每次包裹此前hook，没有once门禁，本函数没有内容或路径脱敏。
- 坏行跳过，无事件返回错误；输入全量收集无上限，us优先于ms，输出覆盖非原子，线程id不可解析为0，普通timer在Chrome不自行创建span。
- 前者仍为路径0；后者为单文件路径；只有truthy debug值按session路由。
- init非幂等；单文件打开失败安装原registry后warning，按session模式安装后仅sweep一次。
- 不据此更新路由；sanitize可碰撞且无长度限制，role未经sanitize。
- 可创建额外worker且guard保留，旧sink不重开；无FD/worker上限，latest仅首开更新且同session临时名可冲突。
- 仍可能被unlink；没有FD检查、周期清理或总大小上限，最近mtime文件不按数量删除。
- 直接拒绝；Pager条目保持客户端时间/PID/version和payload，本层不认证或限制批次大小。
- None writer不由后续write自动重试；写失败warning，poison丢弃，无调用方落盘确认。
- detached丢弃直到后续维护成功；非Unix仅检测存在性，2秒窗口和大批次可超过5MiB。
- 无换行不动，唯一换行在EOF可清空；append不取同锁且可能丢失，读取无上限、无crash原子性，set_len/flush错误忽略。
- 快照空/错误None，session跳过坏行；私有PID+nanos目录非递归创建，Unix0700，冲突panic而不采用已有目录。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
