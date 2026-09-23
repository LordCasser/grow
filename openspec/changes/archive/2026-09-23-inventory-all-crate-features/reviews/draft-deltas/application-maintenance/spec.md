## ADDED Requirements

### Requirement: Installed version and display
版本层 SHALL 以编译期 GROW_VERSION 优先于 package version，installed() 允许运行期 GROW_TEST_VERSION 覆盖并去除两端空白；展示函数分别追加渠道后缀。

#### Scenario: 编译版本更新
- **WHEN** 构建环境 GROW_VERSION 改变
- **THEN** build.rs 触发重新构建版本信息。

#### Scenario: 非法语义版本
- **WHEN** installed() 返回非 semver 文本
- **THEN** installed_semver 返回解析错误；display_version 使用编译版本而非测试覆盖值。

证据：`crates/codegen/version/src/lib.rs` — `installed_semver`；`crates/codegen/version/build.rs` — `GROW_VERSION`。

### Requirement: Process liveness and termination helpers
进程辅助函数 SHALL 在 Unix 用 kill(pid,0) 检查，只有 ESRCH 为不存在，其他错误视为仍存在；Windows 打开 SYNCHRONIZE handle 并零等待，只有 WAIT_TIMEOUT 为仍运行。

#### Scenario: 终止
- **WHEN** 调用 kill_process_by_pid 或指定信号
- **THEN** 默认 Unix SIGTERM，可选 SIGKILL，ESRCH 成功；Windows 两种信号都 TerminateProcess，OpenProcess 的 ERROR_INVALID_PARAMETER 视为已结束，其他错误透传。

#### Scenario: 调用边界
- **WHEN** 传入进程 ID
- **THEN** 本层不持有进程身份句柄跨检查与终止，Unix 将 u32 转 i32，不自动过滤 0 或转换后的负 PID；调用方负责有效 PID 与目标所有权。

证据：`crates/codegen/shell-base/src/util/mod.rs` — `is_process_alive`；`crates/codegen/shell-base/src/util/mod.rs` — `kill_process_with_signal`。

### Requirement: Grow process name probes
is_grow_process SHALL 在 Linux 读取 proc cmdline 并按 grow 子串匹配，Windows 查询完整 image path 并小写后子串匹配，其他平台运行 kill -0 只测存活。

#### Scenario: strict 判定
- **WHEN** 调用 is_grow_process_strict
- **THEN** macOS/BSD 使用 ps comm 首行 basename 小写后匹配 grow 子串，Linux/Windows 委托普通探测；失败返回 false，不提供可执行文件精确身份或 PID 重用防护保证。

证据：`crates/codegen/shell-base/src/util/mod.rs` — `is_grow_process`；`crates/codegen/shell-base/src/util/mod.rs` — `is_grow_process_strict`。


### Requirement: Shell builtin extraction ownership and commit marker

extract_builtin_files SHALL 管理内嵌README.md和workflows/deep-research.rhai，错误仅debug记录，不向调用方返回。创建home后拒绝home自身symlink或非目录，持有.builtin-extraction.lock后读取版本；版本不匹配时尽力删CHANGELOG.json/md。每次调用均按内容核对并写两个文件，即使版本相同；deep-research写入另持workflows/.deep-research.lock。最后写.metadata_version，不回滚已写文件；该marker不是多个文件同时可见的原子事务。

#### Scenario: Same version changed file
- **WHEN** metadata版本相同但内嵌README内容被改动
- **THEN** 仍按编译内嵌内容恢复该管理文件。

证据：`crates/codegen/shell/src/builtin.rs` — `pub fn extract_builtin_files`；`crates/codegen/shell/src/builtin.rs` — `fn extract_builtin_files_transaction`。

### Requirement: Shell builtin managed file replacement and durability

管理文件写入 SHALL 拒绝绝对或非Normal路径组件，逐级创建并拒绝symlink/non-directory父级，管理锁不跟随symlink且要求普通文件。匹配读取要求普通文件、相同长度和精确字节，最多读取预期长度加1；NotFound/InvalidInput视作不匹配，其他错误传播。不同内容使用同目录UUID v7临时文件create_new、write_all、sync_all、rename，然后Unix同步父目录，非Unix父目录同步为空操作。错误尽力删临时文件；rename后父目录同步失败仍可能已有新内容。

#### Scenario: Parent sync failure
- **WHEN** rename成功但Unix父目录sync失败
- **THEN** 返回错误并保持已替换内容，后续重试可继续。

证据：`crates/codegen/shell/src/builtin.rs` — `fn write_managed_file`；`crates/codegen/shell/src/builtin.rs` — `fn managed_file_matches`；`crates/codegen/shell/src/builtin.rs` — `fn ensure_real_parent_dirs`；`crates/codegen/shell/src/builtin.rs` — `fn sync_managed_parent`。

