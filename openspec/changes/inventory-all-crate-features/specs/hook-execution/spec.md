## ADDED Requirements

### Requirement: Hook event catalog
Hooks SHALL 提供15个精确snake_case事件；Prompt/Tool gate分别为user_prompt_submit/pre_tool_use，Stop gate为stop/subagent_stop，其他为Observe；stop、prompt、stop_cancelled忽略配置matcher。

#### Scenario: Hook event catalog boundary
- **WHEN** 解析非规范大小写或未知事件
- **THEN** 拒绝该事件键；完整事件及载荷目录见reviews/hooks.md，类型目录不证明调用方已触发每个事件。

证据：`crates/codegen/hooks/src/event.rs` — `HookEventName`。

### Requirement: Hook envelope and payload selection
HookEventEnvelope SHALL 以camelCase序列化公共字段并flatten载荷，None可选字段省略；事件名与payload由调用方分别提供。

#### Scenario: Hook envelope and payload selection boundary
- **WHEN** 事件名与payload不对应或可选列表为Some空列表
- **THEN** 类型自身不拒绝配对不一致，空列表保留，不能假设序列化执行业务验证。

证据：`crates/codegen/hooks/src/event.rs` — `HookEventEnvelope`。

### Requirement: Hook matching values
HookPayload SHALL 按工具名、通知type、子代理type、来源、结束reason或失败类别提供matcher值；无值事件及空值允许匹配。

#### Scenario: Hook matching values boundary
- **WHEN** matcher为Never但payload没有可匹配值
- **THEN** matcher_allows仍返回true；普通regex不加锚点且没有工具别名转换。

证据：`crates/codegen/hooks/src/matcher.rs` — `matcher_allows`。

### Requirement: Hook payload clipping
载荷裁剪 SHALL 显式执行；truncate_payload超过128KiB时把序列化前缀改为带标记JSON字符串，clip_text按Unicode字符截取后追加计数。

#### Scenario: Hook payload clipping boundary
- **WHEN** 载荷超过阈值
- **THEN** 原类型可能改变，完整序列化先分配，标记和转义可能使最终值超过阈值；不是整个envelope硬上限。

证据：`crates/codegen/hooks/src/event.rs` — `truncate_payload`。

### Requirement: Hook configuration parse boundaries
JSON hook文件 SHALL 恰有hooks一个顶层键；事件/group/handler结构严格解析，合法结构内的坏matcher跳group、坏handler单独跳过并收集错误。

#### Scenario: Hook configuration parse boundaries boundary
- **WHEN** 一个事件结构错误或一个handler缺command
- **THEN** 前者拒绝整个map，后者只拒绝对应handler；其他配置层或文件可继续加载。

证据：`crates/codegen/hooks/src/config.rs` — `build_specs`。

### Requirement: Hook handler defaults and failure policy
command/http handler SHALL 分别要求command/url；超时秒饱和转换为毫秒，Stop默认600秒、其他5秒；仅两个准入事件允许显式on_failure。

#### Scenario: Hook handler defaults and failure policy boundary
- **WHEN** 非准入事件显式on_failure=allow或超时为0
- **THEN** 前者拒绝，后者接受；HookSpec.validate只拒绝非准入Block，不执行全面字段验证。

证据：`crates/codegen/hooks/src/config.rs` — `build_one_spec`。

### Requirement: Hook load time environment
配置加载 SHALL 保留raw命令和URL，剥离四个runner保留env键，再以extra优先执行单次环境展开；env缺失/null为空map，值原样保存。

#### Scenario: Hook load time environment boundary
- **WHEN** 变量不存在或值包含另一变量引用
- **THEN** 缺失引用保留，替换值不递归展开；matcher不展开，raw原文不保证没有字面密钥。

证据：`crates/codegen/hooks/src/config.rs` — `strip_reserved_env_keys`。

### Requirement: Hook environment expansion scanner
环境展开 SHALL 用随机sentinel保护识别的modifier引用并恢复，extra优先于process；扫描器不是完整shell语法解析器。

#### Scenario: Hook environment expansion scanner boundary
- **WHEN** 嵌套modifier或带quote/escape的引用
- **THEN** 不保证shell语义完全保留；裸变量和braced变量识别规则不同，嵌套只扫描首个闭括号，详细边界保留在审阅记录。

证据：`crates/codegen/hooks/src/env_expand.rs` — `expand_env_vars_with_extra`。

