# configuration-rules Specification

## Purpose
定义个人配置、项目配置和 Agent 规则的发现边界。覆盖个人配置的固定来源与解析错误、项目内配置查找，以及规则文件的去重和排序；不从发现行为推导额外的信任授权。

## Requirements

### Requirement: User config authority
个人配置 SHALL 从 GROW_HOME 下的 config.toml 加载，缺失文件按空配置处理，语法错误明确返回错误。

#### Scenario: 配置语法错误
- **WHEN** 个人 config.toml 无法解析
- **THEN** 报告 TOML 行列信息，不静默改用 cwd 的同名文件。

证据：`crates/codegen/config/src/loader.rs` — `load_from_disk`。

### Requirement: Project scoped config discovery
项目配置 SHALL 通过工作区配置发现逻辑查找仓库内 .grow/config.toml。

#### Scenario: 打开项目目录
- **WHEN** find_project_configs 运行于工作目录
- **THEN** 返回该仓库发现范围内的配置路径。具体 trust 与覆盖次序由加载方决定。

证据：`crates/codegen/workspace/src/project_config.rs` — `find_project_configs`。

### Requirement: Agent rule discovery
Agent 规则 SHALL 发现 AGENTS.md 与规则目录中的 Markdown 文件，并去重规范路径。

#### Scenario: 加载目录规则
- **WHEN** 多个发现根指向同一规范路径
- **THEN** 规则发现去重，目录内 Markdown 规则按文件名排序。

证据：`crates/codegen/agent/src/prompt/agents_md.rs` — `find_rules_files`。

### Requirement: Untrusted project LSP cannot shadow permitted sources
LSP 执行配置合并 SHALL 在项目来源参与同名覆盖之前应用项目信任许可。未获准的项目 LSP 配置 SHALL 不遮蔽用户或已允许插件的同名配置。

#### Scenario: 未信任项目同名覆盖
- **WHEN** 未信任项目与用户或已允许插件定义同名 LSP server
- **THEN** 合并保留允许来源的配置，项目命令不进入可执行服务器集合。

#### Scenario: 信任项目覆盖
- **WHEN** 已信任项目与其他来源定义同名 LSP server
- **THEN** 项目配置继续覆盖低优先级来源，其他无冲突允许来源保留。

证据入口：`tools/src/implementations/lsp/config.rs` 的 sourced loader、`workspace/src/handle.rs` 和 `shell/src/agent/mvp_agent/agent_ops.rs`（均在 `crates/codegen/`）。inspect 只展示配置并标记禁用，不授权执行。

### Requirement: Web fetch cache respects configured capacity
web_fetch 文本缓存 SHALL 遵守 max_cache_entries 最大条目数，零表示不保存缓存；更新已有 URL SHALL 不淘汰其他条目。

#### Scenario: 零容量
- **WHEN** max_cache_entries 为零且获取完成的文本尝试进入缓存
- **THEN** 不保留条目，后续缓存查询不命中。

#### Scenario: 满容量更新
- **WHEN** 缓存已满且再次写入已有 URL
- **THEN** 更新该 URL 内容与插入时间，保留其他条目。

#### Scenario: 满容量新增
- **WHEN** 缓存已满且写入新 URL
- **THEN** 淘汰最早插入的条目，插入新内容且总条目数不超过配置上限。

### Requirement: Web fetch bounds response accumulation
web_fetch SHALL 在接收解码后的响应正文时执行 max_content_length 检查，累计缓冲区不超过上限；超限 SHALL 返回 ResponseTooLarge，不等待响应结束。

#### Scenario: 持续分块响应超限
- **WHEN** 未结束的响应提供的解码正文已经超过 max_content_length
- **THEN** 立即停止累积并返回大小错误。

#### Scenario: 恰好上限
- **WHEN** 完整响应正文长度恰好等于 max_content_length
- **THEN** 正常处理正文；上限为零时允许空响应并拒绝非空响应。

### Requirement: Project config watches survive directory creation
已注册项目的配置监听 SHALL 在 .grow 晚于启动创建或被删除后重建时继续观察 config.toml；补挂 SHALL 不依赖配置内容变化或 MCP reload 去重结果，监听范围保持非递归。

#### Scenario: 首次创建项目配置目录
- **WHEN** 注册 cwd 时没有 .grow，随后创建目录和配置文件
- **THEN** 首次配置和后续写入均能触发配置事件。

#### Scenario: 删除并重建目录
- **WHEN** 已监听 .grow 被删除并重建
- **THEN** 新目录配置更新继续可观察。