### Requirement: Shell bundle archive entry accounting

解包 SHALL 只处理Regular条目，最多1000项、单项header size最多1MiB、Regular声明大小checked sum最多50MiB；无关Regular条目也计入，非Regular在计数前跳过。这些限制不构成整个gzip解压流字节或耗时上限。

#### Scenario: Ignored regular entry exceeds limit
- **WHEN** 无关路径Regular条目声明大小超过1MiB
- **THEN** 仍返回大小限制错误。

证据：`crates/codegen/shell/src/bundle.rs` — `pub fn extract_bundle_archive`；`crates/codegen/shell/src/bundle.rs` — `fn archive_rejects_traversal_and_oversized_entries`。

### Requirement: Shell bundle catalog path mapping

归档路径 SHALL 去掉一次前导./；subagents/后的路径或skills/路径必须通过词法校验，否则返回错误；其他路径忽略。缓存只接受agents/<name>.md及skills/<name>/<非空子路径>，拒绝绝对路径、反斜杠、点/双点和控制字符名称；skill文件不限定SKILL.md或扩展名。此校验不检查磁盘父目录或目标的符号链接身份。

#### Scenario: Root agent path ignored
- **WHEN** 归档提供agents/reviewer.md而非subagents/agents/reviewer.md
- **THEN** 条目计入Regular预算但不会写入缓存。

证据：`crates/codegen/shell/src/bundle.rs` — `fn map_archive_path_to_cache_path`；`crates/codegen/shell/src/bundle.rs` — `fn sanitize_relative_path`；`crates/codegen/shell/src/bundle.rs` — `fn validate_bundle_name`；`crates/codegen/shell/src/bundle.rs` — `fn canonical_paths_accept_only_agents_and_skills`。

### Requirement: Shell bundle managed checksum reconciliation

解包 SHALL 用SHA256比较当前完整文件和旧manifest：缺失或匹配旧checksum才写新内容；修改过的托管文件保留旧checksum，未托管文件保持原样且不登记。移除的旧托管项只在当前checksum匹配时删除，修改过的保留并继续登记旧checksum。重复归档路径始终与旧manifest比较，不以本轮新checksum作为更新基线。

#### Scenario: Preserve removed local edit
- **WHEN** 旧manifest中的文件不再出现在包中且已被本地修改
- **THEN** 保留文件，并将旧checksum放入下一manifest。

证据：`crates/codegen/shell/src/bundle.rs` — `fn bundle_file_state`；`crates/codegen/shell/src/bundle.rs` — `pub fn prune_removed_files`；`crates/codegen/shell/src/bundle.rs` — `pub fn extract_bundle_archive`；`crates/codegen/shell/src/bundle.rs` — `fn modified_managed_files_survive_pruning`。

### Requirement: Shell bundle manifest publication failure boundary

缓存manifest读取 SHALL 仅对NotFound返回None，其他读错/JSON错误上抛；解包使用前过滤非法checksum路径。bundle.json的version必须最终非空，重复元数据最后一个生效，不检查semver。内容逐项直接写入，完整遍历后才检查version、清理旧项和直接写manifest；没有事务锁、原子替换或失败回滚，后续错误可能留下已写内容。

#### Scenario: Missing metadata after content
- **WHEN** 归档含有效托管文件但缺少bundle.json
- **THEN** 文件可能已写入，随后返回缺少version错误，不发布新manifest。

证据：`crates/codegen/shell/src/bundle.rs` — `pub fn read_cached_manifest`；`crates/codegen/shell/src/bundle.rs` — `fn sanitize_manifest`；`crates/codegen/shell/src/bundle.rs` — `pub fn extract_bundle_archive`；`crates/codegen/shell/src/bundle.rs` — `fn write_bundle_file`。

### Requirement: Shell bundle cache utility scope

bundled_root SHALL 返回grow_home/bundled；checksum_file读取整个文件后生成小写SHA256，没有归档大小限制；count_entries_by_prefix直接统计manifest键的字符串前缀，不验证路径或按skill目录去重。测试fixture安装器直接生成agent md与skill SKILL.md，复用checksum保留及清理逻辑，但不经过gzip预算或非空version检查。

#### Scenario: Prefix counts files not skills
- **WHEN** 同一skill有多个键且均匹配prefix
- **THEN** 每个文件键分别计数。

证据：`crates/codegen/shell/src/bundle.rs` — `pub fn bundled_root`；`crates/codegen/shell/src/bundle.rs` — `pub fn checksum_file`；`crates/codegen/shell/src/bundle.rs` — `pub fn count_entries_by_prefix`；`crates/codegen/shell/src/bundle.rs` — `pub fn install_test_bundle_fixture`。

