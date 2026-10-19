## ADDED Requirements

### Requirement: Skill loads persist independent context facts
加载结果 SHALL 关联真实 session、输入引用和工具调用身份，持久保存来源、调用参数、结果及实际正文快照。用户触发 SHALL 在一次权威发布中消费该输入并追加原用户消息及整组 Runtime 来源的独立结果，不改写原始用户正文或伪造模型响应；模型触发 SHALL 复用真实工具结果，不重复注入正文。Loaded SHALL 只由完整正文及上下文贡献的提交确认派生，准备事实本身 SHALL 不进入模型 Surface。

#### Scenario: User-triggered load commits
- **WHEN** 用户触发的加载成功完成
- **THEN** 一次权威发布同时确认输入消费、原始用户消息和有序独立结果，结果关联输入、引用序号及真实 ToolEvent；整组确认后才显示 Loaded，依赖采样还须通过首次消费完整性检查。

#### Scenario: Model-triggered load commits
- **WHEN** 已接纳模型工具调用发起技能加载
- **THEN** 结果关联真实 call identity，正文只作为该次工具结果贡献一次，不增加虚假用户输入或 Runtime 副本。

#### Scenario: Prepared body without durable confirmation
- **WHEN** 正文已准备但提交失败或确认状态不明
- **THEN** 不宣称 Loaded，不放行依赖采样；确定失败按失败处理，未知结果按权威日志对账。

#### Scenario: Crash at the user group publication boundary
- **WHEN** 进程在用户输入与多项技能结果发布期间退出，随后恢复
- **THEN** 权威历史只能确认整组未发布或整组已发布，不出现原输入已进入 Surface 但部分技能缺失的状态；未确认时先对账，不重复消费或执行 Hook。

证据入口：`crates/codegen/chat-state/src/timeline.rs` 的 MessageCause 与事实校验、`crates/codegen/sampling-types/src/conversation.rs` 的 ConversationItem、`crates/codegen/shell/src/session/event_tracker.rs` 的持久工具事件。新增行为不得借用虚假的 sampler response admission。

### Requirement: Runtime skill loading reuses causal tool execution boundaries
用户加载 SHALL 在真实 Runtime Step 中使用现有工具开始、终态与 Hook 因果关系，不需要前置 provider 请求。工具开始 SHALL 先于对应 PreToolUse 持久化，Post Hook SHALL 晚于工具终态确认。加载失败的输入 SHALL 合法结算且不被重新消费；用户加载 SHALL 不伪造 Request、模型响应或用量。

#### Scenario: First user load without a model step
- **WHEN** 首条或队列用户技能输入获准执行且尚无模型步骤
- **THEN** 在所属 turn 的真实 Runtime Step 记录加载工具及合法 Hook cause，结束后再决定是否进入模型步骤；该 Runtime Step 不产生 provider 采样用量。

#### Scenario: Failed FIFO and steering settlement
- **WHEN** 已被 turn 预留的 FIFO 技能输入失败，或未消费的技能插话失败
- **THEN** FIFO 沿未送模型的 Handled 语义结算，插话沿适用的 Dismissed / Invalidated 语义结算并保留错误；不对 reserved 输入直接 Dismissed，不关闭原任务来处理失败插话。

证据入口：`crates/codegen/shell/src/session/actor/tool/dispatch.rs` 的 handle_direct_bash_command、`event_tracker.rs` 的 tool_started、`actor/input_admission.rs` 的 complete_unmodeled_fifo_inputs / dismiss_input_ids，以及 `crates/codegen/chat-state/src/timeline.rs` 的 Hook 和 Input fold（Shell 的相对文件均位于 `crates/codegen/shell/src/session/`）。

### Requirement: Skill load identity makes result delivery idempotent
相同加载 identity 与相同 payload 的重复提交 SHALL 返回原结果，不重新读取正文或执行 Hook；相同 identity 的冲突 payload SHALL 拒绝。再次主动调用 SHALL 使用新的输入或模型调用身份。提交确认不明 SHALL 先对账，不以新执行猜测旧结果。

#### Scenario: Acknowledgement lost after commit
- **WHEN** 已提交加载结果的确认丢失，调用方重送同一 identity 与 payload
- **THEN** 返回原正文快照和结果，不重复加载、Hook 或上下文贡献。

#### Scenario: Identity conflict
- **WHEN** 相同加载 identity 携带不同来源或参数
- **THEN** 拒绝冲突，不覆盖原结果。

#### Scenario: Intentional repeat
- **WHEN** 用户再次主动提交相同技能命令
- **THEN** 新输入身份允许新的加载，不因旧历史成功而跳过。

证据入口：`crates/codegen/chat-state/src/actor/mutations.rs` 与 `timeline.rs` 的持久提交、身份校验，以及现有输入 identity 契约；本条只增加加载所需幂等边界。

### Requirement: Skill history replay is observational
恢复、导出导入、fork 与 rewind SHALL 根据持久事实和原正文快照投影加载结果，不执行工具、重跑 Hook 或读取当前技能文件。快照损坏 SHALL 明确失败，不以当前文件代替。加载事实及前提 SHALL 遵守既有历史因果边界，不形成全局已加载状态。

#### Scenario: Source edited after loading
- **WHEN** 技能加载后源文件被修改或删除，随后恢复或导出会话
- **THEN** 已提交记录仍显示原来源、参数和正文快照，不产生新的加载。

#### Scenario: Missing body artifact
- **WHEN** 被持久结果引用的正文快照缺失或损坏
- **THEN** 加载历史明确报告完整性错误，不重新读源文件修补成功记录。

#### Scenario: Fork and rewind
- **WHEN** 从包含加载结果的历史 fork 或 rewind
- **THEN** 仅保留因果边界内的加载事实和上下文贡献，不重新执行它们，也不从被截断历史继承 loaded 标记。

#### Scenario: Incomplete operation during replay
- **WHEN** 历史包含尚未确认终态的加载操作
- **THEN** replay 显示未确认状态，只有取得执行所有权的恢复路径可以对账或继续，纯历史加载不自行重启它。

#### Scenario: Published content withdrawn before first consumption
- **WHEN** 已发布加载在首次关联模型步骤获得已接纳响应前因执行终止而从 Surface 撤下，随后恢复、fork 或导出
- **THEN** 原发布和 Loaded 作为历史保留，当前 Surface 及续跑判断遵守关联到该组的撤下事实，不重新注入正文或改动无关输入 / 工具结果；纯回放不触发执行。

证据入口：`crates/codegen/chat-state/src/timeline.rs`、Shell 会话恢复与 export/fork/rewind 路径，主规范既有只读观察与 Hook 恢复契约。
