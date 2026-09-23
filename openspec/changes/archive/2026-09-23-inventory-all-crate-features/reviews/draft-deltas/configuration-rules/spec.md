## ADDED Requirements

### Requirement: Boolean configuration source resolution
BoolFlag SHALL 按 cli > config::env_bool(env_var) > config > feature_flag(remote) > default 返回首个 Some 值与 ConfigSource；builder 初始默认 false。

#### Scenario: 显式 false
- **WHEN** 高优先级来源提供 Some(false)
- **THEN** 采用 false，不因低优先级 true 覆盖；Resolved Display 输出 value (source)，source 为 cli/env/config/remote/default。

#### Scenario: 值类型边界
- **WHEN** 使用 SessionSearchConfig 或 LazinessDetectorPerModelConfig
- **THEN** 前者 enabled 默认 None；后者 enabled=false、max_nudges_per_session=0、idle_threshold_ms/min_confidence/include_reasoning=None。本类型不启动索引或 classifier，运行默认与触发条件由 harness 解析。

证据：`crates/codegen/config-types/src/flags.rs` — `BoolFlag`；`crates/codegen/config-types/src/flags.rs` — `resolve_bool_flag`；`crates/codegen/config-types/src/flags.rs` — `Resolved`；`crates/codegen/config-types/src/flags.rs` — `SessionSearchConfig`；`crates/codegen/config-types/src/flags.rs` — `LazinessDetectorPerModelConfig`。

### Requirement: Remote settings transport schema
RemoteSettings SHALL 保存当前源码声明的全部可选远程配置字段，使用 snake_case，缺失/null 解析为 None，普通未知顶层字段忽略；None 与 Some(false)/Some(空列表) 保持区别。

#### Scenario: 字段覆盖
- **WHEN** 消费模型/推理、memory/flush/dream、工具/权限/信任、工作树/GC、UI/提示、goal/workflow、compaction 或发布渠道配置
- **THEN** 字段全集按本包 review 的 RemoteSettings 字段表核对；这里只反序列化值，不实施注释描述的优先级、开关、model catalog 或阈值 clamp。

#### Scenario: 严格字段
- **WHEN** 普通 Option<bool/u64/string> 收到错误类型
- **THEN** 可导致整个 RemoteSettings 解析失败；只有声明了 tolerant deserializer 的嵌套项有特别降级，不能概括为所有远程错误都忽略。

证据：`crates/codegen/config-types/src/lib.rs` — `RemoteSettings`。

### Requirement: Doom loop and contextual hint settings
DoomLoopRecoverySettings SHALL 提供默认 None 的 enabled/max_threshold/max_retries 并省略 None；ContextualHintsRemote 提供 undo/plan_mode/image_input/send_now/small_screen/word_select/ssh_wrap 可选 bool。

#### Scenario: 部分配置
- **WHEN** 字段缺失或存在未知键
- **THEN** 缺失项 None，未知键忽略；已知项错误类型仍失败。Doom-loop 2..64/0..5 等注释范围不在此类型 clamp，需 resolver 执行。

证据：`crates/codegen/config-types/src/lib.rs` — `DoomLoopRecoverySettings`；`crates/codegen/config-types/src/lib.rs` — `ContextualHintsRemote`。

### Requirement: Worktree age and tolerant GC schema
WorktreeKindMaxAge SHALL 接受非负整数秒、可解析 u64 的字符串、不区分大小写 never，以及 null/unit 表示 Never；序列化为整数或 never。WorktreeAutoGcSettings 提供 enabled/max_age_secs/min_interval_secs/dry_run/include_orphan_snapshots/max_age_by_kind/include_rebuild/rebuild_min_interval_secs。

#### Scenario: 按种类配置
- **WHEN** max_age_by_kind 为对象
- **THEN** 保留可解析条目到 BTreeMap，坏项跳过，null 值为 Never；未知种类名称在此仍保留，不在 DTO 执行 GC。非对象 map 值降为 None。

#### Scenario: 容错边界
- **WHEN** bool/u64 配置遇到错误标量或数组/对象
- **THEN** 错误标量变 None，但 visitor 未实现 map/seq，数组/对象使 WorktreeAutoGcSettings 解析失败；RemoteSettings 的外层 tolerant wrapper 再警告并丢弃整个 worktree_auto_gc，兄弟 enabled=false 也可能丢失。

证据：`crates/codegen/config-types/src/lib.rs` — `WorktreeKindMaxAge`；`crates/codegen/config-types/src/lib.rs` — `WorktreeAutoGcSettings`；`crates/codegen/config-types/src/lib.rs` — `de_opt_max_age_by_kind_tolerant`；`crates/codegen/config-types/src/lib.rs` — `deserialize_tolerant_worktree_auto_gc`。

### Requirement: Display refresh preservation and tag fallback
DisplayRefreshSettings SHALL 提供可选 probe_enabled/auto_cadence_enabled/floor_ms/ceiling_ms/min_hz/max_hz，将未知键保留在 extra，只有所有字段 None 且 extra 空时 is_default=true。

#### Scenario: 错误值
- **WHEN** 已知 bool/u32 字段类型错误
- **THEN** 错误标量丢为 None，负数/超 u32 整数丢为 None；数组/对象仍失败，RemoteSettings.display_refresh 没有外层 tolerant wrapper，可导致整体解析失败。未知 extra 可再次序列化保留。

#### Scenario: 命令标签
- **WHEN** slash_command_tags 有任何非字符串值或错误容器
- **THEN** 整个 map 被警告并降为 None，而非仅丢弃坏条目；成功时保存 BTreeMap，不在此合并本地配置。

证据：`crates/codegen/config-types/src/lib.rs` — `DisplayRefreshSettings`；`crates/codegen/config-types/src/lib.rs` — `de_opt_u32_tolerant`；`crates/codegen/config-types/src/lib.rs` — `de_opt_bool_tolerant`；`crates/codegen/config-types/src/lib.rs` — `deserialize_tolerant_slash_command_tags`。

### Requirement: Permission rule configuration shape
PermissionConfig SHALL 默认空 rules；PermissionRule 要求 action，tool 默认 Any、pattern 默认 None、pattern_mode 默认 Glob；枚举 wire 使用 lowercase。

#### Scenario: 缺失动作
- **WHEN** serde 解析规则没有 action
- **THEN** 失败，不能因 RuleAction::default 为 Deny 就声称省略 action 自动 Deny；显式 action 为 allow/deny/ask。

#### Scenario: 工具和匹配
- **WHEN** 提供 ToolFilter 或 PatternMode
- **THEN** 支持 any/bash/edit/read/grep/mcp/webfetch，以及 glob/domain；这里只保存规则，不执行匹配和授权。

证据：`crates/codegen/config-types/src/permission.rs` — `PermissionRule`；`crates/codegen/config-types/src/permission.rs` — `RuleAction`；`crates/codegen/config-types/src/permission.rs` — `ToolFilter`；`crates/codegen/config-types/src/permission.rs` — `PatternMode`。

### Requirement: Worktree pool configuration defaults
PoolConfig SHALL 默认 enabled=true、pool_size=2、file_count_threshold=50000、parallelism=3；空对象与 Default 一致，部分覆盖保留其他默认。

#### Scenario: 范围
- **WHEN** 提供 pool_size/parallelism 为 0
- **THEN** usize 类型接受，未在此 clamp 或创建 worker；是否启用池及容量执行继续在工作树实现核查。

证据：`crates/codegen/config-types/src/pool.rs` — `PoolConfig`。

### Requirement: Memory index embedding and search defaults
Memory 配置 SHALL 默认 index 最大 1600/重叠320 字符，embedding provider=api/model=None/dimensions=1024；search 默认 max_results=6、min_score=.35、vector/text weight=.7/.3，workspace/session/global source_weights 各 1。

#### Scenario: 搜索严格性
- **WHEN** MemorySearchConfig 出现未知字段
- **THEN** 拒绝；source_weights 仍为任意字符串 map，不在此校验名称或归一化权重。

#### Scenario: 衰减与多样性
- **WHEN** 使用 temporal_decay 或 mmr
- **THEN** 默认 decay enabled=true/half_life_days=7，effective_half_life_days 在 disabled 或 <=0 时 None（<=0 警告）；MMR 默认 disabled/lambda=.7，反序列化 lambda clamp 到 0..1。公开字段直接构造绕过 clamp，half-life 未显式拒绝 NaN。

证据：`crates/codegen/config-types/src/memory.rs` — `MemoryIndexConfig`；`crates/codegen/config-types/src/memory.rs` — `MemoryEmbeddingConfig`；`crates/codegen/config-types/src/memory.rs` — `MemorySearchConfig`；`crates/codegen/config-types/src/memory.rs` — `effective_half_life_days`；`crates/codegen/config-types/src/memory.rs` — `MmrConfig`。

### Requirement: Memory lifecycle and compaction flush configuration
Memory 生命周期类型 SHALL 默认 initial_injection enabled=true/min_score=None、session save_on_end=true、dream enabled=true/min_hours=4/min_sessions=3/stale_lock_secs=3600/check_interval_secs=None、watcher enabled=true/stale_claim_secs=60、GC max_age_days=30。

#### Scenario: flush 默认
- **WHEN** 构造 MemoryFlushConfig 默认值
- **THEN** enabled=true、soft_threshold_tokens=4000、flush_model=None、max_flush_write_chars=8000、idle_timeout_secs=None、semantic_dedup_threshold=None；显式 dedup 浮点反序列化 clamp 到 0..1。

#### Scenario: 实现分工
- **WHEN** 配置宣告 watcher/dream/save/flush enabled
- **THEN** 这里只保存配置，不能证明 watcher 启动、自动保存或定时 consolidation 已执行；watcher 拒绝未知字段，其他这些结构一般忽略未知字段。

证据：`crates/codegen/config-types/src/memory.rs` — `MemoryInitialInjectionConfig`；`crates/codegen/config-types/src/memory.rs` — `MemorySessionConfig`；`crates/codegen/config-types/src/memory.rs` — `MemoryDreamConfig`；`crates/codegen/config-types/src/memory.rs` — `MemoryWatcherConfig`；`crates/codegen/config-types/src/memory.rs` — `MemoryGcConfig`；`crates/codegen/config-types/src/memory.rs` — `MemoryFlushConfig`。

### Requirement: MCP transport and operational config values
McpServerConfig SHALL flatten untagged transport：先尝试含 command 的 Stdio(args 默认空、可选 env/cwd)，再尝试必需 url 的 HTTP(可选 type/bearer_token_env_var/headers)；enabled 默认 true、max_access 默认 ToolAccess::All。

#### Scenario: 无 transport 或空白
- **WHEN** 配置缺少 command/url 或值为空白
- **THEN** 无 transport 反序列化失败，即使 enabled=false；空白字符串可解析，blank_transport_field 以 trim 检出，但须 caller 显式使用。

#### Scenario: 附加选项
- **WHEN** 配置 setup/startup_timeout_sec/tool_timeout_sec/tool_timeouts/expose_image_base64
- **THEN** 保存可选值；本层不执行超时、输出策略或 ToolAccess 权限。KNOWN_MCP_SERVER_FIELDS 提供已知键表，problem DTO 用 camelCase 并区分 error/warning。

证据：`crates/codegen/config-types/src/mcp.rs` — `McpServerTransportConfig`；`crates/codegen/config-types/src/mcp.rs` — `McpServerConfig`；`crates/codegen/config-types/src/mcp.rs` — `blank_transport_field`；`crates/codegen/config-types/src/mcp.rs` — `KNOWN_MCP_SERVER_FIELDS`；`crates/codegen/config-types/src/mcp.rs` — `McpServerConfigProblem`。

### Requirement: MCP setup select and preferences
resolve_setup SHALL 无 setup 时返回 config clone；有 setup 时仅允许恰一个非空 Select，按 stored preferences 的 field.id 查值并要求匹配某个 option，缺失或不匹配返回 Required。

#### Scenario: 默认与派生
- **WHEN** field 有 default/required 或变量映射
- **THEN** default/required 不绕过偏好要求；derived.from 必须等于唯一 field.id，否则 Invalid，选值无 map 返回 Required。成功只把派生变量注入模板，不自动注入原 field 值。

#### Scenario: 偏好文件
- **WHEN** 构造 McpPreferencesFile::default
- **THEN** version=1、servers 空；serde 文件 version 必需，类型不做版本兼容验证或持久化。每 server 有 values、可选 source(kind/plugin/scope)/updatedAt。

证据：`crates/codegen/config-types/src/mcp.rs` — `resolve_setup`；`crates/codegen/config-types/src/mcp.rs` — `McpPreferencesFile`；`crates/codegen/config-types/src/mcp.rs` — `McpServerPreferences`；`crates/codegen/config-types/src/mcp.rs` — `McpSetupResolution`。

### Requirement: MCP template expansion scope
setup 模板 SHALL 将 {{key}} 的 trim 后 key 查派生变量，只替换一层，未闭合或未解析变量返回 Invalid；成功 clone 清除 setup。expand_strings 接受 caller 替换函数并就地处理相同字符串位置。

#### Scenario: 替换字段
- **WHEN** transport 为 stdio 或 HTTP
- **THEN** stdio 替换 command、args、env 值、cwd；HTTP 替换 url 和 headers 值。不替换 map 键、bearer_token_env_var、transport_type 或 setup schema，不在此执行 shell escaping。

证据：`crates/codegen/config-types/src/mcp.rs` — `render_setup_template`；`crates/codegen/config-types/src/mcp.rs` — `render_setup_templates`；`crates/codegen/config-types/src/mcp.rs` — `expand_strings`。

### Requirement: MCP ACP conversion boundaries
to_acp_mcp_server SHALL 在 disabled 或 setup 仍存在时返回 None；stdio 转 command PathBuf/args/env，HTTP 携带 headers 并可从指定环境变量追加 Authorization Bearer token。

#### Scenario: 协议选择
- **WHEN** HTTP type 不区分大小写等于 sse 或 URL 精确以 /sse 结束
- **THEN** 转 Sse，否则 Http；只拒绝空 URL，不拒绝纯空白，也不验证 URI/header。缺 bearer 环境变量仅警告继续，既有 Authorization 可能与追加项并存。

