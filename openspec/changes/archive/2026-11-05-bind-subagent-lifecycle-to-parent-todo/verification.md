# Incremental verification

## 最终特性与调用链验收

本change的实现、隔离调用链和本地归档已验收，工具/子Agent/开发说明及OpenSpec索引已同步，三份主规范已合入。归档前全量strict为19/19，归档后为18/18；全部归档校验最终859/859、exit0。首次归档校验858通过/1失败，唯一原因为最后的“归档后校验”任务尚未回填（24/25）；确认实际归档与规范结果后勾选该项，再跑全部归档通过，未忽略失败。archive提示delta较多为非阻塞警告，变更保留按owner验收的局部增量。

下列增量记录保留当时的失败、修订与局部验收边界；其中“仍待后续”等字样描述开发时点，当前结论以本节为准。原始运行材料留在临时目录，不复制provider请求或构建日志进仓库；本记录保留结论、环境、指纹及可重跑入口。

最终默认CLI构建与两条真实原生调用链都使用Rust/Cargo/rustdoc 1.93.1、offline/locked、RUST_MIN_STACK=16777216、CARGO_BUILD_JOBS=2、测试线程1、默认workspace target。GROW_HOME/TMPDIR/XDG与空Gitconfig/hooks/template均在独立`/tmp`；provider仅loopback MockInferenceServer，不运行真实provider/MCP/hooks，不改变正常Grow存储。最终binary SHA256 `0cb021c4e645fc662464f5b679ca738e3e3733c3632464b89f9c27c3d3bce7d8`，不是仍含旧prompt append的a5产物；未安装、重启、提交或发布。

| 验收边界 | 最终证据 |
| --- | --- |
| current TODO绑定、Link准入和fresh/resume首请求 | 默认CLI build实际exit0；`test_subagent_todo_context::native_todo_spawn_and_resume_first_requests_use_current_request_only_parent_binding`显式GROW_BINARY、`--ignored --exact --nocapture --test-threads=1`实际1pass/0fail/0ignore、exit0。fresh Plan53/Spawn54、A位置17/18、runtime1977 bytes；resume Plan119/Spawn120、当前B位置19/19、runtime1921 bytes。原task prompt逐字节保留，真实工具别名/schema和Seed/Link一致、sourceA不变、正常Result/Ended、request-only持久隔离均通过。138项dirty/untracked Rust与manifest/lock前后相同。 |
| 完整TODO→后台required spawn→read/ask→edit/remove→收尾→review | `test_subagent_todo_control::native_todo_edit_remove_controls_background_child_through_normal_result_and_review`同binary实际1pass/0fail/0ignore、exit0，测试9.56s。Link53先于Spawn56；child实际typed edit Received62/Consumed68、stop Received134/Consumed140，正文1008/1056 UTF-8 bytes；父级原生读分别观察Updated/Stopping。Stopping时link仍非terminal、没有Result和Accepted，完成的investigation保留，仅受影响项取消。 |
| 正常结果、验收与冷投影 | child `01a1ab27-cdd6-7ae0-8e32-6b9d75280c34`正常Completed Result seq244，父Ended seq219精确引用该child事件；output artifact BLAKE3 `d6e83098ff35f7d41f5474fe255aea6c8ce47e28566e3c085f9ee72e983ea58e`与真实报告核对。原生wait取同报告，父从真实review_binding复制resultId后以source revision3接受，冷fold保持一致。控制只在普通展示隐藏、内部两份typed事实与消费仍保留。正常session关闭后writer被reap；transport143是harness物理退出，与canonical Completed分别验证。 |
| 来源、消费和故障前缀 | 2.6a/c/d.1的exact typed identity、ordinary同ID/同正文反例、真实命令/等待、两durable提交与cold/lostACK证据见各节。2.6d.2主体17项及队列隔离新反例red→green后13项新组+未改旧5形成18个唯一用例，所有实际命令exit0。队列首项未确认不妨碍后续合法ACK/直属child控制；typed消费证明、不可变stop和结果/review责任未放宽。 |
| 读取、预算与跨层授权 | 2.2/2.3实际公共API与native注册回归、直属nested路由、每页重新授权/seq cursor/256KiB、8KiB最终escaped预算和超时alias修复均已独立验收。最终CLI两场景贯通真实Tool→Coordinator→Shell→ChatState及请求/展示投影；nested/multi-child/重开等边界由各owner专项回归验证，未声称完整产品场景穷举这些组合。 |

父亲自读两条最终完整日志及结构化wire/canonical证据，重新核source/binary/log指纹；首请求日志SHA256 `a1b6c55d6f4bc80b5f3d493f446fe3cbef72dd72b0feb64335a1cb238906adec`（`/tmp/grow-todo-final-cli-verify.Axsm5J/`），控制链成功日志SHA256 `8e2c2fcbefc155bb9cbab8c736e8eb91d095bf4b086d38a5d25a5c15ec163dc4`（`/tmp/grow-todo-control-actual-u_lm9si9/`）。最终control test源码SHA256 `ac7fea8e817194686f898af3d33198e4d0fb6203a917833e5acb80b083fe6044`；相对Luna审过的1c88只补现有registry的`background`参数重命名和TruncationConfig等待schema投影，父核真实Agent配置与registry/public helper、精确两hunk并确认全部原断言保留。前两轮各编译后0pass/1fail、exit101，分别为上述两个fixture差异，失败日志保留；没有生产修正或额外CLI重建。

各模块Luna独立review和父级跨边界验收见以下记录。最终queue隔离由fresh Luna `01a1ab21-0842-74f3-93d9-5a2cde5ce434`复核最终twofile及cold-turn修正，原队首阻塞闭合，无新可行动问题；它未重跑测试且未重算hash，父另核实际red/green与指纹。产品场景Luna `01a1ab1d-2204-7422-82cf-8b8864810b66`静态review无可确认finding，最终两处fixture投影由父复核。旧resume reviewer因宿主unconsumed-skill-load compaction校验失败且无结论，未当作通过，改用fresh审查，不修改运行中宿主。

验证未覆盖Windows/Linux实际执行、真实远端provider或非合作模型的一切后续工具调用。固定stop依照既有soft boundary收尾，不是新的工具权限撤销或命令强杀。旧宿主子任务`01a1a77a-72ff-7692-90cb-dbbcfd304e7e`的finalization_pending/Receipt Unknown仍由总体计划保留；本change的通过不证明该旧实例已恢复，整体backlog和Goal也未完成。

## 增量过程记录

第一片ChatState runtime_context七文件增量完成并获父局部验收：runtime_context仅加入请求副本，预算/pressure/source projection计入其成本，Timeline/Surface不新增，memory仍单独持久一次。父已亲核production diff、native/Goal/image/tool pairing及Shell实际build_turn_request。独立luna `01a1a9c9-2d1b-7993-8b1e-aabf12528477` 的只读review未发现该片阻塞问题；其审查本身不构成运行证据。后续执行交付 `01a1a9dd-46ea-74e0-85ce-1dc20772d7eb` 的六项定向测试已由父读回真实结果，每项exact唯一枚举、1 passed/0 failed/0 ignored：

| Package | Test |
| --- | --- |
| chat-state | actor::tests::build_request_runtime_context_is_fresh_and_request_only |
| chat-state | actor::tests::build_request_runtime_context_keeps_actual_memory_durable_once |
| chat-state | actor::tests::build_request_runtime_context_pressure_survives_surface_refresh |
| chat-state | actor::tests::runtime_context_preserves_native_spans_goal_projection_and_tool_pairs |
| chat-state | actor::tests::image_budget_selection_survives_append_until_next_high_water_mark |
| shell | session::actor::turn::runtime_context_tests::turn_request_keeps_local_planning_out_of_stored_messages |

