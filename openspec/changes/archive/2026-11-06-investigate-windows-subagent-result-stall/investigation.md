# Windows：Result 已显示但未递交/结束

## 范围与结论强度

用户报告Windows上subagent已打印Result，却未结束或递交；随后明确无法取得现场，改为纯代码分析。本次结论基于main `8ab486a6e45870561b91dba872934a6e24a795f1`，不依赖Windows日志、进程或用户会话，不把某个源码风险认定为该机器的实际根因。只更新调查和backlog，不实施修复。

源码确认：存在能够让结果正文出现后仍持续等待的控制流；已有30秒收尾期限覆盖不到全部等待。最明确的边界缺口是用量冻结中的Outstanding查询未被120秒期限包住；另有文件快照/刷盘等待和有限恢复停止的行为。还复核了一项已登记的确定顺序错误：child Result关闭Timeline后，SessionEnd仍尝试写入Hook。它不能单独证明父级结果必然丢失。

## 生命周期边界

```text
模型答案可见
  → 协议完成、response接纳、SubagentStop gate
  → Turn.ended持久确认
  → after-turn本地计数、刷盘、文件after-snapshot、用量冻结
  → prompt waiter返回、构造SubagentResult
  → execution_returned
  → [30秒finalization周期]
      输出artifact → child Result → 父usage ACK
      → 请求child Shutdown → 父Ended → planning结果接纳/完成receipt
  → Coordinator确认可交付
```

child独立Session/runtime不等于一个靠stdout EOF关闭的命令进程。Result文本不充当上述任何ACK；存在child Result也不等于父Ended/receipt已确认。读取历史、等待查询、终态提交与模型消费各有所有者。

主要位置为`crates/codegen/shell/src/agent/subagent/handle_request.rs:2196,2421,2470,2509,2538,2671,2713,2792`，以及`mod.rs:611,830,2216,2694`。30秒期限在execution_returned后才创建，不能倒推正文显示后的所有等待都应30秒内结束。

## 发现一：用量冻结的120秒期限没有覆盖查询ACK

**确定的期限覆盖缺口；永久停滞取决于查询方迟迟不回ACK。**

- `session/actor/turn/admission.rs:1580`在正常prompt返回前调用freeze_prompt_usage。
- `session/actor/turn/mod.rs:525-590`创建120秒deadline，每轮先await outstanding_reply_for_prompt，收到reply后才检查deadline。
- `session/actor/turn/settlement.rs:49-70`发送Outstanding后直接`rx.await`，没有timeout或cancel分支。

反例控制流：查询发送成功，接收方/排队operation保留respond_to但没有回应；即使120秒过去，执行仍停在rx.await，deadline分支根本不可达。此时prompt waiter未返回，execution_returned和后面的30秒finalization均未开始。该描述是依据代码推导的可达等待形态，未在本轮运行注入实验，也未证明生产Coordinator平时一定陷入它。

反向证据也必须保留：`tools/.../task/coordinator.rs:813-875`正常处理Outstanding是同步构造并立即发送结果，没有等待child完成的await，因此不能把源码直接描述成“父子互等的必现死锁”。正常前台后代未结算可等待到120秒；background子任务被排除，只标report incomplete。现有`freeze_timeout_marks_report_and_both_ledgers`（subagent_usage_fold_tests.rs:408）让responder持续回应live_ids，验证的是“收到reply但仍有child”，没有覆盖“sender活着但始终不回应”。

后续应由Shell用量冻结owner将同一绝对deadline传入观察等待，并沿已有incomplete语义收敛，保留迟到usage可结算；不能超时记0或捏造child完成。该项与文件快照分别修复，不打包为全局timeout框架。

## 发现二：最终答案之后仍同步等待文件快照和刷盘

**确定的等待路径；是否属于缺陷要区分读操作、持久写入与原所有权。**

`turn/admission.rs:1557-1580`在prompt返回前依次flush_to_disk、FileStateTracker::end_prompt、freeze_prompt_usage。`workspace/src/session/file_state.rs:574-613`重新读取本prompt触及的文件，buffer_unordered(8)只限制并发，没有总deadline；一个read不结束就无法collect完成。默认workspace使用LocalFs（workspace/src/session/mod.rs:132），其try_read_file直接await tokio::fs::read（file_system/local_fs.rs:30）；可注入AsyncFileSystem的接口也没有保证必定返回，ACP实现经gateway请求等ACK。

这里可证的是“最终答案后还要读完文件，read不返回则prompt不返回”，不是已证明普通Windows本地文件会挂死。网络盘/特殊设备/客户端读不回只是条件候选，不能无现场归因。没有触及文件时快照集合为空，也就不存在这项读取等待。

`actor/updates.rs:690-701`的flush_to_disk先等ReplayActor（该段已有独立5秒界），再等PersistenceMsg::FlushAndAck的rx.await；后者没有调用方期限。持久化writer仍持有I/O时不能简单取消、并发换writer或跳过ACK发布成功。此处是写入所有权与可见收尾状态的设计边界，不凭“没timeout”断言底层死锁。正常持久化owner在`persistence.rs:2287`写完后会回ACK。

普通`send_after_turn_event`虽然被await，但`workspace/src/handle.rs:574-589`实际只做本地activity计数和日志，不调用外部Hook或模型，应降低其优先级。SubagentStop gate另有受配置约束的Hook与有限continuations，不能与这项本地after-turn回调混为一谈。

