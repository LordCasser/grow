# cli 逐包核查

包路径：`crates/codegen/cli`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/codegen/cli/Cargo.toml`
- `crates/codegen/cli/build.rs`
- `crates/codegen/cli/src/main.rs`

Cargo feature：`{"default": ["jemalloc", "sandbox-enforce"], "default-bazel": ["jemalloc", "sandbox-enforce"], "jemalloc": ["dep:tikv-jemallocator", "dep:tikv-jemalloc-sys", "dep:tikv-jemalloc-ctl"], "sandbox-enforce": ["pager/sandbox-enforce"], "release-dist": ["pager/release-dist"], "distro-pm": ["update/distro-pm"]}`。

## 功能与规范映射

- [CLI composition and build identity](../specs/cli-entrypoints/spec.md#requirement-cli-composition-and-build-identity)：cli SHALL 组合pager与pager-minimal，默认jemalloc/sandbox-enforce，release-dist和distro-pm透传依赖；build生成版本与短commit。
- [CLI early utility dispatch](../specs/cli-entrypoints/spec.md#requirement-cli-early-utility-dispatch)：main SHALL 先处理mermaid worker，再解析参数；--version、doctor、du在安装运行时及普通启动钩子之前同步退出。
- [CLI runtime bounds and diagnostics startup](../specs/cli-entrypoints/spec.md#requirement-cli-runtime-bounds-and-diagnostics-startup)：普通启动 SHALL 安装minimal与观测钩子、提取指南、terminal restore及可选crash报告，建tokio运行时，结束flush并以2秒shutdown grace退出。
- [CLI jemalloc inspection hooks](../specs/cli-entrypoints/spec.md#requirement-cli-jemalloc-inspection-hooks)：Unix jemalloc模式 SHALL 提供arena purge、epoch后统计、完整stats dump及heap profiling开关/导出钩子。
- [CLI launch environment and sandbox ordering](../specs/cli-entrypoints/spec.md#requirement-cli-launch-environment-and-sandbox-ordering)：async入口 SHALL 先装TLS provider、应用cwd和debug/socket参数，completions/wrap提前分派；其他路径先固定resume目标与保存sandbox profile再应用沙箱。
- [CLI utility command routing](../specs/cli-entrypoints/spec.md#requirement-cli-utility-command-routing)：CLI SHALL 分派inspect、mcp、plugin、models、leader、worktree、sessions、export、trace、trajectory、memory、update等命令给所属模块。
- [CLI headless options propagation](../specs/cli-entrypoints/spec.md#requirement-cli-headless-options-propagation)：headless SHALL 从single/json/file构造prompt并透传session/resume/cwd/permission/trust/model/rules/fork/worktree/restore/agent/tools策略/maxturns/effort及后台等待选项。
- [CLI agent configuration and modes](../specs/cli-entrypoints/spec.md#requirement-cli-agent-configuration-and-modes)：agent SHALL 加载effective配置并应用model/effort/permission/profile/client版本/endpoint；直接stdio、serve、leader分别交shell运行，trust按cwd授信。
- [CLI leader management and discovery output](../specs/cli-entrypoints/spec.md#requirement-cli-leader-management-and-discovery-output)：leader list/info SHALL 提供描述或JSON，PID优先live验证值再lock；连接使用PID或Local目标，info请求GetLeaderInfo后cancel。
- [CLI leader bridge capabilities and forwarding](../specs/cli-entrypoints/spec.md#requirement-cli-leader-bridge-capabilities-and-forwarding)：leader stdio桥 SHALL 注册permission/model/version，关闭terminal/fs/code-nav能力；使用独立stdin reader及stdout pump，每条输出换行flush。
- [CLI session replay cache limitations](../specs/cli-entrypoints/spec.md#requirement-cli-session-replay-cache-limitations)：桥 SHALL 保存initialize和各session load原JSON，按首次出现顺序恢复；session/new待响应确认后用cwd/mcp生成load。
- [CLI replay response synchronization](../specs/cli-entrypoints/spec.md#requirement-cli-replay-response-synchronization)：重放 SHALL 等无method且id匹配的响应，吞掉该响应并转发其他消息；单次静默60秒、总180秒截止。
- [CLI sequential multi session recovery](../specs/cli-entrypoints/spec.md#requirement-cli-sequential-multi-session-recovery)：重连 SHALL 在发送锁内先initialize后逐session load；单session拒绝继续，运输失败停止剩余，返回最近活动或最后成功session。
- [CLI background update gates](../specs/cli-entrypoints/spec.md#requirement-cli-background-update-gates)：自动更新 SHALL 在debug、no_auto_update或truthy GROW_DISABLE_AUTOUPDATER关闭；显式direct stdio还要求managed安装且不用leader。
- [CLI leader periodic update decision](../specs/cli-entrypoints/spec.md#requirement-cli-leader-periodic-update-decision)：leader自动更新 SHALL 按小时回调检查并重新读取cli.auto_update，按ensure_latest_on_disk的relaunch_needed决定后续重启。
- [CLI interactive update handoff](../specs/cli-entrypoints/spec.md#requirement-cli-interactive-update-handoff)：交互启动 SHALL 背景检查并保存下载child waiter；pager退出先flush sandbox，要求更新重启时优先采用waiter，失败或缺失则blocking update。
- [CLI explicit update and leader relaunch](../specs/cli-entrypoints/spec.md#requirement-cli-explicit-update-and-leader-relaunch)：update SHALL 限制json仅用于check、version不能与check组合且需semver；安装后best effort向Reachable旧leader发RelaunchForUpdate。

## 边界

- commit回退unknown，版本优先GROW_VERSION再package；Unix才装jemalloc，release-dist配置profiling但初始inactive。
- 子命令进入async路由，可JSON输出currentVersion/channel；--version写grow版本与渠道文本，写失败exit1。
- shutdown_timeout不无限等待；默认workers=min(cores,8)，显式整数夹1..cores，非法回默认并提示。FD soft limit只best effort提高到平台目标和hard较小值。
- purge仅首次警告，stats缺失返回None，dump返回错误；部分测试在opt.prof=false时提前返回，不能据测试名声明导出已验证。
- 退出1；dashboard不强制leader，禁用时报错，否则设启动环境标志并进入交互。debug文件设置GROW_DEBUG_LOG并移除旧GROW_LOG_FILE。
- 先读取disk-only配置构建AgentConfig；具体操作契约由pager/shell所属包负责，CLI只确定入口和参数传递。
- 切换Json；headless、agent与交互入口执行版本策略，其他utility不统一执行该gate。
- profile规范化后必须文件，否则exit1；leader模式警告忽略per-process plugin dirs，直接模式使用canonical目录。serve在stderr显示secret和WebSocket URL。
- 仅清理stale lock/socket，删除错误忽略；确认为grow则交kill_process_by_pid，失败警告继续。本包不自行证明底层进程身份检测可靠性。
- 有损UTF8转换、去末尾CR/LF并跳过空行；失败保留消息每250ms重试，最多300秒或取消；父死亡绑定失败仅警告。
- pending_new只有单槽，响应未按request id匹配；close处理函数存在但生产forward预筛选不调用普通close，因此不能保证关闭会话从缓存移除。
- error键存在判拒绝，否则判成功；无id请求发送即成功。超时或stdout失败结束重放，不将收到任一通知视为load完成。
- 仍可发布grow/leader_reconnected，params为空或仅一个成功ID，不代表全部恢复。未确认new不重放，new缺cwd不能生成load。
- 视为unmanaged，不stdio自动更新；env空/0/false/off/no为false，其他true。无agent mode最终stdio但is_stdio=false，启动分支不等同显式stdio。
- 返回false维持leader；初始命令/全局gate关闭则不安装周期配置，具体调度及重启归shell。
- 本版本finish_update_on_exit直接await无独立超时；失败回退，更新路径成功才restart，失败提示用户手动update。
- 较新/相同跳过，连接/控制失败仅debug继续；check切换渠道后打印状态，底层持久化与安装由update包核查。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 逐段源码与测试审阅记录

# cli逐包审阅（进行中）

已完整阅读Cargo.toml、build.rs，以及src/main.rs 1–650行。main余下部分、所有内建测试及调用边界继续核查，包保持pending。

- cli为组合入口，连接pager与pager-minimal而避免pager反向依赖；默认jemalloc/sandbox-enforce，release-dist透传pager，distro-pm透传update。Unix jemalloc作为全局分配器，release-dist附带profiling malloc_conf但初始prof_active=false。build.rs读取git短HEAD和GROW_VERSION/CARGO_PKG_VERSION，输出VERSION_WITH_COMMIT；git失败用unknown，版本再缺失用0.0.0。
- endpoint参数仅覆盖cli_chat_proxy_base_url。agent-profile规范化并要求文件，否则stderr报错exit1。serve启动信息在stderr打印地址、secret及携key的ws URL。
- 简单tracing：headless默认off，其他默认error；RUST_LOG有效时附rmcp SSE噪音error directive，fmt stderr带ANSI、不带target，同时sampling/instrumentation/hooks/debug firehose层。
- leader管理list支持JSON或描述；info按PID/Local解析，使用Stdio client获取GetLeaderInfo并结束cancel。描述PID优先socket验证live_info再lock PID，JSON含两来源、分类、socket/lock。
- kill遍历发现项：无PID跳过，非grow进程删除stale lock/socket（删除错误忽略），grow进程交kill_process_by_pid，成功计数；此审阅未运行任何kill操作。实际进程身份可靠性归shell核查。
- env_flag_enabled仅空/0/false/off/no（trim大小写归一）false，其他true。
- StdioReplayState保存initialize原JSON、按首次出现顺序的多session、单个pending_new、last_session_id。load请求立即登记而未等成功；new请求覆盖单个pending，新响应只要含result.sessionId就消费pending，未匹配request id。close方法有删除逻辑，但已读forward函数的预筛选只含initialize/load/new，普通close不会调用缓存函数，需继续核查其他调用方。
- 重放load优先原JSON保留cwd/mcp/meta；new合成load需cwd，保留mcp，使用固定string id。重放响应以无method且id相同判定，error键存在即失败，否则成功（不强制result）。无id请求发送后立即成功。消息间60s、总180s超时，非目标响应原样stdout并flush，目标响应吞掉。
- reconnect先initialize成功再按序load所有缓存session；单load响应error跳过，运输失败中止剩余；返回last_session_id若恢复成功，否则最后一个成功sid。不能把None解释为恢复全部。
- forward_stdio_line使用UTF8有损转换、只去末尾CR/LF、空行跳过；发送失败保留line并每250ms重试，最多300s或cancel。成功send只是队列接受。shutdown flush diagnostics直接exit，无TUI转义码。run_agent_command从650行后继续。

本阶段仅源码阅读，无动态测试，不产生完成声明。

## agent调度、启动和更新补读

新增完整读取main651–2250；生产入口已读完，测试余下2251至文件末尾待核查，仍pending。

- agent启动安装信号flush任务、日志及panic hook；--trust按当前目录授信，预热HTTP/blocking pool。非显式stdio/leader模式打印版本并可检查更新；无mode虽最终stdio，is_stdio判断为false，因此走不同启动更新分支。
- AgentConfig加载effective config，覆盖model、规范effort、permission、profile、client version、endpoint，解析runtime fields。leader服务及连接模式警告忽略plugin dirs，直接模式加载canonical dirs。leader eligibility仅None/stdio，并受配置/沙箱约束。
- leader bridge能力携permission/model/version，但code_nav/terminal/fs_read/fs_write均false。注册后stdin独立reader、stdout独立pump，尝试父进程死亡绑定。重连用bounded policy和token；持有leader_tx锁替换sender并顺序重放，完成后通知connected及grow/leader_reconnected（无恢复session时params为空）。失败cancel后退出；外层select只等待任一task，未显式abort/join另一个，需结合runtime shutdown理解。
- 直接stdio交run_stdio_agent；serve配置bind/secret后run_agent_server；leader周期更新间隔1小时，执行前重新读cli.auto_update=false阻止，ensure_latest_on_disk返回relaunch_needed驱动重启，错误留活。
- fd soft limit best effort升至min(hard,macOS8192/其他Unix65536)，不降低；非Unix无操作。dashboard软命令不强迫leader，禁用时报错，否则设GROW_OPEN_DASHBOARD_AT_STARTUP并清command。
- 默认tokio workers=min(available_parallelism,8)，获取失败1；GROW_WORKER_THREADS以i128解析，数值夹到1..cores，非法或非Unicode回默认并stderr提示；显式值可超过8。运行结束shutdown_timeout2秒，不无限等blocking task。
- Unix jemalloc钩子：purge arena4096失败仅首次警告，stats先advance epoch后读取allocated/active/resident/mapped/retained/metadata，任一失败None；stats_dump输出完整文本；heap hook提供stats/prof_active/prof.dump/opt.prof。dump要求profiling且CString路径不含NUL。
- main先mermaid子进程分派，再parse；--version/doctor/du同步提前返回，未启动runtime和后续钩子。普通启动安装minimal、allocator hooks、memtrace、fd limit、提取用户指南、terminal restore及可选crash handler，展示旧崩溃并collect_crashed，建runtime；结束flush，错误restore stderr并exit1。
- async_main先TLS provider/apply_cwd，设置leader socket/debug env；completions/wrap提前返回；pin resume目标和saved sandbox，冲突exit1，再apply sandbox/dashboard；HTTP client name仅无command且无headless prompt为GrowPager，其余Generic。
- 子命令分派：version JSON/text、agent、inspect、mcp/plugin、models、leader、worktree、sessions、export、trace、trajectory、memory、update、wrap/completions/dashboard。models/worktree/sessions/trace加载disk-only配置；具体子命令逻辑归pager/shell，不在cli推断。顶层leader/no-leader配agent报错，要求agent自身旗标。
- headless解析single/json/file，解析schema后Plain自动变Json，透传session/resume/cwd/permission/trust/model/rules/continue/fork/worktree/restore_code/agent(s)/tools允许拒绝/maxturns/effort/后台等待配置。agent/headless/交互执行版本策略；其他utility分支不统一执行。
- 自动更新总gate禁debug、no_auto_update、truthy GROW_DISABLE_AUTOUPDATER。stdio仅显式stdio+直接+managed+enabled才后台更新，managed通过current_exe与grow_home应用路径规范化相等。交互启动背景检查并停放child waiter，TUI结束先sandbox flush，需更新重启时优先await waiter，失败/缺失退blocking update；该等待本身无超时，最新main更新修复需后续整合核对。
- update --json仅允许--check，check不允许version；显式version需semver。安装成功后发现Reachable且非更新版本的leader，发RelaunchForUpdate，失败仅debug并继续，client.cancel。check渠道切换是否持久化由update实现核查。
- 已读测试覆盖worker边界、版本输出/解析、allocator（opt.prof=false时部分提前return）、managed symlink、stdio gate、dashboard开关、initialize和load缓存。尚未执行cli测试。

## 完整阅读收口

main.rs全部及build/manifest已读；此前pending为历史阶段。测试覆盖顺序new/load、多会话重放、拒绝跳过、交错通知、fallback JSON和runtime退出；close测试直接调用cache函数，不经过生产forward预筛选。并发new响应关联、真实leader重启/外部客户端尚未由这些单测证明。动态测试结果见verification.md。

## 测试终态补记

`cargo test --locked -p cli --bin grow` 已结束：34 passed、0 failed、0 ignored，日志 `/tmp/grow-cli-inventory-tests.log`。默认 debug 配置；profiling 测试可能条件提前返回，不代表所有平台、特性和发布配置均验证。
