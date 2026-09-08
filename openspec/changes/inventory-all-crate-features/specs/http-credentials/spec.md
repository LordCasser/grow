## ADDED Requirements

### Requirement: Credential provider seam
HTTP 认证 SHALL 通过 HttpAuth::apply 和 AuthCredentialProvider::snapshot 提供请求应用与凭据快照；StaticAuthCredentialProvider 将 apply 委托给 inner，并返回构造时传入的可选 bearer。

#### Scenario: 无 key provider
- **WHEN** 构造 provider 的 bearer 为 None
- **THEN** snapshot.token 为 None；具体端点头由 inner.apply 决定。

#### Scenario: 调试输出
- **WHEN** 格式化 StaticAuthCredentialProvider 的 Debug
- **THEN** 只输出 has_bearer，不输出 bearer 内容。

证据：`crates/codegen/auth/src/auth_provider.rs` — `StaticAuthCredentialProvider`；`crates/codegen/auth/src/visibility.rs` — `HttpAuth`。

### Requirement: Optional bearer middleware
启用 auth 的 middleware feature 后，AuthHeaderMiddleware SHALL 在每次请求读取当前 snapshot；合法 token 写入 Authorization，随后调用 next，不在本层重试 401。

#### Scenario: 服务端返回 401
- **WHEN** 带合法 token 的请求返回 401
- **THEN** 返回该响应，测试观察到一次请求。

#### Scenario: keyless 或非法头
- **WHEN** token 为 None 或包含无法构造 HeaderValue 的内容
- **THEN** None 不额外写头；非法值记录警告后继续 next，不凭空生成新 token。

证据：`crates/codegen/auth/src/header_middleware.rs` — `handle`；`crates/codegen/auth/Cargo.toml` — `middleware`。

### Requirement: Origin identity and User Agent
HTTP identity SHALL 支持环境、session meta 和 ClientType 来源；meta 优先 clientIdentifier 后 clientType，合并时保留 primary product 并仅回填缺失 version。

#### Scenario: 身份展示
- **WHEN** origin 与 grow-shell 产品及版本完全相同
- **THEN** User Agent 折叠重复部分；否则保留 origin 与 agent 两部分及 OS/arch。

#### Scenario: 进程锁存
- **WHEN** set_client_name 被重复调用
- **THEN** 第二次设置 panic；headless mode 设置可重复且只向 headless 单向锁存，未设置默认为 interactive。

证据：`crates/codegen/grow-http/src/lib.rs` — `merge_origin_client_info`；`crates/codegen/grow-http/src/lib.rs` — `process_user_agent_string`；`crates/codegen/grow-http/src/lib.rs` — `set_process_client_mode_headless`。

### Requirement: Shared non sampling HTTP clients
非采样 HTTP SHALL 复用 OnceLock 客户端；async 客户端设置 30 秒连接超时及连接池/keepalive，startup blocking 客户端设置 5 秒连接和请求总超时。

#### Scenario: 复用连接
- **WHEN** 重复调用 shared_client 或 shared_startup_blocking_client
- **THEN** 返回缓存客户端 clone，不重复创建 TLS client；async 调用方仍需自行设置请求总超时。

#### Scenario: 认证包装
- **WHEN** 通过 with_auth_header 包装客户端
- **THEN** 安装 auth::AuthHeaderMiddleware，凭据策略遵循认证 provider。

证据：`crates/codegen/grow-http/src/lib.rs` — `shared_client`；`crates/codegen/grow-http/src/lib.rs` — `shared_startup_blocking_client`；`crates/codegen/grow-http/src/lib.rs` — `with_auth_header`。

### Requirement: Bounded pool escape retry
send_with_retry_escaping_pool SHALL 至少执行一次 op，并在多次尝试的最后一次使用新建无 idle pool 的 HTTP/1.1 client；是否重试由调用方判断，backoff 在第 N 次重试前等待。

#### Scenario: 不可重试错误
- **WHEN** op 返回错误且 is_retryable=false
- **THEN** 立刻返回错误，不等待后续尝试。

#### Scenario: 池逃逸失败
- **WHEN** 最后一次无法构建 fresh client
- **THEN** 记录警告并用 pooled client 完成最后尝试；op 包含的 send/body/decode 全部位于重试单元内。

证据：`crates/codegen/grow-http/src/lib.rs` — `send_with_retry_escaping_pool`。