### Requirement: Shell bundle explicit and proactive sync distinction

grow/bundle/sync SHALL 在agent无凭据时返回错误，有凭据时每次下载并spawn_blocking解包；force参数在该路径被忽略，成功始终updated=true，不代表内容发生变化。主动helper仅在非force且manifest mtime严格小于TTL并可解析时跳过，默认TTL一小时；不检查checksum或实际文件完整性，未来mtime不算新鲜。helper本身需要已构造凭据，不含无凭据跳过分支。

#### Scenario: Fresh manifest with missing files
- **WHEN** manifest可解析且mtime在TTL内，但托管文件已删除
- **THEN** 非force主动helper仍跳过；显式sync仍尝试下载。

证据：`crates/codegen/shell/src/extensions/bundle.rs` — `pub async fn handle`；`crates/codegen/shell/src/extensions/bundle.rs` — `pub(crate) fn bundle_cache_is_fresh`；`crates/codegen/shell/src/extensions/bundle.rs` — `pub(crate) async fn maybe_sync_bundle_to_root`；`crates/codegen/shell/src/extensions/bundle.rs` — `pub(crate) async fn sync_bundle_to_root`。

### Requirement: Shell bundle status and entry lookup asymmetry

status SHALL 对无manifest返回hasCache=false；可解析manifest返回true及version，坏manifest报错。agent列表要求目录中存在md文件且键在manifest，skill列表从manifest中skills/前缀及/SKILL.md后缀键取名并检查is_file；排序但不校验checksum、不清洗manifest键。entry/get只支持agent，拒绝空名、斜杠、反斜杠、任意双点子串及单点，直接读UTF8文件而不要求manifest登记；不提供skill内容读取。

#### Scenario: Unmanaged agent readable but not listed
- **WHEN** agents/reviewer.md存在但不在manifest
- **THEN** status不列出，entry/get仍能返回内容。

证据：`crates/codegen/shell/src/extensions/bundle.rs` — `fn status_bundle_at`；`crates/codegen/shell/src/extensions/bundle.rs` — `fn list_cached_entries`；`crates/codegen/shell/src/extensions/bundle.rs` — `fn list_cached_skill_entries`；`crates/codegen/shell/src/extensions/bundle.rs` — `fn get_entry_at`；`crates/codegen/shell/src/extensions/bundle.rs` — `fn validate_entry_name`；`crates/codegen/shell/src/extensions/bundle.rs` — `fn status_reads_only_manifest_backed_entries`。

### Requirement: Shell bundle HTTP response and resource boundary

fetch_bundle SHALL 使用共享OnceLock客户端、30秒连接超时、30秒请求timeout及禁止redirect策略，GET绑定地址并带Bearer key、版本、进程identifier和mode头。非2xx仅返回状态码，不读取响应体进入该错误；成功整段bytes收集到Vec，不设应用层响应大小上限、不验证内容类型或独立签名；网络错误传播，客户端构造失败expect panic。

#### Scenario: Redirect response
- **WHEN** 服务返回302及Location
- **THEN** 返回RequestFailed 302，不向重定向目标发送凭据。

证据：`crates/codegen/shell/src/remote/client.rs` — `fn bundle_http_client`；`crates/codegen/shell/src/remote/client.rs` — `fn apply_deployment_key`；`crates/codegen/shell/src/remote/client.rs` — `pub(crate) async fn fetch_bundle`；`crates/codegen/shell/src/remote/client.rs` — `async fn bundle_fetch_never_forwards_the_credential_across_a_redirect`；`crates/codegen/shell/src/remote/client.rs` — `async fn bundle_fetch_does_not_retain_an_untrusted_error_body`。

### Requirement: Shell bundle background helper reachability and gate

后台同步helper SHALL 先检查捕获凭据、非force的一小时缓存，再以每个MvpAgent的AtomicBool拒绝并发启动；任务await正常返回后才清除gate，仅成功执行同步才向启动时快照的session sender广播，发送失败忽略。gate不是跨agent/进程锁，不覆盖显式sync，也没有取消/panic Drop复位。当前工作树全仓库文本引用核对仅有定义和注释，未发现生产调用，不能将其表述为已接通的启动或重连自动同步。

#### Scenario: Helper returns failed sync
- **WHEN** helper已进入任务且同步返回Err
- **THEN** 清除gate并记录warning，不广播刷新。

证据：`crates/codegen/shell/src/agent/mvp_agent/mod.rs` — `pub(crate) fn maybe_sync_bundle_in_background`；`crates/codegen/shell/src/agent/mvp_agent/agent_ops.rs` — `crate::remote::BundleServiceCredential::from_environment()`。

### Requirement: Shell explicit skill baseline refresh admission