#### Scenario: 项目取消注册
- **WHEN** cwd 已 unwatch 后创建或重建 .grow
- **THEN** 不为该 cwd 重新挂载监听。

### Requirement: Discovery watches follow replacement directories
项目 discovery watcher SHALL 在已存在的 .grow 及其 seed 目录被替换后继续接收更新；曾注册路径 SHALL 不等于当前目录已被监听。普通文件更新 SHALL 不导致无条件重建全部递归监听。

#### Scenario: 已存在的根删除后重建
- **WHEN** 启动时.grow存在，随后删除并重建.grow及workflows
- **THEN** 新workflows的后续修改仍可观察。

#### Scenario: 原子替换与普通修改
- **WHEN** 已监听 skills 或 .grow 被原子替换后执行刷新
- **THEN** 新实体得到注册；随后普通文件修改和重复刷新不重复注册。

#### Scenario: Skills watcher 根重建
- **WHEN** SkillsFileWatcher 的项目 .grow 删除后重建
- **THEN** 根目录及 skills 递归监听恢复，嵌套技能修改继续可观察。

### Requirement: Skill baseline metadata updates reach runtime
技能 baseline 重读 SHALL 将同一路径的元数据变化协调到运行时技能表；完全相同的 baseline SHALL 不因重复加载产生多余协调。

#### Scenario: 原路径停用或修改描述
- **WHEN** baseline 路径不变但 enabled 或 description 改变
- **THEN** 产生协调结果，runtime_skills 携带最新元数据。

#### Scenario: 完全相同的重读
- **WHEN** baseline 内容与顺序均未改变
- **THEN** 不产生新的 pending baseline 协调。

#### Scenario: 描述更新进入提示
- **WHEN** 可见技能在同一路径更新描述
- **THEN** baseline 协调产生的技能提示包含新描述；停用的技能不出现在新提示中。

### Requirement: Refreshed baseline supersedes same-path discoveries
新 baseline SHALL 接管其包含的规范技能路径，旧动态副本 SHALL 不遮蔽该路径的新元数据或条件门控；未包含的动态技能 SHALL 保留。

#### Scenario: 同路径动态副本被刷新
- **WHEN** 已动态发现的技能进入更新后的 baseline
- **THEN** 投影使用最新 baseline 内容，其他路径动态发现不丢失。

#### Scenario: 新增条件门控
- **WHEN** 先前无条件动态技能在新 baseline 中具有尚未触发的 paths 条件
- **THEN** 旧动态副本不使它继续可见。

### Requirement: Dynamic discovery respects known skill authority
后续动态发现 SHALL 不以未应用配置的同路径副本遮蔽已加载 baseline；已知条件技能 SHALL 保持当前门控，直到匹配激活或 baseline 更新。

#### Scenario: 重新发现停用技能
- **WHEN** 已加载 baseline 的技能停用，随后动态发现同一路径的原始启用副本
- **THEN** 保持停用状态，不为该副本产生新发现提示。

#### Scenario: 重新发现条件技能
- **WHEN** 已知未激活条件技能被再次解析且缺少当前门控字段
- **THEN** 仍保留当前门控，真实路径匹配后正常激活。

### Requirement: Skill substitutions do not reinterpret inserted values
技能模板替换 SHALL 只解释原始正文中的 token，插入的参数或上下文值 SHALL 按字面保留。显式 $ARGUMENTS[N] 缺失索引 SHALL 替换为空。

#### Scenario: 参数包含占位符文本
- **WHEN** 参数本身含有 $0 或 ${SESSION_ID} 等文本
- **THEN** 插入后不再展开这些文本。

#### Scenario: 上下文值含占位符
- **WHEN** skill_dir 等上下文值含其他 token 文本
- **THEN** 值作为原文插入，不级联展开。

#### Scenario: 显式大索引
- **WHEN** 正文引用不存在的 $ARGUMENTS[1000000]
- **THEN** 该 token 变为空，不产生部分替换残留。

### Requirement: Skill link resolution edits only destination text
技能内部链接解析 SHALL 只修改目的地址源码范围，保持标签、标题和非链接文本不变。

#### Scenario: 目标和标题同名
- **WHEN** 技能正文链接的目标与 title 都包含同一文件名
- **THEN** 只将目标解析为技能目录内路径，title 保持原文。

#### Scenario: 标签和代码中的同名文本
- **WHEN** 链接标签、嵌套图片或代码片段包含同名地址
- **THEN** 仅实际链接/图片的目的地址被改写，代码和标签文本保留。

#### Scenario: 特殊字符路径
- **WHEN** 目标包含 Markdown 转义括号、字符实体或空格
- **THEN** 新目标仍能被 Markdown parser 解析为正确的技能内绝对路径。

