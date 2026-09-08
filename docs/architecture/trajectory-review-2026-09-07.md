# Trajectory 分类与诊断链路复审 · 2026-09-07

本轮继续基于 `87d1df4c3b0a36718427eb0db190fbf8a3c84098` 检查功能和架构，不涉及安全审计。下文保留修复前的发现和复现证据，行号对应审查时源码；用户随后授权二次分析并修复，当前修复状态与回归结果见文末。前一轮的 13 项发现见 [项目审查报告](/Users/lordcasser/workspace/projects/grow/docs/architecture/project-review-2026-09-07.md)。下面的编号从 F14 继续；架构建议与已验证问题分别列出。

新增 7 项有运行证据的问题：5 项属于 Trajectory 投影或页面，2 项属于离线 classifier 输入。另有 5 组架构建议。优先修复丢失失败诊断、关联范围和批量关系的问题；分类体系的整体调整单独处理。

范围是 Timeline → TrajectoryProjector → 多账本查询 → HTML 筛选/导航，以及相邻的离线 trace classifier。固定输入验证继续使用 `gpt-5.6-luna` 子代理，主审核对生产入口、运行输出和设计约束。这里的“分类”包括事件分类和诊断级别；涉及 LLM classifier 时只验证其输入构造，不推断某个模型必然返回何种分类。

## 已验证问题

### F14 · [P2] 失败 Hook 不进入问题筛选，已记录的耗时也未展示

位置：[trajectory.rs:912](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/trajectory.rs:912)、[event_outcome:1777](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/trajectory.rs:1777)。

`HookEvent::RunFinished` 的权威事件包含 `outcome` 和 `elapsed_ms`，但投影将所有结果映射为 `state="finished"`，耗时给 `None`；`event_outcome` 又没有处理 Hook。真实 `Timeline::record` 接受完整 Hook 生命周期后，使用已编译的 chat-state rlib 投影：

```text
输入：elapsed_ms=123，HookRunOutcome::Failed { message: "boom" }
输出：kind=hook.finished state=finished outcome=None duration_ms=None issue=None
```

失败说明仍保留在 summary 和 canonical details 中，但 Errors / Issues only 按 `issue_severity` 筛选，这条失败会被排除，overview 也不计入错误。[生产 finish_hook_handler](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/actor/hook_dispatch.rs:995) 确实写入这两个字段，因此这是读取投影丢失信息，不是执行端没有采集。

建议直接从 `HookRunOutcome` 映射诊断结果并保留 `elapsed_ms`，不要解析 summary 中的错误文字。执行失败与 Hook 主动阻止后续动作应分别规定诊断语义；本项实际复现的是 `Failed`，没有把所有 Block 都预设为错误。[复现代码、命令与输出](/tmp/grow-review-20260907/trajectory-core/result.md)。

### F15 · [P2] 暂停 Live tail 后，滚到底部无法载入新到事件

位置：[trajectory.html:909](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/trajectory.html:909)、[滚动入口:1057](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/trajectory.html:1057)。

页面原本位于最新窗口，`hasLater=false`。向上滚动会暂停 follow；此时没有选中行，且窗口非空。随后新事件到达，poll 收到更大的 `matchingCount`，却既不走更新窗口分支，也不走 `else if(selected)` 中的游标更新。旧窗口仍被标成没有后续页。

固定 240 行窗口、追加第 241 行，执行从页面提取的原始 `poll()`，得到 `hasLater=false loadLater_guard=false`。用户再滚到底部时，自动 `loadLater()` 被旧标志拦住；Jump to live 仍可恢复，但普通的向后翻页失效。DOM、fetch 与渲染副作用使用 stub，没有启动完整浏览器。

建议暂停状态继续保持当前行和滚动锚点，同时独立更新窗口是否还有前后页，不能把分页元数据刷新绑定到“是否选中事件”。[验证记录](/tmp/grow-review-20260907/trajectory-ui/result.md)。

### F16 · [P2] Pair 按裸 correlation ID 查询，混入其他账本的调用

位置：[scopeRelated:798](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/trajectory.html:798)、[后端筛选:637](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/trajectory.rs:637)。

点击 Pair 会清空 actor 等筛选，只传 `correlation=row.correlation_id`。后端在整棵合并树上比较裸字符串；与 entry ID 不同，这里没有 ledger 范围。