### Requirement: Transport errors and startup budget constants
TransportFailure SHALL 先判断 connect 错误为 Unreachable，再将 timeout/request/body 判为 Interrupted，其余判 Permanent，并保留完整 source 链。

#### Scenario: 嵌套 OS 错误
- **WHEN** source 链含 io::Error
- **THEN** 优先读取 raw_os_error，必要时解析其显示文本中的 os error 后缀；未找到则 None。

#### Scenario: 启动预算配置
- **WHEN** 调用方使用本 crate 暴露的预算常量
- **THEN** fetch/refresh 为 5 秒、auth 为 60 秒、reapply 为 30 秒及 3 次尝试、minimum connect 为 120 秒；常量本身不执行请求或认证刷新。

证据：`crates/codegen/grow-http/src/lib.rs` — `TransportFailure`；`crates/codegen/grow-http/src/lib.rs` — `find_os_error_code`；`crates/codegen/grow-http/src/lib.rs` — `SETTINGS_REAPPLY_TIMEOUT`。

### Requirement: Optional service endpoint predicates
is_cli_chat_proxy_url SHALL 接受与显式 GROW_CLI_CHAT_PROXY_BASE_URL 同 scheme/host/有效端口且路径相等或下一级斜杠前缀的 URL，另有 localhost/127.0.0.1/::1 host 字符串快捷判断。

#### Scenario: 路径范围
- **WHEN** 配置路径为 /v1
- **THEN** /v1 和 /v1/chat 匹配，/v11 不匹配；按实际路径拼接规则处理，配置末尾斜杠不被额外去除。

#### Scenario: bearer 限制
- **WHEN** is_service_api_bearer_url 判断候选
- **THEN** 先要求 https 并拒绝 localhost 及 IPv4/IPv6 loopback，再使用上述端点规则；is_service_api_url 不添加该 https/loopback 限制。这些函数只判定，不发送或注入凭据。

证据：`crates/codegen/shell-base/src/util/mod.rs` — `matches_trusted_base_url`；`crates/codegen/shell-base/src/util/mod.rs` — `is_service_api_bearer_url`；`crates/codegen/shell-base/src/util/mod.rs` — `is_loopback_host`。

### Requirement: Owner restricted file writes
write_secure_file SHALL 创建父目录、以 create/truncate 写入文件、flush 后调用权限收紧；Unix 新建 mode=0600，现有路径写后按权限检查收紧，Windows 写后设置仅当前用户的 protected DACL。

#### Scenario: 单独 open
- **WHEN** 调用 open_secure_file
- **THEN** 只负责打开和新建权限，现有路径不会因 mode 自动收紧；无原子 rename、fsync 或 symlink 拒绝保证，不将其描述为加密存储。

#### Scenario: 权限错误
- **WHEN** ensure_owner_only_permissions 目标不存在或权限操作失败
- **THEN** NotFound 忽略，其他错误返回；Unix 低九位已经 0600 时跳过 chmod，否则设 0600，其他非 Unix/Windows 平台无操作。

证据：`crates/codegen/shell-base/src/util/secure_file.rs` — `write_secure_file`；`crates/codegen/shell-base/src/util/secure_file.rs` — `open_secure_file`；`crates/codegen/shell-base/src/util/secure_file.rs` — `ensure_owner_only_permissions_inner`。


### Requirement: Shell bundle credential authority capture

BundleServiceCredential SHALL 仅从显式GROW_BUNDLE_SERVICE_BASE_URL及GROW_DEPLOYMENT_KEY构造，环境值trim且非空；不回退chat proxy。地址要求HTTPS及host，无用户名密码queryfragment，保留base路径编码并追加bundle/archive。值持有地址和trim后的key，clone不重新读取环境；Debug显示地址与has_deployment_key，不打印key。

#### Scenario: Environment changes after capture
- **WHEN** 已构造并clone凭据后环境base地址改变
- **THEN** 现有值仍绑定原地址，后续重新构造才采用新地址。

证据：`crates/codegen/shell/src/remote/client.rs` — `impl BundleServiceCredential`；`crates/codegen/shell/src/remote/client.rs` — `fn env_string`；`crates/codegen/shell/src/remote/client.rs` — `impl std::fmt::Debug`；`crates/codegen/shell/src/remote/client.rs` — `fn captured_and_background_cloned_capabilities_do_not_drift_on_environment_change`。