构建命令为`cargo +1.93.1 test --locked -p <package> --lib --no-run --message-format=json`，从产物以完整名`--exact`枚举/执行。固定Cargo/rustc/rustdoc1.93.1，RUST_MIN_STACK16777216，JOBS2，独立/tmp GROW_HOME/TMPDIR/空Gitconfig和hooks目录。七文件限定rustfmt与diffcheck通过。压力fixture按context.trim()预期、Shell先将共享零provider anchor对齐空context成本，保留原存储与精确成本断言，不放宽产品语义。

验证时request_builder SHA256 `9bb6ea2a5677e0b5212dbc8ca3c8269c4d61ea817f20e0fc0bc5b98a6cbec3fb`；最终Shell turn `27ff31025870a485a5688a95b6ef19bfa9325ec5c97ed399561f0be90eb90ca1`；验证版本hash清单SHA256 `62c62561721a1c783258da203e15846dc0f27bff96d24c11407930b750ac9b9f`。ChatState commands/handle/actor随后已交`diagnose-lost-primary-session-owner` owner写入，六项证据绑定本片测试版本，不能据此宣称后续恢复改动通过。原始运行材料暂存/tmp/grow-runtime-context-validation.uzno7u，实际通过以本记录结论与最终源码为准。


独立发现：现Shell `actor/planning.rs::local_planning_context` 每步截256字符/前12项，无JSON总字节上限；11条256个三字节CJK正文即8448字节，未计身份/link/replan。父核对源码确认，不属于冻结七文件引入，却不满足本change的8 KiB明确设计。登记为独立renderer修正task2.4a，按同文件写ownership在父项上下文/工具名renderer接口确定后实施，不混入ChatState核心或假称已解决。

完整TODO→spawn/read→edit/remove→child收尾集成仍在后续，未勾选相关tasks。上述局部验收不覆盖renderer总量、强制绑定、父计划查询或控制投递。

## 每请求直属父项上下文（2.4a，已验收）

本片实现与定向运行已完成。父亲自核对renderer的最终UTF-8/JSON escaping预算、关联项优先与完整binding省略，读local_planning_context整个观察期限和出生producer实际接线；`ParentPlanBinding`来自当前durable Spawn的security_parent/task/step，StartupHints字段serde(skip)，cold请求改变原step会在恢复子存储/provider前拒绝。既有lifecycle-root parent字段不参与direct读路由。每请求从canonical self和session-bound父owner读取，无快照cache或新持久关系；父读取失联显示实际错误并保留binding，自计划owner错误仍Err。真实工具alias经现TemplateRenderer解析，无入口不编造fallback调用。

父预审发现名称/Resources mutex等待不在backend五秒内，原owner修为同一绝对5秒覆盖名称解析、backendclone和父查询；新增持Resources的parent/self-only实际actor回归。token/dropguard仅结束本次观察，不取消session或child。最终renderer8192字节包含固定指引/JSON转义，完整保留必需身份，优先第4096关联tombstone及stop责任，可选review结果引用过大整体省略，不截断resultId或捏null。

固定Rust/Cargo/rustdoc1.93.1、MINSTACK16777216、JOBS2、testthreads2，独立/tmp Grow/TMP、空Git配置/hooks。首编译`/tmp/grow-task2.4a-verify.1bflTf/test-1.log`实际exit101，只有新coordinator测试越界访问私有actor.step_control_gate的两个E0616，0项运行。修正仅两个新增测试：持gate断言移到本来有访问权的actor/turn模块，生产/helper可见性未改变。最终v2主组`/tmp/grow-task2.4a-verify-v2.g1Tc80/test-1.log`至test-6.log全部实际exit0，父逐份读完整枚举：pure formatter12、actor/backend8、build_turn_request2、producer2、真实nested coordinator→生产Shell reader→ChatState1、旧runtime_context1，共26个不同测试。test-7原review过滤器选0项，明确不算通过；纠正为`session::actor::tests::planning_dispatch_tests::review_binding_tests`，`/tmp/grow-task2.4a-review-regression.ks6KHa/review-binding.log`实际3pass。合计29唯一、0failed、0ignored，无删失败场景或弱化断言。

覆盖64 KiB正文、4096 steps、CJK/emoji/转义与最大合法身份、长review binding、末项位置/stop、resolved alias/缺入口、五秒backend与mutex、drop取消隔离、gate前查询、cold canonical identity与实际restore handler mismatch、首/后请求刷新/预算/Timeline与Surface和ACP无新增；真实coordinator受控started ACK验证nested direct边、刷新及失联不复用旧事实。父重算`/tmp/grow-task2.4a-01a1aa45-source-freeze-v2.sha256`12/12全部OK，production关键SHA为planning.rs `99fd119cedba699c08bf782475940b64e72dbeaceeb64f9a5155c32dad659e46`、acp_types.rs `5e5a1dedd023253055bdd3b6efa4d51d093932ad7ed04ee337f0870a8dedf367`、handle_request.rs `41d492985e9d6ffc531f689c07af511e7fbb36c600b7d2c66eb972a9aec81bfb`。限定rustfmt/diff检查通过，所有命令真实退出，source/Cargo已释放。

独立Luna `01a1aa70-d25a-7083-94bf-040335027bd9`完成最终v2审查，发现一个本片阻塞：名称已解析成功后，后续parent/backend观察超时被整体timeout分支清掉名字，误报read_entry unavailable；显式TimedOut返回分支同样丢弃name。父确认应保留已解析真实alias，只有名称尚未解析或工具未注册才标入口不可用；共同5秒、错误与binding不变。该发现单独交回原owner，父当时将2.4a标记待修订、保持任务未勾；候选29通过不能覆盖这个缺失分支。修复及新回归已按下节最终hash重新验收。

29项含实际owner和受控运行ACK，尚非真实provider spawn→首次request里程碑；该链和mandatory准入仍待2.4。TaskTool旧prompt拼接保留到链路打通后移除，本片未改准入或通知状态机，不把sharedstop方法复用当作2.6完成。

### 独立review单项修正与最终验收

原owner `01a1aa7f-2631-7fc2-9cc6-7551a741eb57`只修local_planning_context及request_context_tests：把已解析名称保留为本次调用局部值，explicit backend TimedOut与总deadline超时都不再清空alias，仍共用原绝对5秒，名称未解析的持Resources路径仍unavailable。没有新持久状态、缓存、锁或公共接口。

先保留v2生产代码加入两项实际actor/TemplateRenderer alias回归，`/tmp/grow-task2.4a-alias-revision.NTXiOt/red-alias.log`真实exit101，0 passed/2 failed，两项均在read_entry availability unavailable!=available断言失败；父读回这两个失败和终态。修正后相同两新回归及原actor8共10、真实build_turn_request2、真实nestedconsumer1共13 passed/0 failed/0 ignored，green-1.log至green-3.log完整枚举父已读。后台task01a1aa85-1632-7580-91c6-da39be8fd11d由父本人get_command_output确认completed/exit0/77.14s，前后12项hash全OK；不是只依据日志尾或观察超时宣称退出。

父审相对精确v2副本的planning.patch和request_context_tests.patch，确认两文件限域修正和真实typed观察/五秒/无写副作用/取消不误伤断言。再次check source-freeze.sha256 12/12通过；最终planning.rs SHA256 `7faf2c2fc00cabb2c2d3a38176a34b36c4c566effd21966f7b8e498f55593a16`，request_context_tests.rs `d5ac32f1163b5187572d2c72deb152e9a5d172f90446060a926a993dc5dff0ee`，其余十个v2文件不变。固定隔离环境、限定格式/parser/diff检查通过，两个文件和Cargo窗口实际释放。未变formatter/producer/旧review保留先前29项证据，不无理由重复全套；新增两项回归封闭唯一review缺口。父接受该修正及2.4a增量，纯provider首请求与mandatory/control整体里程碑仍按上段保持开放。

## 单个规划控制接纳（2.6d.1，已验收）

