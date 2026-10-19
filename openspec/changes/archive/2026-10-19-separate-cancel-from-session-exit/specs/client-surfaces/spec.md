## MODIFIED Requirements

### Requirement: Goal Active queued inputs are directly editable and retractable

Pager SHALL let the user edit or remove a still-pending queued message while Goal is Active through the queue item's existing edit and cancel actions, without selecting Stop Goal or Stop Turn. It SHALL distinguish a confirmed pending item from an optimistic submission and an already-running item, and SHALL report the Shell's authoritative result instead of treating local UI mutation as success.

#### Scenario: Edit a pending message during Goal execution
- **WHEN** Goal is Active, another turn is running, and the user selects `[edit]` or the queue edit key for a confirmed pending message
- **THEN** Pager enters protected editing only after the Shell confirms the hold; the current turn, subagents, and Goal lifecycle remain unchanged.

#### Scenario: Retract a pending message during Goal execution
- **WHEN** Goal is Active and the user selects `[cancel]` or the queue delete key for a confirmed pending message
- **THEN** successful authoritative removal makes the message disappear and prevents its later execution without opening a Goal／turn stop choice.

#### Scenario: Pending submission or stale row
- **WHEN** the selected row is still an optimistic echo, has already started running, was removed elsewhere, or changed version before an edit or remove request is confirmed
- **THEN** Pager does not claim the operation succeeded; it reconciles with the authoritative queue and preserves any unsaved editing text for recovery or explicit user action.

#### Scenario: Failed edit save
- **WHEN** the Shell rejects or cannot durably admit edited content
- **THEN** Pager keeps the editing draft and shows a failure; it does not silently return to normal composer mode or send the old text as the edit result.

#### Scenario: Turn-stop shortcut remains distinct
- **WHEN** the user confirms the global Quit gesture with two Ctrl+D presses within the confirmation window without selecting a queued item action
- **THEN** Pager requests session-tree suspension, preserves admitted queued messages, and does not infer queue withdrawal or a Goal/turn cancellation; explicit Stop Turn/Stop Goal actions remain separate.


### Requirement: CLI replay runs an isolated observational TUI

`grow replay <session-id> [--speed <value>]` SHALL 以指定 session 的固定历史快照启动只读 TUI，默认 speed 为 1。它 SHALL 复用会话展示组件、没有会话输入框（主动搜索可使用本地搜索编辑器），并且不创建执行 session、连接执行 ACP、调用 provider、运行工具/Hook、恢复 scheduler/workflow 或修改源 session/工作区。必要历史不存在或校验失败 SHALL 明确报错，不进入真实 resume 作为回退。

#### Scenario: Start replay without model configuration
- **WHEN** 历史来源有效，但模型配置无效或没有认证
- **THEN** replay 仍可展示历史，不初始化 provider 或要求修复模型配置。

#### Scenario: Historical execution and interaction appear
- **WHEN** 历史包含工具、Hook、Ask、权限请求或子任务生命周期
- **THEN** 只呈现已记录内容和状态，不重新执行、不发应答、不产生当前权限/任务操作。

#### Scenario: Source cannot be validated
- **WHEN** 指定 session 缺失，或必要 authority/引用损坏
- **THEN** replay 报告来源错误，不新建会话、不恢复执行、不展示无提示的部分成功历史。

#### Scenario: Quit or cancel key is pressed
- **WHEN** 用户在根页面且无局部搜索/选区/详情/帮助时按 q 或 Esc，或在任意播放视图于一秒内连续两次按 Ctrl-D
- **THEN** 只退出播放器并恢复终端，不发送 session cancel 或修改被查看 session；查询编辑中的 q 是字符，Esc 先退出编辑；非编辑搜索结果/纯选区、详情/帮助和子页的 Esc/q 按层级返回。

证据与实施入口：`cli/src/main.rs`、`pager/src/app/cli.rs`、`pager/src/replay_cmd.rs`；复用前序 `export-session-transcript-tree` reader。路径均相对 `crates/codegen/`。

### Requirement: Replay supports ordinary read-only transcript inspection

Replay SHALL 复用普通会话被动浏览组件，支持鼠标与键盘选择、分组折叠、详情、搜索、正文/详情文本选择与复制。浏览 SHALL 仅使用已交付的投影，不访问未来正文、当前执行数据或未捕获外部文件。它 SHALL 不提供会话提交、修改历史、执行工具或批准交互的能力；历史外链、文件或图表 SHALL NOT 触发浏览器、editor、native opener、Mermaid 子进程或其他执行补全。仅用户明确复制时允许调用既有 clipboard backend，不允许历史内容提供待执行命令。

#### Scenario: Inspect a recorded tool or message
- **WHEN** 用户操作已显示内容的详情入口或选择后按 Enter/Ctrl-F
- **THEN** 打开相应查看器，可以搜索/选择/复制当前已交付内容；关闭回原阅读位置，不执行记录中的命令，也不提前展示 reveal 尚未交付的文本。

#### Scenario: Expand grouped rows
- **WHEN** 用户点击分组头或对选中组按 Enter，再查看已展开成员
- **THEN** 正确展开/折叠和打开成员详情；共享 entry 的组头不会误打开第一个成员。

