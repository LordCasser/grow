## ADDED Requirements

### Requirement: Typed turn hook exchange
TurnHookRequest SHALL 以 phase 区分 before/after，payload 保留 turn/model 与 before 的消息数/关系/schema 或 after 的 outcome/耗时/调用数/写入路径/可选取消信息；HookReply 支持有序 System/Developer/User 注入和 Auto/ForceContinue/ForceStop 控制。

#### Scenario: 默认 reply
- **WHEN** 反序列化空对象 HookReply
- **THEN** 得到空 injections 与 Auto；未知 reply 字段被拒绝。

#### Scenario: payload 不完整
- **WHEN** before 缺少契约字段或 after 缺 written_repo_paths
- **THEN** 拒绝反序列化；completed 的取消信息可省略，outcome 仅接受 completed/cancelled/error。

证据：`crates/common/tool-protocol/src/turn_hook.rs` — `TurnHookRequest`；`crates/common/tool-protocol/src/turn_hook.rs` — `HookReply`。

### Requirement: Extension DTO wire naming
extension-types SHALL 提供 hooks/plugins/MCP/marketplace 的 serde DTO，普通 struct 使用 camelCase，动作和 origin 使用 type 标记与 snake_case variant，variant 内字段保持 snake_case；本层不执行动作或验证领域身份。

#### Scenario: 请求路由
- **WHEN** 序列化 HooksActionRequest/PluginsActionRequest/MarketplaceActionRequest
- **THEN** 外层 sessionId 与 action 必需，内层 hook_name/plugin_id/source_url_or_path 保持下划线；字符串格式由 caller 验证。

#### Scenario: 结果状态
- **WHEN** 序列化 ActionOutcome
- **THEN** status 为 success/validation_error/confirmation_required/not_found/internal_error/unsupported；message、requiresReload、requiresRestart 为独立必需字段，不由 status 自动推导。

证据：`crates/codegen/extension-types/src/lib.rs` — `HooksActionRequest`；`crates/codegen/extension-types/src/lib.rs` — `PluginsActionRequest`；`crates/codegen/extension-types/src/lib.rs` — `MarketplaceActionRequest`；`crates/codegen/extension-types/src/lib.rs` — `OutcomeStatus`；`crates/codegen/extension-types/src/lib.rs` — `ActionOutcome`。

### Requirement: Hook metadata and action vocabulary
HookInfo SHALL 表达 name、event、handlerType、matcher、command、url、timeoutMs、sourceDir、disabled，HooksListResponse 同时提供 projectTrusted 和可省略的空 loadErrors；disabled 缺失默认 false。

#### Scenario: 事件集合
- **WHEN** 传输 HookEvent
- **THEN** 支持 session_start/session_end/stop/stop_failure/stop_cancelled、pre_tool_use/post_tool_use/post_tool_use_failure/permission_denied、user_prompt_submit/notification、subagent_start/subagent_stop、pre_compact/post_compact；Display 使用人类可读标签，与 wire string 分离。

#### Scenario: 管理动作
- **WHEN** 传输 HooksAction
- **THEN** 支持 reload/trust/untrust、按 path add/remove、按 hook_name enable/disable、按 hook_names 与 disable 的 toggle_source；不在 DTO 验证 command/http 字段互斥或 matcher 可执行性。

证据：`crates/codegen/extension-types/src/lib.rs` — `HookInfo`；`crates/codegen/extension-types/src/lib.rs` — `HooksListResponse`；`crates/codegen/extension-types/src/lib.rs` — `HookEvent`；`crates/codegen/extension-types/src/lib.rs` — `HookHandlerType`；`crates/codegen/extension-types/src/lib.rs` — `HooksAction`。

### Requirement: Plugin identity origin and displayed inventory
PluginInfo SHALL 拒绝未知顶层字段，要求 origin，并携带 id/name/root/scope/enabled、版本描述、skill/agent 名称和数量、hook/MCP 状态数量以及可选 conflict；scope 为 cli/project/user/config。

#### Scenario: 来源变体
- **WHEN** 传输 PluginOrigin
- **THEN** 支持 cli_override/project_grow/user_grow/config_path/marketplace_install；marketplace_install 的 source_name/git_url 可缺失且 None 时省略；未知 origin variant 拒绝。

#### Scenario: 显示默认
- **WHEN** skillNames/agentNames/hookCount/conflict 缺失
- **THEN** 名称列表空、hookCount=0、conflict=None；空名称列表和 None conflict 省略，数量不根据列表重新计算。hookStatus/mcpStatus 为 active/active_inline/blocked/none，DTO 不自行计算信任。

证据：`crates/codegen/extension-types/src/lib.rs` — `PluginInfo`；`crates/codegen/extension-types/src/lib.rs` — `PluginScope`；`crates/codegen/extension-types/src/lib.rs` — `PluginOrigin`；`crates/codegen/extension-types/src/lib.rs` — `HookStatus`；`crates/codegen/extension-types/src/lib.rs` — `McpStatus`；`crates/codegen/extension-types/src/lib.rs` — `PluginsListResponse`。

### Requirement: Plugin action confirmation field
PluginsAction SHALL 表达 reload、install(source)、uninstall(plugin_id,confirmed)、update(可选 plugin_id)、add/remove(path)、enable/disable(plugin_id)。

#### Scenario: 缺失确认
- **WHEN** 反序列化 uninstall 且没有 confirmed
- **THEN** confirmed 默认为 false；字段本身只表达 caller 输入，不等于本层获得授权或执行卸载。

#### Scenario: 全部更新
- **WHEN** update 未指定 plugin_id
- **THEN** 可解析为 None；实际选取与更新由领域 handler 决定。

证据：`crates/codegen/extension-types/src/lib.rs` — `PluginsAction`。

### Requirement: MCP extension list display state
McpServersListResponse SHALL 提供 servers，每项含 name/enabled/toolCount，及可缺失的 status/tools/configSource；status 为 ready/initializing/unavailable，tool 包含 name 和可省略 description。

#### Scenario: 稀疏元数据
- **WHEN** 列表未带状态、工具详情或配置来源
- **THEN** status/configSource 为 None、tools 为空，序列化省略这些空字段；toolCount 不与 tools 长度自动核对，MCP 连接和能力发现不在本包实现。

证据：`crates/codegen/extension-types/src/lib.rs` — `McpSessionStatus`；`crates/codegen/extension-types/src/lib.rs` — `McpToolInfo`；`crates/codegen/extension-types/src/lib.rs` — `McpServerInfo`；`crates/codegen/extension-types/src/lib.rs` — `McpServersListResponse`。

### Requirement: Plugin component categories and summaries
PluginComponents SHALL 按 skills、commands、agents、mcpServers、hooks、lspServers 六类顺序枚举，缺失列表默认空且空列表不序列化；summary_line 只包含非空类并按数量使用英文单复数，以中点连接。

#### Scenario: 空目录
- **WHEN** 六类都为空
- **THEN** is_empty 为 true、summary_line 为 None；Some(empty components) 与没有 catalog inventory 的 None 可以区分。

#### Scenario: 组件内容
- **WHEN** 构造 ComponentItem
- **THEN** 保存 name 与可选 description，hooks 的 name 可表示事件、description 可表示 matcher；DTO 不验证组件在磁盘实际存在或数量一致。

证据：`crates/codegen/extension-types/src/lib.rs` — `PluginComponents`；`crates/codegen/extension-types/src/lib.rs` — `ComponentCategory`；`crates/codegen/extension-types/src/lib.rs` — `categories`；`crates/codegen/extension-types/src/lib.rs` — `summary_line`；`crates/codegen/extension-types/src/lib.rs` — `ComponentItem`。

### Requirement: Explicit catalog component sanitization
ComponentItem::new 与 PluginComponents::sanitize SHALL 去除 is_control 字符及 U+200B..200F、202A..202E、2066..2069、FEFF，将 name/description 各截为最多 120 Unicode scalar 字符，清洗后空 description 变 None；sanitize 每类仅保留前 50 项。

#### Scenario: 反序列化边界
- **WHEN** 从 serde 或 public struct 字段取得 ComponentItem/PluginComponents
- **THEN** 不会自动 sanitize；渲染 consumer 必须在入口显式调用，空 name 不被拒绝，不去重也不 trim 普通空格。

#### Scenario: 整份市场列表
- **WHEN** 调用 MarketplaceListResponse.sanitize
- **THEN** 仅遍历 source.plugins 中 Some(components) 并清洗组件；不清洗 plugin.name/description、source label、URL 等其他字符串，也不限制 source/plugin 数量。

证据：`crates/codegen/extension-types/src/lib.rs` — `ComponentItem`；`crates/codegen/extension-types/src/lib.rs` — `strip_control_chars`；`crates/codegen/extension-types/src/lib.rs` — `truncate_chars`；`crates/codegen/extension-types/src/lib.rs` — `MAX_COMPONENTS_PER_CATEGORY`；`crates/codegen/extension-types/src/lib.rs` — `MarketplaceListResponse`。

### Requirement: Marketplace catalog wire records
MarketplaceListResponse SHALL 提供 sources，每项保存 sourceName、featured、sourceKind、sourceUrlOrPath、plugins 和可选 error；plugin entry 表达名称、版本描述/分类/作者、tags/keywords/domains/homepage、relativePath、installStatus/installedVersion、可选 components 与 remoteUrl/ref/sha/subdir。

#### Scenario: 缺省和开放字段
- **WHEN** 反序列化未含新增可选目录元数据的 entry
- **THEN** tags/keywords/domains 为空、homepage/components/remote 字段为 None、featured 默认为 false；这些 DTO 不拒绝未知字段。installStatus/sourceKind 保持普通字符串，不在本层枚举或验证 SHA。

#### Scenario: 库存可信性
- **WHEN** components 为 Some
- **THEN** 仅说明 wire 携带库存，SHA 验证必须由 producer 执行；本 serde 类型不提供真实性证明。

证据：`crates/codegen/extension-types/src/lib.rs` — `MarketplaceScanResult`；`crates/codegen/extension-types/src/lib.rs` — `MarketplacePluginEntry`。