### Requirement: Hook source collection
Hook发现 SHALL 按global来源顺序先于project，目录内直接JSON文件按路径排序；缺失来源为空，其他读取错误收集后继续可读兄弟文件。

#### Scenario: Hook source collection boundary
- **WHEN** 来源为明确文件或目录symlink
- **THEN** 明确文件不受目录文件名过滤；目录不递归但is_file及读取跟随symlink，没有独立大小或containment保证。

证据：`crates/codegen/hooks/src/discovery.rs` — `collect_specs_from_sources`。

### Requirement: Hook registry deduplication
注册表去重 SHALL first-wins，键为event、raw command、raw URL、configured matcher、on_failure，Option空与空字符串等价。

#### Scenario: Hook registry deduplication boundary
- **WHEN** 不同source_dir/env/timeout的同raw命令
- **THEN** 仍可能折叠，忽略实际展开命令、enabled和handler_type；append_specs自身不去重。

证据：`crates/codegen/hooks/src/discovery.rs` — `registry_from_specs_deduped`。

### Requirement: Hook registry restore and mutation
HookRegistry SHALL 提供稳定事件顺序展平、追加、前缀删除与显式matcher重编译；磁盘定义是加载快照，禁用名单查询仍动态读取。

#### Scenario: Hook registry restore and mutation boundary
- **WHEN** 反序列化后尚未重编译或模式非法
- **THEN** compiled matcher缺省None；显式重编译非法模式装Never，但缺值匹配边界仍存在，Ignored事件也未被重编译函数排除。

证据：`crates/codegen/hooks/src/discovery.rs` — `recompile_matchers`。

### Requirement: Hook disabled name persistence
禁用名单 SHALL 按用户Grow home下disabled-hooks的trim非空非注释行匹配名称；查询读取错误视为未禁用。

#### Scenario: Hook disabled name persistence boundary
- **WHEN** disable或enable并发写入
- **THEN** 没有锁或原子更新保证；disable查重追加，enable截断重写，名称未限制换行。

证据：`crates/codegen/hooks/src/trust.rs` — `is_hook_disabled`。

### Requirement: Hook owned dispatch plan
规划 SHALL 按registry顺序检查validate、enabled、名单、matcher并生成owned执行或skip项，identity包含name/type/provenance。

#### Scenario: Hook owned dispatch plan boundary
- **WHEN** 计划生成后禁用名单变化
- **THEN** 已clone计划不重新查询；run_one_hook直接执行已规划spec，不重查政策，identity名称仍用户可控。

证据：`crates/codegen/hooks/src/dispatcher.rs` — `plan_dispatch`。

### Requirement: Hook admission chain
Prompt和Tool dispatch SHALL 串行运行，首个显式Deny或on_failure Block失败终止执行后续项；失败原始结果类型保留。

#### Scenario: Hook admission chain boundary
- **WHEN** 首项失败且Block，后项原本MatcherMiss
- **THEN** 最终Deny，首项仍Failed/TimedOut/Cancelled，后项统一PriorBlock；默认失败Allow可继续。

证据：`crates/codegen/hooks/src/dispatcher.rs` — `dispatch_user_prompt_submit`。

### Requirement: Hook stop signal chain
Stop dispatch SHALL 在首个block或force-stop后记录余项PriorBlock；context独自不短路但请求继续；absorb保留首个force-stop及累积信号。

#### Scenario: Hook stop signal chain boundary
- **WHEN** 先执行block而后计划有force-stop
- **THEN** 后项不执行；不能声称后面的force-stop总覆盖前面的block，wants_continuation取决于已吸收信号。

证据：`crates/codegen/hooks/src/dispatcher.rs` — `dispatch_stop`。

### Requirement: Hook observe dispatch and statistics
Observe dispatch SHALL 顺序await所有可执行项，不产生否决；统计按结果类型和elapsed求和。

#### Scenario: Hook observe dispatch and statistics boundary
- **WHEN** 调用non_blocking处理UserPromptSubmit
- **THEN** 特殊委托准入链但只返回results；non_blocking不是后台执行承诺，失败导致的政策Deny仍计failed。

证据：`crates/codegen/hooks/src/dispatcher.rs` — `dispatch_non_blocking`。

### Requirement: Hook command selection and spawn
命令runner SHALL 以空格或指定shell元字符/开头~选择shell，否则相对source_dir直接执行；子进程cwd为ctx.workspace_root，extra后注入四个身份环境键。

#### Scenario: Hook command selection and spawn boundary
- **WHEN** 仅tab/引号/通配符但无触发字符，或路径含空格
- **THEN** 前者不因此进入shell，后者进入且不自动quote；直接路径只检查exists，runner未实现独立sandbox。

