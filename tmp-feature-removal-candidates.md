# Grow 非必要功能删除候选（临时，含执行状态）

R1–R11 已收到逐项决定：R1 保留；R7 因无法排除外部初始化调用而谨慎保留；其余 9 项按对应验证记录逐项清理。R12–R32 已收到第二批决定：逐项清理；R21 改为实现历史对话搜索，R27 改为实现明确的图片降级流程。执行记录分别见对应 OpenSpec change。下列证据保留发现时的背景，当前执行结果以每项末尾和验证记录为准。

## R1 · 未接入生产的 FileOperationLockManager

- 位置：`crates/codegen/tools/src/implementations/editor_infra/file_operation_lock.rs`，由同目录 `mod.rs` 导出。
- 作用：按路径串行化文件操作，并提供阻塞所有路径的独占锁。
- 当前证据：对 `crates` 下 Rust 源码检索 `FileOperationLockManager|file_operation_lock`，只命中自身实现、单元测试和导出。没有发现生产实例化或工具调用；旧审计也记录了同一结论。
- 删除理由：未参与实际文件编辑，却维护另一套锁队列和异步释放协议。旧审计确认其取消窗口可能泄漏锁，保留会形成未来误接入的维护负担。
- 拟删除范围：该实现、仅服务它的测试和导出；目录内其他代码需要在执行删除前重新检查引用。
- 影响与限制：当前仓库未发现调用者，不能推断仓库外没有使用方。按本项目不考虑后向兼容的原则，无需为该公共导出保留兼容层。
- 发现时状态：列为候选，等待逐项核准。


- 当前决定：按用户要求保留，用于后续多 Agent 协作需求评估。

## R2 · Shell 的空 unstable 编译特性

- 位置：`crates/codegen/shell/Cargo.toml` 中 `unstable = []`。
- 当前证据：Shell 源码、脚本、CI、Cargo 配置和架构文档内未发现对应 cfg 或启用点；不转接依赖，也没有启用任何功能。与 Pager 为统一构建约定保留的空 `jemalloc` 不同，这里未找到明确保留理由。
- 删除理由：暴露了没有行为的编译开关，容易让人误以为可开启实验功能。收益很小，优先级低于 R1。
- 拟删除范围：仅此 feature 声明。执行前再次检查全仓构建引用；不删除任何实际实验实现。
- 发现时状态：列为候选，等待逐项核准。

- 用户批准后已删除；Cargo 元数据回归确认仅移除该空 feature，其他声明未变。

## R3 · 未接入执行路径的 ZDR access 开关

- 位置：`crates/codegen/shell/src/util/config/resolve/features.rs` 的 `resolve_zdr_access_enabled`，以及 `crates/codegen/config-types/src/lib.rs` 的 `RemoteSettings.zdr_access_enabled`。
- 证据：全仓检索 `zdr_access_enabled|GROW_ZDR_ACCESS_ENABLED|resolve_zdr_access` 仅命中字段、resolver 本身；resolver 经 `resolve/mod.rs` 通配导出，未发现生产调用或测试调用。
- 删除理由：保留了一个看似控制准入、实际没有接入执行的开关，容易误导维护者。
- 拟删除范围：resolver、对应远程字段，以及只服务该 resolver 的空模块声明/导出。执行前再次核对全仓与生成配置引用。
- 限制：这里只判断该开关的调用状态，不据此推断任何实际数据保留政策或外部服务行为。
- 发现时状态：列为候选，等待逐项核准。

- 用户授权后的执行结果：已删除并完成对应回归；详见 clean-approved-milestone-candidates 的 R3 验证记录。

## R4 · 未接入生产的问答 ID 格式分支

- 位置：`crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs` 的 `use_id_keyed_format` 及条件分支；`format.rs::format_id_keyed_accepted_tool_result`。
- 当前证据：开关使用 `serde(default, skip)`、`schemars(skip)`，JSON 输入不能开启；全仓检索未发现生产代码构造 true，只存在实现和测试。
- 删除理由：维护一套没有生产接入的平行答案格式；有效选项 ID 与 notes 同时存在时，该格式还会丢弃用户补充说明。普通问答格式会保留 notes。
- 拟删除范围：内部格式开关、run 中的备用分支、专用 formatter 和仅服务它的测试。执行前重验引用；不顺带删除 Question/QuestionOption.id，也不删除实际问答能力。
- 限制：未证明仓库外 Rust 调用者不存在；当前默认路径不受备用格式缺陷影响。同题重复 label 校验仍必要，正常问答也是按 label 返回。
- 发现时状态：列为候选，等待逐项核准。