### Requirement: Marketplace action identity
MarketplaceAction SHALL 表达 refresh(可选 source_url_or_path)、install/update/uninstall(source_url_or_path,plugin_relative_path)、add_source(url)、remove_source(source_url_or_path)，封装于带 sessionId 的请求。

#### Scenario: 源选择
- **WHEN** refresh 未传 source_url_or_path
- **THEN** 解析为 None，表达未限定源；路径规范化、URL 准入、安装和删除的实际语义继续在 handler 与 marketplace crate 核查。

证据：`crates/codegen/extension-types/src/lib.rs` — `MarketplaceAction`；`crates/codegen/extension-types/src/lib.rs` — `MarketplaceActionRequest`。

### Requirement: Marketplace source configuration and pin policy
市场来源 SHALL 先加载bootstrap且featured=true，再加载普通sources；git优先path，无两者跳过。require_sha采用环境bool与配置bool OR。

#### Scenario: 实现边界
- **WHEN** 数组解析失败或tilde路径
- **THEN** 普通数组整体丢弃但保留bootstrap；任意首~以HOME展开，不验证来源名称非空或重复；pin默认false，仅任一来源true即可收紧。

证据：`crates/codegen/plugin-marketplace/src/config.rs` — `load_sources`。

### Requirement: Marketplace relative paths and existing ancestor checks
MarketplaceRelativePath SHALL 剥一次./并拒绝空段、点、父目录、absolute和冒号前缀，归一反斜杠为slash。

#### Scenario: 实现边界
- **WHEN** join目标部分不存在
- **THEN** canonicalize最近exists祖先并检查在canonical root内，再拼缺失后缀；不提供文件句柄绑定，dangling symlink可作为缺失后缀。

证据：`crates/codegen/plugin-marketplace/src/types.rs` — `join_under`。

### Requirement: Canonical marketplace index schema
市场 SHALL 仅从.grow-plugin/marketplace.json加载version1严格schema，包含name/plugins及typed local或git source。

#### Scenario: 实现边界
- **WHEN** 索引缺失、未知字段或其他位置
- **THEN** 整体返回错误，不回退目录扫描；entry名称重复、URL/SHA具体格式不在load_index校验。

证据：`crates/codegen/plugin-marketplace/src/index.rs` — `load_index`。

### Requirement: Optional marketplace component catalog
组件catalog SHALL 只从.grow-plugin/plugin-index.json加载version1，未知字段或读取解析错误降级None；成功后sanitize组件。

#### Scenario: 实现边界
- **WHEN** 请求远程组件
- **THEN** 只有index携带SHA且同名catalog SHA精确匹配才展示；local使用None预期SHA可覆盖实际扫描组件，不构成安装身份授权。

证据：`crates/codegen/plugin-marketplace/src/catalog.rs` — `components_for`。

### Requirement: Marketplace indexed discovery and enrichment
扫描 SHALL 按index顺序发现，remote直接透传元数据不clone，local要求受限路径有效目录与可加载manifest。

#### Scenario: 实现边界
- **WHEN** 本地manifest与index不同
- **THEN** name/version来自manifest，description/author仅None补index，其余分类匹配元数据来自index；不校验同名或去重entries。

证据：`crates/codegen/plugin-marketplace/src/scanner.rs` — `scan_marketplace`。

### Requirement: Marketplace component inventory extraction
本地组件 SHALL 扫描skill路径父目录名、一级.md commands/agents、hooks事件与handler数量、MCP/LSP对象keys。

#### Scenario: 实现边界
- **WHEN** 坏JSON或重复组件
- **THEN** 坏读取静默忽略；除hooks外按name/description去重排序，hooks保留数量；.md条目未额外检查普通文件。

证据：`crates/codegen/plugin-marketplace/src/scanner.rs` — `scan_components`。

### Requirement: Marketplace keyword matching precedence
关键词匹配 SHALL 在草稿至少3个Unicode字符时，对ASCII lowercase的keywords、规范domain及name按UTF8字节长度稳定降序寻找首匹配。

#### Scenario: 实现边界
- **WHEN** 同长度或词边界
- **THEN** 同长度保留候选插入顺序；边界是ASCII alnum/_类别变化，非Unicode分词；domain剥scheme/www/path而非完整URL验证。

证据：`crates/codegen/plugin-marketplace/src/matcher.rs` — `match_plugin_keyword`。

### Requirement: Marketplace install reference parsing
安装ref SHALL 接受name及首@拆出的qualifier，把URL、Git shorthand、slash路径、点/tilde起始、盘符和含#输入留给其他解析。

#### Scenario: 实现边界
- **WHEN** 空白或多个@
- **THEN** name仅非空且无slash，不trim；qualifier trim非空但保存原值，多余@保留。

证据：`crates/codegen/plugin-marketplace/src/install_resolve.rs` — `parse_marketplace_ref`。

### Requirement: Marketplace source qualifier resolution
qualifier SHALL 以GitHub owner/repo、local/git slug、原source name或slug name匹配并集，唯一匹配成功，多来源歧义返回所有index。

#### Scenario: 实现边界
- **WHEN** name与其他source owner重合
- **THEN** 返回Ambiguous，不隐式优先任何解释；slug仅ASCII lowercase和空白hyphen化，GitHub辅助不保证owner/repo恰好两段。

证据：`crates/codegen/plugin-marketplace/src/install_resolve.rs` — `resolve_qualified_source`。

### Requirement: Marketplace bare name featured selection
bare name SHALL ASCII不区分大小写匹配entry，唯一直接选，多项时仅恰好一个featured匹配获选并返回other_count。

#### Scenario: 实现边界
- **WHEN** 同featured source有重复同名项
- **THEN** 按entry计数，多个featured仍Ambiguous；不按独立source去重、不trimname。

证据：`crates/codegen/plugin-marketplace/src/install_resolve.rs` — `select_bare_name`。

### Requirement: Marketplace cache identity TTL and leases
Git来源cache SHALL 以URL的16hex DefaultHasher值定位目录与锁，UseTtl按FETCH_HEAD年龄小于5分钟跳过更新，Force强制刷新。

#### Scenario: 实现边界
- **WHEN** 同URL不同branch或调用便捷入口
- **THEN** branch不参与key，fresh时不核对branch；with_mode返回持锁lease，便捷返回path时释放锁；lock每100ms尝试，30秒超时。

证据：`crates/codegen/plugin-marketplace/src/git.rs` — `sync_source_cache_with_mode`。

### Requirement: Marketplace cache fetch and replacement
cache SHALL depth1 clone或fetch origin指定branch/HEAD，detach及hard reset FETCH_HEAD，失败尝试临时reclone后backup替换。

#### Scenario: 实现边界
- **WHEN** reclone失败
- **THEN** 旧cache尽量保留，安装rename失败尝试restore，restore失败报backup位置；不执行git clean或origin一致性检查，cleanup best-effort。

证据：`crates/codegen/plugin-marketplace/src/git.rs` — `reclone_repo`。

### Requirement: Marketplace Git command interaction and timeout
cache Git命令 SHALL 禁交互认证/LFS smudge、null stdin、detach并使用--分隔operand；每clone/fetch/checkout/reset/ls-remote设置15秒等待。

#### Scenario: 实现边界
- **WHEN** stderr量大或后代持pipe
- **THEN** wait后才读stderr，可能因pipe满超时或读后代pipe超时界限失效；kill仅直接child。probe只检查exit成功，不确认HEAD输出。

证据：`crates/codegen/plugin-marketplace/src/git.rs` — `run_git_timed`。

### Requirement: Marketplace local install provenance and reinstall
本地安装 SHALL 校验市场相对路径并委托Local安装，附provenance后insert/save，不施加remote pin gate。

#### Scenario: 实现边界
- **WHEN** 底层AlreadyInstalled
- **THEN** 先best-effort删除旧安装、remove/save registry再重试，失败不保证旧安装恢复；与transactional update不同。

证据：`crates/codegen/plugin-marketplace/src/installer.rs` — `install_from_marketplace`。

### Requirement: Marketplace remote install idempotence and pins
远程安装 SHALL 先校验subdir/URL/ref/SHA，再按provenance来源字符串与plugin_subdir精确检查已有安装；新装委托带label的pin gate。

#### Scenario: 实现边界
- **WHEN** 已有安装但未pin
- **THEN** 返回AlreadyInstalled并跳过新fetch gate，畸形operand仍先拒绝；不检查实际目录存在，底层冲突可走删旧重试。

证据：`crates/codegen/plugin-marketplace/src/installer.rs` — `install_from_remote_url`。

### Requirement: Marketplace transactional update staging and identity
update SHALL 要求entry与provenance归一相对路径相同，按精确来源字符串与路径找旧repo，在staging clone或copy且发现有效插件后才替换final。

#### Scenario: 实现边界
- **WHEN** 远程pin或本地copy
- **THEN** 远程hoist pin、校验并ensure_pinned先于staging；local来自同步来源不受pin gate。installed_at保留、updated_at刷新，新plugins整体替换。

证据：`crates/codegen/plugin-marketplace/src/installer.rs` — `update_from_marketplace_entry_transactional`。

### Requirement: Marketplace update rollback and change reporting
update SHALL 先final rename backup再staging rename final，registry保存失败尝试文件回滚后恢复原registry并保存。

#### Scenario: 实现边界
- **WHEN** 回滚失败或版本不变
- **THEN** 明确报告不一致及backup/重装需求，不宣称崩溃原子性；changed只比较HashMap首插件version，成功reinstalled恒true，不比较代码或commit。

证据：`crates/codegen/plugin-marketplace/src/installer.rs` — `original_registry`。

### Requirement: Marketplace remote staging clone verification
远程staging SHALL 有SHA时init/add/fetch/checkout并验证HEAD与完整SHA不区分大小写相等，保存git_ref优先SHA，否则ref。

#### Scenario: 实现边界
- **WHEN** 未pin或Git阻塞
- **THEN** 未pindepth1可branch；此路径使用tty_utils git output，无cache的15秒deadline；HEAD读取失败在非pin元数据路径可为空。

证据：`crates/codegen/plugin-marketplace/src/installer.rs` — `clone_repo_at_sha`。

