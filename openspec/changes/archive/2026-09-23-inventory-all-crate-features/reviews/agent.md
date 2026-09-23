# agent 逐包核查（进行中）

完整读取manifest及src/lib.rs、timing.rs、error.rs。27份Rust源码，另有prompts模板需要按引用核对；目前不标reviewed。

- manifest default=[]、default-bazel=[]，没有此处启用的业务feature；依赖tools/hooks/config及MiniJinja custom_syntax、vendored-libgit2等。不能以依赖声明推断外部服务或完整插件功能。
- lib导出Agent/AgentBuilder/AgentDefinition、toolset preset、PromptContext及构建错误，resource_roots为私有模块；“provider/permission/compaction属于host”当前为模块边界注释，须后续核对实现及调用方。
- TimingGuard new记录std Instant，Drop向grow_instrumentation发info timing/name/elapsed_us；as_micros u128直接as u64，不是饱和转换。没有主动stop或返回统计API，Drop可在正常或展开路径记录，进程强制退出不保证执行。
- AgentBuildError有Parse/MissingField/UnknownToolOverride/Io/RuntimeBuild/MiniJinja/Tool/InvalidConfig；Io及MiniJinja自动From，RuntimeBuild只包装io不自动From；Display包含具体原错误，分类枚举不代表每条构建路径必检查所有约束。三文件无内联测试。

- `crates/codegen/agent/Cargo.toml` SHA256 `b8135b50be51758a192a2e141161a545ca20f68886a73a4537223330e5ae2e98`
- `crates/codegen/agent/src/lib.rs` SHA256 `d377e85657e348fe42c7ee07009e902405cc57af248a877cba0326e18c6b9363`
- `crates/codegen/agent/src/timing.rs` SHA256 `9db634b7ec38c1141eb574124faecd7d0be42bcb442ad33a2fc2df7ddee62068`
- `crates/codegen/agent/src/error.rs` SHA256 `9a3245df60f44056d444e1284c3dddf508485f9d02df9c1a2591f93137458522`


## agent.rs、resource_roots.rs、repo.rs 完整读取；builder.rs 1–550

- Agent::new公开且不校验definition/context/rendered prompt一致；只读访问definition/name/description/completion、system/role、audience和bridge；tool definitions及AGENTS格式化委托下层。Arc<ToolBridge>内部仍可变，不等价整个session immutable。
- activate_resource_domain与prepare均要求Arc::get_mut成功，已共享返回staged bridge错误；桥方法执行其余激活规则。Agent无自行运行provider/compaction入口，role存为独立Option字符串，上层Timeline投影仍需host核对。agent.rs无测试。
- resource_roots只取home/.grow，None空，user_subdirs直接join任意subdir无containment校验。单测grow-only与None完整阅读未执行。
- RepoDirChain一次git2 discover取workdir，bare repo或失败无root；若root canonical等于home则丢弃，cwd-only。其余按原始路径逐parent上行，每层canonical仅用于与root终止比较，失败退原值，保留原拼写；未匹配到root可一直走到无parent，不能声称任意坏路径必定root内。公开字段可任意构造，不保证来自resolve。
- existing_subdirs_along按chain顺序再subdir顺序探is_dir，跟随可解析symlink，无dedup。repo四测试普通repo/非repo条件断言/home过滤/nonhome保留，两个home测试用serial(home_env)+RAII改HOME；本任务未执行它们，不主动更改HOME。未包含symlink回归，注释性能倍数不作为本轮测量。
- builder前230：默认Primary、Extend、LocalFs、agents_md=true、memory=false、interactive、write=true、subagents/workflow=false、ask=true、noop persistence，其他后端/config默认。ensure_plan_tools按精确ID补control/ask，不去重已有重复；merge_tool_params对每个精确ID逐键覆盖。workflow gate先按canonical ID或Workflow kind移除，再仅Primary且enabled安装一个；coordination gate按精确两个ID移除并Primary装canonical，未按其他typed alias剔除。
- builder231–550所有with入口只赋字段，无即时验证：定义/名字/描述/工具/禁止工具/skills/role/模式、memory、交互、label/env/persistence、MCP cap、backend/fs、owner/scheduler、web/lsp/deploy、write/subagent/workflow/ask开关、models/toggles/skills config/params/plugin/context/prompt cwd。注释中的实际影响待后build证据。
- resolve_definition有explicit definition立即clone，忽略builder中name/description/tools/role等定义覆盖；否则从default_grow_build填可选值、非空skills、disallowed等。build开始非UTF8cwd转字符串点号；preloaded_skills优先于discover_skills，即使该flag false也采用快照；否则discover或空。definition.skills非空则resolve预载、取path集合并开始注入role，余下从551行继续。

累计6/27份Rust完整读取，builder部分读取至550，尚未测试或正式登记。

- `crates/codegen/agent/src/agent.rs` SHA256 `89bc610f8f4b91618f2b92b842d8806afd18c598a9c6cddacf333f7e62f8e72b`

- `crates/codegen/agent/src/resource_roots.rs` SHA256 `ea95dc17b1c5ca19da211b43bbb5e3416e2cb7957db2dae3e9b1c750f250e222`

- `crates/codegen/agent/src/repo.rs` SHA256 `8521b2fa65def4d5c1567a950a1ded9eeb38e068e81bfd1b1789f03e57dada7f`


## builder.rs 完整读取（续551–1937，含测试）

- 指定预载skills解析后其注入文本前置于definition.prompt_body，路径集合用于从listing排除；未解决/空注入不补正文。inject_default_tools=false且原tool_config空立即InvalidConfig，在coordination等注入之前，因此curated空不能靠后续控制面救活。
- inject_default_tools=true时按memory backend/web enabled/lsp存在追加工具；write_enabled仅控制缺write时追加，并不剔除已有write；识别已有write按后缀write或Write跨namespace。plan control/ask也只在默认注入分支补。无memory_backend无论curated都剔精确Grow memory IDs；ask关闭剔精确ID；workflow/coordination canonical注入仍需通过后续allowlist。
- subagents关闭或filter后无可用类型剔精确Grow:task；开启不主动追加缺失Task，只修改已有descriptor。preloaded names仅按definition filter，不再应用toggle或live discovery；Subagent改固定短描述。Primary且preloaded names非空且保留task时live_subagents=None导致expect panic，此公开builder组合未提前验证。
- Task早期移除时检查Grow/GrowConcise bash background参数，没satisfier剔短名lifecycle；后续无Task最终归一化无条件剔task/get_task_output/kill_task，并把所有短名run_terminal_cmd的enabled_background与auto_background_on_timeout=false。因此早期保留不保证最终存在，最终判断按short name而非kind。
- 参数按web、bash、ask顺序merge，精确ID逐键覆盖；随后disallowed匹配移除且未命中warn，own非空allowlist保匹配和SearchTool/UseTool kind。mcp__名称跳过unknown诊断，但此层未据此限制具体MCP调用。已注册但不可用debug，unknown warn，均不回退全部工具。最后session_tools_allowed再限制，MCP meta是否被session允许由config实现待核对。
- finalize SessionContext使用真实cwd/fs/terminal、state_path.parent或temp session folder、env默认空map、owner/parent scheduler、skills与各backend/persistence；subagent=None固定，此字段不是仅凭audience自动填充。失败转ToolError。MCP cap Some后插TruncationCfg其他字段Default，不由setter验证零。
- restored announced names在seed之前；AGENTS读取受definition.agents_md控制，但git discover/gitignore/seed_agents_md无论开关运行，所用gitroot无RepoDirChain home过滤。listing排除预载paths，seed discovery cwd仅discover_skills=true才Some；即使静态preloaded有内容，false也seed None。
- prompt cwd只对展示AGENTS file_path做String::replace所有子串，无组件校验，真实工具cwd不变。PromptContext.render失败unwrap_or_default为空system prompt，role独立render；description render Some才替换，None保原字符串。部分渲染错误未作为AgentBuildError传播。
- Task描述由tool-types共享builder生成；builtin GP/Explore插tools placeholder，其他builtin空fragment，用户描述原文。model slugs排序去重但不trim，空列表建议继承；这是模型提示不是服务端model选择校验。child正文固定GP/Explore而非动态目录，注释提plan不符合当前常量正文。
- 全部测试静态阅读：workflow/coordination gates、Task描述模板/用户shadow/模型排序/参数重命名/child长度/resume文案；四种pager开关组合工具唯一与plan/ask/task；curated empty拒绝；ask参数后注入；session own交集与deny、unknown不给权、canonical工具名、subagent allow描述、Task禁用关闭bash后台但保其他参数、插件frontmatter、MCP meta保留及web启停。测试声称task不存在有处仅查旧名task，不强证新名spawn_subagent也缺失；并非所有入口组合覆盖。未执行测试。

