# Tasks

## 1. Timeline 折叠区分 Handled

- [x] 1.1 `InputFold.consumed` 改为私有 `consumption: Option<InputConsumption>`；Consumed 事件写 `Conversation`，Handled 事件写 `Handled`；全部 validator 的 already-consumed 判定改为 `consumption.is_some()`（`crates/codegen/chat-state/src/timeline.rs`）。
- [x] 1.2 `InputState` 新增 `Handled { route }`；`input_state` 按折叠 kind 输出，`pending_allowed_inputs` 与 Hook Input cause 改用 `consumption.is_none()`（`crates/codegen/chat-state/src/timeline.rs`）。

## 2. ChatState 只读查询

- [x] 2.1 `ChatStateCommand::GetInputState` 与 actor 分发委托 `Timeline::input_state`（`crates/codegen/chat-state/src/commands.rs`、`src/actor/mod.rs`）。
- [x] 2.2 `ChatStateHandle::input_state` 沿 `planning_state` 惯例区分 `Err` 与 `Ok(None)`（`crates/codegen/chat-state/src/handle.rs`）。

## 3. Shell 必要适配

- [x] 3.1 `read_durable_input_status` 去除 Handled 事件扫描，match `InputState::Handled` 输出原 wire `handled`（`crates/codegen/shell/src/extensions/session_admin.rs`）。

## 4. 验证

- [x] 4.1–4.2 父整合后修正两条同步Timeline replay查询的多余unwrap，运行 `cargo +1.93.1 test --locked --offline -p chat-state --lib -- input_state_query timeline::tests --test-threads=1`：150 passed / 0 failed / 3既有ignored。覆盖真实handle、Handled区分、supersedes、closed-vs-missing和cold投影；原validator失败断言保留。
- [x] 4.3 Shell `extensions::session_admin` 7项真实通过，包括 `durable_input_status_maps_isolated_timeline_lifecycles` 的handled wire对照；与5项child终态组合共12项、exit0。
- [x] 4.4 `rustfmt --edition 2024`（直跑，无 Cargo）6 个改动文件 `--check` 全部干净；`openspec validate expose-canonical-input-lifecycle --strict --no-interactive` 通过；`openspec validate --all --strict --no-interactive` 通过（25/25）；逐文件 diff 自查完成。