source owner `01a1aac6-0999-70b0-b392-c41fa3eb687c`已实现并冻结六文件，唯一Cargo及写scope实际释放。父读相对精确dirty前副本的`/tmp/grow-plan-control-2.6d.1.4aU25S/dirty-baseline.patch`及关键测试：只改receive_plan_guidance/私有模块、typed-vs-ordinary receipt域隔离和两处旧fixture；完整责任仍归LocalPlan，同step gate先核本次host binding/immutable payload、durable Received与hydrate、typed receipt，最后soft signal。完整责任+exact receipt在closed状态只读确认；缺receipt的closed前缀拒补但保留责任。固定design前缀和最终16 KiB escaped JSON预算不受delivery progress、当前step或alias变化影响。

两处旧planning_dispatch测试有意适配新顺序：plan ACK丢失cold只有完整Received责任、没有通知，retry补一份typed receipt；补真实本次host binding与父Dispatching claim。父核其原唯一责任、消费一次、cold replay、dispatch retry和后续处理仍保留。新manual两阶段ACK直接要求首Planning、后Notification，在各ACK前核回复/soft signal均不放行；真实blob读回正文，接纳失败无blob或通知，receipt失败/中断/cold重试不丢完整前缀。真实terminal命令测试证明stop只唤wait，命令仍活跃，随后正常exit0并产生唯一结果；既有question、descendant/result/review责任保持。

固定Cargo/rustc/rustdoc1.93.1、offline/locked、默认target、MINSTACK16777216/JOBS2/testthreads2及独立/tmp Grow/TMP/空Gitconfig/hooks/template。父逐份读回`/tmp/grow-control-2.6d.1-verify.JdeQjW/`八份日志：formatter4、receive-control13、typed/ordinary domain2、两旧planning exact2、ordinary exact/stopped2，共23个不同测试全部passed/0failed/0ignored，owner报告八命令均actualexit0。首formatter真实exit101为测试全文搜索progress误中immutable状态in_progress，改JSON key缺失检查，原跨进度全文字节相等断言保留，生产formatter未改；原日志`/tmp/grow-control-2.6d.1-verify.6MHYZk/formatter.log`保留。

父再次`shasum -a 256 -c source.sha256`六项全OK。precise patch SHA256 `c672dd486c5909430e9e7e08efe97f4f7bc2687afc70650526c2afc9581872bf`，manifest SHA256 `c609d07dc41c9825187bc3d6941611da054e21e5051d26bb3e3b35dc91ba7b3b`，全部成功日志manifest SHA256 `d68a7278dbe008b578e81bd489b93003879b9ed4a31c36a34034b3eddd2dde8f`。最终planning.rs `37a92015fbe1e0901528c1c4abad0024ccbdee8ea293dd05fc471b7d86fc35eb`、control.rs `9785fa1736ec33c15f9d9f588246ea085f006b7243187d7e4f8763932ed8a3e9`、inbox `9b0a8dff5df35c73a281f4d4f85e8a406806aded6939beb9db5d1352b63f2f8b`。独立Luna `01a1aadf-968f-7f60-b6b9-10b28a953136`审相同最终增量，亲核precise patch和manifest hash、六源文件，未发现可行动问题；它只做静态审查，23项运行证据由父另核，全部八份日志hash父再次验证一致。父接受该独立review及2.6d.1模块。HandleReplans/exact消费和Stopping ACK仍属2.6d.2，未混入本片。

### 首请求集成候选

source owner `01a1aac7-1838-7702-85cf-c054deab0bac`已冻结并释放新test_subagent_todo_context.rs，SHA256 `bdd6fd2f2b340cde38fc09723f4509a2fb01281d292300fb3288ccb95bcbe3d1`由父重算一致。父已预读整个真实CLI测试：native TODO→fresh/resume first wire request，独立loopback endpoint、全部响应前屏障、当次binding/seq/alias与runtime-only持久分离、正常Result/Ended和source不改。独立Luna `01a1aad6-5da6-78c0-b200-55397cf67577`审此单文件验收入口。接纳owner释放后父取得唯一Cargo，背景`01a1aae0-92a0-7d20-97c0-a6f60b48d123`实际结束：默认`cargo build --offline --locked -p cli --bin grow`exit0；integration首编译exit101/0tests，唯一E0432为test的PlanReadInput错误从crate根导入，实际导出在tool_types::planning。完整build/first-request.log与results.json在`/tmp/grow-todo-first-request-verify-h9y9owtc/`，当前binary SHA256 `a5a180aa8196840048ff7adbc30c00e39a7af73e4580dfc1c146efa572b39dcf`；父已读回诊断与actualexit。源码前后七项未变，该编译失败不代表provider行为失败。父释放窗口给原test owner的resume `01a1aae5-9455-7301-9373-4fbdbff061f0`，仅修该新测试及运行同一真实目标，不需重建未变生产binary。

独立Luna01aad6的有界静态结论未发现问题、0run，未发现该import编译错误，不能拿静态结论替代运行证据。父随后亲核`attempt-01-from-bdd6.patch`只有该导入一行、test与binary最终hash；原全部验收断言未改。`/tmp/grow-todo-context-run-vyhvnfcu/attempt-01.log/json`记录固定1.93.1/offline/locked、MINSTACK16777216/JOBS2、singlethread、explicit current GROW_BINARY的唯一ignored exact测试，实际1passed/0failed/0ignored/0filtered、exit0，后台`01a1aae9-1d4e-7c93-bc2c-ae0e0491b4f7`completed，测试含close及writer已reaped断言，结束build/test进程为空。

父亲自从完整日志解析两份FIRST wire证据并读回`parent-review-evidence.json`：fresh实际parent Plan seq53/Spawn54、current revision1、stepA位置17/18、runtime1977 UTF-8bytes；resume在sourceA正常Result后用同scope revision2 Plan seq119/Spawn120，当前stepB位置19/19、runtime1921bytes。两次真实工具alias来自各请求工具schema并与native PlanReadInput/description相符，current parent/task/child/step、内容/requirements与各自真实Planning/Link/Spawn/Seed一致。旧sourceA字节未变，两新child均正常Completed Result与父Ended.result_ref准确、link终态；新runtime GUIDANCE/结构未进Timeline/Surface/MemoryContext。旧prompt内JSON仍存在，父明确只用新块作为证据。

最终test SHA256 `c622ecb188d4d54c24310930219a9672c96df563e12d2fa68419e8057831672c`、binary `a5a180aa8196840048ff7adbc30c00e39a7af73e4580dfc1c146efa572b39dcf`父重新计算匹配。source-before/after七项一致，唯一导入修正的编译行为已实际验证；父接受此首请求里程碑。随后由独立tools owner `01a1aaef-cc1c-7d12-bc77-6ee04be0473c`移除旧append并加authored prompt不变的真实断言，ACK owner01aaee同时单独负责HandleReplans，双方source-only不交叉；最终新binary重跑后才勾2.4c，不能沿用仍含append的a5产物冒充最终验证。

## 处理控制与最终调用链（实施中，未验收）

SessionActor owner `01a1aaee-0c68-7f52-bd57-0685547e9571`负责2.6d.2，源码已实现并进入定向验证：全部HandleReplans在首次Planning提交前核current host binding与exact typed version2消费，普通保持Updated/NoChange，stop写Stopping且拒no-change，向父ACK选择同typed receipt。parent保逐阶段durable ACK，真实执行、命令、ask、wait、结果和review不因处理指令而完成。它还拥有planning_dispatch_tests唯一写范围，同步去append对原Link/edit交换顺序测试的必要consumer断言，保实际gate/Link/新要求与pending replan责任。

父已亲读生产精准patch、消费/stop混合全拒、current binding、manual durable ACK/cold prefix与parent责任回归。新组12与旧命令owner/Received cold/linked dispatch/Link-edit consumer/parent Handled cold各1，共17个唯一用例已实际通过，六条命令均exit0。父读回全部六份完整日志、核11源与六log指纹全匹配。最终主片planning SHA256 `271e97e90fe993472220ecf64daf0a96ff51b1fef139ed102275559e485f2658`，新tests `b27f64205122d11dac70c1f914ee56f865347c6641d89a5fb3b9e5dc3975d896`，dispatch tests `2710fedf3665696f6fdccd29574de358de72f17c753d1994aecacb091b2f8088`；精确dirty补丁`source-final.diff`及verified-results在`/tmp/grow-control-2.6d.2-verify.woyt4U/`。

