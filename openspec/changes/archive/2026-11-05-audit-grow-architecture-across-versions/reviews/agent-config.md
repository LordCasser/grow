# Agent / config / plugin / extensions 跨版本架构审视

本分区完成了当前实现、调用方和相关测试的审视，以及全部 46 个本地发布标签的模块树、manifest、相邻变化核对。确认 8 项当前问题：配置写入绕开安全持久化入口、空插件快照遗留 Hook、会话插件快照被共享快照替代、规则目录别名越界、原生 Agent 扫描的符号链接环、Skills 配置事件未交付、直接插件更新丢失 registry 更新、插件持久化失败仍报告成功。下文分别给出触发条件、证据、所有者和通过标准；没有修改业务代码，也没有将历史已修复问题重复列作当前缺陷。

## 1. 范围与证据基线

- HEAD 为 `d6128138cacdc22553aba9dafba9bc6cc934e4d1`；v2.6.0 解引用后的 commit 为 `5475dff997f05cf6c67ab8ba24621ac8863c144a`（annotated tag 对象为 `cc78f239d118`），两者不是同一 commit。本分区在提交态的模块树及选定消费方指纹相同，因此提交态证据可复用；这不表示整个仓库相同。
- 当前工作树另含 repo discovery / trust、model / effort、resume 等并行改动。Agent 的 `builder.rs`、`config.rs`、`discovery.rs`、`prompt/agents_md.rs`、`prompt/skills.rs`、`repo.rs`，Shell 的 `extensions/skills.rs`、`mvp_agent/{acp_agent,agent_ops,mod}.rs` 等均为既有活动写面。以下行号以审阅时工作树为准；每项发现另核对提交态，不能把活动修改当作缺陷引入点。
- 核心范围为 `agent`、`config`、`config-types`、`plugin-marketplace`、`extension-types`、`announcements`、Shell `config` / `extensions`；为核对边界读取了 Shell toolset / model catalog / session plugin consumers、Tools 的 skill discovery / SkillManager / LSP config、Workspace project config、Pager 配置重载消费者。其他专项扩展的领域逻辑由对应分区审视；本分区审视其公共分派和配置/插件边界，不宣称逐个复核 Git、PR、terminal、coordination 等领域算法。
- 契约依据为 [configuration-rules](../../../../specs/configuration-rules/spec.md)、[extension-runtime](../../../../specs/extension-runtime/spec.md) 和 [model-sampling](../../../../specs/model-sampling/spec.md)；历史实现与 change 仅用于定位演进。已读取本 change 的 proposal、design、tasks、inventory，并按 `software-engineering` 入口、Rust、目录结构和 handoffs 参考执行。
- 当前 skill catalog 不含 Atlas，`load_skill` 加载失败；Atlas 懒加载 calls 查询也没有返回可用调用图。调用关系结论来自源文件、全仓符号搜索和 Git 对照，不依赖未完成的索引结果。
- 唯一仓库写面为本报告。未编辑代码、backlog、主规范或其他 reviewer 的文件；未提交 Git。父 agent 负责合成、债务登记、活动功能复测及全局 OpenSpec 校验。

## 2. 模块、状态所有者与关键链

| 模块 / 边界 | 当前职责与状态所有者 | 已核对的链与结论 |
|---|---|---|
| `agent` | 解析 AgentDefinition、按来源发现定义、合成 PromptContext、构建 Agent 与工具能力；不拥有 provider 会话或持久化状态 | `discovery → AgentDefinition → AgentBuilder → ToolBridge`；runtime gate、作者 allow/disallow、最终 session clamp 顺序均有实际消费；无生命周期 observer 时禁止背景 Bash。原生递归发现见 AC-05。 |
| Agent prompts / rules | stable mandatory/audience system head 与可变 role 分开；规则按来源和目录层次投影；正文加载由具体读取路径负责 | `prompt/context.rs::{render,render_role}`、`agents_md.rs`、`prompt/skills.rs`、Shell session setup；当前角色/系统头分工不是两个相互独立的可变事实源。规则别名链见 AC-04。 |
| `config` | `$GROW_HOME/config.toml` 的原始/有效配置、环境展开、版本覆盖、campaign 层，以及 atomic / managed text 文件事务 | `loader → effective config → shell typed config`；通用 loader 不把 cwd 同名配置误当个人配置，项目合并由 subsystem 消费方显式完成。managed text rollback 校验身份和原字节，冲突保留目标/备份。 |
| `config-types` | 跨 crate 的配置值、RemoteSettings 和 feature flags；不拥有配置刷新执行器 | 实际 manifest 依赖 `config`、ACP、tool-protocol 等，属于值接口层，并非零依赖 DTO。RemoteSettings 留有上游注入入口，当前产品入口通常传 `None`；不能仅凭字段名或兼容注释判定生产旁路。 |
| `agent::plugins` | discovery / manifest / origin / trust / enable 状态、InstallRegistry、每会话 PluginRegistry，以及共享的 latest handle | 同一个名字的来源优先级、canonical roots、project trust、CLI roots、session `pluginDirs` 均已追踪。`build_for_cwd` 构造会话快照且不改 shared state；`reload` 才发布 shared latest。空值与消费者分歧见 AC-02/03。 |
| `plugin-marketplace` | canonical index、source cache lease、相对路径校验、来源解析、install / transactional update | remote Git operand/ref/SHA 校验、require-sha、staging→backup→publish→registry save→失败回滚均有真实路径；新旧目录与 registry 一致性有专用错误。直接插件 action 未统一消费这些结果，见 AC-07/08。 |
| `extension-types` | hooks/plugins/marketplace ACP wire DTO；仅 serde 是运行依赖，无 shell/agent 回依赖 | Shell 将运行状态转换为 DTO，Pager 消费；PluginComponents 的限长/去控制字符是显式 sanitize，不是反序列化自动保证。旧 `xai-hooks-plugins-types` 已单独追踪，见迁移表。 |
| `announcements` | announcement 值、隐藏 ID 的有界读取与原子保存、格式化 | 1 MiB 上限、非 regular file、FIFO、符号链接、私有临时文件、原子替换失败和稳定 ID 集合有测试覆盖；读取在 blocking worker，未确认新的运行缺陷。其 `tools → grow_home` 依赖只是配置根转发，见独立整理项。 |
| Shell `config` | memory/subagents/tools/plugins 的解析与 gate；ConfigReloader 比较有效值；watcher 管理 native 注册 | Config watcher 的 resident owner acquisition/release、目录实体替换、取消和 blocking worker 所有权已核对；Settings raw 写入链与 plugin 写入链仍分叉，见 AC-01。Skills 事件发出后未到达执行 owner，见 AC-06。 |
| Shell `extensions` | ACP 参数/错误映射、路由、会话 actor 命令、DTO 发布；不应复制 runtime truth | plugins/list 已向指定 session 取 registry；skills 仍取 shared registry。Hooks 列表优先显示 raw command/url；MCP direct result 有明确体积上限。各领域执行细节保留所属分区审查边界。 |
| toolset / model catalog 接口 | toolset authoring 与 operator clamp 分离；ModelsManager 一个锁内拥有 models、config、current selection、revision | file-toolset 标准/concise/hashline 的实际构建与最终工具 ceiling 已核对；model reload 先验证完整候选，再一次发布目录并通知，非法候选不拆散旧快照。活动 model/effort/resume 的最终整体验证由父 agent 负责。 |

