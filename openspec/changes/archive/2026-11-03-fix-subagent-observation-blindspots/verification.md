# 验证记录

工作树包含其他任务并行改动；本次不清理其代码、规范或在用构建产物。HEAD 基线 bbd2c4dfa，验证针对当前工作树。复用共享 target。

## 输出期限与终态

- `cargo test --locked -p tools --lib task_output -- --test-threads=4`：68 passed。包括真实 coordinator 单/双运行 child 的120秒虚拟时钟等待、terminal 停答五秒、两ID一秒调用、输出不可读与失败均计入完成，以及既有公共输出回归。
- 同次产出的 tools test binary 执行 `cancelled_wait --test-threads=1`：2 passed。验证取消单个/所有等待者后的 block_waited 与自动完成唤醒所有权。
- `cargo test --locked -p tool-types --lib is_terminal -- --test-threads=2`：3 passed，覆盖公共单项/列表终态规则。

等待已改为公共调用直接拥有 join_all 的各项 future，取消时直接释放等待回复通道，无额外 spawn 的遗留消费者。已确认的每项结果留在其 future 结果中，其他项到期不覆盖成功项；不再进行期限后的重新查询。

时间测试使用 Tokio 虚拟时钟，不是线上耗时基准。

## 来源与查询入口

- `cargo test --locked -p tools --lib channel_backend -- --test-threads=4`：21 passed，覆盖断链、无注册、剩余原期限和原查询/取消边界。
- `cargo test --locked -p shell --lib observation -- --test-threads=1`：17 passed、1 ignored（既有长历史测量）。包含真实文件的 ACP 冷历史 public handler 往返、其他来源拒绝、损坏 Result 链拒绝、单次 inspection 与五秒总期限；Pager reader 的大于17MiB parent事实及错误Result链接也通过。
- 首次新夹具编译时修正了 canonical/runtime SubagentEvent 同名和临时请求借用；之后被另一任务新增的 compact_flush_journey 测试两个 PathBuf→&Path 错误阻塞。仅为该调用增加两个 `&`，未改变其行为。
- 只读对抗复核确认 reference reader 保留精确来源、Seed/Spawn/Ended/Result、notification owner/settlement 验证；source ChatState存在但不回复仍按原期限未知，不扩大fallback猜测。

## 新补充的运行中幽灵现象

用户确认仍为原会话。本次只读查阅其父/child Timeline，没有修改或恢复业务任务。F9-2-P `01a1a096-3e34-7632-bce5-8d3e4494ff20` 在父 journal seq20523（at_ms1793698741293）被主模型称为已清理；child在此后仍记录request/tool执行，包括run_terminal_command成功，直到at_ms1793698869666才写SubagentResult.cancelled，父seq20659随后写Ended。由此确认认知与实际执行不一致，不能把父消息当终态事实。

该child在近期没有实际send拒绝记录；seq20204的no_direct_route是发给已终态的旧TS child，不能冒充P路由故障。当前证据指向一个独立发现缺口：模型普通输出工具要求已知ID，只在压缩时注入active roster，不能用普通工具实时枚举来纠正丢失/错误的记忆。嵌套child的root可读、direct parent才可发送是既有权限，不以提高可达性放开。

## active 列表

- `cargo test --locked -p tools --lib list_running -- --test-threads=4`：1 passed，真实coordinator同时含healthy/unknown/foreign/Workflow/finalization项，验证保留已授权active身份、不造计数、不泄露其他项。
- `cargo test --locked -p shell --lib extensions::task::tests -- --test-threads=4`：21 passed。列表复用get的DTO，unknown/finalization均可投影，移除原只解构Running的panic分支。

## 主模型发现与消息可达性

- `cargo test --locked -p tools --lib discover -- --test-threads=4`：149 passed（filter也匹配既有skills discovery测试）。相关新回归穿过真实 coordinator + public TaskOutputTool：空task_ids重新发现healthy/unknown child，排除foreign/Workflow；发现ID后direct parent Send成功。root与direct parent可查询nested child，只有direct parent可发送，root拒绝正文明确未确认completion/cleanup。
- 发现整体五秒期限含资源锁；停答Terminal夹具验证不扫描共享命令；断链发现返回错误而非空清单；空清单的公共is_terminal为false。
- `cargo test --locked -p tool-types --lib -- --test-threads=4`：128 passed，含输入schema、描述别名映射、新发现说明、终态和空库存规则。
- 不声称已经复现“活跃coordinator注册丢失”；若来源活跃表不可达，发现保持不可用，不能从磁盘开放Spawn反推活跃执行或恢复消息路由。

## 中断投影与通知责任

- `cargo test --locked -p shell --lib wait_interrupt_tests -- --test-threads=4`：8 passed，覆盖单目标、多目标、纯等待、取消scope与中断缓冲。
- 同次产出的Shell test binary执行 `completion_delivery --test-threads=4`：8 passed，覆盖completion先于defer、正常结果不重复、延迟队列顺序、重复事件和turn取消责任。
- `run_tool` 优先的biased select和defer_wait先于丢等待future的次序保持；不把这组helper/tracker验证描述为完整真实SessionActor intervention/terminal竞争的端到端覆盖。

## 采样取消

- `cargo test --locked -p shell --lib progress_publisher_tests -- --test-threads=4`：5 passed，停答signals actor仍持有receiver时，cancel后Weak progress owner已释放；既有单项gateway ACK停答与本地样本持续更新也通过。
- 最后只读审查发现等待ID边界：执行已用公共resolve_task_ids去除两侧空格/去重，原tracker和中断投影没有。现复用同一公共解析器，保留原tracker排序；新增“执行→wait reservation→completion”身份回归，并让中断单/多ID用例包含空格与重复输入。不是新增任意净化规则。

## 最终回归与边界

- 最新tools test binary的 `implementations::grow_build::task --test-threads=4`：216 passed。第一次扩展运行出现一个测试responder的退出竞争：done oneshot与所有sender释放同时ready时将正常None误判为意外事件；修正仅测试的正常channel关闭分支后全组通过，没有改生产查询来掩盖失败。
- `cargo test --locked -p shell --lib completion_delivery -- --test-threads=4`：9 passed（含身份解析新增回归）。同次新产物重跑 wait_interrupt_tests 8、public ACP observation 3、durable history 2、task extension DTO 21，全部通过。
- 最终代码在共享工作树编译，无业务模型请求、外部收信人消息、业务进程重启或原会话持久化修改。源码修复需要更新运行进程后生效，不以本次library测试宣称现有旧进程已热更新。
- `git diff --check` 通过；归档前 `openspec validate --all --strict --no-interactive` 为24 passed、0 failed。原来其他图片change的严格校验失败已由并行工作消除，本次未删除它们的规范场景。
- 已结清原六条确认问题backlog，保留真实SessionActor terminal/intervention/receipt交错、source ACK积压、worker饱和、异常进程/跨平台及真正注册丢失的验证边界；不宣称这些已全部复现或修复。
- 共用target的在用编译缓存由并行任务共享，未执行全量cargo clean或删除他人的构建产物；临时源码备份在验证后移除。

## 归档

`openspec archive fix-subagent-observation-blindspots --yes` 已归档并合入session-timeline的6项requirement；tasks全部完成。归档后 `openspec validate --all --strict --no-interactive` 为23 passed、0 failed，`openspec validate --archived --strict --no-interactive` 为814 passed、0 failed；`git diff --check`通过。开发说明与backlog归档链接已核对。