失败证据保留：attempt-1编译exit101/0tests为新producer测试E0502，改owned Box::pin；attempt-2编译通过但第12项fixture单poll后等待channel导致自身驱动停滞，取消且不计部分11项通过；attempt-3新12+旧3通过，consumer失败因父错误要求最终review revision为0。父亲核shared update_steps后，以首个含link的真实Planning核Link入场0/1、最终binding均随编辑为1，attempt-4定向consumer和最后cold均通过。原fair gate两顺序、原prompt、lateststep、source/replan/pending断言完整保留，修正均限tests。

独立Luna `01a1ab01-0955-73f1-ba79-3173ed3ba69c`完成同三文件v3静态review，发现投递循环对单条Handled exact消费helper使用`?`，前条历史ordinary-only/typed未消费责任会让后续合法Handled ACK或下游Pending永远不能触达。父亲核for与retry/backoff，可达且影响独立控制进度；17项通过不覆盖该有序多记录反例。父已needs_revision主片，接纳review finding；原owner释放后续派 `01a1ab0d-1505-7be3-8a1a-b10e36fa4021`单目标2.6d.2a，限定producer逐条隔离与同模块回归，先未修生产取得有界red，再green+原新组，禁补造receipt/虚假ACK/清责任。修复与Luna复核尚未返回，2.6d.2未验收。

去append owner `01a1aaef-cc1c-7d12-bc77-6ee04be0473c`已冻结tools TaskTool及首请求test两文件：生产删除prompt复制/step JSON拼接、直接移动input.prompt，现无条件Link ACK与错误/undispatched路径保持；现八组合原测试加空白/CRLF/引号/UTF-8 prompt逐字节相等，真实首请求test加captured Spawn.prompt==TASK_PROMPT，原新runtime与全部身份/结果断言保留。父已亲读精准`/tmp/grow-task-prompt-24c-G8gMY7/{task-mod,test-subagent-todo-context}.delta.diff`。SHA256为tools `23b34818b3ce6c0cae3be84cf6c20656d0720c615116acdb17c6a437cf68d437`、test `8c2617bd224e499d4f80397a770e8993710b07a0c05debc0e40adea4aaa94beb`；当前只有parser/fmt/diff级证据。父核tools和test-support依赖不含Shell，交后继owner `01a1aaf7-4355-7931-9b6c-69c0bc90406a`唯一Cargo，仅tools原TaskTool68项定向测试在Shell写源期间独立执行。`/tmp/grow-task-prompt-24c-tools-1igs4xuy/cargo.log`及attempt02.json实际68passed/0failed/0ignored、exit0，父读完整枚举及隔离环境、重算最终tools/test/lock和log指纹全部匹配；无源码修正。首次启动选到macOS系统临时目录，owner为满足本轮显式/tmp约束在编译阶段取消，父用原任务`01a1aaf8-128d-7281-9a2f-a2699cf9a334`的get_command_output确认cancelled/无测试执行；原日志与metadata仍保留，不计通过。父接受限定tools回归，未给此次owner CLI/build权限；最终二进制与首请求链仍待Shell新source冻结后再验证。

整体产品链test owner `01a1aaf5-d358-7ab3-ad3e-93f248a93f5b`已冻结并释放唯一新test_subagent_todo_control.rs，SHA256 `1c88e37e51824002a71b2ca428fa0e8386c9a3b5ba70523bf7fb80cea59fc9dc`。只有固定rustfmt/parser/--check及空白检查，0Cargo/rustc/build/CLI/tests。验收以真实native TODO→background required spawn→child read/ask→parent edit→child typed consumed/ACK→parent remove→child Stopping/normal Result→parent exact review为一条链，动态change/result身份从真实隔离事实取得。父已读native主场景、source Planning引用、序列/消费与normal result/冷投影helpers，未用私有actor调用替代产品入口。新增invalid-stop-nochange整command不变、Stopping后仍未terminal/未accept、softinterrupt弃旧response不dispatch等反例，最终reap区分transport信号退出与canonical Completed。公共stream返回值明确检查Emitted，live通知/cold history/transcript隐藏而typed内部事实仍保留。

独立Luna `01a1ab0f-0965-7e12-977e-3b2db08b1001`静态审同冻结file，后继 `01a1ab1d-2204-7422-82cf-8b8864810b66`补齐自含报告，未发现可确认finding；范围含barrier/aux隔离、typed消费、invalid-stop整拒、异步原生读ACK、canonical Result/Ended/artifact、native wait/review以及live/cold投影与公开导入签名。它未编译或运行，父接受限定静态review，不把它当3.2通过。

队列修复owner实际red→green已返回：未修271e97生产上新增exact编译后1fail/exit101，有界等后续合法ACK超时；修复保存首项unconfirmed错误并继续scan，末尾仍返回原错误。首green在late fixture复用cold前Turn时InvalidNotification，修test等待ChatState owner恢复事实并建立真实新Turn，未放宽消费。`/tmp/grow-control-2.6d.2a.Q5hfpd/green-2`同exact1pass及全组13pass，unique13/0failed/0ignored、两命令exit0；原旧5沿刚通过且未变的证据。父亲读两完整log与cold-fixture-fix.diff，重算prod46917dc8d0bb5add741e724589cc2930dcba956c178e36b7d49802f8e908ac5f、test04cf56b708d661f20422bb24d621446d330d35fa75728afa1a1752834da64502及成功log/diff指纹全匹配，原历史责任不ACK、后续合法ACK/直属child推进、重复pass和后补真实消费均验证。Luna原生产复核无新问题，后继`01a1ab1e-c751-7f83-bdb1-7dcd5bdc9a43`仅核最后cold-turn小diff，终态尚未返回。

所有源码/Cargo已释放后父接唯一窗口，背景 `01a1ab1e-4f09-7c32-b9ad-d240eee7d5fa`在`/tmp/grow-todo-final-cli-verify.Axsm5J/`先默认CLI build再显式新binary跑去append后的fresh/resume exact。脚本保护全部当前dirty/untracked Rust与manifest/lock前后hash，provider/存储/Git隔离，尚无最终结果。新integration test仍未编译/运行。3.2未验收，不重复宣称本场景覆盖reopen/multi-child/nested/主动same-ID普通receipt反例；这些沿各owner验证记录核对。

## 规划控制通知展示（2.6c，已验收）

候选只在notification_inbox的notice生成/读回与communication_history的可见receipt/restore边界按typed来源排除PlanGuidance。读blob前就跳隐藏来源，既不创建缺正文占位，也不给隐藏receipt构造期待可见notice的ReceiptSource。普通ParentMessage/AgentReply即使正文等于固定控制前缀仍沿原可见路径；通用agent_message、内部receipt/模型消费及去重helper不改，无需改history/mod.rs生产。

父已读完整生产精准diff和三个私有测试源码，确认主例经过实际SessionActor receive_notification_detailed、Jsonl ACK、drain、模型request、retainedpayload、stream_session_history_at和transcript读取；两个history用例覆盖隐藏source不入索引/无blob、普通顺序/旧重复抑制。它们都尚未编译或执行，不能据源码断言已通过。source owner `01a1aaa4-510a-7111-911c-87dfa4fdef55`冻结五文件后释放scope，父再次`shasum --check /tmp/grow-shell-plan-guidance-lwg29074/source-final.sha256`五项全OK；相对dirty基线increment.diff在同目录。原actor只增私有test注册，原五枚举适配和inbox去重helper保留；限定1.93.1 parser/fmt/diff静态检查通过，0Cargo/rustc/测试。当时任务保持未勾，等待实际Shell运行。

