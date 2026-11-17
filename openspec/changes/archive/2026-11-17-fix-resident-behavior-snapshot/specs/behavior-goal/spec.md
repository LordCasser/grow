## MODIFIED Requirements

### Requirement: Current Behavior restores independently of display history

成功挂起 SHALL 保留最后已确认的 Behavior 及其有效 Plan 状态。冷恢复或 resident reconnect SHALL 从当前控制 owner 向客户端提供已确认的 Behavior、Plan phase 和 completed-plan 状态，不依赖任何历史页包含模式切换记录。旧截点或旧绑定的展示数据 SHALL NOT 覆盖当前控制；恢复本身 SHALL NOT 伪造一次选择、审批、Goal restart 或 Workflow 执行。健康且不改变Control的Timeline写入正在等待持久ACK时，SHALL NOT 将该in-flight写入单独作为已确认Behavior不可用的依据；真实写入故障、未确认Control边界和actor失效仍须保持原确认保护。

#### Scenario: A selected Behavior lies before the recent page
- **WHEN** 用户选择非 Normal Behavior，产生超过最近页范围的历史，Ctrl+D 成功后在新进程 resume
- **THEN** 当前控制、后续模型行为和用户界面恢复原 Behavior 及有效 Plan phase，即使最近页尚未返回或没有任何模式记录。

#### Scenario: A newer live selection precedes load completion
- **WHEN** 同一 actor 的较新 Behavior 更新先于旧 load 快照或旧历史页到达
- **THEN** 客户端保持较新状态，不因恢复展示回退模式或结算另一项本地控制意图。

#### Scenario: A Plan phase transition waits for persistence
- **WHEN** Submit、Amend 或审批决定的候选阶段正在等待持久确认，或写入失败
- **THEN** 当前控制查询和实时通知仍只反映上一个已确认阶段与版本；成功确认后两者同步推进，失败不暴露候选阶段。

#### Scenario: Reconnect observes a resident owner
- **WHEN** 用户重新连接仍存活的 actor
- **THEN** 当前 Behavior 来自该 actor，而非用磁盘或已加载正文重新推导；原 Turn 与控制身份不改变。

#### Scenario: Resident reconnect overlaps a healthy unrelated append
- **WHEN** 同一resident actor的询问或其他非Control事实正在健康等待持久ACK，同时客户端load查询当前Behavior
- **THEN** 查询返回该actor上一次已确认的epoch、revision、Behavior与Plan状态，不因无关append in-flight而失败，也不暴露候选Control或伪造新的选择。

证据入口：`crates/codegen/shell/src/session/actor/session_mode.rs`、`actor/spawn.rs`、`agent/mvp_agent/acp_agent.rs`；`crates/codegen/pager/src/app/acp_handler/session_notification.rs`。