### Requirement: Skill frontmatter uses complete delimiter lines
技能元数据解析和正文提取 SHALL 仅将去除行两侧空白后恰为 --- 的行作为分隔符，保持与流式读取一致。

#### Scenario: 前缀伪分隔符
- **WHEN** 开始或结束位置只有 ---suffix 而无完整分隔行
- **THEN** 不解析为完整 frontmatter，不丢弃正文。

#### Scenario: 换行与末尾
- **WHEN** 分隔行使用 LF、CRLF 或关闭分隔行位于 EOF
- **THEN** 元数据与正文边界正确，真正关闭行之后的正文保留。

### Requirement: Skill frontmatter reading bounds source consumption
read_frontmatter_only SHALL 在底层限定读取至 MAX_FRONTMATTER_BYTES 加一个探测字节，超限行不加入 metadata；UTF-8 在探测边界被截断 SHALL 不作为真实编码错误报告。

#### Scenario: 超长单行
- **WHEN** frontmatter 候选包含大于上限的单行
- **THEN** 读取量不超过上限加一，不保留超限行。

#### Scenario: 边界截断多字节字符
- **WHEN** 合法 UTF-8 长行在读取上限处被截断
- **THEN** 按超限结束，而非返回编码错误。

### Requirement: Loaded skill bodies preserve empty snapshots
技能正文加载成功后 SHALL 保存已加载状态，包括空正文。读取已加载正文 SHALL 返回快照，不重新读取磁盘。未加载的 synthetic path SHALL 返回缺少正文错误。

#### Scenario: Empty file body frozen before disk changes
- **WHEN** 空技能正文被加载为快照，随后文件被修改或删除
- **THEN** 读取快照仍成功返回空正文。

#### Scenario: Explicit empty synthetic body
- **WHEN** synthetic path 技能携带已加载空正文
- **THEN** 返回空正文；只有缺失正文时返回错误。

### Requirement: Agent skill preloading respects disabled entries
Agent skills 声明解析 SHALL 在名称解析后尊重选定技能的 enabled 状态。禁用条目 SHALL 不被预加载或注入提示词，且不得因禁用回退到另一个同名条目。

#### Scenario: Disabled skill explicitly named
- **WHEN** agent 声明匹配禁用的 native 技能或 qualified plugin 技能
- **THEN** 该技能不进入预加载结果或提示词。

#### Scenario: Disabled entry precedes enabled name collision
- **WHEN** 已解析名称选中禁用技能，后续目录还有同名启用技能
- **THEN** 不加载后续同名技能；单独声明的启用技能仍正常加载。

### Requirement: Skill toggles use exact catalog identity
技能开关和 disabled 配置 SHALL 使用目录去重身份：原生技能为 name，插件技能为 plugin:name。一次开关 SHALL 只改变指定身份条目，不影响其他同名身份。

#### Scenario: Plugin skill shares native name
- **WHEN** 禁用 plugin:name，且同时存在原生 name
- **THEN** 插件技能禁用，原生技能保持启用。

#### Scenario: Native skill shares plugin name
- **WHEN** 禁用原生 name，且同时存在插件的同名技能
- **THEN** 仅原生技能禁用，插件技能保持启用；列表开关发送所选条目的目录身份。

### Requirement: Skill ignore paths cover plugin candidates
技能发现 SHALL 对插件候选应用与原生来源相同的 ignore 路径过滤，且在合并去重前执行。忽略指定路径 SHALL 不影响其他路径下的同名技能。

#### Scenario: Ignored plugin directory
- **WHEN** ignore 指向插件技能目录
- **THEN** 该目录下技能不进入发现结果，其他路径的原生同名技能仍保留。

#### Scenario: Ignored plugin skill file
- **WHEN** ignore 仅指向某个插件 SKILL.md
- **THEN** 该文件条目排除，未忽略的插件 sibling 继续可见。

### Requirement: Config writes stop on existing-file read errors
配置保存 SHALL 仅在目标不存在时使用空配置；其他读取失败 SHALL 返回错误且不得替换原文件。读改写操作 SHALL 在初始加载失败时停止，不执行修改闭包。

#### Scenario: Existing config is invalid UTF-8
- **WHEN** 设置保存无法将已有文件读取为 UTF-8
- **THEN** 返回错误，原文件字节保持不变。

#### Scenario: Config does not exist
- **WHEN** 设置保存目标文件不存在
- **THEN** 正常创建有效 TOML 配置。

### Requirement: Settings updates edit raw config values
设置读改写 SHALL 从原始 TOML 构造编辑值，不应用环境展开或运行时版本覆盖。未修改的环境变量引用 SHALL 保持字面形式；运行时加载仍按原规则解析。

