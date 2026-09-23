
# shell 逐文件审计

完整读取Cargo.toml、lib.rs、instrumentation.rs、heap_profile/mod.rs并登记哈希。shell是大型会话/agent组合层，入口导出auth、session、leader、tools、extensions等；不把依赖列表视为行为覆盖。默认feature空，test-support测试入口有required-features，default-bazel开启test-support；dhat-heap仅可选依赖开关，实际消费点待核对。

本轮新增instrumentation退出/宏和heap hook两项契约。heap唯一测试用fake hook，若已有安装则提前返回而不证明fake路径；未运行测试。instrumentation无本文件测试；搜索调用点确认session/agent使用但未将搜索结果计为完整调用方阅读。其余347个Rust文件中的未审部分继续pending。本轮不编译。

## active_sessions 完整阅读

完整282行实现和smoke测试，登记哈希。五个单元测试覆盖重复登记、PID划分、十线程登记、try lock争用、坏JSON；smoke顺序调用API并使用假死PID，不是真的启动/崩溃一个TUI进程。未执行测试。新增替换/收集、锁与坏文件、平台PID三项规范；non-blocking只描述try_lock，不保证其他I/O或signal-safe。调用点搜索显示TUI、headless和CLI消费，但调用方尚未据此标记已读。

## builtin.rs 生产路径阅读

首次合并输出截断，改按1–225行读取；全部生产路径已读，测试仅开头，未登记全文件hash。新增资源归属/提交marker和单文件替换/同步边界。不是只在version变化才提取，也不是多文件可见性原子事务；Unix父目录sync与非Unix不同。config/mod.rs首次输出截断，视为未完成阅读，后续须重新分段。没有执行资源提取、测试或编译。

## builtin测试闭合与memory配置入口

builtin补读226–330行，完整330行登记hash。五个测试覆盖版本变更保留用户skills、同版本修复、父路径失败后重试、四线程提取、Unixsymlink父目录；并发是同进程同版本，不证明跨版本与Registry同时发布的所有交错。未运行。cli_models完整38行登记hash，无本文件测试。

config/mod.rs重新读取1–220，MemoryConfig完整，SubagentsConfig仅开头。交叉读取agent/config.rs265–345的resolve_enabled委托，BoolFlag实现尚未核对，不据优先级注释扩大enable契约。memory节整体解析失败仍按原始子节存在阻止remote，补入该边界。两份config文件均未完整登记；无编译。

## config/mod.rs 第二段与agent/config.rs入口

读取config/mod.rs220–450，SubagentsConfig及ModelOverrideConfig完整；agent/config.rs1–180，EnvKeys完整，EndpointsConfig未完。新增3项契约，特别记录struct默认false与resolver缺节默认true、模型空白覆盖不对称、EnvKeys保留原值。catalog与深度spawn边界属于消费方，未据注释认定已核对。通用BoolFlag仍待定位读取；两文件均未完成，不登记全文件hash。本轮未运行或编译。

## config/mod.rs 第三段

读取450–700，覆盖ToolsConfig、origins、apply_sandbox、项目plugin组合及add_plugin_path开头。交叉读取config-types/flags.rs1–155，确认BoolFlag CLI优先，补齐memory/subagent enable契约。origins仅user，project plugin不合并enabled且不执行注释中的enabledPlugins；如实记录。apply_sandbox已读的Linuxbwrap/deny验证和OS条件需与SandboxSettingsConfig及sandbox消费进一步核对再补完整规范，不据一次读取认定沙箱覆盖结束。文件仍pending、无完整hash；未执行或编译。

## config/mod.rs 完整阅读结束

续读700至文件结尾，完整模块登记hash，外置tests尚未读。新增plugin列表全文件写入、postinstall/CTA读取、hook路径与行文件三项契约。添加读错默认空、无锁重写、hook参数换行/缺失尾换行和路径校验后写原串均按实现记录；没有顺带改运行代码。apply_sandbox虽已完整读，其配置解析和调用方语义核对仍待完成。未测试或编译。

## sandbox启动配置交叉核对

补读agent/config.rs380–520及207–275，SandboxSettingsConfig和resolve_string_flag完整；ModelsConfig只登记字段阅读，不推断catalog消费行为。交叉读取sandbox/profiles.rs65–130确认FromStr未知名称必为Custom，纠正可能按warning文字推断未知名直接Off的误解。补入shell启动OS分支两项契约；底层sandbox此前审计记录继续作独立证据，不把本次启动分支阅读当成跨平台实测。agent/config.rs仍未完整，无hash；未执行或编译。

## config/watcher.rs 第一段

读取1–335，ConfigFileWatcher生产实现完整，DiscoveryChange刚进入。补入过滤/分类和cwd注册边界；失败仍插入watched_cwds使同cwd重开不重试，不能照搬start注释认定下次session自动补watch。没有读取后续skills/workflow watcher及测试，不登记全文件hash。未运行文件系统监控或编译。

## config/watcher.rs 完整阅读结束

续读335–843，完整843行登记hash。六个纯测试覆盖分类、root辨识和watch计划，不执行真实OS事件、refresh重挂或Access过滤。新增discovery范围/事件优先级及新目录附加契约。SkillsFileWatcher规划按输入集合而非文件存在决定parent watch；已登记目录删除重建是否OS自动恢复不能凭集合证明。尚须读取消费方如何收到Skills后刷新workflow，不从单事件优先级推断功能完整。未运行或编译。

## config/reloader.rs 完整阅读

完整450行登记hash，新增batch/部分发布、global节diff、project hash抑制3项契约。四个测试覆盖ancestor配置hash、cwd去重、skills/model节diff、UI字段，未执行异步run、cancel、消息发送失败、panic恢复或全链路文件监听。initial effective与后续disk baseline不同，公告先更新，last-known-good日志不能解释为全事务回滚。此reloader不消费DiscoveryChange，skills/workflow事件消费方仍待定位。未运行或编译。

## config/tests.rs 第一段

完整读取1–510行，文件总2191行，尚未登记完整hash。已核对env展开3例，memory启用CLI/env/TOML/remote/no-memory优先级、默认字段、完整/部分TOML和initial injection覆盖。将6个具体测试函数映射到已有两项需求，没有为重复测试另造产品契约。测试with_env_var保存std::env::var的Unicode值，遇原non-Unicode会当缺失；memory局部锁只协调使用同辅助函数的测试，不代表整个进程环境绝对隔离。此段测试名字defaults/full不证明所有未来字段或非法配置边界；坏节fallback当前仍主要由源码证据支持。未运行测试、未编译。

## config/tests.rs 第二段

续读511–1070，覆盖remote flush clamp/local节阻断、dream覆盖、decay有效半衰期、MMR本地与远程clamp、subagent权限解析/深度来源与负值。新增6条测试证据映射，不新增重复需求。注意subagents_invalid_permission_mode_fails_fast同时测试Config::new_from_toml_cfg返回Err；与SubagentsConfig::resolve直接调用的fallback是不同入口。后续需要读取全Config解析流程，不能声称所有产品入口都静默回退。末尾env disable测试未读完，下一段从1070继续。未运行、未登记完整hash、未编译。

## config/tests.rs 完整阅读结束

续读1070–2191，完整2191行登记hash；新增12条测试引用到6项既有契约。包括subagent models-only关闭、toggle默认、模型优先级/空值、tools env、hook行文件、CTA去重及其他TOML值保留、origin、hook路径、真实plugin discovery信任切换。命名uses_active_model仅断言None，不调用模型消费；CTA preserves_other_config只保留语义值，不证明注释/格式保留。hook accepts测试创建grow_home/hooks/my-hooks，运行前需进程级隔离home；folder-trust有release模拟及全局缓存注意事项，未运行这些测试。kill-switch测试只模拟先resolve_and_record再配置读取，不证明所有生产调用方顺序。完整Config/信任消费仍待审；本轮未编译。

## bundle.rs 完整阅读

完整604行登记hash，新增5项契约。四个测试覆盖词法路径、典型解包、traversal/单项超限、修改后保留；未运行，不证明累计预算、重复条目、symlink或中途失败恢复。50MiB只累计Regular声明大小，不能称完整解压流上限；最终缺少version之前可能已写文件。词法路径校验不等于磁盘身份隔离。下载及消费调用方仍待审阅，本模块不作网络来源真实性保证。未编译。

## extensions/bundle.rs 与 remote/client.rs 完整阅读

两文件全文阅读并登记hash，补4项契约。extension四个测试与client八个测试仅静态核对，未执行。缓存新鲜不是完整性；显式sync不应用TTL或内容diff。坏manifest虽然不算fresh，但解包先读取旧manifest仍会失败，注释声称强制重同步不等于已实现自愈。HTTP成功体没有应用层大小限制。交叉读取mvp_agent/mod.rs 1420–1495，看到主动路径单agent atomic gate和成功刷新广播；整个agent文件未完整，不登记hash，gate取消/调用方及广播消费仍待审。

## Bundle 后台helper与显式刷新调用核对

全仓库hidden文本搜索（排除.git与openspec）maybe_sync_bundle_in_background仅定义及注释，未发现调用，不把重连注释当运行事实。局部读取mvp_agent/mod.rs 1357–1490、agent_ops.rs 515–535、acp_agent.rs 1985–2035及bundle路由位置、tests.rs 210–270、run_loop.rs 1900–1965、session_setup.rs 240–300与tools/bridge.rs 370–455；这些大文件尚未完整，不登记hash。remote/mod.rs全文6行登记hash。新增2项契约；广播两测试仅验证消息数量与关闭receiver，不执行刷新任务、HTTP或取消。显式刷新可触及现有session的skill baseline，与bundle扩展自身只写缓存不同。

## extensions/skills.rs 完整阅读

全文阅读登记hash，新增4项契约，覆盖add/remove/reset/list/config/toggle及workflow list。10个测试仅请求/响应serde与路径解析，未运行，不覆盖持久化、timeout、toggle、workflow gate或权限链。交叉读acp_agent.rs 2083–2102确认传入agent registry snapshot。路径canonicalize失败可保持相对路径，不能照注释承诺绝对；addedCount不是真正增量。update_config实现已定位util/config/persist.rs，完整持久化语义仍待审。

## util/config 加载及保存生产路径

load.rs 121行全文阅读登记hash；四个测试直接检查TOML字段，不调用load_config_from_toml，不证明remote.secret进入Config。persist.rs初次全文输出截断，随后明确读取1–220完整生产路径；1334行文件的测试段尚未系统补读，不登记hash。新增4项契约，区分异步save所有读错fallback和同步read helper仅NotFound空、进程内锁与文件发布耐久性。未执行测试、未编译。

## persist.rs 测试221–810

逐段读取221–810，补11条测试函数引用至既有deep merge契约，不增加重复需求。覆盖嵌套未知字段、Option缺省不写、显式false、非table替换和preferred model双解析入口；均为内存TOML操作，full_save_config_simulation未调用save_config，不能证明磁盘写入、锁或崩溃恢复。retired appearance测试仅实际断言ui_theme移除，其余两键仍为生产源码证据。CLI_CONFIG_OPTION_FIELDS是手工静态名单，不会自动发现新增字段，与其注释描述有边界。文件余下811–1334仍待逐段核对，不登记hash。未运行测试、未编译。

## persist.rs 完整结束与compaction resolver

补读811–1334，persist全1334行登记hash；resolve/compaction.rs全文及7个测试阅读登记hash。新增3项解析契约。核对共享compaction/config.rs常量为80，旧测试名85不代表断言85。typed u8层不限制100，注释自然约束不准确；环境阈值不trim而另外两个resolver trim。settings_helpers_target_correct_ui_fields直接执行测试自己的闭包，不调用真实settings wrapper。所有本轮测试仅静态阅读，未执行；wall clock测试需隔离环境，阈值测试EnvVarGuard局部锁只覆盖本辅助器且不保留非Unicode环境原值。后续继续核对实际消费及settings wrapper。

## settings_writes.rs 完整阅读