累计7/27份Rust完整读取；build runtime字段下层行为继续在config/prompt/tools各包核对。

- `crates/codegen/agent/src/builder.rs` SHA256 `de1ac918d27da96fffd5f259776e4a5b9a83328dbddfdc3a9b8559d3352fb9ba`


## config.rs 连续读取1–1360（未完成）

- preset进程OnceLock Mutex注册public/internal，insert同名覆盖但原始name不normalize；lookup才trim/lower/空格下划线转连字符，故注册未规范名可枚举但不正常resolve。native优先外部同名，public enumeration native先外部HashMap顺序不稳定；internal只可按名解析。lookup持锁调用builder fn，重入注册/查询有死锁风险，poison expect panic。晚注册对后续可见，不回写已解析config。
- native四preset grow-build/concise/explore/computer，全部列表本地构造后选一个；preset_names也构造全部public配置，并非纯key查询。workspace全集在default上补write/plan/ask/web/memory/lsp，不代表所有runtime gate启用。
- default工具包含bash/read/edit/list/grep、task生命周期、todo、scheduler、monitor、context recall、MCP meta、goal、workflow/coordination；concise无Task和MCP meta，使用concise文件/执行；hashline直接插调用方给的vector不检查恰3或slots，含Task/MCP而未直接含coordination；computer仅8个文件/执行及命令控制；explore仍含bash和MCP执行入口，readonly是下层permission约束，不能写成工具表无执行。
- bash/task工具名及参数重命名，task output/kill重命名；short_tool_name取最后冒号之后，tool_id_eq精确全名或short，tool_config_eq另认name_override，不trim/casefold/wildcard。
- SubagentPolicy allow/deny接受字符串逗号trim去空、数组原样不trim（与注释不同），null空。filter空allow→None任意peer，deny精确字符串优先，去重BTreeSet；与ordinary tools独立。SubagentFilter直接serde deny_unknown_fields，字段无serde default，不能等同Policy宽松默认。
- AgentDefinition camelCase deny_unknown_fields，description必填但本段无非空校验，name缺省空；tool_config serde跳过并默认grow-build，只有parse/helper显式resolve toolPreset+additional。运行时authored工具、clamp、source/scope/plugin/prompt body均serde skip。普通直接serde并不执行parse的preset resolution。maxTurns只serde拒绝0，public字段可程序构造0；capability、effort/isolation/background/memory等存在不证明builder执行session policy。
- selector plugin:name否则name，无escape；runtime_equivalent比较serialized manifest/toolconfig/authored config并显式比clamp/body/source/scope/plugin，序列顺序及路径原拼写影响相等，序列化失败视不等。CompletionRequirement/Recovery/ToolExec/ToolRetry未deny_unknown_fields且不校验delay大小关系。
- colors8种，AgentDefinition自定义color解析trim+caseinsens，非法类型/名字warn后None；AgentColor自身serde只lowercase枚举。Effort low/medium/high/xhigh/max，isolation none/worktree，memory user/project/local。MemoryScope.resolve_dir直接join agent_name不限制路径，User用config::grow_home并标非project，project/local标已project scoped。
- MCP inheritance默认All，字符串all/none caseinsens但不trim；named/except单key精确小写map，vector名字不trim，permits精确比较。Hooks只校验JSON object，语义延至spawn。McpServerRef string任意Named；单keymap优先且值须object，所以单字段{name:字符串}失败，多字段含name字符串则整个对象作config；serialize Inline统一name→config。
- BashConfig默认120s/200000bytes/prefixNone，当前AgentDefinition不含此字段，不根据孤立类型注释推导可用frontmatter bash覆盖。
- parse trim_start后仅starts_with---、找首newline---，不要求独占delimiter行；closing后找首newline再trim body，空None。from_file再以UTF8file stem覆盖name、记录source/scope；frontmatter_only也读取全文件，只是不设body。scope优先user dirs路径starts_with，再字符串包含.grow/agents路径，其他BuiltIn，无canonical统一。
- primary floor先subagent_only单项返回，否则对authored tool_config加可选write（缺Edit/Write）、应用own deny/allow（保MCP meta），按kind检查read/search/list/lsp、edit/write/delete/move、Execute；不应用session clamp、运行时backend gate或authored_capability_tools字段。builder本身未调用primary eligibility，host选择路径待核对。
- session clamp deny优先，allow None全许、Some空全禁；meta工具无此层例外。strict harness仅!inject_default_tools；override_file_tools不检查strict且按固定三对ID槽替换已有项，没原slot不添加，不支持concise slot，不同步authored snapshot。
- builtin_defaults明确所有字段，内建五个由include_str parse并expect，模板尚未读；下次从1361开始读剩余函数及测试，config不算完整文件。


## config.rs 完成、五个嵌入定义、example及prompt入口

- from_json先移除promptBody字段再serde，要求name.trim非空但保留原name空白；promptBody只有string且trim非空才采用，非string静默忽略。然后resolve preset并设BuiltIn。to_json_value只补prompt_body，跳过的resolved tools/authored/clamp/source/plugin等不恢复，不承诺runtime_equivalent。
- config剩余测试全部静态读取，覆盖native preset别名及computer8工具、write与命令控制区别、strict builtin分类、Markdown/JSON解析、root example字段/hooks、primary floor、selector、subagent allow/deny、max0/枚举/非法字段、MCP refs/inheritance、memory路径、completion、file stem、JSON prompt、builtin枚举、runtime等价。名为round_trips的completion测试仅解析一次断言；from_file_sets_scope_and_path仅断言name/path没有scope。缺少外部preset注册及重入、非法color、数组trim差异等专门测试。未动态运行。
- prompts/agents五份全部读：grow标准extend；concise extend且agentsMd=false；general-purpose subagentOnly，grow-build；explore subagentOnly/explore preset/no default注入/read-only/inheritSkills=false但discoverSkills仍默认true；browser-use subagentOnly/full/grow-build/agentsMd=false，角色要求浏览器范围，但工具preset不是浏览器专属集合。空subagent allow/deny并不禁止进一步委派，实际还受runtime gate。模板正文是产品prompt数据，不是本任务的执行指令。
- root agent.md.example全文读，列toolPreset/additional工具重名覆盖记录、subagent权限、MCP、hooks、memory、completion等示例。注释声称所有后续只能收窄与builder可注入工具不完全一致；顶部提toolConfig.tools不是当前AgentDefinition接受字段；示例不是新的运行时保证，暂不扩展修改。
- prompt/mod.rs仅模块导出。subagent_prompts.rs仅reexport tool-types两个prompt，工具占位符行为属于下层renderer，注释不证明具体缺字段策略。
- prompt/ignore.rs只添加root .gitignore及global路径，global先git2 core.excludesFile否则HOME/.gitignore；无repo直接None，不加载每级嵌套.gitignore或.git/info/exclude。add错误忽略，build错误None；is_ignored无matcher直接false，否则委托tools。该模块无测试；global后追加的规则优先级细节须按ignore库而非注释推断。

