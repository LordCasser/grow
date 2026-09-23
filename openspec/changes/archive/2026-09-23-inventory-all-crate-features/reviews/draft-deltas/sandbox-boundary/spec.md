## ADDED Requirements

### Requirement: Sandbox enforcement feature availability
Sandbox SHALL 默认启用enforce，内核实现限enforce+Unix，其他构建apply为stub；nono固定0.53.0。

#### Scenario: Sandbox enforcement feature availability boundary
- **WHEN** 禁用enforce或非Unix调用apply
- **THEN** 返回Ok且不设置applied；依赖存在不证明当前平台可实施。

证据：`crates/codegen/sandbox/Cargo.toml` — `enforce`。

### Requirement: Sandbox requested and installed state
Sandbox SHALL 分开保存配置请求与安装状态，OnceLock只接受首次值；auto_allow_bash还要求实际active。

#### Scenario: Sandbox requested and installed state boundary
- **WHEN** install未applied的manager
- **THEN** profile_name为空，metrics和log_violation仍可使用已安装logger；请求profile不代表已隔离。

证据：`crates/codegen/sandbox/src/lib.rs` — `requested_confinement_profile`。

### Requirement: Sandbox apply result boundary
SandboxManager SHALL 传播配置/能力构造与Hook保护准备错误，但对后端unsupported或apply错误记录失败后返回Ok。

#### Scenario: Sandbox apply result boundary boundary
- **WHEN** apply返回Ok
- **THEN** 调用方仍需检查is_applied；Off和stub亦可Ok，不推导内核已生效。

证据：`crates/codegen/sandbox/src/lib.rs` — `SandboxManager`。

### Requirement: Sandbox profile configuration precedence
Sandbox配置 SHALL 全局先加载，项目只能补新名称；冲突查询报告不同的同名Custom配置。

#### Scenario: Sandbox profile configuration precedence boundary
- **WHEN** 全局读取或TOML解析失败
- **THEN** 该文件视为无配置，读取错误静默、解析错误警告；无冲突报告不证明加载成功。

证据：`crates/codegen/sandbox/src/profiles.rs` — `load_sandbox_config`。

### Requirement: Sandbox built in profile resolution
内建profile SHALL 区分Workspace全局可读与workspace/state/temp可写、ReadOnly全局可读及state/temp可写、Strict系统读取表加workspace写、Devbox根目录枚举写。

#### Scenario: Sandbox built in profile resolution boundary
- **WHEN** 使用ReadOnly或Strict
- **THEN** ReadOnly并非完全禁止写入；Strict显式系统读取范围和设备表见审阅记录，网络配置由子进程启动点实施。

证据：`crates/codegen/sandbox/src/profiles.rs` — `resolve_profile`。

### Requirement: Sandbox custom profile inheritance
Custom SHALL 默认继承Workspace，只允许继承非Off内建profile，追加read_only/read_write/deny并按Option覆盖network。

#### Scenario: Sandbox custom profile inheritance boundary
- **WHEN** 自定义read_only覆盖base已有write路径
- **THEN** 追加read不会撤销已有write；继承Custom或Off失败，Devbox继承不启用Hook写禁。

证据：`crates/codegen/sandbox/src/profiles.rs` — `ProfileConfig`。

### Requirement: Sandbox filesystem grants and devices
能力转换 SHALL 跳过不存在read路径，尝试创建缺失write目录，单独授权设备文件与目录。

#### Scenario: Sandbox filesystem grants and devices boundary
- **WHEN** write目录创建失败或设备不可打开
- **THEN** 前者警告跳过，设备NotFound/ENXIO/ENODEV跳过；部分其他授权错误传播，不保证所有配置路径都已授权。

证据：`crates/codegen/sandbox/src/profiles.rs` — `capability_set_from_profile`。

### Requirement: Sandbox temporary writable roots
临时可写路径 SHALL 包括/tmp和/var/tmp、macOS存在的private临时根以及存在且为目录的TMPDIR。

#### Scenario: Sandbox temporary writable roots boundary
- **WHEN** TMPDIR为其他目录
- **THEN** 加入列表，不在此处验证可信根或canonical去重；macOS包含整个/private/var/folders。

证据：`crates/codegen/sandbox/src/paths.rs` — `temp_writable_paths`。

### Requirement: Sandbox event and logger persistence
SandboxLogger SHALL 累计事件与relaxed指标，flush将内存队列取出并追加JSONL。

#### Scenario: Sandbox event and logger persistence boundary
- **WHEN** flush写入失败或Mutex poison
- **THEN** 已drain事件不回队；poison时指标可增但事件丢弃，没有fsync/轮转或持久成功保证。

证据：`crates/codegen/sandbox/src/logging.rs` — `flush_to_disk`。

### Requirement: Sandbox exact deny path resolution
精确deny SHALL 相对workspace拼接、排序去重；目录类型按当前is_dir决定。

#### Scenario: Sandbox exact deny path resolution boundary
- **WHEN** 不存在的路径日后成为目录
- **THEN** macOS原先literal不会自动覆盖子树；effective集合不是filesystem canonical化。

证据：`crates/codegen/sandbox/src/deny/mod.rs` — `effective_deny_paths`。

### Requirement: Sandbox Seatbelt deny rule emission
macOS SHALL 对原路径、canonical及private aliases发射read/write和八个具体write动作deny，拒绝无法表达路径。

#### Scenario: Sandbox Seatbelt deny rule emission boundary
- **WHEN** deny与宽write授权竞争
- **THEN** 依赖固定nono与真实e2e核对，不能仅以规则构造成功证明优先级；Linux该入口不发deny，依赖bwrap。

证据：`crates/codegen/sandbox/src/deny/mod.rs` — `apply_deny_paths_to_capability_set`。

### Requirement: Sandbox direct hook ancestor write protection
macOS Hook写禁 SHALL 保持读取，保护leaf并对最深包含write root内现存祖先发create/unlink deny。

#### Scenario: Sandbox direct hook ancestor write protection boundary
- **WHEN** source位于所有write roots外
- **THEN** 没有额外祖先规则；匹配root使用路径前缀，详细alias及边界见审阅记录。

证据：`crates/codegen/sandbox/src/deny/mod.rs` — `ancestors_within_writable_roots`。

