## MODIFIED Requirements

### Requirement: Session storage separation

会话存储 SHALL 区分 summary、权威 Timeline、Sideband 与既有 artifacts/rewind 资源。持久展示历史 SHALL 从 Timeline 事实及其来源引用重建，不再读写独立 updates.jsonl。实时 ACP/Grow session/update 协议保持。

#### Scenario: 恢复本地会话
- **WHEN** 读取新格式会话
- **THEN** 校验 Timeline 与必要引用，从同一事实派生模型上下文和完整历史；summary 不替代权威事实，目录不需要 updates.jsonl。

### Requirement: Accepted response history is rebuilt from Timeline authority

所有 production replay 入口 SHALL 从已完整校验的 Timeline 和选定 branch 派生确定性的 response history；canonical 正文、identity、quarantine 与 anchor 以该事实为准，不保存或对账第二份 response projection。建立 cursor cutoff 或释放 buffered live 前 SHALL 确认同一截点内的历史覆盖与顺序。全量历史加载完成后才允许普通输入；不引入分页、提前 ready 或倒序事件回放。

#### Scenario: Process stops after Timeline admission
- **WHEN** durable admission 后、public Accepted 交付前退出
- **THEN** cold load 派生恰好一份历史，不调用 provider、继续 truncation/pause-turn 或执行工具。

#### Scenario: Projection append acknowledgment is lost
- **WHEN** 某项历史展示的唯一 Timeline 来源可能已提交但 durable ACK 丢失
- **THEN** 按该来源的原 identity/payload exact 核对并同步，相同内容不重复、不同内容拒绝；不存在另一个 projection 文件的成功条件。

#### Scenario: Earlier attempt was discarded
- **WHEN** 较早 attempt 只有 transient candidate，较后 attempt 已接纳
- **THEN** 只展示 exact request/attempt 的已接纳 response，不通过文本或邻近记录认领候选。

#### Scenario: Candidate output precedes durable projection
- **WHEN** candidate 已向可撤回 live client 展示，Timeline durable ACK 尚未确认
- **THEN** candidate 仍可撤回，不提前发布 Accepted、后继 provider/工具或成功 terminal。

#### Scenario: Quarantined response is replayed
- **WHEN** 确定性 quarantine 非零，或早期 attempt 没有 admission
- **THEN** 不将其 candidate 变为 accepted/executable history；按 exact request/attempt 选择有效 response，保留原 diagnostic 语义。

#### Scenario: Tool-bearing response has later tool history
- **WHEN** response 后存在工具调用、结果及独立事件
- **THEN** canonical response 在其 anchor 和依赖边界恢复，工具历史只显示一次且不重新派发。

#### Scenario: Rewind or legacy response
- **WHEN** 当前分支经过 rewind 或 Surface 已 compaction，或读取无可验证 admission 归属的来源
- **THEN** 不复活被 rewind 的正文，也不从已压缩 Surface 反推 transcript；所选完整历史与原来源顺序一致，不猜测缺失 admission 归属，旧 schema 明确拒绝。

#### Scenario: Direct replay without writer authority
- **WHEN** child view、export 或 read-only reader 读取会话
- **THEN** 使用同一纯 projector，不取 writer lease、不写磁盘、不触发执行。

#### Scenario: Cursor intersects one projection record
- **WHEN** cursor 无法证明某个事实展开的全部通知已应用，或 lineage/projector 版本不匹配
- **THEN** 完整替换历史，不跳半个 response，也不在后继事件之后补前件。

### Requirement: Resident replay preserves the physical snapshot frontier

Resident reconnect SHALL 以同一会话 owner 捕获的 Timeline 完整前缀与事实截点作为初始历史和 delta 的共同边界，保持 response、工具和独立事件顺序及恰好一次交付；不使用已删除的 updates offset。Cursor SHALL 绑定 session/lineage、projector 版本、事实覆盖及展开位置，不能把较早 display anchor 当作交付进度。原 live gate 和 load 完成屏障保持。

#### Scenario: Response completes between authority and cache snapshots
- **WHEN** 采样在 resident 恢复屏障期间产生 response 或后继工具事实
- **THEN** 原 writer 暂缓其提交，初始历史只消费完整截点；恢复 live gate 后再放行后续提交，屏障期间活动 attempt 的完整正文仍可交付，不丢失被 mute 的片段。

#### Scenario: Already synthesized response reaches the physical cache
- **WHEN** 同一来源事实已在 snapshot 展示，随后出现在 buffered live
- **THEN** 去重该事实并继续交付后继独立更新，不重排或丢失。

### Requirement: Offline transcript reads detect missing committed user echoes