### Requirement: Marketplace staged plugin discovery and local copying
staged发现 SHALL 根manifest优先，否则只检查一层真实目录，生成按manifest name的RepoPlugin map；local copy递归复制。

#### Scenario: 实现边界
- **WHEN** symlink或重复name
- **THEN** subdir仅lexical检查后is_dir，未canonical containment；copy跟随symlink且无深度/容量限制；重复name覆盖，first version来自不稳定HashMap遍历。

证据：`crates/codegen/plugin-marketplace/src/installer.rs` — `copy_dir_recursive`。

### Requirement: Plugin registry explicit enablement
PluginRegistry SHALL 仅在名称或完整ID列入enabled且未列入disabled时启用插件；所有scope均适用，disabled优先。

#### Scenario: 同时启停
- **WHEN** 同一插件出现在两个列表
- **THEN** 插件保留在registry但enabled为false。

证据：`crates/codegen/agent/src/plugins/registry.rs` — `from_discovered`。

### Requirement: Plugin registry active view
PluginRegistry SHALL 将enabled且trusted的插件纳入active_plugins；enabled_plugins仍包含已启用但未信任的插件。

#### Scenario: 未信任插件
- **WHEN** 插件enabled为true且trusted为false
- **THEN** enabled_plugins包含它，active_plugins不包含它。

证据：`crates/codegen/agent/src/plugins/registry.rs` — `active_plugins`。

### Requirement: Plugin discovery folder trust
插件发现 SHALL 对Project使用调用者project_trusted，对CLI和User设trusted为true，对ConfigPath使用home路径自动信任或TrustStore。

#### Scenario: 项目未信任
- **WHEN** project_trusted为false且发现项目插件
- **THEN** 返回候选的trusted为false，不因候选有manifest而授予信任。

证据：`crates/codegen/agent/src/plugins/discovery.rs` — `collect_plugin`。

### Requirement: Plugin discovery source precedence
插件发现 SHALL 按CLI、项目、用户目录、安装注册表、配置路径顺序收集，按canonical根首次去重，同名按scope优先且同scope首次保留。

#### Scenario: 重复根
- **WHEN** 同一个canonical根经CLI和配置路径发现
- **THEN** 仅保留首次CLI候选。

证据：`crates/codegen/agent/src/plugins/discovery.rs` — `discover_plugins`。

### Requirement: Plugin manifest exact identity
load_manifest SHALL 只读取根plugin.json并校验名称；缺失manifest不得从目录名推断插件身份。

#### Scenario: 缺少manifest
- **WHEN** 目录只有skills而无plugin.json
- **THEN** 返回Missing。

证据：`crates/codegen/agent/src/plugins/manifest.rs` — `load_manifest`。

### Requirement: Plugin manifest name validation
PluginManifest::validate SHALL 仅接受1至64字节ASCII小写字母、数字或连字符的名称且首尾不得为连字符；版本字符串不在该方法校验。

#### Scenario: 非法名称
- **WHEN** 名称包含大写字符
- **THEN** validate返回InvalidName。

证据：`crates/codegen/agent/src/plugins/manifest.rs` — `validate`。

### Requirement: Inline MCP sibling file visibility
PluginManifest SHALL 在mcpServers为inline时仍暴露存在的根.mcp.json；inline hooks与LSP不通过相同默认文件回退。

#### Scenario: 并存MCP
- **WHEN** inline MCP和.mcp.json同时存在
- **THEN** inline_mcp_servers与mcp_config_path均可返回内容或路径。

证据：`crates/codegen/agent/src/plugins/manifest.rs` — `mcp_config_path`。

### Requirement: Plugin hook environment precedence
插件Hook适配 SHALL 覆盖GROW_PLUGIN_ROOT和GROW_PLUGIN_DATA为调用参数并保留其它extra_env，将provenance设为Plugin并命名空间化Hook名。

#### Scenario: 用户覆盖根
- **WHEN** Hook env声明另一个GROW_PLUGIN_ROOT
- **THEN** 适配后的值为插件根参数。

证据：`crates/codegen/agent/src/plugins/hooks_adapter.rs` — `process_hooks_content`。

### Requirement: Local plugin structural refresh
本地插件非force刷新 SHALL 比较非symlink常规文件的相对路径与长度集合；等长内容修改不触发该比较差异，force绕过比较但不绕过信任判断。

#### Scenario: 等长编辑
- **WHEN** 源文件路径和长度均不变且force为false
- **THEN** 快照匹配时跳过复制。

证据：`crates/codegen/agent/src/plugins/local_refresh.rs` — `snapshot_matches_source`。

### Requirement: Plugin install registry version
InstallRegistry::try_load_from SHALL 仅接受version为1的注册表，缺文件返回空表；load_from对读取或解析失败告警后返回空表。

#### Scenario: 不支持版本
- **WHEN** registry.json的version不为1
- **THEN** try_load_from返回Json错误。

证据：`crates/codegen/agent/src/plugins/install_registry.rs` — `try_load_from`。

### Requirement: Plugin source parsing
安装来源解析 SHALL 将owner/repo展开为GitHub URL，三段相对路径作为本地路径；最后非空#后缀用于子目录选择。

#### Scenario: 三段路径
- **WHEN** 解析a/b/c
- **THEN** 得到相对cwd的Local来源。

证据：`crates/codegen/agent/src/plugins/git_install.rs` — `parse_install_source`。

### Requirement: Plugin git operand validation
Git URL和ref校验 SHALL trim并拒绝空值、NUL和首字符连字符；此校验不限制Git协议。

#### Scenario: 选项形式输入
- **WHEN** URL以--开头
- **THEN** 在克隆前返回校验错误。

证据：`crates/codegen/agent/src/plugins/git_install.rs` — `validate_git_operand`。

### Requirement: Plugin immutable pin gate
require_sha启用时Git安装 SHALL 要求完整40或64位十六进制SHA；完整SHA形式ref会提升到SHA槽，显式SHA优先。

#### Scenario: 分支未固定
- **WHEN** 要求SHA但只提供main分支
- **THEN** 返回UnpinnedRemoteRefused。

证据：`crates/codegen/agent/src/plugins/git_install.rs` — `ensure_pinned`。

### Requirement: Plugin SHA checkout verification
SHA安装 SHALL 初始化目标、浅fetch指定SHA并checkout FETCH_HEAD，读取HEAD与预期忽略大小写比较；读取失败或不匹配返回错误并尝试清理目标。

#### Scenario: 固定提交
- **WHEN** 成功安装指定SHA
- **THEN** HEAD必须匹配预期SHA。

证据：`crates/codegen/agent/src/plugins/git_install.rs` — `clone_repo_at_sha`。

### Requirement: Plugin local snapshot copy
本地安装 SHALL 复制整个来源目录树，跳过来源中的文件和目录symlink；安装函数返回结果而不自行保存注册表。

#### Scenario: 本地来源含链接
- **WHEN** 复制目录遇到symlink
- **THEN** 跳过该条目。

证据：`crates/codegen/agent/src/plugins/git_install.rs` — `copy_dir_recursive`。

### Requirement: Plugin install discovery scope
安装目录发现 SHALL 优先采用扫描根的有效plugin.json，否则只检查直接真实子目录；显式subdir拒绝绝对路径与ParentDir组件。

#### Scenario: 子目录越界
- **WHEN** subdir为../escape
- **THEN** 返回InstallFailed。

证据：`crates/codegen/agent/src/plugins/git_install.rs` — `discover_plugins_in_dir`。

### Requirement: Plugin update policy
插件更新 SHALL 对Local返回LiveLocal；Git完整SHA或以v开头且含点的ref返回Pinned，其余可变来源在require_sha下拒绝更新。

#### Scenario: 本地显式更新
- **WHEN** 调用update_repo处理Local
- **THEN** 返回LiveLocal，不复制来源。

证据：`crates/codegen/agent/src/plugins/git_install.rs` — `update_repo`。

### Requirement: Agent resource home independence
用户代理目录 SHALL 使用home/.grow/agents，独立于传入grow_home；无home时不产生用户资源根。

#### Scenario: 自定义配置根
- **WHEN** home和grow_home指向不同目录
- **THEN** 用户代理目录仍在home/.grow/agents。

证据：`crates/codegen/agent/src/discovery.rs` — `user_agent_dirs`。

### Requirement: Agent native lookup precedence
按cwd查找原生代理 SHALL 先查项目目录，再内置，再用户目录；原生递归发现将相对路径去扩展名作为代理ID。

#### Scenario: 嵌套定义
- **WHEN** 发现review/security.md
- **THEN** ID为review/security。

证据：`crates/codegen/agent/src/discovery.rs` — `agent_id_from_path`。

### Requirement: Agent plugin qualified lookup
插件感知查找 SHALL 将含冒号selector保留为plugin:agent命名空间；裸名先原生，插件候选恰一个时才回退。

#### Scenario: 原生冒号文件
- **WHEN** 原生文件名与插件qualified身份相同
- **THEN** qualified查找仍使用指定已启用插件。

证据：`crates/codegen/agent/src/discovery.rs` — `by_name_in_cwd_with_plugins_and_home`。

### Requirement: Agent untrusted plugin role omission
插件代理加载 SHALL 对trusted插件读取完整角色，对未信任插件读取frontmatter定义且不载入prompt_body；解析失败告警并跳过。

#### Scenario: 未信任插件
- **WHEN** 加载该插件代理定义
- **THEN** 不返回角色正文。

证据：`crates/codegen/agent/src/discovery.rs` — `load_plugin_agent_definition`。

### Requirement: Agent project instruction reminder
AGENTS配置格式化 SHALL 保留输入顺序和完整正文，路径与内容中和reminder标签后加入统一包装；空配置列表返回None。

#### Scenario: 长正文
- **WHEN** 格式化5000字符配置内容
- **THEN** 不截断该正文。

证据：`crates/codegen/agent/src/prompt/agents_md.rs` — `render_agents_md`。

### Requirement: Agent rules body extraction
配置读取 SHALL 对rules候选提取正文，普通AGENTS.md保留frontmatter；同canonical文件兼具rule身份时采用rule提取。

#### Scenario: 规则文件
- **WHEN** 规则包含frontmatter
- **THEN** 返回提取后的正文。

