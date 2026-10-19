## Why

用户为更新 Terminal app 等主动退出 Grow 时，需要暂停整个 session 及其子 Agent 树，并在下一次 `grow resume` 后自动继续原任务。当前 Ctrl+C、shutdown 和冷恢复会取消或关闭执行范围，已有子 Agent `resume_from` 又要求终态来源，不能保留原任务树的执行与等待关系。

用户已选择快速退出：中止当前采样和外部命令，不等待测试、构建自然完成；无法确认结果的操作保留未知，恢复后检查现场再处理。

## What Changes

- **BREAKING**：执行会话的全局 Ctrl+C 改为快速挂起并退出；重复按键合并到同一挂起请求，不再先取消 turn 或升级为强退。显式 Stop Turn / Stop Goal / Cancel task 保持真正取消的含义。
- 增加 Running → Suspending → Suspended → Restoring → Running 的会话运行生命周期。挂起不产生 child 的 cancelled/completed 结果，也不改变 Goal 的业务状态。
- 持久化 root、递归 child、前后台归属、当前续跑位置、等待关联、待交付结果及严格校验的 64 位十六进制 runtime source hash；由现有 Timeline 和执行 owner 持有，不引入第二套 TaskStore。
- `grow resume` 自动加载原任务树，保留 session/task identity、工作区、上下文和已完成结果，重新绑定原 child 的等待和通知；不通过新建 child 或重发原任务 prompt 模拟恢复。
- 已派发操作区分已完成、已确认中断与结果未知；尚未派发的调用保留待执行状态，重新检查当前权限。未知副作用不自动重试。
- 活跃命令的终止、结果文件和归属先以同一挂起 operation 的 `CommandsSettled` 事实持久确认，再允许节点 checkpoint；无活跃命令也显式记录空结算，ACK 重试仅复用相同内容。
- 挂起取消、保存和退出准备共用默认五秒期限；超时、ACK 不明或保存失败明确报告未完成，不能以强退或丢弃结算声称冻结成功。
- 保留已接纳输入队列、用户等待和必要恢复资源。Workflow Run 与 Goal 的既有执行权威参与挂起，普通只读 replay/export 不启动恢复。

## Capabilities

### New Capabilities

无。挂起是现有 session/runtime 生命周期的扩展。

### Modified Capabilities

- `client-surfaces`：Ctrl+C/退出、挂起反馈、自动恢复展示及既有按键契约调整。
- `session-timeline`：树级挂起事实、同身份 Turn 续跑、操作中断、结果交付、恢复保护与异常恢复的分流。
- `input-admission`：挂起保留已接纳输入与消费身份，恢复后继续原仲裁。
- `behavior-goal`：会话挂起与 Goal 业务状态分离，保留预算和用量。
- `model-sampling`：挂起时中止 attempt 并结算；未接纳步骤仅在原安全条件、预算及期限允许时重新采样。
- `tool-authorization`：恢复权限等待与当前授权复核，不复用旧一次性 permit。
- `workflow-execution`：同 Run 的会话级挂起、journal/child 关联与原业务等待状态恢复。

## Impact

- Pager root input/quit、OS signal 转正常退出、ACP load/control，以及退出 guard。
- Shell SessionActor 的准入、turn/tool loop、shutdown、冷加载、子 Agent runner 与 Workflow owner。
- ChatState Timeline schema/fold/恢复校验；tools child coordinator 的挂起及同身份重新绑定；sampler 的取消结算。
- 后续实现须更新 `docs/development.md`、`docs/architecture/agent-core-timeline.md` 及相关用户按键说明。
- 不增加外部依赖、通用工具重放框架、进程内存快照、跨设备执行、后台常驻服务或存储迁移项目；旧 MCP 盲重试债务不在本 change 顺带实施。

## Status

2026-10-18：按用户最新要求，本次只交付 OpenSpec change。产品代码和运行测试留待后续统一实施；不修改主规范、不归档。静态核对基线为 `0a0dd07cd3aa35d39365ae971df8f9b316185c9d`，验证范围见 `verification.md`。
