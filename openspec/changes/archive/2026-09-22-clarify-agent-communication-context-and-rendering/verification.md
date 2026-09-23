# 方案核对记录

日期：2026-09-22。

以下前三轮是批准实施前的历史设计记录。用户随后明确同意方案并要求实现；第四轮记录实际代码与验收，不以早期示意验证替代运行时测试。

## Evidence

- 已读取 `openspec/README.md`、`local-coordination` 主规范及 `session-timeline` / `client-surfaces` 相关要求，核对 active changes 和已有 backlog。
- 直接读取并对照 inquiry tool → peer/delegation → SessionActor → InfoRequest Sideband → tool-result，以及父消息 receipt → pending → Consumed → Surface。
- 核对 `SubagentProgress` 的 transient UI 路径、`SubagentCompleted` 的 receipt 交付，以及已被工具结果消费时的 `input: None` 去重边界。
- 已用只读 subagent 核对 Pager；主 agent 再核实实际路径及关键实现，正确 source-tool projection 位于 `pager/src/acp/tracker.rs`。
- Atlas 仅使用 scoped Focus；部分符号 exploration 返回 `symbol_not_materialized` / `closure_boundary`。调用关系结论由目标源码进一步核对，不宣称取得完整全仓调用图。
- Markdown 缺口在 source / receiving / parent receipt 的 plain text block 和 `BlockViewerPane::for_plain_text` 路由；整块 copy 缺口与 viewer 内已有 selection copy 分开记录。
- 本轮没有确认新的后端行为偏差；已知 consumed Surface validation 问题归既有 `fix-sideband-consumed-surface-validation`。passive row 与普通工具耦合已有 backlog，本次不扩展处理。

## Validation

- `openspec validate --all --strict --no-interactive`：通过，21 项通过、0 项失败，包含本 change。
- 文档空白和本地 Markdown 链接检查：7 个 Markdown 文件通过，0 个错误。

第一轮未执行 Rust 测试、真实进程测试或 TUI 视觉验收。现有测试名称是后续验证入口，不是本轮通过记录。

## 第二轮：TUI 内容、交互与 Markdown 设计

按用户要求使用项目内 UI UX Pro Max skill，执行两次 design-system 和四次 UX 检索，实际命令、结果与适用性判断见 [ux-research.md](ux-research.md)。以当前终端主题和键盘路由为约束，新增 [tui-design.md](tui-design.md)，同步 proposal、design、delta specs 和实施 tasks。

新增源码核对：

- 复核 transcript fold/open、双击分支、viewer raw/close，以及 ListPane 搜索、wrap、选择和复制的实际入口；保留 Tab 全局焦点动作。
- 复核既有 Markdown parser、table-width、Mermaid 有界终端图及降级能力，不新增 parser 或依赖。
- `MarkdownContent::new_inner` 会展开 tab，`text()` 并非精确接收原文；方案要求 raw/copy 取 typed 原始正文，避免显示解析破坏保真。
- 区分 API 返回与模型工具结果、投递模式与动态消费状态；收件事实不能在历史回放中变成“等待消费”。

交互示意只包含静态样例及本地视图切换，不连接 Session 或发起 agent 操作。正文中的 Markdown 示例是手工对应的展示，用于评估内容层级；不是实际 Rust renderer 的输出。

### 实际验证

- 浏览器使用本机 Chrome 的临时 headless profile，经 Playwright 加载沙箱 iframe；未使用用户浏览器会话。首次尝试因 bundled Chromium 未安装失败，随后改用已有 Chrome，不安装额外浏览器。
- 320 / 560 / 780 px 外部宽度 × 40 / 60 / 100 示意列宽，共 9 组无外层横向溢出；额外核对窄屏详情中的代码和表格。
- 已回答的答案预览、等待/失败状态、行内展开、详情进入与退出、正文/原文/数据切换、`e` / `Enter` / `r` / `D` / `Esc` 均通过断言。原文示例保留实际 tab 和 fence，数据视图保留 demo identity；父消息详情明确消费状态未提供。
- 首轮交互检查发现 host state 回传导致重复重绘、返回后焦点丢失；修正相同状态不重绘后重新验证，退出恢复原记录焦点。最终浏览器 page error 为 0。
- 已查看 [默认列表](tui-preview-transcript.png)、[正文详情](tui-preview-reader.png)、[窄屏列表](tui-preview-narrow.png)、[窄屏详情](tui-preview-narrow-reader.png)，覆盖深浅色、层级和换行。图中列宽是设计参数，不是终端 cell-width 验证。
- `openspec validate --all --strict --no-interactive`：第二轮当前工作树 22 项通过、0 项失败，包含本 change。条目数量反映当前其他并行变更状态，与第一轮记录分别保留。
- 文档空白与本地 Markdown 链接检查：本 change 的 9 个 Markdown 文件通过，0 个错误。