累计11/27份Rust源码完整读取；另5份内嵌定义和root example已审阅。后续prompt context/template与发现/插件模块尚未读完，crate pending。

- `crates/codegen/agent/src/config.rs` SHA256 `1f60917ebf4fe3870c56164edd999da1f5b73b847bd8637c2c71ba05a3c9840e`

- `crates/codegen/agent/src/prompt/mod.rs` SHA256 `d8eb30c8bffc795031f69b1e21ef41352a1c5b82dcb3bb96af61960c77d9e6bd`

- `crates/codegen/agent/src/prompt/ignore.rs` SHA256 `78bab9d94fe81c6165e29f71da36c633c18a39f3ae47aab2a8ad9f9663440a6e`

- `crates/codegen/agent/src/prompt/subagent_prompts.rs` SHA256 `2392cb0e9c8bcc7e95832c0bfafedb74000b9f20d3a509163871919024adedf9`

- `crates/codegen/agent/prompts/agents/browser-use.md` SHA256 `4a64e6012b466ec95dca832b7ab08940372ed50088b75589368fcc015d911825`

- `crates/codegen/agent/prompts/agents/explore.md` SHA256 `b2592c27dbd0c89ae4a8c0538515ba649638674277dc3e4e7c9e02f96b15d3f9`

- `crates/codegen/agent/prompts/agents/general-purpose.md` SHA256 `30925384c76b66138af6dbc30237eca9ed11ac0ae02c53b7b4d43946d475be87`

- `crates/codegen/agent/prompts/agents/grow-build-concise.md` SHA256 `309fea5b7eef38e5d3f328ca2e65c5a684eace5fa9ffcf04599997f8cef33761`

- `crates/codegen/agent/prompts/agents/grow.md` SHA256 `3a27f55f1e5980b1299296e2760a3fc070ba74451f6a73c9c9d83610ba1baed2`

- `agent.md.example` SHA256 `f04f3241c029b7809ceb7d45d3ea061ca87830adef076e50bdb4c54ed605b0b4`


## prompt/user_message.rs、workspace_user.rs、context.rs 完整读取

- RuntimeContextSnapshot纯render用户角色runtime_context，OS/shell/workspace/date均调用方提供，无本层采集/自动日期rollover。日期%Y-%m-%d，Path.display为lossy可显示形式，字段/status原文插入不转义XML、不滤换行标签；角色归属持久化由host完成，结构本身无serde。Git/JJ标签和提示分别固定；空trim status不显示VCS块。
- VCS_STATUS_CHARACTER_LIMIT实际按UTF8字节10000，截至字符边界再优先最后非首位换行截整行，追加双换行与truncated标记所以总输出可超过10000。三单测canonical/Git-vs-JJ/UTF8后缀已读未运行，后者没断言硬长度。
- workspace user读取GROW_ROOT/GROW_USER Unicode env，缺失无；bare user映到users/name，含任一种斜杠原样。公开resolve仅root/user非空、join且is_dir，非纯无I/O；不限制绝对路径、..、symlink containment或trim。optional入口空GROW_USER先映users/，若存在会返回该目录，不等同公开resolve空user拒绝。Unix含反斜杠按文件名实际语义；12测试只普通/空/不存在/文件及映射布局，不覆盖escape/env空值。
- PromptContext Default Extend/Primary、无body/AGENTS、memoryfalse、interactive、labelGrow；无serde。placeholders只有memory_enabled/is_non_interactive/system_prompt_label三键，无workspace或AGENTS内容。agents_md_user_reminder对两audience直接完整format，不截断；Agent旧方法注释compacted不符本实现。
- render构造空tool renderer，stable只mandatory core+audience；render_with_renderer任一模板error→None，过滤trim空但保留原section空白后双换行拼接。role从bridge snapshot None→None；role_with_renderer Extend含standard、可选body、session extensions，Full只省standard；逐模板error退原模板文本，最后空结果None，不抛构建错误。
- memory/工具指导和role不进入stable头，AGENTS独立提醒，不据context数据宣称已写Timeline；实际host持久化待其他crate核对。systemlabel仍能改变stable core，具体模板待阅读。
- context805行全部含测试已读：placeholders/default、full AGENTS含5000字child、stable-role分离/Full保持head、模板条件(memory/edit/execute/background/hashline)、size上限、显示path及共享GP/Explore文案关键词。很多测试helper将core+audience+standard+extensions自行拼接，不代表生产stable head包含这些层；path测试手工传display路径，不验证builder String::replace本身。shared tool-types prompt与agent/prompts/agents精简定义不是同一资源，不能把共享常量断言自动推广到所有builtin加载路径。未执行测试。

累计14/27份Rust源码完整读取；template实际文件与其5份文本待核对，agent仍pending。

- `crates/codegen/agent/src/prompt/user_message.rs` SHA256 `a7ca496f5122a140d063d9709530270f3dd718cb03768f7bcd909ae882ebabf6`

- `crates/codegen/agent/src/prompt/workspace_user.rs` SHA256 `ca4409729c9bc6d3a984ac64d39745c1ce16859b55afc27c484b86037478f807`

- `crates/codegen/agent/src/prompt/context.rs` SHA256 `2848b73c8ce06ed68606eb045d5ac49c687a81912669f5be80974c387e3ff174`


### prompt/template.rs 与五份嵌入模板完整审阅

- template.rs 全609行读完；5个 include_str 将 Markdown 编译进二进制，运行时不依赖磁盘提示词。DEFAULT_SYSTEM_PROMPT/SUBAGENT_SYSTEM_PROMPT 仅测试别名，分别只指 core 与 subagent audience，不是完整生产拼接。
- core 固定包含 instruction_priority、action_safety、tool_calling、project_instructions_spec、output；仅 grow_client 由 not is_non_interactive 控制。当前文本未引用 system_prompt_label，不能仅由context传入字段推断改label会改变head。授权层级、安全和嵌套AGENTS规则是发给模型的指导文字，不是该文件实现的执行拦截器。
- standard 的 read/edit/execute 建议各按对应kind存在输出；hashline建议要求read名精确hashline_read且edit/search存在。执行与output同时存在输出后台命令段，monitor独立输出watch段，两条件同时满足会有两个同名background_tasks块；不要求kill工具。
- primary audience 指导主代理保持整合与中心证据理解，subagent audience 指导限定范围与证据返回。child模板还固定包含capability_authority与字面search_tool/use_tool名称，不由工具存在性条件控制；不能由该文案证明运行时总有这些工具。
- session extensions 仅memory_enabled且memory_search/memory_get都存在时输出memory段。Full模式仍有extensions（已由context核对），只跳过standard。
- 所有template单测静态阅读完毕：渲染替换/改名、可选工具条件、monitor、稳定head、角色关键词、memory分层、交互条件、无旧discipline块、16KiB严格小于的字节预算。mid_session_switch测试只构造renderer渲染，并未执行真实会话切换；Full determinism只重复渲染body。
- guard扫描只覆盖core与subagent audience，未覆盖包含多数工具变量的standard/extensions；guarantees是字符串包含且不含“ or ”，并非逻辑解析，word_bounded只检查后边界。search_tool/use_tool免guard是测试假设。组合扫14种kind的空/全集/单个/两两集合（108组合×2 memory值），也只渲染core；不是全部2^14组合或完整role层验证。本轮未执行测试。

累计15/27份Rust完整读取，另5份模板完整读取；agent仍pending。

- `crates/codegen/agent/src/prompt/template.rs` SHA256 `e536a2e84459a14674ee440ea5865efe10227d3520fcf8e7c917ce6d490519f9`

- `crates/codegen/agent/prompts/foundation/mandatory-core.md` SHA256 `5accd88f15cb844b7461a3f94090277d80b7aa065fcd43386cb18842b11add97`

- `crates/codegen/agent/prompts/foundation/standard.md` SHA256 `9036fb418a6f1bd571677e9cc4045f9a744be31d3bb010abb1150ca0360c514e`

