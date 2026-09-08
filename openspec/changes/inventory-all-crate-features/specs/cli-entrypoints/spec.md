## ADDED Requirements

### Requirement: CLI composition and build identity
cli SHALL 组合pager与pager-minimal，默认jemalloc/sandbox-enforce，release-dist和distro-pm透传依赖；build生成版本与短commit。

#### Scenario: CLI composition and build identity boundary
- **WHEN** git命令或版本环境不可用
- **THEN** commit回退unknown，版本优先GROW_VERSION再package；Unix才装jemalloc，release-dist配置profiling但初始inactive。

证据：`crates/codegen/cli/build.rs` — `VERSION_WITH_COMMIT`。

### Requirement: CLI early utility dispatch
main SHALL 先处理mermaid worker，再解析参数；--version、doctor、du在安装运行时及普通启动钩子之前同步退出。

#### Scenario: CLI early utility dispatch boundary
- **WHEN** 使用version子命令而非--version
- **THEN** 子命令进入async路由，可JSON输出currentVersion/channel；--version写grow版本与渠道文本，写失败exit1。

证据：`crates/codegen/cli/src/main.rs` — `dispatch_version_if_requested`。

### Requirement: CLI runtime bounds and diagnostics startup
普通启动 SHALL 安装minimal与观测钩子、提取指南、terminal restore及可选crash报告，建tokio运行时，结束flush并以2秒shutdown grace退出。

#### Scenario: CLI runtime bounds and diagnostics startup boundary
- **WHEN** blocking任务不结束或worker配置非法
- **THEN** shutdown_timeout不无限等待；默认workers=min(cores,8)，显式整数夹1..cores，非法回默认并提示。FD soft limit只best effort提高到平台目标和hard较小值。

证据：`crates/codegen/cli/src/main.rs` — `cli_worker_threads`。

### Requirement: CLI jemalloc inspection hooks
Unix jemalloc模式 SHALL 提供arena purge、epoch后统计、完整stats dump及heap profiling开关/导出钩子。

#### Scenario: CLI jemalloc inspection hooks boundary
- **WHEN** mallctl失败或profiling不可用
- **THEN** purge仅首次警告，stats缺失返回None，dump返回错误；部分测试在opt.prof=false时提前返回，不能据测试名声明导出已验证。

证据：`crates/codegen/cli/src/main.rs` — `install_heap_profile_hooks`。

### Requirement: CLI launch environment and sandbox ordering
async入口 SHALL 先装TLS provider、应用cwd和debug/socket参数，completions/wrap提前分派；其他路径先固定resume目标与保存sandbox profile再应用沙箱。

#### Scenario: CLI launch environment and sandbox ordering boundary
- **WHEN** 请求profile与保存会话冲突
- **THEN** 退出1；dashboard不强制leader，禁用时报错，否则设启动环境标志并进入交互。debug文件设置GROW_DEBUG_LOG并移除旧GROW_LOG_FILE。

证据：`crates/codegen/cli/src/main.rs` — `async_main`。

### Requirement: CLI utility command routing
CLI SHALL 分派inspect、mcp、plugin、models、leader、worktree、sessions、export、trace、trajectory、memory、update等命令给所属模块。

#### Scenario: CLI utility command routing boundary
- **WHEN** 运行models/worktree/sessions/trace
- **THEN** 先读取disk-only配置构建AgentConfig；具体操作契约由pager/shell所属包负责，CLI只确定入口和参数传递。

证据：`crates/codegen/cli/src/main.rs` — `async_main`。

### Requirement: CLI headless options propagation
headless SHALL 从single/json/file构造prompt并透传session/resume/cwd/permission/trust/model/rules/fork/worktree/restore/agent/tools策略/maxturns/effort及后台等待选项。

#### Scenario: CLI headless options propagation boundary
- **WHEN** 提供JSON schema且默认Plain输出
- **THEN** 切换Json；headless、agent与交互入口执行版本策略，其他utility不统一执行该gate。

