# Acceptance

本文定义验收要求，不是已通过的测试报告。下表每行对应 delta spec 中一个同名场景；实际测试、失败和覆盖限制记录于 [verification.md](verification.md)，没有对应运行证据的场景不得视为通过。

验证层次：U＝纯状态/差异表测试；I＝actor/coordinator/inbox 集成；F＝写入、ACK、退出与乱序故障注入；E＝mock provider 加实际 runtime/headless 或 Pager 的端到端验证。实现时依据真实 harness 选择最小可证明层次，不用只重复实现逻辑的测试代替行为证据。

公共断言：同时检查持久身份、可重建投影、模型输入与实际执行计数。逻辑去重不等于物理请求绝不重试；不可用/未知必须明确保留。等待测试检查 provider 请求数和 actor idle 调度数，防止“没采样但 host 忙循环”。冻结场景必须在 `suspend-session-tree` 可用后进行，不能用 mock 读写列表声称整树恢复成功。

## Local planning and weak links

规范：[task-planning](specs/task-planning/spec.md)。

| 场景 | 层次 | 输入/竞争与关键断言 | 状态 |
| --- | --- | --- | --- |
| TP-01 | I | 父一项→child 三项；核对父/子 owner、稳定引用、父写不能覆盖子项 | 证据与范围见 verification.md |
| TP-02 | I | 补充消息和新 turn；范围不自动切换，旧摘要不覆盖新计划 | 证据与范围见 verification.md |
| TP-03 | E | 无 TODO 的简单任务/等待；可正常执行，不隐式建 Goal/计划 | 证据与范围见 verification.md |
| TP-04 | I | 同一父项两次并行执行加一次重试；结果身份和处置独立 | 证据与范围见 verification.md |
| TP-05 | I/F | 用 barrier 交换 spawn 与父项提交顺序；新快照或补发责任至少满足其一 | 证据与范围见 verification.md |
| TP-06 | U/I | 分别改内容、验收要求和优先级；只通知直接关联活跃 child | 证据与范围见 verification.md |
| TP-07 | U/I | 状态完成、内容修改+完成两个输入；触发次数分别为零和一次逻辑责任 | 证据与范围见 verification.md |
| TP-08 | U/I | pending/in_progress/cancelled/reopen 状态差异表；重复同值无事件，取消不杀执行 | 证据与范围见 verification.md |
| TP-09 | U/I | 相同字段、排序、UI、用量、验收/回执更新；无额外重规划或模型调用 | 证据与范围见 verification.md |
| TP-10 | E | 已完成调查后新增测试要求；保留调查，调整受影响项，确认来源准确 | 证据与范围见 verification.md |
| TP-11 | I/E | 本地计划已覆盖变更与无 TODO 两种情形；no-change 有理由，无伪修改和下游广播 | 证据与范围见 verification.md |
| TP-12 | I/E | 可控长命令和 task_output 等待中改父项；命令不自动取消，等待可让出，迟到结果仍交付 | 证据与范围见 verification.md |
| TP-13 | E | 收到变化后需用户信息；未处理保持，用户等待不被读回执自动关闭 | 证据与范围见 verification.md |
| TP-14 | I/F | 消费前连续三次修改、存在旧回执的两种顺序；未投递合并，已接收按准确上下文消费，覆盖确认可恢复 | 证据与范围见 verification.md |
| TP-15 | I/F | 处理 A 时收到 B，A 确认乱序/重复；B 不丢、不退回 A | 证据与范围见 verification.md |
| TP-16 | U/I | replace 省略活跃关联项、随后复用 ID；保留 tombstone/责任，身份复用拒绝 | 证据与范围见 verification.md |
| TP-17 | I/F | 编辑与 child 完成/取消竞争；仅记录未应用，结果保留，child spawn/启动计数不增加 | 证据与范围见 verification.md |
| TP-18 | I/E | 三层树，child 实际改项和 no-change 两分支；只沿被改项的直接边传播且无循环 | 证据与范围见 verification.md |
| TP-19 | I | 有旧执行/等待时换范围；拒绝直接覆盖，显式处置后才允许切换 | 证据与范围见 verification.md |

## Waiting and acceptance

规范：[task-planning](specs/task-planning/spec.md)。

