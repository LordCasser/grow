## MODIFIED Requirements

### Requirement: Suspended trees resume the original identities automatically

用户通过执行型 resume 打开已挂起 root 时，系统 SHALL 先验证并重建原树的全部未结束节点、运行配置、工作区、等待与交付关联，再允许可运行节点继续。不得通过新 child identity、`resume_from` 派生任务或重新接纳原用户 prompt 代替恢复。重复或并发 resume SHALL 不启动重叠 writer、重复 child 或重复结果交付。只读查询、export、replay 和历史 child 查看 SHALL 不触发执行。

#### Scenario: Resume the suspended tree
- **WHEN** 用户恢复已确认挂起的 root，且全部必要来源与运行环境有效
- **THEN** 未结束 child 自动以原逻辑身份继续，完成 child 不再执行，父 Agent 按原依赖继续等待或消费原结果，无需模型重新规划或用户逐个恢复。

#### Scenario: A required node cannot be restored
- **WHEN** child 来源、运行路线、工作区或恢复 artifact 缺失、不匹配，或旧 writer 尚未退出
- **THEN** 整树保持执行准入关闭并指出受阻节点与原因；不静默换模型、创建空白 child 或先启动部分树。

#### Scenario: Resume is repeated after work has begun
- **WHEN** 同一挂起状态的 resume 请求重复、ACK 丢失或两个客户端并发恢复
- **THEN** 复用已接纳的恢复实例或返回其状态，不再次创建执行、消费输入或发出完成通知。

#### Scenario: A resumed runtime crashes without another suspension
- **WHEN** 挂起恢复资格已消费，节点已重新运行，随后无新的挂起事实就异常退出
- **THEN** 根据最新执行证据走既有异常恢复，不回退旧 checkpoint 或重发已派发工作；旧挂起记录不授予永久自动重启资格。

目标入口：`crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs::load_session`、`crates/codegen/shell/src/agent/subagent/handle_request.rs`、`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs`。

#### Scenario: Resume during a background child completion notification
- **WHEN** a parent is suspended during the internal Turn triggered by completed child A while background child B remains unfinished, and the root is cold resumed
- **THEN** A retains its unique completed result, B continues automatically under its original child and Turn identities, and the parent retains its notification Turn; suspension creates no cancelled task result, re-admitted original user prompt, replacement Spawn or repeated command execution.