#### Scenario: Unrelated setting update with environment reference
- **WHEN** skills.paths 包含环境变量引用，读改写仅修改 disabled
- **THEN** paths 仍保留原始引用，disabled 更新正常保存。

### Requirement: Settings updates reject invalid writable sections
设置读改写 SHALL 严格解析其写回的配置段；存在但类型错误的段 SHALL 导致错误并保留原文件，不执行修改闭包。缺失段可以使用默认值，未知字段继续按现有合并保留。

#### Scenario: Valid TOML with invalid skill field type
- **WHEN** skills.paths 为整数，用户修改其他设置
- **THEN** 返回 skills 段类型错误，原文件不变。

#### Scenario: Unknown field in otherwise valid section
- **WHEN** 合法配置段包含未知字段并修改一个已知字段
- **THEN** 已知字段更新且未知字段保留。

### Requirement: Empty known skill settings retain unknown fields
技能配置的已知字段全部清空时，保存 SHALL 仅移除已知字段并保留未知字段；仅在没有任何字段剩余时删除 skills 段。

#### Scenario: Unknown-only skill section during unrelated update
- **WHEN** skills 仅包含未知字段，用户修改其他设置
- **THEN** 未知字段保留。

#### Scenario: Last known skill field cleared
- **WHEN** 最后一个已知技能字段被清空
- **THEN** 未知子表保留；没有未知字段时 skills 段删除。

### Requirement: Config temporary files preserve privacy and clean up
配置原子写入 SHALL 使用同目录独占临时文件，失败时清理未提交文件。Unix 新配置 SHALL 默认仅所有者读写；已有目标权限 SHALL 在写内容前应用，权限读取或设置失败 SHALL 返回错误。

#### Scenario: New Unix config
- **WHEN** 创建新的配置文件
- **THEN** 文件权限为 0600 或更严格。

#### Scenario: Existing private config update
- **WHEN** 覆盖已有 0600 配置
- **THEN** 临时文件及最终文件不扩大权限。

#### Scenario: Replacement fails
- **WHEN** 临时文件不能替换目标
- **THEN** 返回错误，目标不变且临时文件被清理。

### Requirement: Settings updates persist explicit field clearing
设置读改写 SHALL 删除修改前存在、修改后明确清空并不再序列化的已知字段，包括嵌套字段。未修改和未知字段 SHALL 保留。

#### Scenario: Restore optional setting defaults
- **WHEN** 取消子任务策略、屏幕模式或默认模型被设为 None
- **THEN** 对应旧配置键删除，重新读取使用默认值。

#### Scenario: Nested clearing with unknown siblings
- **WHEN** 一个已知嵌套字段被清空，同表还有未知字段
- **THEN** 仅已知字段删除，未知字段保留。

### Requirement: Skill configuration path component boundaries
技能添加清理 ignore 与技能路径计数 SHALL 使用路径组件包含关系，不能将仅共享字符串前缀的相邻路径视为祖先或后代。

#### Scenario: Add beside ignored directory
- **WHEN** 添加 foo，ignore 包含 foobar 和 foo 的祖先、后代或自身
- **THEN** 保留 foobar，仅移除与 foo 真正重叠的 ignore，重复添加不重复记录路径。

#### Scenario: Count source skills
- **WHEN** 统计 foo 下的技能，同时存在 foobar 下的技能
- **THEN** 只统计 foo 本身或其后代，添加响应复用相同语义。

### Requirement: Skill management compares resolved path aliases
技能管理 SHALL 在添加去重、取消 ignore、移除和来源计数时比较解析后的文件系统路径。比较不得重写保留条目的配置原文。配置相对路径沿用发现器的进程工作目录基准，请求相对路径使用请求工作目录。

#### Scenario: Add already configured symlink target
- **WHEN** 配置 paths 和 ignore 使用指向现有技能目录的符号链接，用户添加其真实路径
- **THEN** 清除对应 ignore，不新增重复路径，已有 paths 写法保留。

#### Scenario: Remove or count a symlink source
- **WHEN** 配置使用现有符号链接，用户按真实路径移除，或统计该来源
- **THEN** 移除对应配置项，来源计数包含真实路径下的技能且不包含相邻目录。

### Requirement: Raw skill config comparisons expand environment references
技能管理 SHALL 在比较原始配置路径时应用运行时配置的环境变量展开规则，再解析路径；不得将该展开写回保留条目，也不得对普通请求路径额外展开。