### Requirement: Sandbox deny glob dialect
deny含*、?、[ SHALL 进入受限glob语法，拒绝brace/backslash、非独立组件**和不支持字符类。

#### Scenario: Sandbox deny glob dialect boundary
- **WHEN** 纯brace字符串不含检测元字符
- **THEN** 分流为literal而非glob；macOS转换锚定regex，有限parity测试不是全输入内核等价证明。

证据：`crates/codegen/sandbox/src/deny/glob.rs` — `partition_deny_entries`。

### Requirement: Sandbox Linux deny glob expansion
Linux glob SHALL 在启动时关闭ignore/hidden过滤且不下钻symlink，默认depth64、matches4096、visited200000。

#### Scenario: Sandbox Linux deny glob expansion boundary
- **WHEN** 超预算、非权限遍历错误或非UTF8匹配
- **THEN** 拒绝计划；PermissionDenied跳过，后建匹配不覆盖，条目上限不是wall-clock deadline。

证据：`crates/codegen/sandbox/src/deny/glob.rs` — `expand_deny_globs`。

### Requirement: Sandbox bubblewrap command construction
Linux bwrap SHALL 构造cap-drop ALL、根bind、optional只读write路径、Hook计划、read占位覆盖及dev/proc挂载，再reexec原程序。

#### Scenario: Sandbox bubblewrap command construction boundary
- **WHEN** marker已存在或构造失败
- **THEN** 返回None，调用方不能把None当隔离证明；optional不存在write路径跳过，read占位失败拒绝。

证据：`crates/codegen/sandbox/src/lib.rs` — `bwrap_reexec_command_ex`。

### Requirement: Sandbox read deny placeholders
Linux读禁占位 SHALL 在Grow home使用PID后缀的文件或目录chmod000并ro-bind。

#### Scenario: Sandbox read deny placeholders boundary
- **WHEN** 已有同类型占位或路径别名
- **THEN** 复用并chmod；没有随机独占或完整no-follow保证，root读取可能空而非权限错误。

证据：`crates/codegen/sandbox/src/lib.rs` — `bwrap_blocked_placeholder`。

### Requirement: Sandbox required protection classification
保护需求 SHALL 按原始profile/config判定，Custom非空deny要求read deny，非Devbox/Off通常要求Hook写禁。

#### Scenario: Sandbox required protection classification boundary
- **WHEN** glob展开失败或Custom直接继承Devbox
- **THEN** 前者不因结果空而抹去原始read需求；后者豁免Hook写禁，不能只看基础predicate。

证据：`crates/codegen/sandbox/src/lib.rs` — `requires_read_deny`。

### Requirement: Sandbox hook source identity snapshot
Hook写保护 SHALL 校验global来源、配置路径存在性、直接JSON别名和leaf dev/ino/type/nlink。

#### Scenario: Sandbox hook source identity snapshot boundary
- **WHEN** symlink或hardlink出现
- **THEN** 拒绝；同inode原地内容变化不由身份快照检测，snapshot不是内容hash。

证据：`crates/codegen/sandbox/src/hook_write_deny.rs` — `capture_path_identity`。

### Requirement: Sandbox hook mount plan revalidation
Hook bwrap计划 SHALL 重验leaf身份和目录JSON集合，再添加祖先RW及leaf RO绑定。

#### Scenario: Sandbox hook mount plan revalidation boundary
- **WHEN** 祖先更换或检查后发生路径变化
- **THEN** 祖先只重查非symlink目录，不保存原inode；Command执行前仍有时间窗口，不是FD绑定保证。

证据：`crates/codegen/sandbox/src/hook_write_deny.rs` — `revalidate_plan`。

### Requirement: Sandbox hook readonly verification
Linux Hook生效检查 SHALL 先安装TSYNC namespace filter，再重新解析leaf并检查statvfs ST_RDONLY。

#### Scenario: Sandbox hook readonly verification boundary
- **WHEN** 首次安装失败或非Linux调用
- **THEN** Linux失败Result被OnceLock缓存；非Linux返回Ok stub，不能当Seatbelt检查。

证据：`crates/codegen/sandbox/src/hook_write_deny.rs` — `verify_hook_write_deny_enforced`。

### Requirement: Sandbox child network syscall filter
Linux child网络filter SHALL 拒绝八socket和三io_uring入口，先拒绝未知audit arch和x32，其他syscall允许。

#### Scenario: Sandbox child network syscall filter boundary
- **WHEN** 使用继承连接的read/write或非Linux
- **THEN** 此filter不阻断这些read/write，非Linux入口Ok no-op；需调用方在pre-exec安装。

证据：`crates/codegen/sandbox/src/child_net.rs` — `install_child_network_filter`。

### Requirement: Sandbox namespace syscall filter
Linux namespace filter SHALL TSYNC阻断unshare/setns及含NEW标记clone，clone3返回ENOSYS，普通clone允许。

#### Scenario: Sandbox namespace syscall filter boundary
- **WHEN** TSYNC返回失败TID
- **THEN** 返回错误但此前NO_NEW_PRIVS不回滚；filter自身不是完整mount/capability隔离。

证据：`crates/codegen/sandbox/src/child_net.rs` — `install_namespace_lockdown_filter`。

### Requirement: Sandbox exact website policy model
网站策略模型 SHALL 规范化HTTP(S) DNS origin与非零有效端口，精确deny优先allow再default。

#### Scenario: Sandbox exact website policy model boundary
- **WHEN** 只构造Websites策略
- **THEN** 不触发DNS或运行时实施；拒绝IP/userinfo/path/wildcard，子域不自动继承规则。

证据：`crates/codegen/sandbox/src/network_policy.rs` — `WebsiteOrigin`。

### Requirement: Sandbox versioned website policy snapshot
网络策略快照 SHALL 用版本1的紧凑serde JSON及排序集合生成SHA256，并先检查版本再解析policy。

#### Scenario: Sandbox versioned website policy snapshot boundary
- **WHEN** 输入非规范空白或hash大小写变化
- **THEN** 解析不要求原文本canonical；hash比较忽略ASCII大小写，不是签名或来源认证，也未持久到session。

证据：`crates/codegen/sandbox/src/network_policy.rs` — `NetworkPolicySnapshot`。

### Requirement: Sandbox enforcement verification scope
sandbox验证 SHALL 区分真实子进程e2e、构造测试与纯打印smoke；REQUIRE开关使后端不可用失败。

#### Scenario: Sandbox enforcement verification scope boundary
- **WHEN** macOS通过当前e2e
- **THEN** 支持记录7个实际内核场景，不证明Linuxseccomp/bwrap执行；smoke无预期失败退出码。

证据：`crates/codegen/sandbox/tests/deny_paths_e2e.rs` — `skip_if_enforcement_unavailable`。


### Requirement: Shell sandbox settings source and profile parsing

SandboxSettingsConfig SHALL 在effective配置加载/反序列化失败时默认；profile按非空CLI、trim后非空env、非空TOML、off选择，CLI/TOML只检查完全空字符串，不trim。auto_allow_bash按env、配置、false解析。ProfileName解析识别内置别名，其余一律Custom，因此apply_sandbox中的解析错误回退Off在当前FromStr下不会由未知名称触发；未知名称仍走custom加载。

#### Scenario: Whitespace CLI profile
- **WHEN** CLI profile只含空格
- **THEN** 仍优先选中该字符串并作为Custom，不回退环境profile。

证据：`crates/codegen/shell/src/agent/config.rs` — `impl SandboxSettingsConfig`；`crates/codegen/shell/src/agent/config.rs` — `pub(crate) fn resolve_string_flag`；`crates/codegen/sandbox/src/profiles.rs` — `impl std::str::FromStr for ProfileName`。

### Requirement: Shell startup sandbox enforcement failure policy

apply_sandbox SHALL 使用传入设置或加载effective设置，记录resolved配置profile并设置auto_allow_bash；workspace优先canonicalize传入cwd，失败转current_dir，再失败用点路径。Linux如read-deny或hook-write-deny要求bwrap，reexec失败或无可用命令且未处于bwrap时退出1；自报inside且需要hook保护时验证挂载，失败退出1。非Off建立manager并尝试apply，失败先警告；macOS custom或hook保护未applied退出1，Linux同条件仅在也不inside_bwrap时退出1，随后额外验证inside bwrap的hook保护。其余情形可继续install，Off不创建manager。此契约描述启动分支，不证明所有OS内核隔离机制已实际生效。

#### Scenario: Required Linux bwrap failure
- **WHEN** 需要read-deny但bwrap exec失败
- **THEN** 直接退出1，不进入Landlock降级分支。

证据：`crates/codegen/shell/src/config/mod.rs` — `pub fn apply_sandbox`。
### Requirement: Pager resume sandbox resolution and interactive prompt admission

PagerArgs SHALL 把`--resume <nonempty>`及fork-from显式源分类为SessionId，把裸`--resume`或`--continue`分类为MostRecentForCwd，把新会话和`--session-id`单独使用分类为None；无效flag组合若绕过Clap，resume_target也降为None。startup_sandbox_profile忽略空显式值，并比较显式与保存profile解析成ProfileName后的Option：readonly/read-only及none/off等别名相等时允许显式拼写，解析结果不同则Conflict；只有显式值时即使该层不能解析也返回Apply(Some(raw))，两边都解析失败也按相等处理，合法性留给后续sandbox应用。initial_prompt只读取positional prompt并trim两端空白，空白结果为None；它不读取headless single/json/file。可选worktree无值以空串表示，值形式可与interactive prompt组合。

#### Scenario: Sandbox alias on resume
- **WHEN** 显式readonly而保存值为read-only
- **THEN** 解析为同一ProfileName并采用显式readonly，不报告Conflict。

#### Scenario: Whitespace positional prompt
- **WHEN** 交互位置参数只有空白
- **THEN** 解析成功但initial_prompt返回None；不自动转为headless输入。

证据：`crates/codegen/pager/src/app/cli.rs` — `ResumeTarget / PagerArgs::session_to_resume / resume_most_recent / resume_target / startup_sandbox_profile / resolve_startup_sandbox / initial_prompt`。
### Requirement: Tools crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs web fetch, cache, artifact, and SSRF boundary contract
crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs SHALL implement the web fetch, cache, artifact, and SSRF boundary boundary through validate URL/domain and SSRF policy, fetch bounded HTTP content, cache or materialize artifacts, and classify overflow/errors. Its source symbols is_explicit_local_host, is_non_public_ip, is_non_public_ipv4, ipv4_in_cidr, is_non_public_ipv6, is_loopback_addr, is_blocked_for_host, check_ssrf, blocks_rfc1918_10x, blocks_rfc1918_172x, blocks_rfc1918_192168, blocks_link_local, blocks_cgnat_cloud_metadata, blocks_unspecified, blocks_testnet_reserved_and_this_network, blocks_loopback_by_default, allows_explicit_loopback_when_local_binding_enabled, rebinding_hostname_to_loopback_stays_blocked (additional symbols omitted from the title but included in source evidence) follow explicit markers explicit error classification、channel, fanout, or acknowledgement flow、platform/feature conditional、sandbox, trust, or allow/deny policy、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Policy boundary
- **WHEN** input crosses an allow/deny or sandbox check
- **THEN** the explicit policy branch controls admission before execution.

证据：`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `is_explicit_local_host`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `is_non_public_ip`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `is_non_public_ipv4`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `ipv4_in_cidr`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `is_non_public_ipv6`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `is_loopback_addr`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `is_blocked_for_host`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `check_ssrf`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `blocks_rfc1918_10x`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `blocks_rfc1918_172x`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `blocks_rfc1918_192168`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `blocks_link_local`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `blocks_cgnat_cloud_metadata`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `blocks_unspecified`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `blocks_testnet_reserved_and_this_network`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `blocks_loopback_by_default`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `allows_explicit_loopback_when_local_binding_enabled`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `rebinding_hostname_to_loopback_stays_blocked`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `explicit_local_host_detection`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `allows_public_ips`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `blocks_ipv6_link_local`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `blocks_ipv6_unique_local`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `blocks_ipv4_mapped_ipv6_private`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `allows_ipv4_mapped_ipv6_public`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `ssrf_blocks_ip_literal_private`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `ssrf_blocks_loopback_literal_by_default`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `ssrf_allows_loopback_literal_when_opted_in`；`crates/codegen/tools/src/implementations/grow_build/web_fetch/ssrf.rs` — `ssrf_allows_ip_literal_public`。

### Requirement: Tools crates/codegen/tools/src/util/env.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/env.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols GROW_AGENT_ENV, GROW_AGENT_ENV_VALUE, apply_agent_marker, substitute_plugin_tokens, ALL_TOKENS, expands_canonical_tokens_when_both_provided, leaves_tokens_literal_when_both_none, expands_only_root_when_data_none, agent_marker_constants_match_cursor_parity follow explicit markers child process execution、platform/feature conditional、tool definition, schema, or registry projection; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/util/env.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/util/env.rs` — `GROW_AGENT_ENV`；`crates/codegen/tools/src/util/env.rs` — `GROW_AGENT_ENV_VALUE`；`crates/codegen/tools/src/util/env.rs` — `apply_agent_marker`；`crates/codegen/tools/src/util/env.rs` — `substitute_plugin_tokens`；`crates/codegen/tools/src/util/env.rs` — `ALL_TOKENS`；`crates/codegen/tools/src/util/env.rs` — `expands_canonical_tokens_when_both_provided`；`crates/codegen/tools/src/util/env.rs` — `leaves_tokens_literal_when_both_none`；`crates/codegen/tools/src/util/env.rs` — `expands_only_root_when_data_none`；`crates/codegen/tools/src/util/env.rs` — `agent_marker_constants_match_cursor_parity`。

### Requirement: Tools crates/codegen/tools/src/util/shell_env_policy.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/shell_env_policy.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols EnvironmentVariablePattern, deserialize_patterns, ShellEnvironmentPolicyInherit, ShellEnvironmentPolicy, default, is_noop, matches_default_exclude, matches_exclude, matches_include_only, allows, allows_with_inherit, DEFAULT_SECRET_EXCLUDES, CORE_ENV_VARS, create_env, create_env_from_vars, install_policy_base_env, apply_shell_environment_policy follow explicit markers serde/json wire or configuration、explicit error classification、child process execution、platform/feature conditional、tool definition, schema, or registry projection、session, prompt, goal, or subagent context; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Error classification
- **WHEN** an input reaches an explicit error/result branch in crates/codegen/tools/src/util/shell_env_policy.rs
- **THEN** the implementation returns or propagates its declared error without turning failure into a successful tool result.

#### Scenario: Wire/config projection
- **WHEN** a typed input or output crosses the crates/codegen/tools/src/util/shell_env_policy.rs boundary
- **THEN** serde/json field names, defaults, and unknown-field behavior follow the source declarations.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/util/shell_env_policy.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/util/shell_env_policy.rs` — `EnvironmentVariablePattern`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `deserialize_patterns`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `ShellEnvironmentPolicyInherit`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `ShellEnvironmentPolicy`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `default`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `is_noop`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `matches_default_exclude`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `matches_exclude`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `matches_include_only`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `allows`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `allows_with_inherit`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `DEFAULT_SECRET_EXCLUDES`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `CORE_ENV_VARS`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `create_env`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `create_env_from_vars`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `install_policy_base_env`；`crates/codegen/tools/src/util/shell_env_policy.rs` — `apply_shell_environment_policy`。

### Requirement: Tools crates/codegen/tools/src/util/shell_env_policy_tests.rs tool utility and boundary validation contract
crates/codegen/tools/src/util/shell_env_policy_tests.rs SHALL implement the tool utility and boundary validation boundary through provide bounded path, environment, hashing, image, truncation, encoding, or process helper semantics. Its source symbols vars, patterns, apply_policy_reshapes_command_env, apply_noop_or_absent_policy_leaves_command_untouched, default_excludes_drop_secrets_when_enabled, inherit_none_starts_empty_then_set_applies, inherit_core_keeps_only_core_vars, exclude_and_include_only_filter, allows_filters_by_name_case_insensitively, allows_with_inherit_honors_inherit follow explicit markers child process execution; errors, cancellation, persistence, and platform branches are only those visible in the implementation.

#### Scenario: Resource boundary
- **WHEN** crates/codegen/tools/src/util/shell_env_policy_tests.rs performs its filesystem or process operation
- **THEN** the operation follows the implementation’s bounded, cleanup, and failure paths; no stronger durability is inferred.

证据：`crates/codegen/tools/src/util/shell_env_policy_tests.rs` — `vars`；`crates/codegen/tools/src/util/shell_env_policy_tests.rs` — `patterns`；`crates/codegen/tools/src/util/shell_env_policy_tests.rs` — `apply_policy_reshapes_command_env`；`crates/codegen/tools/src/util/shell_env_policy_tests.rs` — `apply_noop_or_absent_policy_leaves_command_untouched`；`crates/codegen/tools/src/util/shell_env_policy_tests.rs` — `default_excludes_drop_secrets_when_enabled`；`crates/codegen/tools/src/util/shell_env_policy_tests.rs` — `inherit_none_starts_empty_then_set_applies`；`crates/codegen/tools/src/util/shell_env_policy_tests.rs` — `inherit_core_keeps_only_core_vars`；`crates/codegen/tools/src/util/shell_env_policy_tests.rs` — `exclude_and_include_only_filter`；`crates/codegen/tools/src/util/shell_env_policy_tests.rs` — `allows_filters_by_name_case_insensitively`；`crates/codegen/tools/src/util/shell_env_policy_tests.rs` — `allows_with_inherit_honors_inherit`。
### Requirement: Nono Cargo package and feature boundary
nono SHALL be vendored at version 0.53.0 with the workspace edition/MSRV, default system-keyring feature, target-specific Landlock/nix/keyring dependencies, build.rs schema generation, explicit library path, and schema integration tests.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/Cargo.toml` — `[package]`；`third_party/nono/Cargo.toml` — `[features]`；`third_party/nono/Cargo.toml` — `[target.`。

### Requirement: Nono upstream manifest provenance
Cargo.toml.orig SHALL preserve upstream nono package identity, sandbox metadata, target dependencies, and original trust dependency intent as provenance for generated/local manifest differences.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/Cargo.toml.orig` — `[package]`；`third_party/nono/Cargo.toml.orig` — `[dependencies]`。

### Requirement: Nono Apache license resource
The vendored package SHALL retain the Apache License 2.0 text.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/LICENSE` — `Apache License, Version 2.0`。

### Requirement: Nono README platform contract
The README SHALL describe capability construction, irreversible application, Landlock Linux support, Seatbelt macOS support, child inheritance, and unsupported-platform behavior.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/README.md` — `Capability-based sandboxing library using Landlock (Linux) and Seatbelt (macOS)`。

### Requirement: Nono capability manifest schema
The schema SHALL require version 0.1.0 and describe filesystem, network, credentials/injection, process, and rollback fields while allowing additional properties during development.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/schema/capability-manifest.schema.json` — `"required"`；`third_party/nono/schema/capability-manifest.schema.json` — `"$defs"`。

### Requirement: Nono manifest type code generation
build.rs SHALL rerun on schema changes, parse the embedded JSON schema, generate typify Rust types, pretty print them, and write capability_manifest_types.rs under OUT_DIR; invalid inputs fail generation.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/build.rs` — `fn main`；`third_party/nono/build.rs` — `include_str!("schema/capability-manifest.schema.json")`。

### Requirement: Nono public module and re-export surface
The crate SHALL expose capability, diagnostic, error, keystore, manifest/conversion, network, path/query, sandbox, scrub/state, supervisor, trust, and undo modules, re-exporting principal public APIs with Linux-only ABI exports target-gated.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/lib.rs` — `pub mod capability`；`third_party/nono/src/lib.rs` — `pub use capability`；`third_party/nono/src/lib.rs` — `pub use sandbox`。

### Requirement: Nono typed error boundary
NonoError SHALL distinguish path/type, capability, platform/backend, keystore/config, process/undo/trust/network, IO, cancellation and setup failures through displayable variants and expose Result<T>.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/error.rs` — `pub enum NonoError`；`third_party/nono/src/error.rs` — `pub type Result`。

### Requirement: Nono filesystem and socket capability model
FsCapability SHALL canonicalize before type checking and retain original/resolved/source/access; UnixSocketCapability SHALL support connect or connect+bind files and non-recursive directory grants with component-wise coverage; access modes enforce explicit containment.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/capability.rs` — `FsCapability::new_dir`；`third_party/nono/src/capability.rs` — `FsCapability::new_file`；`third_party/nono/src/capability.rs` — `UnixSocketCapability::new_file`；`third_party/nono/src/capability.rs` — `UnixSocketCapability::new_dir`；`third_party/nono/src/capability.rs` — `pub fn covers`；`third_party/nono/src/capability.rs` — `pub fn contains`。

### Requirement: Nono capability builders and deduplication
CapabilitySet SHALL build filesystem/socket/network/process/IPC/command/platform permissions, reject unsafe root Seatbelt rules, deduplicate grants with user/profile precedence and access/mode upgrades, answer path coverage, remap procfs self references, and render a summary.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/capability.rs` — `pub fn platform_rule`；`third_party/nono/src/capability.rs` — `pub fn deduplicate`；`third_party/nono/src/capability.rs` — `pub fn path_covered_with_access`；`third_party/nono/src/capability.rs` — `pub fn remap_procfs_self_references`；`third_party/nono/src/capability.rs` — `pub fn summary`。

### Requirement: Nono ancestor canonicalization
try_canonicalize SHALL return full canonicalization when possible and otherwise canonicalize the longest existing ancestor before reappending missing components.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/path.rs` — `pub fn try_canonicalize`；`third_party/nono/src/path.rs` — `pub(crate) fn try_canonicalize_ancestor_walk`。

### Requirement: Nono non-applying permission query
QueryContext SHALL query path and network permissions without applying a sandbox, using canonical paths and original aliases for non-existent paths and distinguishing allowed, insufficient, ungranted, and blocked reasons.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/query.rs` — `pub fn query_path`；`third_party/nono/src/query.rs` — `pub fn query_network`。

### Requirement: Nono sandbox state persistence and revalidation
SandboxState SHALL serialize filesystem/socket/network state and reconstruct only through canonicalizing constructors, rejecting invalid modes, missing paths, and Unix-socket resolved-path drift.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/state.rs` — `pub fn from_caps`；`third_party/nono/src/state.rs` — `pub fn to_caps`；`third_party/nono/src/state.rs` — `pub fn to_json`；`third_party/nono/src/state.rs` — `pub fn from_json`。

### Requirement: Nono host and metadata network filter
HostFilter SHALL provide case-insensitive exact/wildcard allowlists, deny cloud metadata names and link-local resolved IPs, and expose explicit allow/deny reasons; allow_all is deliberate unrestricted mode.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/net_filter.rs` — `pub fn check_host`；`third_party/nono/src/net_filter.rs` — `pub fn allow_all`；`third_party/nono/src/net_filter.rs` — `fn is_link_local`。

### Requirement: Nono platform support and irreversible apply
Sandbox SHALL report target support, detect Linux ABI where applicable, apply capabilities through Linux Landlock or macOS Seatbelt, and return UnsupportedPlatform on other targets.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/sandbox/mod.rs` — `pub fn apply`；`third_party/nono/src/sandbox/mod.rs` — `pub fn support_info`；`third_party/nono/src/sandbox/mod.rs` — `pub fn is_supported`。

### Requirement: Nono Linux Landlock and ABI backend
The Linux backend SHALL cache ABI detection, expose feature/version names, map access modes to ABI rights, calculate scopes, apply Landlock filesystem/network restrictions, and select seccomp fallbacks when needed.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/sandbox/linux.rs` — `pub fn detect_abi`；`third_party/nono/src/sandbox/linux.rs` — `fn access_to_landlock`；`third_party/nono/src/sandbox/linux.rs` — `fn requested_scopes`；`third_party/nono/src/sandbox/linux.rs` — `pub fn apply_with_abi`；`third_party/nono/src/sandbox/linux.rs` — `pub fn seccomp_network_fallback_mode`。

### Requirement: Nono Linux seccomp notification enforcement
Linux seccomp helpers SHALL install notification/block/proxy filters, receive bounded notifications, resolve target paths/open_how/sockaddr, validate IDs, inject granted FDs, and continue or deny operations with typed errors.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/sandbox/linux.rs` — `pub fn install_seccomp_notify`；`third_party/nono/src/sandbox/linux.rs` — `pub fn recv_notif`；`third_party/nono/src/sandbox/linux.rs` — `pub fn resolve_notif_path`；`third_party/nono/src/sandbox/linux.rs` — `pub fn inject_fd`；`third_party/nono/src/sandbox/linux.rs` — `pub fn install_seccomp_block_network`；`third_party/nono/src/sandbox/linux.rs` — `pub fn install_seccomp_proxy_filter`。

### Requirement: Nono macOS Seatbelt and extension backend
The macOS backend SHALL issue/consume/release file extensions, safely escape paths/regex, generate ordered Seatbelt rules for filesystem/socket/network/process/IPC capabilities, and apply via sandbox_init.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/sandbox/macos.rs` — `pub fn extension_issue_file`；`third_party/nono/src/sandbox/macos.rs` — `pub fn extension_consume`；`third_party/nono/src/sandbox/macos.rs` — `pub fn extension_release`；`third_party/nono/src/sandbox/macos.rs` — `fn generate_profile`；`third_party/nono/src/sandbox/macos.rs` — `pub fn apply`。

### Requirement: Nono secret URI validation and redaction
Keystore SHALL recognize and validate op/apple-password/keyring/env/file references, reject forbidden characters/traversal/dangerous destination variables, and redact secret-bearing URI components for diagnostics.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/keystore.rs` — `pub fn validate_op_uri`；`third_party/nono/src/keystore.rs` — `pub fn validate_apple_password_uri`；`third_party/nono/src/keystore.rs` — `pub fn validate_keyring_uri`；`third_party/nono/src/keystore.rs` — `pub fn validate_env_uri`；`third_party/nono/src/keystore.rs` — `pub fn validate_file_uri`；`third_party/nono/src/keystore.rs` — `pub fn redact_op_uri`；`third_party/nono/src/keystore.rs` — `pub fn redact_keyring_uri`。

### Requirement: Nono secret loading and mapping precedence
Secret loading SHALL dispatch env/file/keyring/manager backends with bounded waits, trim and reject empty file secrets, store Unix secret files owner-only, and make explicit mapping pairs override list-derived mappings.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/keystore.rs` — `pub fn load_secrets`；`third_party/nono/src/keystore.rs` — `pub fn load_secret_by_ref`；`third_party/nono/src/keystore.rs` — `pub fn load_secret_file`；`third_party/nono/src/keystore.rs` — `pub fn store_secret_file`；`third_party/nono/src/keystore.rs` — `pub fn build_secret_mappings`；`third_party/nono/src/keystore.rs` — `fn wait_with_timeout`。

### Requirement: Nono sensitive diagnostics scrubbing
ScrubPolicy SHALL default to sensitive flags/headers/query keys, normalize add/remove operations and diff reporting, and redact URL userinfo/query secrets plus sensitive argv/header forms while allowing explicit policy changes.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/scrub.rs` — `pub fn scrub_value_with_policy`；`third_party/nono/src/scrub.rs` — `pub fn scrub_argv_with_policy`；`third_party/nono/src/scrub.rs` — `pub fn scrub_header_with_policy`；`third_party/nono/src/scrub.rs` — `pub fn secure_default`；`third_party/nono/src/scrub.rs` — `pub fn diff_from_secure_default`。

### Requirement: Nono denial analysis and formatter guidance
Diagnostic analysis SHALL classify sandbox versus ordinary failures, infer access/path from stderr and structured syscall lines, sanitize diagnostic text, and DiagnosticFormatter SHALL emit summaries, policy explanations, consolidated denials and actionable grant guidance.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/diagnostic.rs` — `pub fn analyze_error_output`；`third_party/nono/src/diagnostic.rs` — `fn infer_access_from_error_line`；`third_party/nono/src/diagnostic.rs` — `fn sanitize_for_diagnostic`；`third_party/nono/src/diagnostic.rs` — `impl DiagnosticFormatter`；`third_party/nono/src/diagnostic.rs` — `pub fn seatbelt_operation_to_access`。

### Requirement: Nono generated manifest semantic validation
CapabilityManifest SHALL parse/pretty-serialize generated schema types and validate supervised rollback, URI credential env_var requirements, and url_path/query_param injection fields.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/manifest.rs` — `pub fn from_json`；`third_party/nono/src/manifest.rs` — `pub fn to_json`；`third_party/nono/src/manifest.rs` — `pub fn validate`。

### Requirement: Nono manifest to capability conversion
TryFrom<&CapabilityManifest> for CapabilitySet SHALL validate and convert filesystem grants, network mode/ports, process isolation modes, and command allow/block lists; deny rules remain at higher profile/CLI layers.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/manifest_convert.rs` — `impl TryFrom<&CapabilityManifest> for CapabilitySet`；`third_party/nono/src/manifest_convert.rs` — `fn convert_access_mode`；`third_party/nono/src/manifest_convert.rs` — `fn convert_signal_mode`。

### Requirement: Nono supervisor typed messages
Supervisor types SHALL represent capability requests, URL-open requests, audit entries, approval decisions, and child/supervisor messages with serializable fields and granted/denied predicates.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/supervisor/types.rs` — `pub struct CapabilityRequest`；`third_party/nono/src/supervisor/types.rs` — `pub enum ApprovalDecision`；`third_party/nono/src/supervisor/types.rs` — `pub enum SupervisorMessage`。

### Requirement: Nono supervisor approval backend
ApprovalBackend SHALL be a Send+Sync trait receiving CapabilityRequest and returning ApprovalDecision with a stable backend name.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/supervisor/mod.rs` — `pub trait ApprovalBackend`。

### Requirement: Nono supervisor Unix socket transport
SupervisorSocket SHALL bind/connect or create local pairs, exchange length-prefixed JSON frames capped at 64 KiB, pass descriptors with SCM_RIGHTS, expose peer credentials/namespace checks and read timeouts, and clean owned socket paths on Drop.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/supervisor/socket.rs` — `pub fn pair`；`third_party/nono/src/supervisor/socket.rs` — `pub fn send_message`；`third_party/nono/src/supervisor/socket.rs` — `fn write_frame`；`third_party/nono/src/supervisor/socket.rs` — `pub fn send_fd_via_socket`；`third_party/nono/src/supervisor/socket.rs` — `pub fn peer_credentials`；`third_party/nono/src/supervisor/socket.rs` — `impl Drop for SupervisorSocket`。

### Requirement: Nono trust base64 interoperability
Trust base64 SHALL encode URL-safe unpadded and standard padded forms, decode either alphabet through either decoder, ignore whitespace/trailing padding, and reject invalid characters.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/trust/base64.rs` — `pub fn base64url_encode`；`third_party/nono/src/trust/base64.rs` — `pub fn base64url_decode`；`third_party/nono/src/trust/base64.rs` — `pub fn base64_encode`；`third_party/nono/src/trust/base64.rs` — `pub fn base64_decode`。

### Requirement: Nono trust SHA-256 digest
Digest helpers SHALL hash files in 8 KiB chunks and byte slices to lowercase SHA-256 hex, propagating file IO errors.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/trust/digest.rs` — `pub fn file_digest`；`third_party/nono/src/trust/digest.rs` — `pub fn bytes_digest`。

### Requirement: Nono DSSE and in-toto processing
DSSE SHALL parse/validate envelopes and signatures, decode payloads, produce PAE bytes, extract in-toto subjects and keyed/keyless signers, and construct instruction/policy/multi-subject statements with fixed predicate constants.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/trust/dsse.rs` — `pub fn pae`；`third_party/nono/src/trust/dsse.rs` — `pub fn new_instruction_statement`；`third_party/nono/src/trust/dsse.rs` — `pub fn new_policy_statement`；`third_party/nono/src/trust/dsse.rs` — `pub fn new_multi_subject_statement`；`third_party/nono/src/trust/dsse.rs` — `pub fn new_envelope`；`third_party/nono/src/trust/dsse.rs` — `pub fn extract_signer`。

### Requirement: Nono trust policy types and precedence
TrustPolicy SHALL enforce version and size limits, compile include globs, check digest blocklists, match keyed/keyless/GitLab publishers, and represent Deny/Warn/Audit where strictest merging preserves stronger enforcement; blocked outcomes always block.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/trust/types.rs` — `impl TrustPolicy`；`third_party/nono/src/trust/types.rs` — `pub fn matching_publishers`；`third_party/nono/src/trust/types.rs` — `pub fn strictest`；`third_party/nono/src/trust/types.rs` — `pub fn should_block`。

### Requirement: Nono trust policy loading and file evaluation
Policy helpers SHALL load JSON from strings/files, merge policies, evaluate digest/signer/enforcement, and recursively discover included files with bounded depth and skip-directory rules.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/trust/policy.rs` — `pub fn load_policy_from_str`；`third_party/nono/src/trust/policy.rs` — `pub fn merge_policies`；`third_party/nono/src/trust/policy.rs` — `pub fn evaluate_file`；`third_party/nono/src/trust/policy.rs` — `pub fn find_included_files_with_skip_dirs`。

### Requirement: Nono trust module boundary
The trust module SHALL export base64, digest, DSSE, policy and typed verification primitives as one public namespace.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/trust/mod.rs` — `pub mod policy`；`third_party/nono/src/trust/mod.rs` — `pub use dsse`。

### Requirement: Nono undo typed snapshot model
Undo types SHALL provide fixed 32-byte ContentHash parsing/serde/display with prefix/suffix helpers, file states, typed changes, network audit summaries, executable identity, session metadata and snapshot manifests.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/undo/types.rs` — `impl ContentHash`；`third_party/nono/src/undo/types.rs` — `pub struct SnapshotManifest`；`third_party/nono/src/undo/types.rs` — `pub enum ChangeType`；`third_party/nono/src/undo/types.rs` — `pub struct SessionMetadata`。

### Requirement: Nono undo exclusion filtering
ExclusionFilter SHALL combine component/path exclusions and gitignore-style globs, while force-includes override matching exclusions.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/undo/exclusion.rs` — `pub fn new`；`third_party/nono/src/undo/exclusion.rs` — `pub fn is_excluded`；`third_party/nono/src/undo/exclusion.rs` — `fn matches_force_include`。

### Requirement: Nono deterministic Merkle roots
MerkleTree SHALL hash path/content leaves with domain prefixes, sort deterministically, combine odd leaves deterministically, preserve non-UTF8 path distinctions on Unix, and expose root/leaf count.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/undo/merkle.rs` — `pub fn from_manifest`；`third_party/nono/src/undo/merkle.rs` — `pub fn root`；`third_party/nono/src/undo/merkle.rs` — `fn compute_leaf_hash`；`third_party/nono/src/undo/merkle.rs` — `fn compute_internal_hash`。

### Requirement: Nono content-addressed object store
ObjectStore SHALL stream/store file and byte contents by SHA-256, deduplicate existing objects, retrieve and verify content, atomically materialize targets, and use platform clone/copy fallback.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/undo/object_store.rs` — `pub fn store_file`；`third_party/nono/src/undo/object_store.rs` — `pub fn store_bytes`；`third_party/nono/src/undo/object_store.rs` — `pub fn retrieve_to`；`third_party/nono/src/undo/object_store.rs` — `pub fn verify`；`third_party/nono/src/undo/object_store.rs` — `fn clone_or_copy`。

### Requirement: Nono snapshot lifecycle and restore
SnapshotManager SHALL walk roots under entry/byte budgets, apply exclusions, hash/store files, create baseline/incremental manifests, compute create/modify/delete changes, validate tracked paths, restore safely, persist metadata and clean new atomic temp files.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/undo/snapshot.rs` — `pub fn create_baseline`；`third_party/nono/src/undo/snapshot.rs` — `pub fn create_incremental`；`third_party/nono/src/undo/snapshot.rs` — `pub fn compute_restore_diff`；`third_party/nono/src/undo/snapshot.rs` — `pub fn restore_to`；`third_party/nono/src/undo/snapshot.rs` — `fn check_budget`；`third_party/nono/src/undo/snapshot.rs` — `fn validate_manifest_paths`。

### Requirement: Nono undo module boundary
The undo module SHALL export exclusion, Merkle, object-store, snapshot and typed manifest primitives.

#### Scenario: Normal use
- **WHEN** the caller supplies valid inputs
- **THEN** the documented capability boundary is applied.

#### Scenario: Boundary case
- **WHEN** an input or platform violates a precondition
- **THEN** the API returns a typed error or conservative result.

证据：`third_party/nono/src/undo/mod.rs` — `pub mod snapshot`；`third_party/nono/src/undo/mod.rs` — `pub mod types`。
### Requirement: Nix directory descriptor iteration
Dir SHALL open directories by path or descriptor, expose raw descriptors, rewind and iterate directory entries including dot entries, provide owning/non-owning iterator variants, and expose entry inode/name/type while closing owned descriptors on drop.

#### Scenario: Nix directory descriptor iteration implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/dir.rs` — `struct Dir`；`third_party/nix-ohos/src/dir.rs` — `fn open`；`third_party/nix-ohos/src/dir.rs` — `fn openat`；`third_party/nix-ohos/src/dir.rs` — `fn from`；`third_party/nix-ohos/src/dir.rs` — `fn from_fd`；`third_party/nix-ohos/src/dir.rs` — `fn iter`；`third_party/nix-ohos/src/dir.rs` — `fn as_raw_fd`；`third_party/nix-ohos/src/dir.rs` — `fn drop`；`third_party/nix-ohos/src/dir.rs` — `fn next`；`third_party/nix-ohos/src/dir.rs` — `struct Iter`；`third_party/nix-ohos/src/dir.rs` — `type Item`；`third_party/nix-ohos/src/dir.rs` — `fn next`；`third_party/nix-ohos/src/dir.rs` — `fn drop`；`third_party/nix-ohos/src/dir.rs` — `struct OwningIter`；`third_party/nix-ohos/src/dir.rs` — `type Item`；`third_party/nix-ohos/src/dir.rs` — `fn next`；`third_party/nix-ohos/src/dir.rs` — `fn as_raw_fd`；`third_party/nix-ohos/src/dir.rs` — `type Item`；`third_party/nix-ohos/src/dir.rs` — `type IntoIter`；`third_party/nix-ohos/src/dir.rs` — `fn into_iter`；`third_party/nix-ohos/src/dir.rs` — `struct Entry`；`third_party/nix-ohos/src/dir.rs` — `enum Type`；`third_party/nix-ohos/src/dir.rs` — `fn ino`；`third_party/nix-ohos/src/dir.rs` — `fn file_name`；`third_party/nix-ohos/src/dir.rs` — `fn file_type`。

### Requirement: Nix errno conversion and constants
Errno SHALL expose platform errno constants, read/clear the thread-local errno, describe values, convert raw returns and std::io errors, implement Result conversion/sentinel helpers, and preserve unknown numeric errno values through the typed enum representation.

#### Scenario: Nix errno conversion and constants implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/errno.rs` — `fn errno_location`；`third_party/nix-ohos/src/errno.rs` — `fn errno_location`；`third_party/nix-ohos/src/errno.rs` — `fn errno_location`；`third_party/nix-ohos/src/errno.rs` — `fn errno_location`；`third_party/nix-ohos/src/errno.rs` — `fn errno_location`；`third_party/nix-ohos/src/errno.rs` — `fn clear`；`third_party/nix-ohos/src/errno.rs` — `fn errno`；`third_party/nix-ohos/src/errno.rs` — `fn last`；`third_party/nix-ohos/src/errno.rs` — `fn desc`；`third_party/nix-ohos/src/errno.rs` — `fn from_i32`；`third_party/nix-ohos/src/errno.rs` — `fn clear`；`third_party/nix-ohos/src/errno.rs` — `fn result`；`third_party/nix-ohos/src/errno.rs` — `trait ErrnoSentinel`；`third_party/nix-ohos/src/errno.rs` — `fn sentinel`；`third_party/nix-ohos/src/errno.rs` — `fn sentinel`；`third_party/nix-ohos/src/errno.rs` — `fn sentinel`；`third_party/nix-ohos/src/errno.rs` — `fn sentinel`；`third_party/nix-ohos/src/errno.rs` — `fn sentinel`；`third_party/nix-ohos/src/errno.rs` — `fn sentinel`；`third_party/nix-ohos/src/errno.rs` — `fn fmt`；`third_party/nix-ohos/src/errno.rs` — `fn from`；`third_party/nix-ohos/src/errno.rs` — `type Error`；`third_party/nix-ohos/src/errno.rs` — `fn try_from`；`third_party/nix-ohos/src/errno.rs` — `fn last`；`third_party/nix-ohos/src/errno.rs` — `fn desc`；`third_party/nix-ohos/src/errno.rs` — `enum Errno`；`third_party/nix-ohos/src/errno.rs` — `const EWOULDBLOCK`；`third_party/nix-ohos/src/errno.rs` — `const EDEADLOCK`；`third_party/nix-ohos/src/errno.rs` — `const ENOTSUP`；`third_party/nix-ohos/src/errno.rs` — `fn from_i32`；`third_party/nix-ohos/src/errno.rs` — `enum Errno`；`third_party/nix-ohos/src/errno.rs` — `const ELAST`；`third_party/nix-ohos/src/errno.rs` — `const EWOULDBLOCK`；`third_party/nix-ohos/src/errno.rs` — `const EDEADLOCK`；`third_party/nix-ohos/src/errno.rs` — `fn from_i32`；`third_party/nix-ohos/src/errno.rs` — `enum Errno`；`third_party/nix-ohos/src/errno.rs` — `const ELAST`；`third_party/nix-ohos/src/errno.rs` — `const EWOULDBLOCK`；`third_party/nix-ohos/src/errno.rs` — `const EDEADLOCK`；`third_party/nix-ohos/src/errno.rs` — `const EOPNOTSUPP`；`third_party/nix-ohos/src/errno.rs` — `fn from_i32`；`third_party/nix-ohos/src/errno.rs` — `enum Errno`；`third_party/nix-ohos/src/errno.rs` — `const ELAST`；`third_party/nix-ohos/src/errno.rs` — `const EWOULDBLOCK`；`third_party/nix-ohos/src/errno.rs` — `const EDEADLOCK`；`third_party/nix-ohos/src/errno.rs` — `const EOPNOTSUPP`；`third_party/nix-ohos/src/errno.rs` — `fn from_i32`；`third_party/nix-ohos/src/errno.rs` — `enum Errno`；`third_party/nix-ohos/src/errno.rs` — `const ELAST`；`third_party/nix-ohos/src/errno.rs` — `const EWOULDBLOCK`；`third_party/nix-ohos/src/errno.rs` — `fn from_i32`；`third_party/nix-ohos/src/errno.rs` — `enum Errno`；`third_party/nix-ohos/src/errno.rs` — `const ELAST`；`third_party/nix-ohos/src/errno.rs` — `const EWOULDBLOCK`；`third_party/nix-ohos/src/errno.rs` — `fn from_i32`；`third_party/nix-ohos/src/errno.rs` — `enum Errno`；`third_party/nix-ohos/src/errno.rs` — `const EWOULDBLOCK`；`third_party/nix-ohos/src/errno.rs` — `fn from_i32`；`third_party/nix-ohos/src/errno.rs` — `enum Errno`；`third_party/nix-ohos/src/errno.rs` — `const ELAST`；`third_party/nix-ohos/src/errno.rs` — `const EWOULDBLOCK`；`third_party/nix-ohos/src/errno.rs` — `fn from_i32`；`third_party/nix-ohos/src/errno.rs` — `enum Errno`；`third_party/nix-ohos/src/errno.rs` — `const EWOULDBLOCK`；`third_party/nix-ohos/src/errno.rs` — `const EDEADLOCK`；`third_party/nix-ohos/src/errno.rs` — `const EOPNOTSUPP`；`third_party/nix-ohos/src/errno.rs` — `fn from_i32`。

### Requirement: Nix file control and descriptor operations
fcntl SHALL expose platform OFlag/AtFlags/SealFlag/FdFlag/FallocateFlags and related enums plus wrappers for open/openat/renameat/readlink/fcntl/flock/splice/tee/vmsplice/copy_file_range/fallocate/posix_fadvise and descriptor flags, mapping libc failures through Errno with target/feature cfgs.

#### Scenario: Nix file control and descriptor operations implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/fcntl.rs` — `struct AtFlags`；`third_party/nix-ohos/src/fcntl.rs` — `struct OFlag`；`third_party/nix-ohos/src/fcntl.rs` — `fn open`；`third_party/nix-ohos/src/fcntl.rs` — `fn openat`；`third_party/nix-ohos/src/fcntl.rs` — `fn renameat`；`third_party/nix-ohos/src/fcntl.rs` — `struct RenameFlags`；`third_party/nix-ohos/src/fcntl.rs` — `fn renameat2`；`third_party/nix-ohos/src/fcntl.rs` — `fn wrap_readlink_result`；`third_party/nix-ohos/src/fcntl.rs` — `fn readlink_maybe_at`；`third_party/nix-ohos/src/fcntl.rs` — `fn inner_readlink`；`third_party/nix-ohos/src/fcntl.rs` — `fn readlink`；`third_party/nix-ohos/src/fcntl.rs` — `fn readlinkat`；`third_party/nix-ohos/src/fcntl.rs` — `fn at_rawfd`；`third_party/nix-ohos/src/fcntl.rs` — `struct SealFlag`；`third_party/nix-ohos/src/fcntl.rs` — `struct FdFlag`；`third_party/nix-ohos/src/fcntl.rs` — `enum FcntlArg`；`third_party/nix-ohos/src/fcntl.rs` — `enum FcntlArg`；`third_party/nix-ohos/src/fcntl.rs` — `fn fcntl`；`third_party/nix-ohos/src/fcntl.rs` — `enum FlockArg`；`third_party/nix-ohos/src/fcntl.rs` — `fn flock`；`third_party/nix-ohos/src/fcntl.rs` — `struct SpliceFFlags`；`third_party/nix-ohos/src/fcntl.rs` — `fn copy_file_range`；`third_party/nix-ohos/src/fcntl.rs` — `fn splice`；`third_party/nix-ohos/src/fcntl.rs` — `fn tee`；`third_party/nix-ohos/src/fcntl.rs` — `fn vmsplice`；`third_party/nix-ohos/src/fcntl.rs` — `struct FallocateFlags`；`third_party/nix-ohos/src/fcntl.rs` — `fn fallocate`；`third_party/nix-ohos/src/fcntl.rs` — `struct SpacectlRange`；`third_party/nix-ohos/src/fcntl.rs` — `fn is_empty`；`third_party/nix-ohos/src/fcntl.rs` — `fn len`；`third_party/nix-ohos/src/fcntl.rs` — `fn offset`；`third_party/nix-ohos/src/fcntl.rs` — `fn fspacectl`；`third_party/nix-ohos/src/fcntl.rs` — `fn fspacectl_all`；`third_party/nix-ohos/src/fcntl.rs` — `enum PosixFadviseAdvice`；`third_party/nix-ohos/src/fcntl.rs` — `fn posix_fadvise`；`third_party/nix-ohos/src/fcntl.rs` — `fn posix_fallocate`。

### Requirement: Nix runtime feature probes
The feature module SHALL parse Linux kernel version strings, cache the runtime kernel version, and expose socket_atomic_cloexec based on platform/kernel capability constants; parsing tests cover valid and invalid version shapes.

#### Scenario: test:test_parsing_kernel_version
- **WHEN** the embedded test function test_parsing_kernel_version is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/features.rs` — `static VERS_UNKNOWN`；`third_party/nix-ohos/src/features.rs` — `static VERS_2_6_18`；`third_party/nix-ohos/src/features.rs` — `static VERS_2_6_27`；`third_party/nix-ohos/src/features.rs` — `static VERS_2_6_28`；`third_party/nix-ohos/src/features.rs` — `static VERS_3`；`third_party/nix-ohos/src/features.rs` — `fn digit`；`third_party/nix-ohos/src/features.rs` — `fn parse_kernel_version`；`third_party/nix-ohos/src/features.rs` — `fn kernel_version`；`third_party/nix-ohos/src/features.rs` — `static mut`；`third_party/nix-ohos/src/features.rs` — `fn socket_atomic_cloexec`；`third_party/nix-ohos/src/features.rs` — `fn socket_atomic_cloexec`；`third_party/nix-ohos/src/features.rs` — `fn socket_atomic_cloexec`；`third_party/nix-ohos/src/features.rs` — `fn test_parsing_kernel_version`。

### Requirement: Nix interface address enumeration
getifaddrs SHALL obtain and free the libc interface list, convert names, flags, addresses, netmasks, broadcast/point-to-point destinations into InterfaceAddress, work around the XNU netmask issue, and yield an owned iterator that releases native resources.

#### Scenario: test:test_getifaddrs
- **WHEN** the embedded test function test_getifaddrs is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:test_getifaddrs_netmask_correct
- **WHEN** the embedded test function test_getifaddrs_netmask_correct is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/ifaddrs.rs` — `struct InterfaceAddress`；`third_party/nix-ohos/src/ifaddrs.rs` — `fn get_ifu_from_sockaddr`；`third_party/nix-ohos/src/ifaddrs.rs` — `fn get_ifu_from_sockaddr`；`third_party/nix-ohos/src/ifaddrs.rs` — `fn workaround_xnu_bug`；`third_party/nix-ohos/src/ifaddrs.rs` — `fn from_libc_ifaddrs`；`third_party/nix-ohos/src/ifaddrs.rs` — `struct InterfaceAddressIterator`；`third_party/nix-ohos/src/ifaddrs.rs` — `fn drop`；`third_party/nix-ohos/src/ifaddrs.rs` — `type Item`；`third_party/nix-ohos/src/ifaddrs.rs` — `fn next`；`third_party/nix-ohos/src/ifaddrs.rs` — `fn getifaddrs`；`third_party/nix-ohos/src/ifaddrs.rs` — `fn test_getifaddrs`；`third_party/nix-ohos/src/ifaddrs.rs` — `fn test_getifaddrs_netmask_correct`。

### Requirement: Nix kernel module operations
Kernel module wrappers SHALL expose init_module, finit_module, and delete_module with typed ModuleInitFlags/DeleteModuleFlags, pass image/parameters or file descriptors to libc, and return Errno results while relying on OS privilege and platform availability.

#### Scenario: Nix kernel module operations implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/kmod.rs` — `fn init_module`；`third_party/nix-ohos/src/kmod.rs` — `struct ModuleInitFlags`；`third_party/nix-ohos/src/kmod.rs` — `fn finit_module`；`third_party/nix-ohos/src/kmod.rs` — `struct DeleteModuleFlags`；`third_party/nix-ohos/src/kmod.rs` — `fn delete_module`。

### Requirement: Nix public module and path boundary
The crate root SHALL restrict the crate to Unix targets, expose feature-gated *nix API modules and the libc re-export, provide Result/Error aliases over Errno, and implement NixPath conversion for str, OsStr, CStr, byte slices, Path, and PathBuf with stack allocation below 1024 bytes and heap allocation/error handling for longer or interior-NUL paths.

#### Scenario: Nix public module and path boundary implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/lib.rs` — `type Result`；`third_party/nix-ohos/src/lib.rs` — `type Error`；`third_party/nix-ohos/src/lib.rs` — `trait NixPath`；`third_party/nix-ohos/src/lib.rs` — `fn is_empty`；`third_party/nix-ohos/src/lib.rs` — `fn len`；`third_party/nix-ohos/src/lib.rs` — `fn with_nix_path`；`third_party/nix-ohos/src/lib.rs` — `fn is_empty`；`third_party/nix-ohos/src/lib.rs` — `fn len`；`third_party/nix-ohos/src/lib.rs` — `fn with_nix_path`；`third_party/nix-ohos/src/lib.rs` — `fn is_empty`；`third_party/nix-ohos/src/lib.rs` — `fn len`；`third_party/nix-ohos/src/lib.rs` — `fn with_nix_path`；`third_party/nix-ohos/src/lib.rs` — `fn is_empty`；`third_party/nix-ohos/src/lib.rs` — `fn len`；`third_party/nix-ohos/src/lib.rs` — `fn with_nix_path`；`third_party/nix-ohos/src/lib.rs` — `fn is_empty`；`third_party/nix-ohos/src/lib.rs` — `fn len`；`third_party/nix-ohos/src/lib.rs` — `fn with_nix_path`；`third_party/nix-ohos/src/lib.rs` — `const MAX_STACK_ALLOCATION`；`third_party/nix-ohos/src/lib.rs` — `fn with_nix_path_allocating`；`third_party/nix-ohos/src/lib.rs` — `fn is_empty`；`third_party/nix-ohos/src/lib.rs` — `fn len`；`third_party/nix-ohos/src/lib.rs` — `fn with_nix_path`；`third_party/nix-ohos/src/lib.rs` — `fn is_empty`；`third_party/nix-ohos/src/lib.rs` — `fn len`；`third_party/nix-ohos/src/lib.rs` — `fn with_nix_path`。

### Requirement: Nix syscall macro generation boundary
The internal/public macros SHALL generate libc-backed bitflag/enum wrappers, feature-gated module exports, and ioctl request-code/read/write/none/buffer wrappers whose generated functions call libc and convert sentinel failures through Errno.

#### Scenario: Nix syscall macro generation boundary implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/macros.rs` — `macro feature`；`third_party/nix-ohos/src/macros.rs` — `macro libc_bitflags`；`third_party/nix-ohos/src/macros.rs` — `macro libc_enum`；`third_party/nix-ohos/src/macros.rs` — `type Error`；`third_party/nix-ohos/src/macros.rs` — `fn try_from`。

### Requirement: Nix BSD mount operations
The BSD mount backend SHALL model platform MntFlags and NmountError/Nmount option assembly, encode path/string/byte options with owned pointer lifetimes, call nmount, and expose unmount with typed error conversion under BSD-family cfgs.

#### Scenario: Nix BSD mount operations implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/mount/bsd.rs` — `struct MntFlags`；`third_party/nix-ohos/src/mount/bsd.rs` — `struct NmountError`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn errmsg`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn error`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn new`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn fmt`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn from`；`third_party/nix-ohos/src/mount/bsd.rs` — `type NmountResult`；`third_party/nix-ohos/src/mount/bsd.rs` — `struct Nmount`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn push_slice`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn push_pointer_and_length`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn push_nix_path`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn mut_ptr_opt`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn null_opt`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn null_opt_owned`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn str_opt`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn str_opt_owned`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn new`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn nmount`；`third_party/nix-ohos/src/mount/bsd.rs` — `const ERRMSG_NAME`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn drop`；`third_party/nix-ohos/src/mount/bsd.rs` — `fn unmount`。

### Requirement: Nix Linux mount operations
The Linux mount backend SHALL expose MsFlags and MntFlags plus mount, umount, and umount2 wrappers, encode optional filesystem/source/data paths through NixPath, and convert libc failures to Errno under Linux/Android cfgs.

#### Scenario: Nix Linux mount operations implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/mount/linux.rs` — `struct MsFlags`；`third_party/nix-ohos/src/mount/linux.rs` — `struct MntFlags`；`third_party/nix-ohos/src/mount/linux.rs` — `fn mount`；`third_party/nix-ohos/src/mount/linux.rs` — `fn with_opt_nix_path`；`third_party/nix-ohos/src/mount/linux.rs` — `fn umount`；`third_party/nix-ohos/src/mount/linux.rs` — `fn umount2`。

### Requirement: Nix platform mount module selection
The mount module SHALL re-export the Linux or BSD mount backend only for its declared target families and leave platform selection to cfg gates.

#### Scenario: Nix platform mount module selection implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/mount/mod.rs` — `mod linux`；`third_party/nix-ohos/src/mount/mod.rs` — `pub use self::linux::*`；`third_party/nix-ohos/src/mount/mod.rs` — `mod bsd`；`third_party/nix-ohos/src/mount/mod.rs` — `pub use self::bsd::*`。

### Requirement: Nix POSIX message queue operations
The mqueue module SHALL model MQ_OFlag/MqAttr/MqdT and expose mq_open/unlink/close/receive/send/getattr/setattr plus nonblocking flag helpers, converting names and buffers to libc calls and returning typed Errno results.

#### Scenario: Nix POSIX message queue operations implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/mqueue.rs` — `struct MQ_OFlag`；`third_party/nix-ohos/src/mqueue.rs` — `struct MqAttr`；`third_party/nix-ohos/src/mqueue.rs` — `struct MqdT`；`third_party/nix-ohos/src/mqueue.rs` — `type mq_attr_member_t`；`third_party/nix-ohos/src/mqueue.rs` — `type mq_attr_member_t`；`third_party/nix-ohos/src/mqueue.rs` — `fn new`；`third_party/nix-ohos/src/mqueue.rs` — `fn flags`；`third_party/nix-ohos/src/mqueue.rs` — `fn maxmsg`；`third_party/nix-ohos/src/mqueue.rs` — `fn msgsize`；`third_party/nix-ohos/src/mqueue.rs` — `fn curmsgs`；`third_party/nix-ohos/src/mqueue.rs` — `fn mq_open`；`third_party/nix-ohos/src/mqueue.rs` — `fn mq_unlink`；`third_party/nix-ohos/src/mqueue.rs` — `fn mq_close`；`third_party/nix-ohos/src/mqueue.rs` — `fn mq_receive`；`third_party/nix-ohos/src/mqueue.rs` — `fn mq_send`；`third_party/nix-ohos/src/mqueue.rs` — `fn mq_getattr`；`third_party/nix-ohos/src/mqueue.rs` — `fn mq_setattr`；`third_party/nix-ohos/src/mqueue.rs` — `fn mq_set_nonblock`；`third_party/nix-ohos/src/mqueue.rs` — `fn mq_remove_nonblock`。

### Requirement: Nix network interface index and flags
The network interface module SHALL resolve interface names to indexes, model platform interface flags, and provide borrowed/owned iterable Interfaces with name/index access, deterministic cleanup, and display formatting around libc if_nameindex/if_nametoindex.

#### Scenario: Nix network interface index and flags implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/net/if_.rs` — `fn if_nametoindex`；`third_party/nix-ohos/src/net/if_.rs` — `struct InterfaceFlags`；`third_party/nix-ohos/src/net/if_.rs` — `struct Interface`；`third_party/nix-ohos/src/net/if_.rs` — `fn index`；`third_party/nix-ohos/src/net/if_.rs` — `fn name`；`third_party/nix-ohos/src/net/if_.rs` — `fn fmt`；`third_party/nix-ohos/src/net/if_.rs` — `struct Interfaces`；`third_party/nix-ohos/src/net/if_.rs` — `fn iter`；`third_party/nix-ohos/src/net/if_.rs` — `fn to_slice`；`third_party/nix-ohos/src/net/if_.rs` — `fn drop`；`third_party/nix-ohos/src/net/if_.rs` — `fn fmt`；`third_party/nix-ohos/src/net/if_.rs` — `type IntoIter`；`third_party/nix-ohos/src/net/if_.rs` — `type Item`；`third_party/nix-ohos/src/net/if_.rs` — `fn into_iter`；`third_party/nix-ohos/src/net/if_.rs` — `struct InterfacesIter`；`third_party/nix-ohos/src/net/if_.rs` — `type Item`；`third_party/nix-ohos/src/net/if_.rs` — `fn next`；`third_party/nix-ohos/src/net/if_.rs` — `fn if_nameindex`。

### Requirement: Nix network module boundary
The net module SHALL expose the if_ interface submodule as the public network-interface namespace.

#### Scenario: Nix network module boundary implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/net/mod.rs` — `pub mod if_`。

### Requirement: Nix poll and ppoll readiness
PollFd SHALL model a raw descriptor and requested/reported PollFlags, provide event mutation/accessors, and poll/ppoll SHALL wait on descriptor arrays with timeout handling and Errno conversion.

#### Scenario: Nix poll and ppoll readiness implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/poll.rs` — `struct PollFd`；`third_party/nix-ohos/src/poll.rs` — `fn new`；`third_party/nix-ohos/src/poll.rs` — `fn revents`；`third_party/nix-ohos/src/poll.rs` — `fn any`；`third_party/nix-ohos/src/poll.rs` — `fn all`；`third_party/nix-ohos/src/poll.rs` — `fn events`；`third_party/nix-ohos/src/poll.rs` — `fn set_events`；`third_party/nix-ohos/src/poll.rs` — `fn as_raw_fd`；`third_party/nix-ohos/src/poll.rs` — `struct PollFlags`；`third_party/nix-ohos/src/poll.rs` — `fn poll`；`third_party/nix-ohos/src/poll.rs` — `fn ppoll`。

### Requirement: Nix Linux epoll interface
Epoll SHALL model create flags, operations, and events, and wrap epoll_create/epoll_create1/epoll_ctl/epoll_wait with typed descriptor/event conversion and Errno results.

#### Scenario: Nix Linux epoll interface implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/epoll.rs` — `struct EpollFlags`；`third_party/nix-ohos/src/sys/epoll.rs` — `enum EpollOp`；`third_party/nix-ohos/src/sys/epoll.rs` — `struct EpollCreateFlags`；`third_party/nix-ohos/src/sys/epoll.rs` — `struct EpollEvent`；`third_party/nix-ohos/src/sys/epoll.rs` — `fn new`；`third_party/nix-ohos/src/sys/epoll.rs` — `fn empty`；`third_party/nix-ohos/src/sys/epoll.rs` — `fn events`；`third_party/nix-ohos/src/sys/epoll.rs` — `fn data`；`third_party/nix-ohos/src/sys/epoll.rs` — `fn epoll_create`；`third_party/nix-ohos/src/sys/epoll.rs` — `fn epoll_create1`；`third_party/nix-ohos/src/sys/epoll.rs` — `fn epoll_ctl`；`third_party/nix-ohos/src/sys/epoll.rs` — `fn epoll_wait`。

### Requirement: Nix BSD kqueue interface
The event module SHALL model kqueue filters/flags/events and kevent records, provide kqueue/kevent/kevent_ts/ev_set wrappers, and preserve platform-specific udata/time representations.

#### Scenario: test:test_struct_kevent
- **WHEN** the embedded test function test_struct_kevent is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:test_kevent_filter
- **WHEN** the embedded test function test_kevent_filter is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/event.rs` — `struct KEvent`；`third_party/nix-ohos/src/sys/event.rs` — `type type_of_udata`；`third_party/nix-ohos/src/sys/event.rs` — `type type_of_udata`；`third_party/nix-ohos/src/sys/event.rs` — `type type_of_event_filter`；`third_party/nix-ohos/src/sys/event.rs` — `type type_of_event_filter`；`third_party/nix-ohos/src/sys/event.rs` — `enum EventFilter`；`third_party/nix-ohos/src/sys/event.rs` — `type type_of_event_flag`；`third_party/nix-ohos/src/sys/event.rs` — `type type_of_event_flag`；`third_party/nix-ohos/src/sys/event.rs` — `struct EventFlag`；`third_party/nix-ohos/src/sys/event.rs` — `struct FilterFlag`；`third_party/nix-ohos/src/sys/event.rs` — `fn kqueue`；`third_party/nix-ohos/src/sys/event.rs` — `fn new`；`third_party/nix-ohos/src/sys/event.rs` — `fn ident`；`third_party/nix-ohos/src/sys/event.rs` — `fn filter`；`third_party/nix-ohos/src/sys/event.rs` — `fn flags`；`third_party/nix-ohos/src/sys/event.rs` — `fn fflags`；`third_party/nix-ohos/src/sys/event.rs` — `fn data`；`third_party/nix-ohos/src/sys/event.rs` — `fn udata`；`third_party/nix-ohos/src/sys/event.rs` — `fn kevent`；`third_party/nix-ohos/src/sys/event.rs` — `type type_of_nchanges`；`third_party/nix-ohos/src/sys/event.rs` — `type type_of_nchanges`；`third_party/nix-ohos/src/sys/event.rs` — `fn kevent_ts`；`third_party/nix-ohos/src/sys/event.rs` — `fn ev_set`；`third_party/nix-ohos/src/sys/event.rs` — `fn test_struct_kevent`；`third_party/nix-ohos/src/sys/event.rs` — `fn test_kevent_filter`。

### Requirement: Nix Linux eventfd interface
eventfd SHALL expose EfdFlags and wrap libc eventfd with typed descriptor creation and Errno conversion.

#### Scenario: Nix Linux eventfd interface implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/eventfd.rs` — `struct EfdFlags`；`third_party/nix-ohos/src/sys/eventfd.rs` — `fn eventfd`。

### Requirement: Nix BSD ioctl encoding constants
The BSD ioctl backend SHALL define platform ioctl number/parameter types and VOID/OUT/IN/INOUT masks/constants used by ioctl request macros under BSD-family cfgs.

#### Scenario: Nix BSD ioctl encoding constants implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `type ioctl_num_type`；`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `type ioctl_num_type`；`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `type ioctl_param_type`；`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `const VOID`；`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `const OUT`；`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `const IN`；`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `const INOUT`；`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `const IOCPARM_MASK`；`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `macro ioc`；`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `macro request_code_none`；`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `macro request_code_write_int`；`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `macro request_code_read`；`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `macro request_code_write`；`third_party/nix-ohos/src/sys/ioctl/bsd.rs` — `macro request_code_readwrite`。

### Requirement: Nix Linux and OHOS ioctl encoding constants
The Linux ioctl backend SHALL define request number/type/size/direction bit widths, shifts, masks and KVM constants using the musl-or-OHOS integer layout where configured, preserving the alternate glibc layout.

#### Scenario: Nix Linux and OHOS ioctl encoding constants implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `type ioctl_num_type`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `type ioctl_num_type`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `type ioctl_param_type`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const NRBITS`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const TYPEBITS`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const NONE`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const READ`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const WRITE`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const SIZEBITS`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const DIRBITS`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const NONE`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const READ`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const WRITE`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const SIZEBITS`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const DIRBITS`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const NRSHIFT`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const TYPESHIFT`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const SIZESHIFT`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const DIRSHIFT`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const NRMASK`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const TYPEMASK`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const SIZEMASK`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `const DIRMASK`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `macro ioc`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `macro request_code_none`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `macro request_code_read`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `macro request_code_write`；`third_party/nix-ohos/src/sys/ioctl/linux.rs` — `macro request_code_readwrite`。

### Requirement: Nix ioctl macro and command boundary
The ioctl module SHALL provide request-code helpers and ioctl_read/write/none/readwrite/buffer macros that generate unsafe libc ioctl calls with typed pointer/int payloads and Errno sentinel conversion, plus documented example command definitions.

#### Scenario: Nix ioctl macro and command boundary implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro convert_ioctl_res`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_none`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_none_bad`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_read`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_read_bad`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_write_ptr`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_write_ptr_bad`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_write_int`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_write_int`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_write_int_bad`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_readwrite`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_readwrite_bad`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_read_buf`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_write_buf`；`third_party/nix-ohos/src/sys/ioctl/mod.rs` — `macro ioctl_readwrite_buf`。

### Requirement: Nix memory-backed file descriptors
memfd_create SHALL model sealing/close-on-exec flags and create anonymous memory-backed file descriptors through libc, with cfg-dependent errno and flag handling.

#### Scenario: Nix memory-backed file descriptors implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/memfd.rs` — `struct MemFdCreateFlag`；`third_party/nix-ohos/src/sys/memfd.rs` — `fn memfd_create`。

### Requirement: Nix memory mapping operations
mman SHALL model protection/map/remap/lock/advice/sync flags and wrap mmap/mremap/munmap/mprotect/mlock/munlock/mlockall/madvise/msync/shm_open/shm_unlink, preserving pointer/error contracts and target cfgs.

#### Scenario: Nix memory mapping operations implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/mman.rs` — `struct ProtFlags`；`third_party/nix-ohos/src/sys/mman.rs` — `struct MapFlags`；`third_party/nix-ohos/src/sys/mman.rs` — `struct MRemapFlags`；`third_party/nix-ohos/src/sys/mman.rs` — `enum MmapAdvise`；`third_party/nix-ohos/src/sys/mman.rs` — `struct MsFlags`；`third_party/nix-ohos/src/sys/mman.rs` — `struct MlockAllFlags`；`third_party/nix-ohos/src/sys/mman.rs` — `fn mlock`；`third_party/nix-ohos/src/sys/mman.rs` — `fn munlock`；`third_party/nix-ohos/src/sys/mman.rs` — `fn mlockall`；`third_party/nix-ohos/src/sys/mman.rs` — `fn munlockall`；`third_party/nix-ohos/src/sys/mman.rs` — `fn mmap`；`third_party/nix-ohos/src/sys/mman.rs` — `fn mremap`；`third_party/nix-ohos/src/sys/mman.rs` — `fn munmap`；`third_party/nix-ohos/src/sys/mman.rs` — `fn madvise`；`third_party/nix-ohos/src/sys/mman.rs` — `fn mprotect`；`third_party/nix-ohos/src/sys/mman.rs` — `fn msync`；`third_party/nix-ohos/src/sys/mman.rs` — `fn shm_open`；`third_party/nix-ohos/src/sys/mman.rs` — `fn shm_unlink`。

### Requirement: Nix platform sys module graph
The sys module SHALL select and expose AIO, event, ioctl, memfd, mman, personality, pthread, ptrace, quota, reboot, resource, select, sendfile, signal, signalfd, socket, stat, statfs, statvfs, sysinfo, termios, time, timer, timerfd, uio, utsname, and wait APIs through feature and target cfgs.

#### Scenario: Nix platform sys module graph implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/mod.rs` — `pub mod aio`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod epoll`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod event`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod eventfd`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod ioctl`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod memfd`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod mman`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod personality`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod pthread`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod ptrace`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod quota`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod reboot`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod resource`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod select`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod sendfile`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod signal`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod signalfd`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod socket`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod stat`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod statfs`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod statvfs`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod sysinfo`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod termios`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod time`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod uio`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod utsname`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod wait`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod inotify`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod timerfd`；`third_party/nix-ohos/src/sys/mod.rs` — `pub mod timer`。

### Requirement: Nix Linux filesystem quota operations
Quota wrappers SHALL model commands, subcommands, quota types/formats/valid flags and Dqblk limits/timestamps, then expose quotactl on/off/sync/get/set calls with typed Errno results.

#### Scenario: Nix Linux filesystem quota operations implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/quota.rs` — `struct QuotaCmd`；`third_party/nix-ohos/src/sys/quota.rs` — `fn as_int`；`third_party/nix-ohos/src/sys/quota.rs` — `enum QuotaSubCmd`；`third_party/nix-ohos/src/sys/quota.rs` — `enum QuotaType`；`third_party/nix-ohos/src/sys/quota.rs` — `enum QuotaFmt`；`third_party/nix-ohos/src/sys/quota.rs` — `struct QuotaValidFlags`；`third_party/nix-ohos/src/sys/quota.rs` — `struct Dqblk`；`third_party/nix-ohos/src/sys/quota.rs` — `fn default`；`third_party/nix-ohos/src/sys/quota.rs` — `fn blocks_hard_limit`；`third_party/nix-ohos/src/sys/quota.rs` — `fn set_blocks_hard_limit`；`third_party/nix-ohos/src/sys/quota.rs` — `fn blocks_soft_limit`；`third_party/nix-ohos/src/sys/quota.rs` — `fn set_blocks_soft_limit`；`third_party/nix-ohos/src/sys/quota.rs` — `fn occupied_space`；`third_party/nix-ohos/src/sys/quota.rs` — `fn inodes_hard_limit`；`third_party/nix-ohos/src/sys/quota.rs` — `fn set_inodes_hard_limit`；`third_party/nix-ohos/src/sys/quota.rs` — `fn inodes_soft_limit`；`third_party/nix-ohos/src/sys/quota.rs` — `fn set_inodes_soft_limit`；`third_party/nix-ohos/src/sys/quota.rs` — `fn allocated_inodes`；`third_party/nix-ohos/src/sys/quota.rs` — `fn block_time_limit`；`third_party/nix-ohos/src/sys/quota.rs` — `fn set_block_time_limit`；`third_party/nix-ohos/src/sys/quota.rs` — `fn inode_time_limit`；`third_party/nix-ohos/src/sys/quota.rs` — `fn set_inode_time_limit`；`third_party/nix-ohos/src/sys/quota.rs` — `fn quotactl`；`third_party/nix-ohos/src/sys/quota.rs` — `fn quotactl_on`；`third_party/nix-ohos/src/sys/quota.rs` — `fn quotactl_off`；`third_party/nix-ohos/src/sys/quota.rs` — `fn quotactl_sync`；`third_party/nix-ohos/src/sys/quota.rs` — `fn quotactl_get`；`third_party/nix-ohos/src/sys/quota.rs` — `fn quotactl_set`。

### Requirement: Nix select and pselect readiness
FdSet SHALL validate, insert/remove/test/clear descriptors, report highest/iterate descriptors, and select/pselect SHALL wait with timeout and signal-mask support while converting libc errors.

#### Scenario: test:fdset_insert
- **WHEN** the embedded test function fdset_insert is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:fdset_remove
- **WHEN** the embedded test function fdset_remove is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:fdset_clear
- **WHEN** the embedded test function fdset_clear is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/select.rs` — `struct FdSet`；`third_party/nix-ohos/src/sys/select.rs` — `fn assert_fd_valid`；`third_party/nix-ohos/src/sys/select.rs` — `fn new`；`third_party/nix-ohos/src/sys/select.rs` — `fn insert`；`third_party/nix-ohos/src/sys/select.rs` — `fn remove`；`third_party/nix-ohos/src/sys/select.rs` — `fn contains`；`third_party/nix-ohos/src/sys/select.rs` — `fn clear`；`third_party/nix-ohos/src/sys/select.rs` — `fn highest`；`third_party/nix-ohos/src/sys/select.rs` — `fn fds`；`third_party/nix-ohos/src/sys/select.rs` — `fn default`；`third_party/nix-ohos/src/sys/select.rs` — `struct Fds`；`third_party/nix-ohos/src/sys/select.rs` — `type Item`；`third_party/nix-ohos/src/sys/select.rs` — `fn next`；`third_party/nix-ohos/src/sys/select.rs` — `fn size_hint`；`third_party/nix-ohos/src/sys/select.rs` — `fn next_back`；`third_party/nix-ohos/src/sys/select.rs` — `fn select`；`third_party/nix-ohos/src/sys/select.rs` — `fn pselect`；`third_party/nix-ohos/src/sys/select.rs` — `fn fdset_insert`；`third_party/nix-ohos/src/sys/select.rs` — `fn fdset_remove`；`third_party/nix-ohos/src/sys/select.rs` — `fn fdset_clear`；`third_party/nix-ohos/src/sys/select.rs` — `fn fdset_highest`；`third_party/nix-ohos/src/sys/select.rs` — `fn fdset_fds`；`third_party/nix-ohos/src/sys/select.rs` — `fn test_select`；`third_party/nix-ohos/src/sys/select.rs` — `fn test_select_nfds`；`third_party/nix-ohos/src/sys/select.rs` — `fn test_select_nfds2`。

### Requirement: Nix zero-copy file transfer
sendfile SHALL wrap platform zero-copy file-to-socket transfers with optional offset mutation, count/result handling, and SendfileHeaderTrailer/SfFlags support where the target ABI provides it.

#### Scenario: Nix zero-copy file transfer implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/sendfile.rs` — `fn sendfile`；`third_party/nix-ohos/src/sys/sendfile.rs` — `fn sendfile64`；`third_party/nix-ohos/src/sys/sendfile.rs` — `struct SendfileHeaderTrailer`；`third_party/nix-ohos/src/sys/sendfile.rs` — `fn new`；`third_party/nix-ohos/src/sys/sendfile.rs` — `struct SfFlags`；`third_party/nix-ohos/src/sys/sendfile.rs` — `fn sendfile`；`third_party/nix-ohos/src/sys/sendfile.rs` — `fn sendfile`；`third_party/nix-ohos/src/sys/sendfile.rs` — `fn sendfile`。

### Requirement: Nix socket address model
Socket address types SHALL convert IPv4/IPv6/Unix/link/vsock/netlink/system-control addresses to and from libc storage, validate lengths/families, preserve abstract/non-UTF8 Unix names, and provide display/size/raw-storage accessors under platform cfgs.

#### Scenario: test:test_ipv4addr_to_libc
- **WHEN** the embedded test function test_ipv4addr_to_libc is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:test_ipv6addr_to_libc
- **WHEN** the embedded test function test_ipv6addr_to_libc is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:test_datalink_display
- **WHEN** the embedded test function test_datalink_display is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn ipv4addr_to_libc`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn ipv6addr_to_libc`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `enum AddressFamily`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_i32`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `enum InetAddr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_std`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn ip`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn port`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn to_std`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn to_str`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `enum IpAddr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new_v4`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new_v6`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_std`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn to_std`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct Ipv4Addr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_std`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn any`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn octets`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn to_std`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct Ipv6Addr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `macro to_u8_array`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `macro to_u16_array`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_std`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn segments`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn to_std`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct UnixAddr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `enum UnixAddrKind`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn get`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new_abstract`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new_unnamed`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw_parts`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn kind`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn path`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_abstract`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn is_unnamed`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn path_len`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_ptr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_mut_ptr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn sun_len`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn len`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn set_length`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt_abstract`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn eq`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn hash`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `trait SockaddrLike`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_ptr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn family`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn len`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn len`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn set_length`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct SocketAddressLengthNotDynamic`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_mut_ptr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_ptr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn family`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn len`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct SockaddrIn`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn ip`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn port`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `type Err`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_str`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct SockaddrIn6`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn flowinfo`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn ip`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn port`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn scope_id`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `type Err`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_str`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn len`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn set_length`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `macro accessors`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_unix_addr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_unix_addr_mut`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn hash`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn eq`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `trait SockaddrLikePriv`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_mut_ptr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `enum SockAddr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new_inet`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new_unix`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new_netlink`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new_alg`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new_sys_control`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new_vsock`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn family`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn to_str`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_libc_sockaddr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_ffi_pair`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct NetlinkAddr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn pid`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn groups`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct AlgAddr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn eq`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn hash`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn alg_type`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn alg_name`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct ctl_ioc_info`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `const CTL_IOC_MAGIC`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `const CTL_IOC_INFO`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `const MAX_KCTL_NAME`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct SysControlAddr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_name`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn id`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn unit`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct LinkAddr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn protocol`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn ifindex`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn hatype`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn pkttype`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn halen`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn addr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct LinkAddr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn ifindex`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn datalink_type`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn nlen`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn alen`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn slen`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn is_empty`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn addr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct VsockAddr`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn as_ref`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn eq`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn hash`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn cid`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn port`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `struct Raw`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn test_ipv4addr_to_libc`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn test_ipv6addr_to_libc`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn test_datalink_display`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn linux_loopback`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn macos_loopback`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn macos_tap`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn illumos_tap`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn size`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn display`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn size`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn display`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn size`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn to_and_from`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_sockaddr_un_named`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_sockaddr_un_abstract_named`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn from_sockaddr_un_abstract_unnamed`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn abstract_sun_path`；`third_party/nix-ohos/src/sys/socket/addr.rs` — `fn size`。

### Requirement: Nix socket syscall boundary
Socket APIs SHALL model socket types/protocols/flags/messages/credentials and address storage, expose socket/bind/connect/listen/accept/send/recv/sendmsg/recvmsg/shutdown/socketpair/getsockname/getpeername and ancillary-data helpers, and convert libc errors with feature/target gates.

#### Scenario: test:test_recvmm2
- **WHEN** the embedded test function test_recvmm2 is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:can_use_cmsg_space
- **WHEN** the embedded test function can_use_cmsg_space is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/socket/mod.rs` — `enum SockType`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `type Error`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn try_from`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `enum SockProtocol`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct TimestampingFlag`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct SockFlag`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct MsgFlags`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct UnixCredentials`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn pid`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn uid`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn gid`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn default`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn from`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn from`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct UnixCredentials`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn pid`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn uid`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn euid`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn gid`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn groups`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn from`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct XuCred`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn version`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn uid`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn groups`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct IpMembershipRequest`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct Ipv6MembershipRequest`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `macro cmsg_space`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct RecvMsg`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn cmsgs`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct CmsgIterator`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `type Item`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn next`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `enum ControlMessageOwned`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct Timestamps`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn decode_from`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn recv_err_helper`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `enum ControlMessage`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct UnknownCmsg`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn space`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn cmsg_len`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn cmsg_len`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn copy_to_cmsg_data`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn len`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn cmsg_level`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn cmsg_type`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn encode_into`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn sendmsg`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn sendmmsg`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct MultiHeaders`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn preallocate`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn recvmmsg`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct MultiResults`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `type Item`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn next`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn iovs`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `struct IoSliceIterator`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `type Item`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn next`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn read_mhdr`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn pack_mhdr_to_receive`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn pack_mhdr_to_send`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn recvmsg`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn socket`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn socketpair`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn listen`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn bind`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn accept`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn accept4`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn connect`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn recv`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn recvfrom`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn sendto`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn send`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `trait GetSockOpt`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `type Val`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn get`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `trait SetSockOpt`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `type Val`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn set`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn getsockopt`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn setsockopt`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn getpeername`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn getsockname`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn sockaddr_storage_to_addr`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `enum Shutdown`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn shutdown`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn test_recvmm2`；`third_party/nix-ohos/src/sys/socket/mod.rs` — `fn can_use_cmsg_space`。

### Requirement: Nix socket option boundary
Socket-option macros and implementations SHALL encode typed SetSockOpt/GetSockOpt access for platform options, validate buffer lengths/initialization, support credentials/TCP/netlink/security options, and route setsockopt/getsockopt failures through Errno.

#### Scenario: test:can_get_peercred_on_unix_socket
- **WHEN** the embedded test function can_get_peercred_on_unix_socket is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:is_socket_type_unix
- **WHEN** the embedded test function is_socket_type_unix is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:is_socket_type_dgram
- **WHEN** the embedded test function is_socket_type_dgram is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `const TCP_CA_NAME_MAX`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `macro setsockopt_impl`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `type Val`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn set`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `macro getsockopt_impl`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `type Val`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn get`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `macro sockopt_impl`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `struct AlgSetAeadAuthSize`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `type Val`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn set`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `struct AlgSetKey`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn default`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `type Val`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn set`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `trait Get`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn uninit`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_ptr`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_len`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn assume_init`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `trait Set`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_ptr`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_len`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `struct GetStruct`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn uninit`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_ptr`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_len`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn assume_init`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `struct SetStruct`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_ptr`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_len`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `struct GetBool`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn uninit`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_ptr`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_len`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn assume_init`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `struct SetBool`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_ptr`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_len`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `struct GetU8`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn uninit`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_ptr`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_len`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn assume_init`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `struct SetU8`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_ptr`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_len`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `struct GetUsize`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn uninit`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_ptr`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_len`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn assume_init`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `struct SetUsize`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_ptr`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_len`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `struct GetOsString`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn uninit`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_ptr`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_len`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn assume_init`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `struct SetOsString`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn new`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_ptr`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn ffi_len`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn can_get_peercred_on_unix_socket`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn is_socket_type_unix`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn is_socket_type_dgram`；`third_party/nix-ohos/src/sys/socket/sockopt.rs` — `fn can_get_listen_on_tcp_socket`。

### Requirement: Nix filesystem metadata and modes
stat SHALL model file type/mode/flags and wrap mknod/mknodat/stat/lstat/fstat/fstatat/chmod/fchmod/fchmodat/utimes/lutimes/futimens/utimensat/mkdirat, preserving platform-specific flag and timestamp behavior.

#### Scenario: Nix filesystem metadata and modes implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/stat.rs` — `struct SFlag`；`third_party/nix-ohos/src/sys/stat.rs` — `struct Mode`；`third_party/nix-ohos/src/sys/stat.rs` — `type type_of_file_flag`；`third_party/nix-ohos/src/sys/stat.rs` — `type type_of_file_flag`；`third_party/nix-ohos/src/sys/stat.rs` — `struct FileFlag`；`third_party/nix-ohos/src/sys/stat.rs` — `fn mknod`；`third_party/nix-ohos/src/sys/stat.rs` — `fn mknodat`；`third_party/nix-ohos/src/sys/stat.rs` — `fn major`；`third_party/nix-ohos/src/sys/stat.rs` — `fn minor`；`third_party/nix-ohos/src/sys/stat.rs` — `fn makedev`；`third_party/nix-ohos/src/sys/stat.rs` — `fn umask`；`third_party/nix-ohos/src/sys/stat.rs` — `fn stat`；`third_party/nix-ohos/src/sys/stat.rs` — `fn lstat`；`third_party/nix-ohos/src/sys/stat.rs` — `fn fstat`；`third_party/nix-ohos/src/sys/stat.rs` — `fn fstatat`；`third_party/nix-ohos/src/sys/stat.rs` — `fn fchmod`；`third_party/nix-ohos/src/sys/stat.rs` — `enum FchmodatFlags`；`third_party/nix-ohos/src/sys/stat.rs` — `fn fchmodat`；`third_party/nix-ohos/src/sys/stat.rs` — `fn utimes`；`third_party/nix-ohos/src/sys/stat.rs` — `fn lutimes`；`third_party/nix-ohos/src/sys/stat.rs` — `fn futimens`；`third_party/nix-ohos/src/sys/stat.rs` — `enum UtimensatFlags`；`third_party/nix-ohos/src/sys/stat.rs` — `fn utimensat`；`third_party/nix-ohos/src/sys/stat.rs` — `fn mkdirat`。

### Requirement: Nix filesystem statistics backend
statfs SHALL model platform filesystem type/stat structures and wrap statfs/fstatfs plus strict variants, including OHOS-as-musl layout cfgs and typed FsType conversion.

#### Scenario: test:statfs_call
- **WHEN** the embedded test function statfs_call is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:fstatfs_call
- **WHEN** the embedded test function fstatfs_call is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:statfs_call_strict
- **WHEN** the embedded test function statfs_call_strict is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/statfs.rs` — `type fsid_t`；`third_party/nix-ohos/src/sys/statfs.rs` — `type fsid_t`；`third_party/nix-ohos/src/sys/statfs.rs` — `type type_of_statfs`；`third_party/nix-ohos/src/sys/statfs.rs` — `const LIBC_FSTATFS`；`third_party/nix-ohos/src/sys/statfs.rs` — `const LIBC_STATFS`；`third_party/nix-ohos/src/sys/statfs.rs` — `type type_of_statfs`；`third_party/nix-ohos/src/sys/statfs.rs` — `const LIBC_FSTATFS`；`third_party/nix-ohos/src/sys/statfs.rs` — `const LIBC_STATFS`；`third_party/nix-ohos/src/sys/statfs.rs` — `struct Statfs`；`third_party/nix-ohos/src/sys/statfs.rs` — `type fs_type_t`；`third_party/nix-ohos/src/sys/statfs.rs` — `type fs_type_t`；`third_party/nix-ohos/src/sys/statfs.rs` — `type fs_type_t`；`third_party/nix-ohos/src/sys/statfs.rs` — `type fs_type_t`；`third_party/nix-ohos/src/sys/statfs.rs` — `type fs_type_t`；`third_party/nix-ohos/src/sys/statfs.rs` — `type fs_type_t`；`third_party/nix-ohos/src/sys/statfs.rs` — `struct FsType`；`third_party/nix-ohos/src/sys/statfs.rs` — `const ADFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const AFFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const AFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const AUTOFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const BPF_FS_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const BTRFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const CGROUP2_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const CGROUP_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const CODA_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const CRAMFS_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const DEBUGFS_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const DEVPTS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const ECRYPTFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const EFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const EXT2_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const EXT3_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const EXT4_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const F2FS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const FUSE_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const FUTEXFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const HOSTFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const HPFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const HUGETLBFS_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const ISOFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const JFFS2_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const MINIX2_SUPER_MAGIC2`；`third_party/nix-ohos/src/sys/statfs.rs` — `const MINIX2_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const MINIX3_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const MINIX_SUPER_MAGIC2`；`third_party/nix-ohos/src/sys/statfs.rs` — `const MINIX_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const MSDOS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const NCP_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const NFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const NILFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const OCFS2_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const OPENPROM_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const OVERLAYFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const PROC_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const QNX4_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const QNX6_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const RDTGROUP_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const REISERFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const SECURITYFS_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const SELINUX_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const SMACK_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const SMB_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const SYSFS_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const TMPFS_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const TRACEFS_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const UDF_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const USBDEVICE_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const XENFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const NSFS_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `const XFS_SUPER_MAGIC`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn filesystem_type`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn filesystem_type_name`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn optimal_transfer_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn optimal_transfer_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn optimal_transfer_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn optimal_transfer_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn optimal_transfer_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn optimal_transfer_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn optimal_transfer_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn optimal_transfer_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn block_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn block_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn block_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn block_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn block_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn block_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn block_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn block_size`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn flags`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn flags`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn maximum_name_length`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn maximum_name_length`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn maximum_name_length`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn maximum_name_length`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn maximum_name_length`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn maximum_name_length`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn blocks`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn blocks`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn blocks`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn blocks_free`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn blocks_free`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn blocks_free`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn blocks_available`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn blocks_available`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn blocks_available`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn blocks_available`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn files`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn files`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn files`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn files_free`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn files_free`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn files_free`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn files_free`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn filesystem_id`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn fmt`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn statfs`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn fstatfs`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn check_fstatfs`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn check_statfs`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn assert_fs_equals`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn check_fstatfs_strict`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn check_statfs_strict`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn assert_fs_equals_strict`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn statfs_call`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn fstatfs_call`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn statfs_call_strict`；`third_party/nix-ohos/src/sys/statfs.rs` — `fn fstatfs_call_strict`。

### Requirement: Nix portable filesystem statistics
statvfs SHALL model FsFlags/Statvfs fields and wrap statvfs/fstatvfs for path and descriptor inputs, preserving platform flag semantics and Errno conversion.

#### Scenario: test:statvfs_call
- **WHEN** the embedded test function statvfs_call is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:fstatvfs_call
- **WHEN** the embedded test function fstatvfs_call is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/statvfs.rs` — `struct FsFlags`；`third_party/nix-ohos/src/sys/statvfs.rs` — `struct Statvfs`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn block_size`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn fragment_size`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn blocks`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn blocks_free`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn blocks_available`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn files`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn files_free`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn files_available`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn filesystem_id`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn flags`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn name_max`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn statvfs`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn fstatvfs`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn statvfs_call`；`third_party/nix-ohos/src/sys/statvfs.rs` — `fn fstatvfs_call`。

### Requirement: Nix Linux system information
sysinfo SHALL wrap Linux system information and expose load average, uptime, process count, swap totals/free, RAM totals/unused, and scaled memory conversion.

#### Scenario: Nix Linux system information implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/sysinfo.rs` — `struct SysInfo`；`third_party/nix-ohos/src/sys/sysinfo.rs` — `type mem_blocks_t`；`third_party/nix-ohos/src/sys/sysinfo.rs` — `type mem_blocks_t`；`third_party/nix-ohos/src/sys/sysinfo.rs` — `fn load_average`；`third_party/nix-ohos/src/sys/sysinfo.rs` — `fn uptime`；`third_party/nix-ohos/src/sys/sysinfo.rs` — `fn process_count`；`third_party/nix-ohos/src/sys/sysinfo.rs` — `fn swap_total`；`third_party/nix-ohos/src/sys/sysinfo.rs` — `fn swap_free`；`third_party/nix-ohos/src/sys/sysinfo.rs` — `fn ram_total`；`third_party/nix-ohos/src/sys/sysinfo.rs` — `fn ram_unused`；`third_party/nix-ohos/src/sys/sysinfo.rs` — `fn scale_mem`；`third_party/nix-ohos/src/sys/sysinfo.rs` — `fn sysinfo`。

### Requirement: Nix termios safe wrapper
Termios SHALL copy and validate libc termios fields through safe flag/control-character/baud-rate/flow/flush/set-argument types, expose tcgetattr/tcsetattr/tcdrain/tcflush/tcflow/tcgetsid and related operations, and preserve platform-specific mappings.

#### Scenario: test:try_from
- **WHEN** the embedded test function try_from is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/termios.rs` — `struct Termios`；`third_party/nix-ohos/src/sys/termios.rs` — `fn get_libc_termios`；`third_party/nix-ohos/src/sys/termios.rs` — `fn get_libc_termios_mut`；`third_party/nix-ohos/src/sys/termios.rs` — `fn update_wrapper`；`third_party/nix-ohos/src/sys/termios.rs` — `fn from`；`third_party/nix-ohos/src/sys/termios.rs` — `fn from`；`third_party/nix-ohos/src/sys/termios.rs` — `enum BaudRate`；`third_party/nix-ohos/src/sys/termios.rs` — `fn from`；`third_party/nix-ohos/src/sys/termios.rs` — `fn from`；`third_party/nix-ohos/src/sys/termios.rs` — `enum SetArg`；`third_party/nix-ohos/src/sys/termios.rs` — `enum FlushArg`；`third_party/nix-ohos/src/sys/termios.rs` — `enum FlowArg`；`third_party/nix-ohos/src/sys/termios.rs` — `enum SpecialCharacterIndices`；`third_party/nix-ohos/src/sys/termios.rs` — `const VMIN`；`third_party/nix-ohos/src/sys/termios.rs` — `const VTIME`；`third_party/nix-ohos/src/sys/termios.rs` — `struct InputFlags`；`third_party/nix-ohos/src/sys/termios.rs` — `struct OutputFlags`；`third_party/nix-ohos/src/sys/termios.rs` — `struct ControlFlags`；`third_party/nix-ohos/src/sys/termios.rs` — `struct LocalFlags`；`third_party/nix-ohos/src/sys/termios.rs` — `fn cfgetispeed`；`third_party/nix-ohos/src/sys/termios.rs` — `fn cfgetospeed`；`third_party/nix-ohos/src/sys/termios.rs` — `fn cfsetispeed`；`third_party/nix-ohos/src/sys/termios.rs` — `fn cfsetospeed`；`third_party/nix-ohos/src/sys/termios.rs` — `fn cfsetspeed`；`third_party/nix-ohos/src/sys/termios.rs` — `fn cfgetispeed`；`third_party/nix-ohos/src/sys/termios.rs` — `fn cfgetospeed`；`third_party/nix-ohos/src/sys/termios.rs` — `fn cfsetispeed`；`third_party/nix-ohos/src/sys/termios.rs` — `fn cfsetospeed`；`third_party/nix-ohos/src/sys/termios.rs` — `fn cfsetspeed`；`third_party/nix-ohos/src/sys/termios.rs` — `fn cfmakeraw`；`third_party/nix-ohos/src/sys/termios.rs` — `fn cfmakesane`；`third_party/nix-ohos/src/sys/termios.rs` — `fn tcgetattr`；`third_party/nix-ohos/src/sys/termios.rs` — `fn tcsetattr`；`third_party/nix-ohos/src/sys/termios.rs` — `fn tcdrain`；`third_party/nix-ohos/src/sys/termios.rs` — `fn tcflow`；`third_party/nix-ohos/src/sys/termios.rs` — `fn tcflush`；`third_party/nix-ohos/src/sys/termios.rs` — `fn tcsendbreak`；`third_party/nix-ohos/src/sys/termios.rs` — `fn tcgetsid`；`third_party/nix-ohos/src/sys/termios.rs` — `fn try_from`。

### Requirement: Nix vectored and remote I/O
uio SHALL wrap readv/writev/preadv/pwritev/pread/pwrite and process_vm_readv/process_vm_writev, expose RemoteIoVec/IoVec safe slice construction, and preserve pointer/length and Errno boundaries.

#### Scenario: Nix vectored and remote I/O implementation
- **WHEN** the audited file is loaded under its declared cfg/feature context
- **THEN** the listed symbols provide the documented native API boundary and error/resource rules.

证据：`third_party/nix-ohos/src/sys/uio.rs` — `fn writev`；`third_party/nix-ohos/src/sys/uio.rs` — `fn readv`；`third_party/nix-ohos/src/sys/uio.rs` — `fn pwritev`；`third_party/nix-ohos/src/sys/uio.rs` — `fn preadv`；`third_party/nix-ohos/src/sys/uio.rs` — `fn pwrite`；`third_party/nix-ohos/src/sys/uio.rs` — `fn pread`；`third_party/nix-ohos/src/sys/uio.rs` — `struct RemoteIoVec`；`third_party/nix-ohos/src/sys/uio.rs` — `struct IoVec`；`third_party/nix-ohos/src/sys/uio.rs` — `fn as_slice`；`third_party/nix-ohos/src/sys/uio.rs` — `fn from_slice`；`third_party/nix-ohos/src/sys/uio.rs` — `fn from_mut_slice`；`third_party/nix-ohos/src/sys/uio.rs` — `fn process_vm_writev`；`third_party/nix-ohos/src/sys/uio.rs` — `fn process_vm_readv`。

### Requirement: Nix system identification
UtsName SHALL wrap uname, expose trimmed system/node/release/version/machine/domain strings, and retain platform-specific tests for the returned identification fields.

#### Scenario: test:test_uname_linux
- **WHEN** the embedded test function test_uname_linux is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:test_uname_darwin
- **WHEN** the embedded test function test_uname_darwin is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

#### Scenario: test:test_uname_freebsd
- **WHEN** the embedded test function test_uname_freebsd is executed
- **THEN** its assertions cover the file-local API boundary; this static audit does not claim execution success.

证据：`third_party/nix-ohos/src/sys/utsname.rs` — `struct UtsName`；`third_party/nix-ohos/src/sys/utsname.rs` — `fn sysname`；`third_party/nix-ohos/src/sys/utsname.rs` — `fn nodename`；`third_party/nix-ohos/src/sys/utsname.rs` — `fn release`；`third_party/nix-ohos/src/sys/utsname.rs` — `fn version`；`third_party/nix-ohos/src/sys/utsname.rs` — `fn machine`；`third_party/nix-ohos/src/sys/utsname.rs` — `fn domainname`；`third_party/nix-ohos/src/sys/utsname.rs` — `fn uname`；`third_party/nix-ohos/src/sys/utsname.rs` — `fn cast_and_trim`；`third_party/nix-ohos/src/sys/utsname.rs` — `fn test_uname_linux`；`third_party/nix-ohos/src/sys/utsname.rs` — `fn test_uname_darwin`；`third_party/nix-ohos/src/sys/utsname.rs` — `fn test_uname_freebsd`。