Offline transcript SHALL 从已验证输入事实及其原始 display 来源重建用户 echo，保持 exact message/prompt identity、块顺序、display text 和 hide 语义；必要 display 来源缺失或损坏 SHALL 明确失败，不用模型包装后的文本替代。符合条件的 turn terminal SHALL 从对应 durable 事实与原 prompt identity 重建，不猜测 stop reason；recovery-only terminal 不伪装用户完成。

#### Scenario: A committed user echo is missing
- **WHEN** 输入已提交且无独立展示缓存
- **THEN** 原 display 来源完整时直接重建；必要来源缺失则明确失败，不从模型包装文本或相邻消息补齐。

#### Scenario: A matching complete echo exists
- **WHEN** 输入事实含完整 display 来源及可验证身份
- **THEN** 只派生一份正确 echo，不需要磁盘展示缓存，也不修改来源。

#### Scenario: A user echo is truncated or missing a block
- **WHEN** 原 display 来源缺块、损坏或与身份/指纹不符
- **THEN** 明确报错，不按邻近消息、时间或当前模型上下文补齐。

#### Scenario: A terminal row is missing
- **WHEN** 可展示的 terminal 无法关联原 prompt 或缺少必需终态数据
- **THEN** 不虚构完成或原因，读取明确失败。

#### Scenario: A prompt was removed by rewind
- **WHEN** prompt 只在被舍弃分支中
- **THEN** 不展示它，其无展示 echo 不构成当前分支缺失。

### Requirement: Offline transcript reads detect missing durable Workflow and Control projections

Workflow、Goal、Control、CurrentMode 的持久展示 SHALL 由捕获截点内对应版本的 typed 事实或不可变引用完整重建，来源绑定 identity、revision 与展示位置，不再要求独立展示账本或只有 digest 的 expectation。必要来源缺失、损坏或冲突 SHALL 明确失败；不得借用邻近 revision、可变 manifest 或当前 runtime。观察不修复磁盘、不授予执行权威。

#### Scenario: Durable Workflow row is missing
- **WHEN** 捕获 Timeline 含完整合法的 Workflow/Goal/Control/CurrentMode 事实及来源
- **THEN** 离线读者直接派生对应历史，即使不存在 updates 文件也成功。

#### Scenario: Durable Goal or CurrentMode row is missing
- **WHEN** Goal/CurrentMode 事实已提交且未交付展示
- **THEN** 必要版本来源完整时直接重建；来源缺失或损坏则明确失败，不以当前 runtime 状态代替。

#### Scenario: Existing history has no projection expectation
- **WHEN** 新格式历史使用完整 typed 来源而不含旧 expectation，或读到不支持的旧格式
- **THEN** 前者按新格式校验并派生，后者明确 schema 拒绝；不猜测缺失事实、不双读旧 updates。

#### Scenario: Durable Control row has changed
- **WHEN** 需要的版本来源缺失、checksum 错误或身份/revision 冲突
- **THEN** 明确拒绝，不以其他版本替换。

#### Scenario: Ephemeral Workflow or Control snapshot
- **WHEN** 纯瞬时/coalesced 更新未获持久展示义务
- **THEN** 其缺失不使读取失败，也不新增永久事实。

#### Scenario: Captured frontiers exclude a later projection
- **WHEN** 捕获后发生新状态提交
- **THEN** 不混入本次历史，固定截点保持不变。

#### Scenario: A cache-missing expectation still reserves its event identity
- **WHEN** durable 事实尚未交付或已被 rewind 隐藏
- **THEN** 身份仍占用，cold writer 不复用该 identity。

### Requirement: Resume restores ordered communication history

Resume SHALL 从同一 Timeline 截点恢复通信与普通正文，保留稳定 receipt/source/phase identity、原正文、来源时间和可验证锚点；历史恢复不得变成新对话或播放页面，不重新请求、消费通知、运行 Hook 或集中追加全部 receipt 到末尾。

#### Scenario: Reply is absent from display cache
- **WHEN** 历史含可验证请求/回复锚点和后继正文
- **THEN** 回复处于原历史区间，后继正文仍在其后，不依赖 updates cache。

#### Scenario: Reply already has a historical position
- **WHEN** reconnect 已保留同一 receipt
- **THEN** 原位保留一次，load 后不再追加副本。

#### Scenario: Child history is opened after resume
- **WHEN** 首次打开 child 历史
- **THEN** 使用相同顺序、样式与身份规则，不把通信另移到末尾。

