## ADDED Requirements

### Requirement: Ctrl C requests bounded session tree suspension

执行会话界面的全局 Ctrl+C SHALL 请求整树快速挂起并在成功后退出，第一次按键不得先取消 turn、暂停 Goal 或清除已接纳队列；重复按键 SHALL 合并到同一请求，不变为强退。挂起等待默认 SHALL 使用总计五秒的绝对期限，取消、保存、ACK 核对和退出准备共享该期限；期限只约束调用方等待，不证明操作或 I/O 已结束。挂起失败或超时 SHALL 展示未确认及可重试/显式强退选择，不显示保存成功、不无限等待。显式任务取消与紧急强退 SHALL 保持可区分入口。

#### Scenario: Running root receives Ctrl C
- **WHEN** root 正在执行且有活跃 child，用户触发全局 Ctrl+C
- **THEN** 显示正在挂起，发起同一次树级挂起，在持久边界和 writer 退出确认后正常恢复终端并退出。

#### Scenario: Repeated Ctrl C while acknowledgments are pending
- **WHEN** 用户在保存期间再次触发 Ctrl+C
- **THEN** 不重复挂起、不重置期限、不先取消任务，也不调用强制进程退出。

#### Scenario: Suspension cannot finish within the deadline
- **WHEN** 任一所需确认在五秒期限内无法完成
- **THEN** 交互界面恢复可响应并显示挂起未完成；若用户明确强退，仅保留已提交证据并说明本次不能保证完整恢复，不记录 Suspended 成功。

#### Scenario: A child view or multiple roots are open
- **WHEN** 用户从 child view 请求退出，或退出将销毁本进程拥有的多个 root
- **THEN** child 请求归属其 root；将被该退出销毁的每棵树分别完成挂起，不能只保存当前可见节点；不控制其他 owner 的独立会话。

#### Scenario: Local UI and readonly views
- **WHEN** Ctrl+C 被局部复制等既有局部操作消费，或用户处于只读 replay、无 session 的 Welcome
- **THEN** 沿局部操作或只读退出语义处理，不新增执行；进入执行会话的全局退出路径后不得再走 CancelTurn。

目标入口：`crates/codegen/pager/src/app/root/{mod,event_loop}.rs`、`root/dispatch/router.rs`、`app/signal_handler.rs`、`acp/spawn.rs`。

### Requirement: Resume displays and restores suspended work

执行型 resume SHALL 自动恢复所选挂起 root 的任务树，显示恢复中、已运行或具体受阻原因。挂起的 child SHALL 显示为挂起而非取消；操作中断与 Agent 任务状态 SHALL 分别展示。普通历史浏览 SHALL 不启动 child、provider 或工具。恢复提示 SHALL 不冒充新的用户任务、授权或重复完成通知。

#### Scenario: Resume after terminal application update
- **WHEN** 用户退出后更新 Terminal app，再通过 `grow resume` 打开挂起会话
- **THEN** 原任务树自动续接，已完成结果仍可见，未完成 child 无需逐个手动重启，原父等待关系保持。

#### Scenario: A command could not be confirmed stopped
- **WHEN** 挂起记录包含 OutcomeUnknown 命令
- **THEN** 同时显示任务可恢复与该操作结果未知，不用“已冻结”暗示外部命令已经停止。

目标入口：`crates/codegen/pager/src/app/acp_handler/`、`crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs::load_session`。

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
- **WHEN** the user invokes the global Ctrl+C exit gesture without selecting a queued item action
- **THEN** Pager requests session-tree suspension, preserves admitted queued messages, and does not infer queue withdrawal or a Goal/turn cancellation; explicit Stop Turn/Stop Goal actions remain separate.

### Requirement: Agent teardown retires its session writers

When an `MvpAgent` is destroyed while its process runtime continues, it SHALL retire every primary and active child session actor it owns. Controlled suspension exit SHALL first confirm the tree's suspension boundary, then retire runtime resources without producing cancellation terminals or dismissing saved inputs. Other teardown SHALL retain its existing shutdown/failure meaning. A replacement agent SHALL not load the same session until the old writer has released its lease.

#### Scenario: Agent owner ends with resident sessions
- **WHEN** the Agent owner is dropped without a completed suspension handoff while primary or child session actors remain resident
- **THEN** each actor receives the existing shutdown command and the owner releases its handles without blocking the local event loop; this fallback does not claim a successful suspension.

#### Scenario: In-process leader replacement
- **WHEN** a leader generation is stopped and a replacement generation will load the same session
- **THEN** the replacement waits until the old session writer lease is released before loading; it does not overlap writers or treat an active lease as a successful load.

#### Scenario: Owner exits after suspension handoff
- **WHEN** the owned tree has a confirmed suspension boundary
- **THEN** guard/coordinator teardown releases resources without turning suspended children into cancelled tasks or starting new lifecycle Hook effects.

### Requirement: Kitty keyboard teardown has an owned reply fence

Grow 在正常终端退出中 SHALL 先确认输入 reader 不再消费 TTY，并收束已接受的渲染输出，然后发出已推入 Kitty keyboard flags 的 pop。仅当确实推入过 flags、输出可写且当前进程独占 TTY 输入时，SHALL 在仍为 raw mode 的状态下向该终端发送 DA1 查询，有界消费输入直至收到完整 DA1 回复，再关闭 raw mode。fence 的超时或 I/O 错误 SHALL 不阻止 best-effort 终端恢复；不得通过与存活 reader 并发读取来假装完成屏障。

#### Scenario: Late Kitty release on normal quit
- **WHEN** flags 已推入、用户正常退出，终端在旧 10 ms drain 窗口之后、DA1 回复之前发送 keyboard release
- **THEN** writer 帧先于 teardown，pop 先于 DA1；fence 消费 release 并在完整 DA1 后恢复 raw mode，release 不留给 shell。

#### Scenario: Silent or malformed DA1
- **WHEN** 终端不回复、只回复部分序列，或回复 DA2/CSI-u 而非完整 DA1
- **THEN** 不将其判为 fence 完成；到固定有限期限后仍关闭 raw mode、显示光标并返回，不无限等待。

#### Scenario: No Kitty flags
- **WHEN** 会话未成功推入 Kitty keyboard flags
- **THEN** 退出不发 pop 或 DA1，沿既有恢复路径结束。

#### Scenario: Input or output ownership cannot be established
- **WHEN** reader 停止确认超时、writer 无法安全收束、查询写入失败，或没有可用 TTY
- **THEN** 不与 reader 并发读 stdin，不执行不可靠的回复读取；仍执行可行的终端恢复并记录降级原因，退出不因 fence 增加无界等待。

#### Scenario: Forced exit remains fast
- **WHEN** 首次受控 OS 退出信号请求挂起退出，或 panic、显式紧急强退、第二次外部 OS 信号要求立即退出
- **THEN** 前者在成功挂起且条件满足时经过正常 fence；后者只做快速 best-effort teardown，不等待 reader、writer 或 DA1，且不声称挂起成功；普通重复 Ctrl+C 键盘事件不属于该强退路径。