#### Scenario: Select text while records arrive
- **WHEN** 用户拖选正文或详情文本，同时新记录到达、窗口调整或鼠标释放
- **THEN** 已有选区按稳定 entry 身份保持；拖动和失效热区不触发点击，复制失败有可见反馈。

#### Scenario: Local input owns its keys
- **WHEN** 用户在搜索或详情内输入字符、空格、q 或加减号
- **THEN** 优先遵循当前局部控件行为，不触发播放快捷键；F8 可控制播放，Ctrl-C 不退出；Ctrl-D 需要同键连续两次确认后只退出播放器。

#### Scenario: Close nested inspection
- **WHEN** 用户在搜索编辑/选区、详情/帮助或子 agent 页面按 Esc
- **THEN** 逐层退出查询编辑、结果/选区、详情/帮助、子页，根页面才退出；q 在查询编辑内是字符，在其他局部层按同样层级返回；返回保持阅读位置和播放状态。

#### Scenario: Late search result belongs to an old view
- **WHEN** 关闭搜索、修改查询或切换子页后旧后台结果到达
- **THEN** 旧 owner/generation 的结果不抢焦点或滚动位置，查询不包含未来队列。

#### Scenario: Historical auxiliary output is absent
- **WHEN** 记录中的后台任务输出、媒体或外部文件内容无法从已捕获历史获得
- **THEN** 显示可用表示或明确缺失说明，不连接当前任务、不启动命令或读取当前 workspace 文件冒充历史；点击历史链接/媒体不启动 browser/opener/renderer 进程。

## REMOVED Requirements

### Requirement: Ctrl C requests bounded session tree suspension
**Reason**: Ctrl+C must remain an input/cancel shortcut and cannot request exit.
**Migration**: Confirm the default Quit gesture with two Ctrl+D presses instead; the same tree suspension barrier follows confirmation.

## ADDED Requirements

### Requirement: Confirmed quit requests bounded session tree suspension

Ctrl+C SHALL retain its existing composer/local-control behavior and cancel the current turn through the explicit cancellation path. It SHALL NOT request Quit, arm exit confirmation, suspend the tree, or escalate repeated cancellation into exit. At idle it SHALL remain local or a no-op. The default half-page-down shortcut SHALL be uppercase D so it does not conflict with Quit. The default global Quit shortcut SHALL be Ctrl+D, confirmed by a second press of the same shortcut within the existing one-second PendingAction window. The first press SHALL preserve drafts, admitted queues, current turn and Goal state without starting suspension. Expiry or another key SHALL clear the pending confirmation. Welcome, normal/minimal execution views and Dashboard SHALL use this same default confirmation; local scroll/input widgets SHALL NOT consume the registered global Quit key.

Confirmed Quit SHALL request suspension of every session tree owned by the exiting process and exit only after the required durable boundaries and writer retirement. A child view SHALL resolve to its canonical root. Suspension SHALL retain the existing shared five-second absolute deadline, unconfirmed/retry/explicit-force UI, ownership and command interruption rules. Repeated Quit input while an exit operation exists SHALL coalesce without another request, deadline reset or force quit. Cancel and emergency force quit SHALL remain distinct actions. Read-only surfaces SHALL not start execution.

#### Scenario: Ctrl C preserves composer behavior
- **WHEN** the composer has a draft and the user presses Ctrl+C
- **THEN** its existing clear/local behavior runs without exit confirmation or a suspension request; subsequent Ctrl+C can cancel a running turn once the composer no longer consumes it.

#### Scenario: Ctrl C cancels or retries cancellation
- **WHEN** a turn is running or already cancelling and the composer does not consume Ctrl+C
- **THEN** Pager requests or retries cancellation; repeated Ctrl+C never produces Quit or tree suspension.

#### Scenario: Ctrl C at idle
- **WHEN** an execution view is idle, including minimal mode, or Welcome/Dashboard receives Ctrl+C
- **THEN** local input/view behavior may run but the application does not exit or arm Quit.

#### Scenario: First and second default Quit press
- **WHEN** Ctrl+D is pressed once and then again within one second
- **THEN** the first press only arms confirmation while preserving work and drafts; the second enters the existing tree suspension and exit path once.

#### Scenario: Confirmation expires or is interrupted
- **WHEN** the second Ctrl+D occurs after expiry, or another key including Ctrl+C intervenes
- **THEN** the prior arm cannot confirm exit; a fresh pair is required.

#### Scenario: Repeated Quit while persistence is pending
- **WHEN** the confirmed exit is waiting for persistence and another Quit key arrives
- **THEN** the same request and deadline remain, with no additional cancellation or forced process exit.

#### Scenario: Suspension remains unconfirmed
- **WHEN** the five-second caller deadline expires or a required confirmation fails
- **THEN** Pager reports unconfirmed and offers retry or explicit force quit without reporting successful suspension; repeated Ctrl+D does not select force quit.

#### Scenario: Nested and multiple roots
- **WHEN** a confirmed quit starts from a child view or owns multiple root trees
- **THEN** each owned root is suspended with its descendants; another owner's independent sessions are unaffected, and local transcript/selection behavior cannot replace global Quit confirmation.
