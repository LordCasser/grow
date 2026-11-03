# Fork 后的多 session 管理审查

审查日期：2026-11-02。工作区 HEAD：`bbd2c4dfa`；本机安装版：`grow 2.5.5 (5998735)`。本次涉及的删除、Dashboard close/stop、fork failure、quit gate 核心分支在安装 commit 与 HEAD 间没有差异。已有 child 历史恢复改动保留；审查期间另一个 change `fix-suspended-project-rules-resume` 正在修改恢复来源验证，本记录不把它计为本次修复。

用户报告可以复现。问题分布在三层：持久 session 的删除资格、actor 的运行所有权、Pager 页面/列表的本地状态。fork 的来源关系、subagent 的结果依赖、页面关闭和执行结束目前没有在所有入口保持相同语义。

## 截图对应证据

只读检查本机 summary 和 Timeline 结构，未向真实 session 发送取消、关闭、恢复或删除请求。下列 CLI 删除验证只操作临时 Grow home 内的完整副本，验证结束后删除副本。

| 截图条目 | 身份与观察 | 本次能支持的结论 |
| --- | --- | --- |
| SDK Implementation Audit | `01a19a05-5281-7201-bae3-89f599512353`，普通 root，包含 subagent 生命周期 | 它确有结果链保护，不能把所有删除拒绝都当成 fork bug。 |
| Add Gall's Law… | `01a19ba9-e3b1-7b71-a385-8f4cb89da44a`，`session_kind=fork`，parent 为上面的 SDK session；自身没有 subagent 生命周期事实 | 来源 parent 不能据此解释成它自己的 subagent 结果依赖；独立小 fixture 复现 A1。真实副本先在读取预算处失败，见 A2。 |
| Count external library dependencies… | `01a17620-05df-7851-b5d9-59539cb31d9f`，无 summary parent，最后保留 `suspend_requested → commands_settled → checkpoint → tree_suspended` | 临时副本删除明确失败于 `unfinished runtime handoff`，属于恢复保护，见 G1。Inactive 不等于“没有持久恢复责任”。 |

没有捕获截图时刻的输入事件和 actor 内存快照，不能从截图反推某一次按键是否已发送 RPC。以下分别标明执行复现与源码确认。

## 用户旅程

| 用户动作 | 当前路径 | 审查结果 |
| --- | --- | --- |
| 普通 fork | 新 AgentId → 复制独立 Timeline → 绑定新 session → LoadSession | 历史属于新 lineage；summary 仍保存来源 parent。复制失败不回收占位页（A8）。 |
| worktree fork | CreateWorktreeSession → worktree/fork → LoadSession | 同样写入来源 parent，受 A1/A2 删除入口影响。未运行真实 Git worktree 端到端验证。 |
| 切换、重复打开本地 session | `focus_if_session_already_open` 按持久 session id 聚焦现有 AgentView | 已有去重路径，不应为了修复 close 再创建第二套 session 注册。 |
| Dashboard Working 行 Ctrl+X | 分类后调用 `stop_top_level_activity` | 活跃 child/Goal 可让行 Working，却没有可用 stop effect（A5）。 |
| Dashboard Idle/Inactive 行删除 | 两次 Ctrl+X、两次点击或列表 y 确认 → DeleteSession | 确认期重核对 busy 状态已有测试；持久删除仍可能被 A1/A2/G1 拒绝。 |
| 详情关闭 | DashboardOverlayStop → `dispatch_sessions_confirm_close` | 只移除本地 view；最后一页被拒绝（A7）；close 后退出所有权判断会漏掉原 root（A6）。 |
| `/home` | unregister active-session crash marker → Welcome | 导航动作，不停止 actor，也不从 `app.agents` 移除页面。单独使用不等于结束 session。 |
| 欢迎页历史列表删除 | DeleteSession，`after=Stay` | 成功回执只清列表，可能留下同 id 的本地 view（A4）。 |
| Ctrl+D 两次退出 | Quit → 本地 ownership gate → SuspendForExit → 服务端 drain | 既有挂起、重试、force 路径存在；A6 可绕开入口。 |
| 挂起后恢复 | 原 identity 恢复树，验证 checkpoint/来源后释放执行 | 删除拒绝不能通过删 checkpoint、移除完整性核对或递归 rm 解决。当前另有独立恢复 change。 |

## 确认的问题

### A1 · P1：普通 fork 的来源 parent 被当作不可删除的结果链