#### Scenario: 字段丢失
- **WHEN** 转换 stdio cwd 或 timeout/max_access/expose_image_base64
- **THEN** 这些值未进入返回的 ACP server，须通过其他 caller 通路处理；不证明工作目录、权限或超时已传播。McpConfig 以 mcpServers 的 IndexMap 保存声明顺序。

证据：`crates/codegen/config-types/src/mcp.rs` — `to_acp_mcp_server`；`crates/codegen/config-types/src/mcp.rs` — `McpConfig`。

### Requirement: Environment boolean vocabulary
环境布尔解析 SHALL trim并忽略ASCII大小写，识别1/true/yes/on/enabled与0/false/no/off/disabled。

#### Scenario: 未知值
- **WHEN** 输入为空、未知、缺失或非Unicode
- **THEN** 返回None，不擅自选true或false。

证据：`crates/codegen/config/src/lib.rs` — `env_bool`。

### Requirement: Grow home and executable paths
Grow home SHALL 由字符串GROW_HOME或默认home下.grow构造并OnceLock缓存；默认home先canonicalize，无home回退相对路径，mkdir失败不阻止返回。

#### Scenario: 应用程序位置
- **WHEN** 构造application路径
- **THEN** 指向home下bin的grow或grow.exe。

#### Scenario: 环境与缓存
- **WHEN** 设置非Unicode GROW_HOME或缓存后修改环境
- **THEN** user_grow_home的var_os检查不等于grow_home字符串解析；已缓存路径不重新计算。

证据：`crates/codegen/config/src/paths.rs` — `grow_home`；`crates/codegen/config/src/paths.rs` — `user_grow_home`。

### Requirement: Session cwd encoding and marker decoding
CWD编码 SHALL 在URL编码长度不超过255时使用编码文本，否则采用最多40字符slug加BLAKE3前16hex；空slug用workspace。

#### Scenario: 长路径解码
- **WHEN** 编码本身不是可识别绝对路径
- **THEN** 读取目录中的.cwd，拒绝末端目录链接及marker链接、非文件、超过1MiB；Unix使用NOFOLLOW和限长读取。

#### Scenario: marker边界
- **WHEN** marker读取成功
- **THEN** trim后返回，不校验hash对应或绝对性；sessions目录构造不负责写marker，父路径竞态未完全排除。

证据：`crates/codegen/config/src/paths.rs` — `sessions_cwd_dir`；`crates/codegen/config/src/paths.rs` — `.cwd`。

### Requirement: Basic atomic file replacement
基础原子写入 SHALL 使用同目录basename.pid.counter.tmp，create_new后write_all再rename，支持可选Unix mode。

#### Scenario: 写入失败
- **WHEN** 创建、写入或rename失败
- **THEN** 返回错误并尝试删除tmp；当前create_new碰撞失败也会删除同名tmp，不保证清理对象归属。

#### Scenario: 持久性边界
- **WHEN** rename成功
- **THEN** 不额外创建父目录、fsync或检查目标是否并发变化；mode仍受umask影响。

证据：`crates/codegen/config/src/fs_atomic.rs` — `write_atomically`。

### Requirement: User configuration loading and expansion
配置加载 SHALL 对缺失文件返回空table，对其他I/O或TOML语法错误记录并返回；字符串值递归展开环境变量而不展开键，随后应用版本覆盖。

#### Scenario: 语法错误定位
- **WHEN** 解析TOML失败
- **THEN** 按字符计算1-based行列，保留错误。

#### Scenario: 磁盘范围
- **WHEN** load_from_disk被调用
- **THEN** 只读取user home配置，不在此层合并项目配置；环境展开使用no_errors语义。

证据：`crates/codegen/config/src/loader.rs` — `load_from_disk`；`crates/codegen/config/src/loader.rs` — `load_toml_file`。

### Requirement: Hook configuration layer loading
用户Hook配置层 SHALL 单独读取user配置、应用版本覆盖并提取hooks table，保留provenance/source_name/path。

#### Scenario: 展开与来源
- **WHEN** 读取Hook layer
- **THEN** 不展开环境变量，不扫描项目或应用campaigns。

#### Scenario: 无效来源
- **WHEN** 读取、解析或hooks类型错误
- **THEN** 警告并跳过，不构造有效hooks层。

证据：`crates/codegen/config/src/loader.rs` — `HookConfigLayer`。

### Requirement: Recursive configuration patch semantics
配置合并 SHALL 只在两侧均为table时递归，否则整值替换；按patch迭代顺序后者覆盖，canonical strip只移除顶层version_overrides/campaigns/auth_provider/provider。

#### Scenario: 数组覆盖
- **WHEN** 两个patch设置同一数组
- **THEN** 后一个整数组替换，不逐元素合并。

#### Scenario: 路径触及与消费
- **WHEN** 祖先被非table替换或patch数组反序列化失败
- **THEN** 非table祖先触及全部后代，空路径不触及；take_patch_array先移除section，失败也已消费。

证据：`crates/codegen/config/src/loader.rs` — `deep_merge_toml`；`crates/codegen/config/src/config_override.rs` — `take_patch_array`；`crates/codegen/config/src/config_override.rs` — `patch_touches_path`。

### Requirement: Version constrained configuration precedence
版本覆盖 SHALL 先解析全部trim后的semver边界，再按minimum稳定升序应用匹配patch；缺minimum为0.0.0，上下界含等号，同minimum靠后声明获胜。

#### Scenario: 错误版本
- **WHEN** 任一边界非法
- **THEN** 不合并任何patch，但version_overrides section已经移除。

#### Scenario: 空匹配范围
- **WHEN** minimum大于maximum或installed版本非法
- **THEN** 前者不匹配；后者剥离section并成功返回，不应用覆盖。

证据：`crates/codegen/config/src/version_overrides.rs` — `minimum`；`crates/codegen/config/src/loader.rs` — `installed_semver`。

### Requirement: Campaign extraction and priority
Campaign SHALL trim且要求非空id，忽略空patch，数组结构错误时忽略整组；merge按source顺序首次id胜出，filter去除dismissed并保序，apply逆序使列表靠前者赢leaf。

#### Scenario: 磁盘配置层
- **WHEN** ConfigLayers加载user配置
- **THEN** 提取user campaigns后按dismissed应用，不隐式调用merge去重。

#### Scenario: 受影响字段
- **WHEN** 查询campaign触及路径
- **THEN** 使用patch祖先替换语义，祖先非table替换也计入。

证据：`crates/codegen/config/src/campaigns.rs` — `ConfigLayers`；`crates/codegen/config/src/campaigns.rs` — `ids_touching_paths`。

### Requirement: Campaign enablement and dismissed state
Campaign启用 SHALL 同时受GROW_CAMPAIGNS和base.features.campaigns控制，任一显式false均禁用。

#### Scenario: 环境显式启用
- **WHEN** GROW_CAMPAIGNS=true而本地配置false
- **THEN** 仍禁用。

#### Scenario: 状态文件失败
- **WHEN** home下campaigns_state.json缺失或无法解析
- **THEN** 视为无dismissed id；本层加载不负责持久化dismiss操作。

证据：`crates/codegen/config/src/loader.rs` — `GROW_CAMPAIGNS`；`crates/codegen/config/src/loader.rs` — `campaigns_state.json`。

### Requirement: Platform shell selection
Unix shell种类 SHALL 仅由SHELL是否包含zsh选择Zsh或Bash；每种路径缓存按同basename且可执行的GROW_SHELL、SHELL、which、固定目录、硬编码路径依次选择。

#### Scenario: 可执行探测
- **WHEN** metadata不能证明任意执行位
- **THEN** 可能运行detached且null stdio的--version，当前没有deadline；环境路径可以是相对路径。

#### Scenario: 非Unix选择
- **WHEN** 检测Windows shell
- **THEN** 识别显式pwsh/powershell/bash别名/cmd，自动依次where pwsh、固定PowerShell、Git Bash、PowerShell fallback；where也无deadline，显式部分值不验证存在。

证据：`crates/codegen/config/src/shell.rs` — `GROW_SHELL`；`crates/codegen/config/src/shell.rs` — `--version`。

### Requirement: Shell invocation and capability helpers
Shell辅助 SHALL 分别返回Unix utilities、ampersand、chain separator与调用参数；命令存在性使用父进程PATH的which。

#### Scenario: Windows调用
- **WHEN** 选择Git Bash、PowerShell或Cmd
- **THEN** 分别-c并给MSYS转换禁用变量、-NoProfile -NonInteractive -Command、/C；Python UTF8环境由调用方实际应用。

#### Scenario: separator当前行为
- **WHEN** 查询连接符
- **THEN** Unix/pwsh/Git Bash为&&，PowerShell和Cmd为分号；此返回值不证明Cmd分号具备预期连接语义。

证据：`crates/codegen/config/src/shell.rs` — `PYTHONIOENCODING`；`crates/codegen/config/src/shell.rs` — `MSYS_NO_PATHCONV`。

### Requirement: Global hook source inventory
全局Hook来源 SHALL 在无home时返回空；有home时始终列出固定hooks目录及hooks-paths registry，registry本身不参与目录发现。

#### Scenario: 配置目录
- **WHEN** registry包含空行、相对路径或重复绝对路径
- **THEN** trim后忽略空行和相对路径，按词法路径首次去重；缺失configured路径按目录处理。

#### Scenario: 来源不完整
- **WHEN** registry读取出现非NotFound错误
- **THEN** 保留固定来源并标记incomplete；拒绝symlink模式遇见链接返回硬错误，不交付部分列表。

证据：`crates/codegen/config/src/global_hook_sources.rs` — `hooks-paths`；`crates/codegen/config/src/global_hook_sources.rs` — `is_incomplete`。

### Requirement: Hook source path and file constraints
Hook slots SHALL 创建并要求真实hooks目录和registry文件，新registry在Unix使用0600与NOFOLLOW，现有权限不收紧。

#### Scenario: 路径组件检查
- **WHEN** 检查symlink链
- **THEN** metadata错误使检查停止，六个/tmp、/var、/etc及/private对应路径被豁免；检查非handle-relative，不保证消除竞态。

#### Scenario: 文件发现与验证
- **WHEN** 列出目录Hook JSON文件
- **THEN** 只列一层非隐藏小写.json且名称长度大于5，排序；直接校验拒绝链接、非普通文件及Unix多硬链接，不解析JSON，现有configured普通文件不作为目录扫描。

证据：`crates/codegen/config/src/global_hook_sources.rs` — `path_has_symlink`；`crates/codegen/config/src/global_hook_sources.rs` — `validate_direct_hook_json_file`；`crates/codegen/config/src/global_hook_sources.rs` — `validated_hook_json_files_for_sources`。

### Requirement: Hook ancestor pin planning
Hook祖先计划 SHALL 收集真实现存目录链，在缺失、symlink或非目录停止，去重按深度排序，跳过已经mount的目录但继续祖先。

#### Scenario: Linux mount判断
- **WHEN** 路径为根、设备变化或mountinfo条目
- **THEN** 据此识别mount，mountinfo路径不解码八进制转义。

#### Scenario: 其他平台及执行边界
- **WHEN** 生成pin列表
- **THEN** 其他平台mount检测返回false；本层只产生计划，不执行挂载。

证据：`crates/codegen/config/src/global_hook_sources.rs` — `ancestors_to_pin`；`crates/codegen/config/src/global_hook_sources.rs` — `existing_ancestor_chain`。

### Requirement: Managed configuration planning and inspection
ManagedConfig SHALL 在plan验证request和source，生成不可变updated bytes、目标路径与inspection；requested item状态为Absent、Exact或NeedsUpdate。

#### Scenario: inspection范围
- **WHEN** 已有outer包含未请求项目
- **THEN** unmanaged_text排除整个outer，render保留未请求项目；managed_block来自updated内容。

#### Scenario: 计划与应用间隔
- **WHEN** verify_unchanged或apply执行
- **THEN** 重新验证parent、路径解析与source；backup/temp hints只是候选，实际路径由apply返回。

证据：`crates/codegen/config/src/managed_text/mod.rs` — `ManagedConfig`；`crates/codegen/config/src/managed_text/mod.rs` — `verify_unchanged`。

### Requirement: Managed configuration source snapshots
托管配置source SHALL 要求普通文件、metadata大小不超过4MiB、无NUL且UTF8；snapshot比较bytes、BLAKE3、mode和identity。

#### Scenario: 链接解析
- **WHEN** 初始目标缺失、最终链接或父链接
- **THEN** 初始缺失可创建，最终链接最多40次且检测循环，跟随后dangling拒绝；初始父链接经canonicalize物理化。

#### Scenario: 身份与读取边界
- **WHEN** 源发生并发变化
- **THEN** Unix使用dev/ino及07777 mode，其他平台len/mtime；metadata与read分离，实际read不再次限长，不能据此保证无TOCTOU。

证据：`crates/codegen/config/src/managed_text/source.rs` — `MAX_CONFIG_BYTES`；`crates/codegen/config/src/managed_text/source.rs` — `MAX_SYMLINKS`；`crates/codegen/config/src/managed_text/source.rs` — `SourceState`。

### Requirement: Managed parent anchoring
托管配置父路径 SHALL 捕获现存真实目录链，apply创建缺失目录后重验既有前缀并建立anchor。

#### Scenario: 父路径改变
- **WHEN** 既有parent identity或新建路径类型变化
- **THEN** 拒绝继续发布。

#### Scenario: 持久化边界
- **WHEN** 同步anchor
- **THEN** Unix同步打开的目录，非Unix为空操作；路径metadata与打开句柄没有原子身份绑定。

证据：`crates/codegen/config/src/managed_text/source.rs` — `ParentPlan`；`crates/codegen/config/src/managed_text/source.rs` — `ParentAnchor`。

### Requirement: Managed request and marker grammar
托管文本请求 SHALL 使用非空无CR/LF的comment prefix；namespace、owned prefix和item name仅允许ASCII字母数字及点、下划线、横线、空格，items非空且名称唯一，body禁CR与marker候选行。

#### Scenario: 结构错误
- **WHEN** 重复、反向、嵌套、错配、未闭合outer/item或outer内裸内容
- **THEN** 拒绝；outer至多一对，item名称必须属于owned prefix且不等于namespace。