#### Scenario: Manage environment based configured path
- **WHEN** paths 与 ignore 使用环境变量引用一个现有目录，用户添加该真实目录
- **THEN** 清除对应 ignore、不重复添加、保留原 paths 引用；按真实路径移除时删除对应项。

#### Scenario: Literal request path
- **WHEN** 普通请求的路径文本包含 ${HOME}
- **THEN** 路径解析保留该字面文本，不将配置展开规则应用到请求。

### Requirement: Missing skill paths are anchored before canonicalization
在进程工作目录可读取时，技能路径解析 SHALL 先将相对请求 cwd 与路径锚定为绝对路径，不以目标是否存在为条件。

#### Scenario: Missing target with default cwd
- **WHEN** 目标不存在，技能请求使用默认点 cwd 和相对路径
- **THEN** 返回锚定到当前进程目录的绝对路径，添加保存该路径。

#### Scenario: Missing target with relative cwd
- **WHEN** 目标不存在，请求 cwd 本身为相对目录
- **THEN** 将 cwd 与目标一起锚定到当前进程目录，不返回相对配置路径。

### Requirement: Skill reset and config reject invalid parameters
技能 reset 与 config 接口 SHALL 在执行配置读写或发现之前校验可选 cwd 参数，类型错误 SHALL 返回 invalid_params，不回退为有效默认请求。

#### Scenario: Invalid reset parameters
- **WHEN** reset 或 config 收到 null、数组或非字符串且非 null 的 cwd
- **THEN** 返回 invalid_params，不执行配置清空或技能发现。

#### Scenario: Default cwd request
- **WHEN** 请求为空对象或 cwd:null
- **THEN** 保持有效默认 cwd 请求，字符串 cwd 也正常接受。

### Requirement: Skill extension reload has a bounded execution boundary
技能扩展重载 SHALL 将同步扫描与异步请求执行隔离，限制仍未退出的扫描数量，且截止时间包含等待执行资格的时间。请求超时不得被表述为阻塞文件调用已中断。

#### Scenario: Scan remains blocked after timeout
- **WHEN** 一次扫描未退出但调用者已超时，随后又有重载请求
- **THEN** 旧扫描继续占用执行资格，后续请求不得无限创建扫描任务，仍可按截止时间失败。

### Requirement: Skill reload failure is distinct from empty discovery
技能扩展 SHALL 将重载超时或 worker 失败返回为错误，不转换为成功空列表；已完成的配置保存 SHALL 不被宣称回滚。

#### Scenario: Reload fails after settings save
- **WHEN** 添加、移除或 reset 已完成保存，后续重载失败
- **THEN** 返回明确的重载失败，保留已保存配置，不返回空成功技能目录。

#### Scenario: Empty discovery succeeds
- **WHEN** 扫描正常完成且未找到技能
- **THEN** 返回正常的空技能列表。

### Requirement: Config skill scope uses resolved source identity
配置技能 SHALL 根据配置根与仓库根的规范路径包含关系分类 Repo/User，不能因符号链接写法改变同一根的分类；扫描输入原文 SHALL 保持不变。

#### Scenario: External alias points into repository
- **WHEN** 配置路径在仓库外，但链接到仓库内现存技能根
- **THEN** 该配置来源分类为 Repo。

#### Scenario: Repository alias points outside
- **WHEN** 配置路径文本在仓库内，但链接到仓库外技能根
- **THEN** 该配置来源分类为 User。

### Requirement: Automatic skill discovery respects repository boundary through cwd aliases
自动技能发现 SHALL 在 cwd 和 Git root 的规范路径上执行祖先遍历与停止判断，符号链接 cwd 不得导致越过仓库边界或漏掉仓库内来源。

#### Scenario: Cwd alias outside repository
- **WHEN** cwd 是仓库外指向仓库子目录的链接，链接父目录也有 .grow
- **THEN** 发现实际子目录至仓库根的 .grow，不从链接的外部祖先发现项目技能。

#### Scenario: Local grow directory is a link
- **WHEN** 实际 cwd 的 .grow 链接到共享目录
- **THEN** 仍按本地入口赋予 Local scope，不因目标位置改成 User。

### Requirement: Skill walks do not revisit ancestor directories
技能递归扫描 SHALL 在加入技能文件及递归之前拒绝指向当前祖先目录的链接，使用规范目录身份；不同非祖先别名入口 SHALL 保留既有词典序与发现行为。

#### Scenario: Child links back to scan root
- **WHEN** 子目录链接回已有 SKILL.md 的扫描根
- **THEN** 不经链接再次加入根技能或重复递归，正常子技能仍发现。

