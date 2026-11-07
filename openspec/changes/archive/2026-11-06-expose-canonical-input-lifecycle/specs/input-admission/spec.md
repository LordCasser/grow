## ADDED Requirements

### Requirement: Canonical input lifecycle query

ChatState SHALL 提供以 `input_id` 为键的只读 canonical 生命周期查询 `ChatStateHandle::input_state`，由 Timeline 既有输入折叠直接投影，不维护第二份输入登记或事件扫描。查询失败（actor 不可用）SHALL 返回 `Err`，该身份在 Timeline 中无持久 Submitted 事实 SHALL 返回 `Ok(None)`，两者不得混淆。投影 SHALL 区分模型上下文已发布（`Consumed`）与终局处理未发布（`Handled`，携带 route）；被后续接纳取代的输入 SHALL 保持既有 `Dismissed(RouteSuperseded)` 折叠，不新增 registry。同一事件流冷恢复后 SHALL 返回相同投影。Shell `grow/input/admission_status` SHALL 依据该投影输出既有 `handled` wire 形状，不再平行扫描 Handled 事件。

#### Scenario: Handle 查询追踪完整生命周期

- **WHEN** 调用方通过真实 ChatStateHandle 依次查询一个输入在 Submitted、Allowed、Consumed（或 Handled）提交后的状态
- **THEN** 每个阶段返回该阶段的 canonical 投影；Handled 终局返回 `Handled { route }` 而非 `Consumed`。

#### Scenario: 身份不存在与查询失败区分

- **WHEN** 查询 Timeline 中没有 Submitted 事实的 input_id，或 actor 已关闭时查询任一 input_id
- **THEN** 前者返回 `Ok(None)`，后者返回 `Err`，调用方不得把两者当作同一语义。

#### Scenario: Route superseded 保持原折叠

- **WHEN** 一次 Allow 携带 supersedes 取代一个已 Allowed 的旧输入后查询旧 input_id
- **THEN** 返回 `Dismissed { reason: RouteSuperseded }`，该事实仍由输入折叠自身拥有。

#### Scenario: 冷恢复返回同一投影

- **WHEN** 从持久事件重建 Timeline 后查询同一 input_id
- **THEN** 返回与原 Timeline 相同的生命周期投影，含 Handled 与 Consumed 的区分。

#### Scenario: Shell handled wire 形状不变

- **WHEN** `grow/input/admission_status` 观察一个已 Handled 的输入
- **THEN** 输出原有 `handled` 状态形状，不再对事件流做第二次 Handled 扫描。

证据与实施入口：`crates/codegen/chat-state/src/timeline.rs::Timeline::input_state`、`InputState`、`InputFold.consumption`，`crates/codegen/chat-state/src/commands.rs::ChatStateCommand::GetInputState`，`crates/codegen/chat-state/src/handle.rs::ChatStateHandle::input_state`，`crates/codegen/chat-state/src/actor/mod.rs` 查询分发，`crates/codegen/shell/src/extensions/session_admin.rs::read_durable_input_status`。