执行原始 `scopeRelated` 函数，并按后端相等谓词过滤包含 root/child 的固定行，`t:root/7` 和 `t:child/3` 使用同一个合法局部工具 ID `call_1`，查询同时返回两者。各 session 的 tool ID 校验不能提供跨账本唯一性。因此 Pair 可能展示无关子任务的调用和结果，干扰执行链路判断。

建议让关系查询同时携带 source ledger 与 correlation，必要时再带关系类型；不要仅靠碰撞概率较低的 provider ID。此项运行范围是原始页面动作与固定行上的查询谓词，未启动多 session 服务。[JS 复现](/tmp/grow-review-20260907/trajectory-ui/result.md)。

### F17 · [P2] 批量输入/通知被压成单一关联键，部分生命周期无法追踪

位置：[通知消费:1285](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/trajectory.rs:1285)、[输入消费:849](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/trajectory.rs:849)。

Notification 的 Received 使用各自 ID，批量 Consumed/Dismissed 却只保留第一个 ID。真实 chat-state rlib 接受两条 TaskCompleted 收据、启动内部 turn，再接受同一条批量 Consumed，结果：

```text
correlation=id1 → 2 行：Received + Consumed
correlation=id2 → 1 行：只有 Received
```

第二条通知已经消费，Pair 查询却找不到消费记录。Input 则把多个 ID 用逗号拼接；按 `input-a,input-b` 查询也无法匹配单独的 `input-a`、`input-b` 提交行。两者都是用一个字符串承载多对多关系导致的信息丢失，合并计为一个问题。

建议把 batch membership 保留为有界 relation IDs，查询判断成员关系并限定 ledger；不要把逗号解析当成新的 ID 规范。Notification 已通过真实 Timeline 校验和投影运行；Input 的同类行为按源码和固定 JS 谓词确认。[真实 API 输出](/tmp/grow-review-20260907/trajectory-cache/result.md)、[Input 固定用例](/tmp/grow-review-20260907/trajectory-ui/result.md)。

### F18 · [P2] 页面静默丢弃后端支持的精确分类筛选

位置：[restoreUrlState:413](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/trajectory.html:413)、[后端维度筛选:623](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/trajectory.rs:623)。

后端支持 `actor=subagent:child`、`producer=tool:read_file`、`layer=tool.result` 等精确值。页面下拉只有 family 选项，URL 恢复时要求参数值必须已经存在于选项中；不在其中就静默忽略，首次 API 请求也随之省略该筛选。

固定输入这三个精确条件，执行原始 `restoreUrlState()` 和 `urlStateParams()`，恢复结果为 `actor="" producer="" layer=""`，产生的筛选 query 为空。链接本想定位某个 child 的读文件结果，打开页面后却扩大为全体事件。这里使用 stub DOM 执行原函数，未启动完整浏览器。

建议让 URL/query state 独立于下拉选项；精确值可以显示为临时选项或筛选标签。后端提供分类值/族的元数据后，页面再据此展示可用选项。当前 producer 列表也遗漏已存在的 `sideband` 值，而 layer 有未见当前投影生成的 `plugin` 选项，进一步说明选项表容易漂移；这些不另计问题。[验证记录](/tmp/grow-review-20260907/trajectory-ui/result.md)。

### F19 · [P2，离线评估] 用户两次输入的间隔被当成当前 turn 耗时

位置：[compute_turn_elapsed_seconds:587](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/trace_classifier/mod.rs:587)、[classifier 提示约定:146](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/actor/laziness_classifier.rs:146)。

离线 replay 使用下一次 `turn_started_at` 减当前开始时间，把结果填入 `turn_elapsed_seconds`。两个开始时间相隔一天时，固定函数验证输出 `86400`，并以同名字段进入 runtime state。若当前任务只执行一分钟、用户隔天再回来，剩下的用户空闲时间也被计入了任务耗时。

源码与 classifier prompt 还将这个量称为任务耗时的 lower bound，声明它不会大于实际耗时；对于非重叠 turn，包含用户等待时间的间隔恰好可能大于任务执行时间。这会破坏离线“声称工作数小时，实际只工作数分钟”的核对依据。

建议读取当前 turn 的真实终态耗时；输入缺失时省略该字段，或明确标为另一种时间跨度，不能继续以实测 turn duration 命名。在线路径按 turn start 到 classifier fire 计算，不经过此函数，本项不把离线缺陷扩大到在线计时。[固定运行及调用链](/tmp/grow-review-20260907/trace-classifier/result.md)。