配置读取和写回具有不同契约。runtime loader 允许环境展开、version/campaign 覆盖；Settings update 从 raw TOML 编辑，保留环境引用和未知字段，并在读取/写段解析失败时停止。`util/config/persist.rs` 的 `SAVE_LOCK`、同目录 exclusive tempfile、permissions-before-write 与 atomic publish 是现有正确边界；AC-01 不是缺一个新的通用配置框架，而是既有调用方绕过了它。

SkillManager 是 merged startup/discovered catalog 的 runtime owner。`session_setup.rs::reload_skills_from_disk` 从新配置与会话 plugin snapshot 构造 baseline，通过 bridge 更新，再从同一 owner 生成 slash advertisement。`PromptContext` 不再维护平行 AvailableSkills 快照。显式加载按 exact identity 先解析来源再判定 eligibility；预加载失败不静默省略，并保留 catalog 条目。这些已实现的边界与 AC-03、AC-06 的上游交付缺口要分开。

Project plugin 的可执行组件实际使用 folder trust verdict；`plugins/trust.rs`、Shell folder_trust 文件中的部分注释仍描述独立旧 plugin trust。执行结论以 `plugins/discovery.rs` 的 origin/scope 判定与 `active_plugins()` 为准。不能根据旧注释推导“信任文件夹后插件仍必然被独立阻断”。

## 3. 当前可行动发现

P1 表示可丢失用户配置、遗留可执行控制或把另一会话的插件配置投入执行；P2 表示当前可达的错误结果、目录失真或更新/错误反馈失真。优先级用于父 agent 合成，不等于已决定纳入当前 model/effort change。

### AC-01 · P1 · 插件和 CTA 写入绕过安全配置事务，硬读取错误可覆盖原配置

**位置与可达链。** `shell/src/config/mod.rs::add_plugin_path`（709/711/744）、`add_disabled_plugin`（774/776/811）、`add_dismissed_plugin_cta_to_file`（847/851/885）、`add_enabled_plugin`（987/989 及末尾 write）。相邻 remove helpers 传播硬读取错误，但同样直接 `std::fs::write`。插件 actor 的 Add/Enable/Disable/Remove（`session/actor/hooks_plugins.rs:490` 起）、CLI 插件操作及 CTA dismissal 都可达这些函数。对照 `shell/src/util/config/persist.rs:11`、`:51`、`:127`、`:141`。

**触发与不变量。** 已存在 config.toml 含非法 UTF-8，或读取因其他非 NotFound 错误失败，但写入可成功；执行 Add、Enable、Disable 或 dismiss CTA。`read_to_string(...).unwrap_or_default()` 把失败变为空配置，随后只保存本次插件/CTA 段，丢失原字节。语法不合法但仍可读取的 TOML 会在 `toml::from_str` 返回错误，不在此覆盖结论之内。另一个独立触发是 Settings update 已持有 `SAVE_LOCK` 进行异步读改写，同时插件路径从另一 session/thread 直接写同一文件：插件不参与该锁，最后发布者覆盖另一方的新值；直接 truncate/write 还允许读取方观察中间状态。

**契约与现有覆盖为何不够。** configuration-rules 的 `Config writes stop on existing-file read errors`、`Settings updates edit raw config values` 及 atomic tempfile 要求已被 Settings 主入口实现；插件 helpers 有自己的 raw TOML 路径，未复用读取错误处理、串行化与发布机制。普通成功写入/去重测试不能证明失败时保留配置。

**历史。** v1.0.0 的 `grow-shell/src/config/mod.rs` 已有相同读失败置空和直接写入；逐标签核对 46 tag，机制持续至 HEAD。v2.1.5 的 `175b08368` 修好了 Settings 的硬读取错误及临时文件权限，未覆盖这组插件 helpers，因此不是“整个配置保存仍未修复”。

**修复 owner 与通过标准。** Shell config persistence / plugin application owner；将所有 config.toml 变更纳入已有 raw mutation + 同一锁 + atomic publish，保持 env/未知字段。用 path 参数化入口分别验证非法 UTF-8、非 NotFound 读失败、写/rename 失败保留原字节；在 Settings 与 plugin update 之间插入确定性屏障，两个不冲突修改都保留；missing file 仍可创建。无需新增配置服务或合并无关配置功能。

**验证状态。** 真实调用链和失败分支已静态核实；没有执行实际用户 config 写入，也未运行故障注入集成测试。

### AC-02 · P1 · 最后一个插件消失时，空快照不清除旧插件 Hook

**位置与可达链。** `agent/src/plugins/registry.rs::reload:429` 在 registry 为空时发布 `None`（448）；`build_for_cwd:345` 在无发现、无 session roots 时同样返回 None。`shell/src/session/actor/hooks_plugins.rs::reload_plugins_impl:717` 和 `ReloadPlugins` fan-out 都调用 `apply_plugin_registry_snapshot:841`。该函数先把会话 registry 改成新值（849），但 `remove_by_prefix("plugin/")`（890）和 `mark_hook_registry_changed`（906）均位于 `if let Some(ref new_registry)`（854）内部。

**触发与后果。** 会话已有一个插件及已载入 command/HTTP Hook；移除最后一个 plugin path、卸载后显式 reload，或让最后一个 discovery 候选消失，且没有需要保留的 session plugin roots。registry 变为 None、列表为空，旧 `plugin/...` Hook 仍留在 live HookRegistry，后续匹配事件继续执行旧 handler。移除源文件也不会自动删除已解析的 command/url。单纯禁用最后一个插件通常仍保留非空 registry（Some，active 集为空），会走清除分支；此情况不是复现条件。

**不变量与已有正确对照。** 会话发布的新 plugin snapshot 应决定执行组件全集，空集合也应撤销先前贡献。该函数的 MCP 合并刻意无条件执行（918 起），能移除空贡献；hooks 在 Some/None 分支中不对称。旧 HookRegistry 是另一状态 owner，单换 plugin Arc 不会使它失效。

**历史。** v1.0.0 旧路径 `grow-shell/src/session/acp_session_impl/hooks_plugins.rs` 即有 Some-only 清除；46 个 tag 均保留此控制结构，当前未修复。

**修复 owner 与通过标准。** Session plugin adoption / hook registry owner。将清除旧插件 hooks 和 registry generation 变更覆盖 None，保留 config/file/inline Agent hooks。真实 actor 测试先注册一个可观察副作用 Hook，采用 None，再触发同类事件：副作用不发生、hook list 与 plugin list 一致；Some(empty-active) 与 None 都通过，并验证 MCP/skills 清理不回归。

**验证状态。** Some→None 的产生者、两条实际采用链与执行 registry 的清除条件已核实；未运行 Hook 进程集成测试。

### AC-03 · P1 · 会话已有独立插件快照，LSP 启动却使用进程共享快照