#### Scenario: 所有权范围
- **WHEN** outer外出现owned prefix item或畸形owned候选
- **THEN** 拒绝，即使不属于本次请求；候选要求行首comment prefix后有空格/tab再接chevrons，普通嵌入文本保持惰性。

证据：`crates/codegen/config/src/managed_text/format.rs` — `validate_request`；`crates/codegen/config/src/managed_text/format.rs` — `parse_block`；`crates/codegen/config/src/managed_text/format.rs` — `marker_candidate`。

### Requirement: Managed newline and item preservation
托管文本渲染 SHALL 拒绝裸CR与混合LF/CRLF，默认LF并保留原文件换行形式、外围文本与未请求item。

#### Scenario: 新增或替换item
- **WHEN** 请求合法named item
- **THEN** 已有则替换，无则插outer关闭前，无outer则追加；body尾部LF去除后转换为文件换行。

#### Scenario: 末尾换行与精确状态
- **WHEN** 原文件为空或无末尾换行
- **THEN** 新文本保持无末尾换行；Exact按去掉section尾CR/LF后的规范文本比较，空文件新块也无末尾换行。

证据：`crates/codegen/config/src/managed_text/format.rs` — `render_update`；`crates/codegen/config/src/managed_text/format.rs` — `detect_newline`；`crates/codegen/config/src/managed_text/format.rs` — `item_state`。

### Requirement: Managed transaction reservation and publication
变化的托管配置apply SHALL 获取持久同级.grow.lock的阻塞文件锁，重验source后create_new保留backup/temp，写入bytes和Unix mode并sync，再运行可选validator及重验后rename发布。

#### Scenario: 碰撞与无变化
- **WHEN** 候选artifact已存在或plan无变化
- **THEN** 碰撞最多128次尝试且不删除碰撞文件；NoChange仍确保及重验parent/source，但不锁、不运行validator、不重写。

#### Scenario: 锁及权限边界
- **WHEN** 执行变化事务
- **THEN** 锁无超时且open不做NOFOLLOW/type校验；保留mode不保证owner/ACL，新目标Unix mode为0644，不是持久事务日志。

证据：`crates/codegen/config/src/managed_text/transaction.rs` — `apply`；`crates/codegen/config/src/managed_text/transaction.rs` — `128`。

### Requirement: Managed transaction verification and recovery
托管配置发布后 SHALL 重验父目录、同步并核对目标bytes和mode，失败则恢复原文件或删除新目标并再同步验证。

#### Scenario: 发布前失败
- **WHEN** 备份、临时写入、验证或publish失败
- **THEN** 尽力清理本次backup/temp并返回错误，父目录及锁文件可保留。

#### Scenario: 恢复结果
- **WHEN** 发布后校验或阶段失败
- **THEN** 回滚成功删除backup返回原错误；回滚失败返回Recovery包含两类错误并保留backup；成功apply保留实际backup，不保证排除外部并发覆盖。

证据：`crates/codegen/config/src/managed_text/transaction.rs` — `Recovery`；`crates/codegen/config/src/managed_text/tests.rs` — `post_publish_failures_rollback_existing_and_remove_new_target`；`crates/codegen/config/src/managed_text/tests.rs` — `primary_and_rollback_errors_are_both_reported`。

### Requirement: Managed syntax validator lifecycle
托管配置validator SHALL 将temp路径追加给program/args，null stdio并detach，按10ms轮询退出；非零退出失败，group建立或attach失败可降级。

#### Scenario: 超时或wait错误
- **WHEN** spawn/attach之后计时达到timeout或try_wait失败
- **THEN** 尝试group terminate、50ms后kill，再child kill及最多1秒reap，清理错误附带返回。

#### Scenario: 保证边界
- **WHEN** validator正常退出
- **THEN** Unix不显式清理仍活后代；不捕获stderr，不保证spawn本身有界，不把group失败降级当完整进程树清理。

证据：`crates/codegen/config/src/managed_text/validator.rs` — `SyntaxValidator`；`crates/codegen/config/src/managed_text/tests.rs` — `validator_timeout_is_bounded`。


### Requirement: Shell memory section fallback and remote overlay

MemoryConfig::resolve SHALL 尝试将整个memory节反序列化，失败回退整组默认；flush单独从compaction.memory_flush成功解析才覆盖，memory中的flush/root_dir_override/flat_memory_root被serde跳过。远程search、initial_injection、embedding、watcher、dream及compaction flush仅在对应原始本地节不存在时覆盖指定字段，检查的是节存在而非有效性；remote MMR lambda与semantic dedup阈值clamp到0..1，其他字段按代码直接赋值。enabled委托resolve_enabled后由no_memory最后强制false。

#### Scenario: Invalid but present local search
- **WHEN** memory.search存在但导致memory反序列化失败
- **THEN** 使用默认memory，同时阻止远程search覆盖，不能视为不存在本地节。

证据：`crates/codegen/shell/src/config/mod.rs` — `pub struct MemoryConfig`；`crates/codegen/shell/src/config/mod.rs` — `pub fn resolve`。

补充测试证据（本轮仅阅读，未执行）：`crates/codegen/shell/src/config/tests.rs` — `fn memory_config_partial_toml_uses_defaults_for_missing`；`crates/codegen/shell/src/config/tests.rs` — `fn memory_config_local_initial_injection_overrides_remote`；`crates/codegen/shell/src/config/tests.rs` — `fn memory_config_full_toml_parsing`。

补充测试证据（仅静态阅读）：`crates/codegen/shell/src/config/tests.rs` — `fn flush_semantic_dedup_threshold_clamped_from_remote`；`crates/codegen/shell/src/config/tests.rs` — `fn memory_dream_config_remote_ignored_when_toml_present`；`crates/codegen/shell/src/config/tests.rs` — `fn memory_config_local_search_blocks_remote_temporal_decay_and_mmr`。

### Requirement: Shell subagent configuration fallback and depth resolution

SubagentsConfig SHALL 拒绝未知字段，resolve遇整个节解析失败则默认整组值，仍按原始节存在性传递enabled配置；默认struct.enabled=false，resolve在节缺失时向通用resolver提供默认true，不能将这两个默认混为一谈。单agent toggle缺失返回true，不合并全局enabled。resolve_max_depth按有效env整数、TOML、remote、默认1选择，env trim后无效则告警回退；各来源clamp到1..=u32::MAX。resolve本身不调用resolve_max_depth或核对models值是否在catalog。

#### Scenario: Invalid environment depth
- **WHEN** env不是i64整数而TOML max_depth=2
- **THEN** 忽略env并返回2，不直接回退默认1。

证据：`crates/codegen/shell/src/config/mod.rs` — `pub fn resolve_max_depth`；`crates/codegen/shell/src/config/mod.rs` — `pub fn is_subagent_enabled`；`crates/codegen/shell/src/config/mod.rs` — `pub struct SubagentsConfig`。

补充测试证据（仅静态阅读）：`crates/codegen/shell/src/config/tests.rs` — `fn subagents_max_depth_env_beats_toml_and_remote`；`crates/codegen/shell/src/config/tests.rs` — `fn subagents_max_depth_invalid_env_falls_through`；`crates/codegen/shell/src/config/tests.rs` — `fn subagents_config_parses_negative_max_depth_without_dropping_section`。

### Requirement: Shell auxiliary model override blank value asymmetry

ModelOverrideConfig::resolve SHALL 从ModelsConfig读取并trim空白；session_title/image_description远程值仅在相应原始本地键不存在时应用，prompt_suggestion则在本地解析后Unpinned时应用远程值。title/image环境变量存在即覆盖，空白可清除已有配置；suggestion环境变量仅非空才覆为Env，空白保留既有pin。CLI title存在时最后覆盖且空白可清除。解析阶段只保留Env/Pinned/Unpinned来源，不执行catalog校验或采样。

#### Scenario: Blank suggestion environment
- **WHEN** 已有本地或remote suggestion pin，环境变量仅空白
- **THEN** 保留已有pin；同样空白title环境变量则清除title。

证据：`crates/codegen/shell/src/config/mod.rs` — `impl ModelOverrideConfig`；`crates/codegen/shell/src/config/mod.rs` — `fn non_empty_model_override`。

补充测试证据（仅静态阅读）：`crates/codegen/shell/src/config/tests.rs` — `fn model_overrides_prompt_suggestion_blank_values_are_unset`；`crates/codegen/shell/src/config/tests.rs` — `fn model_overrides_cli_session_title_overrides_everything`。

### Requirement: Shell ordered environment key resolution

EnvKeys SHALL 接受单字符串或数组，并按有序名字列表比较语义相等；new只过滤完全空名字，不trim名字或去重，serde直接构造不应用该过滤。resolve_value_with依次查询，返回首个trim后非空的原值，保留值首尾空白；无匹配返回None。Many的is_empty仅检查数组长度，primary查首个非空名字；显示输出键名列表而非值。

#### Scenario: Whitespace value retained
- **WHEN** 首个键值为两个空格、第二个键值为带首尾空格的token
- **THEN** 跳过首个，返回第二个原字符串，不trim token。

证据：`crates/codegen/shell/src/agent/config.rs` — `impl EnvKeys`；`crates/codegen/shell/src/agent/config.rs` — `impl PartialEq for EnvKeys`。

### Requirement: Shell tool filtering flag and origin map boundary

ToolsConfig::resolve SHALL 读取tools.respect_gitignore布尔值，非布尔/缺失默认false；环境变量仅精确0/false/1/true覆盖，不trim或忽略大小写。config_origins仅遍历layers.user，对非table节点以点连接路径记录Config来源，数组整体一个键，不遍历数组元素，也不记录空table或其他layer；点号键与嵌套路径可能映射同一字符串。

#### Scenario: Uppercase environment value
- **WHEN** GROW_RESPECT_GITIGNORE=TRUE且TOML值为false
- **THEN** 环境值不匹配，保留false。

证据：`crates/codegen/shell/src/config/mod.rs` — `impl ToolsConfig`；`crates/codegen/shell/src/config/mod.rs` — `pub fn config_origins`；`crates/codegen/shell/src/config/mod.rs` — `fn walk_toml`。

补充测试证据（仅静态阅读）：`crates/codegen/shell/src/config/tests.rs` — `fn tools_config_env_false_overrides_toml_true`；`crates/codegen/shell/src/config/tests.rs` — `fn config_layers_origins_tracks_source`。

### Requirement: Shell effective plugin project overlay scope

resolve_effective_plugins_config SHALL 从effective配置解析plugins，加载或解析失败用默认；计算cwd的project_scope_allowed后遍历find_project_configs，成功解析的项目plugins仅在trusted时追加paths，无论trusted均追加disabled。项目enabled和cli_plugin_dirs不在此处合并，不去重；项目文件加载/解析失败跳过。该函数没有实现注释所称额外enabledPlugins导入合并。

#### Scenario: Untrusted project disable
- **WHEN** 项目未信任且声明paths和disabled
- **THEN** 忽略项目paths但追加disabled，不追加项目enabled。

证据：`crates/codegen/shell/src/config/mod.rs` — `pub fn resolve_effective_plugins_config`。

补充测试证据（仅静态阅读）：`crates/codegen/shell/src/config/tests.rs` — `fn resolve_effective_plugins_config_gates_project_paths_on_folder_trust`；`crates/codegen/shell/src/config/tests.rs` — `fn discover_plugins_excludes_untrusted_configpath_plugin_end_to_end`。

### Requirement: Shell memory and subagent enable resolver precedence

shell的resolve_enabled SHALL 仅在原始本地节存在时将config_enabled作为Some交给BoolFlag，其优先级为CLI Some、可解析env、本地值、remote、默认。memory额外在最终用no_memory强制关闭；experimental_memory=true会先于env产生CLI true。subagents不传remote启用值，CLI true先于env；无本地节默认true，有空节时反序列化enabled=false可覆盖该默认。

#### Scenario: Memory CLI versus environment
- **WHEN** experimental_memory=true，env解析为false，no_memory=false
- **THEN** enabled仍为true；若no_memory=true则最终false。

证据：`crates/codegen/shell/src/config/mod.rs` — `impl MemoryConfig`；`crates/codegen/shell/src/config/mod.rs` — `impl SubagentsConfig`；`crates/codegen/shell/src/agent/config.rs` — `pub(crate) fn resolve_enabled`；`crates/codegen/config-types/src/flags.rs` — `fn resolve_bool_flag`。

补充测试证据（本轮仅阅读，未执行）：`crates/codegen/shell/src/config/tests.rs` — `fn memory_config_cli_flag_overrides_env_disable`；`crates/codegen/shell/src/config/tests.rs` — `fn memory_config_no_memory_overrides_all`；`crates/codegen/shell/src/config/tests.rs` — `fn memory_config_local_disabled_blocks_remote_enable`。

补充测试证据（仅静态阅读）：`crates/codegen/shell/src/config/tests.rs` — `fn subagents_config_models_without_enabled`；`crates/codegen/shell/src/config/tests.rs` — `fn subagents_config_cli_flag_overrides_env_var`。

### Requirement: Shell plugin list persistence read error asymmetry

插件paths/disabled/enabled添加及CTA dismissed添加 SHALL 读取config失败时视为空配置，成功读取但TOML非法时返回错误；创建缺失table/array，类型错误拒绝，按字符串精确去重，但即使已存在仍重新pretty序列化写全文件。移除paths/disabled/enabled仅NotFound返回成功，其他读取错误传播，保留非字符串项并删除全部匹配字符串；无匹配仍重写。操作无锁、无临时rename或fsync，不保留注释，不协调enabled/disabled互斥，也不校验plugin ID或path存在。

#### Scenario: Already listed plugin
- **WHEN** 再次add_enabled_plugin同ID
- **THEN** 列表不重复，但文件仍重新序列化写入，不是文件I/O no-op。

证据：`crates/codegen/shell/src/config/mod.rs` — `pub fn add_plugin_path`；`crates/codegen/shell/src/config/mod.rs` — `pub fn add_disabled_plugin`；`crates/codegen/shell/src/config/mod.rs` — `pub fn add_enabled_plugin`；`crates/codegen/shell/src/config/mod.rs` — `pub fn remove_enabled_plugin`；`crates/codegen/shell/src/config/mod.rs` — `pub fn add_dismissed_plugin_cta_to_file`。