- 用户授权后的执行结果：已删除并完成对应回归；详见 clean-approved-milestone-candidates 的 R4 验证记录。

## R5 · 未发现生产读取的 AvailableSkills 副本

- 位置：tools/src/types/resources.rs 的 AvailableSkills / has_skill；registry 初始化和 bridge 协调时的写入（均位于 crates/codegen/）。
- 证据：全 crates 检索只有上述生产写入和测试读取，has_skill 无调用；实际 slash 直接读取 SkillManager 并过滤 enabled。
- 删除理由：维护重复技能快照与无调用查询方法，增加同步负担，容易误认为执行权限来源。
- 拟删除范围：该资源类型、has_skill、仅维护副本的写入与仅验证该副本的测试；执行前复核泛型资源和外部调用。不要顺带删除 SkillManager、动态发现合并、SkillInput/SkillOutput、slash 或预加载功能。
- 发现时状态：列为候选，等待逐项核准。

- 用户授权后的执行结果：已删除并完成对应回归；详见 clean-approved-milestone-candidates 的 R5 验证记录。

## R6 · 未找到注册实现的旧 Skill 工具协议

- 位置：tools/src/implementations/skills/skill.rs 的 SkillInput/SkillOutput；ToolInput::Skill、ToolOutput::Skill 及仅处理这些值的匹配分支（均在 crates/codegen）。
- 证据：当前内置 registry 未注册 Skill 工具，全仓未找到执行实现；grow_build/skill 路径不存在。仍有权限、日志、输出和 ACP 转换分支。
- 删除理由：维护已无内置执行入口的协议结构和跨模块处理分支，过时注释还指向不存在的实现。
- 拟删除范围：上述旧 IO 结构、枚举分支及专用处理分支；执行前重新核对外部 ToolPack 和序列化值。此项会涉及旧 ToolInput/ToolOutput 的反序列化兼容面。
- 必须保留：skill.rs 的加载、消息格式化、参数替换、内部链接和 slash/预加载函数；不要整文件删除，也不顺带删除 ToolKind::Skill 模板分类。
- 发现时状态：列为候选，等待逐项核准。

- 用户授权后的执行结果：已删除并完成对应回归；详见 clean-approved-milestone-candidates 的 R6 验证记录。

## R7 · 无仓内调用的整份配置保存入口

- 位置：crates/codegen/shell/src/util/config/persist.rs 的公开 save_config 与其专用 save_config_locked 包装。
- 证据：全仓 Rust 检索未找到实际 save_config 调用，只有定义和文档引用；save_config_locked 仅被该入口调用。当前设置函数使用 update_config，在锁内读取原始 TOML 再修改并保存。
- 删除理由：保留一条未使用的整份 Config 写入路径，容易被新调用者用来保存过时或运行时展开后的配置快照。
- 拟删除范围：上述两个包装函数及仅指向它们的过时注释；不删除 save_config_at、read_config_for_save、update_config、SAVE_LOCK、原子文件操作或实际回归测试。
- 限制：pub use persist::* 暴露公开 API，未证明仓外没有调用者；删除前复核外部使用。
- 发现时状态：列为候选，等待逐项核准。


- 当前决定：谨慎保留；仓库检索不能排除外部初始化使用，未改动配置整份写入路径。

## R8 · 未接入生产入口的旧权限保存协议

- 位置：crates/codegen/pager/src/app/actions.rs 的 PersistPermissionMode、PermissionModePersist、SettingPersistFailedBestEffort，以及旧 effects helper/executor 和专用结果分支。
- 证据：fix-default-permission-persistence-order 后，生产默认权限通过普通 PersistSetting 保存，当前会话通过 NotifySessionPermissionMode 切换；旧 Effect 只剩执行匹配与测试构造，BestEffort/WithRollback 的旧组合不再由生产 setter 发出。
- 删除理由：维护没有生产请求入口的独立保存/通知协议，且与已使用的普通设置保存协调重复。
- 拟删除范围：旧 Effect 和策略枚举、persist_permission_mode_and_notify 及仅供旧路径使用的分支/测试、SettingPersistFailedBestEffort 专用结果；删除前复核 helper 是否有其他调用。
- 必须保留：NotifySessionPermissionMode、普通 PersistSetting(permission_mode)、SettingPersisted/SettingPersistFailed、默认权限回滚、授权与队列逻辑。不要删除权限模式功能。
- 发现时状态：列为候选，等待逐项核准。