**位置与可达链。** `shell/src/agent/mvp_agent/agent_ops.rs:1655` 用 cwd、fresh discovery config、`_meta.pluginDirs` 和该 cwd trust 构造 `session_plugin_registry`，Agent 解析也使用它；`:1736` 的 LSP 段另取 `self.plugin_registry_handle.snapshot()`。`agent/src/plugins/registry.rs:386` 的 `refresh_and_build_for_cwd` 只调用 per-cwd builder，不替换 shared state。shared snapshot 由 launch-dir lazy 初始化（`agent_ops.rs:148`）或其他会话 reload 更新。

**触发与后果。** LSP 开启、`tool_ctx.lsp` 为 None：会话携带仅属于它的 pluginDirs，其 LSP 丢失；或进程/shared snapshot 来自工作区 A，随后创建工作区 B 的会话，A 的 active plugin LSP 被投入 B 的 manager 并启动。LSP loader 只在合并前过滤 B 的 Project config；已作为允许 Plugin 来源传入的 A 配置不会再被该 gate 移除（`tools/src/implementations/lsp/config.rs:20` / `:140`）。因此既有项目 shadow 修复不能覆盖此错误上游快照。

**相关接口失真。** `mvp_agent/acp_agent.rs:2216` 把同一 shared snapshot 传给整个 `grow/skills/*`，而 `extensions/skills.rs:501` 按请求 cwd 发现 native skills 后合并该 registry。`plugins/list` 已根据 sessionId 取会话 registry（`extensions/plugins.rs:99`），Skills 接口只有 cwd，尚不能精确表达 session pluginDirs。可以确认“指定 cwd 的原生目录 + 另一 cwd 的插件目录”混合和 UI/catalog 不一致；修复需明确 skills endpoint 的 cwd/session 作用域，不能假定 cwd 唯一对应一个 session。

**历史。** shared LSP read 与 Skills shared 参数在 v1.0.0 已存在。`ea0a35942`（v2.0.1）强化 per-cwd/session registry 后，LSP shared read 保持不变；46-tag 机制对照不能证明每个早期版本拥有今天全部会话契约，本报告不把新契约倒套到旧版本。

**修复 owner 与通过标准。** Session bootstrap / plugin catalog owner，Skills API owner参与。LSP 使用本次 session construction 的同一 registry；A/B 使用不同插件且同一 server name时各自看到并执行本会话来源，B 不启动 A command；session pluginDirs 的 file/inline LSP 均生效。Skills 列表、toggle 和模型可加载身份使用明确相同作用域，包含两个相同 cwd、不同 pluginDirs 的反例。保留父子 LSP 共享的既有语义，不把每次目录刷新变成 LSP 全量重启。

**验证状态。** 静态核实构造、发布和 source merge；未启动 LSP 进程或 SDK 双会话集成。

### AC-04 · P2 · AGENTS 规则链以 lexical cwd 对比 canonical repo root，别名可越过 checkout

**位置与可达链。** `agent/src/prompt/agents_md.rs::read_agents_config_with_roots:162` 保留原 cwd（167），root 由 repo discovery 规范化返回；循环仅比较 `dir == root.as_path()`（181）并使用 lexical parent。ACP new/load 仅经 `AbsPathBuf::new`（`mvp_agent/acp_agent.rs:276/:684`），该类型只验证 absolute / UTF-8（`paths/src/lib.rs:90`），不 canonicalize，所以 API 调用可达；inspect 也直接调用 production rule discovery。

**触发与后果。** `outside/alias` 是指向 `checkout/src` 的目录符号链接，以该别名启动会话。物理 repo root 是 checkout，但 lexical chain 是 `outside/alias → outside → ... → /`，永远不等于 checkout。仓库根 AGENTS.md 被漏掉，outside 的 AGENTS.md / `.grow/rules` 可加入项目规则，违反 checkout 边界与 root→cwd 覆盖顺序。后续 candidate canonical 去重不能补回已漏掉的祖先，也不能撤销已加入的外部规则。

**历史与活动基线。** v1.0.0 的 git2 workdir + lexical walk 已有此结构；当前 repo-discovery 活动改动改用 path-only helper，但没有改变此 loop，不是该改动新引入。单把 equality 改成 canonical equality仍需测试“别名直接指向仓库内部子目录”的情况：它的 lexical ancestors 中本来没有 checkout root，不能未经验证就认为复用另一个 walker 会完整修好。

**修复 owner 与通过标准。** Agent rules / repo boundary owner，与 Workspace reviewer去重。从同一文件系统身份建立 cwd 到 checkout 的正确祖先链，明确展示路径策略；正常 cwd、repo-root alias、指向内部子目录的外部 alias、linked worktree 均只纳入真实 checkout 内层次与已授权 user roots，不出现外部祖先规则，root AGENTS 保留。home-as-repo 的现有例外单独验证。

**验证状态。** std-only 复刻 production loop 的隔离实验已通过：`includes_repo_rule=false, includes_external_rule=true, reaches_filesystem_root=true`。实验验证路径算法，未调用完整 Git/parser/ACP 流水线。

### AC-05 · P2 · 原生 Agent 递归扫描跟随目录符号链接环，放大扫描和身份目录

**位置与可达链。** `agent/src/discovery.rs::load_definitions_from_dir:714 → agent_markdown_files:747`；`:754` 以 `path.is_dir()` 跟随目录符号链接并递归，无 ancestor identity 或深度界限。返回后 `agent_id_from_path:768` 以 lexical relative path 生成 namespace，`seen_names` 无法合并 `reviewer` 与 `loop/reviewer`。启动、by_name 与 catalog 枚举都消费该发现链。

**触发与后果。** `.grow/agents/reviewer.md` 加 `loop → .`。同一物理定义被枚举并解析为一组越来越长的 Agent ID。当前 macOS 单环实验得到 32 个 lexical entries / 1 个物理文件；这是 OS 的符号链接解析上限终止，不是无限递归。分支环可进一步放大遍历，目录/文件规模及别名数没有应用层预算。Tools skill walker 已有 ancestor canonical identity 与 `MAX_SKILL_WALK_DEPTH`，但它不是此 native Agent walker 的调用方，不能保护这里。

**历史。** 递归 namespace 扫描见 pre-tag `095ab550c`；全部46 tag（含v1.0.0）保留相同递归结构，当前未修复。plugin Agent 的单层枚举与 Skill walker 不能据此被一并判错。

**修复 owner 与通过标准。** Agent discovery owner。沿现有递归 walker 检测祖先环并设置合理遍历边界，不新增发现框架；单环、双分支环、长无环嵌套都可确定结束，合法嵌套 namespace 稳定；非环符号链接的允许/去重策略显式说明。测试不依赖“本机正好32层”。

**验证状态。** std-only 逐字复刻该 walker 的隔离实验已执行并断言 `lexical_entries > 1`、`physical_files == 1`；未执行真实 Agent YAML 解析或全量启动。

### AC-06 · P2 · Skills 配置变更事件没有交付现存 SkillManager，新增自定义路径也不更新 watcher