补充测试证据（仅静态阅读）：`crates/codegen/shell/src/config/tests.rs` — `fn add_dismissed_plugin_cta_is_idempotent`；`crates/codegen/shell/src/config/tests.rs` — `fn add_dismissed_plugin_cta_preserves_other_config`。

### Requirement: Shell plugin postinstall and dismissed set reading

post_install_plugin SHALL 加载registry按repo_key找repo，缺失返回空names和warning；逐个plugin名称调用add_enabled_plugin，错误收集warning继续，返回全部names而非仅成功项，不触发reload、不原子提交整repo。dismissed_plugin_ctas每次调用读取文件，读取/TOML错误或错误结构返回空HashSet，数组只收字符串并去重，函数本身不缓存。

#### Scenario: Partially enabled repository
- **WHEN** 一个plugin启用写入失败而其他成功
- **THEN** 返回所有plugin名称和失败warning，不回滚先前成功项。

证据：`crates/codegen/shell/src/config/mod.rs` — `pub fn post_install_plugin`；`crates/codegen/shell/src/config/mod.rs` — `pub fn dismissed_plugin_ctas_in_file`。

### Requirement: Shell hook path validation and line file persistence

validate_hooks_path SHALL 要求绝对路径，先canonicalize，失败则回退到存在祖先canonicalize后拼接缺失尾段；home canonicalize失败用原home路径，最终按组件starts_with判断在home下，不要求目标存在或为目录。add_hooks_path验证后写原始字符串而非canonical结果，公开_to_file入口不验证路径。添加按已有行trim后等于原参数去重，读错视为空后append；不补已有文件缺失的尾换行。删除不验证，NotFound成功、其他读错传播，删全部trim匹配行，仅找到时以换行重写。无文件锁和原子更新，不拒绝参数内换行。

#### Scenario: Missing final newline
- **WHEN** hooks-paths原内容没有尾换行，追加新路径
- **THEN** 直接writeln追加，可能与原末行拼接，不保证自动分成两条路径。

证据：`crates/codegen/shell/src/config/mod.rs` — `pub fn validate_hooks_path`；`crates/codegen/shell/src/config/mod.rs` — `pub fn add_hooks_path`；`crates/codegen/shell/src/config/mod.rs` — `pub fn add_hooks_path_to_file`；`crates/codegen/shell/src/config/mod.rs` — `pub fn remove_hooks_path_from_file`。

补充测试证据（仅静态阅读）：`crates/codegen/shell/src/config/tests.rs` — `fn validate_hooks_path_rejects_relative_path`；`crates/codegen/shell/src/config/tests.rs` — `fn remove_hooks_path_preserves_others`。

### Requirement: Shell config watch filtering and event classification

ConfigFileWatcher SHALL 默认1000ms去抖，在debouncer前丢弃Access事件，其他事件及错误传给下层；callback忽略Err批次，只按文件名config.toml识别，父目录精确等于传入grow_home则Global，否则Project(path)，不canonicalize或校验项目归属。每批相同事件去重，经无界channel发送，接收端关闭时忽略发送错误。无自写抑制。创建debouncer或grow_home非递归watch失败返回None；extra_paths仅非递归watch各父目录且错误忽略。

#### Scenario: Other config file
- **WHEN** 已监听目录中出现config.toml且parent不等于grow_home
- **THEN** 产生ProjectConfigChanged，不依赖是否来自extra_paths中的原始文件名。

证据：`crates/codegen/shell/src/config/watcher.rs` — `pub fn start`；`crates/codegen/shell/src/config/watcher.rs` — `impl notify::Watcher for AccessFilteredWatcher`。

### Requirement: Shell config cwd watch registration failure retention

watch_path SHALL 以原始PathBuf集合去重，每cwd尝试非递归监听cwd和cwd/.grow，无论成功与否均登记cwd，重复调用不重试失败watch。unwatch_path先移除集合，再尽力取消两个watch，不维护session引用计数或与extra_paths/global watch的独立所有权。缺失目录日志debug，其他添加错误warn。后建.grow不自动注册其内部watch，需调用方刷新或解除登记后重试；不保证打开另一个相同cwd会修复。

#### Scenario: Missing grow directory
- **WHEN** 首次watch_path时.grow不存在，稍后创建并再次watch_path同cwd
- **THEN** 集合命中直接返回，不重新安装.grow监听。

证据：`crates/codegen/shell/src/config/watcher.rs` — `pub fn watch_path`；`crates/codegen/shell/src/config/watcher.rs` — `pub fn unwatch_path`；`crates/codegen/shell/src/config/watcher.rs` — `fn watch_cwd_dirs`；`crates/codegen/shell/src/config/watcher.rs` — `fn log_watch_error`。

### Requirement: Shell discovery watch scope and change priority

SkillsFileWatcher SHALL 将与grow_home路径相等或basename为.grow的root作为有限范围监听：root非递归、skills递归、commands/workflows非递归；其他显式root递归监听。路径去重辅助函数可canonicalize比较，但事件分类用原始strip_prefix/starts_with，不canonicalize。root自身及skills/commands归Skills，workflows归Workflows，其他子节忽略；自定义递归root任意事件归Skills。2秒去抖批次最多发一项，Skills优先于Workflows，错误批次和发送失败忽略；无成功watch返回None。

#### Scenario: Mixed discovery batch
- **WHEN** 同批次包含skills与workflows变化
- **THEN** 只发Skills，接收方须自行处理所需刷新，不额外发Workflows。

证据：`crates/codegen/shell/src/config/watcher.rs` — `impl SkillsFileWatcher`；`crates/codegen/shell/src/config/watcher.rs` — `fn discovery_change_under_root`；`crates/codegen/shell/src/config/watcher.rs` — `fn plan_skills_watch_targets`。

### Requirement: Shell discovery directory attachment and project parent watch

ProjectDiscoveryWatcher SHALL 用registry解析project_root，.grow存在时先监听其root，否则先非递归监听project_root；初始watch失败返回None，再尝试附加.grow、skills、commands、workflows。SkillsFileWatcher的project补充计划取决于dirs_to_watch是否已含project .grow而非该目录实际存在；仅缺该输入且未含project_root时加parent watch。refresh函数仅对存在且未成功登记的目录重试，成功才插入集合；已登记目录删除重建不自动清除登记。调用者须在事件后显式refresh，不在callback自动附加。

#### Scenario: Explicit missing grow root
- **WHEN** dirs_to_watch已列出尚不存在的project/.grow
- **THEN** 计划不额外加project parent watch，不能保证首次创建会被观察。

证据：`crates/codegen/shell/src/config/watcher.rs` — `impl ProjectDiscoveryWatcher`；`crates/codegen/shell/src/config/watcher.rs` — `fn attach_new_refresh_dirs`；`crates/codegen/shell/src/config/watcher.rs` — `fn plan_skills_watch_targets`。

### Requirement: Shell config reload batching and partial publication

ConfigReloader SHALL 优先响应cancel或channel关闭退出，收到首事件后try_recv排空当前可取事件为批；任何config事件均先reload_config，catch_unwind只包此调用，错误/panic记录后仍处理project事件。reload先读effective并比较/更新announcements，然后读disk global；后者失败返回Ok且可已发布announcements。消息发送错误忽略，已更新baseline不回滚；不是跨全部配置/消费者的原子last-known-good事务。

#### Scenario: Global disk load fails after announcement update
- **WHEN** effective读取成功且announcement变化，后续disk读取失败
- **THEN** announcement可已发出，global baseline保留旧值。

证据：`crates/codegen/shell/src/config/reloader.rs` — `pub async fn run`；`crates/codegen/shell/src/config/reloader.rs` — `fn reload_config`。

### Requirement: Shell reload section diff and initial baseline

start_config_reload SHALL 先创建watcher，再读effective作为initial baseline，失败用空table；spawn任务但只返回需保留的watcher。后续global比较使用load_from_disk结果：mcp_servers原始table变化发全局MCP；Memory用固定startup flags/remote设置解析后比较；Skills解析失败用默认再比较；provider/models/model/auth_provider任一原始节变化发ModelsChanged；UI仅比较theme/permission_mode/fork_secondary_model字符串。最后替换global baseline，不承诺其他配置自动重载或纯env变化触发。

#### Scenario: Unrelated global field
- **WHEN** 只改变未在上述diff中的字段
- **THEN** 可更新baseline但不发布对应typed更新。

证据：`crates/codegen/shell/src/config/reloader.rs` — `pub fn start_config_reload`；`crates/codegen/shell/src/config/reloader.rs` — `fn reload_config`；`crates/codegen/shell/src/config/reloader.rs` — `fn model_config_changed`；`crates/codegen/shell/src/config/reloader.rs` — `fn extract_ui_fields`。

### Requirement: Shell project config content suppression scope

project事件 SHALL 按path两级parent取cwd并保序去重，不验证.grow命名；对find_project_configs返回顺序、路径字符串和完整文件字节使用DefaultHasher，NotFound计缺失，其他读错返回None。首次或hash不确定发project MCP；仅新旧Some相等抑制。新Some在发送前缓存，None保留旧hash，不等接收确认；比较整文件而非仅MCP节，hash非无碰撞内容证明。

#### Scenario: Read error followed by old content
- **WHEN** 已有hash，读错时发更新，随后恢复为原hash内容
- **THEN** 旧hash未清除，恢复事件可能被抑制。

证据：`crates/codegen/shell/src/config/reloader.rs` — `fn collect_project_cwds`；`crates/codegen/shell/src/config/reloader.rs` — `fn hash_project_mcp_config`；`crates/codegen/shell/src/config/reloader.rs` — `pub async fn run`。

### Requirement: Shell skills discovery timeout and request defaults

技能扩展 SHALL 对list要求cwd字段，add/remove/toggle的缺省cwd为点；reset/config参数解析失败也回退点。reload先等待load_config，再对带plugin registry的发现施加5秒timeout，超时warning后返回空列表，不返回超时错误；超时不覆盖之前的配置加载。外层传agent registry snapshot，不按请求cwd在此重建registry。

#### Scenario: Discovery times out after add persistence
- **WHEN** add配置已保存，随后技能发现超时
- **THEN** 仍返回成功响应和空列表、addedCount=0，不撤回配置。

证据：`crates/codegen/shell/src/extensions/skills.rs` — `async fn reload_skills`；`crates/codegen/shell/src/extensions/skills.rs` — `pub async fn handle`。

### Requirement: Shell skill path addition and removal semantics

路径解析 SHALL 展开~或~/，HOME优先USERPROFILE，相对路径与cwd join，再尝试canonicalize；失败保留joined路径并有损转UTF8，不保证绝对或存在。add清除与resolved字符串互为前缀的ignore项、按精确值去重paths；addedCount为当前列表的字符串前缀计数，不是新增差值。remove只精确删除paths，不添加ignore、不删除磁盘文件或禁止自动发现；保存失败返回错误。

#### Scenario: Sibling textual prefix in ignore
- **WHEN** 添加路径与既有ignore只有字符串前缀关系而非目录父子关系
- **THEN** 该ignore也会被移除。

证据：`crates/codegen/shell/src/extensions/skills.rs` — `fn resolve_skill_path`；`crates/codegen/shell/src/extensions/skills.rs` — `pub async fn handle`；`crates/codegen/shell/src/extensions/skills.rs` — `fn test_resolve_relative_path_against_cwd`。

### Requirement: Shell skills reset toggle and reporting scope

reset SHALL 将整个skills配置替换默认值；toggle先发现并按精确name验证存在，随后修改disabled名称列表，再重读配置映射已加载列表enabled，不做第二次发现。同名项都按该name标记；此handler返回新列表但不广播现有会话baseline更新。config分别加载配置与发现列表，显示paths/ignore及来源摘要，不保证同一快照；来源计数使用字符串前缀。自动摘要检查cwd/git root/grow home下skills与commands，常规零计数省略，extra_skill_dirs存在时可显示零，读取extra配置失败空列表。

#### Scenario: Unknown skill toggle
- **WHEN** 发现列表不含请求name，包括超时返回空列表
- **THEN** 返回not found，disabled配置不变。

证据：`crates/codegen/shell/src/extensions/skills.rs` — `fn discover_auto_sources`；`crates/codegen/shell/src/extensions/skills.rs` — `fn extra_skill_dirs_from_config`；`crates/codegen/shell/src/extensions/skills.rs` — `fn count_skills_from`；`crates/codegen/shell/src/extensions/skills.rs` — `pub async fn handle`。

### Requirement: Shell settings projection parse fallback

load_config SHALL 在effective配置加载失败或root非table时返回默认Config；cli/models/ui/skills/diagnostics/session逐节反序列化失败各自默认，permission失败为None，management_api_key只取endpoints字符串，ask_user_question只取toolset子节并失败默认。该投影不保留所有root字段，也不通过remote.secret测试证明已加载该secret。

#### Scenario: One malformed section
- **WHEN** root有效但skills无法反序列化
- **THEN** skills默认，其他有效节继续解析。

证据：`crates/codegen/shell/src/util/config/load.rs` — `pub async fn load_config`；`crates/codegen/shell/src/util/config/load.rs` — `pub fn load_config_from_toml`。

### Requirement: Shell settings write serialization and read error asymmetry

save_config及update_config SHALL 使用同一个进程内async mutex；update在锁内从disk加载投影并执行闭包，再重读用户文件合并保存。初次disk加载错误回退空table；保存重读的TOML解析失败拒绝覆盖，但任意读取错误都按空table继续。锁不协调外部进程，save_config传入快照可能在加锁前已过期。独立read_to_string_or_empty仅NotFound为空，其他错误传播，不能将其严格语义套给save。

#### Scenario: Readable malformed TOML
- **WHEN** save重读得到无法解析的文本
- **THEN** 返回拒绝覆盖错误，不写临时文件。

证据：`crates/codegen/shell/src/util/config/persist.rs` — `pub async fn save_config`；`crates/codegen/shell/src/util/config/persist.rs` — `pub async fn update_config`；`crates/codegen/shell/src/util/config/persist.rs` — `async fn save_config_locked`；`crates/codegen/shell/src/util/config/persist.rs` — `pub(crate) fn read_to_string_or_empty`。