父随后亲自运行五条Shell命令（后台`01a1aac0-8571-7722-95a5-260485216405` completed/exit0），其中本展示增量为真实actor live/cold 1、history边界2、原parent receipt恢复exact1，共4唯一passed/0failed/0ignored；原日志为`/tmp/grow-control-parent-verify-nulqwzfe/{ui-live-cold,ui-history,ui-old-parent}.log`。父逐份读回枚举并重算前后八文件hash一致，实际JSONL持久/模型request/stream与transcript路径全部经过。本次也记录一条新test unused_must_use warning，未当作运行失败或忽略不记。

原owner `01a1aac1-d90c-7ee2-a3d9-6dcacfe17031`随后仅把缺blob分支stream返回值补成ReplayEmission::Emitted断言，父亲核`/tmp/grow-plan-guidance-emission-_kuw1cml/correction.diff`及完整exact.log/results.json：同actor用例1passed/actualexit0、新warning消失，前后文件SHA256 `0c15478e456b0fdc6d43364dd97dfab6d0b7e0e424e6cba5761025d68b7bae98`父再算一致。未变pure2+旧1沿用刚取得的实际证据，无理由重复全组。总4个不同用例（actor补断言后重跑一次，不算第五项），所有命令退出且scope/Cargo释放。

父作为独立于实现者的reviewer，核typed来源筛选、history索引/ReceiptSource与ordinary同正文/cold消费边界，接受2.6c。整体Luna审查与真实控制产品链仍按3.2开放，2.6d未接纳/处理前不能称控制功能上线。

## 规划控制发送来源（2.6b，已验收）

Shell runner候选已从真实source ChatState单一快照核原session、request task、原link/step、resolved实际child与同一immutable change；只接受来源Dispatching/Received/Consumed/Handled，比较只忽略进度差异，后续当前step编辑/重开不改原stop payload。不可核owner为unconfirmed/planning_source_unavailable，缺失或不匹配证据为rejected/invalid_planning_source；读取前后取消/端点检查均在转发ReceivePlanGuidance之前。父已读这条实际生产路径，与coordinator直属边资格分工一致。

source owner `01a1aaa0-4a6a-7c03-84f1-e68ee1423563`冻结并释放两文件：interaction.rs SHA256 `7c1b6c4be0b746103ac7024d563d7eb214d1d042f0e11b9e9a6c248dc1151cf4`、新私有interaction_plan_guidance_tests.rs `459ad401f0019831bc50fa82c5372dd59a002b333c3a66864c76dfbd00472cda`。相对dirty基线diff与frozen-hashes.json在`/tmp/grow-guidance-source-odltoqbf/`；普通Send/Ask/reply、ReplanHandled与既有两测试未改。局部1.93.1 parser/fmt/diff检查通过；本次0 Cargo/rustc/测试，不把6项已写测试当通过。父支持下一Shell窗口后再运行`agent::mvp_agent::subagent_coordinator::interaction::plan_guidance_tests`与原对照，独立Luna `01a1aaae-d5a8-7dd0-a351-3cc0a14f8053`已完成此限定静态审查，未发现可行动问题，并核同两文件指纹；它明确6项是测试源码、目标端ACK由fixture返回，不验证下一owner的持久接纳。本模块仍待实际编译/运行后验收。

首个真实Shell验证由`01a1aab4-e949-7760-b624-da72d20857ae`执行上述6项filter，`/tmp/grow-guidance-source-tests-mzjwjdn3/first-1.log`实际exit101、0tests，唯一诊断为范围外TaskTool caller fixture `subagent_bash_permission_tests.rs:1031`的E0599：SessionActor值调用要求&Arc<Self>的handle_planning_command。父亲核日志/first-results.json和receiver/调用方，交回2.4c owner修fixture；2.6b两文件未变、Cargo及scope实际释放，两个既有exact对照尚未运行。该失败不作为来源校验行为失败，也不删场景或改生产receiver掩盖。

TaskTool fixture由原owner修正并exact1pass后，父直接执行`/tmp/grow-control-parent-verify-nulqwzfe/source-new.log`和source-old.log两个真实命令：6项新入口测试、原ACK取消及reply authority两项，共8唯一passed/0failed/0ignored，各actualexit0。整个后台命令`01a1aac0-8571-7722-95a5-260485216405`由父get_command_output确认completed/exit0；包含UI独立4项的五命令合计12，但本模块仅记8。父读回完整枚举，重算前后source JSON八项全部匹配，2.6b两hash仍为独立Luna审过的冻结版，未发生源码或fixture修正。

父亲核production边界、独立review及实际Tier3反例结果后接受2.6b。代码只证明来源授权与转发等待，目标ACK在本测试由fixture返回，child的持久接纳/Stopping仍是2.6d；不把本验收当整控制链已完成。

## 普通TaskTool强制输入（2.4c入口已验收，首请求里程碑仍待完成）

只读调查`01a1aa93-fe08-7780-92bf-a21566ef988b`确认native输入/schema和Link ACK消费是主要生产落点，旧model/effort/depth/cwd成功fixture须补有效规划owner，仍断言实际Spawn及原字段。后续真实CLI/mock firstrequest与去旧prompt拼接仍是独立里程碑，不用旧prompt正文冒充新runtime context。

首实施resume `01a1aa9f-4afe-7d91-b61a-fa74942b2c21`在进入新回合前被宿主compaction拒绝：`replacement would remove or change an unconsumed skill load`，phase为compaction replacement/turn_boundary_persistence_failed，运行报告numTurns=0，新增工具调用0。父通过受支持wait/status取得失败原因，亲核TaskToolInput仍Option、TaskTool仍原可选Link分支；现大dirty差异属于此前已存在工作，未回退或覆盖。不把此宿主失败记作本功能编译失败或受测实现，也未据此归因仓库已修SkillLoad代码发生新回归。用fresh coder `01a1aaa2-8d68-7f73-9d7f-41f3787ba36b`接续同一合同，保留失败交付待后继实际验收后处置，禁止安装/重启宿主或写正常存储。

fresh实现已将输入变为无default的required String，空白runtime拒绝、其余exact保留；所有普通fresh/resume/front/background/required组合无条件先消费现Link ACK，内部request仅Some。保留旧prompt追加和原not_dispatched持久责任，未改ShellLink、coordinator或ChatState。九文件含生产两处、注册及必要Shell/Pager调用方fixture适配；manifest十项包含未修改的planning_dispatch_tests.rs保护项。父已读精准dirtybaseline生产diff、真实ACK门槛测试及caller适配，未将全HEAD既有改动归本片。

固定Cargo/rustc/rustdoc1.93.1、offline/locked、MINSTACK16777216/JOBS2/testthreads2、独立/tmp Grow/TMP与空Git配置/hooks。父逐份读回`/tmp/tasktool-todo-input-rmnihhdr/logs/`三个原始日志及cargo-results-attempt1.json：tool-types `task::tests::` 30、tools `implementations::grow_build::task::tests::` 68、registry `task_tool_` 3，合计101唯一passed/0failed/0ignored，actual exit均0。覆盖required schema/参数rename、missing/null拒绝、8组合持ACK noSpawn→exact单次Spawn、稳定字面身份、空白、owner/prompt/closed/rejected/dropACK noSpawn及原model/effort/resume/depth/cwd目的；无Cargo失败，首fmt误用edition2021后按本仓2024重跑成功，未混作编译失败。

父重算source-ready-sha256.json十项全部匹配，manifest SHA256 `6b051f071e24ec3e3e07b214a0615b42e36c5c45a18b68afc8493423b7b2fc43`；common task.rs `815b0d06df22f3c3fe073f8cda18d233f2ccb691baaa0070e32fe6e3801c4ed5`、tools task/mod.rs `93097b3f02d7516d7a73466e9ef656aaff696f9e911f50abc077691b09443f46`。命令实际退出且Cargo/所有写scope释放；当前Shell验证接续编译新caller。独立Luna `01a1aab6-2971-7242-8a43-d75c9a6467d1`审入口未发现生产合同问题，指出下述Shell fixture编译阻塞；该版保持待修订，后继单项复核`01a1aac0-ca6b-7492-bece-a8ad393f8096`已确认修正关闭、无新问题。原101项证据对应未变生产，Shell/Pager更大集成未在此片执行，真实mock provider首请求与旧prompt移除仍未完成，2.4c整体任务不提前勾选。

