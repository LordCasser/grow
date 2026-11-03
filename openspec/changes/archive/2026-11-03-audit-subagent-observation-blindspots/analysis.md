# 子 Agent 状态观察的同类问题审查

上一轮修复解决了模型工具对原 child 的历史结果查询，但没有覆盖全部观察入口和中断分支。本轮又确认了等待事实失真、总期限未覆盖完整链路、终态投影不一致，以及取消和聚合观察的缺口。以下是分析结果，尚未实施这些修复。

12 个临时诊断探针已运行；其中两个探针重复核对同一个 closed-channel 条件，不按两个缺陷计数。探针确认的是当前反例，不是“产品正确性测试通过”。时间反例均使用 Tokio 虚拟时钟，不是线上耗时或性能基准。方法、边界和结果见 [verification](verification.md)。

## 1. 同一子任务的不同观察入口仍会给出不同结果

| 路径 | 当前实现 | 本轮证据 |
| --- | --- | --- |
| 模型 `get_task_output` / 统一输出工具 | AgentRebuildSpec 为 ChannelBackend 绑定 history_query，缓存缺项后可读已授权的 durable 结果 | 上轮真实会话探针及本轮调用链核对 |
| ACP `grow/subagent/get` | MvpAgent::query_subagent 和 inspect_subagent 各自构造未绑定 history_query 的 ChannelBackend；query 遇到 Unauthorized 就返回，不再核对持久来源 | 静态调用链确认；未执行带历史链的真实 ACP 冷恢复往返 |
| Pager `grow/queue/prompt_status`，没有 promptId | 先执行 read_durable_subagent_lifecycle；这个读取失败会在 runtime snapshot 分支之前返回 | 对原会话 child 的生产 reader 实测得到 WouldBlock：sampling evidence exceeded its bounded read budget |
| 已绑定 history_query，但 coordinator 通道关闭 | ChannelBackend::query 在发送失败时直接 Unavailable；后备 reader 不执行 | 两个等价探针均观察到 Unavailable、history_calls=0 |

ACP 的缺项仍可能表现为 Unauthorized；Pager 的失败则是 Unavailable。不能将这些入口笼统称作同一条已修复通道。Pager 当前实际调用 prompt_status，没有发现它调用 grow/subagent/get 或 list_running。

Pager 路径的关键不是把 16 MiB 额度调大：状态 reader 先验证 child 的整个 bounded Timeline，包括无关采样产物，再验证 parent。原会话已经在这个采样证据预算处被挡住；即使有 live handle，该分支也先于 runtime snapshot。预算耗尽被正确归类为不可用，没有伪造任务终态，但真实状态仍可能无法核对。

来源：`shell/src/session/agent_rebuild.rs::build_agent_inner`；`shell/src/agent/mvp_agent/agent_ops.rs:967-995`；`shell/src/extensions/task.rs:433-470`；`shell/src/extensions/session_admin.rs:499-506,627-658,756`；`tools/src/implementations/grow_build/task/backend.rs:475-490`。这些路径位于 `crates/codegen/`。

**修复边界**：让模型与 ACP 的 parent-owned 查询复用相同来源授权和 canonical 结果读取。状态查询只核对所需事实与引用，保留只读、原期限、root/direct-parent 和隐藏 Workflow 边界；诊断/维护接口自身的预算不因此取消。coordinator 失联时能否返回已验证历史终态需明确处理，未结束任务仍保持未知，不以失联推断死亡。

## 2. 等待返回被误写成任务完成，等待时长也不真实

真实 coordinator 下，一个保持运行的 child，请求 `timeout_ms=120000`，在虚拟时间 **25 秒**返回 Running，正文却说 `Waited the requested 120s`。

原因是工具按配置 cap 预先生成 WaitHint::Elapsed，而 ChannelBackend 把一次 coordinator wait 限制为 25 秒，外加最多五秒响应余量。工具展示的 waited 不是实际测量，也不是后端实际采用的期限。

两个保持运行的 child 更明显：同样在 **25 秒**返回 `0/2 tasks completed (wait_all)`，两项正文却都声称 `another finished`。wait_all_event_driven 忽略每个 query 的 Result 和 snapshot，只要所有查询 future 返回就使用 CompletedEarly。查询到期、观察失败都能被写成“另一项完成”，不是完成事件才会进入这条分支。