#### Scenario: Independent aliases share a target
- **WHEN** 两个非祖先链接指向同一技能目录
- **THEN** 两入口仍按词典序发现，后续身份去重负责消除重复文件。

### Requirement: Injected skill directories include their root skill
Server/Bundled 注入目录 SHALL 使用根 SKILL.md 加递归子目录的统一发现语义，保留对应来源 scope 和规范文件去重。

#### Scenario: Injected directory is itself a skill
- **WHEN** 注入目录根及子目录均有有效 SKILL.md
- **THEN** 二者均被发现并保持 Server 或 Bundled scope。

#### Scenario: Injected path repeated
- **WHEN** 同一注入目录重复配置
- **THEN** 根技能与子技能各保留一次。

### Requirement: Skill description previews bound underlying reads
描述回退 SHALL 在底层读取时限制为 frontmatter 预算加正文预览预算，允许额外一个探测字节；不得为生成短描述完整读取任意长度正文。显式技能正文加载不受此预览上限影响。

#### Scenario: Large body needs fallback description
- **WHEN** 技能需要回退描述且文件超过预览预算
- **THEN** 只读取预算及一个探测字节，在预览内生成描述。

#### Scenario: UTF-8 crosses preview boundary
- **WHEN** 截断边界落在多字节字符内部
- **THEN** 丢弃末尾不完整字符；预览内部非法 UTF-8 仍报读取错误并回退技能名。

### Requirement: Oversized skill frontmatter is not downgraded to plain content
技能发现 SHALL 拒绝已识别 opening fence 后超过 frontmatter 读取预算的文件，不得将截断的元数据当成无 frontmatter 普通技能。

#### Scenario: Restrictions occur after oversized metadata
- **WHEN** 已开始的 frontmatter 超限，限制字段位于未读部分
- **THEN** 读取返回错误，发现跳过技能，不以默认限制值加载。

#### Scenario: Long plain body has no frontmatter
- **WHEN** 长正文没有 opening fence
- **THEN** 保持普通技能发现及有界描述预览。

### Requirement: Malformed skill metadata is not replaced with defaults
技能发现 SHALL 跳过已开始但未闭合的 frontmatter 以及 YAML 语法错误，不得用默认元数据替换损坏的声明。

#### Scenario: Incomplete or invalid header
- **WHEN** frontmatter 有 opening fence 但未闭合，或闭合内容存在 YAML 语法错误
- **THEN** 发现跳过该文件，不生成默认无限制技能。

#### Scenario: Plain Markdown skill
- **WHEN** 内容没有 frontmatter opening fence
- **THEN** 继续允许按目录名和正文描述回退发现。

### Requirement: Skill paths reject invalid field types
技能 paths 限制 SHALL 接受字符串或纯字符串列表，错误类型及混合列表 SHALL 返回解析错误，不得静默丢弃并产生无条件技能。

#### Scenario: Invalid paths type
- **WHEN** paths 为数字、布尔、映射或含非字符串的列表
- **THEN** 发现跳过技能，不将该字段视为缺省或部分有效。

#### Scenario: Valid optional paths
- **WHEN** paths 为合法字符串/字符串列表，或 null、空值、匹配全部
- **THEN** 保留既有分隔、归一化及无条件语义。

### Requirement: Skill invocation switches reject invalid boolean values
user-invocable 和 disable-model-invocation SHALL 接受 YAML 布尔及 true/false 字符串，其他显式值 SHALL 触发解析错误，不得默认为 false。

#### Scenario: Invalid invocation switch
- **WHEN** 任一调用开关为 yes、数字、null、列表或映射
- **THEN** 发现跳过该技能，不按默认调用能力加载。

#### Scenario: Valid switches and defaults
- **WHEN** 开关为合法布尔/字符串或未声明
- **THEN** 保留显式值，缺省 user-invocable 为 true、disable-model-invocation 为 false。

### Requirement: Expanded skill reference index reflects loaded blocks
技能展开提示的引用索引 SHALL 只列出实际成功生成正文块的技能，且保持成功顺序。

#### Scenario: Mixed successful and failed expansions
- **WHEN** 多技能请求中有可加载技能、缺失文件或消失的目录条目
- **THEN** 正文与引用索引只包含加载成功项，不能将失败项列为已加载。

#### Scenario: All expansions fail
- **WHEN** 所有技能均未生成正文块
- **THEN** 继续返回 None，不生成空引用信封。

### Requirement: Plugin skill usage reports expansion outcome
Turn admission 的插件技能使用诊断 SHALL 按实际正文展开结果设置 PluginUsed.success，不得在加载前无条件宣称成功。