证据：`crates/codegen/agent/src/prompt/agents_md.rs` — `read_agents_config_with_roots`。

### Requirement: Skill native ignore filtering
技能列表 SHALL 在原生去重前按canonical路径前缀过滤ignore，允许较低优先来源回退；该阶段不处理随后追加的插件技能。

#### Scenario: 忽略本地同名技能
- **WHEN** repo同名来源仍存在
- **THEN** repo来源可保留。

证据：`crates/codegen/agent/src/prompt/skills.rs` — `filter_skills`。

### Requirement: Skill preload resolution
预加载技能 SHALL 按声明顺序匹配首个ASCII不分大小写裸名或格式化名称，再委托load_skill_content；找不到或加载失败告警并跳过。

#### Scenario: 缺失声明
- **WHEN** 声明的技能没有匹配项
- **THEN** 跳过该项继续处理后续声明。

证据：`crates/codegen/agent/src/prompt/skills.rs` — `resolve_preloaded_skills`。

### Requirement: Skill plugin identity stamping
插件技能 SHALL 使用规范化目录basename作为身份并保留原name为display_name（两者不同时）；附带插件名、版本、root、data与ConfigSource。

#### Scenario: 复制技能
- **WHEN** 两个插件技能目录不同但frontmatter名相同
- **THEN** 各目录生成不同的插件限定身份。

证据：`crates/codegen/agent/src/prompt/skills.rs` — `stamp_plugin_fields`。

### Requirement: Skill same scope collision recovery
原生技能去重 SHALL 对同scope且非Server/Bundled的同名项尝试以合法空闲目录basename重命名；跨scope冲突保留先到项。

#### Scenario: 同scope副本
- **WHEN** 同名副本有不同且空闲的合法目录名
- **THEN** 副本可用目录名保留。

证据：`crates/codegen/agent/src/prompt/skills.rs` — `dedupe_skills`。

### Requirement: Skill provenance path deduplication
技能canonical路径去重 SHALL 保留首项scope，并在首项缺config_source时继承同路径后来项的来源标记。

#### Scenario: 自动及配置重叠
- **WHEN** 同一文件经自动发现和config.paths发现
- **THEN** 只保留一项且可带ConfigToml来源。

证据：`crates/codegen/agent/src/prompt/skills.rs` — `dedupe_skills`。

### Requirement: Skill command source order
每个技能配置根 SHALL 先发现技能再发现命令；技能发现不应用.gitignore，忽略由skills.ignore路径前缀实现。

#### Scenario: gitignored配置根
- **WHEN** 项目.grow被.gitignore忽略
- **THEN** 其中技能和命令仍可发现。

证据：`crates/codegen/agent/src/prompt/skills.rs` — `collect_discovered_paths`。

### Requirement: Skill disabled list marking
技能列表 SHALL 在合并后按精确skill.name匹配disabled并设enabled=false，保留该项；此循环不使用插件限定dedup_key匹配。

#### Scenario: 禁用技能
- **WHEN** disabled含该项裸name
- **THEN** 列表仍有该项且enabled=false。

证据：`crates/codegen/agent/src/prompt/skills.rs` — `list_skills_with_plugins`。

### Requirement: Skill configuration file source
config.paths SHALL 接受直接SKILL.md或目录，目录通过共享技能发现函数扫描；结果标记ConfigToml来源，scope由路径与git根关系决定。

#### Scenario: 直接文件
- **WHEN** config.paths指向有效SKILL.md
- **THEN** 该技能带ConfigToml来源。

证据：`crates/codegen/agent/src/prompt/skills.rs` — `collect_config_skills`。

### Requirement: Skill injected source precedence
技能列表 SHALL 收集server_skill_dirs与bundled_skill_dirs，按scope排序后参与原生名称去重；Server与Bundled冲突不做同scope重命名恢复。

#### Scenario: server与bundled同名
- **WHEN** 两来源提供相同技能名
- **THEN** 按scope优先保留Server项。

证据：`crates/codegen/agent/src/prompt/skills.rs` — `list_skills_with_plugins`。

### Requirement: Skill plugin qualified preservation
技能合并 SHALL 先去重原生技能，再按插件dedup_key去重追加；插件裸name与原生冲突时仍保留插件项。

#### Scenario: 插件原生同名
- **WHEN** 原生与插件name相同
- **THEN** 插件限定项仍保留。

证据：`crates/codegen/agent/src/prompt/skills.rs` — `merge_skills_with_plugins`。

### Requirement: Skill preload message formatting
技能注入格式化 SHALL 仅处理存在且非空body，委托build_skill_message；多项以双换行连接并添加首尾双换行，空集合为空字符串。

#### Scenario: 空正文
- **WHEN** 技能body为空字符串
- **THEN** 该项不产生注入内容。

证据：`crates/codegen/agent/src/prompt/skills.rs` — `format_skills_for_injection`。

### Requirement: Agent plugin hook event filtering
插件Hook适配 SHALL 使用HookEventName::parse_key判定顶层hooks事件，移除不支持事件并返回warning；非法JSON交统一parser处理。

#### Scenario: 未知事件
- **WHEN** hooks包含不支持事件键
- **THEN** 该键被删除且返回warning。

证据：`crates/codegen/agent/src/plugins/hooks_adapter.rs` — `prefilter_unsupported_events`。

### Requirement: Agent plugin hook command display preservation
Hook适配 SHALL 对command做插件token及通用环境变量展开，同时保持command_raw不变。

#### Scenario: 插件根占位符
- **WHEN** command含GROW_PLUGIN_ROOT占位符
- **THEN** 执行command得到替换结果，command_raw保留原字符串。

证据：`crates/codegen/agent/src/plugins/hooks_adapter.rs` — `process_hooks_content`。

### Requirement: Plugin registry snapshot retention
SharedPluginRegistryHandle SHALL 通过Arc快照返回registry；reload替换shared当前值，不修改已取得的旧Arc对象。

#### Scenario: 刷新共享状态
- **WHEN** 消费者已持有旧snapshot后调用reload
- **THEN** 旧snapshot仍引用旧registry。

证据：`crates/codegen/agent/src/plugins/registry.rs` — `snapshot`。

### Requirement: Plugin session registry directory retention
build_for_cwd SHALL 合并disk、启动CLI及session目录；无发现但session目录非空时仍返回携带目录的空registry。

#### Scenario: 会话目录暂空
- **WHEN** session_plugin_dirs非空且无有效插件
- **THEN** 返回Some空registry并保留session目录。

证据：`crates/codegen/agent/src/plugins/registry.rs` — `build_for_cwd`。

### Requirement: Plugin read only registry construction
build_for_cwd SHALL 不刷新本地安装；refresh_and_build_for_cwd先以force=false刷新再构建。

#### Scenario: 纯重建
- **WHEN** 调用build_for_cwd
- **THEN** 本层不调用本地复制刷新。

证据：`crates/codegen/agent/src/plugins/registry.rs` — `refresh_and_build_for_cwd`。

### Requirement: Plugin MCP ownership lookup
PluginRegistry SHALL 只为enabled且trusted插件登记MCP owner，对相同服务器名保留首个声明者；服务器计数为文件与inline键的去重并集。

#### Scenario: 禁用MCP插件
- **WHEN** 插件只在disabled列表
- **THEN** 其服务器不登记owner。

证据：`crates/codegen/agent/src/plugins/registry.rs` — `plugin_mcp_server_names`。

### Requirement: Plugin trust canonical membership
TrustStore SHALL canonicalize候选根后查询信任集合，失败视为未信任；grant成功写入路径行后才更新内存集合。

#### Scenario: 不存在根
- **WHEN** 检查不能canonicalize的路径
- **THEN** 返回false。

证据：`crates/codegen/agent/src/plugins/trust.rs` — `grant_trust`。

### Requirement: Plugin trust revoke failure ordering
TrustStore撤销 SHALL 先移除内存条目再重写磁盘；重写失败返回错误且不恢复内存条目。

#### Scenario: 撤销写失败
- **WHEN** 已信任路径可canonicalize但文件重写失败
- **THEN** 返回错误且当前内存集合已移除该路径。

证据：`crates/codegen/agent/src/plugins/trust.rs` — `revoke_trust`。

### Requirement: Plugin manifest component directory override
Manifest显式skills、commands、agents列表 SHALL 替代默认目录；显式空列表不返回默认目录。

#### Scenario: 空覆盖
- **WHEN** skills为显式空数组且默认skills目录存在
- **THEN** skill_dirs为空。

证据：`crates/codegen/agent/src/plugins/manifest.rs` — `resolve_dirs`。

### Requirement: Plugin install key source identity
安装repo key SHALL 从来源标识生成；Git来源标识包含URL和可选subdir，不含ref或SHA，key散列使用DefaultHasher低32位。

#### Scenario: 不同pin同来源
- **WHEN** URL与subdir相同但SHA不同
- **THEN** 生成相同repo key。

证据：`crates/codegen/agent/src/plugins/install_registry.rs` — `repo_key`。

### Requirement: Plugin registry save replacement
InstallRegistry保存 SHALL 先写PID与时间戳命名的临时文件再rename替换registry.json；rename失败尽力删除临时文件并返回错误。

#### Scenario: 保存替换失败
- **WHEN** 临时文件写成功但rename失败
- **THEN** 返回IO错误。

证据：`crates/codegen/agent/src/plugins/install_registry.rs` — `save_atomic`。

### Requirement: Agent stable prompt composition
PromptContext稳定提示词 SHALL 仅拼接mandatory core与对应audience，过滤空section并以双换行连接；模板错误返回None。

#### Scenario: 角色正文存在
- **WHEN** 调用稳定提示词render
- **THEN** 角色正文不进入稳定头。

证据：`crates/codegen/agent/src/prompt/context.rs` — `render_with_renderer`。

### Requirement: Agent role prompt modes
角色渲染 SHALL 在Extend模式加入standard，在Full模式省略standard；两种模式仍处理角色body与session extensions。

#### Scenario: Full角色
- **WHEN** Full模式启用memory且工具齐全
- **THEN** 仍可包含memory扩展段。

证据：`crates/codegen/agent/src/prompt/context.rs` — `render_role_with_renderer`。