本轮仍未实现 Rust 代码，未执行实际 Ratatui/PTY、provider request 回归、系统剪贴板或无色终端验收；相关内容保留为 tasks 与验收矩阵，不能以浏览器示意代替。临时 standalone QA wrapper 在验证后移除，保留示意源及四张检查截图。


## 第三轮：简洁英文 UI、接收回执与意见交换

用户确认移除“目标主对话不变 / 下步投递”等常驻机制文案，固定 UI 使用英文并参考 Grow 全局风格；随后明确 ACK 只确认接收，双方意见需要进入上下文。本轮据此重写有效方案，第二轮的路由行、tabs、Y 快捷键决定已被替代，前述历史验证记录不代表这些旧决定继续有效。

- 使用 UI UX Pro Max 追加 design-system / UX 查询；拒绝不适用的网页 hero/CTA/大字体建议。luna xhigh subagent 只读核对 Pager，主 agent 复核默认 `◆`、两格缩进、原工具 header、BlockViewer 边框/[x]、ShortcutsBar；示意配色改为实际 GrowNight/GrowDay 中性灰基底。
- Atlas 打开当前项目后做 scoped search；部分查询无匹配，未以此声称无调用。目标源码确认了真实 receipt ID 被发送 adapter 丢弃、Pager 根据 error String 判断 unknown、active gate 早于底层去重，以及 portable history 丢弃孤立 ToolResult。
- 新增 receipt-design.md 和 exchange-design.md。ACK 复用 Received，正式回复使用普通 message，ask 仍是独立 Sideband；第二阶段的 canonical agent-message / 配对工具结果 / 反向 reply 都明确为待实施契约。
- 用户要求改变常规 UI 的投递模式展示，因此 client-surfaces 使用 MODIFIED delta 更新已有要求。首次 strict 校验指出缺少原 scenario 名称；保留完整场景并修正后通过，没有删除原安全或恢复场景规避校验。
- 与并行 prompt 实施任务沟通确认：本任务仅改本 change 目录，没有修改其 prompt/Rust/docs 文件，没有运行 Cargo 或清理共享 target。

### 第三轮实际验证

- 最新交互示意 320/560/780 px × 40/60/100 示意列宽共 9 组无外层溢出；深浅色截图人工查看，当前四张截图已更新为本轮版本。
- 英文固定文本、无 Sideband/next-step 机制行、无常驻 tabs/每行详情按钮均通过检查。
- e 展开、Enter 详情、r 原文、D 数据/返回、Esc 恢复焦点、Waiting/Answered、Sending/Received/Unconfirmed、真实示例 receipt 字段、原文 tab/fence、表格与列表检查通过；浏览器 page error 为 0。
- `openspec validate --all --strict --no-interactive`：21 项通过、0 项失败。当前数量随其他 change 独立归档变化，不改写前两轮的历史记录。
- 本 change 的 13 个 Markdown 文件完成本地链接和尾随空白检查，0 个错误。

浏览器只加载本地设计示意，不连接 agent、不测试真实消息交付。特殊结果的 provider 支持、reply 权限、ACK 故障注入、真实 Ratatui/PTY 与 compaction 坐标均未实施/未运行，tasks 未勾选。最终仍是方案交付，不归档；临时 QA wrapper 验证后移除。


## 第四轮：批准后的实现与运行验证

2026-09-22，用户明确授权写 OpenSpec 并实现完整方案。保留工作树中已有的 prompt、采样恢复、工具权限和其他 active changes，不将其整体归入本变更。

