## ADDED Requirements

### Requirement: Runtime observations preserve query uncertainty and ownership

会话与子任务状态查询 SHALL 区分已确认观察、已确认不存在、不可达、超时及证据无效，并明确不存在所指的 runtime、prompt 或持久记录范围。观察 SHALL 保留 session、适用的 child/prompt identity 及运行实例身份；不得把通信错误、注册表缺项或取消发送失败解释为持久任务已结束。查询 SHALL 在既有授权边界内处理。

#### Scenario: Runtime registration is absent but history exists
- **WHEN** 运行实例已移除，但授权会话仍有可验证持久记录
- **THEN** 查询区分实例不存在和记录存在，返回可确认的持久状态，不报告整个 session 不存在。

#### Scenario: Coordinator cannot answer
- **WHEN** 查询发送失败、响应丢失或超过期限
- **THEN** 返回对应的不可达或超时，保持执行 outcome 未确认，不据此生成 interrupted 或 cancelled 终态。

#### Scenario: Caller lacks source access
- **WHEN** 调用者无权查询目标来源
- **THEN** 返回授权拒绝，不通过状态分类泄漏目标是否存在。

证据与实施入口：`crates/codegen/tools/src/implementations/grow_build/task/backend.rs::inspect/query/cancel`、`crates/codegen/shell/src/extensions/session_admin.rs::handle_prompt_status`。上述区分为本 change 新增契约。

### Requirement: Session health observation remains bounded and read only

状态观察 SHALL 分别报告可确认的执行、持久提交、交付和观察新鲜度。控制查询 SHALL 在总计不超过五秒或调用方更早期限内结束等待，包含排队与加载等待；其完成不得依赖被观察的模型、工具、hook 或写入操作结束。查询并发与快照积压 SHALL 有界。读取持久会话信息 SHALL 遵循只读观察契约，不隐式恢复执行或修改磁盘投影。

#### Scenario: Persistence acknowledgment never arrives
- **WHEN** 会话写入 ACK 一直不返回且运行实例尚未退出
- **THEN** 状态查询仍能返回最后确认的提交位置和观察限制，或在期限内返回超时，不能永久排在该 ACK 后面。

#### Scenario: Durable metadata read exceeds the deadline
- **WHEN** 无 handle 的会话只读扫描超过查询期限
- **THEN** 调用方获得超时；仍在运行的读取保留其工作许可，后续请求不能绕过并发上限，也不启动 writer 修复。

#### Scenario: Execution returned before result publication
- **WHEN** owner 已确认执行返回，但结果提交尚未确认
- **THEN** 观察同时保留这两个事实，不显示为仍等待模型，也不发布未确认的 durable Finished。

证据与实施入口：`crates/codegen/shell/src/session/handle.rs`、`crates/codegen/shell/src/session/actor/spawn.rs::SessionThread`、`crates/codegen/shell/src/session/storage/jsonl/mod.rs`；既有只读与 writer drain 契约继续适用。
