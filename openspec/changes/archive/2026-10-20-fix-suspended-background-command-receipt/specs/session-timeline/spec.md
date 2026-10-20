## MODIFIED Requirements

### Requirement: Suspension acknowledges a durable tree boundary

挂起 SHALL 在停止后继执行准入后核对参与节点及其持久续跑位置，只在全部必要事实确认后报告 Suspended。重复请求 SHALL 复用同一次挂起身份。ACK 丢失、写入失败、节点不可达或等待期限到达 SHALL 报告未确认，保留恢复依据和仍在途的写入所有权，不推断未写入或虚构成功。部分挂起 SHALL 不自动重启已经停住的节点。

#### Scenario: All participating nodes acknowledge
- **WHEN** root 和所有参与 child 的续跑位置、结算与依赖引用均已持久确认
- **THEN** root 可确认整树 Suspended，资源关闭沿原 writer drain 规则完成，随后退出。

#### Scenario: Checkpoint requires command settlement evidence
- **WHEN** 节点准备提交 checkpoint，且挂起 operation 尚未记录命令结算
- **THEN** Timeline 拒绝 checkpoint；节点先为同一 operation 持久记录一次 CommandsSettled（没有活跃命令时也记录空集合），相同内容的 ACK 重试幂等，冲突内容或不同 operation 被拒绝。

#### Scenario: Command runtime state was retired after settlement
- **WHEN** CommandsSettled 已持久确认，命令 runner 已退役或其完成缓存已淘汰，但 checkpoint 写入或 ACK 需要重试
- **THEN** 从同一 operation 的持久结算复用原 call/command 身份、完成或中断证据、后台属性与有界输出；不再次终止命令，不依赖已退役的内存缓存，不将已有证据改为空集合或 Unknown。

#### Scenario: Background command settlement reaches a durable checkpoint

- **WHEN** 挂起已持久记录一个后台命令的 CommandsSettled，且该命令的完成通知尚未接纳
- **THEN** 挂起 SHALL 以 Timeline 接受的终态通知身份持久接纳该结果，再提交节点 checkpoint；相同 operation 的重试 SHALL 复用命令结算和通知，不重新运行命令或写入第二个逻辑通知。

#### Scenario: Root acknowledgment is lost
- **WHEN** 所有节点已保存，但 root 挂起提交的 ACK 丢失
- **THEN** 重试或重启核对同一挂起事实，不追加另一份逻辑任务，不重新执行已保存的工作。

#### Scenario: Process exits during partial suspension
- **WHEN** 挂起意图已提交但只有部分节点保存后进程退出
- **THEN** 恢复先核对原节点与 writer，补齐可验证的挂起事实；无法证明续跑位置的节点保持恢复受阻，不将整树当作完整 checkpoint 自动启动。

#### Scenario: Checkpoint binds the exact runtime source
- **WHEN** SessionActor 持久化运行时 checkpoint
- **THEN** checkpoint 包含工作区、Agent 定义及其跳过序列化的能力源、selector/plugin、role prompt 与模型路线的 64 位十六进制 source hash；原始配置、凭据与 prompt 正文不进入 checkpoint，source hash 格式无效时 Timeline 拒绝写入。

目标入口：`crates/codegen/shell/src/session/timeline_persistence.rs`、`crates/codegen/shell/src/session/actor/spawn.rs`；复用 `session-timeline` 的完整 writer drain、exact ACK 和收尾核对契约。
