## ADDED Requirements

### Requirement: Hook projections preserve tool ownership across resume

终端 SHALL 使用稳定工具调用身份关联实时与恢复的 pre_tool_use/post_tool_use Hook，保留完成工具及合并 Edit 的归属；SHALL 按 occurrence 身份去重，不能把 Hook 猜测挂到最后一个工具或覆盖其他 occurrence 的详情。

#### Scenario: 对话先恢复而 Hook 后到达
- **WHEN** 完整工具历史已重放，之后收到历史 pre/post tool Hook
- **THEN** Hook 附着到对应工具行，不在尾部逐项创建工具 Hook 行。

#### Scenario: 并行工具与合并 Edit
- **WHEN** 两个工具完成顺序不同，或同文件 Edit 已合并
- **THEN** 每个 Hook 仍属于包含其调用的行，同 phase 的多个 occurrence 均保留。

#### Scenario: 重复快照
- **WHEN** 相同 occurrence 在实时与多次恢复快照中出现
- **THEN** 同一视图只展示一次该 occurrence。

#### Scenario: 实时 Hook 先于工具行到达
- **WHEN** 实时 pre/post Hook 早于相应 ACP ToolCall 到达
- **THEN** 按工具身份暂存并在工具行到达时附着；若该工具隐藏或回合结束仍无锚点，则保留显式生命周期记录，不错挂其他工具。

证据入口：`crates/codegen/pager/src/acp/tracker.rs`、`crates/codegen/pager/src/app/acp_handler/session_notification.rs`。

### Requirement: Unanchored historical hooks have a compact inspectable presentation

终端 SHALL 显式识别历史 Hook 快照；没有对应 pre/post 展示位置的历史生命周期 Hook、无可靠工具锚点的 Hook 与说明 SHALL 合并到每视图一个默认折叠的历史条目，保留身份、事件及运行结果。历史 stop SHALL 不影响当前 turn 的待挂接 Hook。根与子会话视图 SHALL 使用相同行为。

#### Scenario: 恢复大量生命周期与隐藏工具 Hook
- **WHEN** 恢复包含大量 notification、session、stop 或没有展示工具行的 Hook
- **THEN** 它们集中在一个可展开历史条目，失败与阻止详情仍可检查。

#### Scenario: 加载结束后才收到快照
- **WHEN** loading 已结束且另一个 turn 正在运行，历史快照才到达
- **THEN** 仍按历史处理，不加入当前 stop stash，也不生成逐项实时通知。

#### Scenario: 加载期间收到实时执行
- **WHEN** resident actor 在客户端加载期间产生新的实时工具 Hook
- **THEN** 按实时身份关联或暂存，不因 loading 状态误认成历史快照。

证据入口：`crates/codegen/pager/src/scrollback/blocks/tool/lifecycle.rs`、`crates/codegen/pager/src/app/acp_handler/session_notification.rs`。
