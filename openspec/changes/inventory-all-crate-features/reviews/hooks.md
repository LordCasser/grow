# hooks 逐包审阅

以下按阶段保留审阅过程；目前全部源码、集成测试及示例已读完，26项要求已映射。阶段中的待办描述是历史状态。

- manifest无feature，公开config/discovery/dispatcher/error/event/matcher/result/runner/trust，env_expand私有；依赖reqwest/process/regex/toml等。lib的v0四事件、command-only说明已落后，runner当前明确Command与Http，事件全量待event核对。src8470行，独立integration774行，另示例JSON与shell/python脚本纳入范围。
- HookError覆盖读取、解析、regex、timeout、command I/O、invalid output、command missing、invalid config、不支持handler，保存名称/路径或source；不是wire serde契约。
- HookMatcher空或*匹配全部；仅ASCII字母数字/_/|为精确列表，空term和重复term删除，纯|列表不匹配任何值。其余是未锚定Regex，空白有意义，不做Bash/Read等别名展开。invalid regex构造Err；never匹配false。但matcher_allows缺matcher或缺value直接true，因此Never在缺值路径仍被绕过。测试覆盖exact/pipe/regex/anchors/invalid/never/空白/无别名，未测试所有matcher_allows组合。
- result模型区分Allow/Deny，StopHookOutcome可同时block/context/force_stop，is_empty只检查Option None；Some空字符串仍非empty。HookRunResult区分Success/Skipped/Blocked/Failed/TimedOut/Cancelled，skip原因MatcherMiss/Disabled/PolicyDisabled/PriorBlock/ProcessInterrupted；HttpInfo同时携带expanded url与raw_url，类型不脱敏，raw_url原文也不能无条件认为不含字面密钥。
- trust文件实际是user_grow_home/disabled-hooks，按trim非空、非#注释行与原hook_name精确比较。读错默认未禁用；disable先读查重后append一行，无锁/原子性/名称换行校验，父目录创建错误忽略；enable缺文件false，其他读错Err，找到后File::create截断并逐行重写，保留其他行但规范化换行，无并发更新或崩溃保证。没有用户home时disableErr、enablefalse、查询false，不是信任审批存储。
- runner入口按HandlerType调command/http，RunContext为session/workspace/process_scope；输出tuple结果/elapsed/可选HTTP元数据。GateHookJson deny_unknown_fields，decision必需，reason可选；精确deny/block生成Deny，None reason补默认而空白reason保留；allow允许，未知值Err。
- StopHookJson camelCase严格未知字段，均可选；block缺/空白reason用默认，approve或缺decision不阻断，未知值Err（即便continue=false也先错误）。additionalContext空白省略但非空保留原值；continue=false设force_stop，stopReason原样可空。多个信号可同时存在，force_stop优先聚合需dispatcher核对，runner转换自身不清除block。

## Env expansion与test support（完整阅读）

补读env_expand110–856，结合此前未截断的1–109，全856行已覆盖；test_support122行全部读完。

