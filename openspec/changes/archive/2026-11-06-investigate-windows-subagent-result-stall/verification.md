# Investigation verification

2026-11-06，macOS工作区main `8ab486a6e45870561b91dba872934a6e24a795f1`。本轮是Windows用户报告的诊断，不修改生产代码，不声称该机器运行本修订。

已读OpenSpec索引、session-timeline直属父级交付/状态观察契约、task-planning结果绑定及旧recover-subagent-terminal-commit/released-delivery修复的验证限制，复用已加载且未变化的software-engineering约束。相关源码包含subagent handle_request/mod/finalization、FinalizationFailure、Turn admission、状态/wait工具和TraceArgs。目录内无额外AGENTS。

确认证据：

- 可见答案、child Turn.ended、prompt waiter返回、execution_returned、canonical SubagentResult、父Ended与receipt有不同提交位置。Turn.ended之后仍有post-turn hook、flush、文件状态收尾和usage冻结，不能仅凭running认定provider仍采样。
- 30秒finalizationcycle从execution_returned之后开始；不会覆盖此前的prompt返回等待，也不会保证超时后自动变Completed。未知I/O/ACK保留原所有者及结果，不授权重写。
- finalization recovery显式拒绝旧actor/persistence未退役时接管writer；相同现象可能跨平台，当前未发现独立Windows提交分支不等于证明Windows底层文件/进程实现无问题。
- `grow trace <session_id> --output <path>`参数来自当前源；v2.6.1与当前trace_cmd.rs的Git blob均为28a3aa9f2fca9f0e5340962fdceeedc1b5c4c16f。trace导出仅本地保存，不启动provider或重放工具。另一个Shell的grow --version输出仍不能单独证明旧运行进程映像。
- 发现工具cua.list_spaces并进行只读枚举，仅有一台macOS relay，未连接目标Windows机器。没有尝试新建沙箱、远程控制或读取别的设备；当前无法直接检索Windows父子Timeline与进程状态。

用户后来明确无法取得Windows现场，改为纯代码分析。验收目标已据此改为控制流、平台路径、反例条件和证据限制；没有将不可取得的版本/trace保留为本次完成前置条件。实际Windows版本/运行身份、最后成功事实、stage/certainty和阻塞栈仍未知，不声称线上根因。

追加核查：

- 亲读freeze_prompt_usage/drain_subagent_usage_for_prompt_bounded及outstanding_reply_for_prompt，120秒deadline仅在query ACK之后检查；没有ACK的控制流无法到检查分支。这是静态期限覆盖缺口，不是已运行的deadlock复现。现有freeze_timeout_marks_report_and_both_ledgers只覆盖及时回应且live_ids非空。
- 亲读Workspace after-turn实现、FileStateTracker::end_prompt、LocalFs/ACPFileSystem、Gateway/acp_send及FlushAndAck。after-turn实际只做计数；文件快照并发8路但无总deadline，刷盘等待还依赖原writer ACK。默认workspace为LocalFs，不把ACP实现当所有CLI默认路径。
- 亲读Coordinator的finish/advance_finalizations、sweep_retiring、OpenSpawnAdmission、GetStatus/Inspect/Wait路径，以及Shellfinalization的in-flight/SessionThread门。最多三次/30秒周期与只读查询不续周期是确定状态行为，现契约允许期限后保持待修复。readiness检查耗尽后owner晚退役可继续pending，但本轮没有改变该政策。
- 亲读Result之后的Shutdown、无条件SessionEndHook::Triggered与Timeline封闭校验，确认此前backlog的顺序错误；即使空handlers也尝试写Hook。错误分支return且父Timeline另有owner，不能单凭它推出父结果必丢或必永久挂起。

Windows文件路径由`01a1aee4-0a6d-7d50-8ed8-5c126372eae7`（bigmodel/glm-5.3默认effort）独立只读审查，父已验收有限源码证据并明确收窄其过强推断。父亲读session/persistence.rs:1074、storage/mod.rs:1490/1540、subagent handle_request.rs:2470/2794和finalization.rs:146；所审四文件在v2.6.1与8ab486a6之间diff为空。未发现直接Windows文件API用法错误，但未执行Windows测试。父不接受“全部失败有界/Windows文件链可排除/30秒保证canonical递交”：timeout不终止同步worker，未知I/O仍持owner并可导致持续pending。目录sync的已有能力限制不作为本轮新活性缺陷。

没有新增或修改业务代码、测试，没有运行Cargo、真实provider、Windows进程或故障注入。调查结论与后续定向反例按owner记录在investigation.md和backlog；本次完成到代码分析，不隐含修复验收。

初次定位时文档校验通过：`openspec validate investigate-windows-subagent-result-stall --strict --no-interactive`、全量strict17/17及`git diff --check`，日志`/tmp/grow-official-chat-GcDkz73j/specs-after-windows-investigation.log`。纯代码分析完成后的归档前strict17/17，`openspec archive investigate-windows-subagent-result-stall --yes`成功；归档后全量strict16/16、archived872/872和diffcheck通过。最终日志为`/tmp/grow-windows-result-static-{specs,archives}-final.log`。这些仅为文档一致性证据。源码和依赖diff为空，未执行Gitcommit/push、安装、重启或发布。当前调查按用户要求完成到纯代码分析，后续独立事项保留backlog。