- `crates/codegen/agent/prompts/audience/primary.md` SHA256 `f6b9ee094fb96c3111e938f74a760c1557d7fbdc62f68efb86c7fc981809c344`

- `crates/codegen/agent/prompts/audience/subagent.md` SHA256 `ec94e3072470933d6263ecd39b316f14729d7f2ed4bf97a0077f4b732c652924`

- `crates/codegen/agent/prompts/extensions/session.md` SHA256 `b1be2daf7dc7fda72a1e889046d886d7e3465f04a4b016544262bbf0e1ccf40a`


### plugins/mod.rs、trust.rs、local_refresh.rs 完整审阅

- mod31行导出9模块及发现、manifest、registry、trust公共入口；模块说明不是执行门禁的证据。
- TrustStore load取config::user_grow_home；无用户根时空集合和空file_path，不回退项目目录。load_from直接接收任意路径。打开不存在返回空；其他打开错误warn返回空。逐行读取错误静默丢弃，trim后跳空/井号注释，剩余原样PathBuf入HashSet，不在加载时canonicalize或验证绝对路径。
- is_trusted canonicalize候选，失败warn false；grant先canonicalize，已有内存条目直接成功，否则创建父目录、append display路径换行，写成功才插入内存；不限制候选必须目录。路径信任不绑定内容hash、版本或worktree。
- revoke同样要求路径仍可canonicalize，已删除根不能通过此接口撤销；先从内存remove再create/truncate重写全文件，失败不恢复内存，HashSet写序无保证，无锁/atomic rename/fsync。load的空file_path导致后续grant不能持久化而不是自动选择其它位置。路径display文本与逐行trim格式不能保证非UTF8、换行或首尾空白路径往返。
- config auto trust候选canonicalize后starts_with dirs::home_dir的原路径；home本身未canonicalize，不使用user_grow_home/GROW_HOME作为边界。仅路径判断，不验证manifest或每个组件。trust模块注释所说技能只列元数据/阻止hooks等尚需registry调用核实。7单测完整读：grant/reload/revoke/comment/不存在；名为under_home的测试实际只检查不存在路径false，没有正向home边界测试。
- refresh_from_disk载registry，只有refreshed>0才save，save失败仅warn、不增加summary.errors也不回滚已换目录。调用时机spawn/reload只是此文件注释，调用方需另查。
- refresh只快照Local条目，git不计skipped。source是目录且auto trust或显式trust才继续，force也不绕过信任；!force按全树非symlink常规文件的相对路径与长度BTreeMap比较，读取失败视不同。检测增删改名/长度变化，不检测等长内容、空目录、权限与mtime变化，subdir只限制后续发现而不是比较/复制范围。
- recopy以dest父路径及文件名+PID生成tmp/backup；先按年龄清理同前缀兄弟项，删除同PID tmp再复制整source，发现失败或零manifest删tmp返回错误。dest先rename backup再提升tmp；两次rename之间dest短暂不存在，不能把注释读成无间隙原子切换。提升失败先删tmp，若dest已有则删backup；否则rename恢复，失败再copy恢复，copy失败error日志保留backup，仍返回最初提升错误。成功删backup并以发现集合替换repo.plugins、更新时间；复制/发现函数实现留待git_install核实。
- sweep按modified.elapsed >=3600秒清理prefix匹配项，错误best effort忽略，不检查PID是否活跃；同PID并发共用路径，跨进程刷新也没有此模块级互斥。不能由“fresh”假设保证超过一小时活跃刷新不被清理。
- local_refresh573行含8测试全部读完：新agent、未变化mtime、改名、注入提升失败恢复、外部未信任skip/已信任refresh、subdir发现、Unix symlink跳过；测试使用serial(home_env)与RAII改变HOME，symlink测试非Unix无创建链接。无等长内容force、save失败、跨进程竞态或stale活跃进程测试。本轮未执行。

累计18/27份Rust完整读取；agent保持pending。

- `crates/codegen/agent/src/plugins/mod.rs` SHA256 `7c4527588fd22f01c17d27bdd77a3f3d9f80f0919b7fb62dbeaf9f03fc9b385e`

- `crates/codegen/agent/src/plugins/trust.rs` SHA256 `60c44ea0a4e44948a986ebdafa33847d5be2e7021aa33a975b4577e2cd310384`

- `crates/codegen/agent/src/plugins/local_refresh.rs` SHA256 `369aeb4b0dabfaa29c7a62f0cdd89d6e65cf64804b69c0bb075dc9bb02e608b7`


### plugins/hooks_adapter.rs 完整审阅

- 全552行含13个测试读完。文件入口read_to_string失败返回空specs和单条带plugin/path的warning；inline入口序列化Value并用plugin_root/plugin.json作为synthetic source path，委托相同pipeline，无文件IO。相对命令解析使用该source，而外部hooks文件使用实际文件位置。
- 预过滤只处理顶层hooks对象的event keys，支持性完全委托HookEventName::parse_key；非法JSON原样交给统一parse_hook_file，未另建parser/事件allowlist。被移除事件产生info日志和返回warning；parse错误前缀plugin名，warn并保留统一parser产出的有效specs。
- 所有已解析spec覆盖extra_env的GROW_PLUGIN_ROOT/GROW_PLUGIN_DATA为调用者参数，保留其它env；layer固定Plugin，name为PLUGIN_HOOK_PREFIX+plugin_name+/+原name，未在此验证plugin_name或路径。适配函数自身不接收信任状态、不进行授权或运行命令，执行许可需registry/runner证明。
- 仅command字段做to_string_lossy→substitute_env_vars→config::expand_env_vars_in_string两阶段展开，有变化才换PathBuf；command_raw不改，args/env其它字段未在适配层展开。通用展开从进程环境解析的实际规则以config为准，不是extra_env插入后就可解析任意自定义env。替换后的plugin_root若含通用变量字符还会进入第二阶段，普通绝对路径测试不能证明任意输入恰好展开一次。
- 测试覆盖事件过滤/非法JSON/无hooks、文件与inline、路径替换且raw保留、空对象parser错误、两个plugin env覆盖、自定义通用变量的brace/bare展开。exactly_once测试只用不含美元符号的root；通用env测试用专属变量但没有serial或RAII，手动remove不恢复原值，测试注释single-threaded不等于harness设置。本轮只阅读，未执行测试。

累计19/27份Rust源码完整读取，agent仍pending。

- `crates/codegen/agent/src/plugins/hooks_adapter.rs` SHA256 `03f3e00851b136b0c11003789a98dfefc8e39891839175f7f45dee743ed013af`


### plugins/manifest.rs 完整审阅

- 731行含全部测试读取完成。PluginManifest camelCase/deny_unknown_fields，name必需；Author亦拒未知字段。validate仅验证name的1..64 ASCII小写/数字/连字符、首尾不为连字符，允许连续连字符及纯数字；version只是Option<String>，未校验semver，URL/email等也是原字符串。直接serde不自动调用validate，load_manifest才串联读取、serde、validate及inline日志。
- load只认root/plugin.json，不存在或非file报Missing，读取失败IoError，JSON/字段错误ParseError；不从目录名推断身份或探测替代manifest。is_file跟随symlink，本函数不独立验证manifest目标在root内。
- skills/commands/agents显式Some列表替换默认目录而非注释所称supplement，Some空列表禁用默认。显式路径按输入顺序join并containment过滤，不要求is_dir、不去重；None才对标准目录is_dir。
- containment分别canonicalize root与候选，失败各自回退原路径再starts_with；因此不等于所有路径fail-closed，未存在的带..候选可能保留词法前缀。显式hooks/MCP/LSP路径通过containment后还要求is_file；绝对路径若实际位于root内可接受。默认目录及默认文件分支只有is_dir/is_file，没有同样containment检查，可能跟随指向root外的symlink。执行是否再次约束待registry/runner核对。
- PathOrInline untagged String优先Path，否则任意JSON Value可Inline，并非只接受object；Option null通常表示None。hooks与LSP inline抑制默认文件路径，MCP inline仍返回存在的.mcp.json，可与inline同时出现，合并优先级不在manifest层。
- normalize_inline_mcp_servers仅在mcpServers字段为object时取该对象再包一层，否则将完整原Value包入；不做服务器schema校验，wrapped对象额外同级字段被丢弃。substitute_env_vars委托tools::util::substitute_plugin_tokens，未自建替换逻辑。
- 测试覆盖名称、最小/完整/未知字段manifest、inline、加载、替换、默认目录、已存在外部路径拒绝、正常内部路径、MCP归一与inline/文件共存；无默认symlink逃逸、canonicalize失败、任意inline类型、显式空目录列表测试。本轮未运行Cargo。