证据：`crates/codegen/cli/src/main.rs` — `HeadlessOptions`。

### Requirement: CLI agent configuration and modes
agent SHALL 加载effective配置并应用model/effort/permission/profile/client版本/endpoint；直接stdio、serve、leader分别交shell运行，trust按cwd授信。

#### Scenario: CLI agent configuration and modes boundary
- **WHEN** 提供profile或plugin dirs
- **THEN** profile规范化后必须文件，否则exit1；leader模式警告忽略per-process plugin dirs，直接模式使用canonical目录。serve在stderr显示secret和WebSocket URL。

证据：`crates/codegen/cli/src/main.rs` — `run_agent_command`。

### Requirement: CLI leader management and discovery output
leader list/info SHALL 提供描述或JSON，PID优先live验证值再lock；连接使用PID或Local目标，info请求GetLeaderInfo后cancel。

#### Scenario: CLI leader management and discovery output boundary
- **WHEN** 执行kill且PID不再是grow进程
- **THEN** 仅清理stale lock/socket，删除错误忽略；确认为grow则交kill_process_by_pid，失败警告继续。本包不自行证明底层进程身份检测可靠性。

证据：`crates/codegen/cli/src/main.rs` — `kill_leaders`。

### Requirement: CLI leader bridge capabilities and forwarding
leader stdio桥 SHALL 注册permission/model/version，关闭terminal/fs/code-nav能力；使用独立stdin reader及stdout pump，每条输出换行flush。

#### Scenario: CLI leader bridge capabilities and forwarding boundary
- **WHEN** 输入含非法UTF8或发送channel关闭
- **THEN** 有损UTF8转换、去末尾CR/LF并跳过空行；失败保留消息每250ms重试，最多300秒或取消；父死亡绑定失败仅警告。

证据：`crates/codegen/cli/src/main.rs` — `forward_stdio_line_to_leader`。

### Requirement: CLI session replay cache limitations
桥 SHALL 保存initialize和各session load原JSON，按首次出现顺序恢复；session/new待响应确认后用cwd/mcp生成load。

#### Scenario: CLI session replay cache limitations boundary
- **WHEN** 多个new并发或关闭已缓存session
- **THEN** pending_new只有单槽，响应未按request id匹配；close处理函数存在但生产forward预筛选不调用普通close，因此不能保证关闭会话从缓存移除。

证据：`crates/codegen/cli/src/main.rs` — `cache_outgoing_acp_state`。

### Requirement: CLI replay response synchronization
重放 SHALL 等无method且id匹配的响应，吞掉该响应并转发其他消息；单次静默60秒、总180秒截止。

#### Scenario: CLI replay response synchronization boundary
- **WHEN** 响应含error或只有id无result
- **THEN** error键存在判拒绝，否则判成功；无id请求发送即成功。超时或stdout失败结束重放，不将收到任一通知视为load完成。

证据：`crates/codegen/cli/src/main.rs` — `replay_request_until_response`。

### Requirement: CLI sequential multi session recovery
重连 SHALL 在发送锁内先initialize后逐session load；单session拒绝继续，运输失败停止剩余，返回最近活动或最后成功session。

#### Scenario: CLI sequential multi session recovery boundary
- **WHEN** 全部或部分恢复失败
- **THEN** 仍可发布grow/leader_reconnected，params为空或仅一个成功ID，不代表全部恢复。未确认new不重放，new缺cwd不能生成load。

证据：`crates/codegen/cli/src/main.rs` — `replay_acp_state_after_reconnect`。

### Requirement: CLI background update gates
自动更新 SHALL 在debug、no_auto_update或truthy GROW_DISABLE_AUTOUPDATER关闭；显式direct stdio还要求managed安装且不用leader。