证据：`crates/codegen/hooks/src/runner/command.rs` — `run_command_hook`。

### Requirement: Hook shell variable preflight
shell命令 SHALL 在spawn前拒绝扫描发现的未设置普通变量；runner键、extra、process var_os、本地赋值启发式及modifier免检。

#### Scenario: Hook shell variable preflight boundary
- **WHEN** 赋值在引用之后或字符串中出现类似赋值
- **THEN** 启发式不检查执行顺序/完整quote语法，不能作为shell静态分析保证。

证据：`crates/codegen/hooks/src/runner/command.rs` — `find_unresolved_env_vars`。

### Requirement: Hook command wait and process scope
命令等待 SHALL 并发写stdin与wait_with_output，写错忽略，等待受单handler超时限制；仅存在scope时尝试创建注册进程组。

#### Scenario: Hook command wait and process scope boundary
- **WHEN** 无scope超时或future被直接abort
- **THEN** 不能保证孙进程组清理；序列化/spawn在timeout之外，scope已关闭且注册失败返回Cancelled。

证据：`crates/codegen/hooks/src/runner/command.rs` — `hook_process_group`。

### Requirement: Hook command output capture boundary
命令结果 SHALL 在完整wait_with_output后分别截取stdout/stderr前64KiB并lossy解码；Observe只检查exit0。

#### Scenario: Hook command output capture boundary boundary
- **WHEN** 子进程输出超过64KiB
- **THEN** 收集内存仍不受该阈值限制，截断标记可能破坏JSON且最终文本略超限。

证据：`crates/codegen/hooks/src/runner/command.rs` — `truncate_output`。

### Requirement: Hook command admission output precedence
命令准入解析 SHALL 对合法JSON deny始终Deny，allow除exit2外Allow；JSON结构解析失败回退退出码0允许、2拒绝、其他失败。

#### Scenario: Hook command admission output precedence boundary
- **WHEN** 合法allow伴exit1或未知decision伴exit2
- **THEN** 当前实现前者Allow，后者Failed；不以主分支未整合修复替代当前事实。

证据：`crates/codegen/hooks/src/runner/command.rs` — `parse_blocking_result`。

### Requirement: Hook command stop output precedence
命令Stop解析 SHALL 优先采用合法Stop JSON；以左花括号开头的坏JSON失败，其他无有效JSON输出按退出码决定。

#### Scenario: Hook command stop output precedence boundary
- **WHEN** 无有效JSON且exit2
- **THEN** 用trim stderr或默认理由block；合法JSON可覆盖exit2，未知decision失败。

证据：`crates/codegen/hooks/src/runner/command.rs` — `parse_stop_result`。

### Requirement: Hook HTTP URL admission
HTTP runner SHALL 在运行时再次展开URL，只允许https，拒绝代码列举的私网/link-local/CGNAT/unspecified地址，允许loopback；所有解析地址需通过。

#### Scenario: Hook HTTP URL admission boundary
- **WHEN** DNS校验后请求再次解析或代理参与
- **THEN** 当前client未绑定已校验地址、未no_proxy；禁用重定向但不能声称消除DNS rebinding，具体地址表见审阅记录。

证据：`crates/codegen/hooks/src/runner/http.rs` — `validate_hook_url`。

### Requirement: Hook HTTP request and timing
HTTP SHALL POST完整JSON envelope，独立为校验和reqwest请求设置完整handler timeout；Observe按状态返回不读正文。

#### Scenario: Hook HTTP request and timing boundary
- **WHEN** 准入请求读取慢正文
- **THEN** 完整response.text无容量上限，elapsed固定在响应头阶段；没有共享总预算，正文超时可保留status。

证据：`crates/codegen/hooks/src/runner/http.rs` — `run_http_hook`。

### Requirement: Hook HTTP decision and metadata
HTTP Prompt/Tool SHALL 对有效decision JSON忽略status，坏JSON在2xx允许；Stop非2xx失败，2xx坏JSON为空outcome。

#### Scenario: Hook HTTP decision and metadata boundary
- **WHEN** 有效allow伴500或Stop畸形JSON伴200
- **THEN** 当前前者Allow，后者空Stop；HttpInfo保存完整expanded URL和最多200字节边界预览加标记，raw/error替换不是全面脱敏。

证据：`crates/codegen/hooks/src/runner/http.rs` — `parse_http_blocking_result`。