Shell首次编译E0599由2.4c caller fixture使用SessionActor值调用&Arc方法引入，原owner `01a1aab9-1aa6-7012-bf3f-dd29ac3ce1d1`仅修subagent_bash_permission_tests.rs：同一actor置入Arc、真实Link处理后等待planning delivery结算，再用唯一可变借用切换原深度ceiling。首修订编译通过但重复建立causal Turn导致TurnAlreadyActive，日志保留；第二版install_test_foreground复用基础turn，原fastpath无approval/一次Spawn和depth hardstop不Spawn断言全保留。`/tmp/tasktool-fixture-arc-q8i1eayc/revised-exact-attempt2.log/json`实际exact1passed/exit0，父读回完整小diff、日志和release证据，重算最终该文件SHA256 `2b894e30fcb23b10d19aab7b0b278fbc094e86734aa788209e9dbdebea2a2e28`一致。Luna只复核这处变化及证据，不重复其余文件；父接受入口及此必要caller修正，后续父Shell12项运行也成功编译全部lib。

## Shell Link原子准入（2.4b，已验收）

父亲核`PlanningCommand::Link`的最终分支：现step gate下复查admitted prompt，读取ChatState当前canonical plan并核session owner，仅exact ID、非removed且Pending/InProgress步骤可追加link。当前scope来自canonical快照，SelectScope后的历史步骤不能补用，自定义scope跨后续regular turn仍有效。required两值均沿同一准入，task重复/自身SID拒绝与原commit→hydrate→ACK和统一重试调度保持。未改2.4a读取/renderer、通知或TaskTool schema。

三个文件为planning.rs、新私有actor/tests/planning_link_admission_tests.rs和tests/mod.rs一行注册。父已读生产精准变更、真实mailbox/ChatState/manual ACK夹具及11项测试，核gate实际先后次序、ACK前投影/回复不可见、丢ACK未确认后cold仅一份binding、无owner/foreign/expired prompt失败不变、canonical状态优先于陈旧资源投影。完成本身不生成stop，Link后cancel/remove/intent保留同一link与原replan责任。

固定Rust/Cargo/rustdoc1.93.1、offline/locked、MINSTACK16777216/JOBS2/单测试线程与独立/tmp Grow/TMP、空Git配置/hooks。`/tmp/grow-planning-link-admission.q5fr7m/02-planning-link-admission-serialized-comparison.log`实际11 passed/0failed/0ignored、exit文件0；四个既有exact `task_link_and_parent_commit_exchange_order_without_losing_latest_requirements`、`active_or_unknown_link_blocks_scope_switch_without_faking_terminal`、`optional_undispatched_scope_switch_succeeds_without_review`、`required_undispatched_scope_switch_needs_released_review`分别1 passed/exit0，日志03-legacy-link-1至4。父逐份读回，总15唯一，无空过滤器。首次编译exit101仅新增fixture比较Vec<TimelineEvent>要求不存在的PartialEq，修为完整序列化值比较；首失败原日志保留，不改生产类型或弱化事实断言。

父对source-final.sha256再次check三项全OK：planning.rs `5318abfc56f3a24bc7411074dc661efcbdba9e11e72893cc64ce9817d003ba07`、新test `41d4e6a60282e5393c728370c8df4b09d3748e1d2af6a5003438c7c63e22667c`、tests/mod.rs `f0ecd5a6a4c68b1b6a4cf96aa4b229fe545b55a12138e86885345d4107f50785`。production/注册保持source-ready版本，只有上述fixture修正；限定格式/diff检查通过，实际命令和三文件scope/Cargo均已释放。Shell编译也闭合2.6a五文件机械enum适配，无额外exhaustive错误。本模块验收不代替2.4c schema、真实provider首请求或控制整体milestone；整体独立review仍开放。

## 独立规划控制通知事实（2.6a，已验收）

ChatState候选为`NotificationSource::PlanGuidance {parent_session_id,message_id}`增加独立私有identity；agent_message固定interrupt=true，沿现版本2/Session owner，不允许owner override。父已读相对脏代码基线的实际production diff，确认复用pending/parent receipt/消费后payload保留与durable receive exact检查；Typed Consumed沿现AgentReply同一输入核对，不添query/index。只有精确PlanGuidance receipt消费能由现`consumed_notification_turn(source,version)`证明，普通同sender/message ID先消费仍保控制pending。

实际固定Rust/Cargo/rustdoc1.93.1、MINSTACK16777216、JOBS2、testthreads2、独立/tmp Grow/TMP和空Git配置/hooks，四条`cargo test --offline --locked -p chat-state --lib <filter>`最终均真实exit0。父逐份读回`/tmp/grow-plan-guidance-jzvxd5ui/attempt2/`完整日志：durable_agent_message 6、agent_message_consumption 2、parent_receipt_history 2、notification_consumer_query 3，合计13唯一（7新增、6原有对照），0failed/0ignored，无零命令。真实manual ACK覆盖持久确认前不发布、ACK丢失/失败原event重试不重复、cold spawn_from_timeline保exact消费者；pure Timeline覆盖same-ID来源隔离、typed完整批次及None/User/receipt/source/target/message/reply/hash/bytes/order/duplicate失败不变、消费后payload保留与cold一致。

首次编译exit101仅新增fixture错误用assert_eq比较未实现PartialEq的TimelineError，两处改为matches!(InvalidNotification)，未改生产错误或删断言；原log`/tmp/grow-plan-guidance-jzvxd5ui/1-durable_agent_message.log`保留。最终9个授权源码与10个保护文件SHA256由父逐项重算全部匹配`attempt2/tested-hashes.json`，完整feature.diff明确Shell5文件仅9行必要枚举caller适配。timeline.rs SHA256 `487bbfcdc026889cacbf39dc497e35e8d0ff78b3e9d51d71f2fe48fb868eff7c`；actor/mod.rs `fee2c994ea2dcce67815883a4bd8ff8ad1a5b1b10a340a061244369278625f9a`。format/diff检查通过，所有命令实际退出且写scope/Cargo已释放。

独立Luna `01a1aa97-ecea-7532-975e-530a5911aa24`已完成最终数据owner增量审查，未发现可行动问题；它核对同19项hash和四份实际日志，未自行执行测试。父接受此限定独立review及13项公共API增量。随2.4b的真实Shell lib编译已闭合五文件机械enum适配，无额外exhaustive错误。此处不生成控制、不核host provenance、不隐藏UI或接线中断/Handling，2.6b/c/d及整体生命周期仍待后续，不将本模块验收等同于控制已上线。

## 纯规划 stop 保留增量（2.5，已验收）

父将本片差异与`/tmp/grow-tooltypes-stop.qPFyAZ/planning.before.rs`逐项比较，读回16项新公共纯API测试及边界，接受该tool-types增量。共享`ReplanChange::is_stop()`由removed/Cancelled派生，`LocalPlan::link_stop_requested()`扫描事实，不添持久map或directive；ReplanOutcome::Stopping仅表示接纳收尾。旧普通Pending可合并为首次stop，此后原stop身份/payload/progress保留，后续有效意图Unapplied且不抬高真正pending revision；新执行link仍可接受同step的新要求。

新ACK同时受advance_replan和validate_transition_from约束：stop只接受Consumed→Handled/Stopping，普通变化不能Stopping；已提交的历史stop Updated/NoChange能validate/load并exact保留，不能被用作本轮新写ACK。直接构造transition删除/替换Pending stop、在stop后追加推进目标或同次提交抹停止同样拒绝。terminal/undispatched仍可按原真实原因处置，不捏造child结束、canonical Result、wait或required delivery验收。