累计20/27份Rust完整读取；agent仍pending。

- `crates/codegen/agent/src/plugins/manifest.rs` SHA256 `2b001fcac6347589aad2d34386a1d806000752de45dad67c179b01dbbfef8762`


### plugins/install_registry.rs 完整审阅

- 全642行含11测试已读。Registry version/repos必需，嵌套record均deny_unknown_fields；InstallKind内部tag type精确Git/Local，字段snake_case，Option省略None。install_dir跳serde，直接反序列化为空路径，必须try_load_from回填；字段路径/时间/commit/version字符串不在此验证有效性或containment。没有enable状态字段，不能把安装登记等同于启用。
- try_load_from读取registry.json，缺失空version1，IO/JSON/非1版本错误返回Err；load_from将所有错误warn后转空registry。empty不读盘。save不验证version，public字段可设非1并写出随后load拒绝的文件。
- save_atomic创建目录、pretty序列化，检测GROW_TEST_FAIL_REGISTRY_SAVE_AFTER_SERIALIZE存在即错误，该开关未cfg(test)而是生产路径亦生效。临时名PID+UTC纳秒（无纳秒则0），write后rename到registry.json；rename失败尽力删tmp，write失败未显式清理可能部分tmp。没有fsync、锁或读改写合并，不保证断电durability/并发无丢更新。
- get/get_mut/insert/remove仅内存，insert同key替换；find_plugin按HashMap任意顺序首次命中，同名跨repo不稳定；list按repo key排序。find_repo_key_by_plugin_root以list顺序遍历，join记录subdir，再原path/调用者canonical参数或installed canonical等值比较；不自行canonicalize调用者两个参数，也不限制subdir越界。
- resolve_install_dir优先disk-only有效config plugins.install_dir string，只展开开头~/，否则PathBuf原样（可相对/空），加载错误/无字段退config::grow_home()/installed-plugins；不等同TrustStore的user_grow_home无home拒绝回退策略。
- repo_key basename移尾斜线及.git再按/取末段，小写ASCII字母数字连字符保留、其它字符变连字符并trim两端；hash实际DefaultHasher对原source取低32位8hex，不是注释声称SHA256，也未在此规范化source。可空basename形成-leading-hyphen，32位不保证唯一或跨Rust版本稳定。
- 错误类型还承载PluginNotFound/AlreadyInstalled/ShaMismatch/UnpinnedRemoteRefused/InstallFailed，仅定义不证明此模块执行对应策略。11测试含key形状/两样本不等、CRUD、serde往返、跨repo查找、subdir路径匹配、Git/Local缺subdir及Local往返；所谓save_and_load只直接serde并未验证load_from回填目录，未覆盖版本拒绝、故障开关或并发。未运行测试。

累计21/27份Rust完整读取；agent仍pending。

- `crates/codegen/agent/src/plugins/install_registry.rs` SHA256 `48cb50974cec448cf754d7a50441e49dfadd1b2bc5a81c2528795a73417fde1f`


### plugins/discovery.rs 生产实现读取（1–632行；测试未完成）

- scope序CLI0/Project1/User2/Config3；origin额外区分user目录和registry安装（包括可选marketplace显示名/git URL），不进入ID。PluginId为scope/SHA256(lossy路径字符串)前4字节hex/name，构造函数不自行canonicalize或验证name，调用collect才提供canonical。
- populate_plugin_lists已在任一enabled/disabled出现name或ID就不改，否则CLI/Config追加name到enabled，User/Project追加disabled；只修改内存config。discover_plugins自身并不按enabled/disabled过滤候选。
- 扫描顺序CLI入参顺序→cwd向git根项目目录→user_grow_home/plugins→registry→config路径。无user home不扫user目录，但registry load仍使用独立resolve_install_dir。parent只扫直接子目录，is_dir跟symlink，read_dir单entry错误跳过、目录错误warn；对子目录原路径排序，不是全局canonical排序。
- registry按key排序，内部plugins.values HashMap顺序；subdir拒绝绝对及ParentDir/RootDir/Prefix词法组件，未做symlink落点containment。root重读manifest，不取registry保存name/version作权威。
- collect canonical失败跳过；先将canonical插seen再load manifest，即使manifest失败也占用去重位置。同物理根由首次来源固定scope。CLI/User恒trusted，Config用home自动信任或TrustStore；Project完全取调用者project_trusted，不查per-plugin TrustStore。纠正trust模块旧注释：当前项目插件实际信任粒度是folder verdict。组件路径均解析并返回，trusted=false不在此抹掉可执行组件；是否执行待registry核对。
- 同名按scope最低胜，同scope首次胜；实际顺序继承各来源扫描，不能概括成canonical path字典序。conflict只保留最后写给胜者的一条冲突文字，不是全部loser清单；反序移除保留其余candidate顺序。日志has_hooks/MCP/LSP按文件路径Some，只inline可被日志漏计，不能当完整能力计数。

本文件测试从633行起仍需续读，累计完整文件数仍21/27，不登记完成哈希。


### plugins/discovery.rs 测试续读完成

- 676–1297行已读完，连同此前1–675行构成全文件。测试覆盖user根有无、CLI/User信任、无manifest拒绝、registry根与subdir发现、marketplace/git来源字段、../逃逸拒绝、重复根、CLI胜User、ID格式/同参稳定/不同路径、项目folder信任与非项目不受folder影响。
- dedup测试重复同一路径，没有symlink别名；collision测试仅CLI先/User后，未覆盖同scope、后到更高优先级或多次冲突文字。ID测试不保证32位无碰撞。注册subdir逃逸测试只覆盖词法../，不证明symlink目标约束。
- discover_real_project_plugin测试实际调用总入口，但只断言trusted标志，未运行MCP或验证执行拒绝；不创建git repo、不验证多层repo目录链。会读取宿主user/registry来源，按特定项目名筛选结果，并非完全隔离发现测试。无populate_plugin_lists的直接测试。未运行Cargo测试。

累计22/27份Rust完整读取，剩余agent discovery、prompt agents_md/skills、plugins registry/git_install五份；agent仍pending。

- `crates/codegen/agent/src/plugins/discovery.rs` SHA256 `be332abfffe51e9e83d13b503f54860c81b2847113227e459ccd8e4c82369bc3`


### prompt/agents_md.rs 完整审阅