- 用户授权后的执行结果：已删除并完成对应回归；详见 clean-approved-milestone-candidates 的 R8 验证记录。

## R9 · 技能 YAML 自动修复的闲置 helper

- 位置：tools/src/implementations/skills/discovery.rs 的 quote_problematic_values、RECOVERABLE_KEYS、recover_scalar_fields。
- 原因：严格损坏元数据修复后，生产不再尝试重新引号化或只恢复标量字段，以免改变或丢失限制。上述私有 helper 无调用。
- 拟删除范围：仅两个 helper 和常量及专属说明；保留严格 YAML 解析、合法纯 Markdown 回退与错误回归。
- 发现时状态：列为候选，等待逐项核准。

- 用户授权后的执行结果：已删除并完成对应回归；详见 clean-approved-milestone-candidates 的 R9 验证记录。

## R10 · 未接入采样的技能 model/effort 字段

- 位置：ParsedFrontmatter、SkillInfo、workspace-types rpc/skills 的 model/effort 及对应解析/测试。
- 证据：仓内只见解析、复制、序列化及测试；技能展开返回提示文本，不消费这两个字段。指南此前误称 override，已纠正。
- 删除理由：没有实际覆盖效果，却保留了易误导的覆盖配置面。
- 拟删除范围：仅技能元数据字段和专属解析、说明、测试；不得删除会话模型、reasoning effort 或任务 runtime overrides。执行前核对外部 RPC 消费。
- 发现时状态：列为候选，等待逐项核准。

- 用户授权后的执行结果：已删除并完成对应回归；详见 clean-approved-milestone-candidates 的 R10 验证记录。

## R11 · 已失效的技能 slash 重写选择器

- 位置：shell/session/slash_commands.rs 的 SkillSlashRewrite、resolve 的 _skill_rewrite 参数，admission/slash_exec 固定传参及测试传参。
- 证据：resolve 完全不读取参数；生产只构造 RewriteToRun，Passthrough 仅见测试。技能调用统一保留原文并单独展开正文，没有两套策略，也没有配置接入。
- 删除理由：内部闲置参数扩大调用面，旧注释误称有 run 前缀和工具调用策略，已纠正注释。
- 拟删除范围：该枚举及闲置形参/实参；保留现有技能解析、原文保留、正文展开、out-of-band 模型工作拒绝和行为回归。与 R6 旧 Skill 工具协议分开处理。
- 发现时状态：列为候选，等待逐项核准。

- 用户授权后的执行结果：已删除并完成对应回归；详见 clean-approved-milestone-candidates 的 R11 验证记录。

## R12 · 没有运行时消费者的恢复程度缓存

- 位置：Pager AgentSession.restore_degree、默认 None、load/fork 两处赋值及专属缓存测试。
- 证据：非测试 Pager 的 .restore_degree 仅见赋值；没有渲染或控制流读取。当前实际反馈使用 code_restored + restore_summary。
- 删除理由：仅为未来显示占位的状态投影及内部转发，不影响当前恢复操作或反馈。
- 拟删除范围：会话缓存、初始化、赋值、专属缓存断言及仅为填充缓存存在的内部转发参数；删除前再次核对新增消费者。
- 必须保留：--restore-code 功能、恢复执行、共享 RestoreDegree/RestoreDecision、Shell wire 元数据、协议解析与非法值校验、恢复结果摘要。不要删除代码恢复或其协议能力。
- 状态：已获批并删除，201 项会话回归及 8 项协议解析回归通过。证据见 OpenSpec audit-restore-degree-cache。

## R13 · 仅测试自身的原始按键格式化辅助函数

- 位置：pager/src/input_log.rs 的 cfg(test) format_key_code_raw 和 format_key_code_raw_shows_punctuation。
- 证据：全 crates 引用仅有定义及该函数自身的测试；生产 snapshot_entries 使用 sanitize_key_code，其他测试也不依赖原始格式化函数。
- 删除理由：维护一个不参与产品或测试夹具的原始输出格式及自证测试，没有验证生产行为。
- 拟删除范围：仅此辅助函数与专属测试。保留 input recorder、RawInputEntry、脱敏函数、环形容量测试、导出与真实快捷键。
- 状态：已获批并删除，输入日志回归 7/7 通过。审计见 audit-input-recorder-boundary。