`shell/src/session/fork.rs::fork_session` 总会把来源 id 放入 `CopySessionOptions.parent_session_id`，`jsonl/mod.rs:4958` 保存它。`reject_linked_single_session_delete` 在 `jsonl/mod.rs:3085` 无条件将 summary parent 加入 linked 集合，没有先证明 canonical Spawn/Seed/Result 关系。

隔离 fixture 仅含一条普通历史，没有任何 Subagent/Seed 事件。fork 后删除仍返回 `InvalidInput: ... linked to ... test-session-123`。这超出了主规范“Explicit deletion preserves cross-session result sources”所保护的 canonical 结果链。worktree fork 使用相同来源字段，也经过这个入口。

修复切口：区分来源谱系与执行/结果依赖，以实际 canonical 关系判定删除资格；保留真正的 child 证据保护。不能删掉所有 parent 校验来绕过。

### A2 · P1：观察用途的固定读取预算成为显式删除的规模上限

`jsonl/mod.rs:318` 的 `bounded_validated_timeline` 明确用于 read-only/maintenance，预算为 16 MiB、50,000 events、4,096 references。显式删除在 `:3082` 和 `:3001` 复用该入口；sampling 部分还按每个去重 chunk 的 64 KiB 最坏情况预留预算（`:395`）。普通 loader 使用另一条不受该观察预算限制的校验路径。

Gall's Law 的临时完整副本执行安装版 `grow sessions delete <id>`，exit 1：`sampling evidence exceeded its bounded read budget`。后续只读快照中 Timeline 约 12.69 MB，126 个 sampling chunk 声明约 6.52 MB，最坏预留约 8.26 MB。会话正常累积历史就能越过这个删除入口的固定预算；“重新打开后删除”也不改变历史规模。

修复切口：为显式删除定义能够处理正常长期历史的依赖核对方式，保持有界资源使用与 writer 所有权；不靠全局调大常量、跳过证据验证或直接删除目录。

### A3 · P1：删除被拒绝前，服务端已经终止 actor，Pager 未协调失败后的页面

`extensions/session_admin.rs:1863` 先调用 `teardown_live_session_before_delete`，随后 `:1870` 才核对并删除存储。前者会 Cancel、Shutdown、kill process scope、drain writer、移除 resident 与广播 roster removed。A1、A2、G1 或存储错误都可能让后一步失败。

Pager 的 `task_result.rs:1944` 对 DeleteSessionFailed 仅显示 toast。`acp_handler/settings.rs::handle_sessions_changed` 的 removed 只改 leader roster，不处理本地 `app.agents`。因此一次失败删除能留下指向已终止 actor 的本地会话页；再次 resume 同 id 还会被 `focus_if_session_already_open` 当成已打开而只聚焦，无法靠这个入口重新加载。

这是源码确认的失败顺序与状态收敛缺陷；本轮没有向真实 resident 执行删除来制造此状态。既有 teardown 测试确认了前半段确实关闭 owner。

修复切口：在已有 lifecycle transaction 内协调可预知拒绝与 teardown；一旦已经结束 owner，即使后续磁盘操作失败，也必须向客户端发布可恢复的准确状态。不能让删除先于 writer retirement。

### A4 · P1：欢迎页删除成功也可能留下已删除的本地会话

`/home` 保留 `app.agents`。欢迎页历史列表删除使用 `AfterSessionDelete::Stay`，而 `task_result.rs:1879` 在该分支清完 picker/roster 就直接 return，跳过后面按 session id 回收本地 AgentView 的公共逻辑。

触发路径：打开 A → `/home` → 在历史列表删除 A → 打开 Dashboard 或再次打开 A。磁盘删除成功，页面仍可持有 A 的旧身份；已有本地去重也会阻止重新加载。隔离 TaskResult 探针已复现本地绑定保留；未运行真实 TUI 输入到磁盘的端到端探针。

修复切口：同 id 的本地状态退休与删除后的导航目的分开处理；Stay 只决定留在当前 surface。

### A5 · P1：Working 分类覆盖了后台执行，停止动作没有覆盖

`AgentActivityProjection::working` 包含 child、workflow、active Goal；`classify_top_level` 使用该投影。但 `dashboard.rs:1923` 的 stop helper 仅在 foreground state 非 idle 时发送 CancelTurn，另外处理 bg tasks、scheduled tasks 和本地排队输入。