| 场景 | 层次 | 输入/竞争与关键断言 | 状态 |
| --- | --- | --- | --- |
| TP-20 | I/F | 结果在注册前/中/后到达；不丢唤醒，any/all 对成功与非成功状态正确求值 | 证据与范围见 verification.md |
| TP-21 | U/I | 缺失 ID、越权、自等待、祖先环；立即明确拒绝，无悬空等待 | 证据与范围见 verification.md |
| TP-22 | I/E | child 失败/取消、命令 Interrupted/OutcomeUnknown、产物丢失；唤醒决策但不自动重试/成功 | 证据与范围见 verification.md |
| TP-23 | E | child 等 grandchild，结束一个模型 turn；child task 仍未完成，原 owner 继续并最终交付 | 证据与范围见 verification.md |
| TP-24 | E | 只有正文“等待测试”；不制造持续执行，未完成 TODO 展示真实 | 证据与范围见 verification.md |
| TP-25 | I/E | Stop、预算或 max-turns 与唤醒竞争；只收尾，不重置限制或新增 provider 请求 | 证据与范围见 verification.md |
| TP-26 | I/E | 必需 child 成功交付；父 TODO 不自动完成，验收引用 canonical result | 证据与范围见 verification.md |
| TP-27 | I/E | 改交付要求后原结果仍符合；显式复核可接受，不要求新版本结果/重跑 | 证据与范围见 verification.md |
| TP-28 | I | 必需交付需修订与无关局部未勾选并存；前者未闭合，后者不自动全局阻塞 | 证据与范围见 verification.md |
| TP-29 | I/E | 多执行弃用其中一个或删除父项；有理由处置等待/义务，活执行 owner 不消失 | 证据与范围见 verification.md |
| TP-30 | E | 独立 worktree 结果尚未集成到目标目录；不能宣告目标交付完成，不自动 merge | 证据与范围见 verification.md |
| TP-31 | I/E | 无 Goal 的 child 连续 yield/wakeup 直到累计限制；原执行 owner 计数和恢复不清零，不新建无限续跑 | 证据与范围见 verification.md |
| TP-32 | I/E | 父项取消/移除时命令在途、通知暂不可达；执行仍可能继续须可见，送达后先评估再派新副作用 | 证据与范围见 verification.md |

## Durability, delivery and restoration

规范：[session-timeline](specs/session-timeline/spec.md)。

| 场景 | 层次 | 输入/竞争与关键断言 | 状态 |
| --- | --- | --- | --- |
| ST-01 | F | 父项提交后、发送前退出；合法恢复可补交付，同一来源只提交一次 | 证据与范围见 verification.md |
| ST-02 | F | 来源写失败和 ACK 丢失；不提前成功/通知，未知时 exact 核对 | 证据与范围见 verification.md |
| ST-03 | I/F | 篡旧 Resources/UI 派生缓存；Timeline 重建胜出，不再造变更 | 证据与范围见 verification.md |
| ST-04 | F/E | Received/Consumed/Handled 各边界合法冻结；Consumed 后通过工作投影继续，不重复注入消息 | 证据与范围见 verification.md |
| ST-05 | F | child 处理事实已提交、父 ACK 丢失；核对原确认，较新来源责任仍在 | 证据与范围见 verification.md |
| ST-06 | I/E | root→child→grandchild，root 观察生命周期；模型结果只交给直接父级，真实内部/Workflow 路径回归 | 证据与范围见 verification.md |
| ST-07 | I/F | waiter 完成与 steering/通知并发、ACK 重试；同 call 一份 tool_result，回执一次逻辑消费 | 证据与范围见 verification.md |
| ST-08 | F | artifact 缺失、链接冲突、父接纳失败；终态与提交/可用性/交付各自真实 | 证据与范围见 verification.md |
| ST-09 | E | 三层树有长命令、未处理变化和待验收结果时冻结/退出/resume；原身份和责任恢复，命令不盲重放 | 证据与范围见 verification.md |
| ST-10 | I/E | child 挂起中发生父变化，恢复时仍有 paused Goal/awaitUser/权限/预算限制；责任不丢且准入不开 | 证据与范围见 verification.md |
| ST-11 | I/E | 无 checkpoint 的 crash、fork、rewind、resume_from、export/replay；不据旧计划启动工作 | 证据与范围见 verification.md |
| ST-12 | I/F | 清理遇到待投递、未处理、待验收引用；必要资源保留，义务收敛后不永久占用 | 证据与范围见 verification.md |
| ST-13 | I/F | inbox/内存/存储达到容量；有界反压，已提交责任不丢，未提交不报成功 | 证据与范围见 verification.md |
| ST-14 | I/F | A 已 Received，B 在消费边界前/后到达；A immutable，确认只能覆盖实际已消费集合 | 证据与范围见 verification.md |
| ST-15 | I/F | Consumed 未 Handled 后正常终态/确认冷崩溃/旧实例不明；分别未应用或未确认，不复活或造终态 | 证据与范围见 verification.md |
| ST-16 | I/E | child 验收 grandchild 后或提前结束；最终结果携带局部验收/未闭合摘要，root 无重复模型回执 | 证据与范围见 verification.md |