后续文件快照若需有界收尾，应在Workspace文件状态owner明确“after-snapshot不可得”的语义，不能把没读到的内容当文件不存在或伪造可完整rewind。刷盘则须保留原writer/未确认位置，不套同一个读超时fallback。

## 发现三：有限恢复耗尽后会持续pending，查询不会重启恢复

**确定的状态行为，不自动列为违反现契约的缺陷。**

`tools/.../task/coordinator.rs:1846-1908`保留finalization_pending，以2秒recoverytick推进，每周期最多3次或30秒；`shell/.../subagent/finalization.rs:146-161`遇原output I/O仍在途、actor或persistence线程未退役就返回暂不能接管。`SessionThread::is_finished`（session/actor/spawn.rs:3562）同时检查actor和persistence，actor已退出本身还不够。

控制流反例：三次推进时旧owner都没退役，三次机会耗尽；稍后owner退出，但Coordinator不会仅因该事实给原pending重置attempt。`sweep_retiring`只清退役slot。List/GetStatus/Wait并不创建新恢复周期，只有新Turn准入的OpenSpawnAdmission（coordinator.rs:789-810）等既定恢复入口会开新周期。因此可以出现正文还在、pending长时间维持，反复查询没有变化。

这符合session-timeline“期限结束返回待修复、查询只读、未知mutation保留所有者”的当前契约。若改善自动收敛，先确定是否只让原周期内的readiness拒绝不消耗mutation/receipt次数，或在既有owner退出信号上唤醒剩余预算；跨周期自动续跑属于政策变更，不能无限刷新deadline或把未知写入再发一次。新父Turn也须仍通过实际准入/权限门，不能保证输入一定可立即接纳。

## 发现四：Result关闭Timeline后才发SessionEnd，顺序确实错误

**已有backlog的确定源码问题；不能单独认领Windows不递交的根因。**

- `handle_request.rs:2509`先durable child SubagentResult，再于2671发送Shutdown。
- `chat-state/src/timeline.rs:4079-4082`在subagent_result_recorded后拒绝所有新event。
- `session/actor/run_loop.rs:575-598`在Shutdown时无条件dispatch SessionEnd；`hook_dispatch.rs:766-774`即使handlers为空也先写Hook::Triggered。

所以在child Result已确认的正常路径，SessionEnd的第一项写入会被SubagentTimelineEnded拒绝，进入`session-end hook lifecycle was not durable`错误分支。它不是“Hook配得太慢”，也不是仅在Windows上成立。此前隔离child测试已记录同一错误，本轮只亲核控制流，不借旧日志冒充当前Windows证据。

后果是SessionEnd正常收尾未完成；不能据此声称canonical Result被回滚、父Ended一定写不成或必然永久挂住。父级提交走另一Timeline，该错误分支也会return；恢复中仍须独立检查planning/receipt与actor/persistence退役。修复owner应统一child终态与SessionEnd的持久顺序，让最后一项canonical Result真正是最后事实；不能放宽封闭Timeline保护或仅吞掉错误日志。此项保持独立修复边界，沿用backlog登记，不在本次分析顺带实施。

## 平台与未发现的直接原因

bigmodel/glm-5.3以默认effort独立检查了成功非空输出→write_immutable_blob_to_directory→Windows ContainedDirectory::write_atomic→child Result路径。父复核了关键实现：临时文件有READ/WRITE/DELETE权限及共享模式；sync_all使用可写句柄；无替换rename后遇AlreadyExists读回内容比对；artifact失败保留执行结果与pending，不捏造child Result。

在该有限路径没有发现直接的Windows API用法错误，也没有发现一个由本次下一版本提交引入的变动：`git diff --name-status v2.6.1 8ab486a6 --`针对handle_request.rs、subagent/mod.rs、session/persistence.rs、session/storage/mod.rs为空。这不是全部Windows行为正确性的证明，cfg(windows)测试在本轮未执行。

子报告称“文件链可排除、所有失败有界、30秒结果会递交”的措辞过强，父明确未接受这部分：timeout_at约束可继续调度的runtime上的caller等待，不取消spawn_blocking里的同步write/sync。worker仍可保留output_write_in_flight，recovery因原owner未退役而拒绝新writer，最终仍是pending。因而不能根据30秒wrapper排除I/O参与此现象，更不能把带pending的返回等同于canonical结果已递交。Windows目录sync为no-op是已有耐久性能力边界，也不是本轮新证实的活性缺陷。

当前尚未发现可据以认定“Windows专有结果提交算法错误”的证据。同步文件I/O、原进程/线程所有权、特殊文件和外部Hook均须按实际路径区分；不把杀毒软件、句柄锁或stdout管道当无依据的默认根因。

旧Released(None)迟到结果已有单独修复与验证，本次未见新证据支持将它认定为本Windows现场原因。当前模型协议切换和created_at修复也不能替代此收尾链分析。

## 后续验证边界

后续若实施，分别用“不回应Outstanding但保留sender”“一个after-read永不返回”“原I/O/线程在前三次检查后才退出”“正常child Result后请求SessionEnd”建立各owner的定向反例与对照。验收必须保留真实结果、费用、严格父子ref、未知写入所有权和只交付一次，不重跑历史provider/工具。当前交付到纯代码分析；没有新增测试/业务代码、Windows运行复现或线上根因声明。