全文登记hash，新增3项契约，枚举所有writer字段组及限制。交叉核对client-support/ui_config.rs两个Option skip属性以及已读persist深合并：screen_mode空串、cancel ask不能清除已存键，静态推导未动态复现。campaigns.rs仅330–370局部，默认模型campaign完整事务待审；不把注释only sanctioned writer当全仓库唯一写入证明。文件无内置测试，未运行Cargo。

## campaigns.rs 生产路径

读取1–335并结合前段已读330–370，生产实现完整，测试后段待读，不登记全文hash。新增4项契约，区分best-effort advisory锁和成功持锁、坏状态改名意图和实际忽略错误、dismiss与config双文件非原子顺序。resolve_dismissable_campaigns层失败实际空列表，注释fallback不构成备用路径。JoinError继续写不等于外层future取消后继续写；未运行测试或编译。

## campaigns.rs 完整结束

续读370至文件末尾，结合先前1–370完成全文并登记hash。六个测试映射已有三项契约。真实临时文件测试仅覆盖坏JSON改名成功与FIFO cap，不验证锁失败、并发进程、rename失败或fsync；models_default_persist_targets_only_model_campaigns仅调用ids_touching_paths，不调用persist_user_choice/set_default_model，不能证明完整写入顺序或故障处理。未运行测试、未编译。

## util/config mod、hints、tips 完整阅读

三文件全文阅读并登记hash；初次合并输出截断后补读hints220至末尾及tips1–85以补齐缺口。新增4项契约。hints测试环境guard直接移除原环境且不恢复原值，局部mutex不代表全进程协调；tips八测试覆盖show_tips及tag解析/合并，没有执行merge_tips或debug override。注释内置tips与exclude_default语义均以实现为准。未运行测试或编译，pick_and_advance消费待审。

## tips轮换跨包核对

沿util/mod.rs的shell_base::util::*重导出定位shell-base/src/util/tips.rs并全文交叉核对；该功能已有Persistent tip rotation契约，未在shell另造重复契约。非空列表读取cursor、模选择后写cursor+1，读错回零写错忽略，当前模块不加载remote设置；文件头网络来源描述不作为shell当前配置路径证据。没有重新运行既有测试。

## 公告与权限配置生产路径

announcements.rs全文及三个测试读取登记hash，新增公告契约；permissions.rs生产路径完整输出，测试后段合并输出截断，尚不登记hash，新增两项权限契约。worktree.rs合并输出也截断，未据零散测试推断生产规则，后续按范围完整读取。公告测试无坏列表回退断言；权限display clamp总为ask与launch实际解析不同，不能混同。未运行测试或编译。

## permissions.rs 完整结束与auto_mode resolver

permissions补读330至末尾，结合前段完成全文并登记hash；auto_mode.rs全文登记hash，新增1项契约。launch gate-off测试只断言解析为Ask，不启动classifier，不证明运行时零请求。环境测试手工set/remove未恢复原值且panic时无Drop恢复；未执行测试。Auto enabled单字段提取独立于整节解析，坏兄弟字段不应被误写为丢弃kill switch。磁盘失败gate默认true与load_permission_mode失败Ask是不同入口；未编译。

## worktree.rs 1–420

按范围读取1–420，生产实现完整及type/restore测试，GC测试从420后继续。新增2项契约，交叉读workspace/worktree/mod.rs2417–2478确认adapter未知kind过滤及Never映射；workspace文件尚未完整，不登记hash。shell的remote参数入口与workspace本地启动不传remote不同，不能互换结论。未执行测试或编译，shell文件保持部分阅读。

## worktree.rs 完整阅读结束

续读421至文件末尾并确认闭合，全文登记hash，9条GC测试源码映射已有adapter契约。15个GC测试覆盖env/local/remote优先级、上下clamp、默认manual永不过期及逐kind覆盖；只检查ResolvedWorktreeAutoGc字段，不调用删除、跨平台rebuild或定时调度。clear_auto_gc_env直接删环境，不保存原值；serial只协调使用同机制测试。未执行Cargo，底层fast-worktree已审计功能不重复新增。

## resolve小模块完整阅读

mod/features/crash_handler/tool_approvals/mcp五文件完整登记hash，补3项契约。features的ZDR gate及mcp三个BoolFlag底层默认/消费仍待交叉核对后映射，不因文件全文已读宣称所有功能合同闭环；本批仅映射确定的crash/approval及MCP数值限制。上述文件无内置测试，未运行或编译。

## ZDR与MCP gate交叉核对

交叉读config-types/flags.rs36–146、agent/config.rs1630–1745、run_loop.rs790–835、agent/app.rs580–615，补2项契约；均部分大文件，不登记新hash。ZDR crates文本引用仅定义，作为已实现resolver但未发现生产消费记录。MCP liveness/restart加载effective，而app recursive gate使用load_from_disk，不从统一包装推断来源相同。未运行测试或编译，dispatcher/restart完整生命周期后续审阅。

## system_prompt与toolset resolver完整阅读

两文件全文登记hash，新增3项契约。system_prompt实际仅identity label，未有正文替换/文件读取；六测试只调用from_tiers，未验证model_id到slug两阶段回退。with_env_cleared无panic恢复且只保留Unicode旧值；env_wins测试不恢复原值。toolset无内置测试，环境policy失败返回None的后续影响仍需消费核对。未运行测试或编译。

## display_refresh.rs 完整阅读

全文含20个测试阅读登记hash，新增2项契约；测试核对分层、倒置边界、宽容字段、reason及逐时钟覆盖，未执行实际probe或显示器刷新。guard清除环境不恢复原值，仅局部mutex。公开policy可手工构造倒置floor/ceiling导致clamp panic，resolver保证排序不等于所有公开输入保证；Hz范围也未强制非零，不按默认55推断任意policy有效性。未运行测试或编译。

## resolve/version.rs 完整阅读

全文及8个测试阅读登记hash，新增3项契约。测试pure from_layers/target/floor，无真实channel cache或更新器；floor_is_semver_max_ceiling_is_semver_min_across_layers实际上只构造user单层，环境收紧由另一个测试支持。anti-downgrade只比较软minimum不比较当前安装版本。OnceLock失败None永久缓存已记录，未运行测试或编译。

## resolve/ui.rs 完整阅读

全文及9测试读取登记hash，新增2项契约。mouse三个测试假设环境未设置而不主动清理，其他测试局部mutex并手动移除环境、不恢复原值。没有测试实际鼠标协议、推理块渲染或工具分组；remote false可被更高层覆盖，不按kill switch措辞扩大效力。未运行测试或编译。至此resolve目录文件已全文阅读，整体shell包仍pending。

## util/config/mcp.rs 1–375

读取1–375，新增3项契约：项目覆盖/setup顺序、preferences持久化恢复、setup目录冲突优先级。Config投影字段及PoolConfig委托读取已看，禁用工具保存只读到开头，后续解析器/load_all/scoped加载仍待全文核对；不登记hash，不宣称trust链完整。无测试运行或编译。

## util/config/mcp.rs 360–660

续读360–660，补3项契约覆盖禁用工具、server开关及条件写入。注意禁用工具固定tmp且坏TOML清空，与toggle严格读取及unique tmp不同；用户后项目非原子，失败不回滚。upsert仅读到序列化插入，尚未完整不据注释推断后续。未登记hash、未运行测试或编译。

## util/config/mcp.rs 640–900

续读640–900，新增2项契约覆盖upsert/delete及disabled tools/access投影。delete缺entry不做附属清理；upsert移除禁用名单不改变传入enabled=false。parser只读到unknown field分支，完整parser后续继续；scoped合并实现仍需补读确认覆盖全链路。未登记hash、未运行或编译。

## util/config/mcp.rs 900–1130

续读900–1130，补3项契约并收紧此前setup目录TOML占名为有效解析定义。all_toml名称并非原始key全集，invalid条目实际不占名，与CLI注释不同。scoped实现确认只合并有效项，坏项目项不覆盖全局；此前access过滤结论针对有效项目覆盖。插件registry加载仅读开头，下一段继续。未运行、未登记hash。

## util/config/mcp.rs 1130–1320

补读1130–1320，生产路径结束，新增2项契约；原始key定位与运行有效entry目录不等价。测试读到blank transport开头，后段继续。首批测试区分disabled但无command/url仍无法反序列化，与disabled空字符串transport不是同一输入；未运行或登记完整hash。

## util/config/mcp.rs 测试1320–1595

续读1320–1595，9条测试源码映射5项已有契约。max_access测试构造已合并scope map，未执行磁盘信任或覆盖发现；project override测试手写IndexMap insert循环，并未调用实际scoped loader，不作为完整文件层叠集成证明。stdio测试只解析类型不启动进程；JSON兄弟保留测试不连接服务。下一段adds_new_servers尚未结束；未执行测试、不登记完整hash。


## MCP 配置测试续审：项目合并与 preferences

继续阅读 util/config/mcp.rs 至1910行附近，完整核对上述11个测试（后续 enabled 更新测试尚未读完）。项目新增、禁用及保留无关全局 server 三项测试手工将解析结果插入 IndexMap，并未调用项目文件发现或 merged loader，不能证明真实层级、trust 或 setup 行为。timeout 与 expose_image_base64 测试仅证明字段解析及缺省为 None，不证明实际工具调用超时或图像输出。

JSON 文件测试覆盖临时文件读取、缺失返回 None，以及 ${VAR:-default} 经 load_mcp_json_file 转为 ACP Http URL；后者假设测试变量未设置，未隔离宿主环境。known names 测试创建临时 Git 项目并断言有效 enabled/disabled 定义均被收录，但未断言集合完整性或隔离全局配置。preferences 测试使用显式临时路径，验证 Missing、坏 JSON 为 Corrupt、拒绝覆盖，以及移除坏文件后的保存重读；只断言 site 和 plugin 字段，未覆盖权限、并发、故障清理或完整对象相等。skills 两个测试只覆盖默认 paths/ignore 为空与字符串反序列化，不涉及路径展开/扫描。

本轮仅审阅测试源码，未执行 Cargo 测试，不增加测试通过数；11处来源补入既有5项契约，不新增重复需求。mcp.rs 尚未完整阅读，不登记全文件哈希，shell 保持 pending。


## MCP 配置文件阅读闭合

已读完 util/config/mcp.rs 共2049行，登记全文件 SHA256。末尾五个测试分别覆盖内存启停列表与已有定义同步、无定义名称不创建 mcp_servers、显式 false 翻为 true 后再次调用返回 false、最近项目定义解除禁用且保留注释/祖先仍 false，以及坏 TOML 被拒绝且原字节保留。最近项目测试确实创建临时 Git 仓库和嵌套 .grow 配置，调用 nearest_project_mcp_definition 与 clear_sticky_project_disabled_at；没有调用完整 save_mcp_server_enabled_in，不能证明用户写成功后项目写失败的回滚行为。再次调用返回 false 仅断言函数返回值，未直接观察文件 mtime 或系统调用次数。五处测试来源补入既有两项契约，本轮未执行测试。配置目录源码阅读完成不等于 shell 的完整功能覆盖，shell 继续 pending。


## util辅助模块

完整阅读mod/hooks/limits/subprocess四文件并登记hash，新增6项契约。mod含7个路径/tilde测试，hooks一个来源布尔投影测试，limits一个Unix宿主限制测试，subprocess八个执行测试，均未运行。子进程超时测试只断言错误类型及耗时，未证明后代PID消失；多个shell脚本测试未加Unix cfg，不能视为Windows通过证据。hooks assemble的pure注释与实际全局路径/文件读取不符；cgroup注释“any read error返回None”不适用于单字段读取。run_detached调用方、取消生命周期及Hooks上层trust产生仍需后续追踪，未以辅助模块阅读代替完整集成验收。


## util生产调用核对与bootstrap