**位置与可达链。** `shell/src/config/reloader.rs:226` 比较 `[skills]` 并发出 `ConfigUpdate::Skills`。leader 的 `agent/app.rs:865` 仅记录日志；in-process 的 `pager/src/acp/spawn.rs::config_reload_request:211` 只匹配 models/MCP/subagent toggle，其 wildcard 丢弃 Skills。`SkillsFileWatcher` 分类仅 skills/commands/workflows（`config/watcher.rs:640`），不把 config.toml 转为 discovery change；custom roots在启动时从 `skills_paths` 构造（Pager `spawn.rs:176/:518`），没有 Skills event reconfigure 消费方。

**触发与后果。** 会话已启动，直接编辑 config.toml 的 skills.disabled/ignore，或新增仓库和 home 之外的 skills.paths，但不改已监听技能目录。Config watcher 可检测变化，现存 SkillManager baseline 和 slash advertisement仍旧；新路径的后续文件变化也没有对应 native watch。显式 reload 或某次无关 skills directory event可能让状态追上，不能证明该配置事件已生效。Pager 正常 toggle effect 在成功后另发 `grow/skills/refresh-baseline`（`pager/src/app/root/effects/mod.rs:2628`），因此本项不声称常规 UI toggle必然失败。

**不变量与覆盖。** 已有 `session_setup.rs::reload_skills_from_disk:145` 可从 fresh config + session registry 更新唯一 owner；缺的是把配置变化交付它，并同步 discovery roots。只测试 ConfigReloader“发出正确 enum”或 SkillManager“采用 baseline”不能覆盖中间丢弃。

**历史。** v1.0.0 leader 即为日志处理；当前 v2.6.0 / HEAD及活动修改仍保留。后续 watcher lifecycle修复解决 ownership/drop，不等于此事件语义已经交付。

**修复 owner 与通过标准。** Shell runtime config routing / discovery watcher owner；复用已有 internal reload skills 和 owner生命周期。对 leader/in-process分别只写 config（不碰技能目录），确认 disabled/ignore立即体现在模型可调用目录与 slash list；添加外部 custom root后再创建/修改技能可观察，移除该root释放watch；取消与失败配置不遗留owner、不发布错误快照。

**验证状态。** 两个生产配置消费者、watch分类与现存恢复入口已静态核实；未运行平台文件事件集成。

### AC-07 · P2 · ACP 直接插件 Update 丢弃更新结果，再保存旧安装记录

**位置与可达链。** `extensions/plugins.rs:120 → execute_plugins_action → session/actor/hooks_plugins.rs::PluginsAction::Update:414`。该分支以 immutable `registry` 读旧记录，`:452` 调 `git_install::update_repo`；拿到 `UpdateStatus::Updated(result)` 后只读 `changed` 生成 message（460），最后 `registry.save()`（481）仍保存旧数据。`agent/src/plugins/git_install.rs:702` 的 `update_repo` 实际执行 git pull 并返回 new_commit/plugins，不修改传入 InstalledRepo。

**触发与后果。** 未启用 require-sha 的可更新 branch 安装（或策略允许此路径），远端提交变化；ACP plugin action执行 Update。checkout 已更新，安装 registry 的 commit、updated_at和插件集合仍旧。再次对同一远端 tip更新，比较基准仍为旧 commit，可能持续显示“updated”，实际状态与 registry 消费方失配。Pinned/local no-op不触发本项。

**已有正确对照与历史。** CLI `shell/src/plugin.rs:407` 调 `apply_update_to_registry:228` 更新 commit、timestamp和plugins；marketplace路径另外有 transactional更新。actor从 v1.0.0 旧 hooks_plugins路径到46个tag一直没有这步，本项不是 git pull自身错误。复用低层 git helper并没有复用 application-level语义。

**修复 owner 与通过标准。** Shell plugin application owner。统一现有CLI/ACP更新结果采用逻辑，明确 marketplace provenance选择；在本地 bare remote + branch fixture中新增commit及plugin，ACP一次更新后 registry与checkout/pluginmap一致，第二次相同tip为already-up-to-date；失败时不把未采用结果发布成功。无需重做installer。

**验证状态。** 参数可变性、返回值和CLI对照已静态核实；未执行网络或Git pull集成。

### AC-08 · P2 · 插件安装/卸载吞掉关键持久化错误并返回 Success

**位置与可达链。** 同一 actor 的 Install 在 `hooks_plugins.rs:325` 将 `registry.save()` 失败仅 warn，随后返回 Success（339）；Uninstall 在 `:395` 删除 repo失败仅 warn，仍 remove record/save，并返回Success（403）。CLI `shell/src/plugin.rs::save_registry_or_warn:23` 被install/uninstall/update调用，也将关键save失败降为日志。低层 InstallRegistry::save和remove_repo_path已返回Result，错误在application层被丢失。

**触发与后果。** 安装文件已发布但registry文件不可写/rename失败；或卸载repo目录因权限/I/O失败。调用者收到成功，磁盘artifact和durable registry可能分离：已安装内容未登记、卸载后目录仍在却记录被移除，或文件已删而旧registry仍在。日志不足以让ACP调用者判断部分完成，也使重试缺乏明确状态。此项与AC-07的“无错误但保存旧值”不同。

**历史与正确对照。** v1.0.0的actor已存在同类warn-and-continue，46 tag保持；marketplace transactional update的save失败有回滚或明确不一致错误，可作为现有边界参考，但不能据此断言普通install/uninstall同样具备事务保证。

**修复 owner 与通过标准。** Plugin application / persistence owner。关键I/O失败向typed outcome传播；部分完成时保留可诊断的artifact/record状态，成功只在该操作承诺的durable边界后发布。分别注入registry save失败、repo removal失败、rollback失败，ACP/CLI均返回可识别错误且不声称完整卸载/安装；正常路径保持行为。与AC-01共享的是错误纪律，不要求合并成一个巨型事务框架。

**验证状态。** 真实调用链与错误丢弃已静态核实；未运行故障注入安装实验。

## 4. 已修复历史与当前问题的界限

