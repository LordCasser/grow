## ADDED Requirements

### Requirement: Cancelling turns reconcile against authoritative prompt state

Pager SHALL 在发送取消后用短时、单请求在途的 prompt-status 对账窗口覆盖 `TurnCancelling`。只有 shell 返回 terminal、unknown 或查询错误时才收敛本地状态；时间本身 SHALL NOT 伪造成功终态。

#### Scenario: Cancel targets a vanished session
- **WHEN** Pager 已进入 `TurnCancelling`，但 shell 不再拥有该 session，因而不会发送取消终态
- **THEN** Pager 在短窗口后查询该 prompt，依据 unknown/error 退出 cancelling、恢复输入，并提示 session/prompt 已不可用。

#### Scenario: Cancel is still being processed
- **WHEN** 权威查询仍返回 running
- **THEN** Pager 保持 `TurnCancelling`，刷新下一次对账窗口，且同一 prompt 同时最多一个查询在途。

#### Scenario: Cancel terminal notification was missed
- **WHEN** 权威查询返回该 prompt 的 terminal 状态
- **THEN** Pager 通过既有 single-finalizer 路径结束原 turn，不重复添加终态或重复推进队列。

证据入口：`crates/codegen/pager/src/app/root/dispatch/turn.rs`、`task_result.rs` 及其 tests。

### Requirement: Failed initial session load returns to its origin

Pager SHALL 把首次 resume 创建的 Agent 视为加载期临时实体。加载失败时 SHALL 删除该临时实体，恢复发起前的 Welcome、Agent 或 Dashboard，并显示可操作的失败信息；不得留下没有 session identity 的可见输入页。

#### Scenario: Resume from welcome fails
- **WHEN** Welcome 中选择的 session 加载失败
- **THEN** 临时 Agent 被删除，界面回到 Welcome，用户可以再次选择并重试。

#### Scenario: Resume from an existing agent or dashboard fails
- **WHEN** 用户从既有 Agent 或 Dashboard 发起 resume 且加载失败
- **THEN** 界面恢复到该发起 surface，既有 Agent 状态不被临时失败页替换。

#### Scenario: In-place reload fails
- **WHEN** 失败结果属于已有 Agent 的原地 reconnect/reload window，而不是首次加载临时 Agent
- **THEN** 继续由既有 reload-window 回滚语义处理，不删除真实 Agent。

证据入口：`crates/codegen/pager/src/app/root/dispatch/session/load.rs` 与 `tests/session/load.rs`。

## MODIFIED Requirements

### Requirement: Ordinary agent status shows session usage
没有 Goal 的普通主会话 Agent 视图 SHALL 在原 Goal 插槽显示本 session 跨 resume 的 lifetime 累计 token 与输入缓存命中率，并在点击时打开 Usage 页。数据 SHALL 来自可恢复 session ledger 的变化投影，不使用定时轮询、context 窗口压力或 prompt 总额累加替代账本。子 Agent 内嵌视图不增加一个指向父会话用量的入口。

#### Scenario: Calls and late child settlement
- **WHEN** 普通会话产生主调用、已归属的子任务消费或不完整标记
- **THEN** 状态栏按账本的 lifetime 累计 input + output（含 cache hit）更新，重复累计快照不会重复加账，缓存率按总 cached input / 总 input 计算。

#### Scenario: Empty or incomplete usage
- **WHEN** 没有输入、缓存数超过输入或账本不完整
- **THEN** 无效比例显示 N/A；不完整累计显示 ≥，缓存率仅表示 recorded usage，不伪装精确完整用量。

#### Scenario: Open usage or Goal details
- **WHEN** 用户点击普通会话用量或已有 Goal 的状态区域
- **THEN** 普通用量打开既有 Usage 面板的 Usage 页，Goal 继续打开 Goal 详情；窄屏下点击区域不能超出可见区域。

#### Scenario: Resume or reconnect
- **WHEN** 客户端重新连接存活进程或在新 actor incarnation 从持久 Timeline 恢复会话
- **THEN** normal 状态栏显示此前与当前 incarnation 的 lifetime 总计，不重置为零或只显示 resume 后新增用量；resident reconnect 不重复累计。

#### Scenario: Usage details across resumes
- **WHEN** session 至少经历一次有新增模型消费的冷 resume
- **THEN** Usage 页顶部显示 lifetime 总计，并按 Initial run、Resume #1… 展示各 incarnation 的新增消费；各段之和与 lifetime 已知总量一致。

证据入口：`crates/codegen/chat-state/src/actor/state.rs`、`shell/src/extensions/usage.rs`、`pager/src/views/usage_modal.rs`、`pager/src/app/status_blocks.rs`。