## Goal and authority

规范：[behavior-goal](specs/behavior-goal/spec.md)、[tool-authorization](specs/tool-authorization/spec.md)。

| 场景 | 层次 | 输入/竞争与关键断言 | 状态 |
| --- | --- | --- | --- |
| BG-01 | I/E | Active Goal 只有等待，模拟长时间无状态变化；采样与 idle 重排不增长，不累计 blocked turn | 证据与范围见 verification.md |
| BG-02 | I/E | 一步等 child、一步可做；允许独立推进；无计划时原 Goal 审计仍可用 | 证据与范围见 verification.md |
| BG-03 | I/F | 结果到达同时控制/用户输入/权限等待已入队；原优先级生效，边界重新检查 | 证据与范围见 verification.md |
| BG-04 | I/E | TODO 全勾选但必需执行/交付/复核未闭合；complete 拒绝并返回具体引用 | 证据与范围见 verification.md |
| BG-05 | I/E | 全目标达成且必需义务闭合，child 无关探索项未勾；不机械拒绝，原执行收尾仍负责 | 证据与范围见 verification.md |
| BG-06 | I | TODO 修改不改 Goal id/revision/预算；真正 Goal revision 改变仍隔离旧结果唤醒 | 证据与范围见 verification.md |
| BG-07 | I/E | 已审批 Plan 的细化与越界两组输入；审批与 Goal/Plan 互斥保持 | 证据与范围见 verification.md |
| BG-08 | I/E | root Goal→普通 Task child→grandchild；逐级验收摘要可核对，child 无 Goal 修改权，root 不扫描所有复选框 | 证据与范围见 verification.md |
| BG-09 | I/F | Goal Paused/预算关闭/真实用户或权限等待时收到变化和结果；只持久保存，采样/批准计数为零 | 证据与范围见 verification.md |
| TA-01 | U/I | 伪造 owner、sibling/历史步骤引用；调用拒绝且无状态副作用 | 证据与范围见 verification.md |
| TA-02 | U/I | 未消费/其他 child 的确认、缺失结果引用；不接受伪确认或伪验收 | 证据与范围见 verification.md |
| TA-03 | I/E | 父项变化要求 read-only child 写文件/跑越权命令；必须经过原 Gate，无新授权 | 证据与范围见 verification.md |
| TA-04 | I | 普通 Send 到终态/Workflow child，内部变化到挂起 child；既有准入限制和来源待投递各自保持 | 证据与范围见 verification.md |

## Presentation and context

规范：[client-surfaces](specs/client-surfaces/spec.md)、[context-compaction](specs/context-compaction/spec.md)。

| 场景 | 层次 | 输入/竞争与关键断言 | 状态 |
| --- | --- | --- | --- |
| CS-01 | E | turn 结束仍 in_progress/等待；live、headless、resume 均无假 completed | 证据与范围见 verification.md |
| CS-02 | I/E | 通用 ACP 缺少等待枚举；保守 in_progress，Grow 稳定身份不丢 | 证据与范围见 verification.md |
| CS-03 | E | 同父项活跃与终态 child；待处理、未应用、待验收状态和旧结果均可查看 | 证据与范围见 verification.md |
| CS-04 | E | 在导出树中浏览同名步骤与嵌套 child；不混 owner，无 provider/工具/通知消费 | 证据与范围见 verification.md |
| CC-01 | I | 大树完成历史+单一必需等待；有界局部上下文保留引用，不逐轮注入整树 | 证据与范围见 verification.md |
| CC-02 | I | fork/summary 含父 TODO；child 当前投影与权限仍独立 | 证据与范围见 verification.md |
| CC-03 | I/F | 压缩快照后到达父更新/结果；发布时保留新责任，不被旧 summary 覆盖 | 证据与范围见 verification.md |
| CC-04 | I/F | 压缩包含 Consumed 未 Handled 的消息；处理责任与内容引用保留，不再次消费 | 证据与范围见 verification.md |

## Regression boundaries

实现后的回归应覆盖既有 Goal definition fence、budget 与用户输入优先级；agent message exact receipt 与来源保持；child result finalization/late receipt 重试；Workflow journal 和 Plan 审批；资源只读观察及冻结协议。已有测试只提供回归入口，不能拿它们替代本 change 新增的端到端断言。正式验证记录需列出实际执行命令与失败/跳过项，冻结依赖未完成时不得归档本 change。