### Requirement: Shell settings selective deep merge and removals

保存 SHALL 合并cli/models/ui/session及非默认skills，默认skills直接删除整节；toolset只合并ask_user_question且两字段均None时完全跳过。表递归合并、其他值替换，未序列化字段保留，空序列化table不改原节，非table或序列化错误删除目标节。每次保存删除ui_theme、selection_highlight_duration_ms、double_click_action旧UI键。pretty TOML重写保留语义字段，不保留注释或格式；本函数不从Config回写permission、diagnostics或management_api_key。

#### Scenario: Unset ask fields
- **WHEN** 传入ask两个字段均None，文件已有ask配置
- **THEN** 保留原配置，不以None删除旧值。

证据：`crates/codegen/shell/src/util/config/persist.rs` — `fn merge_toml_tables`；`crates/codegen/shell/src/util/config/persist.rs` — `fn merge_section`；`crates/codegen/shell/src/util/config/persist.rs` — `fn merge_ask_user_question_section`；`crates/codegen/shell/src/util/config/persist.rs` — `fn remove_retired_ui_keys`。

补充测试源码证据（未执行）：`crates/codegen/shell/src/util/config/persist.rs` — `fn ask_user_question_merge_writes_subtable_without_splatting_toolset`；`crates/codegen/shell/src/util/config/persist.rs` — `fn merge_section_nested_display_refresh_preserves_future_knob`；`crates/codegen/shell/src/util/config/persist.rs` — `fn merge_section_updates_modeled_fields_preserving_unmodeled`；`crates/codegen/shell/src/util/config/persist.rs` — `fn merge_section_session_default_does_not_leak_load_envrc`；`crates/codegen/shell/src/util/config/persist.rs` — `fn merge_section_session_explicit_value_does_not_drag_load_envrc`；`crates/codegen/shell/src/util/config/persist.rs` — `fn session_load_envrc_explicit_false_round_trips`；`crates/codegen/shell/src/util/config/persist.rs` — `fn settings_write_removes_retired_appearance_keys`；`crates/codegen/shell/src/util/config/persist.rs` — `fn merge_section_replaces_non_table_section`；`crates/codegen/shell/src/util/config/persist.rs` — `fn merge_section_cli_only_updates_set_fields_preserves_unmodeled`；`crates/codegen/shell/src/util/config/persist.rs` — `fn merge_section_models_only_updates_set_fields_preserves_others`；`crates/codegen/shell/src/util/config/persist.rs` — `fn persist_preferred_model_flow_roundtrips_via_load_and_new_from_toml_cfg`。

### Requirement: Shell settings temporary publication durability scope

配置写入 SHALL 以目标扩展名替换生成含PID和纳秒的临时路径，普通write后rename；Unix尝试复制原mode但忽略chmod错误，mkdir错误也忽略至后续IO。没有create_new、防链接身份校验、文件/目录fsync或跨进程锁。async save rename失败不清临时文件；同步atomic_write_string在rename失败时尽力清理，但write失败可留临时文件，helper本身不获取SAVE_LOCK。

#### Scenario: Rename fails
- **WHEN** 临时写入完成但rename返回错误
- **THEN** async save传播错误且可能保留临时文件，同步helper先尽力移除临时文件再传播。

证据：`crates/codegen/shell/src/util/config/persist.rs` — `async fn save_config_locked`；`crates/codegen/shell/src/util/config/persist.rs` — `pub(crate) fn atomic_write_string`；`crates/codegen/shell/src/util/config/persist.rs` — `pub(crate) async fn lock_config_writes`。

### Requirement: Shell settings typed wrapper field mapping

设置writer SHALL 通过update_config写ui布尔项：compact_mode为直接bool，其余show_timestamps/show_timeline/page_flip_on_send/combine_queued_prompts/simple_mode/invert_scroll/vim_mode/remember_tool_approvals/show_thinking_blocks/prompt_suggestions/group_tool_verbs为Some(bool)。contextual_hints写undo/plan_mode/image_input/send_now/small_screen/word_select/ssh_wrap，display_refresh写auto_cadence_enabled；ask timeout写toolset.ask_user_question，show_tips及auto_update写cli。这里只持久化，不执行对应运行时效果。

#### Scenario: Plan hint setting
- **WHEN** 调用set_contextual_hint_plan_mode
- **THEN** 写plan_mode字段，而非注释中的behavior。

证据：`crates/codegen/shell/src/util/config/settings_writes.rs` — `pub async fn set_contextual_hint_plan_mode`；`crates/codegen/shell/src/util/config/settings_writes.rs` — `pub async fn set_ask_user_question_timeout_enabled`；`crates/codegen/shell/src/util/config/settings_writes.rs` — `pub async fn set_auto_update`。

### Requirement: Shell settings numeric and string validation boundary

数值writer SHALL 将max_thoughts_width clamp至40..500转u16、scroll_speed至1..100及scroll_lines至1..10转u8。fork_secondary_model只限制UTF8字节长度256，空串原样写；default_model转交campaign writer，空串变None。theme/auto_dark_theme/auto_light_theme/scroll_mode/keep_text_selection/render_mermaid/hunk_tracker_mode/default_selected_permission直接保存字符串，不在此验证注释列出的枚举或catalog，也不trim。

#### Scenario: Invalid scroll mode string
- **WHEN** 调用set_scroll_mode传入未列举字符串
- **THEN** writer仍提交Some(value)，验证不在本函数。

证据：`crates/codegen/shell/src/util/config/settings_writes.rs` — `pub async fn set_max_thoughts_width`；`crates/codegen/shell/src/util/config/settings_writes.rs` — `pub async fn set_scroll_speed`；`crates/codegen/shell/src/util/config/settings_writes.rs` — `pub async fn set_scroll_lines`；`crates/codegen/shell/src/util/config/settings_writes.rs` — `pub async fn set_fork_secondary_model`；`crates/codegen/shell/src/util/config/settings_writes.rs` — `pub async fn set_default_model`；`crates/codegen/shell/src/util/config/settings_writes.rs` — `pub async fn set_scroll_mode`。

### Requirement: Shell settings clear sentinel deep merge limitation

set_screen_mode空串及set_cancel_subagents_on_turn_cancel的ask SHALL 将投影字段置None后调用update_config。对应字段skip_serializing_if None，通用merge保留未序列化旧键，因此已有screen_mode或cancel_subagents_on_turn_cancel不会由这两条路径删除；不能把函数注释中的clear视为磁盘键已移除。

#### Scenario: Reset existing screen mode
- **WHEN** 文件已有screen_mode，调用set_screen_mode空串
- **THEN** 投影None被省略，合并保留旧screen_mode。

证据：`crates/codegen/shell/src/util/config/settings_writes.rs` — `pub async fn set_screen_mode`；`crates/codegen/shell/src/util/config/settings_writes.rs` — `pub async fn set_cancel_subagents_on_turn_cancel`。

### Requirement: Shell campaign dismissal bounded persistence

campaign dismissal SHALL 在无user home时跳过，其他保存失败仅warning。进程mutex内尽力取得文件advisory锁，打开或加锁失败仍继续；旧状态NotFound为空，其他读取错误传播，坏JSON尽力改名json.corrupt后从空开始，改名失败不阻止继续。新增空id跳过、已见id不移动位置，列表超32从头淘汰；PID+nonce临时write后rename，rename失败尽力清temp，无fsync或目录创建。旧列表本身不先清空id或去重，FIFO淘汰可能让仍有效campaign再次出现。

#### Scenario: Re-dismiss existing id
- **WHEN** 已有id再次提交且另有新id
- **THEN** 旧id不移到队尾，未来仍可按原位置被淘汰。

证据：`crates/codegen/shell/src/util/config/campaigns.rs` — `fn dismiss_campaign_ids_at`；`crates/codegen/shell/src/util/config/campaigns.rs` — `pub fn dismiss_campaign_ids`。

补充测试源码（未执行）：`crates/codegen/shell/src/util/config/campaigns.rs` — `fn dismiss_persists_handles_corrupt_and_caps`。

### Requirement: Shell campaign governed model state projection

campaign字段同步 SHALL 当前仅登记models.default；effective与base不同且active patch触及该路径才标记driven，恢复值保存原base。加载层失败保留cfg字段值，只清driven/recovery；加载成功将effective字符串投影回default。campaign_driven_models_default独立重新加载层及dismiss状态，仅active非空且effective/default较base改变并为字符串时返回，不自行校验模型catalog。

#### Scenario: Campaign no-op
- **WHEN** active campaign写入与base相同default
- **THEN** 不标记campaign-driven，也不提供恢复值。

证据：`crates/codegen/shell/src/util/config/campaigns.rs` — `fn apply_campaign_fields`；`crates/codegen/shell/src/util/config/campaigns.rs` — `pub fn sync_campaign_fields`；`crates/codegen/shell/src/util/config/campaigns.rs` — `fn campaign_driven_models_default_from`。

补充测试源码（未执行）：`crates/codegen/shell/src/util/config/campaigns.rs` — `fn campaign_driven_models_default_tracks_local_and_dismissals`；`crates/codegen/shell/src/util/config/campaigns.rs` — `fn campaign_field_flags_campaign_win_not_local_config_win`；`crates/codegen/shell/src/util/config/campaigns.rs` — `fn dismissed_id_is_dropped_from_local_layer`；`crates/codegen/shell/src/util/config/campaigns.rs` — `fn campaign_field_reset_clears_driven_state`。

### Requirement: Shell campaign user choice bookkeeping order

persist_user_choice SHALL 在spawn_blocking中查找用户层未dismiss且触及字段的campaign并尝试dismiss，再await update_config；dismiss选择忽略kill switch，层加载失败返回空列表而非另读备用campaign。dismiss失败仅日志，JoinError也继续写配置；dismiss成功而配置写失败不回滚dismiss。该顺序不是两个文件原子事务；外层future取消不保证继续配置写入。

#### Scenario: Config write fails after dismissal
- **WHEN** dismiss已保存，随后update_config返回错误
- **THEN** 用户选择返回错误，dismiss保持。

证据：`crates/codegen/shell/src/util/config/campaigns.rs` — `fn resolve_dismissable_campaigns`；`crates/codegen/shell/src/util/config/campaigns.rs` — `pub async fn persist_user_choice`。

补充测试源码（未执行）：`crates/codegen/shell/src/util/config/campaigns.rs` — `fn models_default_persist_targets_only_model_campaigns`。

### Requirement: Shell default model campaign writer value boundary

persist_models_default SHALL 将None当空串、按字节长度限制256后调用models.default字段的campaign选择writer；非空字符串原样Some，不trim或查catalog，空串投影None。reasoning_effort仅Some时赋值，None保持现有值；最终None字段是否删除受通用merge的省略保留规则约束，不能仅凭clear注释认定磁盘旧default删除。

#### Scenario: Whitespace model choice
- **WHEN** value只含空格且长度未超256
- **THEN** 写入Some空格字符串，非空校验不在此执行。

证据：`crates/codegen/shell/src/util/config/campaigns.rs` — `pub async fn persist_models_default`。

### Requirement: Shell worktree hint mode defaults

WorktreeHintMode SHALL 只识别精确ask/always/never，其他字符串debug并Never；缺失或非字符串new模式默认Never，fork默认Ask，因此非法fork字符串与缺失fork不同。project_picker_disabled只取bool，缺失/类型错false；磁盘加载失败按默认hints解析。

#### Scenario: Invalid fork string
- **WHEN** fork_worktree_mode为未知字符串
- **THEN** 返回Never，而非缺失字段时的Ask。

证据：`crates/codegen/shell/src/util/config/hints.rs` — `pub fn from_config_str`；`crates/codegen/shell/src/util/config/hints.rs` — `pub fn resolve_pair`；`crates/codegen/shell/src/util/config/hints.rs` — `pub fn resolve_hints_from_disk`。

### Requirement: Shell contextual hint master resolution

七项contextual hint SHALL 各自用BoolFlag按GROW_CONTEXTUAL_HINTS环境、本地Option、默认true解析；undo/plan_mode/image_input/send_now/small_screen/word_select/ssh_wrap均不传remote feature值。主环境有效时覆盖所有本地设置。解析只返回gate，不自行显示提示。

#### Scenario: Master zero
- **WHEN** 环境主开关为0且本地全true
- **THEN** 所有七项关闭。

证据：`crates/codegen/shell/src/util/config/hints.rs` — `pub fn resolve_contextual_hints`；`crates/codegen/shell/src/util/config/hints.rs` — `fn contextual_hints_env_master_zero_forces_all_off`。

### Requirement: Shell startup tips local override semantics

tips解析 SHALL 在cli.show_tips精确bool false时先返回空；debug构建随后可用GROW_TIPS_OVERRIDE按竖线拆分，保留空项且不trim。普通路径无有效tips节返回空；exclude_default=true返回空，包括丢弃本地tips，否则只返回本地列表，没有注释所称内置默认列表合并。resolve_tips_from_disk实际使用传入raw_config，不自行加载配置，非空才委托pick_and_advance。

#### Scenario: Exclude default with local tips
- **WHEN** tips节有本地列表且exclude_default=true
- **THEN** 普通路径结果仍为空。

证据：`crates/codegen/shell/src/util/config/tips.rs` — `pub fn merge_tips`；`crates/codegen/shell/src/util/config/tips.rs` — `pub fn resolve_tips`；`crates/codegen/shell/src/util/config/tips.rs` — `pub fn resolve_tips_from_disk`。

### Requirement: Shell slash command tag merge parsing

slash command tags SHALL 从TOML表逐项保留字符串，再以环境JSON map按键覆盖。空/空白环境无覆盖；JSON任一非字符串值导致整份解析失败并warning，而TOML非字符串仅跳过该项。名称和tag不做枚举校验或trim。channel_from_toml_opt仅返回cli.channel字符串，无渠道有效性判断。

#### Scenario: Mixed JSON values
- **WHEN** 环境JSON同时含字符串及数字值
- **THEN** 整份环境覆盖忽略，本地有效tag保留。