### Requirement: Hook executable examples
本包 SHALL 提供安全shell、递归grep限制、session/tool日志和cargo build Stop示例；示例不是完整安全策略或调用方生命周期实现。

#### Scenario: Hook executable examples boundary
- **WHEN** 运行递归grep示例自测或Stop脚本
- **THEN** 自测43项符合自身期望但quoted flag/depth/comment有识别缺口；Stop仅end_turn运行build，300秒配置，自身不实现8轮上限。

证据：`crates/codegen/hooks/examples/hooks/bin/no-recursive-grep-guard.py` — `command_is_recursive`。



### Requirement: Shell hook source assembly and trust projection
Hooks装配 SHALL 每次discover_hooks重新读取hook_config_layers；配置spec先于文件spec，随后统一first-wins去重并合并错误。全局路径解析reject_symlinks=false，configured软错误保留固定来源，硬错误省略全局来源；项目仅git_root/.grow/hooks，trusted=false时排除。as_sources按当时is_dir决定Directory否则HookFile；trust是传入布尔值，不在本函数重验。assemble_hooks仍读取全局路径和文件，非完全无IO纯函数。

#### Scenario: Untrusted project
- **WHEN** 调用方传入trusted=false
- **THEN** 只省略项目文件来源，全局来源及传入config_layers仍处理。

源码证据：
- `crates/codegen/shell/src/util/hooks.rs` — `pub fn discover_hook_source_paths`。
- `crates/codegen/shell/src/util/hooks.rs` — `pub fn discover_hooks`。
- `crates/codegen/shell/src/util/hooks.rs` — `pub fn assemble_hooks`。
- `crates/codegen/shell/src/util/hooks.rs` — `pub fn as_sources`。
### Requirement: Pager live stop hook prompt batch stashing

Live stop batches SHALL be grouped by prompt id and event name. A prompt change flushes stale groups standalone; a same-name repeat extends only when merge_same_name is true, otherwise it renders standalone; new names append in order.

#### Scenario: Prompt switch
- **WHEN** the pending prompt differs
- **THEN** stale groups flush before the new batch is created.

#### Scenario: Merge
- **WHEN** the name matches and merge is enabled
- **THEN** new runs extend the stored group.

#### Scenario: No merge
- **WHEN** the name matches and merge is disabled
- **THEN** new runs render standalone.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `stash_live_stop_batch`。

### Requirement: Pager hook occurrence dedup attachment batching and annotations

HookExecution SHALL claim occurrence identity, map all run statuses, attach live pre/post runs to the latest tool when available, render replay/loading tool hooks standalone, and route stop hooks to foreign standalone, active-turn batch, accepting terminal marker or standalone fallback. Every annotation becomes a session event after placement.

#### Scenario: Duplicate
- **WHEN** occurrence identity was already claimed
- **THEN** runs and annotations are dropped.

#### Scenario: Tool replay
- **WHEN** pre/post arrives during replay/loading
- **THEN** it renders standalone rather than attaching to an unrelated tool.

#### Scenario: Active stop
- **WHEN** a stop hook belongs to the current running turn
- **THEN** it is stashed for terminal folding.

#### Scenario: Annotation
- **WHEN** a claimed occurrence carries messages
- **THEN** each becomes a HookAnnotation event.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `HookExecution`、`stash_live_stop_batch`。

### Requirement: Shell crates/codegen/shell/src/extensions/hooks.rs extension method and user-facing command boundary contract

crates/codegen/shell/src/extensions/hooks.rs SHALL 维护 extension method and user-facing command boundary 的入口 ExtResult, ListRequest, hook_spec_to_info, ClientHookGroup, ClientHooks, ClientHookDispatch, ADVERTISED_BLOCKING_EVENTS, ADVERTISED_DECISIONS, ADVERTISED_STOP_SIGNALS, ClientHookDecision, ClientHookResponse, parse_client_hooks, reconnect_client_hooks, parse_hook_group, WireGroup, MAX_HOOK_TIMEOUT_SECS, handle, make_spec (plus 10 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** ExtResult, ListRequest, hook_spec_to_info, ClientHookGroup, ClientHooks, ClientHookDispatch, ADVERTISED_BLOCKING_EVENTS, ADVERTISED_DECISIONS, ADVERTISED_STOP_SIGNALS, ClientHookDecision, ClientHookResponse, parse_client_hooks, reconnect_client_hooks, parse_hook_group, WireGroup, MAX_HOOK_TIMEOUT_SECS, handle, make_spec (plus 10 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/extensions/hooks.rs`。