### Requirement: Agent role template failure fallback
角色模板渲染 SHALL 对单段渲染失败使用原模板文本作为回退，最终无非空段返回None。

#### Scenario: 坏角色模板
- **WHEN** 某角色模板不能渲染
- **THEN** 该段使用原文本而非整体抛错。

证据：`crates/codegen/agent/src/prompt/context.rs` — `render_role_with_renderer`。

### Requirement: Agent prompt placeholder fields
PromptContext占位符 SHALL 提供memory_enabled、is_non_interactive、system_prompt_label，不将AGENTS内容混入这些占位符。

#### Scenario: 构造占位符
- **WHEN** context含AGENTS配置
- **THEN** AGENTS仍由独立reminder接口输出。

证据：`crates/codegen/agent/src/prompt/context.rs` — `placeholders`。

### Requirement: Agent audience instruction delivery
Agent的AGENTS提醒 SHALL 对Primary与Subagent使用相同完整配置格式化，不为子代理截断正文。

#### Scenario: 子代理长配置
- **WHEN** Subagent context含长AGENTS正文
- **THEN** 提醒保留全文。

证据：`crates/codegen/agent/src/prompt/context.rs` — `agents_md_user_reminder`。

### Requirement: Agent resource activation ownership
Agent资源域准备及激活 SHALL 要求tool bridge的Arc唯一可变所有权；已共享时返回错误。

#### Scenario: bridge已共享
- **WHEN** 克隆bridge Arc后尝试激活
- **THEN** 无法取得唯一可变引用并返回错误。

证据：`crates/codegen/agent/src/agent.rs` — `activate_resource_domain`。

### Requirement: Agent runtime snapshot rendering
RuntimeContextSnapshot SHALL 渲染调用者提供的OS、shell、路径、日期与可选VCS快照；日期格式为年-月-日，不在render收集实时系统信息。

#### Scenario: 指定日期
- **WHEN** 输入固定date后render
- **THEN** 使用输入日期。

证据：`crates/codegen/agent/src/prompt/user_message.rs` — `RUNTIME_DATE_FORMAT`。

### Requirement: Agent VCS status byte cap
normalize_vcs_status SHALL trim并对空状态返回None；超10000字节时向UTF8边界回退并可退至换行，再追加截断标记。

#### Scenario: 多字节长状态
- **WHEN** 状态超过10000字节
- **THEN** 截断不切开UTF8字符，标记可使结果超过原字节阈值。

证据：`crates/codegen/agent/src/prompt/user_message.rs` — `normalize_vcs_status`。

### Requirement: Agent workspace user path interpretation
工作区user标识 SHALL 将无斜线的名字放入users子目录，有斜线的标识按传入路径处理；解析依赖目录存在，不自行增加containment验证。

#### Scenario: 裸用户名
- **WHEN** user为alice
- **THEN** 候选目录为root/users/alice。

证据：`crates/codegen/agent/src/prompt/workspace_user.rs` — `users`。

### Requirement: Agent built in prompt embedding
内置提示词 SHALL 通过include_str编译嵌入foundation、audience和extensions Markdown，运行时不依赖这些文件仍存在。

#### Scenario: 打包运行
- **WHEN** 二进制运行环境无prompts目录
- **THEN** 内置模板字符串仍可使用。

证据：`crates/codegen/agent/src/prompt/template.rs` — `include_str!`。

### Requirement: Agent definition strict fields
AgentDefinition的manifest反序列化 SHALL 使用camelCase并拒绝未知字段，description必需；tool_config与运行时来源字段不由manifest直接反序列化。

#### Scenario: 未知字段
- **WHEN** manifest包含未知键
- **THEN** 解析返回错误。

证据：`crates/codegen/agent/src/config.rs` — `AgentDefinition`。

### Requirement: Agent file derived identity
AgentDefinition::from_file SHALL 从文件读取定义并用文件stem覆盖name，登记source_path和路径推断scope；frontmatter-only入口不保留角色body。

#### Scenario: 不同声明名
- **WHEN** 文件review.md声明另一个name
- **THEN** from_file返回name为review。

证据：`crates/codegen/agent/src/config.rs` — `from_file`。

### Requirement: Agent JSON identity validation
AgentDefinition::from_json SHALL 要求name.trim非空，保留原name字符串；promptBody单独取出，非字符串忽略，非空字符串trim后保存。

#### Scenario: 空JSON名称
- **WHEN** name只含空白
- **THEN** from_json返回错误。

证据：`crates/codegen/agent/src/config.rs` — `from_json`。

### Requirement: Agent JSON runtime field omission
AgentDefinition::to_json_value SHALL 输出可序列化manifest字段并恢复promptBody，不承诺保留跳过序列化的运行时工具配置、来源与clamp。

#### Scenario: 运行时定义导出
- **WHEN** 定义包含source_path
- **THEN** 导出JSON不以source_path复现运行时身份。

证据：`crates/codegen/agent/src/config.rs` — `to_json_value`。

### Requirement: Agent runtime equivalence comparison
runtime_equivalent SHALL 比较manifest序列化、工具配置及额外运行时clamp、body、来源、scope和plugin身份；序列化失败视为不等。

#### Scenario: 来源变化
- **WHEN** 其他内容相同但source_path不同
- **THEN** runtime_equivalent返回false。

证据：`crates/codegen/agent/src/config.rs` — `runtime_equivalent`。

### Requirement: Agent selector plugin namespace
selector_identity SHALL 对有plugin_name的定义返回plugin:name，否则返回原name，不增加转义。

#### Scenario: 插件定义
- **WHEN** plugin_name为p且name为review
- **THEN** selector为p:review。

证据：`crates/codegen/agent/src/config.rs` — `selector_identity`。

### Requirement: Agent native preset precedence
工具preset查找 SHALL 规范化查找名称并优先内置preset，外部注册不能覆盖同名内置；注册存储使用传入键。

#### Scenario: 注册内置名
- **WHEN** 注册与内置同名builder后查找
- **THEN** 仍优先内置preset。

证据：`crates/codegen/agent/src/config.rs` — `toolset_for_preset`。

### Requirement: Agent primary capability eligibility
primary_agent_issues SHALL 对显式subagent_only直接视为不适用于主代理；主代理按工具配置及自身筛选检查读、写和执行能力，不将该检查等同最终session权限。

#### Scenario: 子代理定义
- **WHEN** subagent_only为true
- **THEN** is_primary_agent_eligible不将其作为可选主代理。

证据：`crates/codegen/agent/src/config.rs` — `primary_agent_issues`。

### Requirement: Agent strict harness predicate
is_strict_harness SHALL 由inject_default_tools的否定决定，不依据角色名字符串推断。

#### Scenario: 禁用默认注入
- **WHEN** inject_default_tools为false
- **THEN** is_strict_harness返回true。

证据：`crates/codegen/agent/src/config.rs` — `is_strict_harness`。

### Requirement: Agent file tool override scope
override_file_tools SHALL 只替换已存在的约定Grow/hashline读写工具位置，不自动新增缺失位置，不同步authored快照。

#### Scenario: 缺少目标工具
- **WHEN** 调用override_file_tools但原配置无相应工具
- **THEN** 不因覆盖请求新增该工具。

证据：`crates/codegen/agent/src/config.rs` — `override_file_tools`。

### Requirement: Agent MCP inheritance matching
McpInheritance::permits SHALL 按All、None、Named或Except策略匹配精确server名称；此判断不执行服务器启动。

#### Scenario: 排除列表
- **WHEN** server处于Except列表
- **THEN** permits返回false。

证据：`crates/codegen/agent/src/config.rs` — `permits`。

### Requirement: Agent memory scope directory resolution
MemoryScope::resolve_dir SHALL 按User、Project或Local生成代理记忆目录及project标志，agent_name直接参与路径join，不在此验证containment。

#### Scenario: 用户scope
- **WHEN** 解析User记忆目录
- **THEN** 使用用户配置根并标记非project。

证据：`crates/codegen/agent/src/config.rs` — `resolve_dir`。

### Requirement: Agent builder explicit definition precedence
AgentBuilder SHALL 在存在显式definition时直接克隆该定义；名称、描述、工具等用于生成默认定义的setter不覆盖该显式定义。

#### Scenario: 显式定义
- **WHEN** from_definition后设置名称
- **THEN** resolve_definition仍使用显式定义名称。

证据：`crates/codegen/agent/src/builder.rs` — `resolve_definition`。

### Requirement: Agent curated empty toolset rejection
AgentBuilder SHALL 在inject_default_tools=false且原tool_config为空时返回InvalidConfig，早于补入控制工具。

#### Scenario: 空curated配置
- **WHEN** 禁用默认注入且无工具
- **THEN** build返回错误。

证据：`crates/codegen/agent/src/builder.rs` — `build`。

### Requirement: Agent default optional tool injection
AgentBuilder默认注入 SHALL 根据memory backend、web、LSP和write开关补入可选工具；write关闭不移除已在定义中的write工具。

#### Scenario: 已声明write
- **WHEN** write开关关闭但定义已含write
- **THEN** 不因该开关删除已声明工具。

证据：`crates/codegen/agent/src/builder.rs` — `with_write_file_enabled`。

### Requirement: Agent memory backend removal
AgentBuilder SHALL 在没有memory backend时移除约定Grow memory工具，即使定义为curated。

#### Scenario: 无后端
- **WHEN** 定义包含Grow memory工具但未提供backend
- **THEN** 最终装配移除该工具。

证据：`crates/codegen/agent/src/builder.rs` — `build`。

### Requirement: Agent workflow tool runtime gate
工作流工具装配 SHALL 先移除已有canonical ID或Workflow kind项，仅在Primary且后台工作流启用时加入canonical工具，随后仍受权限筛选。

#### Scenario: 子代理
- **WHEN** Subagent启用后台工作流标志
- **THEN** 不因此注入工作流工具。

证据：`crates/codegen/agent/src/builder.rs` — `apply_workflow_tool_gates`。

### Requirement: Agent coordination tool audience gate
协调工具装配 SHALL 移除约定canonical协调ID，并仅为Primary补入；随后仍受允许与拒绝列表限制。