### Requirement: Shell auth provider reference persistence and shared slots
AuthProviderRef序列化 SHALL 仅保存name和非默认fail_closed，不保存config、slot或token；普通反序列化产生unresolved独立空slot，attach_trusted_config才加入按name共享的进程缓存，None附加空配置。fail_closed拒绝重新attach。相等比较仅name/config，不比较resolved/fail_closed/slot；Debug包含config，不能视为命令脱敏。slot映射无驱逐，历史已附加name保留。

#### Scenario: Deserialized provider
- **WHEN** 普通ref从字节恢复但未重新attach
- **THEN** 缓存读取和mint不可用，不继承共享slot。

源码证据：
- `crates/codegen/shell/src/auth/auth_provider.rs` — `pub struct AuthProviderRef`。
- `crates/codegen/shell/src/auth/auth_provider.rs` — `pub(crate) fn attach_trusted_config`。
- `crates/codegen/shell/src/auth/auth_provider.rs` — `impl PartialEq for AuthProviderRef`。
- `crates/codegen/shell/src/auth/auth_provider.rs` — `fn provider_slot`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn attach_trusted_config_lets_a_revived_ref_mint`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn deserialized_ref_never_drops_the_shared_token`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_ref_serializes_name_only_and_drops_config`。

### Requirement: Shell auth provider cache identity and refresh outcomes
缓存 SHALL 按provider name共享异步锁并在mint期间持锁；token identity包含command/args/token_ttl_secs/cwd原始值，不含timeout_secs。到期前60秒即stale；cached_token只try_lock，忙、unresolved、不可用或stale均None；bearer_resolver每次读取该缓存，不回退chat-state旧key。ensure_fresh_token对新鲜同key返回Unchanged、不同key返回Rotated，冷或stale缓存mint；失败返回MintFailed且保留原slot，失败waiter不共享失败结果。不可用配置经locked_slot清掉共享缓存。

#### Scenario: Only timeout changes
- **WHEN** 缓存仍在有效期内且仅修改timeout_secs
- **THEN** token identity不变，继续使用新鲜缓存。

源码证据：
- `crates/codegen/shell/src/auth/auth_provider.rs` — `fn token_identity`。
- `crates/codegen/shell/src/auth/auth_provider.rs` — `fn minted_token_is_stale`。
- `crates/codegen/shell/src/auth/auth_provider.rs` — `pub(crate) fn cached_token`。
- `crates/codegen/shell/src/auth/auth_provider.rs` — `pub(crate) fn bearer_resolver`。
- `crates/codegen/shell/src/auth/auth_provider.rs` — `pub(crate) async fn ensure_fresh_token`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_token_is_cached_while_fresh`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_token_reminted_when_expired`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_pre_turn_refresh_semantics`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_removed_from_config_drops_cached_token`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_config_edit_invalidates_cached_token`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_timeout_edit_does_not_invalidate_token`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_cwd_edit_invalidates_cached_token`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_concurrent_mints_single_flight`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn failed_pre_turn_mint_does_not_serve_the_stale_token`。

### Requirement: Shell auth provider rejected token recovery
401恢复 SHALL 优先采用不同于rejected_key且新鲜的缓存；同一key、相同token identity且mint不足30秒返回None而不重跑，identity修改绕过此guard。其他情况以mark_expired=true重mint；失败将现有slot expires_at设为当前时间，保留值仅作后续helper上下文；成功替换slot并返回token，不保证新token字符串与被拒绝值不同。

#### Scenario: Recovery mint fails
- **WHEN** 服务器拒绝缓存token且helper重取失败
- **THEN** 缓存被标记过期，cached_token不再返回该值。

