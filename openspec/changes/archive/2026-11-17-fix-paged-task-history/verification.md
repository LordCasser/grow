# 分页Tasks恢复验收

## 失败证据与设计修订

v2.6.16官方Core job133505406714在原PTY `ctrl_d_resume_restores_interrupted_command_as_terminal_in_tasks`第444行失败：Timeline CommandsSettled断言已通过、正文Taskfailed存在，Tasks history无命令行。paged replay earlyreturn固定cut、没有旧replay任务状态接纳，正文projector也未填中心bg_tasks。

调查初期曾假设HistorySnapshot.sources保有所有taskfacts；该假设被indexed hydrate路径推翻。它只从TASK_CUT注入未结算start，已完成Task presentation需要从专门物理引用读取，不能直接复用prepare_indexed过滤后的updates。design据此增加同事务Task物理ref表，未实施全量正文回放。已发布CommandsSettled同样可只剩索引引用，须用现task_terminal reader补全。

## 索引模块：已验收

父亲读TASK_DISPLAY同事务写入、固定manifest cut读取与引用/源文件/类别/seq/分支验证，schema51→52按原派生索引失效重建，不改journal。接口只返回prefix原task presentation；tail与settlement由调用方合并。

- Rust1.93.1：`background_task_history`7 passed、0 failed。
- 同工具链`working_snapshot::tests`58 passed、0 failed、1既有ignored。
- 亲读回归覆盖已完成普通任务和openstart、旧cut在后续publication后全等、rewind隐藏但旧cut仍可见、prefix/tail分开、无关响应正文即使损坏也不被getter读取、source/class/seq/cut损坏报错、旧schema重建时journal字节不变。
- 索引子交付01a1e5b8-d5dd-7580-89ae-ee75ab45fef4已接受；Windows尚待后继官方原生/分发门禁。

## Shell模块：已验收

父亲读prefix/tail合并、物理去重、published task terminal reader及MvpAgent发送diff。六项定向测试通过：两项lightweight_tasks（真实index中已发布CommandsSettled且Runtime不在内存，140正文行前start、ordinary prefix/tail完成优先与去重、rewind后settlement不复活）；paged_history_replays_only_task_state_and_drains_every_ordered_send（恰好两条ordered任务状态、双标记/persist/leader、末条未ACK时仍Pending、metadata不回写facts）；canonical_multimessage_response_cursor_requires_complete_matching_proof、history_restores_distinct_command_terminals_across_suspensions、fixed_timeline_snapshot_replays_complete_response_before_independent_history。

测试专用正文投影计数同时具备完整canonical响应的阳性对照；窄任务投影不增加，paged replay增量等于原cut capture一次，未额外解码正文。未发布tail的domain rewind（working_snapshot.rs原2894处）或presentation marker（原3008处）会放弃indexed hydrate并走dense history，未另建分支机制。Shell子交付01a1e5b4-b821-7ac0-bd5f-84f87532913e已接受。

## Pager模块与整链验证

双标记`isReplay`和`grow/taskStateOnly`只用于任务中心恢复，不向正文追加，不推进正文cursor。无真实start的单独完成不制造空tool-call身份；非空retained stdout保留，truncated只累积。父修正两处新增测试String实参；cursor阳性对照原依赖instrumentation，改在显式启用的隔离测试进程执行，保留普通replay必须记录cursor的断言并验证子进程实际执行1项，未放松oracle。

- Rust1.93.1默认CLI构建task01a1e5c6-505f-7900-ac72-6c6e34b44dd6退出0。
- 同一binary原失败PTY `ctrl_d_suspend_and_continue::ctrl_d_resume_restores_interrupted_command_as_terminal_in_tasks`经exact runner验证恰好1项执行并通过（task01a1e5c8-ca44-7461-ad22-f985894daaa5，14.79s），原断言未改。
- Pager完整background_tasks组37 passed、0 failed；单独metadata parser通过（task01a1e5cf-d744-76a2-935f-b3e5c83923ab整体exit0）。最终Pager全库7735 passed、0 failed、15既有ignored，覆盖最终cursor子进程及parser，Pager子交付已接受。
- 当前生产源码`RUSTFLAGS="-D warnings" cargo check --locked --profile release-dist --features release-dist -p cli --bin grow`与全workspace/all-targets均通过（task01a1e5d0-6123-78c3-9926-a9db6ccb6853整体exit0）。
- 父只对两处新增Pager生产文件执行固定rustfmt机械布局后，全仓fmt与git diff --check均通过。
- Shell/Pager最终全库task01a1e5d0-8e6f-7ed3-a5ac-65944337cc73退出0：Pager7735 passed/15既有ignored、Shell4877 passed/14既有ignored，合计12612 passed、0 failed。Shell测试图仍报告范围外input_admission.rs:2827既有irrefutable-if-let warning；生产分发-Dwarnings检查已通过，未为此混改业务代码。
- 官方38项PTY选组task01a1e5ce-17c6-78e0-8744-710d3852b722整体退出0（1452.4s，含编译/锁等待）；每项exact runner均报告恰好1项通过。父机械比对38个package/target/name及顺序与core-regression.yml完全一致。包括原Tasks恢复、三项Plan、Minimal历史、140轮跨页恢复、100+50同Turn用量、timeline direct seek、部分释放与并发冷恢复，全部通过。
- 开发者说明同步Tasks中心状态与正文分页的独立归属、同cut索引52及诊断入口。全仓fmt、git diff --check、相对链接/requirement锚点及OpenSpec严格/归档校验通过。

本项功能与本机整链验收完成；正式后继发行的同SHA Windows/其他平台构建、三平台原生与公开资产验收仍由prepare-release-v2-6-17承担，不能以本地通过替代。