证据：`crates/codegen/shell/src/util/config/tips.rs` — `fn parse_slash_command_tags_json`；`crates/codegen/shell/src/util/config/tips.rs` — `fn merge_command_tags`；`crates/codegen/shell/src/util/config/tips.rs` — `pub fn channel_from_toml_opt`。

### Requirement: Shell announcement replacement and fallback

resolve_announcements SHALL 对缺失announcements使用共享默认；显式列表完整反序列化成功则原序返回，空列表关闭；任一反序列化错误warning并整体回默认，不保留部分有效项。本模块不筛选、排序或持久化dismiss。

#### Scenario: Invalid configured list
- **WHEN** announcements无法解析为Vec<Announcement>
- **THEN** 整体使用默认公告。

证据：`crates/codegen/shell/src/util/config/announcements.rs` — `pub fn resolve_announcements`；`crates/codegen/shell/src/util/config/announcements.rs` — `fn explicit_empty_list_disables_announcements`。

### Requirement: Shell launch permission parsing and display clamp

权限模式 SHALL 仅精确识别always-approve/auto/ask，其他字符串Ask。ui表含permission_mode但值非字符串也Some(Ask)阻断remote；ui非table或键缺失则允许remote。launch CLI优先，否则加载effective配置，加载失败直接Ask不再用remote；请求Auto但磁盘gate关闭时降为Ask，AlwaysApprove不受此Auto gate影响。clamped_display_permission_mode对三种模式均返回ask，不是实际launch resolver。

#### Scenario: Malformed local permission key
- **WHEN** ui.permission_mode为数字且remote为always-approve
- **THEN** 返回Ask，remote不覆盖本地坏值。

证据：`crates/codegen/shell/src/util/config/permissions.rs` — `pub fn permission_mode_from_ui_if_set`；`crates/codegen/shell/src/util/config/permissions.rs` — `pub fn resolve_permission_mode`；`crates/codegen/shell/src/util/config/permissions.rs` — `pub fn effective_permission_mode_for_launch`；`crates/codegen/shell/src/util/config/permissions.rs` — `pub fn clamped_display_permission_mode`。

### Requirement: Shell plan approval and remote secret readers

load_require_plan_approval SHALL 仅对effective ui.require_plan_approval布尔true返回true，加载错/缺失/错型false；它只返回配置不执行plan审批。load_remote_secret_sync独立加载effective配置，取remote.secret字符串原样返回，包括空白/空串；加载错/缺失/错型None，不进行认证或trim。auto_mode_session_active只在gate true且请求Auto时true。

#### Scenario: Empty remote secret
- **WHEN** remote.secret为有效空字符串
- **THEN** 返回Some空串，不视为缺失。

证据：`crates/codegen/shell/src/util/config/permissions.rs` — `pub fn load_require_plan_approval`；`crates/codegen/shell/src/util/config/permissions.rs` — `pub fn load_remote_secret_sync`；`crates/codegen/shell/src/util/config/permissions.rs` — `pub fn auto_mode_session_active`。

### Requirement: Shell auto permission gate and classifier defaults

Auto gate SHALL 独立读取auto_mode.enabled的bool，按BoolFlag环境、本地、默认true解析；磁盘加载失败视本地缺失，因此有效环境未设时gate仍true。完整AutoModeConfig另行整节反序列化，失败warning后默认，不影响独立enabled提取。分类timeout默认30000ms，clamp到1000..120000ms且越界warning；prompt type缺失Full，reasoning effort原样Option，不在此发起classifier。

#### Scenario: Malformed sibling field
- **WHEN** auto_mode.enabled=false但同节其他字段错型
- **THEN** 独立gate保留false；完整AutoModeConfig解析可回默认。

证据：`crates/codegen/shell/src/util/config/resolve/auto_mode.rs` — `fn auto_permission_mode_from_toml`；`crates/codegen/shell/src/util/config/resolve/auto_mode.rs` — `pub fn auto_permission_mode_enabled_from_disk`；`crates/codegen/shell/src/util/config/resolve/auto_mode.rs` — `pub fn resolve_auto_mode_config_from_disk`；`crates/codegen/shell/src/util/config/resolve/auto_mode.rs` — `pub fn auto_mode_classify_timeout`；`crates/codegen/shell/src/util/config/resolve/auto_mode.rs` — `pub fn auto_mode_classifier_defaults`。

### Requirement: Shell crash handler and approval persistence gates

crash handler与remember approvals gate SHALL 分别按GROW_CRASH_HANDLER/diagnostics.crash_handler及GROW_REMEMBER_TOOL_APPROVALS/ui.remember_tool_approvals经BoolFlag解析，环境优先本地，默认false；同步磁盘加载失败视本地缺失，环境仍可启用。二者均不传remote，不在resolver实际安装handler或保存审批。

#### Scenario: Disk failure with env enabled
- **WHEN** 配置加载失败但对应有效环境开关启用
- **THEN** resolver仍返回true。

证据：`crates/codegen/shell/src/util/config/resolve/crash_handler.rs` — `pub fn resolve_crash_handler_enabled`；`crates/codegen/shell/src/util/config/resolve/tool_approvals.rs` — `pub fn resolve_remember_tool_approvals`。

### Requirement: Shell MCP startup timeout precedence

MCP启动timeout SHALL 先加载effective mcp.startup_timeout_sec非负转换且非零，再优先使用环境MCP_TIMEOUT正u64毫秒向上取整秒，其次GROW_MCP_STARTUP_TIMEOUT_SECS正u64，最后配置或30秒。环境trim；零/非法/负值落下一层，没有上限clamp；resolved别名同一实现，不启动MCP进程。

#### Scenario: One millisecond override
- **WHEN** MCP_TIMEOUT为1且秒环境/本地也有效
- **THEN** 返回1秒。

证据：`crates/codegen/shell/src/util/config/resolve/mcp.rs` — `pub fn resolve_mcp_startup_timeout_secs`；`crates/codegen/shell/src/util/config/resolve/mcp.rs` — `fn mcp_startup_timeout_from_env`。

### Requirement: Shell MCP output limit global and project split

全局MCP输出limit SHALL 使用tools环境解析优先effective mcp.max_output_bytes正整数usize，最后共享默认；cache函数将结果交tools setter。按cwd入口在存在有效环境limit时返回None，否则仅允许trusted project scope，按find_project_configs顺序取最后有效正值；读取错/非法值不清除已选值。返回None不表示无限制，而是没有本地project覆盖。本模块不实际截断输出。

#### Scenario: Environment suppresses project override
- **WHEN** tools环境解析返回Some且project含有效limit
- **THEN** cwd resolver返回None，让全局环境值生效。

证据：`crates/codegen/shell/src/util/config/resolve/mcp.rs` — `pub fn resolve_max_mcp_output_bytes`；`crates/codegen/shell/src/util/config/resolve/mcp.rs` — `fn project_max_mcp_output_bytes`；`crates/codegen/shell/src/util/config/resolve/mcp.rs` — `pub fn resolve_max_mcp_output_bytes_for_cwd`。

### Requirement: Shell ZDR access resolver reachability

ZDR access resolver SHALL 经BoolFlag按GROW_ZDR_ACCESS_ENABLED、本地features.zdr_access_enabled布尔、传入remote字段、默认false选择；错型本地视缺失。该函数只返回gate，本工作树crates引用搜索未发现调用，不能宣称它已在认证或产品入口执行ZDR准入限制。

#### Scenario: No source enabled
- **WHEN** 环境、本地与remote均无有效值
- **THEN** resolver返回false，不据此推断实际用户已被入口拒绝。

证据：`crates/codegen/shell/src/util/config/resolve/features.rs` — `pub fn resolve_zdr_access_enabled`；`crates/codegen/config-types/src/flags.rs` — `impl<'a> BoolFlag`。

### Requirement: Shell MCP watcher and restart gate wiring

三个MCP规范resolver SHALL 默认true，CLI>对应GROW_MCP环境>本地features>feature_flag；util包装和Config方法均传CLI/remote None。session actor只有非subagent且liveness开启才安装event channel与dispatcher，其内auto_restart开启才提供RestartActions；两开关分别加载effective配置。app的recursive config watch在建cwd channel前只读取disk配置，关闭不建channel；此名称不意味着递归目录监控，实际watch行为见watcher契约。

#### Scenario: Restart on but liveness off
- **WHEN** session liveness关闭而restart开启
- **THEN** 该actor分支不会创建dispatcher及其RestartActions。

证据：`crates/codegen/shell/src/agent/config.rs` — `pub fn resolve_mcp_liveness_watchers`；`crates/codegen/shell/src/agent/config.rs` — `pub fn resolve_mcp_auto_restart`；`crates/codegen/shell/src/agent/config.rs` — `pub fn resolve_mcp_recursive_config_watch`；`crates/codegen/shell/src/session/actor/run_loop.rs` — `if !session.startup_hints.is_subagent && liveness_watchers_enabled`；`crates/codegen/shell/src/agent/app.rs` — `let recursive_config_watch_enabled =`。

### Requirement: Shell system prompt identity label tiers

system prompt label SHALL 按环境、本地模型、本地agent、传入ModelInfo、remote全局、共享默认标签选取，每层trim后空值落下一层。模型层先按model_id取Option，只有None才尝试routing model slug；已存在空白model_id标签阻断slug回退，随后在层解析中落到agent。本模块只选身份标签，不读提示词文件或替换/追加正文。

#### Scenario: Blank catalog label with routing label
- **WHEN** catalog id标签空白且routing slug标签非空
- **THEN** 不选routing标签，继续agent及后续层。

证据：`crates/codegen/shell/src/util/config/resolve/system_prompt.rs` — `pub fn resolve_system_prompt_label`；`crates/codegen/shell/src/util/config/resolve/system_prompt.rs` — `pub fn resolve_system_prompt_label_from_tiers`。

### Requirement: Shell search shadow and shell environment config

搜索shadow开关 SHALL 以DISABLE_EMBEDDED_SEARCH_TOOLS=true同时关闭find_bfs/grep_ugrep，否则各自primary GROW_TOOLS变量优先旧alias GROW变量、本地toolset.bash字段、默认true；disable=false不强制开启。login capture按GROW_LOGIN_ENV、本地login_shell_capture、默认true。shell_environment_policy整节反序列化失败warning并返回None，不提供部分有效策略；实际环境继承由消费方处理。

#### Scenario: Explicit disable false with local off
- **WHEN** 总disable为false且单工具本地false，无环境覆盖
- **THEN** 该工具仍关闭。

证据：`crates/codegen/shell/src/util/config/resolve/toolset.rs` — `pub fn resolve_search_tools_enabled`；`crates/codegen/shell/src/util/config/resolve/toolset.rs` — `fn resolve_search_tool_enabled`；`crates/codegen/shell/src/util/config/resolve/toolset.rs` — `pub fn resolve_shell_env_policy`；`crates/codegen/shell/src/util/config/resolve/toolset.rs` — `pub fn resolve_login_shell_capture`。

### Requirement: Shell ask user timeout parameter resolution

ask-user参数 SHALL 从effective配置读取，enabled按对应GROW环境、本地bool、工具默认；seconds按trim后正u64环境、本地正整数、工具RESPONSE_TIMEOUT秒数。本地整数零/负值warning后忽略，错型无该warning；环境有效时不再解析本地秒数。最终构造NonZeroU64，无上限clamp；即使enabled=false仍解析seconds。这里只构造参数，不等待用户或发超时响应。

#### Scenario: Invalid local seconds with valid env
- **WHEN** 环境秒数有效且本地为负数
- **THEN** 返回环境秒数，不触发本地负数warning分支。

证据：`crates/codegen/shell/src/util/config/resolve/toolset.rs` — `fn ask_user_question_timeout_secs`；`crates/codegen/shell/src/util/config/resolve/toolset.rs` — `fn ask_user_question_timeout_secs_from_toml`；`crates/codegen/shell/src/util/config/resolve/toolset.rs` — `pub(crate) fn resolve_ask_user_question_params_from_disk`。

### Requirement: Shell display refresh policy bounds precedence

显示刷新policy SHALL 对probe及auto布尔按对应GROW环境、本地ui.display_refresh、remote对象、默认解析；默认probe true、auto false。数值按本地、remote、默认选取，floor/ceiling默认8/16ms并先clamp1..100；倒置时保留优先级较高端并将另一端收拢，同层倒置回默认。Hz默认55/165，仅排序不额外范围clamp。共享宽容类型允许坏字段不丢其他有效字段。

#### Scenario: Mixed priority inverted bounds
- **WHEN** 本地min_hz100且remote max_hz90
- **THEN** 结果100..100，保留本地端。

证据：`crates/codegen/shell/src/util/config/resolve/display_refresh.rs` — `pub fn resolve_display_refresh`；`crates/codegen/shell/src/util/config/resolve/display_refresh.rs` — `fn order_bounds`；`crates/codegen/shell/src/util/config/resolve/display_refresh.rs` — `fn wrong_typed_field_does_not_drop_siblings`。

### Requirement: Shell motion cadence decision and override composition

cadence决定 SHALL 依次检查probe关闭disabled、auto关闭flag_off、无Hz probe_skip、Hz超policy区间hz_out_of_range；否则round(1000/Hz)转u64且至少1，再clamp floor/ceiling，reason applied。merge逐时钟以传入Some环境值优先auto值再默认16ms，不再验证传入值；auto_applied要求auto存在且至少一个时钟未被环境覆盖。两个环境均Some且reason不是flag_off/disabled时改env_override，即使没有probe Hz。此模块不执行平台探测或实际绘制。

#### Scenario: Both clocks overridden without probe
- **WHEN** auto开启但Hz缺失，两个环境时钟都Some
- **THEN** 使用两环境值，auto_applied=false，reason env_override。

证据：`crates/codegen/shell/src/util/config/resolve/display_refresh.rs` — `pub fn decide_auto_cadence`；`crates/codegen/shell/src/util/config/resolve/display_refresh.rs` — `pub fn merge_motion_cadence`；`crates/codegen/shell/src/util/config/resolve/display_refresh.rs` — `pub fn resolve_motion_cadence`；`crates/codegen/shell/src/util/config/resolve/display_refresh.rs` — `fn both_env_reports_env_override_without_probe_hz`。

