# Design: Expose canonical input lifecycle

## 状态所有者与数据流

- Timeline 仍是输入生命周期的唯一所有者（`LifecycleFold.inputs`）。本 change 只改该折叠的内部表示与查询投影，不新增状态实体。
- `InputFold.consumed: bool` 改为私有 `consumption: Option<InputConsumption>`：`Conversation` 表示 `Consumed` 事件已把输入发布进模型可见 Surface；`Handled` 表示 `Handled` 事件终局处理而未发布。两者互斥且一次性，所有 validator 的 already-consumed 判定由 `input.consumed` 改为 `input.consumption.is_some()`，判定语义与权限不变。
- `Timeline::input_state` 按 `consumption` kind 输出：`Conversation` → `InputState::Consumed { route }`，`Handled` → 新 `InputState::Handled { route }`；`pending_allowed_inputs` 与 Hook Input cause 的未终局判定改为 `consumption.is_none()`。route 仍取折叠自身字段，route 规则不变。
- 持久 `InputEvent` schema、validator 权限、`TIMELINE_SCHEMA_VERSION` 均不变：`InputState::Handled` 只是内存查询投影，不是新事件。

## 查询接口

- `ChatStateCommand::GetInputState { input_id, reply: oneshot::Sender<Option<InputLifecycle>> }`；actor 分发只做 `self.state.timeline.input_state(&input_id)`，与 `GetPlanningState` 共用同一 mailbox 串行语义，查询不会与写竞争读到撕裂状态。
- `ChatStateHandle::input_state(&self, input_id: &str) -> Result<Option<InputLifecycle>, String>` 沿 `planning_state` 惯例：`query(...).await.ok_or_else(...)`。actor 关闭（send 失败或 reply 丢弃）→ `Err`；身份不存在 → `Ok(None)`。命名沿用 `GetPlanningState`/`planning_state` 的 subject 模式。

## 调用方

- Shell `extensions/session_admin.rs::read_durable_input_status`：删除 Handled 事件平行扫描，改为穷尽 match 上的 `InputState::Handled { .. } => InputAdmissionObservation::Handled`，原 wire `handled` 形状（无 route 字段）不变。
- 其余 `InputState` 消费方经全仓 rg 核对：`session/storage/jsonl/mod.rs` 仅用 `matches!` 判定 `Submitted | Allowed`（未终局），无需新分支；Shell `input_admission.rs`/`interjection.rs` 使用自身 `ExistingInputState`，不经本投影；timeline.rs 内部测试断言随折叠更新。

## 失败与恢复

- 查询是纯读，唯一失败模式是 actor 不可用（`Err`）。冷恢复由 `Timeline::from_events` 重放同一事件流得到同一 `InputFold`，投影一致；Tier2 测试同时覆盖 handle 查询与冷恢复。
- 不新增持久化、迁移或版本变化；RouteSuperseded 继续由 `fold.dismissed` 拥有。