## R14 · 未注册的输入日志 ActionId 占位

- 位置：pager/actions/mod.rs 的 ActionId::DumpInputLog，以及 agent_view/mod.rs、dashboard/state.rs 的无操作匹配分支。
- 证据：全仓未找到对应 ActionDef 注册或构造入口；typed ActionId 无字符串配置反序列化。已存在引用仅声明与不可执行分支。
- 删除理由：闲置注册标识造成看似存在快捷键注册的错觉，并扩大穷尽匹配维护范围。
- 拟删除范围：仅 ActionId 变体和专属无操作 match arms。必须保留 app::actions::Action::DumpInputLog、Esc 后 d 的真实入口、input recorder、文件导出及 debug slash 命令。
- 状态：已获批并删除；注册 18/18、诊断 3/3、Esc-d 1/1 回归通过。证据见 audit-debug-action-registration；不声称仓库外没有公共类型消费者。

## R15 · 隐藏的 GBOOM 射击小游戏

- 位置：pager-render/src/gboom/、render/gboom_overlay.rs，Pager 的 /gboom 注册、OpenGboom、AgentView 游戏状态及事件循环/键盘协议专属接入。
- 证据：隐藏但可执行的娱乐彩蛋，独立游戏与渲染文件共约 3,251 行（含测试、注释）。并非诊断器，也不是未接入代码；仅显式调用时开启。
- 删除理由：与编码代理主功能无关，却维护独立引擎、逐帧图像输出、输入按键生命周期和专属调度分支；可作为缩减产品维护面的候选。
- 拟删除范围：小游戏本体和专属入口、状态、输入/调度/协议 hooks 与测试。删除前逐项核对共享消费者。
- 必须保留：通用 Kitty/图片显示、普通键盘协议与终端恢复、公共动画时钟、焦点和取消处理、真实 /debug 诊断功能。不要按名称把 sampler/shell 的 doom-loop 恢复误删。
- 状态：已获批并删除；命令 374/374、事件循环 80/80、媒体 3/3、弹层 1/1 回归通过。审计见 audit-gboom-feature-boundary；没有测量二进制节省量。

## R16 · 仅测试自身的剪贴板扩展名映射

- 位置：client-support/src/clipboard.rs 的 cfg(test) platform::extension_for_class 和 tests::macos_helpers::extension_mapping。
- 证据：全仓引用只有定义、该测试的导入及断言；生产 read_clipboard_image_from_class 直接分派路径/MIME，native_image_type_from_types 负责原生类型选择，两者都不调用此函数。
- 删除理由：测试维护的映射不参与产品行为，包括 JPEGAufs 和 unknown→bin 分支，无法对实际图片读取提供回归保护。
- 拟删除范围：仅该辅助函数和独占测试模块。保留真实类型选择、私有临时文件、附件门禁、剪贴板测试钩子、诊断示例和原生读图回退开关。
- 状态：已获批并删除；原生类型 3/3、MIME 10/10 回归通过。证据见 audit-clipboard-helper-reachability。

## R17 · 托管配置 inspection 的重复原文快照

- 位置：config/src/managed_text/mod.rs 的 ManagedTextInspection.original_text、original_text() 和 plan 内对应 text.to_owned() 初始化。
- 证据：仓库内唯一读取者是 typed_inspection_and_item_updates_share_one_validated_parse 的一个断言；doctor 预览使用 managed_block，冲突检测使用 unmanaged_text/requested_item_state，事务使用 SourceState.bytes。
- 删除理由：每次规划已有配置都复制完整原文，但产品路径没有消费者，增加计划持有和 clone 的重复数据。未测量实际内存收益。
- 拟删除范围：此字段、访问器、初始化和专属断言及其无用局部变量。保留整项 typed_inspection 测试其余覆盖、SourceState.bytes、非托管正文、条目状态、预览、备份及回滚。外部编辑器同名 original_text 与此无关，必须保留。
- 状态：已获批并删除；托管配置回归 34/34 通过。审计见 audit-managed-inspection-consumers；不声称仓库外没有公共 API 消费者。

## R18 · 未接入生产发送的旧图片内容构建链