### Requirement: Shell thinking and tool grouping UI gates

show_thinking_blocks与group_tool_verbs SHALL 经共享BoolFlag按各自GROW_SHOW_THINKING_BLOCKS/GROW_GROUP_TOOL_VERBS环境、effective ui布尔、remote对应字段、默认true解析，并返回获胜ConfigSource。本地显式true可覆盖remote false，remote在此是较低优先级默认而非不可覆盖禁用；错型本地按缺失处理。resolver只给gate，不实现分组或删改模型消息。

#### Scenario: Local true over remote false
- **WHEN** 有效环境缺失，本地group_tool_verbs=true，remote=false
- **THEN** 结果true且来源Config。

证据：`crates/codegen/shell/src/util/config/resolve/ui.rs` — `pub fn resolve_show_thinking_blocks`；`crates/codegen/shell/src/util/config/resolve/ui.rs` — `pub fn resolve_group_tool_verbs`；`crates/codegen/shell/src/util/config/resolve/ui.rs` — `fn resolve_ui_bool`。

### Requirement: Shell mouse reporting command exposure gate

mouse reporting toggle SHALL 按GROW_MOUSE_REPORTING_TOGGLE环境、effective ui.mouse_reporting_toggle有效bool、已解析UiConfig字段、默认false解析；两个本地来源都标Config。effective显式false优先struct true，错型则可回退struct。该gate决定命令暴露配置，不在此打开终端鼠标模式或检测终端支持。

#### Scenario: Malformed effective value with parsed fallback
- **WHEN** effective字段错型但UiConfig字段Some(true)，环境缺失
- **THEN** 返回true且来源Config。

证据：`crates/codegen/shell/src/util/config/resolve/ui.rs` — `pub fn resolve_mouse_reporting_toggle`；`crates/codegen/shell/src/util/config/resolve/ui.rs` — `fn resolve_mouse_reporting_toggle_falls_back_to_ui_struct`。


### Requirement: Shell instruction path classification and home expansion
路径分类 SHALL 先将直接位于grow_home或其rules目录的文件判为user，即使位于workspace；其余workspace后代判非user，再判断grow_home前缀。此判断仅为Path词法比较，不检查文件名、存在性、符号链接或canonical identity。expand_home仅展开裸~和~/前缀，home不可用时保留原串，~user不展开。

#### Scenario: Workspace under Grow home
- **WHEN** 文件位于grow_home下workspace的深层目录
- **THEN** 保持project分类，直接home或rules文件例外。

源码证据：
- `crates/codegen/shell/src/util/mod.rs` — `pub(crate) fn is_user_instruction_path`。
- `crates/codegen/shell/src/util/mod.rs` — `pub(crate) fn expand_home`。


### Requirement: Pager permission mode value and persist policy

PermissionModeKind SHALL 在Ask/Auto/AlwaysApprove与ask/auto/always-approve及shell同名runtime枚举之间精确双向映射，未知canonical返回None；Auto不视为AlwaysApprove。PermissionModePersist::WithRollback携带旧canonical，约定磁盘失败回滚内存并抑制ACP通知，但不恢复soft-default latch；BestEffort不携带旧值，失败保持乐观内存状态且ACP通知不受磁盘成功约束。枚举只声明策略，具体执行由effect消费方承担。

#### Scenario: Unknown permission canonical
- **WHEN** 从未知字符串构造PermissionModeKind
- **THEN** 返回None，不回退到Ask。

#### Scenario: Permission persist strategies
- **WHEN** 创建typed setter与cycle mode的持久化effect
- **THEN** 前者使用WithRollback旧值，后者使用BestEffort且无可恢复载荷。

源码证据：`crates/codegen/pager/src/app/actions.rs` — `PermissionModeKind / PermissionModePersist / Effect::PersistPermissionMode`。


### Requirement: Pager startup display refresh probe scheduling

pager启动 SHALL 将GROW_MIN_DRAW_MS与GROW_SCROLL_CADENCE_MS分别trim解析为u64，缺失、空或非法使用默认值，并限制1..100ms；环境变量存在即视为set，即使内容非法。probe关闭时不调用显示刷新探测；probe开启且auto cadence开启、两个cadence环境变量未同时存在时，在固定paint clocks前同步探测；其他开启场景先按无Hz解析clocks，再在spawn_blocking诊断任务中异步探测。MotionClocks在start返回时固定为解析后的min_draw与scroll Duration，诊断任务还记录terminal快照、probe结果及effective cadence。

#### Scenario: Both cadence overrides present
- **WHEN** 两个cadence环境变量均存在且probe开启
- **THEN** 显示刷新率不影响已固定clocks，probe仅在后台用于诊断。

#### Scenario: Auto cadence needs refresh rate
- **WHEN** auto cadence开启且至少一个环境override缺失
- **THEN** 主路径同步探测一次并将Hz交给resolve_motion_cadence。

源码证据：`crates/codegen/pager/src/app/display_refresh_startup.rs` — `parse_cadence_ms / cadence_ms_from_env / start / spawn_terminal_and_display_refresh_diagnostics`。
### Requirement: Pager live Auto permission gate emergency downgrade

When a settings update carries auto_permission_mode_enabled, Pager SHALL replace `auto_mode_gate`. If false, it first captures live top-level session ids whose AgentSession is Auto, invokes the shared displayed-Auto downgrade, then fire-and-forget sends `grow/permission_mode_changed` with ask for each captured id over the shared ACP channel; send failures and response receivers are ignored. Whether disabling or enabling, slash permission commands are resynchronized. Absent gate fields leave this path untouched, and the captured notification list does not independently traverse child AgentViews.

#### Scenario: Disable
- **WHEN** the pushed gate is false
- **THEN** displayed Auto is downgraded and previously-Auto live root sessions receive ask notifications.

#### Scenario: Enable
- **WHEN** the pushed gate is true
- **THEN** no downgrade notification is sent and slash gates are resynchronized.

#### Scenario: No live id
- **WHEN** an Auto top-level agent has no session id
- **THEN** it is not included in outbound notifications.

#### Scenario: Transport failure
- **WHEN** the ACP send fails
- **THEN** the function does not retry or report that failure.

#### Scenario: Omitted field
- **WHEN** the update lacks the gate
- **THEN** existing gate and slash exposure are not changed by this block.

证据：`crates/codegen/pager/src/app/acp_handler/settings.rs` — `handle_settings_update`、`notify_sessions_leave_auto`。

### Requirement: Pager pushed permission soft-default latch

A presence-aware pushed permission_mode SHALL re-arm the future-session default only while `permission_mode_from_soft_default` remains true. Resolution uses one effective-config read and precedence `[ui]` over remote over Ask; requested Auto is clamped to Ask when the current auto gate is off. The resolved runtime enum and canonical current-ui string are replaced. Live sessions are untouched, nothing is persisted, omitted fields do nothing, and once the user has claimed the selector this update path ignores both null and string pushes.

#### Scenario: Soft latch
- **WHEN** the field is present and soft-default ownership remains
- **THEN** local UI config and remote value are resolved into the future-session default.

#### Scenario: Auto gated
- **WHEN** resolution requests Auto while the feature gate is false
- **THEN** Ask is stored.

#### Scenario: Explicit null
- **WHEN** the present field clears the remote value
- **THEN** resolution proceeds with remote absent.

#### Scenario: User-owned
- **WHEN** permission_mode_from_soft_default is false
- **THEN** the pushed field is ignored.

#### Scenario: Live session
- **WHEN** the future default changes
- **THEN** existing session permission modes are not changed here.

证据：`crates/codegen/pager/src/app/acp_handler/settings.rs` — `apply_soft_default_permission_mode`、`handle_settings_update`。

### Requirement: Pager settings display flag and session picker precedence

A valid settings update SHALL directly replace show_resolved_model when present. For a present session_picker_grouped remote value, resolution prefers `GROW_SESSION_PICKER_GROUPED` only for exact 1/true/0/false strings, then effective `[cli].session_picker_grouped` boolean, then the remote value. Invalid environment strings and config-load/type failures fall through. Absent fields preserve prior values. The handler duplicates startup precedence locally rather than calling a shared resolver.

#### Scenario: Display flag
- **WHEN** show_resolved_model is present
- **THEN** the app boolean is replaced.

#### Scenario: Environment
- **WHEN** the grouping env is an accepted exact string
- **THEN** it overrides config and remote.

#### Scenario: Config
- **WHEN** env is absent or invalid and effective config has a boolean
- **THEN** the config value overrides remote.

#### Scenario: Remote fallback
- **WHEN** neither higher source resolves
- **THEN** the pushed grouping value is stored.

#### Scenario: Omitted
- **WHEN** a field is absent
- **THEN** its prior app value remains.

证据：`crates/codegen/pager/src/app/acp_handler/settings.rs` — `handle_settings_update`。

### Requirement: Pager remote verb-grouping resolution and transcript invalidation

Every valid settings update SHALL load disk config once and resolve group_tool_verbs from local config plus the update's optional remote value, including None to clear a previous remote enable. Only when the resolved value differs from the appearance cache does Pager set the cache, clear group expansion and invalidate heights for every top-level agent scrollback and each direct child scrollback. It does not recursively invalidate grandchildren in this loop, preserve expansion ids across grouping shapes or report individual invalidation changes.

#### Scenario: Remote clear
- **WHEN** the update omits or clears a prior remote group value
- **THEN** local/default resolution is recomputed.

#### Scenario: No flip
- **WHEN** resolved grouping equals the cache
- **THEN** transcript expansion and heights are left alone.

#### Scenario: Flip
- **WHEN** resolved grouping differs
- **THEN** cache changes and root plus direct-child transcripts clear expansion and invalidate heights.

#### Scenario: Nested child
- **WHEN** a child owns its own subagent view
- **THEN** that grandchild is not visited by this loop.

证据：`crates/codegen/pager/src/app/acp_handler/settings.rs` — `handle_settings_update`。

### Requirement: Pager pushed tips and slash tags as local re-resolution signals

When a valid update carries tips, Pager SHALL ignore the pushed vector's contents as data, re-resolve tips from the already-loaded local disk config, choose and advance a tip using grow_home when nonempty, or clear the current tip when empty. When slash_command_tags is present as null or a valid map, Pager SHALL ignore the contained remote value and replace command tags by resolving effective local config; malformed tag values are deserialized as absent so they do not trigger re-resolution or fail sibling fields. Omitted fields preserve current projections.

#### Scenario: Tips signal
- **WHEN** tips is present
- **THEN** local tips are reloaded and current tip is advanced or cleared.

#### Scenario: Tags map
- **WHEN** slash_command_tags contains a valid map
- **THEN** its presence triggers local effective-config tag resolution.

#### Scenario: Tags clear
- **WHEN** slash_command_tags is explicit null
- **THEN** the same local re-resolution occurs.

#### Scenario: Malformed tags
- **WHEN** the field is present with another JSON shape
- **THEN** a warning is logged, tags stay unchanged and sibling settings still parse.

#### Scenario: Omitted
- **WHEN** neither signal is present
- **THEN** current tips and tags remain.

证据：`crates/codegen/pager/src/app/acp_handler/settings.rs` — `handle_settings_update`、`deserialize_settings_update_tags`。

### Requirement: Pager dashboard stable identity filters and durable preferences

The pager dashboard SHALL classify row state and parse explicit agent, state, PR-like and free-text filters without confusing ordinary dispatch text with a filter. Persistent pin and reorder identities SHALL serialize top-level sessions and descendants by session identity, resolve them against live agents with first-wins collision handling, drop unresolved or oversized input, preserve unrelated configuration tables, refuse to overwrite unparseable configuration, remove the retired onboarding table, and write replacement bytes atomically. In-memory garbage collection SHALL remove stale selection, pin, reorder, rename, peek, hover, click and attached-popup references while preserving surviving reorder order. This file does not prove row classification performed in row.rs, concurrent writer serialization, process-crash durability on every filesystem, or that external callers maintain public cursor invariants.

#### Scenario: Filter parsing
- **WHEN** search text uses a:, s:, # or plain syntax
- **THEN** it becomes the matching bounded filter value, with empty known prefixes clearing and unknown state tokens falling back to substring matching.

#### Scenario: Stable persisted identity
- **WHEN** pins or reorder positions cross a process restart
- **THEN** session-based keys resolve to current row ids, unresolved identities drop, and roster-only or session-less rows are not persisted.

#### Scenario: Conservative configuration write
- **WHEN** dashboard preferences are stored
- **THEN** unrelated TOML remains intact, retired onboarding data is removed, malformed nonempty TOML is not overwritten, and the replacement is written through a synced temporary file and rename.

#### Scenario: Stale reference collection
- **WHEN** live rows no longer contain previously referenced identities
- **THEN** all stale dashboard references and delete confirmation are cleared while surviving order is retained.

证据：`crates/codegen/pager/src/views/dashboard/state.rs`。

### Requirement: Pager UI preference model permission and setting persistence effects

Announcement visibility, memory fullscreen, project-picker opt-out, dashboard state, worktree mode, preferred model and generic settings SHALL be delegated to their persistence helpers or blocking writers. Worktree mode SHALL debug-assert one of two known config keys. Preferred-model persistence SHALL report failure for reducer rollback; generic setting persistence SHALL return the original rollback value on failure. Permission mode SHALL either use the combined persistence-and-notification helper or send grow/permission_mode_changed directly, with direct notification failure reduced to a warning. This file does not prove durable write atomicity, concurrent writer ordering, config schema validation, rollback application or remote permission-mode convergence.

#### Scenario: Generic setting succeeds
- **WHEN** persist_setting completes
- **THEN** SettingPersisted returns the key and requested value.

#### Scenario: Generic setting fails
- **WHEN** persist_setting fails
- **THEN** SettingPersistFailed returns key, rollback value and error.

#### Scenario: Dashboard persistence fails
- **WHEN** writing or joining the blocking dashboard task fails
- **THEN** the error is warned and the effect still returns CancelComplete.

#### Scenario: Permission notification fails
- **WHEN** grow/permission_mode_changed cannot be sent
- **THEN** the failure is warned and no failure-bearing TaskResult is produced.