| 首个包含的发布版本 / commit | 当时变化与证据 | 当前判断 |
|---|---|---|
| v1.1.0，目录/依赖归一化 | `grow-*` crate改为当前名；`xai-hooks-plugins-types → extension-types` 为独立DTO迁移，manifest只改包名，lib有6处品牌文本替换；serde-only依赖保持 | 不是凭当前路径缺失推断早期没有插件DTO。当前Shell/Pager → DTO，marketplace → agent/config/DTO，无DTO → host回边。 |
| v1.1.8，`0cd90573b` | ModelsManager把models/config/current置于同一CatalogState；完整候选校验后一次提交，再发model通知；reload与显式选择不再从过时分锁快照提交 | 已修复历史，不列作当前“模型目录撕裂”。当前revision/snapshot及活动model控制另由父agent复测。 |
| v2.0.0，`889930fbc` / Timeline重建系列 | Agent策略、Timeline事实与投影分离；plugin.json与marketplace index改为单一canonical schema，删除alternate manifest/name guessing/旧token等兼容路径 | 当前必需root plugin.json、deny_unknown_fields。旧 `.claude-plugin` / convention fallback不能当作当前发现路径。 |
| v2.0.1，`ea0a35942` | 强化capability floor/operator clamp、qualified selectors、per-cwd/session plugin registry及lifecycle边界 | 构造与调用契约已有真实owner；AC-03是残留消费者没有使用该快照。 |
| v2.0.3，`5aabf367b` | workflow runtime-owned gate | 保留runtime gate，不以作者工具声明自行授权workflow。 |
| v2.0.4，`2ffa3223a` | 删除managed/signed policy/cache/config loading层，收缩到个人配置与明确runtime覆盖 | 不将被删除的旧配置层、远端policy实体或其测试缺口算作现存架构。 |
| v2.1.5，`175b08368` | Settings硬读错误停止、私有tempfile权限先于写入；skill ignore覆盖plugin；disabled exact key；managed text rollback检查身份/字节，冲突保留目标与backup | 修复实际存在；AC-01只指仍绕过这些边界的plugin/CTA writers。Skill别名修复不能自动保护独立AGENTS/native Agent walker。 |
| v2.1.5，`cfe976164`、`aca7b172b`、`2c7e887c2` | 移除不生效skill model/effort metadata、无消费announcement状态、重复config inspect等旧路径 | 这些不是当前“未接线字段”问题；当前announcement独立保存实现已核对。 |
| v2.4.1，`404aaa799` | skill preload/loading先解析精确来源身份再校验eligibility，缺失/禁用/歧义显式失败，预加载条目仍在catalog | 已修复历史；不把strict preload失败写成“丢技能”。 |
| v2.4.1 `f497f6a10`、v2.4.2 `02432cb64` | config watcher交付与resident owner生命周期归一，取消/重建释放对应watch | 已有正确ownership边界；AC-06是Skills enum未路由，属于不同缺口。 |
| v2.5.5，`c67f4fe49` | native/plugin/CLI subagent catalog与toggle一致；canonical selector、exact filesystem entry重建和validated reload | 当前模型/effort工具、resume活动面另验；不沿旧设计推断当前子代理权限绕过。 |
| v2.6.0，`219220f4e` | mutable guidance移入role，system head保留stable mandatory/audience；command/subagent观察工具拆分；blocking worker拥有native watch注册/drop | 这些变化有当前消费，不能仅以模块或工具数增加认定过度设计。 |

## 5. 独立整理项与未升级为缺陷的假设

- `agent/src/config.rs:638` 的 `ToolExecConfig` / `ToolRetryConfig` 无当前字段/调用消费；`register_toolset_preset` / `register_internal_toolset_preset` 的注册调用在仓库中只有声明，当前preset解析仍携带全局mutable registry与初始化顺序契约。注释说可供out-of-tree调用，但没有本仓产品需求证据。建议Agent owner独立确认是否仍有受支持产品调用，再删除无消费实体或收缩入口；不能仅凭零搜索结果把公开API判为运行bug。
- `announcements` 为取得grow_home依赖整个 `tools` crate，`tools::util::grow_home`实际转发 `config`。这是可单独收缩的依赖边，不要求新建paths/config框架，也不影响本次持久化正确性结论。
- `plugin-marketplace` 依赖 `agent` 获取manifest/install/registry能力、依赖 `extension-types` 共用catalog组件DTO；`agent` 又依赖tools/hooks，crate依赖范围较广但目前有真实调用且无回环。若拆边，应由已知构建/复用需求驱动，不能因为crate名称像leaf就判定错误。
- Shell tools config中“显式standard”与未设置在remote fallback下可能合流；已核对的产品入口remote_settings为None，未证明实际生产触发，本报告不把它列为当前阻塞。RemoteSettings字段多或历史名称残留也不单独构成缺陷。
- ConfigLayers / hooks raw source / effective model config确有重复parse/read，但分别服务raw编辑、有效覆盖与provenance；本次未证明重复读导致错误快照。可行动的不一致已经具体落在AC-01/03/06，不提出跨仓缓存或通用配置重构。

## 6. 全部发布标签的树、manifest 与相邻变化

### 6.1 路径迁移和覆盖集合

| 代号 | v1.0.0 实际路径 | v1.1.0 起当前路径 / 演进 |
|---|---|---|
| A | `crates/codegen/grow-agent` | `crates/codegen/agent` |
| C | `crates/codegen/grow-config` | `crates/codegen/config`；v2.0.4删除旧managed/signed loading |
| T | `crates/codegen/grow-config-types` | `crates/codegen/config-types` |
| P | `crates/codegen/grow-plugin-marketplace` | `crates/codegen/plugin-marketplace` |
| E | `crates/codegen/xai-hooks-plugins-types` | `crates/codegen/extension-types`；单独DTO包迁移，不能用去grow前缀推导 |
| N | `crates/codegen/grow-announcements` | `crates/codegen/announcements` |
| S | `crates/codegen/grow-shell` 下选定配置/扩展文件 | `crates/codegen/shell` 下相同功能集合；历史`src/session/acp_session_impl/hooks_plugins.rs`在Timeline重建后为`src/session/actor/hooks_plugins.rs` |
| O | `crates/codegen/grow-tools` 下技能/registry文件 | `crates/codegen/tools` 下相同功能集合 |
| Q | old shell/tools/workspace/pager 消费方 | 同时匹配旧/新crate路径，再以职责前缀归一；未把迁移后的文件缺失当功能消失 |

A/C/T/P/E/N列为整个crate的Git tree短hash，包含manifest、源码和crate内测试；S/O/Q为选定文件`relative_path → Git blob`排序映射的SHA-256前9位。manifest单独为各crate `Cargo.toml` 原始字节SHA-256前8位。全表没有依靠文件大小或keyword次数推断缺陷。

S选择`src/config/`、`src/extensions/`、`src/util/config/`、`src/agent/config*`及旧`src/config.rs`。O选择`src/implementations/skills/`、`src/registry/`、`src/types/skill_discovery_tracker/`及旧同名`.rs`。Q选择Shell的`src/tools/{config,tool_context}.rs`、`src/agent/app.rs`、`src/agent/models.rs`和`models/`、`provider_catalog.rs`、`config_model_override_parse.rs`、`mvp_agent/{acp_agent,agent_ops}.rs`、`src/plugin.rs`、历史各路径的`hooks_plugins.rs`/`session_setup.rs`，另加Tools `implementations/lsp/config.rs`、Workspace `project_config.rs`、Pager `acp/spawn.rs`。Q的keys使用shell/tools/workspace/pager前缀；S/O去掉版本对应crate根。

相邻计数为相对路径映射中新增、删除、blob变化的key数，顺序固定A/C/T/P/E/N/S/O/Q；内部rename可能算删除+新增，crate根rename已归一。v1.0.0一行是初始库存数量，其后均相对上一行。已结合rename-aware Git diff/stat、path-scoped commit history与架构转折源码核对。复用仅指本分区这些源码集合和manifest相同，不表示跨crate依赖、全仓行为或产物相同，也不替代逐版本编译。