### F20 · [P2，离线评估] 后台任务的启动回执被当成任务结束

位置：[count_outstanding_dispatches:206](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/trace_classifier/mod.rs:206)。

replay 看到后台工具调用时加入 unresolved map，看到任何相同 call ID 的 ToolResult 就删除。可是 `run_terminal_command(is_background=true)` 的正常返回是后台任务启动确认，提供 task ID 后进程仍在运行；工具调用完成和后台任务结束是两个不同事实。[工具契约](/Users/lordcasser/workspace/projects/grow/crates/codegen/tools/src/implementations/grow_build/bash/mod.rs:1490) 明确描述这一行为。

固定输入后台 `sleep 600` 调用及 `backgrounded, task_id=task-123` 回执，计数函数输出 `terminal_only=0 terminal_plus_subagents=0`。这个零随后作为 runtime state 输入 classifier，其 prompt 会用“零后台任务”质疑已启动后台工作的陈述。这里验证的是构造了错误计数，未运行模型证明具体误判。

建议从 task 生命周期恢复状态；没有任务终态资料时保持未知，不能把 tool result 当 terminal receipt。在线 terminal 计数来自 live ToolBridge，本项范围仍为离线 replay。[固定运行输出](/tmp/grow-review-20260907/trace-classifier/result.md)。

## 分类架构建议

### B1 · 先定义各维度回答的问题，再收敛映射

当前分类来自 [dimensions](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/trajectory.rs:545)、[message_dimensions](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/trajectory.rs:718)、[workflow_row](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/trajectory.rs:2481)、[sideband_row](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/trajectory.rs:2914)，页面再使用字符串前缀解释这些值。建议保留现有事实来源，并把维度契约明确成：

| 维度 | 应回答的问题 | 需要避免的混用 |
| --- | --- | --- |
| actor | 这条事实属于哪个执行实体？ | 不把归属与实际生产者混为一谈 |
| producer | 谁产生了动作或内容？ | 工具名称、模型、core 与 actor 身份分开 |
| class | 这是内容、生命周期、治理还是观察记录？ | 不同时用它表示实体是主任务还是辅助任务 |
| kind | 发生了哪一种具体事件？ | 不从状态拼接后丢掉原事件区别 |
| state / outcome | 执行到哪一步、结果如何？ | “已完成一次调用”不等于“动作成功” |
| visibility | 是否在当前模型 Surface 中？ | LogOnly 不表示失败，也不表示事件不重要 |
| layer | 内容或上下文位于哪一层？ | 不承担失败判断或实体归属 |

两个具体的分类重叠值得整理：

- `ToolEvent::Started/Completed` 与模型 Surface 中的工具结果都归入 `class=message`，完成事件和结果消息还共享 `kind=tool.result`。前者是执行事实，后者是上下文内容；建议用 lifecycle/message 区分，保留原有 correlation 关系。这样统计执行次数与查看模型看到的结果不会混用同一集合。
- Sideband 的 request/attempt/result/end 全部归入 `class=auxiliary`，但 actor 已能表达 Sideband 身份。若 class 定义为事件职能，request/attempt/end 应是 lifecycle，result 是内容；“辅助任务”由 actor 维度表达即可。它们仍可保持 LogOnly，不需要把结果插回主对话。

这些是分类契约调整，不属于已复现的产品故障。应先列出所有 typed event 的映射表及筛选预期，再一次性更新投影和页面；无需新建事件账本，也不需要为显示分类扩散新的持久化实体。

### B2 · 从 typed event 一次生成分类、终态与摘要

[row](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/trajectory.rs:507) 先调用 `describe`，丢掉其前两个返回值，再调用 `dimensions` 和 `event_outcome`，最后把字符串交给 `trajectory_issue_severity`。同一个事件枚举分散到多份 match；增加事件时，即使每份 match 都能编译，也可能像 F14 一样因兜底分支而漏掉结果。

另一个源码可见的例子是 `InputEvent::Handled` 与 `Consumed`：describe 的事件标签原本分别是 handled/consumed，但两者 state 都是 consumed；经过上述转换后 kind 都成为 `input.consumed`。summary 和 visibility 仍能解释区别，所以不另计为完整信息丢失；但 kind 已不能单独回答“是进入模型上下文，还是在框架内处理完毕”。