#### Scenario: CLI background update gates boundary
- **WHEN** current_exe无法规范化或与grow_home应用路径不同
- **THEN** 视为unmanaged，不stdio自动更新；env空/0/false/off/no为false，其他true。无agent mode最终stdio但is_stdio=false，启动分支不等同显式stdio。

证据：`crates/codegen/cli/src/main.rs` — `stdio_auto_update_enabled`。

### Requirement: CLI leader periodic update decision
leader自动更新 SHALL 按小时回调检查并重新读取cli.auto_update，按ensure_latest_on_disk的relaunch_needed决定后续重启。

#### Scenario: CLI leader periodic update decision boundary
- **WHEN** 配置关闭或下载失败
- **THEN** 返回false维持leader；初始命令/全局gate关闭则不安装周期配置，具体调度及重启归shell。

证据：`crates/codegen/cli/src/main.rs` — `LeaderAutoUpdateConfig`。

### Requirement: CLI interactive update handoff
交互启动 SHALL 背景检查并保存下载child waiter；pager退出先flush sandbox，要求更新重启时优先采用waiter，失败或缺失则blocking update。

#### Scenario: CLI interactive update handoff boundary
- **WHEN** adopted waiter未结束或完成失败
- **THEN** 本版本finish_update_on_exit直接await无独立超时；失败回退，更新路径成功才restart，失败提示用户手动update。

证据：`crates/codegen/cli/src/main.rs` — `finish_update_on_exit`。

### Requirement: CLI explicit update and leader relaunch
update SHALL 限制json仅用于check、version不能与check组合且需semver；安装后best effort向Reachable旧leader发RelaunchForUpdate。

#### Scenario: CLI explicit update and leader relaunch boundary
- **WHEN** 发现较新leader或控制请求失败
- **THEN** 较新/相同跳过，连接/控制失败仅debug继续；check切换渠道后打印状态，底层持久化与安装由update包核查。

证据：`crates/codegen/cli/src/main.rs` — `run_update_command`。


### Requirement: Shell model catalog initialization exchange

list_models SHALL 发送ACP V1 InitializeRequest，声明默认fs capability与terminal=false，并携带clientType/clientVersion meta。仅从InitializeResponse.meta.modelState反序列化SessionModelState返回；缺失或解析失败报错，不创建session、不自行探测provider或渲染列表。

#### Scenario: Absent model state
- **WHEN** InitializeResponse缺少modelState
- **THEN** 返回明确错误，不用空模型列表掩盖。

证据：`crates/codegen/shell/src/cli_models.rs` — `pub async fn list_models`。
### Requirement: Pager screen mode relaunch argv reconstruction

build_screen_mode_relaunch_args SHALL 丢弃argv0、`--`及其后全部内容、所有裸positional prompt、旧minimal/fullscreen/continue/fork-session/restore-code，以及resume/session-id/worktree/worktree-ref/ref的分离值和等号形式；短resume、session-id、worktree同样剥离。其余flag保留，是否消费后续非dash值由PagerArgs的Clap takes_values元数据动态生成并缓存，等号形式直接保留。结果末尾固定追加`--resume <session_id>`及请求方向的`--minimal`或`--fullscreen`，避免重提原prompt、重复创建worktree或保留冲突mode。

#### Scenario: Preserve ordinary option values
- **WHEN** 原argv包含model、cwd、leader-socket等空格分隔值及裸prompt
- **THEN** 保留这些flag和值，删除裸prompt并追加新的resume与mode。

#### Scenario: Strip one shot worktree directives
- **WHEN** 原argv包含worktree、ref、restore-code及旧resume
- **THEN** 这些指令及其值全部删除，不在relaunch中创建第二工作树或重复restore。

源码证据：`crates/codegen/pager/src/app/screen_mode_relaunch.rs` — `value_taking_flag_tokens / build_screen_mode_relaunch_args`。
### Requirement: Pager CLI parser command and option topology