### 已落实的实现

- 保留实际工具名。ask 继续使用 frozen、tool-free Sideband；send 的正式意见使用目标 Notification inbox。新增 `reply_to`，只允许当前 Session 实际收到消息的反向回复，不开放任意 peer 发送。
- `received` 返回真实 durable receipt；`rejected` / `unconfirmed` 使用 typed error。取消后的 send 保留回执接收通道，由 Shell 完成有界查证，不重发。completed child 路由只读核验，关闭目标不打开 writer。
- canonical `AgentMessage` 随 `Notification::Consumed` 原子写入 Surface。Chat Completions、Responses、Messages 生成完整 `receive_agent_message` 历史调用/结果；不注册可执行工具、不增加模型 tool count、不附人类权限证据。轨迹、压缩、token 估算和 portable history 均保留来源。
- Pager 的问、答、消息和原因分别使用现有 Markdown renderer；默认有界预览，英文状态；`r` / `D` 与既有导航、选区和复制入口共存。raw 正文保留 tab/CRLF，Data 单独查看。读取过程中延迟替换选区内容，终态不强制跳到底部；approval-only metadata 更新也刷新原记录。

### 验证环境与边界

Rust 使用独立 `/tmp/grow-agent-communication-target`，3 个构建任务、关闭 incremental 和 debug symbols，测试线程栈 16 MiB。没有清理共享 target。编译器为本机实际可用工具链，验证未声称使用仓库声明以外的指定版本。

真实进程 fixture 使用隔离 GROW_HOME、临时 Git workspace 和 loopback SSE provider，不调用付费或远端模型。request adapter 单测覆盖三类 endpoint；真实父子往返本轮实际走 Chat Completions。未做远端供应商验收、真实系统剪贴板或新的人工 PTY 视觉验收；前述浏览器截图只是设计示意。

### 场景对应

| 契约 | 实际验证入口 |
| --- | --- |
| ask 对双方上下文的边界、繁忙前台、冷恢复和独立进程 | `scripts/test_local_coordination.py`；Shell coordination / recap display-only 回归 |
| 父子 send / reply、真实 receipt、只读 child、后续请求可见且无重复 | `scripts/test_agent_messages.py`；`agent_opinions_reach_both_requests_once_without_ack_loops` |
| Received ACK 屏障、失败/丢失 ACK、精确幂等与冲突 | ChatState `durable_agent_message_received_is_hidden_until_ack`、`durable_agent_message_receive_retries_lost_and_failed_ack_without_duplicate_receipts` 及 Timeline agent-message validation |
| 发送端 ToolResult 保存失败/ACK 丢失 | `source_tool_result_ack_loss_does_not_redeliver_received_message`：重试同一持久事件，目标 receipt 不增加，直到本端 durable ACK 才暴露结果 |
| ACK/cancel 优先、结束 source 只读、非接收者拒绝 | Tools coordinator/backend tests；Shell `durable_ack_wins_ready_cancellation_but_absence_stays_unknown`、`reply_authority_requires_own_receipt_and_exact_original_sender` |
| inactive 旧回执、暂停收件不唤醒 | `message_receipt_retry_survives_inactive_target_without_new_admission`、`stopped_session_accepts_reply_without_waking_or_interrupting` |
| 原子消费、部分组提交恢复、稳定 Surface 坐标 | Timeline `agent_message_consumption_is_exact_atomic_and_one_surface_coordinate`；Shell `mixed_message_and_task_receipts_resume_after_partial_group_commit` |
| 三类 wire 配对、portable/native 边界、压缩、统计与原文 | sampling-types conversation tests；ChatState compaction_utils / state / trajectory / counts tests |
| 通信冷恢复不依赖 updates cache | Shell storage `communication_receipt_replay_uses_timeline_without_updates_cache`；Pager coordination replay / cursor reload tests |
| Markdown、40/60/100 列、GrowNight/GrowDay/NO_COLOR、原文 | Pager communication body tests，单独 `NO_COLOR=1` 进程执行 |
| 同长度更新、Data-only 更新、选区冻结及终态不跳尾 | `views::block_viewer::tests::communication_*`；coordination approval 原位更新 |
| Minimal 和不会被前台完成提前提交的 inquiry | pager-minimal 整包回归 |