#### Scenario: Plugin skill expansion fails
- **WHEN** 请求的插件技能正文读取失败或目录条目已消失
- **THEN** 对应 PluginUsed.success 为 false。

#### Scenario: Mixed plugin expansion results
- **WHEN** 同一请求中部分技能成功生成正文块、部分失败
- **THEN** 逐引用分别记录成功或失败，不以整体存在正文代替单项结果。

#### Scenario: Skill interjection
- **WHEN** 技能通过 interjection 注入运行中的 turn
- **THEN** 保留派发诊断，不新增代表 turn 归属的 PluginUsed 事件。

### Requirement: Skill expansion preserves selected source identity
技能展开 SHALL 同时匹配解析时选定的路径、限定名称与插件身份，不得仅凭同一路径使用其他来源条目。

#### Scenario: Native and plugin skills share a path
- **WHEN** 原生技能和插件技能路径相同，用户选定插件技能
- **THEN** 展开使用该插件条目的正文快照和插件变量。

#### Scenario: Selected source disappears
- **WHEN** 选定插件条目消失但目录仍有同路径原生技能
- **THEN** 该引用展开失败，不回退到原生条目。

### Requirement: Skill envelope attributes escape special characters
技能正文块的 name/args、引用索引的 name/path 及预加载消息的 name/description/path SHALL 转义 XML 属性特殊字符，正文内容 SHALL 保持原样。

#### Scenario: Quoted skill arguments and paths
- **WHEN** 名称、参数或路径包含双引号、尖括号或 &
- **THEN** 对应属性输出实体转义，字符不得形成额外属性或标签。

#### Scenario: Body and duplicate references
- **WHEN** 正文包含 Markdown/标记且引用重复
- **THEN** 正文保持原样，引用仍按原始名称与路径去重。

### Requirement: Managed syntax validator exit cleanup
托管配置语法检查器 SHALL 在观察到直接子进程退出后清理其已持有进程组，再报告验证结果；清理 SHALL 使用有界等待。

#### Scenario: Successful validator leaves descendants
- **WHEN** 检查器以零状态退出且同组后代仍运行
- **THEN** 终止同组后代，再返回成功；清理失败则返回验证错误。

#### Scenario: Failed validator leaves descendants
- **WHEN** 检查器以非零状态退出
- **THEN** 执行相同清理，保留退出错误并附加清理失败信息。

### Requirement: Managed config rollback preserves observed conflicts
托管配置事务 SHALL 在回滚前核对已发布文件身份、字节和权限；观察到变化或符号链接替换时 SHALL 保留当前目标、保留已有原始备份并报告实际备份路径，不继续覆盖或删除目标。该检查 SHALL NOT 被描述为对非协作写入者的原子 CAS。

#### Scenario: Existing target changes after publication
- **WHEN** 发布失败后的恢复发现目标已被外部编辑、替换或删除
- **THEN** 返回恢复错误，保留现状及实际原始备份，不覆盖外部内容。

#### Scenario: New target changes after publication
- **WHEN** 本次创建的目标在回滚前发生可观察变化
- **THEN** 返回恢复错误并保留现状，不删除新内容。

#### Scenario: Published target remains unchanged
- **WHEN** 发布后发生错误且目标仍为本事务发布的身份、内容和权限
- **THEN** 按既有规则恢复原内容或移除本次新建文件。

### Requirement: Managed config reads obey byte budgets
托管配置源读取 SHALL 以实际读取量执行4 MiB上限；发布与回滚前验证 SHALL 以计划输出字节长度为上限，回滚后验证 SHALL 以原始内容长度为上限。允许读取一个额外字节以检测超限，超限 SHALL 返回错误而非接受截断内容。

#### Scenario: Source grows beyond metadata size
- **WHEN** 元数据检查后读取到超过4 MiB的源内容
- **THEN** 在最多读取4 MiB加1字节后拒绝，不继续收集。

#### Scenario: Published file grows beyond planned length
- **WHEN** 验证读取的内容超过计划长度
- **THEN** 有界拒绝，并遵循既有冲突恢复契约保留外部内容。

#### Scenario: Exact budget
- **WHEN** 内容长度恰好等于读取预算
- **THEN** 返回完整内容，继续已有内容与权限检查。

### Requirement: Managed source fields share a file handle
托管配置源状态的内容、身份与权限 SHALL 来自同一个已打开文件对象，不将路径预检元数据与另一个文件的内容合并。

#### Scenario: Path replaced after opening
- **WHEN** 源文件打开后其路径被另一个文件替换
- **THEN** 已打开源的读取仍使用该句柄的内容、权限和身份，后续路径读取使用新的对象；不混合两者。