证据：`crates/codegen/pager/src/app/root/effects/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/tools/config.rs shared tool runtime and notifications contract

crates/codegen/shell/src/tools/config.rs SHALL 维护 shared tool runtime and notifications 的入口 PRODUCTION_MAX_TIMEOUT_SECS, BashToolConfig, to_bash_params_json, exists, AskUserQuestionToolConfig, WebFetchToolConfig, resolve_params, ShellToolsetConfig, default, resolve_file_toolset, HashlineSchemeConfig, validate, FileToolset, tool_configs, file_toolset_default_is_standard, standard_toolset_configs, hashline_toolset_configs, file_toolset_override_never_grants_edit_to_read_only_toolsets (plus 21 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** PRODUCTION_MAX_TIMEOUT_SECS, BashToolConfig, to_bash_params_json, exists, AskUserQuestionToolConfig, WebFetchToolConfig, resolve_params, ShellToolsetConfig, default, resolve_file_toolset, HashlineSchemeConfig, validate, FileToolset, tool_configs, file_toolset_default_is_standard, standard_toolset_configs, hashline_toolset_configs, file_toolset_override_never_grants_edit_to_read_only_toolsets (plus 21 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/tools/config.rs`。

### Requirement: Shell crates/codegen/shell/src/util/config/mod.rs shell utility and configuration support contract

crates/codegen/shell/src/util/config/mod.rs SHALL 维护 shell utility and configuration support 的入口 the file module entrypoint。实现显示该边界包含 MCP integration boundary、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 the file module entrypoint 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/util/config/mod.rs`。

### Requirement: Shell crates/codegen/shell/src/util/config/resolve/mod.rs shell utility and configuration support contract

crates/codegen/shell/src/util/config/resolve/mod.rs SHALL 维护 shell utility and configuration support 的入口 the file module entrypoint。实现显示该边界包含 platform or feature-gated branches、MCP integration boundary、prompt/subagent/goal context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/util/config/resolve/mod.rs`。
### Requirement: Tools crates/codegen/tools/src/implementations/skills/discovery.rs skill discovery and listing runtime contract
crates/codegen/tools/src/implementations/skills/discovery.rs SHALL implement the skill discovery and listing runtime boundary through discover, filter, deduplicate, budget, and render skill/rule listings with retry state. Its source symbols MAX_DESCRIPTION_LEN, MAX_NAME_LEN, MAX_FRONTMATTER_BYTES, MAX_BODY_PEEK_BYTES, MAX_SKILL_WALK_DEPTH, SKILL_SUBDIRS, find_skill_paths, find_command_paths, scan_md_files, find_skill_md_paths, walk_for_skill_md, coerce_to_string, parse_boolean_frontmatter, yields, coerce_tool_list, split_top_level, coerce_path_list, normalize_skill_paths (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、repository/worktree scope; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/skills/discovery.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/skills/discovery.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/skills/discovery.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/skills/discovery.rs` — `MAX_DESCRIPTION_LEN`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `MAX_NAME_LEN`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `MAX_FRONTMATTER_BYTES`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `MAX_BODY_PEEK_BYTES`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `MAX_SKILL_WALK_DEPTH`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `SKILL_SUBDIRS`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `find_skill_paths`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `find_command_paths`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `scan_md_files`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `find_skill_md_paths`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `walk_for_skill_md`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `coerce_to_string`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `parse_boolean_frontmatter`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `yields`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `coerce_tool_list`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `split_top_level`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `coerce_path_list`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `normalize_skill_paths`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `parse_skill_paths`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `parse_metadata`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `ParsedFrontmatter`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `SkillParseError`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `normalize_skill_name`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `is_valid_skill_name`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `quote_problematic_values`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `needs_quoting`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `RECOVERABLE_KEYS`；`crates/codegen/tools/src/implementations/skills/discovery.rs` — `recover_scalar_fields`。

### Requirement: Tools crates/codegen/tools/src/implementations/skills/mod.rs skill discovery and listing runtime contract
crates/codegen/tools/src/implementations/skills/mod.rs SHALL implement the skill discovery and listing runtime boundary through discover, filter, deduplicate, budget, and render skill/rule listings with retry state. Its source symbols mod follow explicit markers typed inputs and outputs; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Primary path
- **WHEN** the main entrypoint in crates/codegen/tools/src/implementations/skills/mod.rs is called
- **THEN** typed output is produced according to its explicit conversion or dispatch path.

证据：`crates/codegen/tools/src/implementations/skills/mod.rs` — `mod`。

### Requirement: Tools crates/codegen/tools/src/implementations/skills/skill.rs skill discovery and listing runtime contract
crates/codegen/tools/src/implementations/skills/skill.rs SHALL implement the skill discovery and listing runtime boundary through discover, filter, deduplicate, budget, and render skill/rule listings with retry state. Its source symbols SkillInput, SkillOutput, build_skill_message, build_skill_block, SkillRef, build_skill_information, format_skill_name, extract_skill_display_text, extract_command_args, escape_xml, SubstitutionContext, apply_substitutions, resolve_skill_internal_links, extract_skill_body, load_skill_content, load_skill_with_body, test_escape_xml, test_extract_skill_body (additional symbols omitted from the title but included in source evidence) follow explicit markers serde/json wire or configuration、filesystem or durable persistence、explicit error classification、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、repository/worktree scope; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/skills/skill.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/skills/skill.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/implementations/skills/skill.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/implementations/skills/skill.rs` — `SkillInput`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `SkillOutput`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `build_skill_message`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `build_skill_block`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `SkillRef`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `build_skill_information`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `format_skill_name`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `extract_skill_display_text`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `extract_command_args`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `escape_xml`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `SubstitutionContext`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `apply_substitutions`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `resolve_skill_internal_links`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `extract_skill_body`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `load_skill_content`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `load_skill_with_body`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `test_escape_xml`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `test_extract_skill_body`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `test_extract_skill_body_no_frontmatter`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `load_skill_content_trusts_preloaded_body_with_leading_hr`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `load_skill_content_rejects_synthetic_path_without_body`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `test_format_skill_name`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `test_format_skill_name_plugin`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `test_build_skill_message_exact_format`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `test_build_skill_message_special_chars_in_fields`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `test_build_skill_message_empty_content`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `test_build_skill_message_multiline_content`；`crates/codegen/tools/src/implementations/skills/skill.rs` — `test_substitutions_arguments_full`。

### Requirement: Tools crates/codegen/tools/src/implementations/skills/types.rs skill discovery and listing runtime contract
crates/codegen/tools/src/implementations/skills/types.rs SHALL implement the skill discovery and listing runtime boundary through discover, filter, deduplicate, budget, and render skill/rule listings with retry state. Its source symbols SkillScope, default_true, fn, SkillInfo, dedup_key, label, skill_name_from_path, default, label_prefers_display_name_then_name, extracts_skill_name, nested_path, non_skill_returns_none, lowercase_returns_none, bare_skill_md_returns_none follow explicit markers serde/json wire or configuration、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context、repository/worktree scope; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/implementations/skills/types.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

证据：`crates/codegen/tools/src/implementations/skills/types.rs` — `SkillScope`；`crates/codegen/tools/src/implementations/skills/types.rs` — `default_true`；`crates/codegen/tools/src/implementations/skills/types.rs` — `fn`；`crates/codegen/tools/src/implementations/skills/types.rs` — `SkillInfo`；`crates/codegen/tools/src/implementations/skills/types.rs` — `dedup_key`；`crates/codegen/tools/src/implementations/skills/types.rs` — `label`；`crates/codegen/tools/src/implementations/skills/types.rs` — `skill_name_from_path`；`crates/codegen/tools/src/implementations/skills/types.rs` — `default`；`crates/codegen/tools/src/implementations/skills/types.rs` — `label_prefers_display_name_then_name`；`crates/codegen/tools/src/implementations/skills/types.rs` — `extracts_skill_name`；`crates/codegen/tools/src/implementations/skills/types.rs` — `nested_path`；`crates/codegen/tools/src/implementations/skills/types.rs` — `non_skill_returns_none`；`crates/codegen/tools/src/implementations/skills/types.rs` — `lowercase_returns_none`；`crates/codegen/tools/src/implementations/skills/types.rs` — `bare_skill_md_returns_none`。

### Requirement: Tools crates/codegen/tools/src/reminders/agents_md.rs reminder generation and completion display contract
crates/codegen/tools/src/reminders/agents_md.rs SHALL implement the reminder generation and completion display boundary through discover, filter, deduplicate, budget, and render skill/rule listings with retry state. Its source symbols discover follow explicit markers timeout, budget, or rate limit、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

#### Scenario: Policy boundary
- **WHEN** input crosses an allow/deny or sandbox check
- **THEN** the explicit policy branch controls admission before execution.

证据：`crates/codegen/tools/src/reminders/agents_md.rs` — `discover`。

### Requirement: Tools crates/codegen/tools/src/types/agents_md_tracker.rs tool type, schema, context, and output contract contract
crates/codegen/tools/src/types/agents_md_tracker.rs SHALL implement the tool type, schema, context, and output contract boundary through discover, filter, deduplicate, budget, and render skill/rule listings with retry state. Its source symbols AGENT_FILENAME, RULES_DIR, DISCOVERY_TIMEOUT, MAX_WALK_DEPTH, MAX_RULE_ENTRIES, MAX_RULE_FILES, MAX_FILE_BYTES, MAX_DISCOVERY_BYTES, AgentsMdTracker, AgentsMdDiscovery, new, seed, check_paths, scan, is_ignored, append_to_prompt, on_compaction, reminded_paths (additional symbols omitted from the title but included in source evidence) follow explicit markers filesystem or durable persistence、explicit error classification、timeout, budget, or rate limit、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/types/agents_md_tracker.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/types/agents_md_tracker.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

#### Scenario: Budget boundary
- **WHEN** a timeout, size, rate, or token budget is reached
- **THEN** the source applies its configured cap or timeout path and reports the corresponding result.

证据：`crates/codegen/tools/src/types/agents_md_tracker.rs` — `AGENT_FILENAME`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `RULES_DIR`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `DISCOVERY_TIMEOUT`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `MAX_WALK_DEPTH`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `MAX_RULE_ENTRIES`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `MAX_RULE_FILES`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `MAX_FILE_BYTES`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `MAX_DISCOVERY_BYTES`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `AgentsMdTracker`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `AgentsMdDiscovery`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `new`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `seed`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `check_paths`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `scan`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `is_ignored`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `append_to_prompt`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `on_compaction`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `reminded_paths`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `ignored`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `denied`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `read_regular`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `busy_scan_is_shared_across_snapshots_and_remains_retryable`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `discovery_requires_delivery_and_compaction_rejects_stale_scans`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `discarded_discovery_and_reseed_leave_rules_retryable`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `scan_bounds_deadline_rule_count_and_total_bytes`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `special_files_are_rejected_without_blocking_or_acknowledging`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `access_and_deliver`；`crates/codegen/tools/src/types/agents_md_tracker.rs` — `build_test_gitignore`。
### Requirement: Pager task-result test: bundle_status_ready_populates_state
BundleStatusReady SHALL populate cache presence, version, agent names, and skill names in bundle state.

#### Scenario: Bundle status success
- **WHEN** a ready bundle status contains all fields
- **THEN** bundle state mirrors the response.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `bundle_status_ready_populates_state`。

### Requirement: Pager task-result test: bundle_status_failed_logs_but_keeps_state
BundleStatusFailed SHALL report the failure without clearing previously known bundle state.

#### Scenario: Bundle status failure
- **WHEN** status retrieval fails after a cached state exists
- **THEN** cache and version remain unchanged.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `bundle_status_failed_logs_but_keeps_state`。

### Requirement: Pager task-result test: catalog_entry_ready_opens_viewer
A ready catalog entry SHALL open the active agent block viewer as plain text containing the entry.

#### Scenario: Catalog entry success
- **WHEN** catalog content loads successfully
- **THEN** the active agent has a plain-text block viewer.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `catalog_entry_ready_opens_viewer`。

### Requirement: Pager task-result test: catalog_entry_failed_shows_system_message
A failed catalog entry load SHALL leave the block viewer closed and append a system error message.

#### Scenario: Catalog entry failure
- **WHEN** catalog retrieval fails
- **THEN** scrollback grows and no viewer is opened.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `catalog_entry_failed_shows_system_message`。

### Requirement: Pager task-result test: rollback_known_key_reverts_cache_and_no_effect
A known setting persistence failure SHALL roll back its in-memory UI value without emitting another effect.

#### Scenario: Known setting rollback
- **WHEN** compact_mode persistence fails with a false rollback value
- **THEN** the value is false and no effect is emitted.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `rollback_known_key_reverts_cache_and_no_effect`。

### Requirement: Pager task-result test: rollback_unknown_key_does_not_panic
An unknown setting rollback key SHALL surface an inconsistency without panicking or emitting an effect.

#### Scenario: Unknown setting rollback
- **WHEN** persistence fails for an unrecognized key
- **THEN** dispatch returns safely with no effects.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `rollback_unknown_key_does_not_panic`。

### Requirement: Pager task-result test: persist_failed_toast_contains_key_and_error
A setting persistence failure toast SHALL include the setting key, error text, and failure marker.

#### Scenario: Setting failure toast
- **WHEN** compact_mode persistence is denied
- **THEN** the toast contains all three user-facing details.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `persist_failed_toast_contains_key_and_error`。

### Requirement: Pager task-result test: rollback_reverts_thread_local_cache_too
Setting rollback SHALL update both AppView state and the thread-local appearance cache so rendering cannot retain the optimistic value.

#### Scenario: Thread-local setting rollback
- **WHEN** a compact-mode optimistic update is rolled back inside a thread
- **THEN** both caches become false.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `rollback_reverts_thread_local_cache_too`。
### Requirement: Nix environment clearing boundary
clearenv SHALL remove all process environment variables through libc where available or a fallback iteration, returning ClearEnvError when the platform operation reports failure; the API remains explicitly unsafe/non-threadsafe.

#### Scenario: Nix environment clearing boundary implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/env.rs` — `struct ClearEnvError`；`third_party/nix-ohos/src/env.rs` — `fn fmt`；`third_party/nix-ohos/src/env.rs` — `fn clearenv`。