最终执行结果与独立发现见下方收尾记录。


### 最终执行结果

- `cargo check --locked -p shell -p pager -p pager-minimal -p cli`：通过。
- `cargo build --locked -p cli --bin grow`：通过。Apple linker 仅提示大型测试/CLI 的 unwind table 性能警告，无链接失败。
- sampling-types：293 passed。
- chat-state：最终整包 516 passed、1 ignored。新发送端故障测试最初错误地从未完成历史调用启动，触发既有恢复修补；改为实际 live send 接纳后注入 failed/lost ACK，测试通过，未改变生产恢复规则。
- tools：最终整包 2,361 passed、7 ignored；包含 ACK/cancel 与 completed reply 只读路由。新增 backend fixture 最初未绑定 Session，等待不存在的事件；绑定真实 source 并增加 bounded wait 后通过。
- agent：477 passed。通信说明压缩到原 prompt 字节预算内，未提高预算断言。
- shell：整包 3,880 passed、3 ignored、1 个新混合通知 fixture 因遗漏 prompt_index 失败；补齐真实接纳坐标后，全部 24 个 notification_drain 测试通过。其他本轮新增接收、权限、回复和原子消费测试均在整包中通过。最早旧提示文案断言也已同步为 `Message unavailable`。
- pager：最终整包 7,230 passed、11 ignored、11 failed；下列独立恢复问题保持未修改。本次 communication、coordination、viewer、同长度/metadata 更新和选区测试均通过，额外 `NO_COLOR=1` 独立进程的 14 个 communication 测试通过。
- pager-minimal：87 passed。
- `scripts/test_agent_messages.py`：真实 parent→read-only child→parent 往返通过；两次发送各有真实 receipt，下一安全步骤完整可见，无 ACK 循环、权限请求或重复 runtime 收件。进程重启后已消费 reply 只恢复一次，没有新推理或重投递。
- `scripts/test_local_coordination.py`：10 组真实独立进程场景全部通过，覆盖 busy/FIFO、工具与 API、同 ID 去重、审批/取消、跨来源隔离、Sideband 无工具、崩溃和重启恢复。
- 精确文件 `rustfmt --check`、`git diff --check`、Python 语法校验通过。
- 归档前 `openspec validate --all --strict --no-interactive`：21 passed、0 failed。

### 未混入本次的 Pager 回归问题

11 项失败包括 7 个 `acp_handler::tests::subagents` 恢复场景、`dashboard_attach_subagent_lazily_replays_deferred_transcript`、`ensure_subagent_child_replayed_releases_retained_memory_once`、`first_parent_notice_does_not_block_child_updates_replay`，以及 `session_loaded_drains_pending_first_prompt_to_front`。

前 10 项的恢复夹具写入 summary/updates，未提供当前 replay reader 所需 Timeline。已逐段比较 `with_reconciled_replay_lines` 与 fixture `write_session_summary`，两者均与 HEAD 相同；本次没有更改这些读取/夹具逻辑。最后一项位于未改动的 pending-first-prompt 调度路径。这里记录失败与定位范围，不把未运行的基线构建声称为已验证，也不通过放宽严格加载、删除场景或改写无关夹具让整包变绿。后续作为独立债务处理。本次真实消息 cold-load 和 durable communication replay 已另外通过。

本次保留已授权功能的完成状态；以上失败没有被计作通过。全仓库其他 packages、真实远端 provider、系统剪贴板和新人工 PTY 视觉验收未执行。


### 归档与清理

- `openspec archive clarify-agent-communication-context-and-rendering --yes`：完成，主规范新增 7 项、修改 3 项要求。
- 归档后 `openspec validate --all --strict --no-interactive`：20 passed、0 failed。
- `openspec validate --archived --no-interactive`：354 passed、0 failed。
- 归档目录的本地 Markdown 链接检查通过，0 个缺失链接。
- 已删除本次独立 `/tmp/grow-agent-communication-target`（约 9.2 GiB）与本次 Python 编译缓存，未清理共享 target。未创建 Git commit。