| 版本 | A / C / T / P / E / N tree | manifest集 | S / O / Q 内容指纹 | 相邻变化 A/C/T/P/E/N/S/O/Q | 证据复用 |
|---|---|---|---|---|---|
| v1.0.0 | `942de8ecc / 6b8347549 / 129b0cbac / 9fcaab606 / 65c85308c / 008be64cd` | M0 | `c2d1388a9 / 2381585a9 / 7d0c588fc` | 48/25/7/12/2/3/71/10/15 | 初始库存已查 |
| v1.1.0 | `6ec67a50d / 45916aee1 / 48f5b9ee2 / dab5d5ae0 / fb6df1f50 / 37f0c09f5` | M1 | `909cc34e8 / 631766a08 / c0314da23` | 34/13/7/8/2/2/48/6/14 | 相邻差异已查 |
| v1.1.1 | `6ec67a50d / 45916aee1 / 48f5b9ee2 / dab5d5ae0 / fb6df1f50 / 37f0c09f5` | M1 | `909cc34e8 / 631766a08 / c0314da23` | 0/0/0/0/0/0/0/0/0 | 复用v1.1.0 |
| v1.1.2 | `6ec67a50d / 45916aee1 / 48f5b9ee2 / dab5d5ae0 / fb6df1f50 / 37f0c09f5` | M1 | `3d165c4d1 / 631766a08 / e6157224b` | 0/0/0/0/0/0/4/0/3 | 相邻差异已查 |
| v1.1.3 | `6ec67a50d / 45916aee1 / 48f5b9ee2 / dab5d5ae0 / fb6df1f50 / 37f0c09f5` | M1 | `2777b36dc / 631766a08 / 4dd1eea5f` | 0/0/0/0/0/0/2/0/1 | 相邻差异已查 |
| v1.1.4 | `9f88bb9f4 / 45916aee1 / 48f5b9ee2 / dab5d5ae0 / fb6df1f50 / 37f0c09f5` | M2 | `e3210c4ef / 631766a08 / 9b77444af` | 2/0/0/0/0/0/3/0/1 | 相邻差异已查 |
| v1.1.5 | `a5c7106f9 / 45916aee1 / 48f5b9ee2 / dab5d5ae0 / fb6df1f50 / 37f0c09f5` | M3 | `187325cdc / c5394189e / f08b44824` | 4/0/0/0/0/0/4/1/3 | 相邻差异已查 |
| v1.1.6 | `342af20e3 / 45916aee1 / 48f5b9ee2 / dab5d5ae0 / fb6df1f50 / 37f0c09f5` | M3 | `4f04a110a / 537cde104 / e4f2244c9` | 5/0/0/0/0/0/4/1/1 | 相邻差异已查 |
| v1.1.7 | `e470c8b7f / 45916aee1 / 48f5b9ee2 / dab5d5ae0 / fb6df1f50 / 37f0c09f5` | M4 | `76ed1400e / 7d85843ca / 45f3cce5e` | 1/0/0/0/0/0/1/1/1 | 相邻差异已查 |
| v1.1.8 | `e470c8b7f / 45916aee1 / 9d2f9ecf0 / dab5d5ae0 / fb6df1f50 / 37f0c09f5` | M4 | `931e347c8 / 3a131ff56 / a506ed71e` | 0/0/1/0/0/0/7/1/5 | 相邻差异已查 |
| v1.1.9 | `e470c8b7f / 45916aee1 / 9d2f9ecf0 / dab5d5ae0 / fb6df1f50 / 37f0c09f5` | M4 | `931e347c8 / 3a131ff56 / a506ed71e` | 0/0/0/0/0/0/0/0/0 | 复用v1.1.8 |
| v1.1.10 | `e470c8b7f / 45916aee1 / b9a1acba2 / dab5d5ae0 / bcb3903e0 / 37f0c09f5` | M4 | `eb09e5ed0 / 55e68bff2 / a506ed71e` | 0/0/1/0/1/0/2/2/0 | 相邻差异已查 |
| v1.1.11 | `7bf75aae4 / 45916aee1 / b9a1acba2 / dab5d5ae0 / bcb3903e0 / 37f0c09f5` | M4 | `eb09e5ed0 / 014a500ac / a506ed71e` | 2/0/0/0/0/0/0/2/0 | 相邻差异已查 |
| v2.0.0 | `0100f81c3 / 455a77dfa / 686e47a5b / 1f3d6270a / c30b3478b / 6ea62d8db` | M5 | `cb2f41e0b / f593df292 / d7df4e4d1` | 34/7/3/8/1/1/47/7/17 | 相邻差异已查 |
| v2.0.1 | `f16d74291 / 455a77dfa / ddc17dd43 / 1f3d6270a / c30b3478b / 6ea62d8db` | M5 | `c6ed793d2 / 8d7579aa6 / 0f60a303d` | 7/0/1/0/0/0/4/2/6 | 相邻差异已查 |
| v2.0.2 | `f16d74291 / 455a77dfa / ddc17dd43 / 1f3d6270a / c30b3478b / 6ea62d8db` | M5 | `a0d15299f / 8d7579aa6 / 2eacb49de` | 0/0/0/0/0/0/2/0/3 | 相邻差异已查 |
| v2.0.3 | `2a71d31ce / 455a77dfa / ddc17dd43 / 1f3d6270a / c30b3478b / 6ea62d8db` | M5 | `1767e4bbf / 8d7579aa6 / 2eacb49de` | 2/0/0/0/0/0/2/0/0 | 相邻差异已查 |
| v2.0.4 | `f63e90ccf / 121e79625 / 901df8d8e / 1f3d6270a / c30b3478b / 6ea62d8db` | M6 | `14c7255ce / 8d7579aa6 / 16b16b1c7` | 1/15/2/0/0/0/23/0/6 | 相邻差异已查 |
| v2.1.0 | `c9b8d8915 / 121e79625 / bb84d88cf / 1f3d6270a / c30b3478b / 6ea62d8db` | M7 | `8c0422754 / 7662283fd / d6f2e27b1` | 3/0/1/0/0/0/37/2/9 | 相邻差异已查 |
| v2.1.1 | `c9b8d8915 / 121e79625 / bb84d88cf / 1f3d6270a / c30b3478b / 6ea62d8db` | M7 | `ab5591fe6 / 7662283fd / d6f2e27b1` | 0/0/0/0/0/0/1/0/0 | 相邻差异已查 |
| v2.1.2 | `c9b8d8915 / 121e79625 / bb84d88cf / 1f3d6270a / c30b3478b / 6ea62d8db` | M7 | `0616ed290 / 0ff9761de / 51e9394fa` | 0/0/0/0/0/0/6/2/3 | 相邻差异已查 |
| v2.1.3 | `02a6484c2 / 4d2e8c750 / bb84d88cf / 1f3d6270a / c30b3478b / 6ea62d8db` | M7 | `0616ed290 / e175f8cf6 / 51e9394fa` | 2/1/0/0/0/0/0/1/0 | 相邻差异已查 |
| v2.1.4 | `02a6484c2 / 4d2e8c750 / bb84d88cf / 1f3d6270a / c30b3478b / 6ea62d8db` | M7 | `92077a1a4 / e175f8cf6 / e11fca96f` | 0/0/0/0/0/0/1/0/1 | 相邻差异已查 |
| v2.1.5 | `a725d2c04 / 4a8bdfed4 / 49696306f / d0b5530d8 / c30b3478b / a7a662bbf` | M8 | `43b4a1f70 / 7ee6f5394 / ba9093558` | 3/7/1/1/0/2/11/6/4 | 相邻差异已查 |
| v2.1.6 | `7e9895870 / 4a8bdfed4 / 49696306f / d0b5530d8 / c30b3478b / a7a662bbf` | M8 | `ef4b7807c / 710e13f0e / b395876de` | 3/0/0/0/0/0/1/2/3 | 相邻差异已查 |
| v2.1.7 | `7e9895870 / ed2d1cc97 / b7eafebc6 / d0b5530d8 / c30b3478b / a7a662bbf` | M8 | `42b3494ad / 710e13f0e / 97dda3f04` | 0/2/2/0/0/0/5/0/1 | 相邻差异已查 |
| v2.1.8 | `7e9895870 / ed2d1cc97 / b7eafebc6 / d0b5530d8 / c30b3478b / a7a662bbf` | M8 | `c4f4eef67 / 710e13f0e / 97dda3f04` | 0/0/0/0/0/0/1/0/0 | 相邻差异已查 |
| v2.1.9 | `7e9895870 / ed2d1cc97 / b7eafebc6 / d0b5530d8 / c30b3478b / a7a662bbf` | M8 | `f518189e4 / 710e13f0e / 97dda3f04` | 0/0/0/0/0/0/1/0/0 | 相邻差异已查 |
| v2.1.10 | `7e9895870 / ed2d1cc97 / b7eafebc6 / d0b5530d8 / c30b3478b / a7a662bbf` | M8 | `f518189e4 / 710e13f0e / 10e5a1f7f` | 0/0/0/0/0/0/0/0/1 | 相邻差异已查 |
| v2.1.11 | `7e9895870 / ed2d1cc97 / b7eafebc6 / d0b5530d8 / c30b3478b / a7a662bbf` | M8 | `f518189e4 / 710e13f0e / 3b55b30fd` | 0/0/0/0/0/0/0/0/1 | 相邻差异已查 |
| v2.2.0 | `7dc6fd5fa / 042f8f219 / b7eafebc6 / d0b5530d8 / c30b3478b / f5304b514` | M9 | `475f9e46a / 033906a3d / e3389099d` | 9/1/0/0/0/1/7/3/3 | 相邻差异已查 |
| v2.2.1 | `7dc6fd5fa / 042f8f219 / b7eafebc6 / d0b5530d8 / c30b3478b / f5304b514` | M9 | `e76a956eb / 033906a3d / e3389099d` | 0/0/0/0/0/0/2/0/0 | 相邻差异已查 |
| v2.2.2 | `7dc6fd5fa / 042f8f219 / b7eafebc6 / d0b5530d8 / c30b3478b / f5304b514` | M9 | `385995735 / 033906a3d / e3389099d` | 0/0/0/0/0/0/2/0/0 | 相邻差异已查 |
| v2.3.0 | `7dc6fd5fa / 042f8f219 / b7eafebc6 / d0b5530d8 / c30b3478b / f5304b514` | M9 | `385995735 / 033906a3d / 8be0706ce` | 0/0/0/0/0/0/0/0/1 | 相邻差异已查 |
| v2.3.1 | `7dc6fd5fa / 042f8f219 / b7eafebc6 / d0b5530d8 / c30b3478b / f5304b514` | M9 | `385995735 / 033906a3d / 8be0706ce` | 0/0/0/0/0/0/0/0/0 | 复用v2.3.0 |
| v2.4.0 | `c7704d60d / 042f8f219 / b7eafebc6 / d0b5530d8 / c30b3478b / f5304b514` | M9 | `2d0bb3f6b / 93d13060e / 06b90b9fe` | 1/0/0/0/0/0/7/5/5 | 相邻差异已查 |
| v2.4.1 | `a42925ad4 / f292ac6a1 / b7eafebc6 / d0b5530d8 / c30b3478b / f5304b514` | M9 | `51cfc1c12 / acb97c299 / 818cb0b93` | 5/1/0/0/0/0/3/3/1 | 相邻差异已查 |
| v2.4.2 | `a42925ad4 / f292ac6a1 / b7eafebc6 / d0b5530d8 / c30b3478b / f5304b514` | M9 | `c7aa5b9e1 / acb97c299 / 0a53a0fef` | 0/0/0/0/0/0/1/0/3 | 相邻差异已查 |
| v2.4.3 | `a42925ad4 / f292ac6a1 / b7eafebc6 / d0b5530d8 / c30b3478b / f5304b514` | M9 | `a8e5cfbb9 / acb97c299 / 0a53a0fef` | 0/0/0/0/0/0/1/0/0 | 相邻差异已查 |
| v2.5.0 | `da12ec668 / 1eaec254d / b7eafebc6 / d0b5530d8 / c30b3478b / f5304b514` | M10 | `ef9b91c0a / 842444931 / d2e16e817` | 1/2/0/0/0/0/8/4/3 | 相邻差异已查 |
| v2.5.1 | `da12ec668 / 1eaec254d / b7eafebc6 / d0b5530d8 / c30b3478b / f5304b514` | M10 | `02485bbdc / 842444931 / d2e16e817` | 0/0/0/0/0/0/1/0/0 | 相邻差异已查 |
| v2.5.2 | `da12ec668 / 1eaec254d / 4f8c1b575 / d0b5530d8 / c30b3478b / f5304b514` | M10 | `ab438a74a / 842444931 / c2a836d8b` | 0/0/1/0/0/0/1/0/2 | 相邻差异已查 |
| v2.5.3 | `da12ec668 / 1eaec254d / 4f8c1b575 / d0b5530d8 / c30b3478b / f5304b514` | M10 | `ab438a74a / 842444931 / c2a836d8b` | 0/0/0/0/0/0/0/0/0 | 复用v2.5.2 |
| v2.5.4 | `da12ec668 / 1eaec254d / 4f8c1b575 / d0b5530d8 / c30b3478b / f5304b514` | M10 | `ab438a74a / 842444931 / c2a836d8b` | 0/0/0/0/0/0/0/0/0 | 复用v2.5.2 |
| v2.5.5 | `6a5105ddb / 1eaec254d / 4f8c1b575 / d0b5530d8 / c30b3478b / f5304b514` | M10 | `60c5e7584 / 842444931 / ea1200a55` | 4/0/0/0/0/0/4/0/3 | 相邻差异已查 |
| v2.6.0 | `aa864efa8 / 1eaec254d / 4f8c1b575 / d0b5530d8 / c30b3478b / f5304b514` | M10 | `f2e3ad0ff / 090fc62a9 / af3def667` | 7/0/0/0/0/0/7/2/7 | 相邻差异已查 |
| HEAD | `aa864efa8 / 1eaec254d / 4f8c1b575 / d0b5530d8 / c30b3478b / f5304b514` | M10 | `f2e3ad0ff / 090fc62a9 / af3def667` | 0/0/0/0/0/0/0/0/0 | 复用v2.6.0 |