PagerArgs SHALL 关闭Clap内建version退出并把`-v/--version`解析为可由启动层提前处理的布尔意图；根命令可选择Agent、Inspect、Doctor、Du、Leader、Mcp、Plugin、Memory、Models、Sessions、Wrap、Export、Trace、Trajectory、Update、Version、Completions、Worktree或Dashboard，未选subcommand时进入交互/headless参数面。leader-socket、debug及debug-file为global，可置于subcommand之后；leader/no-leader、minimal/fullscreen、experimental-memory/no-memory、resume/continue互斥，single/prompt-json/prompt-file/positional prompt互斥，update的alpha/stable/enterprise互斥。permission-mode仅接受ask/auto/always-approve，max-turns与background-wait-timeout必须至少1，后者默认600秒并与no-wait-for-background冲突。隐藏参数仍可显式解析；schema上的requires/conflicts只证明参数接纳，不证明具体命令已经执行。

#### Scenario: Global option after nested command
- **WHEN** `--leader-socket`或`--debug-file`出现在`agent leader/stdio`之后
- **THEN** 仍写入根PagerArgs对应字段。

#### Scenario: Conflicting prompt sources
- **WHEN** 同时给出positional prompt与`--single`
- **THEN** Clap以ArgumentConflict拒绝，不生成PagerArgs。

证据：`crates/codegen/pager/src/app/cli.rs` — `Command / PagerArgs / LeaderMgmtCommand`。

### Requirement: Pager agent transport and ephemeral plugin arguments

AgentArgs SHALL 接受model、任意reasoning-effort字符串、agent-profile、可重复plugin-dir、leader选择、proxy override及Stdio/Serve/Leader mode；leader与no-leader互斥。canonical_plugin_dirs逐项dunce canonicalize，只保留当前存在的目录，非目录或失败项向stderr警告并跳过，不去重且不要求目录位于特定根下。Serve默认绑定127.0.0.1:2419，secret可由参数或GROW_AGENT_SECRET提供并原样clone；缺失时generate_random_key从无连字符UUID文本循环取指定长度，当前调用取12个ASCII十六进制字符。LeaderArgs只表达no_exit_on_disconnect与no_auto_update开关；解析和规范化不启动transport、不加载插件或验证profile文件。

#### Scenario: Mixed plugin paths
- **WHEN** 重复参数包含目录、普通文件与不存在路径
- **THEN** 保持有效目录的输入顺序和重复项，其他项告警后跳过。

#### Scenario: Serve secret absent
- **WHEN** 参数和环境均未提供secret
- **THEN** 返回12字符随机UUID十六进制前缀作为本次调用结果。

证据：`crates/codegen/pager/src/app/cli.rs` — `AgentArgs / AgentArgs::canonical_plugin_dirs / AgentCmd / ServeArgs::get_secret / generate_random_key / LeaderArgs`。

### Requirement: Pager launch path anchoring and current directory switch

PagerArgs::apply_cwd SHALL 在改变进程cwd之前读取一次launch cwd；相对leader-socket与debug-file先连接到该launch目录，绝对路径保持绝对，所有CurDir组件被过滤，但ParentDir不解析、路径不canonicalize。无法读取launch cwd时相对值保持相对。完成两个路径变换后才对`--cwd`调用set_current_dir；失败返回包含原cwd与OS错误的错误，此函数不回滚已变换的PagerArgs值，但因按值消费，调用方拿不到该Self。未给cwd时不改变进程目录。

#### Scenario: Relative log before cwd change
- **WHEN** launch目录为/launch、debug-file为relative.log且另给cwd
- **THEN** debug-file先成为/launch/relative.log，再尝试切换cwd。

#### Scenario: Parent components
- **WHEN** 路径为logs/../debug.log或../leader.sock
- **THEN** 只删除点组件，`..`按词法保留而不解析文件系统。

证据：`crates/codegen/pager/src/app/cli.rs` — `anchor_to_launch_dir / strip_cur_dir / PagerArgs::apply_cwd / apply_cwd_from`。