crates Rust检索run_detached_with_timeout与shell范围git_bin/CommandLog/RunOptions，只有定义及本模块测试，未发现生产runner调用。auth/auth_provider.rs:360–470明确mint_provider_token只复用shell_c并使用run_capped；已修正两项辅助器契约的生效范围，不将注释所称共享runner推定为现状。session/actor/tool/mod.rs:470–525确认AbortOnDrop持有dispatch drainer；inspect/mod.rs:310–350确认词法路径分类用于Scope展示。session/actor/spawn.rs:1570–1615确认无registry override时才resolve_and_record信任并发现hooks，override直接采用；其他reload入口仍待完整阅读。

agent/init.rs完整阅读并登记hash，新增bootstrap顺序/Once初始化契约。init_process参数cfg当前未使用，remote变换仅path_not_found_hints；校验失败先于进程初始化，ModelsManager失败晚于初始化。源码审计未执行测试，局部读取文件不登记全文件hash。


## Auth provider实现

完整阅读auth/auth_provider.rs（中段命令构造承接前轮已读360–470），登记hash并新增4项http-credentials契约。本文件末尾为测试helper及外部auth_provider_tests.rs模块声明，外部测试尚未阅读/执行；token_output解析与trusted table来源仍待深入。PartialEq注释称反序列化必不相等过强：实现只比较name/config，空config可相等。run_capped所称whole run timeout实际在spawn/enrollment/resume后才启动；guard在resume成功后才建立，尚未将此前错误路径当作已验证的跨平台回收保证。认证helper环境只移除指定第一方变量，未据此宣称所有继承认证上下文均被清空。


## Token stdout解析