- expand_env_vars_with_extra以extra优先、process env其次，非Unicode环境值视为缺失；shellexpand no_errors保留未解析plain引用。单次展开不递归处理替换值，empty extra覆盖process值。注释的幂等仅对无进一步引用/环境变化的输入成立，值中带$VAR经过第二次调用可继续展开。
- modifier masking每次fastrand两u64拼128位hex sentinel，以PUA前后缀包裹，替换被识别modifier的起始${，展开后全局restore。debug_assert只查input和extra值是否碰撞，不查process env；release没有注释所暗示的return input fallback，仍继续mask/replace。随机化降低偶发碰撞，不是安全随机身份机制。
- iterator裸变量必须ASCII字母/_开头，后续数字可用；braced名字却接受数字开头，读ASCII alnum/_前缀，后面任意内容到首个}算modifier，不解析合法Shell修饰符。空/无合法前缀的braced yield空name且非modifier；未闭合只跳当前$继续扫描，特殊$字符组合跳两字节，不做Shell quote/escape语法分析。
- 嵌套${A:-${B}}只识别到第一个}，mask仅替换外层${，内层${B}仍原样留在masked字符串中送给shellexpand；因此不能凭mask_helper测试声称完整展开时嵌套内层必然保留。现有nested测试只直接调用mask_helper/iterator，不含extra B的expand端到端断言。普通modifier、plain+modifier混合、set-but-empty、legacy sentinel/UTF8/替换值非递归等有对应测试。
- 本helper不做shell quoting或URL编码，替换内容是否成为命令语法取决于后续runner；HTTP运行前再展开与command加载时展开的不对称仍需config/runner具体调用核对。
- with_env_var保存var_os原值，catch_unwind恢复后resume panic，支持嵌套；没有锁，唯一测试变量命名约定不是线程同步。四测试覆盖正常/原缺失/panic/显式unset，部分测试默认起始测试变量未设置，不能作为任意环境下的完全隔离证明。后续运行本包测试采用串行。

## Event 目录与载荷（此前完整读取，本阶段补存）

完整覆盖 event.rs 903 行。事件表同时生成 exact snake_case wire/display/parse 与稳定声明顺序；未知名称错误列出已知键。15 事件及 gate/matcher：session_start Observe/Tested，user_prompt_submit Prompt/Ignored，pre_tool_use Tool/Tested，post_tool_use、post_tool_use_failure、permission_denied Observe/Tested，stop Stop/Ignored，stop_failure Observe/Tested，stop_cancelled Observe/Ignored，notification、subagent_start Observe/Tested，subagent_stop Stop/Tested，pre_compact、post_compact、session_end Observe/Tested。此为元数据，不独立证明调用方实际触发和取消语义。

- Envelope 为 Serialize camelCase，含事件名、session/cwd/workspace/timestamp 字符串及可选 transcript/client/prompt/permission，payload flatten untagged；事件名与 payload 是独立公开字段，没有配对验证，时间等字符串不作格式检查。
- 载荷覆盖启动 source/model/agent，结束 reason/turn/tool 计数，stop reason/stopHookActive/lastAssistantMessage/backgroundTasks/sessionCrons，失败 kind/details，取消 category/trigger，工具前后 input/result/truncated/duration/background/subagent，权限拒绝工具输入，prompt，notification type/message/title/level，子代理 id/type/description/stop phase，压缩 source。Option None 省略，Some 空列表保留；后台任务和 cron 类型仅描述快照，不实现任务或调度。SubagentStopPhase Gate/Observe 均可序列化，不能从类型断言 Observe 已有发射调用。
- matcher 值为工具名称、通知 type、子代理 type、session_start/compact source、session_end reason、stop_failure error.as_str；stop/prompt/stop_cancelled 无值。通常仅 exact 空字符串变 None，空白保留；notification 不回退 level。
- StopFailureKind 六值 RateLimit/AuthenticationFailed/InvalidRequest/ServerError/ContextWindowExceeded/Unknown，CancellationCategory 五值 HookDenied/PermissionRejected/PermissionCancelled/PermissionTimedOut/MidTurnAbort，snake_case wire。
- clip_text 按 Unicode 字符取前 max 个再追加省略及剩余计数，结果可长于 max；stop entry 1000、取消 trigger 64 是显式 helper 限额，序列化不会自动裁剪。
- truncate_payload 先完整序列化，超过 128 KiB 后按 UTF-8 边界截取序列化前缀，加标记并包装为 JSON String。会改变原值类型；先行分配不受限，标记及二次转义可能使最终 JSON 超限，也不是整个 envelope 的硬限制；truncated 标记依靠调用方设置。
- 测试覆盖 event exact key、traits、枚举 wire、Unicode 裁剪、stop 快照与字段命名。取消 envelope 大小测试手动裁剪且其他字段短，不证明一般上限；没有事件/payload 配对或实际事件发射的端到端保证。

## Config 完整阅读

完整覆盖 config.rs 1369 行（含全部测试）。

- HooksMap JSON/TOML 均先解析整个事件到 matcher group 结构：未知事件、错误 group/handler 字段、非字符串 env 值导致该 map 整体失败。MatcherGroup 和 RawHandler deny_unknown_fields；hooks 数组必需，允许空数组/空 map。JSON 文件顶层必须恰有 hooks 一个键；frontmatter helper 接收的直接是 hooks 值，默认 source_dir 为 .。
- 通过结构解析后，build_specs 按事件声明顺序排序，事件内保留 group/handler 源顺序；名称为 file stem 或配置 source_name 加 event/group/handler 索引。无效 regex 跳整个 group，handler 类型/必需字段/on_failure 错误只跳对应 handler，收集错误并保留其他有效项。HashMap 结构错误遇到哪个事件不保证稳定。
- 配置层按传入顺序逐层处理，一个层解析失败不影响后续层；保留各层 provenance 和目录。注释称高权威优先，但函数不自行排序或 dedup；去重由 registry 承担。相对 command 此处仍保存相对 PathBuf，与 source_dir 分开；不能把解析阶段描述为已拼接/检查路径。
- HandlerType 仅 command/http。command 必须有 command，http 必须有 url；空字符串并未在这里拒绝，存在另一类型字段时被忽略而非报错。超大 timeout 秒饱和乘 1000，零允许；Stop/SubagentStop 默认 600000ms，其他 5000ms，没有一般最大值检查。
- 原始 on_failure Option 保留字段存在性：只有 pre_tool_use/user_prompt_submit 允许显式配置，其他事件即使显式 allow 也拒绝；缺省 Allow。HookSpec::validate 只检查 Block 是否属于这两个事件，不验证 command/url、超时、matcher 或字段一致性；反序列化后的 Allow 无法恢复原字段是否缺省。
- HookSpec derive serde 严格未知字段，compiled matcher 使用 serde(skip)；反序列化本身不会按 configured_matcher 重新编译。registry 边界行为后续核对。enabled 构造时 true；raw command/url 保留原文，不能保证原文没有字面密钥。
- 空 matcher 转 None；忽略 matcher 的事件保留非空原文供显示，但不编译，非法 regex 因而也不产生该错误；其他事件编译一次，matcher 不作环境展开。按源码确认此事实；测试 parse_hook_file_matcher_is_not_env_expanded 的 pattern 实际没有 $，不足以独立证明展开区别。
- env 缺失/null 都为空 map，值必须字符串且原样保留。先删除 RUNNER_ALWAYS_SET_ENV 中保留键并 warning，再以 extra 优先展开 command/url；缺失变量引用保留。这里只调用展开，未作 command 存在性或 URL/SSRF 验证。HTTP 是否运行时再展开需 runner 核对。
- hook_origin 优先 provenance User/Plugin；File 才按 global/、project/、agent:、plugin/ 前缀分类，其他 Unknown。名称前缀是 File 的分类启发式，不是可信身份校验。
- 测试覆盖 JSON/TOML、层叠加和 registry 去重示例、错误文件、handler 类型、超时默认、admission on_failure、source_dir、matcher 差异及 env 行为。测试读取完成尚未执行本包测试，不能将测试存在算作通过。

## Discovery 完整阅读

完整读取 discovery.rs 974 行及 config/global_hook_sources.rs 的共享文件名谓词；dispatcher 已读到290行，后续继续。

- 注册表是内存快照，没有文件监听；磁盘修改只在再次显式加载时进入新快照，不能把注释“仅新session”提升为禁止当前session调用reload的类型保证。has_enabled_hooks 每次调用仍检查 validate/enabled 和磁盘 disabled名单，故文件定义快照与动态禁用查询不同。
- hooks_for 返回指定 map key 的列表；len/is_empty 包含 disabled 项。append_specs 校验后追加，不去重、不编译 matcher；remove_by_prefix 删除所有匹配名称；all_hooks/into_specs 按 ALL 声明顺序展平并保留事件内顺序。derive Deserialize 本身不走 append 校验，也不保证 map key 与 spec.event 一致。
- recompile_matchers 仅处理 configured_matcher Some，成功覆盖，非法模式安装 Never 并 warning；None 不清除既有 matcher。没有检查事件 MatcherPolicy::Ignored，因而可把加载时忽略的 pattern 重新编译。Never 仍受 matcher_allows 缺值直接允许的边界影响，不能笼统称所有事件 fail-closed。wire restore 必须由调用方显式调用，未调用时 skipped compiled matcher 为 None。
- HookSource 支持明确文件或目录。collect 按 global 来源顺序全部在 project 之前，分别加 global/ 和 project/ 前缀，不去重；load 在 collect 后调用 first-wins 去重。该层不自动发现默认路径、不作来源权威认证。
- 去重键精确为 event、command_raw.unwrap_or_default、url_raw.unwrap_or_default、configured_matcher.unwrap_or_default、on_failure。忽略名称、类型、source_dir、展开后的命令/URL、enabled、timeout、env、provenance；None 与空字符串相同。不同目录的同名相对命令也可能被折叠，先到的 disabled 项也可能压掉后续 enabled 项；程序构造缺 raw 字段的不同实际命令可碰撞。不能描述为按最终可执行内容去重。
- 文件/目录 NotFound 静默空；其他读取错误记录；目录 dirent 错误记录后继续兄弟项。目录仅直接子项，Unicode 文件名、小写 .json 后缀且长度>5、非点开头、path.is_file；排序后逐个读取。不递归、不限制文件大小，is_file/read_to_string 会跟随符号链接，未做 canonical containment。明确 HookFile 不执行目录文件名过滤。单文件坏数据不影响其他文件。
- 测试覆盖文件顺序、global/project 顺序、快照不随写盘变化、缺失来源、混合来源、错误隔离、同命令去重、first-wins timeout/env、非法on_failure注册拒绝与matcher重编译。名为 all_hooks_covers_every_event_type 的测试仅列9事件，当前目录15，不能声称覆盖全部。gate drift测试硬编码4项，没有直接读取 agent blockingEvents；两端同步仍需跨crate核对。未执行本包动态测试。

## Dispatcher 初段（1–290）

- plan_dispatch 读取 payload.match_value，按传入 event 从registry挑选，不验证 envelope.hook_event_name/event/payload一致性。顺序判定 validate失败→PolicyDisabled、enabledfalse→Disabled、disabled文件名单→PolicyDisabled、matcher拒绝→MatcherMiss，否则clone spec。计划固化后不是逐次重查政策；safe_identity 只含 name/type/provenance，但name用户可控且未限长/脱敏。
- run_one_hook 为公开的已规划执行入口，直接运行，不重新检查 enabled/validate/matcher/名单。raw failure/timeout/cancel 保留各自结果类型；在 Prompt/Tool gate 下 on_failure Block 另生成 Deny effect，不把结果伪装成显式Blocked。默认Allow；Stop失败为空signals。传入 gate 由调用方给定，不自动按spec事件推导。
- Stop outcome 可同时携带 block/force_stop/context；decisive 为 stop_reason或block_reason存在，context本身不decisive。force_stop缺reason补默认；最终聚合优先级后续阅读。
- pre_tool计划空直接Allow；串行执行，prior_block后所有后续项均记PriorBlock，覆盖原计划Skip原因。剩余dispatcher尚未覆盖，不作完整包结论。

## Dispatcher 完整阅读

本阶段补读291–1314，结合前段完成全部生产逻辑及测试。

- Tool/Prompt admission 均串行按计划执行，首个显式 Deny 或 on_failure Block 的失败成为最终决定；余项逐个记录 PriorBlock，不执行，也不保留原先 matcher/disabled skip 原因。空计划 Allow；默认失败开放继续后续项。Prompt helper 没有其他三个dispatcher的dispatch span/count记录。
- StopDispatchResult::absorb 作为公开聚合器可累加多个 block/context，first force-stop 赢且不删除已累积block；wants_continuation 要求没有prevent且block/context至少一个。实际 dispatch_stop 在首个block或force-stop就停止后续执行，因此“后面任何force-stop都覆盖前面block”并不成立；只可能同一结果或外部直接absorb时出现共存。context单独继续后续hook，也请求继续工作。
- stop_detail 展示优先force-stop，再block；仅context结果属于Success。超时/执行失败在Stop gate产出空signals，未把已输出但未成功完成的JSON当决定。错误事件传入dispatch_stop：debug构建assert panic，release记录error返回默认空结果。
- dispatch_non_blocking 的“non-blocking”是无否决语义，执行仍逐项await且等全部完成，不在本层spawn后台任务。UserPromptSubmit特殊委托准入函数后只返回results，decision被此返回类型丢弃；其余非Observe事件仅debug_assert保护，release仍会按Observe gate运行。调用者事件路由必须在其他crate另核对。
- span.enter guard跨await保留，total_duration_ms是各实际运行elapsed之和（as_millis转i64），不含规划/跳过/调度开销，也不是独立wall-clock计时。Failed/TimedOut/Cancelled统一计num_failed；on_failure导致的Deny仍计失败而非num_blocking，后者仅数HookRunResult::Blocked。没有全链共享deadline，耗时预算依赖各runner。
- 测试覆盖规划顺序与skip、两准入事件显式/失败阻断、默认失败开放后继续deny、Stop首block短路/context非decisive/force-stop、Stop200ms超时、子代理matcher、观察失败继续。stop_exit2_fail_open_and_context 名称并不反映断言：exit2在此测试生成block且后续crasher/context都跳过。absorb多信号测试不等于dispatcher执行多个decisive hook。
- 本模块没有做持久事件写入、调用方取消恢复或真实生命周期保证；plan的owned结构只是让调用方有机会先记录Triggered。这里没有运行本包测试，后续完成runner和integration后统一执行。

## Command runner 完整阅读

完整读取 runner/command.rs 1440 行及测试。以下是本隔离工作树事实，不采用另一main已报告修复替代。

- 先完整serialize envelope（没有总输入上限），command缺失或序列化失败返回Failed。shell启发式仅检查空格、|、&、;、>、<、$、开头~；tab/newline、引号、通配符、反引号本身不触发。带空格的实际可执行路径也进入shell解释，不自动quote。Unix使用PATH中的sh -c；非Unix调用config shell argv。非shell路径绝对原样、相对join source_dir，只检查exists，不查文件/可执行权限或canonical边界；spawn错误另返回Failed。
- shell预检用共享变量扫描器，忽略modifier/特殊参数；runner四保留变量、extra包含、process var_os存在、启发式本地赋值存在都视为可解析。空值算存在；process非Unicode在此算存在（与加载时展开不同）。未知列表排序去重后拒绝spawn。局部赋值扫描不是shell parser：不识别quote/执行顺序和作用域，后面的赋值也可消除前面引用警告；read选项仅按token略过，不懂带参数选项。声明处须在起始或往前跳空格tab后为; & | newline ( {，连续A=x B=y的B不自动视为新语句。
- child继承父进程环境；先extra后注入GROW_HOOK_EVENT/NAME/SESSION_ID/WORKSPACE_ROOT，注入值分别来自envelope/spec/ctx，不校验它们之间一致性。current_dir是ctx.workspace_root，不是envelope.cwd或source_dir。调用detach_command，三标准流pipe，kill_on_drop true；这里没有独立sandbox或env白名单。
- 仅ctx.process_scope Some时尝试创建并attach ProcessGroup；失败warning后仍运行。scope已关闭register失败返回Cancelled；创建失败时不会走该拒绝分支。session关闭后实际结果取决于进程退出状态，不保证总是Cancelled。无scope没有group；直接future abort跳过函数后面的显式group.kill，只依靠drop及外部scope生命周期。该分支不能声称取消/超时总能清理孙进程。
- timeout包住并发stdin write_all与child.wait_with_output，序列化/预检/spawn/注册在deadline之外。stdin写错被忽略；等待同时包含输出EOF，后代保持管道可能令正常退出的leader仍等到超时。非成功等待结果后有group才kill，elapsed在kill前采样，不含清理耗时。
- wait_with_output先无限量缓冲stdout/stderr，之后分别前64KiB lossy UTF8并追加标记；不是有界capture，UTF8替换及标记可使返回字符串超过64KiB。GROW_HOOK_DEBUG严格1仅trace字节数；stderr debug也仅长度，但失败/decision原因可能含用户输出或名称，不是普遍脱敏保证。
- Observe只按exit0 Success、其他Failed，不解释JSON。Prompt/Tool对非空stdout尝试严格GateHookJson，反序列化失败直接按退出码：0 Allow、2默认Deny、其他Failed。合法JSON deny在任何退出码都Deny；合法allow除exit2外都Allow（包括非零错误）；未知decision转Failed甚至exit2也不覆盖。故“任意格式错误都失败”或“任意exit2都阻断”均不准确。
- Stop有效JSON在任何退出码优先，允许空对象或approve消除exit2的默认block；转换未知decision Failed。解析失败且trim后以{开头则Failed，不按exit2阻断；其他非JSON按0空outcome、2以trim stderr或默认理由block、其他Failed。截断标记可破坏原JSON并改变以上分支。
- resolve_command_path只检查command Option并拼接，不检查handler_type或shell语法，虽然注释称非command None；程序构造Http且command Some仍返回路径。
- 测试完整覆盖解析阶梯、Stop组合、截断标记、路径、真实超时、大输入不读stdin、env引用/缺失/默认/tilde、scope关闭清理。/dev/tty测试在无控制终端时直接return并打印skipping；scope清理测试固定sleep后验证marker，不能替代abort且无scope的场景。tilde测试只对exit126最多重试8次。部分UNSET命名测试未显式清环境，仍有外部环境前提。未运行本包测试。

## HTTP runner 完整阅读

完整读取 runner/http.rs 886 行及测试。

- URL在运行时以extra/process再次展开，使用spec.url（已可能加载时展开），不是重新展开url_raw；ctx完全未使用，没有进程scope或session取消接口。缺url Failed且无HttpInfo；其他路径通常携带expanded URL/raw URL/可选status/preview。
- 校验仅https，检查明确IP或lookup_host返回的每个地址，任何blocked地址即拒绝，空解析失败。IPv4拒绝10/8、172.16/12、192.168/16、169.254/16、100.64/10和0.0.0.0；允许127/8。IPv6拒绝unspecified、fe80/10、fc00/7，mapped IPv4递归检查，允许::1；未覆盖所有特殊地址段，不能称只允许全球可路由公网。
- host_str先parse IpAddr，否则用host:port做DNS；没有单独的url::Host分支规范化IPv6。源码明确已知请求再次DNS解析边界，校验未绑定到client，也未禁用默认代理。重定向明确Policy::none，每次构造新client，TLS构造失败expect panic。
- 校验timeout与随后reqwest timeout分别使用完整spec.timeout_ms，不是共享总预算；序列化/client构造在这些异步timeout之外。收到响应头时固定elapsed，随后正文读完/失败返回仍用该旧elapsed，未计正文耗时。Observe只看2xx立即返回，不读取正文，preview None。
- 准入与Stop都先response.text读取完整正文，无64KiB硬限制；即使非2xx也先读正文。preview只对trim非空正文取前最多200字节UTF8边界并追加...，不是正文内存上限，也不脱敏响应。serialize请求同样完整分配且无总envelope限制。
- Prompt/Tool空白2xx Allow、非2xx Failed；合法GateHookJson无论status均采用decision（allow也能覆盖500），未知decision Failed。无效JSON在2xx warning后Allow，在非2xx Failed。与command相同共享词汇，不代表状态/退出码阶梯相同。
- Stop首先要求2xx，否则Failed而不采纳body中的block；2xx空白空outcome，合法Stop JSON转换，任何反序列化失败warning后空outcome，包括以{开头的畸形JSON（不同于command Stop）。未知decision在成功反序列化后仍Failed。
- send/body错误without_url后替换log_url（优先raw，否则expanded），降低展开URL泄漏；SSRF错误仍带解析host/IP，HttpInfo.url明确保存完整展开值，raw原文也可含字面密钥。不声称所有日志、错误和元数据无敏感信息。正文超时保留status但elapsed仍为响应头时值。
- 测试覆盖IP分类、非https/坏URL/失败DNS、展开后SSRF、状态/JSON解析、URL错误处理与client禁用redirect。public IP测试仅校验不联网；redirect测试直接client走本地HTTP绕过run_http_hook的https校验，不证明TLS端到端。secret测试接受TimedOut且该分支不执行错误文本断言，不能保证每次实际执行了without_url分支。无IPv6 URL完整成功路径、真实TLS、DNS绑定或总预算回归（另一main报告不计入）。本阶段未运行测试。

## Integration 与示例完整阅读

完整读取 tests/integration.rs 774行、examples/README.md、5个JSON与5个脚本；至此本包Rust及示例读取完成，尚待规格映射和测试终态。

- 13个integration测试走load→dispatcher→command，覆盖退出码deny、默认失败/timeout开放、matcher、观察、first deny、stdin字段、pipe，五种观察事件(PostToolUseFailure/PermissionDenied/PreCompact/PostCompact/StopFailure)实际cat捕获JSON，以及环境注入、direct exec展开、HTTP展开后SSRF拒绝、未知event整文件拒绝。并非15事件全部真实生命周期调用，HTTP也没有成功TLS请求。
- 部分测试只断言最终Allow，默认fail-open下不能独立证明command成功执行；direct_exec展开测试虽检查解析后路径，却未断言Success或外部marker。reserved env集成测试先经过配置剥离，无法单独证明程序构造extra spoof时spawn覆盖；捕获NAME却未断言NAME正确。stdin测试仅grep字段存在，不查值一致。捕获路径插入shell未quote，依赖temp目录无空格。
- safe-shell示例仅matcher Bash，核心matcher不做别名映射，因此不会覆盖canonical run_terminal_command。脚本用grep/sed提取首个紧凑JSON command，不是真JSON解析；转小写后substring blocklist，可能误伤文本/漏过其他拼法，fork模式部分是尾部匹配。未形成通用shell安全边界。
- no-recursive-grep示例用多个shell名称regex、5s，Python解析JSON支持toolInput/tool_input，缺失/坏输入允许，识别recursive时打印Deny JSON但exit0（注释声称exit2和stderr，实际不是）。depth>=5直接允许；词法器无backslash完整语义，遇#直接终止剩余整个字符串而非仅当前行，wrapper扫描和shell后续参数拼接是启发式。
- 该Python把任一部分quoted的token当非flag；实际shell会去引号后把grep "-r"传成-r，因此SELF_TEST_CASES中把它期待为False并不证明无递归。不解析全部grep选项参数，wrapper跳过参数也可能误判。README关于所有递归grep必然把整目录读进内存/OOM的描述不是本代码可证明的事实，只记录为示例作者理由，不提升为系统契约。
- session-log/tool-logger用HOME/.grow固定路径追加，shell字符串拼JSON无转义/并发锁/轮转；timestamp是执行时date UTC，不是输入timestamp。字段提取依赖紧凑JSON，tool background缺省false。tool logger注册pre+post，其中pre仍经Tool gate但脚本不输出decision、exit0允许，不代表事件本身Observe。
- stop-verify只当提取reason=end_turn时运行cargo build --quiet并丢弃输出，失败输出block，成功/其他reason退出0，timeout300s。脚本自身不实现8轮上限、不使用stopHookActive；README的上限及/hooks-trust等需shell调用方另核对。README统一默认5秒遗漏Stop默认600秒，退出码/JSON描述也未列精确优先级，不作为事实权威。
- Python --self-test 本轮43/43通过，仅其自带期望集；Rust串行nocapture测试已启动，日志 /tmp/grow-hooks-inventory-tests.log，终态待记录。

## 已完成能力映射

## 功能与规范映射

- [Hook event catalog](../specs/hook-execution/spec.md#requirement-hook-event-catalog)：Hooks SHALL 提供15个精确snake_case事件；Prompt/Tool gate分别为user_prompt_submit/pre_tool_use，Stop gate为stop/subagent_stop，其他为Observe；stop、prompt、stop_cancelled忽略配置matcher。
- [Hook envelope and payload selection](../specs/hook-execution/spec.md#requirement-hook-envelope-and-payload-selection)：HookEventEnvelope SHALL 以camelCase序列化公共字段并flatten载荷，None可选字段省略；事件名与payload由调用方分别提供。
- [Hook matching values](../specs/hook-execution/spec.md#requirement-hook-matching-values)：HookPayload SHALL 按工具名、通知type、子代理type、来源、结束reason或失败类别提供matcher值；无值事件及空值允许匹配。
- [Hook payload clipping](../specs/hook-execution/spec.md#requirement-hook-payload-clipping)：载荷裁剪 SHALL 显式执行；truncate_payload超过128KiB时把序列化前缀改为带标记JSON字符串，clip_text按Unicode字符截取后追加计数。
- [Hook configuration parse boundaries](../specs/hook-execution/spec.md#requirement-hook-configuration-parse-boundaries)：JSON hook文件 SHALL 恰有hooks一个顶层键；事件/group/handler结构严格解析，合法结构内的坏matcher跳group、坏handler单独跳过并收集错误。
- [Hook handler defaults and failure policy](../specs/hook-execution/spec.md#requirement-hook-handler-defaults-and-failure-policy)：command/http handler SHALL 分别要求command/url；超时秒饱和转换为毫秒，Stop默认600秒、其他5秒；仅两个准入事件允许显式on_failure。
- [Hook load time environment](../specs/hook-execution/spec.md#requirement-hook-load-time-environment)：配置加载 SHALL 保留raw命令和URL，剥离四个runner保留env键，再以extra优先执行单次环境展开；env缺失/null为空map，值原样保存。
- [Hook environment expansion scanner](../specs/hook-execution/spec.md#requirement-hook-environment-expansion-scanner)：环境展开 SHALL 用随机sentinel保护识别的modifier引用并恢复，extra优先于process；扫描器不是完整shell语法解析器。
- [Hook source collection](../specs/hook-execution/spec.md#requirement-hook-source-collection)：Hook发现 SHALL 按global来源顺序先于project，目录内直接JSON文件按路径排序；缺失来源为空，其他读取错误收集后继续可读兄弟文件。
- [Hook registry deduplication](../specs/hook-execution/spec.md#requirement-hook-registry-deduplication)：注册表去重 SHALL first-wins，键为event、raw command、raw URL、configured matcher、on_failure，Option空与空字符串等价。
- [Hook registry restore and mutation](../specs/hook-execution/spec.md#requirement-hook-registry-restore-and-mutation)：HookRegistry SHALL 提供稳定事件顺序展平、追加、前缀删除与显式matcher重编译；磁盘定义是加载快照，禁用名单查询仍动态读取。
- [Hook disabled name persistence](../specs/hook-execution/spec.md#requirement-hook-disabled-name-persistence)：禁用名单 SHALL 按用户Grow home下disabled-hooks的trim非空非注释行匹配名称；查询读取错误视为未禁用。
- [Hook owned dispatch plan](../specs/hook-execution/spec.md#requirement-hook-owned-dispatch-plan)：规划 SHALL 按registry顺序检查validate、enabled、名单、matcher并生成owned执行或skip项，identity包含name/type/provenance。
- [Hook admission chain](../specs/hook-execution/spec.md#requirement-hook-admission-chain)：Prompt和Tool dispatch SHALL 串行运行，首个显式Deny或on_failure Block失败终止执行后续项；失败原始结果类型保留。
- [Hook stop signal chain](../specs/hook-execution/spec.md#requirement-hook-stop-signal-chain)：Stop dispatch SHALL 在首个block或force-stop后记录余项PriorBlock；context独自不短路但请求继续；absorb保留首个force-stop及累积信号。
- [Hook observe dispatch and statistics](../specs/hook-execution/spec.md#requirement-hook-observe-dispatch-and-statistics)：Observe dispatch SHALL 顺序await所有可执行项，不产生否决；统计按结果类型和elapsed求和。
- [Hook command selection and spawn](../specs/hook-execution/spec.md#requirement-hook-command-selection-and-spawn)：命令runner SHALL 以空格或指定shell元字符/开头~选择shell，否则相对source_dir直接执行；子进程cwd为ctx.workspace_root，extra后注入四个身份环境键。
- [Hook shell variable preflight](../specs/hook-execution/spec.md#requirement-hook-shell-variable-preflight)：shell命令 SHALL 在spawn前拒绝扫描发现的未设置普通变量；runner键、extra、process var_os、本地赋值启发式及modifier免检。
- [Hook command wait and process scope](../specs/hook-execution/spec.md#requirement-hook-command-wait-and-process-scope)：命令等待 SHALL 并发写stdin与wait_with_output，写错忽略，等待受单handler超时限制；仅存在scope时尝试创建注册进程组。
- [Hook command output capture boundary](../specs/hook-execution/spec.md#requirement-hook-command-output-capture-boundary)：命令结果 SHALL 在完整wait_with_output后分别截取stdout/stderr前64KiB并lossy解码；Observe只检查exit0。
- [Hook command admission output precedence](../specs/hook-execution/spec.md#requirement-hook-command-admission-output-precedence)：命令准入解析 SHALL 对合法JSON deny始终Deny，allow除exit2外Allow；JSON结构解析失败回退退出码0允许、2拒绝、其他失败。
- [Hook command stop output precedence](../specs/hook-execution/spec.md#requirement-hook-command-stop-output-precedence)：命令Stop解析 SHALL 优先采用合法Stop JSON；以左花括号开头的坏JSON失败，其他无有效JSON输出按退出码决定。
- [Hook HTTP URL admission](../specs/hook-execution/spec.md#requirement-hook-http-url-admission)：HTTP runner SHALL 在运行时再次展开URL，只允许https，拒绝代码列举的私网/link-local/CGNAT/unspecified地址，允许loopback；所有解析地址需通过。
- [Hook HTTP request and timing](../specs/hook-execution/spec.md#requirement-hook-http-request-and-timing)：HTTP SHALL POST完整JSON envelope，独立为校验和reqwest请求设置完整handler timeout；Observe按状态返回不读正文。
- [Hook HTTP decision and metadata](../specs/hook-execution/spec.md#requirement-hook-http-decision-and-metadata)：HTTP Prompt/Tool SHALL 对有效decision JSON忽略status，坏JSON在2xx允许；Stop非2xx失败，2xx坏JSON为空outcome。
- [Hook executable examples](../specs/hook-execution/spec.md#requirement-hook-executable-examples)：本包 SHALL 提供安全shell、递归grep限制、session/tool日志和cargo build Stop示例；示例不是完整安全策略或调用方生命周期实现。

## 边界

- 拒绝该事件键；完整事件及载荷目录见reviews/hooks.md，类型目录不证明调用方已触发每个事件。
- 类型自身不拒绝配对不一致，空列表保留，不能假设序列化执行业务验证。
- matcher_allows仍返回true；普通regex不加锚点且没有工具别名转换。
- 原类型可能改变，完整序列化先分配，标记和转义可能使最终值超过阈值；不是整个envelope硬上限。
- 前者拒绝整个map，后者只拒绝对应handler；其他配置层或文件可继续加载。
- 前者拒绝，后者接受；HookSpec.validate只拒绝非准入Block，不执行全面字段验证。
- 缺失引用保留，替换值不递归展开；matcher不展开，raw原文不保证没有字面密钥。
- 不保证shell语义完全保留；裸变量和braced变量识别规则不同，嵌套只扫描首个闭括号，详细边界保留在审阅记录。
- 明确文件不受目录文件名过滤；目录不递归但is_file及读取跟随symlink，没有独立大小或containment保证。
- 仍可能折叠，忽略实际展开命令、enabled和handler_type；append_specs自身不去重。
- compiled matcher缺省None；显式重编译非法模式装Never，但缺值匹配边界仍存在，Ignored事件也未被重编译函数排除。
- 没有锁或原子更新保证；disable查重追加，enable截断重写，名称未限制换行。
- 已clone计划不重新查询；run_one_hook直接执行已规划spec，不重查政策，identity名称仍用户可控。
- 最终Deny，首项仍Failed/TimedOut/Cancelled，后项统一PriorBlock；默认失败Allow可继续。
- 后项不执行；不能声称后面的force-stop总覆盖前面的block，wants_continuation取决于已吸收信号。
- 特殊委托准入链但只返回results；non_blocking不是后台执行承诺，失败导致的政策Deny仍计failed。
- 前者不因此进入shell，后者进入且不自动quote；直接路径只检查exists，runner未实现独立sandbox。
- 启发式不检查执行顺序/完整quote语法，不能作为shell静态分析保证。
- 不能保证孙进程组清理；序列化/spawn在timeout之外，scope已关闭且注册失败返回Cancelled。
- 收集内存仍不受该阈值限制，截断标记可能破坏JSON且最终文本略超限。
- 当前实现前者Allow，后者Failed；不以主分支未整合修复替代当前事实。
- 用trim stderr或默认理由block；合法JSON可覆盖exit2，未知decision失败。
- 当前client未绑定已校验地址、未no_proxy；禁用重定向但不能声称消除DNS rebinding，具体地址表见审阅记录。
- 完整response.text无容量上限，elapsed固定在响应头阶段；没有共享总预算，正文超时可保留status。
- 当前前者Allow，后者空Stop；HttpInfo保存完整expanded URL和最多200字节边界预览加标记，raw/error替换不是全面脱敏。
- 自测43项符合自身期望但quoted flag/depth/comment有识别缺口；Stop仅end_turn运行build，300秒配置，自身不实现8轮上限。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