#### Scenario: Opened object validation
- **WHEN** 已打开对象不是普通文件或长度超过上限
- **THEN** 在收集内容前拒绝，实际读取仍受既有字节预算限制。

### Requirement: Managed plans preserve source readability
托管配置规划 SHALL 拒绝最终内容超过4 MiB的计划及正文或注释前缀含NUL的请求，不写出已知违反源读取规则的文件。

#### Scenario: Adding a block exceeds the source limit
- **WHEN** 渲染后的完整输出超过4 MiB，包括原文与标记
- **THEN** 规划返回明确超限错误，原文件与缺失状态保持，不生成事务文件。

#### Scenario: Exact output limit
- **WHEN** 输出恰好4 MiB且其他验证通过
- **THEN** 允许应用，后续可正常读取和幂等规划。

#### Scenario: NUL in requested body
- **WHEN** 任一请求条目正文包含NUL
- **THEN** 在规划写入前返回无效请求错误，保持源不变。

#### Scenario: NUL in comment prefix
- **WHEN** 构造的注释前缀包含NUL
- **THEN** 返回无效请求错误，不允许该前缀进入渲染。

### Requirement: Atomic state writers preserve unowned temporary paths
The shared atomic state writer SHALL leave a preexisting temporary path untouched when exclusive creation fails. Cleanup after write or publication failure SHALL apply only after this attempt has successfully created its temporary file. Successful replacement SHALL preserve the requested Unix creation-mode behavior.

#### Scenario: Temporary name collision
- **WHEN** exclusive creation fails because the temporary path already exists
- **THEN** both that path and the existing destination remain unchanged

#### Scenario: Publication fails after creation
- **WHEN** this attempt creates a temporary file but cannot publish it
- **THEN** its temporary file is cleaned up while the destination remains intact

### Requirement: Credential helper failures do not echo output payloads
Credential helper failures SHALL NOT include raw stdout or stderr content in returned or logged failure messages. Diagnostics SHALL preserve structural failure information without credential values. Pipe draining and output limits SHALL remain in force.

#### Scenario: Helper stderr contains a credential
- **WHEN** a helper exits unsuccessfully after writing credential-bearing stderr
- **THEN** the failure reports exit status and output size without echoing stderr.

#### Scenario: Malformed JSON contains sensitive values
- **WHEN** token JSON cannot be decoded because a field contains an invalid value
- **THEN** the failure reports JSON category and location without embedding that value.

### Requirement: Relative credential helper cwd resolves once
A configured relative credential-helper cwd SHALL resolve against the Grow process working directory before program resolution and child startup. The resolved directory SHALL be used consistently for relative executable paths and the child working directory.

#### Scenario: Relative executable under relative cwd
- **WHEN** a direct-exec helper specifies a relative cwd and a command such as ./token.sh
- **THEN** it starts the program inside that directory without applying the cwd prefix twice, and relative file reads use that directory.

### Requirement: Valid short-lived helper tokens remain sendable
The credential helper refresh margin SHALL trigger proactive refresh without itself making an otherwise valid token unavailable to the request-time bearer resolver. Actual expiry, token-shaping configuration changes, and a failed attempted refresh SHALL make the cached token unavailable for sending.

#### Scenario: Newly minted token inside refresh margin
- **WHEN** a helper returns a token with 30 seconds remaining
- **THEN** the request-time resolver returns that token until actual expiry or invalidation.

#### Scenario: Proactive refresh fails
- **WHEN** refreshing a near-expiry cached token fails
- **THEN** the prior token is unavailable for sending even if its original expiry is still in the future.

### Requirement: Retired inert configuration surfaces are not exposed
Shell SHALL NOT expose the empty unstable Cargo feature or the unconsumed ZDR-access setting/resolver. Removing these inert surfaces SHALL NOT introduce a new access policy.

#### Scenario: Shell feature and setting inventory
- **WHEN** inspecting supported Shell build features and remote setting types
- **THEN** the empty unstable feature and unused zdr_access_enabled field are absent.

### Requirement: Builtin publication syncs a readable directory
On Unix, builtin extraction SHALL synchronize renamed managed files through a readable descriptor opened relative to the existing pinned directory capability. The generation marker SHALL remain the last published file and failures SHALL remain observable to the transaction caller.

#### Scenario: First extraction on Linux
- **WHEN** a fresh Grow home receives one builtin extraction transaction on Linux
- **THEN** all managed files and the matching version marker are published successfully without requiring repeated startup attempts to advance the generation.

#### Scenario: Managed parent is invalid
- **WHEN** a managed parent is a file or a symlink
- **THEN** extraction fails without publishing a successful generation marker or writing through the symlink.