#### Scenario: 子代理协调
- **WHEN** 构建Subagent audience
- **THEN** 不保留此gate控制的canonical协调工具。

证据：`crates/codegen/agent/src/builder.rs` — `apply_coordination_tool_gates`。

### Requirement: Agent task lifecycle normalization
最终工具装配 SHALL 在没有Task工具时移除TaskOutput/KillTask，并将bash后台和自动后台参数置false，覆盖此前参数合并值。

#### Scenario: Task被筛掉
- **WHEN** 最终配置没有Task
- **THEN** 不保留Task生命周期工具或bash后台能力参数。

证据：`crates/codegen/agent/src/builder.rs` — `build`。

### Requirement: Agent parameter merge overwrite
工具参数合并 SHALL 针对精确工具ID合并JSON对象，同名参数以调用方覆盖值为准。

#### Scenario: 重复参数
- **WHEN** 覆盖配置与已有params含同一键
- **THEN** 合并后使用覆盖值。

证据：`crates/codegen/agent/src/builder.rs` — `merge_tool_params`。

### Requirement: Agent preloaded catalog precedence
AgentBuilder SHALL 优先使用显式预加载技能catalog，即使discover_skills=false；否则按发现开关选择扫描或空集合。

#### Scenario: 冻结catalog
- **WHEN** 提供preloaded_skills且关闭发现
- **THEN** 构建仍使用预加载catalog。

证据：`crates/codegen/agent/src/builder.rs` — `with_preloaded_skills`。

### Requirement: Agent session tool clamp last
AgentBuilder SHALL 在自身allow/deny筛选之后应用definition.session_tools_allowed收紧最终工具集合。

#### Scenario: session限制
- **WHEN** 工具通过定义筛选但session拒绝
- **THEN** 最终集合移除工具。

证据：`crates/codegen/agent/src/builder.rs` — `session_tools_allowed`。

### Requirement: Agent task model guidance catalog
Task模型指导 SHALL 排序并去重公开model slugs后用于描述文本；描述本身不实施模型访问权限。

#### Scenario: 重复slug
- **WHEN** 目录包含重复模型名
- **THEN** 指导只列一次该名。

证据：`crates/codegen/agent/src/builder.rs` — `task_model_guidance`。

### Requirement: Agent task user description preservation
Task描述构建 SHALL 对用户定义代理保留原description；被用户定义shadow的内置也使用用户描述，不再加入原内置工具片段。

#### Scenario: 覆盖内置
- **WHEN** 项目代理shadow explore
- **THEN** 描述取用户定义。

证据：`crates/codegen/agent/src/builder.rs` — `build_task_description`。

### Requirement: Agent authored tool filtering
构建器 SHALL 先应用disallowed_tools，再应用非空tools allowlist；allowlist保留SearchTool/UseTool kind例外，未知条目不授予其它工具，随后session clamp仍可移除例外工具。

#### Scenario: 未知allowlist
- **WHEN** tools仅含未知条目
- **THEN** 不回退放行全部工具。

证据：`crates/codegen/agent/src/builder.rs` — `unresolved`。

### Requirement: Agent repository chain home guard
RepoDirChain SHALL 通过一次git发现取得workdir，若根canonical等于home则按非repo处理；非repo链仅含cwd。

#### Scenario: home为git根
- **WHEN** cwd所在git workdir就是home
- **THEN** git_root为None且dirs只有cwd。

证据：`crates/codegen/agent/src/repo.rs` — `is_home_dir`。

### Requirement: Agent repository chain canonical stopping
RepoDirChain SHALL 从cwd向父目录遍历，使用每层canonical路径与root比较停止，输出保留原路径拼写。

#### Scenario: 符号路径别名
- **WHEN** 某层canonical等于repo root
- **THEN** 在该层停止。

证据：`crates/codegen/agent/src/repo.rs` — `resolve`。

### Requirement: Agent construction timing events
TimingGuard SHALL 在Drop时向grow_instrumentation记录timing事件、名称与经过微秒数。

#### Scenario: guard离开作用域
- **WHEN** TimingGuard正常Drop
- **THEN** 输出timing日志事件。

证据：`crates/codegen/agent/src/timing.rs` — `TimingGuard`。

### Requirement: Agent primary subagent merge toggles
原生子代理列表 SHALL 以三个内置子代理为起点，合并发现定义并按精确name toggle过滤；遗漏toggle默认为启用。

#### Scenario: 关闭explore
- **WHEN** toggle中explore为false
- **THEN** 原生列表不含explore。

证据：`crates/codegen/agent/src/discovery.rs` — `merge_subagents`。

### Requirement: Agent background refresh stale sweep
本地刷新清理 SHALL 仅按同名前缀临时或backup兄弟项的mtime年龄至少一小时判定陈旧，不在此检查所属PID是否活跃。

#### Scenario: 年轻临时项
- **WHEN** 匹配前缀但不足一小时
- **THEN** sweep不因年龄清理它。

证据：`crates/codegen/agent/src/plugins/local_refresh.rs` — `sweep_stale`。

### Requirement: Agent refresh registry save reporting
本地刷新入口 SHALL 仅在refreshed大于零时保存registry；保存失败只记录warning，不改变已计算summary计数。

#### Scenario: 保存失败
- **WHEN** 复制刷新成功但registry.save失败
- **THEN** 返回summary仍包含refreshed计数。

证据：`crates/codegen/agent/src/plugins/local_refresh.rs` — `refresh_local_installs_from_disk`。

### Requirement: Agent stable resource roots
用户Agent与Skill资源根 SHALL 为home/.grow；home不存在时资源根列表为空，不以runtime配置根替代。

#### Scenario: 无home
- **WHEN** 请求用户资源根且home为None
- **THEN** 返回空列表。

证据：`crates/codegen/agent/src/resource_roots.rs` — `user_roots`。

### Requirement: Agent instruction gitignore inputs
AGENTS gitignore构建 SHALL 使用repo根.gitignore与全局core.excludesFile或home/.gitignore，不递归加载嵌套.gitignore；无repo根返回None。

#### Scenario: 无repo
- **WHEN** 构建gitignore且git_root为None
- **THEN** 返回None。

证据：`crates/codegen/agent/src/prompt/ignore.rs` — `build_gitignore`。

### Requirement: Agent builder resource discovery seeding
AgentBuilder SHALL 在AGENTS读取开关关闭时仍初始化bridge的AGENTS和技能发现状态；初始AGENTS集合为空，技能发现cwd由discover_skills决定。

#### Scenario: 关闭AGENTS
- **WHEN** definition.agents_md为false
- **THEN** 仍执行seed_agents_md。

证据：`crates/codegen/agent/src/builder.rs` — `seed_agents_md`。

### Requirement: Agent builder preloaded listing exclusion
AgentBuilder SHALL 从技能发现listing中排除已预加载技能的精确path，避免同一预加载文件再次列出。

#### Scenario: 已预加载路径
- **WHEN** 技能path处于preloaded_skill_paths
- **THEN** listing不包含该path。

证据：`crates/codegen/agent/src/builder.rs` — `listing_skills`。

### Requirement: Agent builder display path rewriting
AgentBuilder SHALL 仅对AGENTS显示file_path作working_dir字符串替换，不改变工具使用的真实working_directory。

#### Scenario: 显示cwd覆盖
- **WHEN** 指定prompt_working_directory
- **THEN** AGENTS显示路径被替换，实际工具cwd仍为原目录。

证据：`crates/codegen/agent/src/builder.rs` — `display_cwd`。

### Requirement: Agent builder empty stable prompt fallback
AgentBuilder SHALL 将稳定提示词render的None转为空字符串；角色独立渲染，description只在bridge渲染返回Some时替换。

#### Scenario: 稳定渲染失败
- **WHEN** prompt_context.render返回None
- **THEN** Agent仍使用空system_prompt继续构建。

证据：`crates/codegen/agent/src/builder.rs` — `unwrap_or_default`。

### Requirement: Agent plugin component path fallback limits
显式组件路径containment SHALL canonicalize根与候选，失败各自回退原路径进行前缀检查；默认组件目录或文件只检查类型，不保证symlink目标containment。

#### Scenario: 默认路径链接
- **WHEN** 默认skills目录指向root外且is_dir为true
- **THEN** 默认目录分支可返回该路径。

证据：`crates/codegen/agent/src/plugins/manifest.rs` — `is_path_contained`。

### Requirement: Agent session clamp empty distinction
session_tools_allowed SHALL 优先拒绝deny匹配，allowlist为None时允许其它工具，为Some空列表时不允许任何工具。

#### Scenario: 显式空allow
- **WHEN** session allowlist为Some空列表
- **THEN** 所有工具均不通过。

证据：`crates/codegen/agent/src/config.rs` — `session_tools_allowed`。

### Requirement: Agent explicit subagent policy
SubagentPolicy SHALL 独立于普通工具名授权；生成过滤器时空allow表示不限制，deny按精确name优先。

#### Scenario: allow与deny同时匹配
- **WHEN** 同一代理名既允许又拒绝
- **THEN** allows返回false。

证据：`crates/codegen/agent/src/config.rs` — `subagent_filter`。

### Requirement: Agent subagent list input normalization
子代理allow/deny反序列化 SHALL 对逗号字符串分项trim并去空；数组元素按原字符串保留。

#### Scenario: 数组空白
- **WHEN** 数组含带空白名称
- **THEN** 不按逗号字符串规则trim该元素。

证据：`crates/codegen/agent/src/config.rs` — `deserialize_string_or_vec`。

### Requirement: Agent MCP inheritance input schema
McpInheritance反序列化 SHALL 接受不分大小写但不trim的all/none字符串，或精确单键named/except映射。

#### Scenario: 多键映射
- **WHEN** mcpInheritance同时含named与except
- **THEN** 反序列化失败。

证据：`crates/codegen/agent/src/config.rs` — `McpInheritance`。

### Requirement: Agent presentation color tolerance
AgentDefinition颜色字段 SHALL 对不支持的颜色值告警后设None，而不因该呈现字段丢弃整个有效代理定义。

#### Scenario: 未知颜色
- **WHEN** 代理声明chartreuse
- **THEN** 代理仍可解析且color=None。