grow/skills/refresh-baseline SHALL 快照当前session sender并逐个发送RefreshSkillBaseline，忽略发送失败后立即返回ok true，不等待会话刷新完成。actor接收后登记activity并spawn_local，按session cwd、重新加载skills配置及当前plugin registry列出skills；更新bridge baseline后尝试应用pending effects，无SkillManager时bridge不更新。该分支没有subagent跳过条件，不等价于重建整个MvpAgent或刷新subagent定义。

#### Scenario: Closed session receiver
- **WHEN** 广播包含已关闭接收端
- **THEN** 其他sender仍收到消息，扩展响应仍为ok true。

证据：`crates/codegen/shell/src/agent/mvp_agent/mod.rs` — `pub(super) fn broadcast_refresh_skill_baseline`；`crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs` — `"grow/skills/refresh-baseline"`；`crates/codegen/shell/src/session/actor/run_loop.rs` — `SessionCommand::RefreshSkillBaseline =>`；`crates/codegen/tools/src/bridge.rs` — `pub async fn update_skill_baseline`；`crates/codegen/shell/src/agent/mvp_agent/tests.rs` — `async fn broadcast_refresh_skill_baseline_tolerates_dropped_receiver`。


### Requirement: Shell agent bootstrap ordering and process initialization
bootstrap SHALL 克隆输入配置，仅以remote_settings.path_not_found_hints的Some值覆写同名字段，然后validate_model_filters，初始化进程单例，再构造ModelsManager；任一步返回错误不回滚已完成初始化。init_process通过进程Once仅首次设置日志版本、记录资源上限和提取builtin文件。exit_on_config_error恢复原生stderr、输出Configuration error并退出1。

#### Scenario: Model manager construction fails
- **WHEN** filters校验通过但ModelsManager::from_config失败
- **THEN** bootstrap返回错误，已执行的Once初始化不会回滚或下次重复。

源码证据：
- `crates/codegen/shell/src/agent/init.rs` — `pub fn bootstrap`。
- `crates/codegen/shell/src/agent/init.rs` — `fn resolve_config`。
- `crates/codegen/shell/src/agent/init.rs` — `fn init_process`。
- `crates/codegen/shell/src/agent/init.rs` — `pub(crate) fn exit_on_config_error`。
### Requirement: Pager bundle response schemas

pager SHALL 将bundle status解析为camelCase的hasCache、可选version、agents及skills，其中skills缺失默认为空；status及entry get DTO均拒绝任何未知字段。entry get要求kind、name、content三个字符串。BundleState把version收敛为非可选String并以has_cache false、空version及空目录为默认，但本文件不定义从响应到state的转换。

#### Scenario: Legacy bundle field
- **WHEN** bundle status响应包含未声明legacy字段
- **THEN** 反序列化失败，不静默忽略。

#### Scenario: Missing skills catalog
- **WHEN** bundle status省略skills但其余必填字段存在
- **THEN** 解析成功且skills为空数组。

源码证据：`crates/codegen/pager/src/app/bundle.rs` — `BundleState / BundleStatusResult / EntryGetResult`。

### Requirement: Shell crates/codegen/shell/src/plugin.rs shell module boundary contract

crates/codegen/shell/src/plugin.rs SHALL 维护 shell module boundary 的入口 save_registry_or_warn, InstallOutcome, install_source_is_local, install_plugin, UninstallOutcome, UninstallError, fmt, uninstall_plugin, RepoUpdateOutcome, UpdateError, PluginUpdateSelector, repo_update_requires_reload, apply_update_to_registry, MarketplaceSourceRoot, update_marketplace_repo, marketplace_root_for_provenance, configured_marketplace_git_source, update_plugins (plus 75 additional private symbols)。实现显示该边界包含 filesystem or durable record I/O、explicit error/result paths、platform or feature-gated branches、session/timeline state projection、git/worktree context；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** save_registry_or_warn, InstallOutcome, install_source_is_local, install_plugin, UninstallOutcome, UninstallError, fmt, uninstall_plugin, RepoUpdateOutcome, UpdateError, PluginUpdateSelector, repo_update_requires_reload, apply_update_to_registry, MarketplaceSourceRoot, update_marketplace_repo, marketplace_root_for_provenance, configured_marketplace_git_source, update_plugins (plus 75 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Durable boundary
- **WHEN** save_registry_or_warn, InstallOutcome, install_source_is_local, install_plugin, UninstallOutcome, UninstallError, fmt, uninstall_plugin, RepoUpdateOutcome, UpdateError, PluginUpdateSelector, repo_update_requires_reload, apply_update_to_registry, MarketplaceSourceRoot, update_marketplace_repo, marketplace_root_for_provenance, configured_marketplace_git_source, update_plugins (plus 75 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/plugin.rs`。