源码证据：
- `crates/codegen/shell/src/auth/auth_provider.rs` — `pub(crate) async fn recover_rejected_token`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_401_recovery_has_fresh_mint_guard`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_401_recovery_reminted_under_edited_config`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn failed_401_remint_invalidates_the_cached_token`。

### Requirement: Shell auth helper execution capture and mint context
mint SHALL 将timeout默认30秒并clamp到1..600；args为Some即直接exec trim后的command，否则平台shell执行原command。cwd trim非空后展开home，多组件相对program按cwd拼接。helper继承环境但最后移除FIRST_PARTY_CREDENTIAL_ENV_VARS；有旧token时设置上下文token/可用expiry，mark_expired为true设置GROW_AUTH_EXPIRED=1，false时不显式移除继承同名变量。spawn后必须group创建/attach/resume成功才capture，guard仅在resume成功后创建；capture的timeout覆盖并发读两流再wait，不覆盖spawn/enrollment。stdout保存1MiB+1以检测超限，stderr64KiB且错误仅debug，超限继续排空后报错；未完成guard Drop请求group kill，正常capture完成先disarm再检查stdout大小。解析token后expiry优先parsed、配置TTL、JWT。

#### Scenario: Helper holds inherited pipes open
- **WHEN** 直接child已退出但后代保持stdout或stderr未关闭
- **THEN** capture仍等待管道，可能达到timeout并杀group，而不是使用通用runner的退出后2秒排空。

源码证据：
- `crates/codegen/shell/src/auth/auth_provider.rs` — `async fn run_capped`。
- `crates/codegen/shell/src/auth/auth_provider.rs` — `struct AuthHelperProcessGuard`。
- `crates/codegen/shell/src/auth/auth_provider.rs` — `async fn mint_provider_token`。
- `crates/codegen/shell/src/auth/auth_provider.rs` — `fn scrub_first_party_credentials`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_refresh_sets_expired_env`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_expiry_source_precedence`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_unusable_expiry_still_mints`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_args_run_without_a_shell`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_command_times_out`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn dropping_helper_future_kills_its_process_group`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_zero_timeout_clamps_to_one_second`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn mint_error_messages_distinguish_failure_modes`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn re_mint_hands_the_prior_token_back_to_the_command`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_output_over_cap_fails_closed`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_helper_env_scrubs_first_party_credentials`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `fn resolve_program_resolves_against_cwd`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_resolves_relative_program_against_cwd`。
- `crates/codegen/shell/src/auth/auth_provider_tests.rs` — `async fn provider_command_runs_in_cwd`。

### Requirement: Shell credential helper token output acceptance
parse_token_output SHALL 先拒绝非成功exit，再要求stdout为UTF-8并trim非空；首字符为{时必须反序列化为含String access_token及可选u64 expires_in的JSON，token再trim非空；未知JSON字段默认容忍。其他开头整个trim字符串作为bare token，不尝试解析数组、JSON字符串或错误文字。两条路径均在trim后拒绝内部Unicode控制字符，但不验证完整HTTP header合法性。stderr不参与本解析器判定。

#### Scenario: JSON error object
- **WHEN** 成功exit且stdout为仅含error的JSON对象
- **THEN** 因缺少access_token返回错误，不作为bare bearer。

源码证据：
- `crates/codegen/shell/src/auth/token_output.rs` — `pub(crate) fn parse_token_output`。
- `crates/codegen/shell/src/auth/token_output.rs` — `fn reject_control_chars`。
- `crates/codegen/shell/src/auth/token_output.rs` — `fn parse_token_output_rejects_invalid_json_payloads`。


### Requirement: Shell credential helper expiry hint parsing
expiry_after_seconds SHALL 对u64转i64、Duration构造或日期相加溢出返回None，零秒生成当前时间；JSON expires_in经此转换而bare token解析本身不推算到期。jwt_expiry要求恰好三个点分段，仅以URL_SAFE_NO_PAD解码第二段为JSON并提取i64 exp秒时间；不校验header、签名、issuer、audience或token有效性，不要求exp为未来。此结果只供缓存到期提示。

#### Scenario: Overflow lifetime
- **WHEN** JSON expires_in可反序列化但时间计算溢出
- **THEN** token解析成功且expires_at=None，上层可继续采用其他到期来源。

源码证据：
- `crates/codegen/shell/src/auth/token_output.rs` — `pub(crate) fn expiry_after_seconds`。
- `crates/codegen/shell/src/auth/token_output.rs` — `pub(crate) fn jwt_expiry`。
- `crates/codegen/shell/src/auth/token_output.rs` — `fn expiry_after_seconds_returns_none_on_overflow`。
- `crates/codegen/shell/src/auth/token_output.rs` — `fn jwt_expiry_reads_only_valid_compact_numeric_claims`。


### Requirement: Shell auth provider tolerant config parsing and model attachment
auth_provider解析 SHALL 对整节非table生成warning并忽略，对单项反序列化失败warning且跳过；未知字段warning但保留可解析项。空command、TTL不大于60秒、timeout不在1..600只生成问题报告，不在该解析器删除项或修正数值。new_from_toml_cfg先解析auth_provider，再将可变provider映射交给provider catalog解析。resolve_model_list仅遍历cfg.config_models构造条目，再按name从cfg.auth_providers附加配置；缺失附加空配置，fail_closed引用保持封闭。这些函数本身不检查folder trust，输入配置的可信来源必须由调用方保证。

#### Scenario: Parse warning preserves provider
- **WHEN** provider可反序列化但timeout_secs=0
- **THEN** 保留原配置并报告warning，实际mint再clamp到1秒。

源码证据：
- `crates/codegen/shell/src/agent/config.rs` — `fn parse_auth_providers`。
- `crates/codegen/shell/src/agent/config.rs` — `pub fn new_from_toml_cfg`。
- `crates/codegen/shell/src/agent/config.rs` — `pub fn resolve_model_list`。
- `crates/codegen/shell/src/agent/provider_catalog.rs` — `pub(crate) fn auth_config_issues`。


### Requirement: Shell catalog credential resolution and auth fact states
resolve_credentials SHALL 优先model.own_credential，其次auth_provider缓存，否则无key；不在此执行helper，保持模型base_url与auth_scheme。first_own_credential只用trim判断api_key非空，返回原字符串；无可用api_key才解析EnvKeys。try_resolve_model_credentials重新加载effective配置、解析并按精确catalog id查找，失败None。auth facts对空id或配置不可用为Unknown，catalog存在为Byok（包括无key本地端点），不存在为NotByok；返回effective_auth_provider，不在该函数内实现memo。 这两个按id查询入口的effective加载链为shell campaigns→ConfigLayers::load→load_from_disk：仅用户grow_home/config.toml及版本/本地campaign覆盖，无cwd回退或项目祖先发现；不应描述为先读取项目auth_provider再按trust过滤。此结论限定于这些查询入口，不涵盖接受外部Config的所有构造路径。

#### Scenario: Keyless configured model
- **WHEN** catalog中存在无key模型
- **THEN** auth facts为Byok；BYOK描述端点归属而非secret存在。

源码证据：
- `crates/codegen/shell/src/agent/config.rs` — `pub fn resolve_credentials`。
- `crates/codegen/shell/src/agent/config.rs` — `pub(crate) fn first_own_credential`。
- `crates/codegen/shell/src/agent/config.rs` — `pub fn resolve_model_auth_facts_and_provider`。
- `crates/codegen/shell/src/agent/config.rs` — `fn byok_from_lookup`。

加载链源码证据：
- `crates/codegen/shell/src/util/config/campaigns.rs` — `pub fn load_effective_config`。
- `crates/codegen/config/src/loader.rs` — `pub fn load_from_disk`。
- `crates/codegen/config/src/loader.rs` — `pub fn effective_config_base`。

### Requirement: Shell auxiliary model cached credential admission
resolve_aux_model_sampling_config SHALL 拒绝trim为空的id，按catalog精确查找并构造sampler，仅api_key为Some才返回；有effective_auth_provider但缓存无key时warn并None，不在此mint。无helper的keyless模型同样最终None，不能据主模型可keyless推定辅助模型可用。stamp_session_local_sampler_fields总是复制attribution_callback和指定max_retries，仅目标base_url通过is_service_api_bearer_url时复制active bearer_resolver，否则保留cfg原resolver。

#### Scenario: Keyless auxiliary model
- **WHEN** catalog存在但resolve_credentials返回api_key=None
- **THEN** 辅助配置解析返回None，fallback或失败由caller处理。

源码证据：
- `crates/codegen/shell/src/agent/config.rs` — `pub fn resolve_aux_model_sampling_config`。
- `crates/codegen/shell/src/agent/config.rs` — `pub fn stamp_session_local_sampler_fields`。


### Requirement: Shell session provider refresh and auth memo boundaries
model_auth_state SHALL 对同catalog id且非Unknown的memo直接复用，否则查询配置；fresh Unknown有同id旧memo时返回旧值，无旧值则返回Unknown且不缓存，新确定结果写memo。invalidate清空memo。预刷新仅Rotated尝试写chat-state key，读取credentials后核对expected route revision，不符则丢弃；Unchanged/Unusable/MintFailed不写，失败记录日志。401恢复使用当前chat-state key，无key则ensure，成功写回不传revision；此局部函数本身不约束上层重试次数。 refresh_byok_credential先快照route、读取当前credentials并重验revision；有provider走预刷新，否则重读配置，只有非空结果且与当前key不同才按revision写回。错误恢复在前序rate-limit等分支之后，将status=401或Auth kind视认证失败；有provider且恢复成功才prepare_sampler_for_turn并返回RefreshByokAndResubmit。无provider且错误credential为missing才尝试普通BYOK重读；provider恢复失败不会落入这个无provider分支。 普通key重读还要求配置解析所得base_url和auth_scheme与route快照精确相同，否则推迟到完整route应用；解析无key不清空chat-state旧key。该刷新函数只更新key，不更新endpoint或auth_scheme。

#### Scenario: Route changes during refresh
- **WHEN** helper完成但写回前发现route revision已变
- **THEN** 预刷新返回false且不更新chat-state key。

源码证据：
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `fn model_auth_state`。
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `pub(crate) fn invalidate_model_auth_memo`。
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `async fn refresh_provider_token_pre_turn`。
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `async fn try_provider_401_recovery`。
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `pub(in crate::session::actor) async fn set_chat_api_key`。

- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `pub(crate) async fn refresh_byok_credential`。

### Requirement: Shell session sampler reconstruction and bearer resolver precedence
reconstruct_full_config SHALL 循环读取route快照及chat-state采样配置/credentials，只有复核revision相同才组合；缺采样配置用空base_url/model及256000context默认。auth_scheme与首选bearer_resolver来自route，其他核心采样字段来自chat-state；route resolver缺失才使用catalog provider.bearer_resolver，因此动态请求可读取共享token缓存。max_retries来自actor，stream_tool_calls缺省false，attribution=None。URL派生header后根据actor compaction配置及有无last compaction prompt索引添加x-compactions-remaining/x-compaction-at；后者仅无summary时添加。这次summary异步读取在最初revision循环之后，不属于该循环保护。

#### Scenario: Route resolver exists
- **WHEN** route已有resolver且catalog也有auth provider
- **THEN** 保留route resolver，不以provider resolver覆盖。

源码证据：
- `crates/codegen/shell/src/session/actor/turn/sampling.rs` — `pub(in crate::session::actor) async fn reconstruct_full_config`。


- `crates/codegen/shell/src/session/actor/tests/rewind_cross_compaction_tests.rs` — `async fn rewind_before_compaction_clears_stale_compaction_marker`。
- `crates/codegen/shell/src/session/actor/tests/rewind_cross_compaction_tests.rs` — `async fn run_clears_marker_scenario`。

### Requirement: Shell recovered authentication retry accounting
AuthRetrySchedule SHALL 在恢复成功返回重提结果后按实际SentCredential计数：Missing独立累计前50次UnchargedResubmit，第51次RunawayGuard；Sent与Unknown共用3次拒绝预算，依次退避1/2/4秒，随后Exhausted。每次先检测SystemTime相对Instant多出至少30秒的漂移，最多8次同时重置两种计数；显式reset则重建全部状态含重置额度。turn循环对Missing睡100ms，对Backoff发送Retrying并睡指定时长，两个终止结果返回带401的internal error；预算检查在helper恢复及sampler准备之后，不限制此前helper执行次数。 turn内创建一次schedule；model_changed处理、首次CompactAndResubmit、ImageInputUnsupportedAndResubmit、成功ResetContinuationAndResubmit、Steered以及取得正常Response后显式reset。仅agent_changed的该分支不重置认证预算；预算不是整个turn累计上限。

#### Scenario: Missing then rejected
- **WHEN** 先一次Missing恢复再一次Sent恢复
- **THEN** Missing不消耗拒绝预算，Sent仍为第1次并退避1秒。

源码证据（测试未执行）：
- `crates/codegen/shell/src/session/actor/auth_retry.rs` — `pub(super) fn on_recovered_401`。
- `crates/codegen/shell/src/session/actor/auth_retry.rs` — `fn reset_after_suspend`。
- `crates/codegen/shell/src/session/actor/auth_retry.rs` — `fn rejected_credentials_use_exact_bounded_schedule`。
- `crates/codegen/shell/src/session/actor/auth_retry.rs` — `fn missing_credentials_do_not_charge_rejection_budget`。

### Requirement: Shell crates/codegen/shell/src/auth/mod.rs credential and authentication flow contract

crates/codegen/shell/src/auth/mod.rs SHALL 维护 credential and authentication flow 的入口 the file module entrypoint。实现显示该边界包含 platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/auth/mod.rs`。