两个隔离探针分别构造 foreground Idle + running child、foreground Idle + active Goal。两者均显示 Working，Ctrl+X 返回空 effect，不 arm delete。用户会不断收到“先停止”，却无法通过这个管理入口停止。运行中 workflow 具有相同的静态覆盖缺口，本轮未单独执行 workflow 探针。普通 turn 的该路径还设置 `pause_goal=false`，不能把它当成停止 Goal 的动作。

修复切口：停止动作与当前 Working 的真实来源对齐，复用现有取消/Goal 控制与确认流程；不要把分类强改成 Idle 以开放删除。

### A6 · P1：关闭页面移除了退出所有权证据，但没有结束服务端 session

`session/modal.rs:74` 的 close 仅返回 UnregisterActiveSession 并移除 AgentView；这个 effect 只改 crash marker，不调用现成的 `grow/session/close` 或执行树挂起。roster 不会因此退休，列表重建时还可把该 id 作为 roster-only session 显示。隔离 roster 探针复现了关闭后仍有同 id 行。

更严重的是，`router.rs:1414` 只根据仍存在的页面及 `attached_as_viewer` 判断是否需要 SuspendForExit。隔离探针保留一个只读 viewer，再关闭唯一 owned root 的页面，随后 Quit 只得到 `[UnregisterActiveSession(viewer), Quit]`，没有挂起请求。服务端原 root 没有被 close effect 终止，这破坏了退出前确认所有 owned root 的契约。

`attached_as_viewer` 本身还随每轮消息来源变化（`acp_handler/mod.rs:237`），不是稳定的 process ownership 账本。这里不主张新增第二套所有权系统，应先复用现有 runtime/连接 owner；不能用“页面不在了”证明工作已结算。leader 与 in-process 最终后果可能不同，本轮只复现 Pager 发出的错误 effect，未声称已经观察到真实数据损失。

### A7 · P2：Dashboard 无法关闭最后一个本地 session

`session/modal.rs:57` 因 `app.agents.len()==1` 无条件拒绝 close，理由是没有可返回的页面。但调用方已经回到 Dashboard，界面有 New Agent 和 Welcome；`/home` 也只是导航，不释放这最后一个 view。

隔离探针从只有一个 root 的详情执行确认关闭，页面仍留在 map。已有 `close_only_agent_is_refused_with_toast` 测试把旧限制固定为预期，不能把既有测试通过当成旅程闭环。

修复切口：明确最后一页关闭后的落点，并与 A6 一起定义“关闭”究竟是 detach、挂起还是结束执行；不要继续把它隐藏在仅数量相关的拒绝里。

### A8 · P2：fork 复制失败留下无 session identity 的占位页

`session/fork.rs:709` 的 ForkSessionFailed 只清进度、结束 command 并写失败行，没有回收创建的 placeholder 或恢复父页面。已有 `fork_session_failed_pushes_turn_failed_block` 明确断言 placeholder 保留；它没有覆盖之后的关闭/删除旅程。

用户留在一个 fork 失败页面，删除因没有 session id 被拒绝，只有一个本地 view 时还会遇到 A7。若复制已经成功而 LoadSession 失败，还应分别核对保存副本与显示占位页的责任，不能把所有失败一律当作“磁盘上没有新 session”。本轮只对复制 RPC 失败做源码及既有测试确认。

修复切口：保存发起 surface，按复制和加载的实际提交边界回收临时页面、保留应保留的历史并给出重试路径。

## 保护行为与待验证边界

### G1 · 挂起 checkpoint 受保护，但删除入口没有给出终止恢复责任的闭环

Count external library 的临时副本返回 `session has an unfinished runtime handoff; resolve it before deleting history`。这是 `jsonl/mod.rs:3003` 的明确护栏，既有测试覆盖未消费 checkpoint 拒绝和 ExecutionReleased 后可删除，符合恢复依赖保护规范。

缺口在产品入口：Inactive 行仍展示删除能力，用户收到“resolve”却没有在删除流程中得到检查/明确终止原工作的方法。真实 subagent 结果链也被规范明确禁止单条删除，现有入口只建议等待 TTL。后续必须独立定义显式放弃挂起/关联树的行为契约，不能在本审查中删事实或递归删除会话来“修好按钮”。

### R1 · leader 多客户端退出的作用域仍需专项端到端验证

