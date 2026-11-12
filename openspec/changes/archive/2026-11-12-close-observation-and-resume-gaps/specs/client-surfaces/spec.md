## MODIFIED Requirements

### Requirement: Resume displays and restores suspended work

执行型 resume SHALL 自动恢复所选挂起 root 的任务树，显示恢复中、已运行或具体受阻原因。挂起的 child SHALL 显示为挂起而非取消；操作中断与 Agent 任务状态 SHALL 分别展示。普通历史浏览 SHALL 不启动 child、provider 或工具。恢复提示 SHALL 不冒充新的用户任务、授权或重复完成通知。Tasks 面板 SHALL 从所选 Timeline 历史恢复已结算后台命令的终态。

#### Scenario: Resume after terminal application update
- **WHEN** 用户退出后更新 Terminal app，再通过 `grow resume` 打开挂起会话
- **THEN** 原任务树自动续接，已完成结果仍可见，未完成 child 无需逐个手动重启，原父等待关系保持。

#### Scenario: A command could not be confirmed stopped
- **WHEN** 挂起记录包含 OutcomeUnknown 命令
- **THEN** 同时显示任务可恢复与该操作结果未知，不用“已冻结”暗示外部命令已经停止。

#### Scenario: Resume after a background command was settled during suspension
- **WHEN** resume 所选历史保留后台命令的开始记录和同一 task ID、tool-call ID 的 `CommandsSettled` 终态，包括来自较早挂起操作的结算
- **THEN** Tasks 面板恢复同一行的终态及记录时间，已中断命令不显示为 Running，已有普通完成记录时不重复投影，不重新执行命令或发送完成通知。

目标入口：`crates/codegen/pager/src/app/acp_handler/`、`crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs::load_session`。

#### Scenario: A foreground command moved to the background before suspension
- **WHEN** 恢复已结算的命令且该命令曾在前台执行
- **THEN** Tasks 按结算中的真实起止时间展示完整耗时。旧结算缺失时间时使用现有记录时间，仍恢复终态。