- 位置：pager-render/src/prompt_images.rs 的 load_for_send、MAX_SEND_BYTES、build_content_blocks_with_workspace、build_content_blocks_with_prefixes、build_content_blocks_with_prefixes_and_caps、resolve_orphan_placeholders。
- 证据：全仓符号引用显示builder互调与测试；外部调用在pager/views/prompt_widget/tests.rs，未发现生产发送调用。当前生产使用pager/app/root/effects/helpers.rs::append_prompt_images，先检查句柄及总预算，再有界读取。
- 删除理由：旧链维护另一套发送、恢复和预算逻辑，并保留完整读取后才检查的旧行为；修改这些代码不会改善当前发送路径，却增加测试与认知负担。
- 拟删除范围：仅旧函数链、专属常量及独占测试/注释；逐项区分仍验证PastedImage恢复、源文件、缓存和实际发送的测试，不整段删除测试模块。
- 必须保留：append_prompt_images、PastedImage及真实显示编号、图片持久化与恢复、client-support的共享占位加载器、剪贴板、渲染和实际发送验证。
- 状态：已获批并删除；图片 142/142、PromptWidget 241/241、真实发送加载器 4/4 回归通过。未测量二进制/编译时间收益；公共API的仓库外使用未知。证据见audit-image-number-metadata-and-legacy-builder。

## R19 · 无内部编号解释消费者的ACP imageDisplayNumber元数据