完整阅读auth/token_output.rs及内嵌3测试，登记hash并新增两项要求。测试源码覆盖时间溢出、JSON坏payload与控制字符、JWT基本格式及整数exp；本轮未运行。JSON测试用外部true命令构造成功ExitStatus，不能认为不依赖宿主命令或已验证Windows。解析接受任意非{开头且无控制字符的非空文本，不等于验证bearer真实有效；JWT仅读取第二段exp，未验签。上层mint先用解析到期再TTL再JWT，expires_in溢出会成为None并继续fallback。


## Auth provider测试：缓存与恢复

已阅读auth_provider_tests.rs前420行，完成首14测试核对；expiry_source_precedence尚未读完。14处来源关联既有4项契约。缓存与401测试通过真实计数shell helper与测试专用过期/回拨mint时间函数验证预期，但本轮未运行。并发测试tokio::join!两次ensure并断言计数文件只有一行，覆盖单provider两个成功请求，不证明跨进程、失败重试或公平性。cwd变更测试只观察cached_token为None，未尝试启动不存在目录。serde测试覆盖正常ref，不覆盖fail_closed序列化。expired-env测试假设宿主未设置GROW_AUTH_EXPIRED，代码没有先清除该变量；测试头注释“不修改process env”不等于环境完全隔离。计数helper路径插入shell时未引用，测试依赖临时路径不含shell特殊字符及Unix工具。文件未读完，不登记全文件hash。


## Auth provider测试：期限、回收与失败

续读auth_provider_tests.rs至910行，完整核对13个测试并映射至既有3项契约；下一relative program执行测试未读完。expiry优先级测试通过计数证明JSON短到期胜TTL、TTL胜JWT及仅JWT近到期重mint；overflow测试证明无可用到期仍能mint并缓存。取消测试cfg(unix)，运行shell后代写PID，drop future后轮询kill(pid,None)返回错误；它不调用suspend_until_process_group_enrolled，且未区分该探测的具体errno，不能证明所有生产启动窗口或Windows行为。timeout测试仅错误与耗时，零timeout慢例只检查MintFailed没有精确计时。stdout超限用/dev/zero验证专用超限错误。scrub测试以独立5变量列表与生产常量比较，在子Command设置值后直接调用scrub及output，未经过mint_provider_token。prior-token上下文测试首轮预期none依赖宿主未设置同名变量。两个失败刷新测试只检查cached_token不可返回旧值，未执行真实HTTP请求。resolve_program测试为路径投影，无exec。全部为源码核对，本轮未执行测试，尚不登记整份测试文件hash。


## Auth provider测试阅读闭合

auth_provider_tests.rs全部955行阅读完成并登记hash；末尾两个cfg(unix)测试分别创建可执行token.sh验证args=Some([])相对程序按cwd运行，以及通过cat token.txt验证shell命令current_dir。对应两处来源补入执行契约。累计29个测试函数已核对源码，未执行；不能据此声明Windows或真实HTTP认证链路通过。


## Auth配置解析与附加入口

核对config.rs:1080–1245、1740–1810及provider_catalog.rs:114–164，新增宽容解析和model attachment要求。警告不等于删除条目或clamp；模型列表从空IndexMap构造，仅当前config_models为基础，不能把旧“Layer 6”注释推定为仍有远端catalog层。函数本身接受传入配置，不执行trust判断；可信层加载与项目过滤仍待继续追踪。auth/mod.rs完整阅读登记hash，仅导出helper类型/常量和测试辅助函数。config.rs与provider_catalog.rs仅局部已读，未登记整文件hash。本轮无测试执行。


## Catalog认证查询与辅助模型

阅读config.rs:2320–2570，sampling_config_for_model仅开头尚未闭合。新增认证facts和辅助模型准入两项要求：主catalog中keyless仍Byok，但resolve_aux_model_sampling_config要求Some api_key；不能将端点归属与辅助调用可用性混同。first_own_credential注释说trimmed实际仅用trim判空并返回原值。stamp仅对目标service URL覆盖resolver，非service保留原值。已定位config/mod.rs重导出util/config加载器，但本轮未完成项目trust过滤全链路，不宣称已证明。没有新全文件hash，也未执行测试。


## 认证按id查询的配置来源闭合

复核shell config/mod重导出→util/config/mod→campaigns.rs:124的load_effective_config→config loader.rs ConfigLayers::load及load_from_disk，确认try_resolve_model_credentials与with_resolved_model仅读用户grow_home/config.toml，经环境展开、版本覆盖与本地campaign应用。未遍历项目祖先、无cwd配置回退，因此前轮“项目过滤仍待核实”在这两个查询入口应收敛为根本不加载项目层；已更新现有契约并附3处来源。其他外部Config构造及项目模型配置入口仍不由此证明。重读load.rs仅确认TUI配置投影，未重复登记hash。源码核对，无Cargo执行。


## Provider catalog生产解析

承接已读auth_config_issues，现完整阅读provider_catalog.rs生产部分至测试模块；第一测试仅开头，测试尚未完整审阅，不登记文件hash。新增层级解析与默认继承两项契约。关键边界：options类型错误整组默认、provider层未知字段整体失败、模型非空map整体替换、任一模型认证字段阻断整个provider认证组。inline auth即使被显式引用遮蔽仍登记合成name。警告不代表配置被拒绝或helper一定执行，最终取决于模型认证覆盖。无测试执行。


## Provider catalog测试闭合与准入

provider_catalog.rs八个内嵌测试已完整阅读，登记全文件hash。覆盖层级catalog、同wire名不同provider独立凭据、keyless准入、空catalog拒绝、旧flat模型形态忽略并warning、旧reasoning scalar忽略、多个reasoning default拒绝和global default精确身份。没有HTTP或helper执行，product_credentials测试名宽于断言：只构造keyless配置并检查None，未注入产品凭据。没有map继承或inline auth冲突的测试，不能据这八项证明所有分支。结合config.rs:1288–1352及1845–1851实际校验函数，新增LLM准入契约；trim default用于校验但不修改原配置，精确lookup不trim。所有测试仅源码审阅。


## 模型到Sampler与ACP投影

阅读config.rs:2540–2671，新增Sampler字段及ACP元数据两项契约。sampling_config_for_model总将bearer_resolver置None，不因为model附有auth_provider就自动建立动态解析器；需沿session caller另核对。inject_url_derived_headers保留值仅针对精确大小写键，与先前global extra_headers的case-insensitive存在性检查不同；alpha_test_key形参当前丢弃，不能依据注释称会注入非生产access header。ACP meta基础两字段使is_empty分支实际不走None。未执行测试，未登记config.rs整文件hash。


## Session认证刷新及resolver附加

阅读session/actor/turn/sampling.rs:705–950，确认reconstruct_full_config通过.map(AuthProviderRef::bearer_resolver)方法指针附加provider resolver；仅检索bearer_resolver(会漏掉此生产引用，不能据该检索声明无生产caller。route resolver优先，预刷新写回检查revision，401 helper局部写回不传revision。model_auth_state的确定memo会直接返回，不每次重读磁盘；尚需沿invalidate调用确认刷新时机。重建采样先在revision循环内组合route与chat-state，后续compaction summary读取不在循环内；没有据此宣称整体函数无竞态。新增2项契约，未执行测试且未登记整文件hash。


## 会话认证刷新上层分支

核对turn/sampling.rs:1660–1720与2040–2110，补齐refresh_byok_credential的route前后检查及错误恢复分支；401也包含Auth kind，provider分支失败不会退入无provider的missing-credential普通重读。恢复结果携带error.credential传上层，重试预算最终消费逻辑仍待核查，不将函数注释once当作预算已验证。另读535–705工具定义阶段，确认ContextRecall取决于last compaction prompt、delegated goal隐藏GoalLifecycleUpdate、Workflow同时检查captured/live behavior，但完整工具过滤caller尚待后续建模。本轮未执行测试。


## 认证恢复预算

完整阅读auth_retry.rs并登记hash，结合turn/mod.rs:1320–1395核对消费位置；新增Missing独立50次与Sent/Unknown三次退避预算要求。两个内嵌测试只验证1/2/4及缺失不计拒绝，未覆盖51次、时间漂移或8次重置；未执行测试。图像投影及native continuation重置成功分支显式reset预算，尚未遍历所有reset位置。model_switch.rs:545–567、800–825确认两处route replace后invalidate_model_auth_memo；只是已找到的调用点，不代表热重载全链路已验证。


## 认证预算重置与route匹配

检索turn/mod.rs全部auth_retry_schedule引用并阅读935–978、1260–1395上下文：每turn新建，model_changed/首次overflow重提/图像投影/成功continuation reset/Steered/正常Response均重置，单agent_changed该分支不重置。由此不能把3次与50次描述为整个turn总上限；现有契约已补充精确范围。sampling.rs:2110–2131补全reload_api_key_from_config：base_url或auth_scheme与route不一致时不刷新，无key也不清除chat-state。仅修正文档，不将此行为扩展为代码整改。


## 每步工具定义投影

承接sampling.rs:590–702已读部分，补读session_mode.rs:1–49及tools registry/types.rs:1378的builtins-only实现，新增工具可见性契约。builtins-only名称并非基于ToolIdentity namespace，而是client_name不含双下划线；不把名字筛选描述成完整安全授权。MCP等待耗时不包含prepare inner。Workflow同时受captured/live behavior过滤，Plan另有按名字过滤。相关文件均只局部阅读，不登记全文件hash，未运行测试。


## 图像辅助路由准入

阅读sampling.rs:90–260，完成unsupported查询/mark入口、resolve_image_description_route及通知构造，新增路由准入要求。project_conversation_images_for_text_model_once只读到缓存命中shadow开头，剩余生成/故障/持久化尚待读，不能据函数注释宣称完整恢复链已验证。已见materialize失败AcknowledgementLost、无图group空报告及辅助路由成功后才起算240秒预算，后续逐group与提交边界待继续。无全文件hash，无测试执行。


## 图像shadow生成与提交

续读sampling.rs:250–565并结合前轮已读565–590，完成该投影函数及三次SurfaceChanged包装器源码核对，新增完整覆盖及Sideband证据契约。失败日志称omitting group不意味着最终允许丢图：unresolved非零会阻止整次projection提交。240秒预算不能覆盖磁盘恢复与Sideband结算，provider timeout还使用group开头的remaining；不据注释宣称严格wall-clock上限。Sideband结果可先持久化而projection后失败，缓存/持久恢复是独立阶段。底层恢复键、cache及Timeline apply仍依既有/后续模块审计，不以此声称运行验收完成。无Cargo测试。


## ImageDescribe缓存及envelope

阅读image_describe.rs:1–230，完成prompt/envelope/cache/恢复wrapper，persist_user_images尚未读完。新增缓存身份与文本封装契约：URL digest不能泛称图片内容hash，model身份不在key中，未发现容量/TTL；body保留换行但tab等ASCII控制删除，尖括号替换并非原样转录保证。spawn_blocking恢复wrapper不设timeout，底层选取完成Sideband规则待追踪。仅局部阅读，无文件hash或测试执行。


## 图片资产与请求构造

续读image_describe.rs:225–420，生产代码完整，测试只完成资产保存及source-local prompt，PDF/cache测试尚未读完。新增资产保存与请求构造要求。资产保存测试只断言消息片段及目录项数，不检查字节相等、权限、批次回滚或symlink安全；尚未运行。无效MIME回退扩展名不等于编码转换。多图request实际将全部URL按序附于同一User，温度与output预算交由后续路径决定。文件仍未完整读完，不登记hash。


## 图像描述模块阅读闭合

image_describe.rs全部源码及18个内嵌测试已读，登记hash；新增17处测试引用至既有两契约。多图测试断言两张Image内容与revision变化缓存miss，不真正渲染PDF或比较图像顺序。资产字节测试确认保存内容相等，remote uri测试确认inline bytes优先（单零字节也接受，非图片解码验证）。Unix symlink assets测试断言InvalidData且外部目录空；不覆盖并发替换或Windows reparse。空输入测试确认不创建assets。封装测试覆盖ASCII控制位置、尖括号、段落及普通Unicode，不代表所有Unicode控制或prompt注入均被消除。本轮全部仅源码核对，未运行测试。


## 持久图像描述恢复选择

阅读storage/jsonl/mod.rs:455–635，完成恢复函数和read_sideband_ledgers_from_directory。新增反向spawn候选、Completed边界、revision/SurfaceId/证据区间匹配契约。读取所有parent引用Sideband，非候选的读取或校验错误也可阻止本次恢复；caller前轮已见记录warning并转新描述请求。result.source_event_seqs[1]依赖先行ledger验证，尚未在本轮展开统一validator，不将索引直接推定为已确认bug。没有新whole-file hash或测试执行。


## 图像恢复Result引用前置校验

核对storage/mod.rs:2164–2280开头与chat-state sideband.rs:210–224、601–646：加载器先SidebandTimeline::from_events再validate_parent；source_event_seqs为固定[u64;2]而非Vec，Result校验要求[0,last_attempt]，不存在此前待确认的长度索引疑点。记录已有attempt、唯一Result、非空raw/finish及证据范围约束，补2处来源到恢复契约。统一validator后半还含compaction等跨账本检查，当前仅读到replacement匹配中段，不宣称该大函数已完整阅读。无新文件hash、无Cargo测试。


## 跨账本投影验证闭合

续读storage/mod.rs:2270–2461，结合前轮2164起已读完成validate_sideband_ledgers，新增摘要/图像/标题投影一致性契约。摘要允许规范前缀后双换行附加段落，不能写成全文逐字等于Result；图像则精确重封装相等。标题Generated只验证Result完成引用而不在此比较文字，Fallback要求带error的Failed/Cancelled；上层标题生成规则待单独审阅。不登记大文件全hash，无测试执行。


## Sideband完成读取及中断恢复

读取storage/mod.rs:1634–1683与jsonl/mod.rs:633–658，检索283–288确认recover_sidebands开关。完成结果要求Completed紧随且为ledger末尾；中断恢复逐份Durable追加Cancelled，Result已写并不自动算Completed。新writer所有权获得方式及load具体入口仍待展开，不据开关注释证明全局唯一writer。另读append_session_title_durable入口，但bookkeeping失败处理待继续核查。无全文件hash或Cargo运行。


## 轻量加载及Summary协调

阅读jsonl/mod.rs:195–330、2721–2803，新增校验/恢复/摘要协调顺序契约。recover_sidebands=false不意味着load_light_data完全无写入：标题/模型协调不受此bool控制。标题同seq冲突拒绝、落后才修复；模型无canonical selection直接保留summary。ensure_writer_lease仅核对调用在前，具体获取与复用语义仍待阅读。无大文件hash或测试运行。


## Writer租约与过期会话准入

阅读jsonl/mod.rs:2325–2445，完成lease名称/获取及cleanup遍历。缓存命中直接成功，busy跳过；parent定位与底层open属性仍待核对，delete只读尾部不能视为完整删除审计。未实际删除session或执行Cargo。首次文档脚本语法错误未写文件，修正后完成写入。


## 会话隔离删除

阅读jsonl/mod.rs:2235–2338并结合前轮尾部至2360，完成delete入口及隔离/identity/清缓存流程。新增实际错误边界：rename后sync仅warn、部分恢复错误被忽略、递归中断无回滚、清理锁文件报错时实体可能已不存在。底层remove_all_contents的handle规则尚待展开，不将高层检查视为所有竞争窗口已证明安全。没有执行删除命令或改动会话数据，未编译。


## id-only查询及实体身份

阅读jsonl/mod.rs:2100–2257，新增查询遍历/错误筛选/重复id/物理身份及缓存规则。cwd目录打开错误全跳过与目标session错误通常传播不同；可能跳过无法读取的候选，不能称全存储唯一性扫描无遗漏。普通路径边扫描边缓存，后遇重复返回错误并未清前次缓存；shared_read不入缓存。bound路径刻意复用已验证句柄，不因Summary后损坏阻止canonical写入。未运行测试，未登记大文件hash。


## 普通打开与绑定句柄区别

阅读jsonl/mod.rs:1990–2102，补齐open_session：缓存只持目录，命中也重读Summary并校验请求id/cwd，和bound_session_directory命中直接返回不同。首次Summary通过才入cache，避免把两个入口统称缓存绕过Summary。另完整阅读collect_updates：迭代错误逐行跳过，首个及最终汇总warning，非仅尾部容错；read_updates_from_directory缺文件空，其他open错误传播。UpdatesIterator具体错误/边界仍待核对，暂未新增其完整契约。无Cargo构建。


## updates提交边界与容错

阅读storage/mod.rs:2580–2705和1944–2080，结合前轮collect_updates，新增完整行/超限排空/offset/错误跳过要求。stream_position注释“next unread/EOF”对未提交尾部不准确：保存最后完整行边界，reader可已消费尾部。未结束尾部再次继续同iterator与从保存offset重开不同，当前未宣称iterator可自动拼接稍后追加。SessionUpdate serde下半仍待读；未登记文件hash或执行测试。


## SessionUpdate封装

续读storage/mod.rs:2690–2895，完成SessionUpdate serde与Envelope实现，新增wire/disk区别与from_value/from_str边界。BorrowedEnvelope无timestamp字段，不能声称两入口对元数据同样严格；两者返回SessionUpdate都丢弃时间戳。PersistedData/Light结构已读，Light缺updates/rewind_points而保留timeline及派生投影。CopySessionOptions仅声明读到strip_reasoning，尚不能依注释认定fork实现覆盖。无测试执行或全文件hash。


## updates追加与bookkeeping

阅读jsonl/mod.rs:1880–2008、1496–1548，新增update追加错误分类和补换行规则。NotCommitted包装所有append错误，不等于磁盘没有发生改变；底层写入或durable sync失败可发生在字节已写后，需后续独立故障审计，不能在文档中宣称exactly-once。已有坏尾不是truncate而补换行，这可能使其成为读取器完整坏行再被跳过。read_timeline局部还确认固定TIMELINE_FILE读取，name提取不用于选另文件。无测试执行。


## JSONL追加锁

阅读jsonl/mod.rs:1669–1738，完整核对lock_append_contained：5秒deadline只从锁文件打开后起算，10ms竞争重试，不是整个追加事务上限。将NotCommitted/实际写入不确定性独立登记backlog，尚未沿调用方证明重复bug。build_and_publish_session_opened只读前半至sync_tree，待续，不登记全文件hash。无测试或构建。


## Staging发布事务

续读jsonl/mod.rs:1735–1818，结合前轮1700起完成build_and_publish_session_opened及build_publish_and_cache。新增build→sync_tree→no-replace发布→parent sync顺序与成功后sync仅warn的契约，和updates追加全部错误NotCommitted的边界分开。创建失败清理是否可能碰撞其他实体需底层create_child确认，当前不据名称随机性或包装注释证明。底层publish handle实现尚待独立核对。无测试执行。


## 版本化JSONL读取

核对jsonl/mod.rs:1810–1955、590–630及storage/mod.rs:1877–1943，补充版本必须u64精确匹配、完整记录严格解析和缺失文件调用方差异。版本化读取先收集全部RawValue，随后版本/类型检查；后部JSON损坏可先于前部版本不匹配被报告。版本错误序号基于集合enumerate，不等同物理行号。未执行运行时测试，未登记未通读文件hash。


## Timeline拥有的Workflow恢复候选

阅读jsonl/mod.rs:2440–2725，完成load_workflow_runs_sync，新增候选截断不回补、cleared零字节读取、manifest降级与不可访问跳过差异、immutable脚本和args的Timeline hash验证。resolve_workflow_restore_manifest内部仍需续读，未将其未核实语义扩入契约。read_optional_json_sync仅发现定义，无生产调用证据，不将宽松JSON读取描述为实际恢复入口。无Rust测试或构建。


## Workflow manifest事实协调

阅读workflow/store.rs:1–145、280–485，完成resolve_workflow_restore_manifest、冻结字段比较与from_restored装载路径。validate_reconciled_projection只修改clone，不能将selector返回manifest描述为已经协调；from_restored才协调真实state。open lifecycle固定Interrupted/Completion/process_interrupted。后续persist实现和manifest decoder仍待续读，本轮不登记整个文件hash或运行测试。


## Workflow注册、回执和codec

续读workflow/store.rs:125–280、485–550，结合前轮已读280–485，补齐register/persist/persist_ack/remove、manifest codec与hash实现。内存模式不施加文件模式大小限制；register分锁检查并非原子唯一注册，暂不推断实际并发调用bug。persist_ack等待actor回执的具体落盘范围仍需沿consumer核对。本轮不添加全文件hash，测试未读完也未执行。


## Workflow actor落盘与清除

阅读workflow/store.rs:550–735、persistence.rs:2080–2140及jsonl/mod.rs:3240–3278，闭合persist_ack到storage结果链。写入cleared或完全相同revision均成功无操作；clear先标记后删state，仅局部事务，未声明删除整个运行目录。锁超时从打开state.lock后起算。store生产函数已读至测试模块入口；测试尚未完整读，不登记全文件hash。无构建。


## Workflow store完整源码与测试证据

读完store.rs共1300行，登记SHA256，补充16项测试函数证据。acknowledged_persist_returns_storage_failure由模拟consumer回传disk full，不是真实ENOSPC注入；并发revision测试只有两个线程一次竞争，不证明任意调度或跨进程可靠性；immutable测试只改便捷script.rhai并检查0000和内存缓存，不证明操作系统不可修改。unsupported_manifest_is_not_restored传入None Timeline，首先验证缺失spawn拒绝，不单独证明版本判断。恢复测试使用占位hash直接调用from_restored，不能替代JSONL恢复入口hash检查。所有测试仅阅读，未执行。


## 完整加载与rewind延迟读取

阅读jsonl/mod.rs:3268–3405并复核validated_timeline及updates入口。完整load验证Sideband但不修复其中未结束任务；标题/模型协调在updates和rewind读取前，可先写后失败。append_rewind_point明确Buffered，不能按消息已处理声称durable落盘。load_light(false)已有契约，本次补齐完整load对比与独立rewind接口；无全文件hash或运行时测试。


## Rewind intent存取

阅读jsonl/mod.rs:3405–文件尾，persistence.rs:295–350、2140–2162与常量，actor/rewind.rs:377–430。补齐16KiB限额、FilesOnly顺序豁免、typed解码先于version校验与回执错误传播。recover_pending_rewind仅读入口，尚未据注释宣称恢复幂等。load_prompt_records/open_timeline_reader包装已读，底层reader仍待后续。无构建或全文件hash。


## Pending rewind重放与补偿

阅读actor/rewind.rs:425–565，完成recover_pending_rewind及apply/rollback文件函数。target短路不mark_reverted；FilesOnly无prompt分支校验；desired按已有points顺序first-wins而非本函数显式排序。补偿仅覆盖成功changed列表，失败操作本身可能副作用与底层FS实现相关。next_points先于Timeline提交，跨崩溃重放仍需沿FileStateTracker和merge实现验证，未据注释宣称整体幂等。无测试或全文件hash。


## Rewind历史加载跨crate核对

阅读workspace/src/session/file_state.rs:318–380、415–470、590–650、710–745。get_rewind_points对内存集合排序；ensure_historical_loaded同步读取失败只warn返回，外层Vec接口仍返回已有内存数据。与recover_pending_rewind连用不能证明历史完整性，replace_rewind_points随后清空lazy_source。注释never operating on partial set不足以证明调用链事实；独立登记backlog，不在本审计修改runtime。merge稳定排序并按prompt去重，target=0清空，只有target-1存在才合并后续快照，否则丢弃后续集合；before first-wins、after last-wins。workspace未全读，不登记hash或完成状态。


## resident历史修复准入

阅读actor/rewind.rs:560–文件尾，复核chat-state actor/mod.rs:574–604、handle.rs:423–442；补充本会话标记两次检查、dry_run同样拒绝和报告日志条件。底层修复算法已有chat-state契约，本次不重复生成算法需求。shell rewind文件前半未完整读，不登记全文件hash。未执行运行时测试。


## Rewind picker投影

阅读actor/rewind.rs:1–175，完成picker/count/window函数。picker以chat snapshot范围为准，snapshot缺失空列表；file counts单独无该范围限制。handle_rewind前半确认Goal存在即拒绝、FilesOnly豁免conversation index校验，后续预览尚待续读，本次不提前定义完整请求契约。无构建或全文件hash。


## Rewind正常提交完整路径

续读actor/rewind.rs:175–377，结合前轮覆盖完成整个文件并登记hash。预览success始终false，force允许冲突但仍先读取；正常提交对points/Timeline失败有补偿，与recover_pending_rewind不同。UI marker失败时Timeline已提交且intent保留。after快照不存在与删除快照均None。成功reverted_files包括无需写入路径。未执行测试，整体崩溃一致性仍待验证。


## Rewind持久化测试证据限定

读取jsonl/tests.rs:1635–1740，完整阅读intent roundtrip/双clear及snapshot roundtrip两个测试；读取rewind_cross_compaction_tests.rs:255–324完整补偿测试。前者测试临时目录写后读及文件不存在，不是断电durability实验；后者模拟consumer对intent返回Ok而不实际落盘，对ReplaceRewindPoints回传注入错误，断言文件回到after且tracker保留一项，不覆盖真实ENOSPC、部分写、Timeline失败或pending intent磁盘状态。三项测试引用已补到对应契约，全部未执行。其余截到的测试不记完整覆盖，无hash登记。


## Pending rewind恢复测试边界

完整阅读rewind_cross_compaction_tests.rs:324–441两个测试。rolls_forward手动写intent并在同一个已装配actor上调用recover，before快照直接放内存，断言prompt=1、文件before和tracker空；没有模拟进程死亡、重开lazy ledger或检查intent文件删除，不覆盖前述历史加载失败风险。版本测试写入当前版本+1的完整typed结构，验证错误链component/persisted/current，不能证明非法结构与异版本并存时版本优先。新增两个引用，未运行测试、未登记部分测试文件hash。


## Rewind后的compaction派生状态测试

阅读rewind_cross_compaction_tests.rs:441–文件尾。run_clears_marker_scenario在seed_compacted_timeline后回退到3，再显式设置Dynamic(true)，断言last compaction marker=None及重建header=1；不能推断所有provider配置均发该header。context_recall测试覆盖未开始、Started未结束、Failed、seed完成与回退前分支后的工具定义可见性，不覆盖执行授权。seed辅助函数前部仍需全文阅读，当前引用属于已读调用/断言证据；测试未执行，无全文件hash。


## 跨compaction测试全文件闭合

读完rewind_cross_compaction_tests.rs:1–255，结合前轮后半完成全文件并登记hash。seed_compacted_timeline手动提交Started/Sideband/ Summary/replace/Completed并追加prompt，不运行真实summary采样，也未在该辅助函数构造Sideband ledger文件。回退场景断言system、UI0、P0–P2精确保留；FilesOnly越界且无快照仍成功，ConversationOnly越界拒绝。file counts只验证直接填充的内存快照2/1及缺项，不覆盖lazy磁盘扫描。新增4个辅助函数证据引用；全部测试未执行。


## JSONL初始化准入

阅读jsonl/mod.rs:3100–3244并复核2648–2707。新增已有会话仅验证返回、NotFound创建、prepared staging内容顺序及发布后lease失败不回滚边界。wrapper的Buffered/Durable选择和summary patch转交已读，底层patch字段语义仍待独立核对。copy_session_data_sync尾部仅读返回，不记完整覆盖。无测试或全文件hash。


## Summary patch合并语义

阅读summary_write.rs:1–255，完成apply_patch及锁包装read_modify_write。标题相等seq忽略并非冲突错误，updated_at非单调，bool仅title应用状态；锁无5秒timeout，与Workflow state.lock不同。lineage.apply_to及底层write_summary_atomic仍待续核对，未扩写其语义。文件测试未读，不登记hash或执行测试。


## Summary写回durability与并发测试

阅读summary_write.rs:255–370，storage/mod.rs:662–744、1463–1488签名、2149–2164。write_summary_atomic传durable=false/replace=true；serialize_summary先验证format并限制pretty字节大小。并发测试两个共享adapter任务各300次patch，断言计数300、branch=main、activity Some；没有逐步断言时间单调，也不能据注释称所有调度下必定发现去锁错误。测试仅源码阅读，未执行。底层write_atomic后半与Windows完整实现仍待核对。


## Contained文件发布durability

续读storage/mod.rs:740–785、1485–1558，结合前轮662–744、1463–1488完成Unix/Windows write_atomic主体。目录sync在发布成功后仅warn，区别于文件sync在发布前失败；Unix no-replace成功link后的unlink不检查，不能声明无tmp残留。Windows helper内部权限/rename细节尚待续核对，未将Unix预检查泛化。无运行时测试或全文件hash。


## Summary patch全文件测试核对

续读summary_write.rs:370–文件尾，结合此前1–370完成全文件并登记hash。补充4测试引用：Unix summary与lock symlink拒绝且外部字节不变；Generated seq7标题；User9拒绝Generated8；两task seq11/10竞争25次收敛11。这不是User来源优先级测试：胜者同时具有更高seq；不能声明User永远覆盖Generated。测试直接调用projection修复，不证明对应Timeline/Sideband事实存在。未执行测试。


## Staged fork复制主体

阅读jsonl/mod.rs:2818–3108，完整核对copy_session_data_sync、fork_filter_surface和prompt blob复制。control/announcement来自完整source Timeline最新投影，不是target截断位置；inherit_control不控制announcement继承。blobs复制发生在strip_reasoning前。截断/cwd/update转换helper仍需核对，未据注释扩写实现。无运行时测试或全文件hash。


## Fork update截断与通知ID

阅读storage/mod.rs:2955–3075及jsonl/mod.rs:2790–2828。updates target按User run计数，不直接比较promptIndex值；hostTurn重置run且不计数。转换仅notification顶层session_id，过滤仅列出的6类Grow投影。surface截断与cwd helper由crate::sampling导入，尚待定位实现。本轮修订已有契约，不新增数量，无测试或hash登记。


## Fork cwd与surface helper跨crate核对

复核sampling-types/conversation.rs:3361–3408、3501–3585。cwd helper字面replace而非JSON结构变换或路径边界替换；tool arguments注释safe不能证明含引号/反斜杠target仍保持有效JSON，当前只记录实现，不扩张安全保证。surface helper有legacy/markers-only/progressive选择，与updates计数区别已写入。helper下层之前sampling-types审计记录可供后续核对，本轮未补读其全部细节或更新hash。


## Surface截断分支闭合

复核sampling-types/conversation.rs:3398–3501，结合上一轮3361起完整覆盖截断helper链。legacy/progressive忽略首个非synthetic User作preamble，不是首个任意User；starts_prompt_turn synthetic可在此前计数。markers-only不验证连续或单调。已将具体规则补入fork契约，不重复新增sampling-types能力，未执行测试或更改hash。


## Authority缓存与parent目录

阅读jsonl/mod.rs:1–40、330–425、728–790，结合已读authority函数。缓存命中不重解析root；clone共享Arc，独立构造不同缓存。updates_snapshot_len缺失传播与updates加载缺失空集合不同。ensure_cwd_marker内部尚待续读，本轮仅记录调用边界。无测试或全文件hash。


## Cwd marker创建链闭合

阅读jsonl/mod.rs:675–728、790–865，完成ensure_cwd_marker与session_directory。marker字节精确比较、AlreadyExists专门处理，非canonical路径比较；Explicit最终名字由配置路径给出。scan_opened_sessions只读入口至cwd枚举，后续身份拒绝与重复处理仍待续读。无测试或hash登记。


## 会话枚举完整准入

阅读jsonl/mod.rs:850–960完成scan_opened_sessions/list_sessions_sync/recent。候选读取失败跳过，但cache canonical重读失败传播；hidden在候选上过滤，缓存句柄重读后不再筛hidden。重复id在可选cwd筛选之后检查，不能宣称任何全局重复均阻止特定cwd枚举。recent limit只截最终集合，不限扫描IO。无测试或全文件hash。


## Timeline追加bookkeeping

阅读jsonl/mod.rs:958–1138，完成追加异步包装和summary事件映射。Timeline成功后summary失败仅warn，区别于updates的Committed错误；不增加num_messages。底层序列感知追加读到prefix mutex前，未依注释认定完整幂等保证，待续读。无运行时测试或全文件hash。


## Timeline prefix与尾部重试

续读jsonl/mod.rs:1135–1260，结合1090起完成append主体。目录sync错误在追加中传播，与原子发布包装不同。尾部截断早于prefix/seq检查，失败可能已删未提交尾；同seq要求原始字节完全匹配。prefix缓存先更新再sync，不能将返回错误视为无记录。load_timeline_prefix仅读到循环入口，内部折叠和限额待续读。无运行测试或hash登记。


## Timeline prefix重载完整核对

阅读jsonl/mod.rs:1245–1330，完整核对load_timeline_prefix。每行JSON+Timeline.accept验证，空行不跳过；哈希含换行原始字节。record buffer有界但Timeline fold仍保留事件，不能称总内存固定。consumed检查可捕获短读，不证明任意外部并发修改均被检测。Sideband append仅读至尾部截断前，待续。无测试或全文件hash。


## Sideband尾部追加

续读jsonl/mod.rs:1325–1430，结合1290起完成append_sideband_line_sync。同seq重试与父Timeline相似，但无prefix hash或全ledger折叠，不能泛化父追加历史防护。读尾函数只读到usize转换，后续读取和反向扫描待续。无运行时测试或全文件hash。


## 账本尾部扫描资源边界

阅读jsonl/mod.rs:1425–1498、1617–1635，结合1390起完成read_timeline_tail与反向扫描。8KiB固定buffer不限制总扫描距离，超长未提交尾仍可线性读；最后完整行先限额再分配，换行占预算。空完整末行不是None。通用append路径包装已读，底层补换行已在前轮覆盖。本轮修改两项既有契约，无运行测试或hash登记。


## Prefix stamp字段核对

阅读jsonl/mod.rs:40–150，完整核对LedgerFileStamp。Unix有inode/device/ctime，Windows未在本结构存file ID；modified失败None。缓存快路径的依据是metadata，不是每次内容hash，避免将foreign write必改stamp注释扩写为所有平台保证。OpenedSession timeline_events错误包装仅读前半，待续。无测试或hash登记。


## Strict envelope export

阅读jsonl/mod.rs:140–195，完成timeline_events错误包装和update_envelopes。exact committed envelopes指保留JSON Value，不等于字节镜像；from_value校验后返回原Value。坏完整记录与replay容错不同，零起index是迭代条目。暂未追踪全部export调用方，未据方法注释扩展整个导出事务保证。无测试或全文件hash。


## Prefix cache key与测试wrapper

阅读jsonl/mod.rs:425–468、1545–1618。生产prefix按id/NUL/cwd持久共享，而测试append_timeline_line_sync每次构造None；后续审计测试应区分有无跨调用缓存。append_jsonl_line_sync_with是测试专用路径和注入sync闭包，并非生产contained函数本身，不能仅凭其通过证明生产句柄路径实现。无运行测试或全文件hash。


## Dormant标题与测试锁补读

阅读jsonl/mod.rs:658–684、1630–1675，完成append_session_title_durable及测试lock helper。标题先读Timeline重建再prepare，并不调用validated_timeline全引用验证。测试锁以with_extension(jsonl.lock)命名，生产锁追加.lock，非jsonl扩展输入会不同；测试timeout可注入而生产固定5秒。仅补充已有契约和测试适用限制，未执行测试。


## Lineage summary映射

阅读persistence.rs:1160–1218，完成SessionLineage.apply_to：四字段覆盖，parent prompt允许清空，forked_at仅非new且原为空时设置独立now，不将subagent_seed写入summary。validate_current_format只读标题校验前半，待继续，不扩写完整结构验证。本轮无测试或hash登记。


## Summary格式与展示

阅读persistence.rs:1193–1338，完成validate_current_format/new/展示helper。validate只校验format及title结构，不含cwd generation或物理身份；source合法不证明ledger引用。标题长度检查原字符串chars而非trim后。hidden显式false覆盖subagent前缀。new已读字段初始化，但git metadata/label lookup内部待核对，当前不扩写其语义。无运行测试或全文件hash。


## Summary隐藏测试证据

阅读persistence.rs:1330–1445，完成is_hidden_tests模块与head roundtrip/optional defaults两个测试。补充三项hidden测试引用，覆盖subagent/subagent_fork/subagent_resume、普通None/fork/worktree和显式true/false。reasoning effort序列化测试仅None省略与Xhigh往返，head测试直接serde不经过decode_summary验证，不能推断无版本数据可恢复。relocation metadata测试仅读输入，未记完整覆盖。测试均未执行，文件hash不登记。


## Summary decoder版本门禁

阅读persistence.rs:565–630，完成decode_summary/read_summary_in_session_dir。先Value版本后再次原bytes typed parse；decoder无字节预算，读取包装负责bounded。错误路径注入只作用decode结果，版本错误保留分类。most_recent helper仅读入口，待续。无运行测试或全文件hash。


## Resume sandbox profile选择

阅读persistence.rs:619–720及402–414、552–565，完整profile查找链。显式ID失败不fallback cwd，最新无profile不找更旧；best-effort与Result列表入口错误策略不同。sessions_root实取parent后由adapter固定sessions子目录。prompt blob前缀提取仅读循环前半，待续。无测试或全文件hash。


## Prompt blob识别与冻结

阅读persistence.rs:705–830，完成reference提取、bytes验证及两freeze入口。固定行首且lowerhex，不解析通用URI；原bytes hash不保证UTF-8。路径freeze逐项重开session，directory版本才复用一个句柄。写初始集合只读包装器，集合匹配实现待续。无测试或全文件hash。


## 初始blob集合与Timeline引用验证

阅读persistence.rs:830–930，完成初始集合写入和verify_timeline_prompt_blobs。集合先精确相等，但逐项hash/写入不是全部预验证再写，后项失败可有先前文件；外层staging负责清理。Timeline验证遍历全部Messages而非当前surface，并先sampling_evidence验证。materialize路径版已读，directory版仅到OpenOptions，待完整核对后写契约。无测试或hash登记。


## 请求prompt导出生命周期

续读persistence.rs:925–1008，结合880起完成materialize两入口及PromptBlobExport。directory版readonly+读回hash，持TempDir守护生命周期；中途错误可留下items部分替换但临时目录随局部对象drop，不应重用失败request。readback使用无界std::fs::read，当前仅记录范围，不把同进程生成文件等同任意外部修改下总预算。immutable writer仅读参数校验前半，待续。未运行测试或hash登记。


## Immutable writer冲突与同步

续读persistence.rs:1000–1060，结合970起完成write_immutable_blob_to_directory。通用函数无filename hash验证或readonly设置，immutable是no-replace加同内容验证。已存在分支parent.sync传播，区别于新发布write_atomic目录sync告警成功。PendingCwdSwitchReminder结构已读但其语义调用链未审计，不据结构推断实际提醒行为。无测试或hash登记。


## Summary serde字段完整核对

阅读persistence.rs:1057–1163完成Summary字段。显式default/skip约束补入规范；字段注释中source workspace仅worktree等不是decode验证。last_active仅durable活动注释也不能覆盖已有Buffered bookkeeping事实，以代码路径为准。未将元数据用途注释当作所有恢复调用行为。无测试或全文件hash。


## 本地会话resolution

阅读persistence.rs:413–552，完成exists、repo candidates、跨cwd Option/Result和directory查找包装。repo注释称检查restored children，但实现仅同ID精确cwd，规范按代码记录；SameRepo由候选位置命名，不验证git。错误降级路径与Result区别明确。无测试或全文件hash。


## 模型选择事实与控制回执

阅读session/persistence.rs:1–303、storage/jsonl/mod.rs:2774–2800及actor/spawn.rs:710–765。确认生成器、严格选择折叠、较窄回执提取和summary协调的不同保证；首条from无外部基准验证，回执提取不等同完整事实验证。恢复admission内部冲突逻辑不在本次片段范围，未作额外推断。新增独立契约；未运行Rust测试，未登记全文件hash。


## 模型恢复测试覆盖核对

完整阅读storage/jsonl/tests.rs:4096–4273的四项模型测试：summary修复测试从Timeline追加切换事实后load_without_updates，并再次读盘断言新model和High；缺字段测试仅给to_model_id，断言加载InvalidData；传输断续测试保持model/provider等不变，仅伪造第二条from endpoint，断言连续性错误；回执测试仅单事件有效intent、None effort，断言Sampling target。它们不覆盖全部字段拒绝分支、首条from与summary不一致、空intent提取、重复intent或真实进程重启。上述是测试源码覆盖证据，本轮未执行测试，不能据此标记场景运行通过。


## 持久化错误ACP分类

完整阅读persistence.rs:2170–2287及此前1–95的类型化版本错误链遍历。版本类型优先，普通kind/OS分类仅看外层错误；文本版本提示不等同类型化错误。四项测试分别覆盖StorageFull、普通InvalidData、直接类型化版本和io::Error包装版本；没有实际制造ENOSPC/EDQUOT，也未验证Windows OS错误码。测试源码已读，本轮未执行。worktree touch部分仅读开头，待核对后续调用与等待逻辑。


## Worktree活跃刷新调度

阅读persistence.rs:1983–2005、2288–2415和2440–2475，追踪shell/session/worktree.rs的workspace re-export及workspace/worktree/mod.rs:746–754。记录打开等待2秒、超时任务继续、JoinError忽略和任意持久化流量触发一小时间隔的事实。它不是常驻timer，也不是GC锁；本次未审查DB touch实现及GC全链，不据注释推断成功写入或竞争完全消除。未运行测试，无全文件hash登记。


## Light resume绑定来源与actor启动

完成persistence.rs:2423–2508及actor_channel、noop构造的阅读。区分adapter轻量数据加载和shell返回PinnedRewindSource，记录claim_writer=false仍会打开rewind ledger，非NotFound错误可阻断恢复；只有writer路径启动actor。打开句柄不等于验证延迟ledger或冻结内容。未运行测试、未登记全文件hash。


## 删除结果与TTL入口

阅读persistence.rs:2510–2650及storage/search.rs:361–364。删除结果只确认本地adapter删除，搜索enqueue无完成确认；Once正常错误返回亦不重试。TTL正i64直接as u32截断，4294967296变0是语言转换事实，未执行真实清理。adapter cutoff使用Duration::days(i64::from(ttl_days))；具体删除筛选沿既有adapter审计，不将入口注释扩成新的保证。未运行测试。


## 清理生产调用与skip参数

核对acp_agent.rs:45–67、665–705两处生产调用及jsonl/mod.rs:2410–2445筛选。initialize无skip、load带当前目录但均后台不等待；Once赢家由执行顺序决定，不能保证load的skip覆盖initialize。adapter仍检查时间并尝试writer lease，不能仅凭调度认定实际误删。TTL越界风险具备生产入口，但真实启动/恢复竞争需独立验证。未执行清理或测试。


## Agent名称序列化测试

结合前次2650起片段，续读persistence.rs:2715–2763，完成agent_name_persistence_tests六项：名称roundtrip、缺失默认None、None省略、Some输出、四个名称样例及带标题/模型的JSON解码。测试直接serde解码，不等同decode_summary完整格式校验或实际会话恢复；没有证明agent配置仍存在或目录变动后的选择。2765–2792测试fixture直接写summary与可选.cwd，不创建Timeline，不当成完整会话初始化。后续session_exists_tests仅开头已读，尚待完整审阅。未运行测试。


## 会话查找测试范围

完整阅读persistence.rs:2793–2916两个模块共10项测试：exists覆盖根缺失/空、有效summary命中、文件冒充目录、不同ID和两个cwd不同ID；summary查找覆盖缺失、无匹配、保留head字段和坏JSON返回None。多cwd测试使用不同ID，不证明重复ID拒绝；没有覆盖repo候选顺序、权限错误Result传播、有效目录中的伪造身份或完整Timeline恢复。fixture只写summary，因此存在查找通过不等于可恢复会话。随后sandbox测试fixture及首个显式ID样例已读，其余待续。未运行测试。


## Persistence尾部测试完成

续读2970–3343，完成sandbox profile八项、cwd查找五项、repo候选三项和actor生命周期一项。sandbox测试覆盖显式profile/None、空ID无cwd、updated排序、坏summary跳过、last_active优先、hidden过滤及无会话；不验证OS sandbox应用，也未测试显式ID缺失时有cwd不回退、最新会话无profile不回退旧会话。cwd测试区分真实summary与images stub；repo测试两个有效同ID目录仍按候选优先，与全局重复ID拒绝不是同一入口。repo候选测试不创建Git仓库，不能证明仓库归属验证。actor测试仅最后一个handle drop后rx None，不启动actor、不验证pending flush或多clone。测试源码已读未运行；文件其他中段仍未全量覆盖，不登记全文件hash。


## Durable句柄错误分类

结合1660–1735续读1735–1813，完成durable update/Timeline句柄接口。update dispatch与ack丢失分开，Timeline仅io错误；没有timeout。retry_exact依据内部错误永久性，不依据提交变体；底层NotCommitted准确性沿既有backlog，不从句柄分类反推磁盘未写。Error空实现不暴露内部source；是否影响调用方诊断需另行核对，不推断已发生诊断丢失。未运行测试。后续merge仅读到空chunk判定，待续。


## 通知合并与pending drain

结合此前1830起片段完成1860–1982：空chunk未检查Text及顶层meta，合并采用incoming顶层字段，无长度上限。drain依据storage提交分类恢复或丢弃pending，注释称sync不意味着本函数另做sync。普通actor Update失败后的替换顺序尚待下段核对；未推断所有失败均无丢失。未运行测试。


## Actor分派与flush确认边界

阅读persistence.rs:1995–2105：普通ACP切换旧pending写失败不恢复，区别于drain_pending；Grow直接写且不排空ACP。FlushAndAck单位值不传播flush错误，Sideband继续写且仅回其结果，Timeline不drain。确认接口名字不能证明所有先前更新已持久化。记录源码路径，尚未执行故障注入；Timeline索引/标题通知分支已读，后续可补独立投影契约。


## Timeline追加后通知

核对persistence.rs:2032–2069及actor/summary.rs:243–285，记录精确索引触发事件集合、gateway缺失、标题meta及确认不等待下游。不将fire-and-forget解释为成功送达。summary.rs:1–145也已阅读到provider超时分支，生成后半段仍未覆盖，不登记全文件hash。无Rust测试执行。


## 标题生成与采纳顺序

结合前次1–145阅读，续读actor/summary.rs:140–242完成生成实现。区分route恢复分支、activity拒绝后route消耗、Sideband终态和canonical标题采纳；失败不等同立即重试。标题parser/schema/fallback helper细节及schedule调用时机仍需对应证据，不据“first real user”注释证明调用约束。未运行provider或Rust测试。


## 标题helper完整阅读

完整阅读helpers/session_title.rs并登记hash。14项既有测试覆盖backend格式、未知字段、UTF8截断、提醒剥离和fallback词数/skill显示；未执行。单独strip未闭合测试不证明title_source_text不会回退原始提醒。fallback前10词不保证160字符，schema词数仅description，本地parser不强制。未运行模型请求或Rust测试。


## 标题调度生产入口

核对turn/admission.rs:765–837、tool/dispatch.rs:550–572及run_loop.rs:1398–1409。User origin和直接命令均调度；直接命令先durable用户消息，source取prompt Text不取执行输出。手工标题先消费route，即使commit失败也不恢复，不直接取消已运行任务。admission更早输入分支未在此完整复审，不将局部顺序扩成全部输入模式证明。未运行测试。


## 标题actor文件覆盖确认

当前actor/summary.rs共282行，前序已分段阅读1–145、140–242及243–文件末尾；不存在末尾测试模块。现登记完整文件SHA-256。生成、采纳及通知已映射到对应契约，但尚未运行行为测试，不能因文件覆盖而标记shell crate完成。


## Persistence独立测试文件

完整阅读persistence_tests.rs六项测试并登记hash。两个disposition测试只匹配枚举，不执行sync；失败drain测试probe拒绝两次Buffered追加，确认durable路径未到达；FIFO测试读回两文本及summary计数2；noop测试仅update Unsupported。CurrentModel测试使用FlushAndAck排序后读盘断言模型变化且effort/agent保留，并非仅凭ack证明成功。ActorGuard以abort停止，不覆盖自然channel关闭时flush。未运行测试；普通不可合并切换失败、Grow重排和flush失败确认仍无此文件回归覆盖。


## Summary中段测试覆盖

阅读persistence.rs:1440–1658，完成relocation roundtrip与title_projection_tests五项。旧cwd形状测试同时缺destination_cwd，单凭失败断言不能区分缺必填与未知字段拒绝。旧title测试直接serde拒绝后另用decode_summary断言版本类型化错误，支持版本预检优先；部分title只有title字段明确validate失败。display来源测试不读取Sideband，不能证明引用完整性。head None省略测试断言带条件，不能当成所有环境均进入两个断言的证明。未运行测试，未登记全文件hash。


## Summary构造完整核对

阅读persistence.rs:1183–1193、1253–1290，补齐同步元数据收集、独立时间采样、空default model以及可选字段初值。没有根据Result返回类型推断构造存在当前错误分支；Git/worktree helper内部错误降级规则待各自代码证据。未运行测试。


## Actor消息分派尾部与关闭

完成persistence.rs:2100–2170，结合先前CurrentModel分支核对剩余变体。非Ack操作只日志，带Ack仅传storage结果且忽略接收端消失；均不自行flush ACP pending。自然channel关闭仅一次flush，失败恢复的pending随actor退出丢弃，没有重试循环。先前actor_lifetime测试只验证rx关闭，不能证明尾部flush。此处静态审阅，未执行故障注入。


## Prompt blob身份构造边界

复核persistence.rs:688–707，补齐两个纯构造helper的原UTF8字节hash、无写入/限额检查。对照既有覆盖记录确认1330–1445测试此前已审阅，不重复计新增测试覆盖。整体文件是否完整仍以分段证据核对，暂不新增全文件hash。未构建。


## Timeline channel桥接

完整阅读timeline_persistence.rs并登记hash。两项测试仅断言消息variant：ack测试未发送或await确认，不能证明durability、ack传递或失败分支；flush测试只接收入队消息。桥接不负责永久错误分类，发送失败显式返回预填io错误receiver。未运行测试。


## ACP SDK MCP反向桥接

完整阅读session/acp_mcp.rs并登记hash。三项测试覆盖坏注册条目、同名保留首条、缺meta；未测试reverse调用超时或响应验证，未运行测试。AcpServerEntry字段校验由其类型负责，本模块未额外校验名称或serverId内容，不从注释推断IPC机制或远端取消。


## MCP启动shell wrapper

完整阅读session/mcp_servers.rs并登记hash，无本地测试模块。此文件负责override与两批clients组装，不是客户端/全局server定义合并入口；底层启动并发、timeout实际执行由mcp crate负责，未从wrapper推断。ACP names与build分两次锁，配置读取在锁外。未运行测试或启动server。


## NotificationSender完整阅读

完整阅读notifications.rs并登记hash，无本地测试。两个durable方法仅持久化，不使用gateway gate；Sideband所有storage错误包装NotCommitted，区别于update保留分类。底层sync错误可在写入后出现，包装不能证明未提交；是否影响调用者重试待独立验证。未运行测试。


## Goal通知投影

完整阅读goal_notification.rs并登记hash，无本地测试。明确通知与持久化独立、tokens_used原样传递、cleared哨兵及耗时格式。发送器无gateway_enabled不等于所有调用方未做门控；调用时机仍需上层证据。未运行测试。


## TurnCompleted纯构造

完整阅读turn_completion.rs并登记hash。文件只做载荷构造，不负责续行或持久化；四项测试直接传JSON，不调用prompt_complete_fields，identity/usage均None，不能证明生产错误映射或计量字段完整性。未运行测试。


## Extension结果封装

完整阅读session/result.rs并登记hash。文件是扩展RPC结果封装，不是turn完成分类。单项测试覆盖success/failure JSON字段存在及null，未覆盖partial、with_data失败和to_ext_response；注释称error字符串或结构体但字段实际任意Value。未运行测试。


## Replay wire标签

完整阅读wire_tags.rs并登记hash。单项测试固定五标签与两前缀，不运行真实回放或对抗性JSON；派生器先to_value后取tag，prefix自行拼接，其注释不证明任意字段顺序支持。未运行测试。


## Replay事件与flush请求

完整阅读replay_events.rs并登记hash。此文件是内存事件类型及请求helper，不解析存储JSON。单项测试名称虽含flush_replay_actor，实际只手工发送flush_with_ack并回ack，没有调用timeout wrapper或真实actor。timestamp仅上界且普通加法，未验证生产可达溢出；调用方额外合并约束待核对。未运行测试。


## Replay窗口调用方前段

定位唯一生产调用agent/update_chunk_merge.rs并阅读1–155。ReplayBuffer先计算session ID相等与时间窗口；不同ID时取出pending和incoming一并返回且计数归零，不能将底层窗口宽松误述为跨会话可合并。时间越窗分支替换pending返回旧通知，但此分支未重置pending_count/pending_bytes；后续计数/合并逻辑待续，暂不判定实际影响。BufferingSettings派生Default为零，与serde缺字段默认100/2048/10不同，需追踪初始化调用后写完整契约。本轮未运行测试。


## ReplayBuffer计数与阈值续读

阅读agent/update_chunk_merge.rs:155–285完成consume主分派、merge协议分派及flush。正常合并路径取prev_count后清零，next_count饱和加1，next_bytes重新estimate合并后载荷，>=阈值立即返回且缓冲为空；不可合并两条立即返回。越窗分支在这些计数重建之前提前返回，因此保留旧计数，已确认源码事实；影响需结合测试及payload estimator继续审阅。flush取出pending并归零；不同协议不合并。文本macro仅开始读，不把部分实现写为完整规则。本轮未构建。


## Replay metadata续读

阅读update_chunk_merge.rs:280–440，完成merge_meta和chunk ID范围辅助函数，ACP match尚未读完。两侧顶层meta存在时只处理new.chunkId并保留prev其他键，不更新timestamp/token等；已有非数组chunkIdRange时新ID不写入。连续ID将标量或单元素范围扩成二元素范围，范围末端+1用普通加法，不去重/排序；字符串ID允许parse u64。文本macro无annotations时合并并保留prev Text/chunk meta，区别于持久化merge要求meta=None。这些为局部源码事实，待完整ACP/Grow分支后汇总契约；未运行测试。


## Replay合并生产实现闭合

续读430–605，完成ACP/Grow分派、same_tool_call及payload估算。Grow同ID优先于index且清meta；估算不含元数据，检查在合并分配后。结合1–430写入完整生产合并契约；测试模块仅fixture开头已读，不登记全文件hash。未运行测试。


## Buffering初始化配置

阅读acp_agent.rs:124–141及run_loop构造定位738–740。缺失/非法配置均None（非法告警），空对象按serde默认，与派生Default区别有实际入口证据；显式零不验证。此处不推断已存活actor是否热更新，计时器执行逻辑尚待完整核对。未运行测试。


## Replay定时flush调用链

阅读run_loop.rs:734–757、1077–1087，确认固定max(20,2*duration)周期入队，不是随chunk重置deadline。FlushReplay先await emit_buffered后可选ack，无pending也ack；未进一步推断emit_buffered送达/持久化保证。配置u64乘法无checked保护，极大值影响待独立验证。未运行计时测试。


## ReplayBuffer首批测试

阅读update_chunk_merge.rs:605–750完成六项测试：首条计数/字节、第二条合并、max_items等值flush、越窗、连续ID建range和扩展已有range。越窗测试名称称does_not_buffer但断言pending仍Some；旧hello与新world同为5字节且原count=1，所以不能验证越窗重建计数正确，恰好掩盖前述保留旧计数事实。尚未阅读后续测试，未运行测试。


## Chunk范围测试续读

阅读update_chunk_merge.rs:750–927完成六项测试：连续1–4压单范围、1/2/4保留间隙、尾单值4加5转范围、尾单值4加6保留分段、1/2/4/3保持到达顺序及flush归零。均使用正常数值ID，尚未证明重复、字符串、溢出或坏范围行为。后续streaming测试仅开头已读，不记完成。未运行测试。


## Streaming与Grow测试续读

阅读update_chunk_merge.rs:930–1054完成四项测试。文本flush测试只手工flush与空第二次flush，不执行计时器也不比较meta。grow_same_id实际后续两条ID为None，覆盖index fallback及保留首name，不覆盖两侧Some相同ID不同index。different_tool_call_id测试同时改变ID与index，不能独立证明ID优先规则；bufferable测试断言pending/count=1。fixture Grow meta均None，未证明合并清除已有meta。后续跨协议测试待续。未运行测试。


## ReplayBuffer文件阅读完成

续读1054–1070完成最后跨协议测试，断言ACP旧条和Grow新条立即返回且pending清空。全文件1070行已分段阅读，登记hash；共17项测试，未发现本文件对settings=None、max_bytes阈值、跨session、annotations、非法meta、缺timestamp或算术极值的直接回归。这里是文件内覆盖范围，不等同全仓库无测试。全部测试仅审阅未运行。


## Buffered通知落点

阅读actor/updates.rs:250–395，完成emit_buffered、日志、direct和transient入口。ACP实际入持久化队列，不能沿replay_events顶部注释宣称所有buffer输出无持久化；Grow分支不写盘。两者日志先于gate检查，日志sent不证明送达。context pressure只读构造前段待续。未运行测试。


## 临时压力通知与flush_to_disk

阅读actor/updates.rs:370–455，完成context pressure和flush_to_disk。压力通知内层meta标grow/contextPressure，顶层meta为totalTokens、当前毫秒、transient=true，走transient不持久化、不补event ID。flush_to_disk先等待replay wrapper（失败告警后继续），再发FlushAndAck，发送失败和ack取消均忽略且无Result；第二次等待无timeout。结合已审计FlushAndAck不传播写错误，方法名与注释不构成成功持久化保证。extract_update_info仅读前段待续。未运行测试。


## 更新日志及Grow处理前段

阅读updates.rs:440–535，结合414起完成extract_update_info：message/thought/user只类型名，ToolCall带ID/title及Debug kind/status，ToolCallUpdate可选status，Plan/commands数量、CurrentMode ID，其他None。build_notification_meta只生成eventId与当前时间。Grow处理入口注释称SubagentProgress在store前返回，但已读代码仅抑制该类型debug日志，随后仍normalize meta并无条件入队；后续分支待完整审阅后形成契约，不能依据注释断言transient。SubagentStart hook在入队后await且错误传播，后续goal分支尚未读完。未运行测试。


## Grow处理后段与retry入口

续读updates.rs:530–650完成handle_grow_session_notification：SubagentProgress确实return，但发生在此前普通持久化入队之后；无gateway转发，SubagentSpawned hook成功后仅当前goal ID匹配且runtime可用才emit GoalUpdated，不在此读取progress作为usage。persist_update_only单独加meta入队、失败告警，无ack。Notification hook生成UUID v7 cause并await observe hook。send_grow_notification遇当前prompt的Failed/Exhausted先缓存并返回，Retrying清旧pending再发送带promptId；finish_sampling_failure后段尚待续。未运行测试。


## 采样失败通知owner完成

结合590起send入口，阅读634–660完成finish。错误缓存单槽覆盖，owner不匹配保留，匹配成功恢复也消费，failed布尔由上层给定。transient家族及completed hook projection读至717，forward下层尚未审计，不提前声明全部无hook。未运行测试。


## Grow durable发送重试前段

阅读updates.rs:718–830，完成audit/passive/exact及persist-only durable入口。passive复制同一meta给live不同update，先exact append成功再forward；exact重试固定通知clone，biased cancel优先，retry_exact允许时每100ms重试，无次数上限，首轮和每10次告警，次数饱和。取消返回NotCommitted Interrupted，但取消发生于已入队等待时不能证明磁盘未写；需按语义保留不确定性。response usage构造只读前段待续，forward仍未完整阅读。未运行故障注入。


## Grow构造与forward闭合

阅读828–930，完成metadata覆盖、hooked/unhooked forward。不同于buffer Grow路径，unhooked本身无gateway gate；不推断外部调用者全无门控。临时普通Grow也可触发hook，passive避免hook。acking测试fixture只自动ack Timeline，不能称全部持久化模拟成功。测试模块仅开头待续，未运行测试。


## Sampling失败通知测试

完整阅读updates.rs:929–1010的sampling_failure_notice_waits_for_exact_turn_outcome：三组合failed/resumed，缓存期无Grow队列和live Ext通知，stale owner不消费，Retrying清缓存，两次finish只产生期望数量Failed live通知并校验promptId/eventId。测试手动设current_prompt并手动finish，不是provider恢复或真实重启；末尾没有断言最终失败通知已写盘。两个后续fixture已读，仅观察channel，persisted命名不代表磁盘。未运行测试。


## Actor事件标识与行为切换FIFO测试证据

阅读updates.rs:1043–1347，完成三个测试源码核对。actor_persisted_grow_lines_carry_event_id覆盖own emission、meta缺失的inbound、persist-only三入口，断言test-actor前缀及相邻两组ID不同；没有断言全部三者两两不同，也没有现有ID保留或调用方覆盖测试。emit_notification_direct_stamps_unstamped_acp_lines以CurrentModeUpdate验证缺少meta时补ID。两者观察PersistenceMsg队列，不是文件落盘或重连集成验证。

behavior_current_mode_update_rides_event_pipeline_in_id_order在已有文本后请求plan，检查模式通知未绕过event队列、队列共3项且模式通知已有ID；手工调用ReplayBuffer/emit_buffered后观察文本和模式两条持久化消息及数值ID递增。退出使用两次normal请求，队列共4项，第三项是normal模式，随后读取3条ACP并断言ID递增。AvailableCommands不进入持久化的依据还需结合生产实现：辅助函数只读到指定数量，并未在读完后断言队列没有多余消息。此测试手工排空事件并驱动buffer，不运行真实actor事件循环、客户端去重或磁盘恢复；abandoned分支也未在此覆盖。三个测试均仅阅读，未执行。


## 行为确认、Goal窗口与合成唤醒测试边界

完成updates.rs:1347–1737文件尾部六个测试的源码审阅。interrupting_behavior_switch_parks_then_confirms_on_second_request先进入Plan，再请求normal取得ConfirmationRequired，断言提示包含Select it again to confirm及remaining_ms在7500–8000，立即同目标再次请求得到Applied；没有推进时钟验证过期、异目标切换、Pager按键或Enter/Esc语义，断言报错文字中的Enter/Esc不构成覆盖。behavior_switch_rejects_non_idle_foreground_without_surface_append手动设置Plan和RegularTurn stub，要求normal被拒、Plan不变、pending为空、conversation序列化前后相同；未检查所有foreground种类或持久化队列无写入。

completed_goal_receipt_survives_every_behavior_switch_until_clear创建并完成Goal，分别选择normal及ask后仍Complete，最终Behavior为Clarify。名称中的every与until_clear超出实际测试范围：没有遍历全部Behavior，也没有调用clear或恢复磁盘。goal_usage_follows_active_pause_restart_windows手动选择Goal、同步窗口并记录匹配goal ID的380 tokens；pause后同步窗口，记录None归属的120不计入；restart后同步再记录匹配ID的25得到405。此处restart是tracker状态转换，不是进程重启；未隔离验证暂停状态下仍携带匹配旧ID的计量。

synthetic_prompt_behavior_tests两个测试先进入Plan，第二个额外停放normal切换，然后spawn_local调用SubagentCompleted/Internal的handle_prompt，轮询turn_behavior为Plan后abort，检查Plan保留及第二个pending仍Some。两个调用实际都显式传BehaviorId::Plan，与邻近注释所描述历史硬编码Normal不同；不能据此证明错误Normal参数被覆盖，也未覆盖bash/monitor/workflow等其他origin。轮询只检查状态值，未等待handle_prompt返回或完成provider调用；未断言pending目标、期限或身份前后相等。以上六项均只阅读源码，未执行Rust测试。updates.rs前部仍有未审范围，不登记全文件hash。


## updates前部闭合

完整阅读1–249，连同此前250–1737覆盖全文件并登记SHA256。新增出站入队契约，区分ACP完整meta、Grow buffered无meta和模式内外层meta，Edit路径处理仅词法strip_prefix而非安全路径验证。前部usage辅助：record_subagent_usage仅Some parent pin与当前相同才归prompt，空by_model且非incomplete跳过chat-state调用；_subagent_id未用。mark_apply_miss_incomplete对无pin但有live仍标prompt incomplete，session始终标记，返回sticky或ledger任一成功；finalize仅fail_closed才标双方ledger，随后取report标记snapshot并清sticky，不论snapshot是否Some。这些usage辅助已阅读，后续仍需接合既有usage契约，不能从函数名推断持久化成功。未执行Rust测试。


## 入站Grow与响应usage契约补齐

复核470–590及806–840，将此前已读事实形成两项delta。明确SubagentProgress注释与实际入队顺序不符；入站方法不校验session_id，ensure ID使用actor session身份但原通知身份不在此替换。此为函数边界，不能据此宣称外部可注入跨会话通知，调用方准入仍需独立证据。ResponseCompleted只是无副作用字段投影，饱和减法不代表provider计数已验证。未运行测试。


## Event ID跨crate实现核对

确认shell util以pub use重导出shell-base，未另有本地event_id实现。完整复核shell-base事件ID辅助及五个测试源码，补入入站契约非null任意JSON ID保留、缺失/null ID生成、已有timestamp键保留的语义。已有ID测试只检查字符串值不变，未断言缺失timestamp不会补齐；其他类型ID及null时间规则来自实现，不冒充已测。五个测试未执行。


## Subagent usage归属与ack入口

结合updates.rs前部与run_loop.rs:2033–2081形成归属、sticky、ack契约。读取turn/mod.rs freeze入口确认默认120秒传给drain后finalize，尚未读完drain不能推断硬时限。subagent_usage_fold_tests.rs:1–86首测试覆盖匹配pin的40 input计入prompt，随后三组失配/缺pin/无live只计session，合计160；直接调用辅助函数，没有运行命令ack路径或注入chat-state失败。当前仅源码审阅，未执行测试，也未登记未读完整的测试文件hash。


## Usage drain与报告快照实现

阅读turn/mod.rs:49–87、149–289。UsageDrainOutcome.report_incomplete为fail_closed/background_live/sticky_report逻辑或；from_outstanding_reply在None时fail_closed，Some按live_ids非空决定fail_closed，另外两标志照抄。drain先计算Instant deadline，再每轮await outstanding查询；None立即fail_closed，live_ids空立即成功返回，即使查询结束已超deadline也不会转timeout；仅仍有live_ids时检查deadline，继续则固定sleep 50ms。因单次查询尚待追踪，不能把max_wait或默认120秒描述为整个调用的硬超时。

snapshot_prompt_usage_marked先分别swap(false,Relaxed)消费actor/shared两份unattributed后台标记，再await prompt ledger，合并调用方incomplete、两个已消费标记及ledger.incomplete；查询错误调用project_from_ledger(None,true)。标记消费发生在查询之前，查询失败也不恢复。error_path_usage_fallback则先查询outstanding并转may_undercount，再读取ledger交给for_error_path，不在此消费这两个后台标记。两个projection具体None/零值规则待核对下层。

mark_subagent_usage_not_applied优先显式prompt ID，否则当前live ID；无可用ID时将actor/shared后台标记均置true但返回false。有ID但无subagent_event_tx或send失败返回false，不设置这两个兜底标记；send成功await oneshot，未设本地timeout。由此上层SessionOnly命令虽忽略返回bool，仍可能等待ack不返回，不能把“忽略结果”解释成无等待。本轮仅阅读源码，未执行测试。


## Outstanding查询等待与PromptUsage投影

核对settlement.rs:123–168，outstanding本地确无timeout，无通道返回默认reply，发送/接收失败None；clear为无ack队列消息，finalize调用clear不能证明协调器已清理。notification.rs:1–137显示project_from_ledger(None,true)保留默认计数的不完整报告，None,false省略；for_error_path有ledger时始终设incomplete。scrub在整体incomplete或totals partial时清除totals与每model cost ticks，只有totals partial才同步model partial；Self::from与剩余类型仍需沿既有映射继续核对。未做挂起故障注入，等待无上限为源码控制流事实。


## PromptUsage wire与headless字段闭合

续读notification.rs:138–367，结合前部完成PromptUsage/Model转换与headless投影。注意is_token_empty不验证计数一致性，reasoning-only可被视为空；cost零值注释不对应字段skip规则（仅None省略）。project_result_usage只赋值，不移除旧JSON键，早返回也保留已有usage/cost；需沿caller核对是否总构造新result，不能直接宣称已发生旧费用泄露。attach_result_usage_fail_closed仅读开头，后段及文件测试仍待审；本文件不登记全hash。未运行Rust测试。


## Headless usage解析失败与调用方初核

完成notification.rs:369–381解析包装，完整阅读1765–1844三个投影测试。partial/incomplete测试用新空对象，验证费用省略和uncached计数；空incomplete不写usage/num_turns；解析字符串失败保留ok并写incomplete。均未测试预先带cost对象，不把省略费用推断为清理旧字段。pager/headless.rs:279–298及350–363的普通JSON成功/错误入口均新建result后附usage，当前两个调用处没有复用已有费用对象；其他reducer调用已定位但尚未完整阅读，不扩大结论。未运行测试。
Luna/high子代理完整读取shell crate的Cargo.toml、383个Rust文件、1个Python runner和3个bench，共385个文件；已有189条delta保持原契约并补全来源，新增301条源码事实需求，消除了pending_mapping。完整文件哈希与test/bench属性计数已登记；未运行Cargo、测试、bench、Python runner或外部服务。shell仍跨越启动、session、JSONL持久化、IPC、终端、配置、MCP和认证，拆分债务保留在backlog。