证据：`crates/codegen/agent/src/config.rs` — `AgentColor`。

### Requirement: Agent authored toolset resolution
AgentDefinition解析入口 SHALL 解析声明preset；直接serde反序列化跳过tool_config并使用默认配置，不等同完整解析入口。

#### Scenario: 未知preset
- **WHEN** 通过完整解析入口声明未知preset
- **THEN** 返回配置错误。

证据：`crates/codegen/agent/src/config.rs` — `resolve_declared_toolset`。

### Requirement: Pager slash registry ACP replacement collision qualification and triggers

set_acp_commands/set_acp_state SHALL 在每次更新先删除全部RegistrySource::Acp对象而保持Builtin，再按新advertisement重建ACP目录。ACP名称与任一builtin canonical/alias按Unicode lowercase结果冲突，或与help、hooks-list/trust/untrust/add/remove阻止名忽略ASCII大小写冲突时，不得占用原名；只有meta.path为string、meta.scope可解析且scope非Plugin的skill可改名为`<scope>:<原name>`后加入，Plugin scope、缺失/无效meta或普通命令直接丢弃。非冲突ACP命令原样追加。rebuild_triggers对缺工具或hard-hidden命令既不建key也不建trigger；menu-hidden命令保留canonical/alias key但不建trigger；其他命令每个canonical和alias各生成一条CommandTrigger，复制description、usage、args flags、kind、source、command_index与RegistrySource。Builtin发生精确canonical/alias key重复时panic；ACP对象间重复不panic，后写key覆盖先前索引但各自trigger仍保留。is_builtin只根据当前key映射索引的RegistrySource判定；commands_by_index越界返回None。

#### Scenario: Local skill collides with builtin
- **WHEN** ACP广告名login，meta含path且scope为local，而builtin已有login
- **THEN** 保留builtin login并注册local:login为ACP命令及dropdown trigger。

#### Scenario: Plugin skill collides
- **WHEN** 同名ACP skill的scope为plugin
- **THEN** 丢弃该广告，不虚构plugin:前缀。

#### Scenario: ACP generation replacement
- **WHEN** 第二次同步不再包含上一代flush命令
- **THEN** flush从commands、key与trigger移除，builtin保持。

源码证据：
- `crates/codegen/pager/src/slash/registry.rs` — `client_collision_qualified_name / CommandTrigger::new / CommandRegistry::apply_acp_commands / rebuild_triggers / is_builtin / commands_by_index`。

### Requirement: Pager ACP slash metadata skill classification and taxonomy projection

AcpSlashCommand::from SHALL 复制AvailableCommand name/description，始终has_args=true且args_required=false；只有Unstructured input提供arg hint，其他或缺失input无placeholder。meta同时含string path与可反序列化SkillScope时保存两者并is_skill=true；meta无skill keys为普通ACP。scope key存在但值不可解析时无论path如何均按foreign普通ACP透传且meta_malformed=false；否则只有path或scope单边存在、或path非string而scope有效时标meta_malformed=true。合法Plugin scope在此仍是skill，collision时是否丢弃由registry负责。grow/commandKind和grow/commandSource字符串按ASCII lowercase但不trim解析，合法显式值分别覆盖类别与来源；source builtin与built-in同义。缺失或非法显式taxonomy时，有有效skill path或存在任意workflowDefinitionId key的命令kind回退Run；source依次按skill、workflow、Extension回退，普通kind为Extension。kind/source独立解析，一个合法不要求另一个合法。

#### Scenario: Unknown skill scope
- **WHEN** meta含path且scope值不属于Local/Repo/User/Plugin
- **THEN** 按非skill HostCommand分类而非Malformed错误。

#### Scenario: Explicit taxonomy
- **WHEN** meta提供grow/commandKind=view与grow/commandSource=builtin
- **THEN** 类别和来源采用显式值，不按workflow或skill fallback。

#### Scenario: Workflow marker
- **WHEN** meta存在workflowDefinitionId键但没有skill path或显式taxonomy
- **THEN** kind为Run、source为Workflow。

源码证据：
- `crates/codegen/pager/src/slash/acp_command.rs` — `AcpSlashCommand::from / parse_command_kind / parse_command_source / SlashCommand metadata methods`。

### Requirement: Pager ACP slash malformed host and raw skill execution routes

AcpSlashCommand::run SHALL 首先拒绝meta_malformed并返回`Malformed skill metadata for /<name>`。非skill命令构造HostCommandRequest：args.trim为空时command精确为`/<name>`，否则为`/<name> <raw args>`，保留非空args的原始前后空白；description复制ACP描述并由HostCommandRequest生成invocation ID。有效skill命令不得在pager读取skill_path、检查文件存在、替换SKILL_DIR或组装XML；它用同一raw invocation作为display_text及唯一ACP Text ContentBlock，返回InjectSkill{display_as_skill:true,scheduled_task_preview:None}交Shell解析和展开。已带local:/user:等qualification的name原样保留，不再次加scope。skill_path与skill_scope仅作为分类证据，run不访问路径，因此缺失文件仍生成InjectSkill。

#### Scenario: Missing skill file
- **WHEN** 有效skill metadata的path指向不存在文件
- **THEN** 仍生成单Text block的raw /skill args，不在pager报IO错误。

#### Scenario: Qualified collision name
- **WHEN** command name已经是local:compact且无参数
- **THEN** display与模型block均为/local:compact，不双重加scope。

#### Scenario: Foreign command with arguments
- **WHEN** 普通ACP命令args非空
- **THEN** 返回HostCommand而不是InjectSkill，Shell拥有执行语义。

源码证据：
- `crates/codegen/pager/src/slash/acp_command.rs` — `AcpSlashCommand::run`。
### Requirement: Pager hook and plugin catalog push with skills refetch

HooksChanged and PluginsChanged SHALL update only an open extensions modal. Plugins seed groups, replace loaded data and move non-loading skills to Loading for a root-session FetchSkillsList; disappearance yields a modal error when possible. Nonempty installed plugin updates produce one joined version notice.

#### Scenario: Hooks
- **WHEN** the modal is open
- **THEN** hook data becomes Loaded with trust and errors.

#### Scenario: Plugins
- **WHEN** plugin data changes and skills are not Loading
- **THEN** plugins load and a skills refetch is scheduled when session identity remains.

#### Scenario: Closed
- **WHEN** no extensions modal exists
- **THEN** catalog pushes store nothing here.

#### Scenario: Installed
- **WHEN** update tuples are nonempty
- **THEN** one joined old-to-new notice is appended.

证据：`crates/codegen/pager/src/app/acp_handler/session_notification.rs` — `HooksChanged`、`PluginsChanged`、`PluginUpdatesInstalled`。

### Requirement: Pager mCP add and setup form projection

扩展弹窗 SHALL 将 MCP add 的 `[URL / Command, Name]` 表单转换为启用且 full-access 的 server 配置：HTTP(S) 生成 Streamable HTTP transport，其他输入按空白拆成 stdio command 与 args，空名称从主机有效段或命令派生，空地址拒绝；setup 表单仅接受恰好一个且含选项的字段，已保存值优先于默认值，方向键或 j/k 有界移动，Enter 提交当前选项，Esc 取消。

#### Scenario: HTTP add
- **WHEN** 提交 HTTP(S) 地址且名称为空或显式给出
- **THEN** 构造 Streamable HTTP 配置并从主机或显式字段确定名称。

#### Scenario: Stdio add
- **WHEN** 提交命令及参数
- **THEN** 首 token 成为 command、其余 token 成为 args，server 默认启用且授权全部工具。

#### Scenario: Setup selection
- **WHEN** server 暴露一个有选项的 setup 字段
- **THEN** 保存值覆盖默认值，键盘在范围内选择并可提交键值映射；其他 setup 形状不建立表单。

证据：`crates/codegen/pager/src/views/extensions_modal.rs`。

### Requirement: Pager hook source grouping filtering and navigation

Hooks 页 SHALL 对 name、event、matcher、command 与 URL 执行大小写不敏感的 substring 优先/有序子序列 fuzzy 搜索，再按启用状态过滤并按 source_dir 合组；搜索期间折叠组强制展开，正常前后导航跳过过滤掉的项并在折叠组间使用首个代表项。来源标签区分用户或项目插件、全局/项目 hooks 与可移除自定义路径，项目 `.grow/plugins` 和 `.grow/installed-plugins` 不得误判为自定义来源。

#### Scenario: Hook query
- **WHEN** 查询命中 hook 任一可搜索字段或有序字符子序列
- **THEN** hook 被纳入；空查询纳入全部，无法形成子序列时排除。

#### Scenario: Grouped navigation
- **WHEN** 在启用/禁用过滤和折叠 source group 下前后移动
- **THEN** 只返回可见 hook 索引，并跨组选择正确代表项。

#### Scenario: Source labeling
- **WHEN** source_dir 属于插件、全局/项目 hooks 或其他路径
- **THEN** 返回稳定显示标签及仅对自定义路径为真的 removable 标记。

证据：`crates/codegen/pager/src/views/extensions_modal.rs`。

### Requirement: Pager skill and workflow discovery presentation

Skills 页 SHALL 先按启用状态过滤；非空查询仅以大小写不敏感 substring 匹配 skill 的 label、slash name、说明或作者，名称/作者命中排在仅说明命中之前。插件 skill 保留显示 label 与 slash identity 两种检索入口及插件来源，workflow 仅展示符合小写字母/数字/单连字符命名约束且名称或说明命中查询的项，并以不可选择的 Workflows 分组附带 path/when-to-use 详情。

#### Scenario: Skill ordering
- **WHEN** 查询同时命中名称、作者与仅说明
- **THEN** 名称或作者命中先展示，description-only 随后，fuzzy-only 字符序列不算命中。

#### Scenario: Plugin skill identity
- **WHEN** plugin skill 以 label 或 slash name 搜索并应用 enabled 过滤
- **THEN** 同一 skill 可被两种身份找到且来源标识为 plugin。

#### Scenario: Workflow admissibility
- **WHEN** workflow 名称为空、过长、连字符非法或字符集非法
- **THEN** 不渲染；合法 workflow 进入 Workflows 分组并可显示元数据。