#### Scenario: Incoming inquiry cache omits a phase
- **WHEN** 多个 source peer 使用相同 inquiry ID 或已有部分 phase
- **THEN** 按 peer、inquiry ID、phase 选择各自事实，不跨 peer 去重，不新建询问/审批。

#### Scenario: Historical anchor or body is absent
- **WHEN** receipt 无可验证锚点或其正文不可读取
- **THEN** 沿既有明确有界降级/缺失说明处理，不以文本、当前时间或重新执行伪造精确历史。

#### Scenario: Reconnect cursor crosses synthesized history
- **WHEN** 当前 cursor 不能证明同一 Timeline 截点的 receipt 已完整应用
- **THEN** 按原完整替换规则重建，不能因 display anchor 较早跳过该事实，重复 reload 不重复显示。

### Requirement: Forked response history belongs to the new lineage

普通 fork SHALL 在新 Timeline lineage 中保留已继承的响应展示。父 response projection SHALL 经父 Timeline 核对后转换成继承历史，不能把父 admission identity 作为子 Timeline authority；恢复 SHALL 不重新采样或执行工具。

#### Scenario: Fork inherits projected text and reasoning

- **WHEN** 普通 fork 继承父会话的 admitted text、reasoning 及其后工具历史
- **THEN** fork 的 typed/raw/direct replay 各显示一份有序历史，且不携带父 admission/candidate 身份。

#### Scenario: Fork excludes discarded response history

- **WHEN** 父会话含 rewound、quarantined response 或 fork 指定截断点
- **THEN** 子会话只继承所选历史，不复活被排除的 raw candidate；子 lineage 不携带父展示游标或执行资格。

### Requirement: Full transcript export preserves recoverable conversation presentation

完整会话树导出 SHALL 使用与会话界面一致的用户可见内容选择、正文与状态语义，覆盖可恢复的 ACP 和 Grow 会话展示，包括 thinking、工具结果和子 agent 生命周期。它 SHALL 保留现有隐藏用户回显、显示文本、response admission、rewind、去重和 Hook observational 规则，不把模型 Surface、原始 provider 请求或 trajectory 调试行作为用户消息。每份文档 SHALL 包含身份及直接子文档的相对链接；child 正文 SHALL 保存在 child 文档中。

#### Scenario: Display text differs from model input
- **WHEN** 用户消息携带 display text、skill 包装或 hideFromScrollback 标志
- **THEN** transcript 采用相同展示选择，不泄漏包装后的内部输入或伪造用户消息。

#### Scenario: Conversation contains reasoning and tool details
- **WHEN** 会话包含可见 thinking、工具参数展示、结果、错误或已有截断标记
- **THEN** Markdown 保留相应正文和状态，不再仅输出工具一行摘要或无条件省略 thinking。

#### Scenario: Conversation contains Grow presentation
- **WHEN** 已保存或可从事实重建子 agent、通信、Hook、压缩、后台任务、Goal 或 Workflow 会话展示
- **THEN** 导出保留相应用户展示，父级生命周期不代替完整 child transcript，历史 Hook 不重新执行。

#### Scenario: Response cache is incomplete or stale
- **WHEN** canonical response 需要从 Timeline 派生历史，或存在 discarded/quarantined/rewound candidate
- **THEN** 使用与 resume 相同的 projector 重建去重历史，不复活排除内容、不猜测无法证明的响应位置。

#### Scenario: Optional display material is unavailable
- **WHEN** 已记录的附件引用或截断结果缺少可选展示材料
- **THEN** 保留可得引用及明确 unavailable/truncated 说明，不把当前工作区文件或重新执行结果充当历史正文；必要 authority 损坏仍失败。

#### Scenario: Existing interactive export is used
- **WHEN** 用户调用 TUI `/export` 或既有 `/transcript`
- **THEN** 继续输出当前活动 root/child 视图的单份完整 transcript，保留紧凑展示、路径和剪贴板语义，不自动升级成目录树或仅导出视窗可见的一页。

证据与实施入口：`pager/src/acp/tracker.rs`、`pager/src/scrollback/export.rs`、`pager/src/scrollback/block.rs`、`pager/src/app/acp_handler/`、`pager/src/app/root/dispatch/transcript.rs`；路径均相对 `crates/codegen/`。

### Requirement: Offline Control snapshots use captured authority

来源 reader SHALL 校验捕获 Timeline 中的 Control snapshot，并在必要的只读末尾投影保留最新 Behavior/Goal 状态。该 snapshot SHALL 明确表示截点状态，未知中间事件顺序 SHALL 保持估计，不用当前机器时间伪造历史。