- 位置：client-support/src/placeholder_images.rs的IMAGE_DISPLAY_NUMBER_META_KEY、display_number_meta、display_number_from_meta，以及生产/旧builder中专用.meta写入点。
- 证据：专用读取函数只被测试使用；key字面量无生产解释逻辑，注释所指AttachedImages无定义。image_normalize和input_inbox仍泛化保留_meta，但不按此编号选择图片。
- 删除理由：维护了未被当前内部行为消费的专用协议与“按编号解析”的过时说明。它是候选，不等于图片编号整体无用；仓库外ACP消费者仍未知。
- 拟删除范围：仅专用key、setter/getter、专用写入和独占断言；执行前核对已归档编号保持契约，必要时在用户确认后的change中调整协议。
- 必须保留：PastedImage.display_number、文本[Image #N]和真实匹配逻辑、图片内容/URI/持久化身份、泛化_meta保存和转码透传测试。不得以此候选删除所有ACP元数据。
- 状态：已获批并删除；图片恢复 54/54、真实发送 4/4 回归通过。证据见audit-image-number-metadata-and-legacy-builder。

## R20 · 无生产调用的同步路径图片查看器构造器

- 位置：pager-render/src/prompt_images.rs 的 ImageViewerState::open_from_path。
- 证据：全仓引用仅见该方法定义及三个直接测试；实际 Enter 图片入口使用 open，已有后台加载链通过 open_from_path_deferred 表达加载状态。
- 删除理由：重复实现文件读取、尺寸识别、格式转换和查看器组装，当前没有接入用户入口。
- 拟删除范围：仅 open_from_path 及独占测试/注释；相关正常图片、缺失文件、标题等有效断言应迁移到实际加载路径，不整段删测试。
- 必须保留：真实图片查看器、ImageViewerState::open、后台加载链及其 owner/目标校验、图片发送与存储。后台链目前缺少生产 admission，需要接线修复，不属于本候选。
- 状态：等待用户确认，尚未删除；仓库外公共API使用未知。证据见 audit-image-viewer-entrypoints。

## R21 · 始终返回None的历史选择空壳API

- 位置：pager/src/views/history_search.rs 的 HistorySearchState::selected。
- 证据：方法体始终None，全仓Rust检索未发现调用；键盘和鼠标接受均使用selected_text。
- 删除理由：没有产品行为，却暴露一个看似可获取选择结果的误导接口。
- 拟删除范围：仅此方法及专属注释；保留HistoryEntry、selected_text、导航、渲染及历史功能。
- 状态：等待用户确认，尚未删除。仓库外公共API使用未知；证据见audit-prompt-history-and-draft-entrypoints。

## R22 · 独立的隐藏scroll-debug命令别名

- 位置：pager/src/slash/commands/scroll_debug.rs及builtin_commands注册。
- 证据：该命令与/debug scroll均返回ToggleScrollDebugHud；debug.rs测试明确锁定同一Action，router有真实HUD消费。
- 删除理由：重复的命令入口和使用说明，/debug scroll已提供同一功能。不是未启用代码，也不是建议删除滚动诊断本身。
- 拟删除范围：仅独立ScrollDebugCommand、注册和专属测试/说明；若确认，调整/debug中引用它的等价性测试。
- 必须保留：/debug scroll、ToggleScrollDebugHud、实际HUD、FPS和滚动日志记录器、相关行为验证。
- 状态：等待用户确认，尚未删除；旧脚本/人工习惯可能仍使用该别名，仓库外使用未知。证据见bound-export-completion-enumeration/design.md的邻接审计。

## R23 · 未构造的ActivePaneSnapshot::Other枚举项

- 位置：pager/src/input_log.rs 的ActivePaneSnapshot::Other。
- 证据：全仓Rust检索无构造/引用；record_input对现有ActivePane六项逐一映射，没有fallback。
- 删除理由：无当前行为消费者的残留诊断分类。
- 拟删除范围：仅Other变体；保留其余pane类别、原始输入ring、字符脱敏和真实dump入口。
- 状态：等待用户确认，尚未删除。公共API仓库外消费者未知；审计见align-input-dump-target-ownership。

## R24 · 未被公告行为消费的persistent字段

- 位置：announcements/src/lib.rs的Announcement.persistent及default_announcements赋值。
- 证据：字段参与serde传输，默认Some(false)；当前共享过滤、pager公告/welcome展示和隐藏控制路径没有读取它。
- 删除理由：配置表面暴露了当前无实现语义的开关，容易让使用者误以为可控制公告持久展示。
- 拟删除范围：仅该字段及专属默认赋值/说明；保留dismissible、expires_at、公告传输、所有展示入口和hidden_ids真实持久化。
- 状态：等待用户确认，尚未删除。仓库外配置/ACP消费者可能使用该字段，使用情况未知；证据见audit-announcement-storage-boundaries。

## R25 · 未找到调用方的诊断设备标识模块

- 位置：diagnostics/src/id.rs、lib.rs 的 pub mod id 与 diagnostics/Cargo.toml 的 mid 依赖。
- 证据：仓库 Rust 调用/导入检索未找到 diagnostics::id::agent_id 消费者；诊断事件未使用该设备标识。其他 agent_id 是 UI/子 Agent 标识，不属于此候选。mid 仅被该模块使用。
- 删除理由：当前未启用，却保留设备指纹计算、磁盘缓存及依赖维护负担。缓存无界读取、任意非空值接纳与外部命令无显式期限仅是该闲置路径的代码风险，未复现为当前启动故障。
- 拟删除范围：仅设备标识模块、公开导出、专属测试和独占 mid 依赖；保留真实诊断日志/事件、session/subagent 标识，不删除用户已有 agent_id 文件。
- 状态：等待用户确认，尚未删除。公共库仓库外使用未知；证据见 audit-diagnostic-device-id-reachability。

## R26 · 未使用的会话统一日志快照函数

- 位置：diagnostics/src/unified_log.rs 的 snapshot_session_log。
- 证据：全仓 Rust 符号检索只有定义，无测试或生产调用方；snapshot_log 有 Leader 测试消费者，必须保留。
- 删除理由：闲置的整文件读取与 JSONL 过滤路径，增加维护表面。
- 拟删除范围：仅 snapshot_session_log 及专属说明，保留 snapshot_log、真实日志写入/裁剪和测试隔离。
- 状态：待用户确认，尚未删除；公共库的仓库外调用未知。证据见 isolate-leader-integration-log-output。

## R27 · 未接通启用入口的可选图片正规化缓存

- 位置：shell/session/normalize_cache.rs 的可选 moka 缓存机制、agent/config.rs 的 apply_remote_settings_side_effects、config-types 的 image_normalize_cache_enabled。
- 证据：global缓存默认关闭；唯一非测试 set_enabled 位于 apply_remote_settings_side_effects，但全仓没有调用该函数。生产正规化目前经绕过分支直接计算，启用缓存的命中验证只见于测试。
- 删除理由：当前未启用的缓存和开关增加维护表面；若决定保留，应另行明确配置权威并接通启用流程。
- 拟删除范围：仅可选缓存/断开的hook与字段，实际实施前梳理类型和调用依赖；必须保留图片正规化、结果类型、错误处理和最近加入的后台计算并发限制，不能整文件直接删除。
- 状态：等待确认，尚未删除或启用。公共hook的仓库外调用未知；证据见 audit-normalization-cache-activation。

## R28 · 无调用方的旧可选 JSON 读取器

- 位置：shell/src/session/storage/jsonl/mod.rs 的私有方法 read_optional_json_sync。
- 证据：全仓 Rust 符号检索仅见定义；实际控制、signals 和公告状态由 Timeline 恢复，Workflow manifest 使用目录 capability 的有界读取。
- 删除理由：未启用的 exists/read_to_string/错误转 None 路径增加维护表面。无界读取和吞错仅是闲置实现的属性，未认定为当前运行故障。
- 拟删除范围：仅该私有方法；保留实际可选状态恢复、Timeline 校验、有界文件读取及相关功能。
- 状态：等待用户确认，尚未删除或接线。证据见 audit-unused-jsonl-helpers。

## R29 · 无调用方的旧摘要锁及 Workflow 路径构造器

- 位置：shell/src/session/storage/jsonl/mod.rs 的私有方法 summary_lock_file、workflows_dir。
- 证据：全仓 Rust 检索仅见各自定义；真实写锁使用 writer lease，Workflow 恢复通过已固定目录打开相对 run 路径。
- 删除理由：两个旧路径助手已不参与当前调用链，容易被误用为现有存储权限入口。
- 拟删除范围：仅这两个方法；保留 writer lease 协议、Workflow 存储和恢复。rewind_points_file 仍有三个测试消费者，不在此候选内。
- 状态：等待用户确认，尚未删除；不删除任何已有用户锁文件或 Workflow 目录。证据见 audit-unused-jsonl-helpers。

## R30 · 未接入的 workspace 文件回退入口及独占响应类型

- 位置：workspace/src/session/file_state.rs 的 rewind_files、FileRewindResponse、FileRewindConflict、ConflictType。
- 证据：全仓 Rust 检索中 rewind_files 只有定义；三个类型只在该文件中服务这个入口。实际 shell handle_rewind 使用自己的预览、持久化意图、文件应用/补偿和 Timeline 提交链。
- 删除理由：保留了第二套未启用的文件回退实现和响应模型；它缺少 shell 的事务协调，不能仅为复用而接入。
- 拟删除范围：该函数及确认独占的三个类型、专属说明；保留 shell 回退、RewindResponse、RewindConflictInfo、merge_rewind_points_from、文件快照及事务恢复。
- 状态：等待用户确认，尚未删除。公开库的仓库外调用情况未知；证据见 audit-disconnected-rewind-entrypoints。

## R31 · 仅被测试调用的回退 tracker 便利方法

- 位置：FileStateTracker::merge_and_remove_from、max_prompt_index。
- 证据：仓库内调用均在 file_state 测试模块；实际 ConversationOnly 回退调用纯 merge_rewind_points_from，先持久化再 replace_rewind_points。
- 删除理由：未参与当前生产路径的包装接口增加维护和测试表面。
- 拟删除范围：仅两个方法及专属测试调用；确认后先将独有断言迁移到实际完整历史读取/纯合并路径。必须保留纯合并函数、truncate_from（取消仍使用）、完整读取与失败重试、replace_rewind_points 和实时捕获。
- 状态：等待用户确认，尚未删除；公开方法的仓库外使用未知。证据见 audit-disconnected-rewind-entrypoints。

## R32 · 未接入生产消费者的 401 attribution 回调

- 位置：sampler/src/attribution.rs 的 Auth401AttributionCallback、SharedAttributionCallback、SamplingConsumer，以及 SamplerConfig/SamplingClient 的可选回调字段和转发链。
- 证据：全仓唯一 trait 实现为 client 测试模块的 CountingCallback；唯一 Some 回调构造也在测试中。生产配置默认 None，aux/workflow 仅克隆转发，未发现启用入口。
- 删除理由：保留未启用的诊断扩展和跨层配置字段；注释仍引用已不存在的 shell token_suffix。短 token 会完整跨回调边界，不能把截断称为脱敏。
- 拟删除范围：可选回调、专属枚举/别名/调用点及纯转发字段；确认后核对专属测试并保留有效认证断言。
- 必须保留：请求构建时认证状态捕获、auth_rejected、SentCredential 和认证重试预算；bearer resolver 与认证方式诊断。sent_bearer/current_sent_bearer_prefix/截断助手还有这些消费者，不得随回调整体删除，是否改成仅保存 presence 需另行核对。
- 状态：等待用户确认，尚未删除或启用。仓库外公开库消费者未知。证据见 audit-disconnected-auth-attribution。