证据：`crates/codegen/pager/src/views/extensions_modal.rs`。

### Requirement: Pager installed plugin grouping filtering and display

Plugins 页 SHALL 按 discovery origin 映射到固定排序的 Project、User、具名 Marketplace、Direct installs、CLI override 与 Custom paths 分组；同组保持输入数据顺序，组头显示准确单复数计数并可选择折叠。首次数据到达仅一次性把已知组设为折叠，后续刷新保留用户展开状态；搜索强制打开匹配组，状态过滤移除空组，插件行显示版本、组件摘要、路径、说明和 disabled 状态。

#### Scenario: Origin grouping
- **WHEN** 插件来自任一 PluginOrigin
- **THEN** 得到固定 rank、稳定 collapse key 与显示 label，并按 rank/label/key 排序。

#### Scenario: Collapse and search
- **WHEN** 组被折叠或查询命中其子插件
- **THEN** 常态隐藏子行，搜索时强制显示；同组插件不拆组且兄弟组不受影响。

#### Scenario: Status and fields
- **WHEN** WHEN过滤 enabled/disabled 并渲染插件
- **THEN** 无匹配子项的组消失，行保留数据索引、组件/路径字段和禁用标记。

证据：`crates/codegen/pager/src/views/extensions_modal.rs`。

### Requirement: Pager marketplace flat indexing navigation and action mapping

Marketplace 页 SHALL 以稳定 flat index 统一计数、选择解析及前后导航：匹配 source 占一个 header 加其插件槽位，不匹配 source 仍保留 `plugins.len().max(1)` 个索引槽但不产生可选项；折叠 source、错误 source 和无插件 source 不暴露子项，搜索按插件名 fuzzy 匹配并强制打开命中 source。当前选择解析为 source header 或对应 source 内 plugin；键位分别映射 install、refresh、update、add source、uninstall 与 remove source，输入值去除首尾空白。

#### Scenario: Index agreement
- **WHEN** 多来源、查询、错误或折叠状态改变可见项
- **THEN** count、next、prev 与 resolve 使用同一 flat-index 算法并对同一索引达成一致。

#### Scenario: Selection identity
- **WHEN** 当前行是 source header 或 plugin row
- **THEN** 返回 source index 以及 None 或精确 plugin index，越界或隐藏槽返回 None。

#### Scenario: Marketplace actions
- **WHEN** 用户触发 i/r/u/a/d/x
- **THEN** 解析为对应 marketplace action，add-source 表单提交裁剪后的 source。

证据：`crates/codegen/pager/src/views/extensions_modal.rs`。

### Requirement: Pager marketplace catalog component projection

Marketplace catalog 行 SHALL 仅从结构化 PluginComponents 生成折叠摘要与展开字段，覆盖 skills、commands、agents、MCP servers、hooks、LSP servers 六类；展开时每类最多列八个名称并追加剩余计数。空 catalog 只在展开时显示 `no detectable components`，无 catalog 的远程项只在展开时显示安装后可见提示，本地无 catalog 不虚构内容；旧扫描计数字段不渲染，展开时用分类枚举替代折叠摘要且不重复。

#### Scenario: Collapsed summary
- **WHEN** catalog components 非空且行未展开
- **THEN** 显示结构化类别计数摘要一次，不显示旧扫描计数或占位符。

#### Scenario: Expanded enumeration
- **WHEN** 行展开
- **THEN** 按六个固定类别列名称、每类上限八项并以 +N more 收束，折叠摘要不重复。

#### Scenario: Unknown contents
- **WHEN** catalog 为空、远程项无 catalog 或本地项无 catalog
- **THEN** 仅展开视图显示适用占位语，本地未知内容保持空白。

证据：`crates/codegen/pager/src/views/extensions_modal.rs`。

### Requirement: Pager mCP server section tool selection and setup rows

MCP Servers 页 SHALL 按来源 section 对过滤后的 server 合组并发布可选择的 section header、server 行及仅对已展开 raw server index 生成的 tool 子行；首次加载默认折叠每个 plugin section 而保留 Local 展开，搜索强制显示 section children。选择解析必须区分 section/server/tool，并从最近的 `mcp-tools:{server_index}` 父行计算 tool index；错误、加载、越界或无父行返回 None。server/tool 行分别显示来源、连接/启用状态、工具计数与描述，setup 只对支持的 server 建立。

#### Scenario: Section defaults
- **WHEN** MCP 列表首次含 Local 和 plugin server
- **THEN** plugin sections 一次性进入 collapsed 集，Local 保持展开；搜索忽略 section collapse。

#### Scenario: Tool row identity
- **WHEN** raw server index 被加入 tools-expanded
- **THEN** 只生成该 server 的 tool 子行，选择子行得到 `(server_index, tool_index)`，header 与无效映射返回 None。

#### Scenario: Selectability and status
- **WHEN** picker 构建 section/header/tool 行
- **THEN** section header 可键盘选择且普通点击路径可折叠，server/tool 的 enabled 状态驱动 dim/badge 与 Space 文案。

证据：`crates/codegen/pager/src/views/extensions_modal.rs`。

### Requirement: Pager MCP plugin marketplace skill workflow and hook administration effects

The pager root executor SHALL expose list and action RPCs for MCPs, hooks, plugins, marketplaces, skills and workflows, deserialize successful extension responses into their typed view models, and return user-facing failures otherwise. Skill toggle SHALL refresh the skills baseline only after a successfully parsed toggle response. Marketplace update check SHALL list update_available plugins, apply their updates serially, collect only typed successful outcomes and notify the session when at least one succeeds. CTA install/reload/MCP probing SHALL preserve agent and plugin correlation, with fixed retry and installed-dismiss delays. MCP upsert/delete/server/tool toggles SHALL send their method-specific field conventions and report their transport result. This file does not prove extension authenticity, marketplace trust, dependency resolution, transactional multi-update behavior, reload completion, MCP process health, skill path scope or workflow execution.

#### Scenario: Malformed list
- **WHEN** an extension list response cannot deserialize
- **THEN** the corresponding Loaded TaskResult carries a generic load failure rather than partial data.

#### Scenario: Skill baseline refresh
- **WHEN** grow/skills/toggle returns a parseable skills list
- **THEN** grow/skills/refresh-baseline is attempted and its failure is ignored.

#### Scenario: Marketplace updates
- **WHEN** multiple plugins report update_available
- **THEN** each update is attempted serially and only successful typed outcomes are returned and notified.

#### Scenario: CTA timing
- **WHEN** CTA MCP retry or installed dismissal is scheduled
- **THEN** it waits the configured fixed delay before fetching or returning the timeout result.

#### Scenario: MCP mutation
- **WHEN** an MCP server or tool is upserted, deleted or toggled
- **THEN** the matching grow/mcp method receives session, names and requested enabled/config state.

证据：`crates/codegen/pager/src/app/root/effects/mod.rs`。
### Requirement: Pager task-result test: stale_workflows_result_does_not_repaint_replaced_session_modal
A workflows list result for a replaced session SHALL not repaint the current extensions modal with stale data.

#### Scenario: Stale workflows result
- **WHEN** the modal belongs to a different session than the result
- **THEN** the modal remains in its loading state.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `stale_workflows_result_does_not_repaint_replaced_session_modal`。

### Requirement: Pager task-result test: marketplace_list_loaded_sanitizes_components_at_ingestion
Marketplace component metadata SHALL be sanitized when ingested, removing control escapes and bounding descriptions before the modal stores it.

#### Scenario: Marketplace metadata sanitization
- **WHEN** a marketplace response contains escape codes and an oversized description
- **THEN** the stored name is clean and the description is bounded to the documented length.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `marketplace_list_loaded_sanitizes_components_at_ingestion`。

### Requirement: Pager task-result test: plugins_action_success_sets_result_notice_and_autoreload_preserves_it
A successful row-level plugin action SHALL set a row-anchored result notice, and a subsequent generic auto-reload result SHALL preserve the triggering notice.

#### Scenario: Plugin action notice
- **WHEN** an update succeeds and then registry auto-reload completes
- **THEN** the specific message remains anchored to the acted row.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `plugins_action_success_sets_result_notice_and_autoreload_preserves_it`。

### Requirement: Pager task-result test: tab_wide_action_success_sets_tab_wide_result_notice
A tab-wide plugin action SHALL set an unanchored footer result notice carrying its message.

#### Scenario: Tab-wide plugin notice
- **WHEN** a registry-wide action succeeds with no pending row
- **THEN** the result notice has no row index and keeps the action message.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `tab_wide_action_success_sets_tab_wide_result_notice`。

### Requirement: Pager task-result test: uninstall_result_notice_is_footer_only_not_row_anchored
A successful uninstall SHALL use a footer-only result notice because the acted row is removed, even when a row action was pending.

#### Scenario: Uninstall notice
- **WHEN** an uninstall succeeds and requires reload
- **THEN** the notice has no stale row anchor.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `uninstall_result_notice_is_footer_only_not_row_anchored`。

### Requirement: Pager task-result test: confirmation_required_builds_plugins_confirmation_with_confirmed_true
A plugin action returning ConfirmationRequired SHALL build a confirmation overlay from the exact server message, captured row, and confirmed plugin action.

#### Scenario: Plugin confirmation
- **WHEN** the backend requires confirmation for uninstall
- **THEN** the modal stores the action with confirmed=true and emits no second action.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `confirmation_required_builds_plugins_confirmation_with_confirmed_true`。

### Requirement: Pager task-result test: available_commands_refreshed_updates_generation
A nonempty available-command refresh SHALL replace the session command list and increment its generation.

#### Scenario: Command refresh
- **WHEN** one available command is received
- **THEN** the command is stored and generation advances once.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `available_commands_refreshed_updates_generation`。

### Requirement: Pager task-result test: available_commands_refreshed_empty_is_noop
An empty available-command refresh SHALL be a no-op and SHALL not advance the generation.

#### Scenario: Empty command refresh
- **WHEN** the refresh contains no commands
- **THEN** state and generation remain unchanged.

证据：`crates/codegen/pager/src/app/root/dispatch/tests/task_result.rs` — `available_commands_refreshed_empty_is_noop`。