最终Q集合比初始核心inventory更宽，因而v2.1.9/10/11、v2.2.2/v2.3.0在核心树相同时仍有consumer变化，不能标为整个分区复用。表中仅对最终完整选定集合相同的版本标注复用。

### 6.2 Manifest集合

顺序为A/C/T/P/E/N/S/O；相同集合显式复用，不把每次workspace version发布当作新的架构实体。以下为crate manifest原始字节指纹，workspace依赖解析仍须结合相应tag根manifest；相同crate manifest不承诺依赖锁定相同。

| 集合 | 首次标签 | A / C / T / P / E / N / S / O manifest SHA-256前8位 |
|---|---|---|
| M0 | v1.0.0 | `082c5a0f / 30ca0cac / 630ac554 / 38698ac8 / 7d447cff / 22c9072f / 4f7ff844 / de389db3` |
| M1 | v1.1.0 | `796d7230 / 7e9438c1 / dc795f0c / f54d68b8 / 2f29630b / f14e99b8 / 2fa33676 / c385ded3` |
| M2 | v1.1.4 | `bdebcdee / 7e9438c1 / dc795f0c / f54d68b8 / 2f29630b / f14e99b8 / a71cfd3f / 3d7dc329` |
| M3 | v1.1.5 | `bdebcdee / 7e9438c1 / dc795f0c / f54d68b8 / 2f29630b / f14e99b8 / 03dd3a7c / a8c5f237` |
| M4 | v1.1.7 | `bdebcdee / 7e9438c1 / dc795f0c / f54d68b8 / 2f29630b / f14e99b8 / fbdef936 / a8c5f237` |
| M5 | v2.0.0 | `bdebcdee / 8d251430 / dc795f0c / f54d68b8 / 2f29630b / f14e99b8 / 2579d59c / 1219fb2b` |
| M6 | v2.0.4 | `bdebcdee / bed1017c / dc795f0c / f54d68b8 / 2f29630b / f14e99b8 / 2579d59c / 1219fb2b` |
| M7 | v2.1.0 | `b8135b50 / bed1017c / dc795f0c / f54d68b8 / 2f29630b / f14e99b8 / 7bddad60 / 9be2db48` |
| M8 | v2.1.5 | `f1a9a98e / bed1017c / dc795f0c / f54d68b8 / 2f29630b / fa80abbf / 7acc7a8e / 9be2db48` |
| M9 | v2.2.0 | `f1a9a98e / bed1017c / dc795f0c / f54d68b8 / 2f29630b / fa80abbf / 7acc7a8e / 2c74348a` |
| M10 | v2.5.0 | `f1a9a98e / bed1017c / dc795f0c / f54d68b8 / 2f29630b / fa80abbf / df7ed18c / 2c74348a` |