- 全784行读取含测试。AgentConfigFile序列化保存filename/path/content原String；find_agent_files只枚举精确AGENTS.md且is_file，读取目录失败/entry错误静默跳。rules只直接子项扩展名md大小写不敏感，按filename排序，无is_file筛选；最终read_to_string失败跳，非递归。symlink跟随，未做root containment。
- 公共async入口取optional_workspace_user_dir、tools grow_home；内部全同步fs/git调用，无异步IO。git2 discover workdir后自行从cwd父链按原路径等值root停止再reverse，不使用RepoDirChain的canonical停止/home-root特殊处理；路径拼写不一致可能走到文件系统根。非git只cwd，忽略传入workspace_user_dir。有git时workspace user不在canonical链则插入index1，未验证它属于repo。
- home扫描AGENTS.md与rules，先于project AGENTS.md与.grow/rules；每根named先于排序rules。build_gitignore同时用于候选，具体语义见ignore；不读取frontmatter globs做条件过滤。root去重以canonical+rules_subdirs组合，scan_named_files合并OR。
- 文件canonical去重失败退原路径；重复候选is_rule OR，后遇project会将原home候选移到project位置并替换显示路径、更新所有索引。故同物理AGENTS被rules symlink引用也会按rule抽body。读rule委托extract_skill_body去frontmatter，named保留全文；空内容仍可返回。路径display可能lossy或相对，不保证struct注释所谓绝对。
- format按传入Vec顺序不排序，空列表None，否则固定system-reminder包装，逐文件From路径与内容经neutralize_reminder_tags，不使用file_name字段，无长度上限/截断。包装文案说root到cwd但还含home/workspace user，优先级靠提示文字而非本模块执行规则。提醒标签中和不等于通用XML/Markdown转义或语义防注入。
- 测试包含精确名/替代名、规则排序、5000字符不截断、workspace插入/去重/None、非repo、home/project重合与嵌套顺序、Unix同物理named/rule合并、frontmatter处理、reminder大小写/下划线/带属性中和。未测gitignore、cwd别名停止边界、规则globs条件（实现无此条件）、外部workspace目录限制；with_options测试可能读宿主growhome，with_roots部分测试显式隔离。全部只静态阅读，本轮未执行。

累计23/27份Rust完整读取；agent仍pending。

- `crates/codegen/agent/src/prompt/agents_md.rs` SHA256 `ce052b89a6b38dd6dcb77abddaf9ca99556805ed89780a4f0d737dd673221e0f`


### plugins/registry.rs 生产实现读取至650行

- LoadedPlugin保存组件路径/inline/计数/信任/启用，data_dir每次取config::grow_home再join plugin-data/id，未创建目录，public id不自行containment。from_discovered全部scope都要求显式enabled且不disabled（name或完整ID精确匹配）；双方均列warn且disabled胜，双方未列warn且禁用，并非仅Project默认禁用。
- 所有候选包括disabled/untrusted也读组件元数据并计数；active_plugins=enabled&&trusted，enabled_plugins只enabled。注册表本身不启动hooks/MCP/LSP、不阻断调用，消费者必须选择正确视图。list按scope/name，get按name；from_discovered若绕过discovery传同名，plugins后者覆盖而mcp_owners首次claim保留，接口自身不再去重校验。
- MCP owner仅enabled+trusted时按传入顺序首次server名胜，精确名不含namespace。MCP计数是文件mcpServers键与normalized inline键去重并集，不解析server有效性；hook计数直接累加任意event的内层hooks数组，包含不支持事件/无效handler，不能视为可执行spec数。LSP只算对象键数，文件优先且坏文件不退inline。
- skill_md_paths委托find_skill_md_paths，按normalized父目录basename首个去重，无法取UTF8basename则保留；显示skill_names仍原basename。commands/agents只直接read_dir file_type常规file且精确.md，不跟文件symlink、不排序/去重；commands计入skill_count/skill_names，重叠目录可重复，未按frontmatter有效性过滤。
- SharedHandle Arc<RwLock<Option<Arc>>>快照便宜clone，锁poison unwrap panic。build_for_cwd只读盘，disk config CLI dirs后追加启动CLI再session dirs，同scope优先实际是此顺序；不会写配置populate结果，不刷新local。发现空且有session dirs保留空registry以便重建，无session dirs返回None，和启动CLI是否存在无关。
- refresh_and_build先force=false刷新再调用build（信任重新load）；reload按调用者force刷新，合并启动CLI而不合并session dirs，discover后替换shared最新Arc，返回所有发现数而非active数。旧Arc快照不变；读取/构建在写锁外，不保证并发reload按开始顺序提交。调用者可直接给force=true，无本层用户动作验证；实际spawn/reload wiring留待host核对。

测试从625附近开始尚未读完，registry不计完整文件；累计23/27不变。


### plugins/registry.rs 测试续读完成

- 全1243行已读取。测试涵盖根SKILL、同basename/重叠路径去重、惯例目录计数、空registry/查询、origin传递、disabled name/ID、未列默认禁用、populate四scope及保留已有项、list排序、data_dir包含ID、inline hooks/MCP保存、MCP文件与inline名去重和owner、enabled但untrusted排除active。此前discovery中“无populate直接测试”仅限该文件，本registry含对应测试。
- inline_mcp_ownership_not_tracked_for_untrusted同时未enabled，因此单独不能证明信任门禁；其它active测试覆盖enabled/untrusted，但没有同条件MCP owner独立断言。data_dir只断言子串，未验证完整根解析或路径安全。pre_enabled测试注释提及外部settings，实际只手工构造DiscoveryConfig，不能推导该配置文件被加载。
- 本文件无SharedHandle build/reload/旧snapshot保留测试，无同名输入owner一致性、跨plugin同server冲突顺序、hook/LSP计数有效性或enabled/disabled同时列入的专门断言。统计测试只证明样例目录计数，未证明与完整技能解析结果始终一致。本轮未执行Cargo测试。

累计24/27份Rust完整读取，剩余discovery.rs、prompt/skills.rs、plugins/git_install.rs；agent仍pending。

- `crates/codegen/agent/src/plugins/registry.rs` SHA256 `e6b55519f2251e4dc74c8330f9a966b4a5cf9aa0b8dc638be0135cbbb99168da`


### discovery.rs 生产实现读取至760行

- project_agent_dirs复用RepoDirChain/.grow/agents链。用户agent资源固定home/.grow/agents，传入grow_home故意忽略；discover项目深到浅再user，名字首个有效定义胜，不列builtin。invalid_primary只筛非subagentOnly且不满足floor的文件定义，不含plugin。
- 原生agent递归按原path排序，nested相对路径去扩展名/拼接成为ID；is_dir跟symlink，无visited/depth限制，循环链接可无限递归。exact.md，不使用gitignore；载入成功后才占seen名字。按名查直接join name.md，未拒绝绝对/..，不要求来自枚举目录；原生查找到后name设原selector，scope显式覆盖。
- by_name内置先于user，by_name_in_cwd项目先于内置再user；all_subagents seed builtin subagent_variants，再merge所有发现定义（不要求subagent_only），仅project允许覆盖内置子代理名，普通同名按Project>User>Bundled>BuiltIn严格更高才替换。toggle精确name遗漏默认true，仅原生merge阶段应用。
- plugin列表使用enabled_plugins（可不trusted），agent_dir仅直接read_dir .md、顺序未排序；load插件trusted读body，untrusted仅frontmatter（配置文件实现仍读全文件）；标plugin_name，列表名plugin:def.name，重复qualified首个留。追加plugin阶段不应用toggle；scope列表Project映Project，其它User，但加载def.scope仍由from_file路径推断，未在helper覆盖。
- plugin-aware按名首个冒号保留为插件命名空间，不允许原生冒号名shadow；qualified只找enabled指定plugin，遍历agent目录拼agent_name.md，未做selector containment。bare先原生，再收所有enabled插件文件候选，只有候选数恰1才解析返回，同plugin重叠目录也可被计成多匹配；多个候选中坏manifest也导致歧义，不先过滤有效性。
- plugin加载失败warn None；untrusted也返回frontmatter定义而非整体拒绝。body存在才做plugin token替换，不替换其它配置字段；列表description未替换token。ConfigSource优先plugin元数据否则scope，缺source_path用空PathBuf。parse_agent_frontmatter_only只是委托。

本文件测试尚未完成，完整文件数仍24/27。


### discovery.rs 全部测试续读完成