来源：`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs:95,178-184,557-577`；`task/backend.rs:84-100`；`task/coordinator.rs::process_deadlines`。

**修复边界**：统一工具承诺与 backend 实际等待策略；区分终态、等待到期、观察失败和外部中断。等待提示必须来自真实原因与时长。不能靠重复声称“自动通知会到达”掩盖提前返回，也不能简单延长控制查询期限。

## 3. 内层五秒期限没有覆盖整个公共调用

| 反例 | 已观察行为 | 期限漏在哪一层 |
| --- | --- | --- |
| 非阻塞查询，terminal actor 的 get_task 不回复，subagent backend 可用 | 六秒时工具仍未返回，尚未发送任何 subagent query | run_single_task 先等待 terminal；backend 的五秒计时尚未开始 |
| 两个 ID，请求等待一秒，coordinator 接收请求但不回任何 query | 整个工具调用耗时 **21 秒**，最后两项均 observation_timed_out | 初次 resolve 逐项花十秒，随后等待一秒，再逐项花十秒复核；整体 deadline 在初次 resolve 之后才建立 |
| ACP 非阻塞 get，Query 和 Inspect 各延迟四秒 | 公共 extension handler 在 **8 秒**后成功返回 | 两个步骤各自有五秒期限，没有共同的 RPC 总期限 |

这三个探针穿过公共工具或 extension handler；没有实际睡眠，也没有启动命令或模型。TerminalBackend 的生产 get_task / wait_for_completion 本身是 mailbox 发送和 response 等待，没有把这些等待纳入 subagent backend 的期限。

来源：`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs:188-195,303-337,428`；`tools/src/computer/local/terminal.rs:2731-2774`；`shell/src/extensions/task.rs:437-445`。

**修复边界**：在公开观察入口明确一次调用的总期限和等待预算，覆盖 terminal 探测、排队、各 ID、来源读取和元数据读取。保留底层尚未退出的 worker/owner，不用超时伪造取消。批量等待的 timeout_ms 是否只表示纯等待时间，也必须与总观察期限区分说明；当前不能宣称传入的一秒会限制整个调用。

## 4. 终态事实、结果可读性和完成计数没有统一

生产 formatter 将 CompletedOutputUnavailable 投影为 `output_unavailable`，有 ended，表示 canonical 完成但正文不能读取。然而 multi summary 的 is_terminal_status 漏掉它。一个 output_unavailable 和一个 failed 的公共工具结果会显示 **1/2 tasks completed**，两项实际都有终态。

通用 `TaskOutputResult::is_terminal` 同时漏掉 output_unavailable 和命令的 timed_out；反方向，`TaskOutputOutput::is_terminal` 对任何 MultiResult 都返回 true，即使两项都是 Running、summary 是 0/2。探针也确认了这个错配。当前仓内没有找到这些通用 helper 被生产完成控制路径使用，不能据此宣称它们已经导致调度错误；multi summary 的矛盾则直接进入模型上下文。

来源：`crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs:343-365,726-743`；`crates/common/tool-types/src/task.rs:472-492`；命令 timed_out 的产生位于 `crates/codegen/tools/src/implementations/task_output/tool.rs::snapshot_to_result`。

**修复边界**：用一处实际任务终态规则生成计数和通用判定；已结束与正文可读性分开，不把 output_unavailable 改成成功正文或“仍运行”。正式回归需同时验证单项、混合列表、全部 Running、timed_out 和 observation_*。

## 5. 中断等待的合成结果绕过了观察不确定性

Shell 的 interrupted_wait_tool_result 没有读取任务状态，只要参数带 task ID 就固定产生 Running，并宣称任务仍在运行。对 `[child-a, child-b]`，它只取第一个 ID，构造单项 Result，第二个 ID 不进入结构化结果。

这条路径由用户或 parent intervention 打断等待时触发。中断只说明停止等待，不能证明目标此刻仍在执行。正常 run_tool 完成在 select 中有优先级，这是已有的正确保护；但如果结果尚在查询/传输而目标已结束，不能用这个优先级证明任务仍运行。现有测试反而固定断言了 `keeps_task_running`。

本轮探针确认了 helper 的合成内容和身份丢失；尚未跑真实 SessionActor 中“任务 terminal 与 intervention 交错”的完整组合，因此不把该竞争事故当作已经线上复现。