#### Scenario: Display cache omits the last Goal transition
- **WHEN** 捕获 Timeline 已提交退出/clear Goal，且其前一历史状态为 active
- **THEN** 结尾 snapshot 恢复已提交模式/Goal 截点，既有历史正文顺序不被猜测重排，来源文件不变。

#### Scenario: Control is invalid
- **WHEN** Control revision、architecture 或 Behavior/Goal ownership 不合法
- **THEN** 离线读取明确失败，不静默展示冲突控制状态。

### Requirement: Image recovery is one visible operation row

When a text-only model requires image recovery, the client SHALL render the recovery lifecycle as one ordinary operation row with a stable operation identity, not as a NOTICE or a fabricated model tool call. The row SHALL show a running indicator while the configured vision model is called, update in place when local OCR begins, and reach a clear English terminal state for success, partial success, failure, or cancellation. Repeated updates and session replay SHALL update or reconstruct that same row. Cursor reconnect SHALL merge a repeated operation into the existing row while preserving its EntryId. Root and child session notifications SHALL project the operation into the scrollback belonging to the owning session. A terminal row SHALL not retain a running indicator or be reopened by late progress.

#### Scenario: Vision recovery succeeds
- **WHEN** the configured vision model converts one image
- **THEN** the running row settles in place to `Converted 1 image`.

#### Scenario: OCR follows vision recovery
- **WHEN** vision recovery fails or is unavailable and local OCR starts
- **THEN** the existing row changes to show OCR progress and then settles to the final result, without adding a second progress row.

#### Scenario: Recovery is partial or fails
- **WHEN** only some image groups are converted, or no recovery source succeeds
- **THEN** the same row reports the converted and unconverted counts, or a clear failure state, in English.

#### Scenario: Recovery is cancelled or replayed
- **WHEN** the owning operation is cancelled or its persisted lifecycle is replayed
- **THEN** the row reaches a terminal state when the owning turn is terminal, while a genuinely active reconnect remains running; replay never creates duplicate rows for updates sharing the operation identity.

#### Scenario: Offline snapshot ends during recovery
- **WHEN** an offline transcript snapshot ends without a terminal event for a running recovery operation
- **THEN** the read-only projection stops its running indicator and labels the row `Image recovery incomplete at snapshot`, without changing persisted history or claiming that the operation failed or was cancelled.

#### Scenario: Driver RPC wins the terminal race
- **WHEN** the driver's prompt response arrives before the durable turn notification
- **THEN** the shared exact-prompt terminal boundary settles that prompt's operation once; a stale terminal for another prompt has no effect.

#### Scenario: Resident reconnect restores an active conversion
- **WHEN** reconnect finishes with an authoritative active foreground prompt
- **THEN** only that prompt's unfinished conversion remains animated after generic replay cleanup; older unfinished rows stop, and a failed reload labels unresolved conversion status unavailable.

#### Scenario: Cursor reconnect merges a live completion
- **WHEN** cursor reload 前 scrollback 已有某 operation 的 Started 行，reload 期间该 operation 收到 Completed，随后 cursor tail 合并成功
- **THEN** 最终 scrollback 仅有该 operation 的一行，保留 reload 前行的 EntryId，并显示 Completed 内容且不再运行。

#### Scenario: Terminal recovery ignores late progress during merge
- **WHEN** cursor reconnect 合并的旧行已是终态，而 tail 对同一 operation 含迟到的 running progress
- **THEN** 保留原 EntryId 与终态内容，行保持非运行，且不重复输出已提交的行。

#### Scenario: Child image recovery lifecycle is projected
- **WHEN** child session 通过 live 通知或 transcript replay 发送同一 operation 的 Started、进度及 Completed lifecycle
- **THEN** 对应 child view 显示且只显示一个 operation 行，更新复用该行并在终态停止运行；parent view 不冒领该 child 行。

#### Scenario: Child replay and late progress preserve a terminal row
- **WHEN** child lifecycle replay 重复已终态 operation，或终态后到达迟到进度
- **THEN** 终态内容保持且运行状态为 false，不产生重复行。

#### Scenario: Child prompt completion settles unfinished recovery
- **WHEN** child 当前 prompt 的 `TurnCompleted` 到达但 operation 没有自己的终态通知
- **THEN** 只结束该 child 当前 prompt 的 unfinished recovery；其他 prompt 的 stale terminal 不改变当前行。

#### Scenario: Child history replay preserves durable recovery order
- **WHEN** Pager 从 child 的持久化 history 恢复 ImageRecovery lifecycle 与 ACP 输出
- **THEN** 两类事件按 Timeline 的来源/展示锚点顺序投影，recovery 行处在其原始会话位置，并按随后匹配的 prompt terminal 收尾。