v1.0.0的E包运行依赖只有serde，dev依赖serde_json；v1.1.0迁移后同样如此，后续未把runtime插件对象塞回DTO层。当前P通过extension-types共用marketplace component/listing数据，Shell负责runtime→wire转换；这条依赖是实际存在的，和旧crate名字变化是两件事。

### 6.3 可复核方式

```sh
git tag --list 'v*' --sort=v:refname
git rev-parse 'v1.0.0:crates/codegen/grow-agent'
git rev-parse 'v2.6.0:crates/codegen/agent'
git diff --find-renames --stat v1.0.0 v1.1.0 -- crates/codegen
git log --follow --format='%h %s' -- crates/codegen/shell/src/session/actor/hooks_plugins.rs
git show 175b08368 -- crates/codegen/shell/src/util/config/persist.rs
```

选定内容指纹的编码为`sha256(json.dumps(relative_path_to_blob, sort_keys=True, separators=(',', ':')).encode()).hexdigest()[:9]`。相邻比较以对应版本实际旧/新crate根归一后的key做并集比较；历史路径来自各tag tree，不以当前路径硬查。完整中间inventory与adjacent diff曾生成在`/tmp/grow-agent-config-audit-history.json`、`/tmp/grow-agent-config-audit-adjacent.md`，它们是本次session的临时证据，永久结论和全部tag表已写入本报告，不依赖临时文件长期存在。

## 7. 验证记录和边界

| 已执行 | 结果与适用范围 |
|---|---|
| Git tag / tree / manifest / normalized blob inventory | 46 release tag全部覆盖，加HEAD对照；保留旧crate路径，检查相邻差异与重要转折源码，零变化范围显式复用。未对每个tag编译。 |
| 当前entry→consumer→test静态审视 | 8项发现均有实际入口和状态/错误消费方；相关既有tests已读取，不将没有测试本身当失败。 |
| 46-tag已审阅机制对照 | 八组相关控制结构/语句在全部tag可定位；这是历史机制覆盖，不声称早期版本已经采用当前全部OpenSpec契约。首次引入证据不足时只说最早已覆盖v1.0.0。 |
| std-only Rust隔离实验 | `rustc --edition=2024 /tmp/grow-agent-config-audit-probes.rs -o /tmp/grow-agent-config-audit-probes` 后执行通过；输出为`agent_scan: lexical_entries=32, physical_files=1`及`rule_chain: includes_repo_rule=false, includes_external_rule=true, reaches_filesystem_root=true`。 |
| `git diff --check`（分区现有业务文件） | 审阅时通过；只是基线格式检查，不代表活动model/resume实现验证完成。 |
| 报告矩阵和链接验证 | 46 release行+HEAD与inventory逐单元格匹配，8项发现编号完整，3个相对规范链接可解析；另从Git独立重验282个crate tree ID及376个raw manifest digest，全部一致。 |
| 新报告空白检查 | `git diff --no-index --check /dev/null <report>`无诊断；no-index因文件有内容返回1，不能据此误报为空白错误。 |

隔离实验仅从现有函数复制两个路径算法；fixture在系统临时目录建立`checkout/src/.grow/agents/reviewer.md`、`loop → .`、`outside/alias → checkout/src`、repo/outside各一个AGENTS.md。扫描结果按canonical path统计物理文件；规则链使用已规范化root复刻lexical parent循环。实验没有调用完整Git discovery、Agent parser、SkillManager、ACP或Hook执行，不把该结果包装成端到端测试。

未运行Cargo build/test、历史版本编译、平台原生watcher集成、真实Git插件更新、registry故障注入、LSP进程或Hook进程；这些重验证由父agent资源调度，后续修复应按各发现的通过标准补齐。没有访问网络市场、改用户配置或安装/卸载插件。当前模型工具、effort、resume和repo/trust工作树的最终回归由其执行者/父agent验收，本报告只提供交叉边界事实。全局`openspec validate --all --strict --no-interactive`及archive校验也由父agent在合成后执行。

后续最有区分力的验证是：AC-01在真实writer之间设置并发屏障和I/O错误；AC-02直接驱动actor从Some到None再触发Hook；AC-03用两会话不同pluginDirs验证真正启动的LSP command；AC-06只修改配置而不碰任何skills目录。成功路径反复跑通不能替代这些触发场景。