来源：`crates/codegen/shell/src/session/actor/tool/result.rs:105-145`；`tool/mod.rs:466-508,5656`。

**修复边界**：只对等待操作报告中断；保留所有被查询身份和已确认事实，未知项不合成为 Running。继续保留 completion_delivery.defer_wait 的收据所有权转移，不重复消费完成通知。

## 6. 采样器仍有一段不监听取消

上一轮已解开 UI gateway ACK 与本地采样的依赖，但 `signals_handle.snapshot().await` 在取消 select 之外。故障探针让 signals actor 保留 receiver 但不处理请求：publisher 到达这个等待点后，即使取消 token，也仍持有 progress owner。只有释放 signals actor、让 reply channel 关闭，它才退出。

本轮没有观察到用户实际 signals actor 停答。这里确认的是该失败条件下的取消缺口，不是对真实会话内存增长的归因。runner 的 drop guard 只发取消信号，不能使这段 await 自行返回。

来源：`crates/codegen/shell/src/agent/subagent/mod.rs:2482-2494`；`handle_request.rs` 的 progress_cancellation / _progress_lifetime。

**修复边界**：在同一 sampler owner 内让取消覆盖 signals 请求等待，保留“一项 UI 交付在途”和独立 child token，不增加新 watchdog、重试墙或生命周期缓存。

## 7. 列表的一项未知会遮住其他已知项

真实 coordinator 内同时保留 healthy 和 unknown-progress child。healthy 单项 query 正常返回，list_running 却整体返回 Unavailable。原因是 active_inspection 对缺样本返回 Err，列表使用 collect<Result<Vec<_>,_>>，一项失败丢掉其余成功观察。

这是已复现的可用性边界，现有契约未承诺 partial list，不能直接认定为违反“必须返回部分成功”的要求。ListRunning 不包含 pending/suspended 也符合其当前范围，不应把它当全任务清单。若调用方需要重连时看见所有仍有责任的身份，应先明确列表范围和未知项表达。

来源：`crates/codegen/tools/src/implementations/grow_build/task/coordinator/query.rs:333-355`；`backend.rs::list_running`。当前 Pager 没有调用该接口的证据。

## 已核对但没有得到同类错误终态的路径

- blocking waiter 的 child identity 在 pending/active/suspended 转换、完成、期限和 receiver drop 中保留；当前源码与既有测试没有给出“等候方离开被当成任务完成”的反例。
- finalization 与 physical retirement 的 owner 不随普通 completed cache 淘汰；这次没有得到丢失持久收尾责任或重复执行 child 的反例。
- Unavailable/TimedOut/InvalidEvidence 的普通工具投影仍保持 observation_*，不再直接标 Running；问题主要在合成中断结果和外围入口。
- 未发现查询来源检查被本轮反例绕过。history reader 的 root/direct-parent 与隐藏 Workflow 校验仍在，不能为提高可用性放开这些约束。

## 后续验证缺口

以下不计为本轮已运行反例：

| 边界 | 还缺什么证据 |
| --- | --- |
| ACP 冷恢复读取原历史 child | 合法 Spawn/Seed/Ended/Result fixture 经真实 extension 往返，与模型入口比较；当前只有调用链证据 |
| Pager 的物理展示/取消收敛 | 从真实 prompt_status 响应到 UI reducer 的场景；本轮只复现生产 reader 拒绝，没有跑 PTY |
| 中断与已写 terminal 竞争 | 控制原工具结果传输、intervention 和 receipt ACK 的真实 SessionActor 测试 |
| 已完成任务的输出 worker 饱和 | queue_completed_output_read 的 try_acquire 失败目前会返回 Unavailable，可能连已确认终态一起遮住；需独立故障注入，不能只靠静态分支归因 |
| history query 等待源 ChatState ACK | 期限后仍排队的 GetTimelineEvents、重复查询的源快照克隆与资源释放；当前代码会先读完整来源事实，尚无积压上界测量 |
| 进程异常、Windows 和实际 signals 停答 | 本轮不提供 crash/power-loss/跨平台结论，也不把受控 actor 停答当线上故障证据 |

修复顺序宜先补入口一致性与完整期限，再统一等待/终态投影，随后处理中断与采样 owner。每项分别建立行为 change，把这里的反例转为断言正确行为的回归；本轮不引入统一状态服务或第二份生命周期账本。
