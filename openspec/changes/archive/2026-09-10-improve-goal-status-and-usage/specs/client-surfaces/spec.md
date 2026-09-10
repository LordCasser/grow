## ADDED Requirements

### Requirement: Goal tool success closes its running UI row
CreateGoal、GetGoal 与 UpdateGoal 成功结果 SHALL 投影为带原工具调用 id 和结构化输出的 ACP Completed 更新，使客户端在当前 turn 内停止对应工具行的运行状态与计时。

#### Scenario: Read or create completes while the turn continues
- **WHEN** Goal 查询或创建成功，模型随后继续推理或执行其他工具
- **THEN** 对应 Goal 工具行立即完成，不把后续工作耗时记为该工具的读取或创建耗时。

#### Scenario: Goal update or failure
- **WHEN** Goal 状态更新成功或工具返回错误
- **THEN** 既有成功 Completed 和错误 Failed 语义保持不变，不把错误伪装成成功。

证据入口：`crates/codegen/shell/src/session/acp_conversion.rs::acp_tool_update`、`crates/codegen/pager/src/acp/tracker.rs`。

### Requirement: Detailed token counts use comma grouping
Usage 与 Goal 详情中的完整 token 数字 SHALL 使用每三位逗号分隔，并保留不完整账本的 ≥ 标记；预算及未分类历史 SHALL 使用相同格式。紧凑状态栏可以保留 k/M 单位。

#### Scenario: Large incomplete Goal usage
- **WHEN** Goal 详情包含累计、缓存命中、缓存未命中、输出、预算或未分类历史的大数
- **THEN** 例如 100000000 显示为 100,000,000，≥ 和分类含义不变。

### Requirement: Ordinary agent status shows session usage
没有 Goal 的普通主会话 Agent 视图 SHALL 在原 Goal 插槽显示本会话账本累计 token 与输入缓存命中率，并在点击时打开 Usage 页。数据 SHALL 来自既有 session ledger 的变化投影及当前连接快照，不使用定时轮询、context 窗口压力或 prompt 总额累加替代账本。子 Agent 内嵌视图不增加一个指向父会话用量的入口。

#### Scenario: Calls and late child settlement
- **WHEN** 普通会话产生主调用、已归属的子任务消费或不完整标记
- **THEN** 状态栏按账本的累计 input + output（含 cache hit）更新，重复累计快照不会重复加账，缓存率按总 cached input / 总 input 计算。

#### Scenario: Empty or incomplete usage
- **WHEN** 没有输入、缓存数超过输入或账本不完整
- **THEN** 无效比例显示 N/A；不完整累计显示 ≥，缓存率仅表示 recorded usage，不伪装精确完整用量。

#### Scenario: Open usage or Goal details
- **WHEN** 用户点击普通会话用量或已有 Goal 的状态区域
- **THEN** 普通用量打开既有 Usage 面板的 Usage 页，Goal 继续打开 Goal 详情；窄屏下点击区域不能超出可见区域。

#### Scenario: Resume or reconnect
- **WHEN** 客户端重新连接存活进程或在新进程恢复会话
- **THEN** 投影当前进程账本，与 Usage 的 since start or last resume 窗口一致，不从历史通知恢复旧进程总额。

证据入口：`chat-state/src/actor/mutations.rs`、`shell/src/extensions/usage.rs`、`pager/src/views/agent_status.rs`、`pager/src/app/agent_view/render.rs` 和 `mouse.rs`（均位于 `crates/codegen/`）。