`SuspendForExit` RPC 只带 exit_id。`runtime_handoff.rs::start_exit_suspension` 遍历 MvpAgent 的全部 resident roots；leader 转发只对 new/load 注入 client 身份，不对该退出请求建立 root 集合。服务端还有 agent 级的共享 exit_suspend slot。需要用两个独立客户端核对一个客户端退出时，另一个客户端 root 的准入、checkpoint、deadline 与重试 owner。当前代码没有提供充分的客户端作用域证据；本轮没有运行双客户端退出，不将潜在后果计为已复现缺陷。

## 修复顺序

先处理 A6 的退出所有权，以及 A3/A4 的删除状态收敛；再分别修 A1 的来源关系和 A2 的长期历史删除路径。A5 复用现有控制入口补齐停止。A7/A8 是页面生命周期闭环。G1 与 R1 需要各自的行为设计/专项验证，不和当前审计混成统一 session 管理框架。

所有问题尚未在本 change 修复。本次只保存审查和验证证据。


## 后续实施结果（2026-11-02）

上文保存审查时的事实。其后逐项建立 change、复现、实施并验证，当前工作区的状态如下；没有对本机真实 session 发送删除请求。

| 问题 | 现有工作区结果与验证记录 |
| --- | --- |
| Ctrl+X 导引失效 | [首按配置前缀、第二按停止/关闭、子页与切换边界](../2026-11-02-fix-agent-command-prefix-routing/verification.md)已修复。 |
| A1 | [只按 canonical 结果引用保护删除](../2026-11-02-allow-independent-fork-session-deletion/verification.md)已修复。 |
| A2 | [显式删除独立验证，不受维护观察预算阻塞](../2026-11-02-delete-session-without-maintenance-observation-budget/verification.md)已修复。 |
| A3/A4 | [preflight/teardown/writer 归属与成功/失败页面收敛](../2026-11-02-reconcile-session-deletion-with-runtime-and-views/verification.md)已修复，保留晚拒绝历史和草稿，不移除替换 binding。 |
| A5 | [普通 child、Goal、Workflow 与后台工作停止](../2026-11-02-fix-dashboard-background-stop/verification.md)已修复，等待权威状态更新后才开放删除。 |
| A6/A7 | [真实 runtime close ACK 后关闭，允许最后一页](../2026-11-02-close-dashboard-sessions-through-runtime/verification.md)，[退出由 runtime/leader owner 确认](../2026-11-02-scope-exit-suspension-to-runtime-owners/verification.md)已修复。 |
| A8 | [空页回来源、保留失败草稿](../2026-11-02-retire-empty-failed-fork-placeholders/verification.md)；[来源先关与复制后首次 replay 失败](../2026-11-02-preserve-fork-failure-identity-after-source-close/verification.md)已修复。 |
| G1 | [显式永久删除可以放弃独立本地 continuation](../2026-11-02-allow-explicit-discard-of-independent-suspended-history/verification.md)已实现；TTL 和真实跨 session 结果保护保留。 |
| R1 | [leader 从现有 session_driver 注入可信 scope，各 client 独立挂起 owner](../2026-11-02-scope-exit-suspension-to-runtime-owners/verification.md)已修复；并发 backend/leader 用例验证，不冒充双终端手工观测。 |
| 加载页导航 | [replay 期间保留 Dashboard/前后 session/attached frame 导航，后台失败不抢焦点](../2026-11-02-allow-session-navigation-during-replay/verification.md)已修复。 |
| 导引与真实组合路径 | [后台停止文案和实际 PTY 菜单/fork/cycle/close](../2026-11-02-clarify-session-stop-command-guidance/verification.md)已校正并验证。 |

最终 Pager 定向回归：Dashboard 679（2 ignored）、fork 55、load 67、modal 17、delete 59、close 114、Ctrl+X 19、exit 44、quit 38；这些过滤有重叠，不作为唯一测试数相加。Shell storage 159、公开删除故障注入、leader/socket/挂起测试见各增量记录。真实终端组合路径、运行中 Ctrl+D 挂起后新进程继续、stdio close 与永久 delete 使用最终构建独立验证。

验证边界：没有安装或替换全局 grow；现有运行进程不会自动获得修改。默认 jemalloc 的本机依赖编译失败（generated header），保留 sandbox 的系统 allocator 构建通过。worktree 失败通过实际 dispatch/回执验证，未新增真实 Git worktree 成功路径端到端测试。恢复来源漂移/同进程 restore retry 等独立架构事项仍见 [恢复准入方案](../2026-11-02-analyze-session-recovery-admission/design.md) 与 [backlog](../../../backlog.md)，不通过削弱 source/writer/出生权限 gate 绕过。