建议用一个小的内部投影结果替代位置含义不清的长 tuple，在各事件族内同时确定 kind、state、outcome、duration 和摘要。诊断级别从 typed outcome 得出；展示字符串最后生成。避免为每个字符串字段都单独创建一套复杂类型层级。回归重点是完整事件矩阵，至少包含成功、执行失败、主动取消、重试、策略跳过及批量消费。

### B3 · 把关系身份与摘要文字分开

多账本视图的 `entry_id=t:<ledger>/<seq>` 已有明确作用域；关联查询也应遵守同样的身份原则。`correlation_id` 不适合同时承载单个工具调用、Hook occurrence/run、通知 ID 列表和输入 ID 列表，再统一做字符串相等查询。

建议先在现有查询中带上 ledger 范围；批量事件提供可匹配的 relation IDs，而不是逗号拼接或只保留首项。它们是从既有事件派生的查询索引，不是第二份权威事实。多个关系按钮可以根据事件已有的 typed refs 展示，不必建立泛化图数据库。

### B4 · 离线分类评估接回权威 Timeline，缺失的事实保持未知

当前 [trace classifier 输入](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/trace_classifier/mod.rs:33) 仍是 `afterStateHistory + turn_started_at` 的 JSON 数组；[grow trace 导出](/Users/lordcasser/workspace/projects/grow/crates/codegen/pager/src/trace_cmd.rs:37) 已是会话快照的 tar.gz。现有离线入口可用于明确提供该 JSON 格式的 fixture，但不能直接消费当前导出物，也没有从这份输入恢复运行时任务终态的依据。

建议让评估器消费现有 Timeline 的 turn 边界和任务生命周期，再复用生产 classifier 的输入组装。确实不能恢复的计数或时间输出为 unknown/缺省，不用推测值冒充运行时实测值。优先修正输入语义，再讨论分类准确率和阈值；否则离线评估容易把数据构造差异当成模型问题。不需要兼容旧格式时，可以直接收敛到当前 trace，而不是保留两套独立事实模型。

还应核对共享 prompt 的计数名称：它声称 `outstanding_background_tasks_and_subagents` 包含子代理，但 [在线采样函数](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/actor/laziness.rs:1038) 只统计 terminal tasks，源码也明确说明排除了子代理。最小收敛方式是让字段和提示准确说明 terminal-only；若确实要覆盖子代理，再接入已有生命周期读模型。该相邻语义问题仅做源码审查，不另计一次模型误判。

### B5 · 摘要的计算过程也应有界，而不只是最终字符串有界

[describe_message](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/trajectory.rs:1523) 先对所有 item 获取全文、收集并 join，最后只截取 240 个字符；[summarize_json_value](/Users/lordcasser/workspace/projects/grow/crates/codegen/chat-state/src/trajectory.rs:1752) 对数组/对象先完整序列化，再截取 120 个字符。最终 row 很小，但首次投影或新增长输出时仍可能产生与全文大小成正比的临时字符串。

可以复用 [workflow_result_preview](/Users/lordcasser/workspace/projects/grow/crates/codegen/shell/src/session/trajectory.rs:2458) 的现有思路：结构化大值只概括字段或元素数量，消息摘要按字符预算逐段提取，预算用完就停止拼接。canonical details 继续从原账本读取。这是有源码依据的计算量优化，尚未做完整服务器的性能测量，不计为功能缺陷；也不要误将仅用于显式完整 snapshot 的 payload hydration 当成每次列表轮询路径。

## 验证边界与排除项

