# config-types 逐包核查

包路径：`crates/codegen/config-types`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；已执行 cargo test --locked -p config-types --all-features，48 项测试通过；另直接调用编译后真实类型验证 3 项边界。

## 模块与开关

- `crates/codegen/config-types/Cargo.toml`
- `crates/codegen/config-types/src/flags.rs`
- `crates/codegen/config-types/src/lib.rs`
- `crates/codegen/config-types/src/mcp.rs`
- `crates/codegen/config-types/src/memory.rs`
- `crates/codegen/config-types/src/permission.rs`
- `crates/codegen/config-types/src/pool.rs`

Cargo feature：`{"default-bazel": []}`。

## 功能与规范映射

- [Boolean configuration source resolution](../specs/configuration-rules/spec.md#requirement-boolean-configuration-source-resolution)：BoolFlag SHALL 按 cli > config::env_bool(env_var) > config > feature_flag(remote) > default 返回首个 Some 值与 ConfigSource；builder 初始默认 false。
- [Remote settings transport schema](../specs/configuration-rules/spec.md#requirement-remote-settings-transport-schema)：RemoteSettings SHALL 保存当前源码声明的全部可选远程配置字段，使用 snake_case，缺失/null 解析为 None，普通未知顶层字段忽略；None 与 Some(false)/Some(空列表) 保持区别。
- [Doom loop and contextual hint settings](../specs/configuration-rules/spec.md#requirement-doom-loop-and-contextual-hint-settings)：DoomLoopRecoverySettings SHALL 提供默认 None 的 enabled/max_threshold/max_retries 并省略 None；ContextualHintsRemote 提供 undo/plan_mode/image_input/send_now/small_screen/word_select/ssh_wrap 可选 bool。
- [Worktree age and tolerant GC schema](../specs/configuration-rules/spec.md#requirement-worktree-age-and-tolerant-gc-schema)：WorktreeKindMaxAge SHALL 接受非负整数秒、可解析 u64 的字符串、不区分大小写 never，以及 null/unit 表示 Never；序列化为整数或 never。WorktreeAutoGcSettings 提供 enabled/max_age_secs/min_interval_secs/dry_run/include_orphan_snapshots/max_age_by_kind/include_rebuild/rebuild_min_interval_secs。
- [Display refresh preservation and tag fallback](../specs/configuration-rules/spec.md#requirement-display-refresh-preservation-and-tag-fallback)：DisplayRefreshSettings SHALL 提供可选 probe_enabled/auto_cadence_enabled/floor_ms/ceiling_ms/min_hz/max_hz，将未知键保留在 extra，只有所有字段 None 且 extra 空时 is_default=true。
- [Permission rule configuration shape](../specs/configuration-rules/spec.md#requirement-permission-rule-configuration-shape)：PermissionConfig SHALL 默认空 rules；PermissionRule 要求 action，tool 默认 Any、pattern 默认 None、pattern_mode 默认 Glob；枚举 wire 使用 lowercase。
- [Worktree pool configuration defaults](../specs/configuration-rules/spec.md#requirement-worktree-pool-configuration-defaults)：PoolConfig SHALL 默认 enabled=true、pool_size=2、file_count_threshold=50000、parallelism=3；空对象与 Default 一致，部分覆盖保留其他默认。
- [Memory index embedding and search defaults](../specs/configuration-rules/spec.md#requirement-memory-index-embedding-and-search-defaults)：Memory 配置 SHALL 默认 index 最大 1600/重叠320 字符，embedding provider=api/model=None/dimensions=1024；search 默认 max_results=6、min_score=.35、vector/text weight=.7/.3，workspace/session/global source_weights 各 1。
- [Memory lifecycle and compaction flush configuration](../specs/configuration-rules/spec.md#requirement-memory-lifecycle-and-compaction-flush-configuration)：Memory 生命周期类型 SHALL 默认 initial_injection enabled=true/min_score=None、session save_on_end=true、dream enabled=true/min_hours=4/min_sessions=3/stale_lock_secs=3600/check_interval_secs=None、watcher enabled=true/stale_claim_secs=60、GC max_age_days=30。
- [MCP transport and operational config values](../specs/configuration-rules/spec.md#requirement-mcp-transport-and-operational-config-values)：McpServerConfig SHALL flatten untagged transport：先尝试含 command 的 Stdio(args 默认空、可选 env/cwd)，再尝试必需 url 的 HTTP(可选 type/bearer_token_env_var/headers)；enabled 默认 true、max_access 默认 ToolAccess::All。
- [MCP setup select and preferences](../specs/configuration-rules/spec.md#requirement-mcp-setup-select-and-preferences)：resolve_setup SHALL 无 setup 时返回 config clone；有 setup 时仅允许恰一个非空 Select，按 stored preferences 的 field.id 查值并要求匹配某个 option，缺失或不匹配返回 Required。
- [MCP template expansion scope](../specs/configuration-rules/spec.md#requirement-mcp-template-expansion-scope)：setup 模板 SHALL 将 {{key}} 的 trim 后 key 查派生变量，只替换一层，未闭合或未解析变量返回 Invalid；成功 clone 清除 setup。expand_strings 接受 caller 替换函数并就地处理相同字符串位置。
- [MCP ACP conversion boundaries](../specs/configuration-rules/spec.md#requirement-mcp-acp-conversion-boundaries)：to_acp_mcp_server SHALL 在 disabled 或 setup 仍存在时返回 None；stdio 转 command PathBuf/args/env，HTTP 携带 headers 并可从指定环境变量追加 Authorization Bearer token。

## 边界

- 采用 false，不因低优先级 true 覆盖；Resolved Display 输出 value (source)，source 为 cli/env/config/remote/default。
- 前者 enabled 默认 None；后者 enabled=false、max_nudges_per_session=0、idle_threshold_ms/min_confidence/include_reasoning=None。本类型不启动索引或 classifier，运行默认与触发条件由 harness 解析。
- 字段全集按本包 review 的 RemoteSettings 字段表核对；这里只反序列化值，不实施注释描述的优先级、开关、model catalog 或阈值 clamp。
- 可导致整个 RemoteSettings 解析失败；只有声明了 tolerant deserializer 的嵌套项有特别降级，不能概括为所有远程错误都忽略。
- 缺失项 None，未知键忽略；已知项错误类型仍失败。Doom-loop 2..64/0..5 等注释范围不在此类型 clamp，需 resolver 执行。
- 保留可解析条目到 BTreeMap，坏项跳过，null 值为 Never；未知种类名称在此仍保留，不在 DTO 执行 GC。非对象 map 值降为 None。
- 错误标量变 None，但 visitor 未实现 map/seq，数组/对象使 WorktreeAutoGcSettings 解析失败；RemoteSettings 的外层 tolerant wrapper 再警告并丢弃整个 worktree_auto_gc，兄弟 enabled=false 也可能丢失。
- 错误标量丢为 None，负数/超 u32 整数丢为 None；数组/对象仍失败，RemoteSettings.display_refresh 没有外层 tolerant wrapper，可导致整体解析失败。未知 extra 可再次序列化保留。
- 整个 map 被警告并降为 None，而非仅丢弃坏条目；成功时保存 BTreeMap，不在此合并本地配置。
- 失败，不能因 RuleAction::default 为 Deny 就声称省略 action 自动 Deny；显式 action 为 allow/deny/ask。
- 支持 any/bash/edit/read/grep/mcp/webfetch，以及 glob/domain；这里只保存规则，不执行匹配和授权。
- usize 类型接受，未在此 clamp 或创建 worker；是否启用池及容量执行继续在工作树实现核查。
- 拒绝；source_weights 仍为任意字符串 map，不在此校验名称或归一化权重。
- 默认 decay enabled=true/half_life_days=7，effective_half_life_days 在 disabled 或 <=0 时 None（<=0 警告）；MMR 默认 disabled/lambda=.7，反序列化 lambda clamp 到 0..1。公开字段直接构造绕过 clamp，half-life 未显式拒绝 NaN。
- enabled=true、soft_threshold_tokens=4000、flush_model=None、max_flush_write_chars=8000、idle_timeout_secs=None、semantic_dedup_threshold=None；显式 dedup 浮点反序列化 clamp 到 0..1。
- 这里只保存配置，不能证明 watcher 启动、自动保存或定时 consolidation 已执行；watcher 拒绝未知字段，其他这些结构一般忽略未知字段。
- 无 transport 反序列化失败，即使 enabled=false；空白字符串可解析，blank_transport_field 以 trim 检出，但须 caller 显式使用。
- 保存可选值；本层不执行超时、输出策略或 ToolAccess 权限。KNOWN_MCP_SERVER_FIELDS 提供已知键表，problem DTO 用 camelCase 并区分 error/warning。
- default/required 不绕过偏好要求；derived.from 必须等于唯一 field.id，否则 Invalid，选值无 map 返回 Required。成功只把派生变量注入模板，不自动注入原 field 值。
- version=1、servers 空；serde 文件 version 必需，类型不做版本兼容验证或持久化。每 server 有 values、可选 source(kind/plugin/scope)/updatedAt。
- stdio 替换 command、args、env 值、cwd；HTTP 替换 url 和 headers 值。不替换 map 键、bearer_token_env_var、transport_type 或 setup schema，不在此执行 shell escaping。
- 转 Sse，否则 Http；只拒绝空 URL，不拒绝纯空白，也不验证 URI/header。缺 bearer 环境变量仅警告继续，既有 Authorization 可能与追加项并存。
- 这些值未进入返回的 ACP server，须通过其他 caller 通路处理；不证明工作目录、权限或超时已传播。McpConfig 以 mcpServers 的 IndexMap 保存声明顺序。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## RemoteSettings 字段全集

以下逐字段从已阅读的 RemoteSettings 声明核对。所有条目映射到 Remote settings transport schema；类型默认仅为 None，运行时默认和优先级必须在 resolver 所属包继续检查。

| 字段 | Rust 类型 |
| --- | --- |
| `leader_mode` | `Option<bool>` |
| `non_git_workspace_capture` | `Option<bool>` |
| `login_shell_capture` | `Option<bool>` |
| `release_channel` | `Option<String>` |
| `memory_enabled` | `Option<bool>` |
| `memory_search_max_results` | `Option<u32>` |
| `memory_search_min_score` | `Option<f32>` |
| `memory_initial_injection_enabled` | `Option<bool>` |
| `memory_initial_injection_min_score` | `Option<f32>` |
| `memory_embedding_model` | `Option<String>` |
| `memory_embedding_dimensions` | `Option<u32>` |
| `flush_enabled` | `Option<bool>` |
| `flush_soft_threshold_tokens` | `Option<u64>` |
| `flush_idle_timeout_secs` | `Option<u64>` |
| `flush_semantic_dedup_threshold` | `Option<f64>` |
| `memory_temporal_decay_enabled` | `Option<bool>` |
| `memory_temporal_decay_half_life_days` | `Option<f64>` |
| `memory_mmr_enabled` | `Option<bool>` |
| `memory_mmr_lambda` | `Option<f64>` |
| `memory_watcher_enabled` | `Option<bool>` |
| `dream_enabled` | `Option<bool>` |
| `dream_min_hours` | `Option<u64>` |
| `dream_min_sessions` | `Option<u64>` |
| `dream_check_interval_secs` | `Option<u64>` |
| `lsp_tools_enabled` | `Option<bool>` |
| `folder_trust_enabled` | `Option<bool>` |
| `write_file_enabled` | `Option<bool>` |
| `file_toolset` | `Option<String>` |
| `inference_idle_timeout_secs` | `Option<u64>` |
| `mcp_startup_timeout_secs` | `Option<u64>` |
| `max_mcp_output_bytes` | `Option<u64>` |
| `doom_loop_recovery` | `Option<DoomLoopRecoverySettings>` |
| `worktree_auto_gc` | `Option<WorktreeAutoGcSettings>` |
| `goal_enabled` | `Option<bool>` |
| `workflows_enabled` | `Option<bool>` |
| `tips` | `Option<Vec<String>>` |
| `slash_command_tags` | `Option<std::collections::BTreeMap<String, String>>` |
| `non_git_warning` | `Option<bool>` |
| `session_title_model` | `Option<String>` |
| `image_description_model` | `Option<String>` |
| `prompt_suggestion_model` | `Option<String>` |
| `default_model` | `Option<String>` |
| `auto_background_on_timeout` | `Option<bool>` |
| `allow_background_operator` | `Option<bool>` |
| `ask_user_question_timeout_enabled` | `Option<bool>` |
| `ask_user_question_timeout_secs` | `Option<u64>` |
| `subagent_worktree_snapshot_enabled` | `Option<bool>` |
| `image_normalize_cache_enabled` | `Option<bool>` |
| `path_not_found_hints` | `Option<bool>` |
| `contextual_hints` | `Option<ContextualHintsRemote>` |
| `worktree_type` | `Option<String>` |
| `restore_code` | `Option<bool>` |
| `cancel_rewind_enabled` | `Option<bool>` |
| `session_recap` | `Option<bool>` |
| `ask_user_question_enabled` | `Option<bool>` |
| `web_fetch_enabled` | `Option<bool>` |
| `web_fetch_proxy` | `Option<String>` |
| `web_fetch_allowed_domains` | `Option<Vec<String>>` |
| `show_resolved_model` | `Option<bool>` |
| `zdr_access_enabled` | `Option<bool>` |
| `remember_tool_approvals` | `Option<bool>` |
| `crash_handler_enabled` | `Option<bool>` |
| `show_thinking_blocks` | `Option<bool>` |
| `group_tool_verbs` | `Option<bool>` |
| `display_refresh` | `Option<DisplayRefreshSettings>` |
| `auto_mode` | `Option<serde_json::Value>` |
| `permission_mode` | `Option<String>` |
| `session_picker_grouped` | `Option<bool>` |
| `suggestions_enabled` | `Option<bool>` |
| `suggestions_ai_enabled` | `Option<bool>` |
| `auto_compact_threshold_percent` | `Option<u8>` |
| `subagents_max_depth` | `Option<u32>` |
| `system_prompt_label` | `Option<String>` |
| `compaction_wall_clock_budget_secs` | `Option<u64>` |
| `compaction_verbatim_input` | `Option<bool>` |
| `compaction_pre_prune` | `Option<bool>` |
| `compaction_pre_prune_token_budget` | `Option<u64>` |

## 验证范围与差异

六个源码模块、manifest 全部阅读，default-bazel 是空 feature。48 项现有测试通过，日志 `/tmp/grow-config-types-tests.log`；未执行任何服务器连接、GC 或领域动作。

临时 Rust 调用已编译 config_types/serde_json rlib，确认三个边界：worktree_auto_gc 中 enabled=false 与 max_age_secs=[] 会令整个嵌套对象降为 None；display_refresh 的 floor_ms=[] 会使整个 RemoteSettings 解析失败；PermissionRule 缺少 action 会失败。复现源码 `/tmp/grow-config-types-probe.rs`，结果 `/tmp/grow-config-types-probe.log`。不按“所有错误类型均忽略”或“省略 action 默认拒绝”的注释扩大实际行为。

MCP to_acp 丢弃 cwd 与本地策略字段不直接证明整条调用链丢失，其他通道是否补传需继续审阅 shell/mcp。Memory 默认 enabled 同样不证明运行态已启动。