实际首次且唯一测试命令`rustup run 1.93.1 cargo test --locked -p tool-types --lib planning:: -- --test-threads=2`，固定RUSTC/RUSTDOC1.93.1、MINSTACK16777216、JOBS2、独立/tmp Grow/TMPDIR和空Gitconfig/hooks，exit0；`/tmp/grow-tooltypes-stop.qPFyAZ/planning-first.log`完整枚举55 passed/0 failed/0 ignored，父已逐项读回。55为新stop16+原planning26+原read13，保留Released(None)和分页全部39项旧回归；不沿用早期消息对旧子组数的误计。

测试包括edit→cancel/remove→reopen/edit→dispatch/receive/consume/stopping完整纯链、不同交付阶段stop不被覆盖、旧普通ACK不清新stop、较旧immutable消息晚到、同step多link及新link、replace tombstone/ID复用拒绝、terminal/undispatched、no-op/纯completion与组合编辑、直接提交边界、历史Handled事实以及停止ACK不清执行/wait/必需验收责任。此处是纯owner API证据，未将其当作消息/命令实际停止的运行证据。

父重算受测SHA256一致：planning.rs `3e145b3b67929eabc6d03d742b32c9c17891ec58b25f64dd30f517ed96fdf458`；新planning/stop_tests.rs `94302b903ef2eb562747650774d97d2579d16c77e2390a862c7004df36b27014`。限定rustfmt/diff检查通过，无首轮失败或源码修正；既有read.rs/Cargo/lock未变，Cargo与两个文件写scope已释放。Shell HandleReplans的Stopping接线、固定隐藏控制和最终运行链仍待2.6与整体独立review，不宣称完整change验收。

## 原生 todo_read 注册增量（2.3，已验收）

父已读新工具真实`Tool::run`、FinalizedToolset严格flat解析/名称参数重命名和完整JSON返回测试、metadata v7/schema同步，以及Shell工具行/raw投影和权限分类。自读发`PlanningCommand::Read`，父读走绑定backend；Resources session reader无模型身份参数。5秒包含资源锁等待，取消/drop只结束观察；每页先重获授权快照，再校验cursor。新ToolKind::PlanRead独立于Plan，不覆盖TodoWrite的模板映射。

固定Rust/Cargo/rustdoc1.93.1、MINSTACK16777216、JOBS2，使用`/tmp/grow-todo-read.OWsGRs` Grow/TMPDIR、空Gitconfig/hooks。最终9条定向命令实际合计101 passed、0 failed、0 ignored，无零测试命令；父逐份读回完整测试枚举：

| crate/filter | 实际passed | 原始记录 |
| --- | ---: | --- |
| tools `todo::read::tests` | 15 | child01a1aa3d-9952 terminal `call_2f853a4dfd3c4da8885368fa33f68c03.log` |
| tools `todo_read_public_registry` | 2 | registry.log |
| tools `tool_taxonomy::tests`，无UPDATE的schema精确比对 | 8 | taxonomy.log |
| tools `registry::tool_kind_contract` | 2 | kind-contract.log |
| tools 既有`todo::tests` | 26 | todo-write.log |
| agent 标准default/concise/hashline toolsets，exact | 1 | agent-toolsets.log |
| workspace `permission::types::tests` | 12 | workspace-classification.log |
| Shell `session::acp_conversion::tests` | 33 | shell-acp-rerun.log |
| Shell `session::actor::tool::authorization::tests` | 2 | shell-authorization.log |

首Shell编译E0004/exit101保留于shell-acp.log：ToolInput新增TodoRead后，workspace `AccessKind::from_tool_call`的既有穷尽匹配需要同功能适配。父核对并授权仅在InternalControl分支补TodoRead、保留实际wire工具名和回归；未增加文件系统/进程权限。修正后受影响workspace/Shell组全部重跑通过，已通过且未变更的tools/agent组未重复。标准ACP回归证明读取只完成工具行和完整raw事实，不产生Plan更新或新的聊天正文；模型prompt输出等于完整共享PlanReadOutput JSON。

父对`source-sha256.txt`与`permission-source-sha256.txt`运行shasum --check，最终19文件全部OK。核心read.rs `f359f52831e6c90f5784fb7e0f8ad73cd2477719752616f5107f0a0da8d59ceb`（原source-ready17fb...仅测试补self+parent drop）；registry/types.rs `85fcb6e5bc98ebc73b2f9269d6972b7f7fc94e508dd602c520e479415f80b7b3`；tool_taxonomy.rs `1c2d0682d6e984e9f2f82cd7b7c4d6d597ec8b11fd4e72cb89a670b4871c9855`；schema `c0982e629e7324540e316a5156662758f7f52a777fb99bd83048fb37b7515548`；必要workspace caller `8f8aeaeb3caed09c045b0cf264388b2d6e613481bfcc7b1044ea7209e820bc95`。限定格式和diff检查通过，Cargo和本片写scope释放。

独立Luna `01a1aa48-06fc-78c0-aa63-1a0e88b5d0f9`已完成2.2a–d及2.3的只读审查，核对canonical self owner、session-bound真实直属边和exactlink、每页授权/cursor/大小限制、native schema/完整模型JSON、ACP与None权限及新增workspace caller，未发现可行动问题或本片验收阻塞；它未重跑测试，运行结论引用父实际读回的101项证据。父接受本原生读取增量并勾选2.3，早期source-ready执行由最终注册/验证交付替代。

完整原生Tool→Coordinator→Shell调用链仍留里程碑，不因分侧真实测试或本片独立review宣称整体change通过。强制绑定、首请求/每请求renderer及stop控制尚未完成。未触正常Grow存储、真实provider/MCP/hooks，未提交/安装/重启。

## 直属父快照 Shell 增量（2.2d）

父已读ShellChildRunner同步lookup source/parent、从入口建立5秒绝对期限及spawn_local只读过程的完整实现；真实parent ChatState `planning_state()`一次取得seq/plan，存在绑定时只接受相同parent/task/实际child SID/step的link并生成同scope delegation。None、InvalidEvidence、Unavailable、TimedOut与观察取消按约定区分，未调用消息、计划修改或child取消接口。

固定Rust/Cargo/rustdoc1.93.1、MINSTACK16777216、JOBS2、offline，独立`/tmp/grow-parent-plan-validation.fkuB8d` Grow/TMPDIR/xdg和空Git配置/hooks。实际命令`rustup run 1.93.1 cargo test --locked -p shell --lib agent::mvp_agent::subagent_coordinator::parent_planning::tests:: -- --test-threads=1`首次9 passed、0 failed、0 ignored，exit0；本轮没有源码修正。父读回`attempt-1.log`完整9项枚举，重算日志SHA256 `8e096d7fc455866e76acd057e5e05b51634dc46f1462f96c276f6502ea3275f3`。

测试从实际trait handler经MvpAgent registry lookup到真实ChatState owner；覆盖root decoy/A/B直属父、17步骤与完整五类事实、tombstone/terminal/reviews、same-step retry不可替代exactlink、bound/unbound的snapshot owner核对与无计划区别、缺失/关闭handle和query失联、真实ACK阻塞下5秒期限/观察取消，以及Timeline/Surface/Resources/ACP/command无新增。持久化为受控ACK fixture，不是磁盘故障恢复或完整ChannelBackend→Coordinator→ShellRunner集成；后者仍留里程碑。

父重算受测SHA256完全匹配：subagent_coordinator.rs `3275a44430025c3c85fc81a12bdc7767afa73fb41eb8a085b46851cb59908163`；新parent_planning.rs `d37e73b6b1b9ef4c5b0bb92dff5e81f660053bb5294233101cac8f5115edca23`；新parent_planning_tests.rs `6835323150860aa09559836bfc4afb9502da453a0768284df0bb266becd59957`。限定rustfmt/diff检查通过，Cargo和三个文件写scope已释放。接受2.2d局部增量；工具注册、首请求父项上下文及整体独立review尚未通过。未触正常Grow存储、provider/MCP/hooks。