- 本轮不重新运行整仓测试，也不启动 Cargo 构建；复用先前编译产物和提取的原始函数做有界验证。前一轮的 3,731 项测试结果不能替代这里的边界用例。
- F14 和 F17 的 Notification 用例经过真实 chat-state rlib 的 Timeline 校验与投影；F15/F18 执行原始页面函数，DOM/网络/渲染使用 stub；F16 执行原始关联动作并核对后端筛选谓词。没有运行完整浏览器与多 session HTTP 服务。
- F19/F20 使用提取的时间计算、dispatch 计数逻辑及真实 `ConversationItem` 类型，省略无关日志和未使用的数据字段；进入 classifier request 的调用链由源码核对，没有构建完整 Shell 或调用模型。
- 本轮新验证目录合计约 17 MiB。结束时磁盘剩余约 17 GiB，共享 `target` 约 22 GB；未运行 Cargo、未清理或复制共享缓存。审查期间工作区另有四个源码文件发生并行修改（session lifecycle、其测试、subagent request、worktree），本轮未修改这些文件，也不将这些后续改动计为已审查范围；本报告引用的 Trajectory/classifier 文件未发生改动。
- overview 的横轴按 arrival order 做等宽事件分桶，时间戳只用于范围和详情。这是明确的设计，晚到的旧时间戳出现在尾部不算排序错误。
- append 快路径只校验已提交边界附近的 64 KiB。对同一文件原地改写更早内容再追加，确实可以绕过历史重读，但该场景违反权威 writer 的 append-only 约定；未发现正常追加下对应的缓存错误，不计入功能发现。
- LLM 分类验证只证明输入字段与真实含义不一致，不声称已运行真实模型并观察到某种误判。

## 二次分析与修复记录

| 范围 | 当前实现 |
| --- | --- |
| F14 | Hook 完成事件保留 elapsed_ms，直接从 HookRunOutcome 映射 outcome；Failed/TimedOut 进入错误筛选。主动 Block 沿用独立结果语义。 |
| F15 | 暂停追尾且未选中事件时仍更新分页元数据，能向后载入新到事件。 |
| F16 | `source` 限定账本；Pair 使用 `related_to=<entry_id>`，查询在该账本内按完整关系集合求交集。Turn/Step 定位同步携带 source。 |
| F17 | 保留 batch 的全部成员关系。列表最多传输 16 个 relation_ids，并提供 relation_count；后端查询使用完整索引，不受摘要截断影响。 |
| F18 | URL 恢复保留精确 actor/producer/layer 值；筛选展示、清除、URL 持久化采用同一状态。 |
| F19、F20 | 离线回放只使用显式 turn_duration_ms 和 outstanding_background_tasks。缺失即未知，不从相邻输入时间或工具回执推断；在线继续传入实时测量，prompt 明确后台计数仅含 terminal tasks。 |

Trajectory 页面由并行任务“优化Trajectory页面交互”负责，当前任务负责 Rust 投影和查询；双方共同确认 source、related_to、relation_ids 和 relation_count 的接口契约，避免交叉覆盖同一文件。页面测试使用实际 Chrome 和 mock HTTP fixture；Rust 测试另行验证真实 Timeline 和缓存查询，两者的覆盖范围分别记录。

分类器验证固定输入及实际构造的 ConversationRequest，不调用真实模型，也不据此宣称分类准确率提高。仓库没有发现生成 afterStateHistory 格式的 exporter；未带测量字段的外部 trace 会保留未知值。补采集端测量应在对应 exporter 内独立实现，不能用推断值填补。B1/B4 的分类元数据集中化及更大范围重构继续拆分记录。

修复后验证：

- chat-state 全套 461 项通过，覆盖 Hook outcome/耗时投影及 batch 成员保留。
- Shell Trajectory 40 项通过，覆盖真实缓存查询、跨账本 source、相关查询冲突、完整 17 成员关系与 16 项摘要界限。[日志](/tmp/grow-review-20260907/fix-shell-trajectory-tests.log)。
- 离线 trace_classifier 27 项、在线 laziness 83 项定向测试通过；这些数字按测试命令计数。[回放日志](/tmp/grow-review-20260907/fix-shell-trace_classifier-tests.log)、[在线日志](/tmp/grow-review-20260907/fix-shell-laziness-tests.log)。
- 真实 Chrome 的 8 组交互回归通过：分类切换、精确 URL 筛选、Turn/Step 0 定位、键盘操作、超摘要上限的跨账本 batch 关联、暂停后加载新增事件、空结果、响应式/主题/抽屉焦点边界。[脚本](/Users/lordcasser/workspace/projects/grow/scripts/test_trajectory_ui.cjs)、[日志](/tmp/grow-review-20260907/fix-trajectory-ui-tests.log)。

```sh
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline \
  -p shell --lib --quiet trajectory -- --test-threads=2
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline \
  -p shell --lib --quiet trace_classifier -- --test-threads=2
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline \
  -p shell --lib --quiet laziness -- --test-threads=2
NODE_PATH=/tmp/grow-trajectory-qa/node_modules \
  CHROME_BIN='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' \
  node scripts/test_trajectory_ui.cjs
```