- 全1468行已读。测试确认builtin子代理实际三个general-purpose/explore/browser-use，纠正函数文档只列两个。覆盖原生.md/坏文件/路径ID、home资源根、项目shadow、用户/bundled不shadow内置、toggle原生、scope merge、插件qualified可见、native bare优先、未知color宽容及body token替换。
- test_discover_dedup_by_name未init git且仅断言一条，不验证父定义实际被扫描或子description胜出；test_merge_invalid_user_agent只是空Vec，未实际解析坏文件；unknown by_name测试可能访问宿主用户目录。Bundled测试仅synthetic scope，不证明存在bundled扫描入口。
- qualified_plugin_identity_cannot_be_shadowed测试只验证lookup，不验证列表；生产列表先merge原生冒号名后遇同qualified会skip插件，可能展示native描述而lookup加载插件。独立债务登记，不在盘点改运行时。
- 插件toggle不在追加阶段应用、bare同插件多个目录歧义、未信任body不加载、symlink递归环及selector越界均无该文件专门测试；现有token测试使用伪root参数，不证明实际文件在该root内。全部静态阅读，未运行Cargo测试。

累计25/27份Rust完整读取；剩余prompt/skills.rs和plugins/git_install.rs，agent保持pending。

- `crates/codegen/agent/src/discovery.rs` SHA256 `3a7147d2d46b4888cc1d5d3c7e6d98fdc92578a13f7af4d1af32ae2dd02d0ee8`


### plugins/git_install.rs 生产实现读取至870行

- parse_install_source不返回错误：最后非空#拆subdir，含://或git@视Git；git@前缀在首冒号后拆最后@，其它URL拆最后非空@，可能将userinfo误作ref。两非空/段且无/./~开头视GitHub shorthand，不查本地存在；其它本地相对cwd，~/无home退“.”，不canonicalize。尾空#留在main，字段未经subdir验证。
- URL/ref trim后拒空、NUL、首-，无协议allowlist；SHA trim后精确40/64 ASCIIhex。ensure_pinned仅policy true要求完整SHA；hoist显式SHA优先，否则full-ref提到SHA并清ref。normalize只Git，local原样。source ID URL或local.to_str（失败“local”）加可选#subdir，不含ref/SHA，故不同pin共享安装key。
- install先normalize/pin gate，再registry key重复拒绝；创建install目录后clone或整树copy，不自动insert/save registry。local无trust检查；copy错误、discover返回Err不清理已复制目录，唯空plugins清理。已有未登记目标无显式拒绝，copy会合并覆盖；clone失败清target，不能假定target始终新目录。
- clone普通depth1、可选branch、--隔开URL/target，通过tty_utils git_command；SHA路径init/remote add --/fetch depth1 --/checkout FETCH_HEAD，HEAD与SHA忽略大小写比较，否则清理并报ShaMismatch。命令output同步等待无本层timeout，stdio策略委托tty_utils。非pin读HEAD失败仅commit空字符串记录，pin路径必须读成功。
- remove_repo_path删symlink本身或目录，普通文件/不存在返回Ok且不删。cleanup_plugin_data用记录root未canonicalize生成PluginId，与discovery canonical ID可能不同；路径无额外验证，删除错误忽略。
- discovery subdir拒绝绝对/..等词法组件，is_dir跟symlink无canonical containment；root有有效manifest立即返回单插件，否则仅一层真实目录file_type扫描，不排序、不递归、不跟子目录symlink。坏manifest静默忽略，read_dir失败得空Vec；repo_plugin_map同名最后覆盖（继承无序read_dir）。
- build_installed_repo用result内部规范化kind，忽略传入source，时间设同一now、marketplaceNone；update Local直接LiveLocal，不检查源；Git fullSHA或v开头且有点启发式视Pinned（并不查询tag类型）。可更新路径require_sha=true拒绝，然后pull --ff-only依据当前checkout上游而非显式URL/ref，比较registry旧commit与新HEAD，发现组件后返回结果但不持久化，也不因零插件回滚。
- copy递归跳过源内symlink及特殊文件，文件fs::copy，目录create_dir_all；不保留目录mtime/权限，不限制深度、src/dst重合或dst已有symlink，存在TOCTOU可能性不宣称强隔离。本次不执行安装/删除。

测试未读完，仍25/27完整文件。


### plugins/git_install.rs 测试续读完成

- 全1590行已读。解析测试覆盖HTTPS/SSH/GitHub shorthand、ref/subdir组合、三段本地路径、绝对/相对路径；发现测试覆盖root、多直接子目录、selector、词法逃逸及manifest版本。operand测试明确接受ext::helper-specific-address，不能把本层校验描述为协议隔离。
- 真实Git测试使用file://临时origin与git命令，缺git时提前return，harness可能显示passed而未执行Git。正确pin测试checkout+HEAD，错误pin测试实际为fetch-by-sha失败InstallFailed，不覆盖成功fetch后HEAD不等的ShaMismatch分支。两项update测试无新增origin提交，仅验证pull后subdir重发现/缺失报错，未证明changed=true或快进冲突处理。
- argv --、格式错误早于目录创建、40/64hex/hoist/显式空SHA拒绝、require_sha拒绝、规范化kind、真实ref槽SHA安装都有覆盖；规范化持久metadata测试只构造InstalledRepo并未save/reload。64hex仅格式测试，无SHA256 Git仓库集成；无远程SSH/HTTP认证验证。
- 本文件无local copy成功/源删除独立性、copy失败清理、已存在未登记目标、重复manifest名、cleanup data canonical路径、超时或源/目标嵌套测试；local_refresh已有部分copy/symlink集成测试（此前已读）。本轮未执行测试或任何安装。

累计26/27份Rust完整读取；剩余prompt/skills.rs，agent仍pending。

- `crates/codegen/agent/src/plugins/git_install.rs` SHA256 `2af852a04d90c87e194491190e21f0199057839d4b640136ecac17463a7f79cf`


### prompt/skills.rs 生产实现及测试开头读取至850行

- SkillsConfig5个Vec默认空，无deny_unknown_fields。list原生roots→config paths→server→bundled，filter ignore后stable scope排序再native去重，然后追加插件；ignore不施加插件，disabled只精确skill.name匹配（不是qualified/dedup key），在merge后设enabledfalse。None cwd仍能有workspace/config/server/bundled/plugin来源，不是注释所称仅User。
- collect_skill_config_dirs自行git父链原路径停root，非repo只cwd；workspace .grow随后加入（无git也加），用户roots固定home/.grow，config file则监视parent，存在目录canonical首个去重。此函数不含server/bundled/plugin专属目录，不能孤立视为完整watch集合。scope先检查dir.parent等当前进程home再cwd再git prefix，否则User，使用原路径不是canonical。
- 每config根skills先commands，但不同根先后优先于类型；canonical同路径首次收集，原生bundled用root/bundled再find_skill_paths（内部布局留tools确认）。不看gitignore。tilde仅~/且HOME或USERPROFILE存在才展开，无home保留原样，相对config路径不自动相对session cwd。config来源stamp ConfigToml，scope仍决定优先级。
- native dedupe同时canonical path/name：同路径保留首项但补后来config_source；同scope非Server/Bundled冲突尝试把新项改为normalized目录名，失败若新项是目录名owner尝试移动旧项，否则可替换display_name已Some旧项。crossscope保持先到覆盖。rekey要求目录名合法/空闲/不同，display_name保存旧name；seen_names的历史映射不统一删除。
- plugin按enabled_plugins读skill与commands，不因trusted=false停止元数据解析；stamp scope CLI Local/Project Repo/User User/Config Plugin，写plugin root/data/version/source，目录名非空且不同就改identity（此处未再is_valid）。native名字碰撞仅debug，插件按dedup_key首个保留，不与native做路径去重。
- filter ignore是canonical path starts_with，不是glob；canonical失败回原路径。format单skill要求body存在且非empty，不trim、不检查enabled，委托build_skill_message；多skill双换行连接并首尾双换行。注释plain markdown与</skill>残留不能证明实际wrapper，要以tools函数为准。
- resolve_preloaded按声明顺序首个ASCII不分大小写裸name或format_skill_name匹配，不自行筛enabled或判断插件bare歧义、不去重；load_skill_content对冻结body权威规则委托tools，成功clone/body非空Some，空body也push；找不到/加载失败warn跳。
- 已读测试开头server/bundled覆盖及递归路径前两项，其余850后待读。完整文件数仍26/27，agent pending。