## 直属父路由 tools 增量（2.2c）

父已核对session-bound ChannelBackend、ChildRunner seam、coordinator真实active child SID→immediate parent/task/step的路由、RetiringChild保存真实SID及全部相关终态分类，接受此tools增量。读取不借lifecycle root宽观察权限，不等待Shell或模型；Workflow只读仍沿真实直接边。5秒、取消、断通道/response dropped与NoParent分别保留既定错误，不创建通知、结果或恢复工作。

固定Rust/Cargo/rustdoc1.93.1、MINSTACK16777216、JOBS2，独立`/tmp/grow-parent-planning-kEr30v` Grow/TMPDIR与空Git配置/hooks。实际执行`cargo test --locked -p tools --lib implementations::grow_build::task::coordinator::tests::parent_planning_tests -- --nocapture --test-threads=2`为12 passed、0 failed、0 ignored；既有`coordinator::tests -- --nocapture --test-threads=2 --skip parent_planning_tests`为77 passed、0 failed、0 ignored，含6项model-control。父已读回`routing-final.log`与`coordinator-regression.log`完整测试枚举，真实exit文件为0。旧回归中的panic输出来自明确的故障注入，测试本身通过。

12项覆盖root/no-parent与unbound、root→A→B只路由A、task ID不同于真实SID、Workflow无虚构step、pending/cancelled/suspended/execution-returned/completed/failed/finalization，以及completed缓存淘汰后的retiring；host读取阻塞期间其他read/query/spawn仍推进，5秒超时、取消、丢弃与关闭不改变读者或child生命周期。首轮实际11 pass/1 fail，日志`routing-initial.log`/exit101保留：fixture主动创建未确认终态后，现有恢复timer首次tick已执行一次，被末尾绝对0断言误计。修正仅用paused time和mailbox屏障确认读取前已有计数，再逐次严格比较read前后registry与恢复计数；父已读此修改，生产API未动。

父重算最终Git blob hash与受测版本一致：parent_planning.rs `191ba7497663229df829ad029707a5e833641395`；backend.rs `6c0447ce0c28f82d780295e6d15f3cff8b2ebd3a`；coordinator_state.rs `646e93e88ffb4b6ed3750af6701750f0e1b76e0b`；coordinator.rs `5297332d41635bd04f1a0647c36e0afbb69a8ab4`；task/mod.rs `37abec78fc1e1b53482a9e605b26fc2dde330c56`；types.rs `dd8befdad4ee868e8aba107a284522d209e6a17b`；coordinator_tests.rs `8da67de2fc21f45a9abe99ea09301f50e8775685`；新routing测试 `a59c74849c7fb60c4e6ad2d2841e9a7e66900d05`。限定rustfmt/diff检查通过，Cargo与tools写scope释放。

此处验收到真实ChannelBackend→coordinator→recording runner的路由与不变量，不以录制返回的fixture快照证明Shell实际parent handle/PlanLink验证；2.2d与原生todo_read整链路另行验收。未运行真实provider/MCP/hooks或全workspace。

## 自身计划 owner Read 增量（2.2b）

父已核对`todo/runtime.rs`的typed Read命令、Shell `handle_planning_command`在step gate前直接返回的真实实现、新增5项入口测试及旧TodoWrite exact适配，并读回两个原始日志。Read只查询现ChatState owner，返回同一计划事实seq和完整LocalPlan；None与Err保留，不依赖当前prompt、不hydrate/commit或调度planning delivery。

固定1.93.1、RUST_MIN_STACK16777216、JOBS2，env-i/offline、独立 `/tmp/grow-task2-2b-self-read.CojzSD` GROW_HOME/TMPDIR/空Git配置/hooks。`cargo test --locked -p shell --lib session::actor::tests::planning_read_tests:: -- --test-threads=1`实际5 passed、0 failed、0 ignored；`cargo test --locked -p tools --lib implementations::grow_build::todo::tests::write_is_sent_to_actor_and_projects_its_response -- --exact --test-threads=1`实际1 passed、0 failed、0 ignored。日志为`01a1aa13-a93b-7de3-a080-bd92eb84af65/terminal/call_69e0f9ff425b4953bd523a5ca648391d.log`及`call_c67a3c5a48834b53bd9a901d514b9090.log`，各自明确EXIT=0；父已确认非零测试枚举。首轮编译因新test导入私有helper产生E0603，仅在本fixture保留观察receiver修正，生产与原断言未变。

五项覆盖无计划不创建/通知、17项完整权威计划与故意陈旧Resources隔离、重复读取无Timeline/Surface/ACP/Resources/delivery副作用、持有step gate仍有界返回、实际Tool/Step事件推进但计划事实seq保持，以及关闭ChatState owner保留Err。父重算最终受测SHA256：runtime `42c77a39e630e027ba7a268fcfb5422800edc7e2e69870058a9122090e35cd59`；todo/mod `0ce7dce83e79b1925ec0ad56bc3fa0320ea4745f5a5135d78186d313446e0072`；Shell planning `386ea3bef9b00615c6fc7604fc0b47ea272aba03a2ebd5d6f1c3cdd085c2f9a6`；tests/mod `d08956ba469f6ef47e9ed3d23eb46f47473c4da971f5274a7a5339e0d47ef5b8`；新test `a2d0b3fabc794ecc9ef68a8f3f4debeb051a23517d9370fb187c12435831d89e`。限定格式/diff检查通过，Cargo和写scope已释放。

接受此局部增量，不证明parent授权、todo_read原生注册、renderer或stop整链路完成。未调用真实provider/MCP/hooks、未触正常Grow存储；整体独立review与里程碑仍保持开放。

## 只读分页纯投影增量（2.2a）

父已核对 `tool-types/src/planning/read.rs` 最终生产实现、schema、过滤/游标/字节边界测试和原始运行日志，接受此独立增量。它复用原 LocalPlan 五类事实，只克隆页内记录，不改变持久 schema、计划转换或授权 owner；无计划时也能通过 `PlanReadQuery::validate()` 复用参数规则。新增 Rust 输入字段直接为 `target/step_id/cursor/limit`，不含 query 成员。

实际命令 `rustup run 1.93.1 cargo test --locked -p tool-types --lib planning:: -- --test-threads=2` 退出0：39 passed、0 failed、0 ignored，其中13项新读取公共API测试、26项现planning回归。固定rustc/rustdoc1.93.1、RUST_MIN_STACK16777216、JOBS2，独立 `/tmp/grow-plan-read.KbDKHq/{grow,tmp,hooks}` 和空Git配置。首次 `cargo +1.93.1` 因本机cargo不是rustup shim在编译前失败；上述替代命令为唯一实际测试运行。父读回日志 `01a1aa0f-2dfa-7720-a61c-5c0d472dc152/terminal/call_0b3add9f2aea498e9738e8fef24edf68.log` 确认完整测试名和非零通过数。

证据覆盖五类记录混合跨页重组与limit变化、tombstone和wait/review关联、未知步骤/空计划、严格平面wire、全部cursor身份域和计划seq/revision、畸形及越界cursor、完整UTF-8/转义requirements的256 KiB实际输出限制、外壳与next_cursor开销、单条过大明确失败且不跳过，以及输入借用不变。限定rustfmt和diff空白检查通过。父重算最终SHA256与测试版本一致：`planning.rs` 为 `b2b88b196c83e9a9286e21c7e26a88cc26549d2a4120c5544310681aab424557`；`planning/read.rs` 为 `8e8121731e66c4950e6816dd7485fe231dca3cbb1ee6b92301c74c6e47c33d9e`。原Released(None)差异保留。

此验收仅包括已授权快照的纯读取契约。Session绑定身份、同一Timeline快照取得、父owner授权、工具注册与整个控制链路尚未以本片证明；未运行其余104项tool-types测试或全workspace，未调用真实provider/MCP/hooks。
