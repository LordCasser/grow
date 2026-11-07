# Expose canonical input lifecycle

## Why

Shell 稳定输入重试（F3）需要按稳定 `input_id` 读取已存在输入的 canonical 生命周期事实，且要求查询失败与身份不存在可区分。当前缺口有两个：

1. `Timeline::input_state` 把 `Handled` 事件与 `Consumed` 事件折叠成同一个 `InputState::Consumed`，Shell `grow/input/admission_status` 只能在事件流上再平行扫一遍 Handled 事件才能输出 `handled` 状态，同一事实存在两份推导。
2. ChatState 没有面向 handle 的单输入查询入口，调用方只能整段导出事件自行折叠。

## What Changes

- chat-state Timeline 折叠把内部 `InputFold.consumed: bool` 改为私有 `consumption: Option<InputConsumption>`（`Conversation` / `Handled` 最小互斥 enum）；`Consumed` 事件写 `Conversation`，`Handled` 事件写 `Handled`。内存查询投影 `InputState` 新增 `Handled { route }` 变体。持久 `InputEvent` schema、validator 权限与 route 规则不变，`TIMELINE_SCHEMA_VERSION` 不变。
- 新增只读查询 `ChatStateCommand::GetInputState` 与 `ChatStateHandle::input_state(&self, input_id: &str) -> Result<Option<InputLifecycle>, String>`。actor 分发只委托 `Timeline::input_state`，不建第二缓存或事件扫描；actor 不可用返回 `Err`，身份无持久 Submitted 事实返回 `Ok(None)`。
- Shell `extensions/session_admin.rs::read_durable_input_status` 去除 Handled 事件平行扫描，直接 match `InputState::Handled` 输出原 wire `handled` 形状。

## Non-goals

- 不实现 Shell admission 政策、prompt_queue、commands、ACP agent 改动；F3 caller（重复提交短路）、F1、F2 另行交接，依赖本接口。
- 不改持久事件编码、validator 权限、route 规则；不为 RouteSuperseded 新建 registry（`fold.dismissed` 已拥有该事实）。
- 不为查询引入新缓存、索引或第二份输入登记。

## Affected capabilities

- input-admission：新增 canonical lifecycle 查询契约，并区分 Handled 终局。