### prompt/skills.rs 测试续读851–1500行

- 已读递归混合/空目录/缺目录/深度上限/父子SKILL共存测试；这些调用tools discovery共享函数，不把实现归属误记在agent。深度测试只浅层与超限，不精确覆盖limit边界。
- 首段抽取跳heading、多行合并、空/纯heading、UTF8 description fallback；首个多字节测试自己计算char boundary后调用抽段，不直接验证生产截断，而且构造prefix+filler长度恰使2048落在emoji边界。另一个真实文件测试经parse_skill_files只断言非空描述。
- frontmatter测试覆盖allowed-tools逗号/空格/list、model/effort、license/compatibility、metadata author/short-description提取、名称规范化、无frontmatter目录fallback、colon值恢复及全符号拒绝。都是共享parser测试，不证明allowed-tools/model被运行时执行。
- workspace目录传参测试覆盖加入、cwd内去重、None不扫兄弟、递归；helper注释“bare git”不符Repository::init普通repo。config paths覆盖目录/直接SKILL/root SKILL、repo/user scope、缺路径、重复路径；ConfigToml stamp测试刚开始，1501后待读。

完整文件数保持26/27，本轮未运行Cargo。


### prompt/skills.rs 全部测试续读完成

- 1501–2591行完整读取；全文件2591行完成。后半测试覆盖ConfigToml stamp、plugin root/data与目录identity、root SKILL及重叠目录merge、ignore精确/目录前缀、自定义路径、auto/config路径去重及来源继承、ignore后低优先fallback、disabled保留列表、bundled低优先、gitignored .grow技能/命令仍发现、command无frontmatter fallback、同根skill胜command。
- 同scope冲突测试覆盖新项rekey、归还basename owner、多个claimant、basename已被更高scope占、rekey后压低scope、同scope跨root、副本被真实owner替换、crossscope隐藏、同basename首个胜及真实复制目录 stale frontmatter实例。大部分直接synthetic路径输入dedupe，不验证多进程文件变动或canonical别名。
- 本文件没有resolve_preloaded_skills/format_skills_for_injection直接测试；未覆盖disabled预加载、qualified disabled、ignore插件、未信任插件解析、同名插件bare预加载歧义。部分list_skills总入口测试读取宿主用户技能，仅通过特定名字筛结果；helper roots版本才隔离。

agent的27/27份Rust源码及manifest已全部静态审阅，另10份嵌入提示词及agent.md.example已读；包测试、契约映射和正式证据登记未完成，inventory继续pending，整体仍51/61。

- `crates/codegen/agent/src/prompt/skills.rs` SHA256 `0d051e3a3a61ee8cf54a336769f49c626edc9f64cf20222e429cbd70ce8ae1b9`


## 正式契约登记

已将99项契约登记feature-map与extension-runtime delta；469项测试通过，测试后cargo clean释放1.5GiB。
- extension-runtime/Plugin registry explicit enablement
- extension-runtime/Plugin registry active view
- extension-runtime/Plugin discovery folder trust
- extension-runtime/Plugin discovery source precedence
- extension-runtime/Plugin manifest exact identity
- extension-runtime/Plugin manifest name validation
- extension-runtime/Inline MCP sibling file visibility
- extension-runtime/Plugin hook environment precedence
- extension-runtime/Local plugin structural refresh
- extension-runtime/Plugin install registry version
- extension-runtime/Plugin source parsing
- extension-runtime/Plugin git operand validation
- extension-runtime/Plugin immutable pin gate
- extension-runtime/Plugin SHA checkout verification
- extension-runtime/Plugin local snapshot copy
- extension-runtime/Plugin install discovery scope
- extension-runtime/Plugin update policy
- extension-runtime/Agent resource home independence
- extension-runtime/Agent native lookup precedence
- extension-runtime/Agent plugin qualified lookup
- extension-runtime/Agent untrusted plugin role omission
- extension-runtime/Agent project instruction reminder
- extension-runtime/Agent rules body extraction
- extension-runtime/Skill native ignore filtering
- extension-runtime/Skill preload resolution
- extension-runtime/Skill plugin identity stamping
- extension-runtime/Skill same scope collision recovery
- extension-runtime/Skill provenance path deduplication
- extension-runtime/Skill command source order
- extension-runtime/Skill disabled list marking
- extension-runtime/Skill configuration file source
- extension-runtime/Skill injected source precedence
- extension-runtime/Skill plugin qualified preservation
- extension-runtime/Skill preload message formatting
- extension-runtime/Agent plugin hook event filtering
- extension-runtime/Agent plugin hook command display preservation
- extension-runtime/Plugin registry snapshot retention
- extension-runtime/Plugin session registry directory retention
- extension-runtime/Plugin read only registry construction
- extension-runtime/Plugin MCP ownership lookup
- extension-runtime/Plugin trust canonical membership
- extension-runtime/Plugin trust revoke failure ordering
- extension-runtime/Plugin manifest component directory override
- extension-runtime/Plugin install key source identity
- extension-runtime/Plugin registry save replacement
- extension-runtime/Agent stable prompt composition
- extension-runtime/Agent role prompt modes
- extension-runtime/Agent role template failure fallback
- extension-runtime/Agent prompt placeholder fields
- extension-runtime/Agent audience instruction delivery
- extension-runtime/Agent resource activation ownership
- extension-runtime/Agent runtime snapshot rendering
- extension-runtime/Agent VCS status byte cap
- extension-runtime/Agent workspace user path interpretation
- extension-runtime/Agent built in prompt embedding
- extension-runtime/Agent definition strict fields
- extension-runtime/Agent file derived identity
- extension-runtime/Agent JSON identity validation
- extension-runtime/Agent JSON runtime field omission
- extension-runtime/Agent runtime equivalence comparison
- extension-runtime/Agent selector plugin namespace
- extension-runtime/Agent native preset precedence
- extension-runtime/Agent primary capability eligibility
- extension-runtime/Agent strict harness predicate
- extension-runtime/Agent file tool override scope
- extension-runtime/Agent MCP inheritance matching
- extension-runtime/Agent memory scope directory resolution
- extension-runtime/Agent builder explicit definition precedence
- extension-runtime/Agent curated empty toolset rejection
- extension-runtime/Agent default optional tool injection
- extension-runtime/Agent memory backend removal
- extension-runtime/Agent workflow tool runtime gate
- extension-runtime/Agent coordination tool audience gate
- extension-runtime/Agent task lifecycle normalization
- extension-runtime/Agent parameter merge overwrite
- extension-runtime/Agent preloaded catalog precedence
- extension-runtime/Agent session tool clamp last
- extension-runtime/Agent task model guidance catalog
- extension-runtime/Agent task user description preservation
- extension-runtime/Agent authored tool filtering
- extension-runtime/Agent repository chain home guard
- extension-runtime/Agent repository chain canonical stopping
- extension-runtime/Agent construction timing events
- extension-runtime/Agent primary subagent merge toggles
- extension-runtime/Agent background refresh stale sweep
- extension-runtime/Agent refresh registry save reporting
- extension-runtime/Agent stable resource roots
- extension-runtime/Agent instruction gitignore inputs
- extension-runtime/Agent builder resource discovery seeding
- extension-runtime/Agent builder preloaded listing exclusion
- extension-runtime/Agent builder display path rewriting
- extension-runtime/Agent builder empty stable prompt fallback
- extension-runtime/Agent plugin component path fallback limits
- extension-runtime/Agent session clamp empty distinction
- extension-runtime/Agent explicit subagent policy
- extension-runtime/Agent subagent list input normalization
- extension-runtime/Agent MCP inheritance input schema
- extension-runtime/Agent presentation color tolerance
- extension-runtime/Agent authored toolset resolution
