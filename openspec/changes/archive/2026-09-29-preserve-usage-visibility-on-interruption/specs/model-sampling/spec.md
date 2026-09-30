## ADDED Requirements

### Requirement: Cancellation preserves confirmed attempt usage

Provider attempt 在取消或逻辑期限结束前，若已按当前协议与归一化规则确认完整总消费，采样 SHALL 保留该 usage、缓存字段可用性及已报告费用，并以原 attempt 身份向所有适用账本结算各一次；取消可丢弃候选正文，但 SHALL NOT 仅因未发布响应终态而丢弃已确认的消费。没有可信完整总量时 SHALL 沿既有未知结算规则处理，保留费用与精确预算限制。取消 SHALL NOT 为收集尚未收到的 usage 延长 provider 工作、启动重试或新请求；已准入的 evidence 与结算确认仍遵循现有完成屏障。

#### Scenario: Chat usage observed before a pending stream tail

- **WHEN** Chat 已有 finish reason 并解析确认完整 usage，随后结束帧或 EOF 持续 pending，此时用户取消或 steer
- **THEN** 该 usage 按原 attempt 身份持久结算一次，缓存读写可用性与已报告费用保留；请求按取消结束，不等待剩余尾部、不接纳候选、不执行其工具、不重试。

#### Scenario: Final Chat chunk contains preview and usage

- **WHEN** 同一 Chat chunk 包含最后一段可见输出、finish reason 和完整 usage，用户在该段预览交出后立即取消
- **THEN** 解析器在交出预览前已确认并暂存完整用量；该 attempt 沿取消路径结算 Known，不因解析器在预览 yield 处被丢弃而改记 Unknown。

#### Scenario: Logical deadline during the same tail wait

- **WHEN** 完整 usage 已确认但流尾仍 pending，逻辑采样期限触发取消并转换为期限失败
- **THEN** 失败保留原 attempt 的已确认用量，完成既有 evidence 与账本 ACK 后结束，不把已知消费降为未知，不重新起计时期限。

#### Scenario: Cancellation before trustworthy usage

- **WHEN** provider 已开始工作，但取消时只收到内容或不满足当前协议完整性条件的 usage 中间值
- **THEN** 该 attempt 以未知总消费结算，不用估算 token、缓存命中或零值代替缺失字段；精确预算仍按未知消费关闭后续准入。

#### Scenario: No provider attempt admitted

- **WHEN** 取消发生在任何 provider attempt 获得准入之前
- **THEN** 不因本次取消新增 provider 消费或未知消费结算，沿既有取消生命周期结束。

#### Scenario: Buffered terminal competes with cancellation

- **WHEN** 完整终态与取消同时可见，或取消发生在已确认用量的持久结算 ACK 等待期间
- **THEN** 该 attempt 的已确认用量只结算一次，ACK 屏障不被跳过，不因取消再写一份 unknown，也不因重复终态二次累计。

#### Scenario: Attempt isolation

- **WHEN** 已确认 usage 的 attempt 结束后，同一逻辑请求的另一 attempt 或另一请求在没有完整 usage 时被取消
- **THEN** 后者仍按自己的证据结算为未知，不能借用先前 attempt 的用量或费用。

#### Scenario: Protocol completeness remains authoritative

- **WHEN** Messages 只有 terminal delta、缺少当前完整 usage 所需的 message_stop 或 full input 构成，或者 Responses 尚无可信 terminal usage
- **THEN** 取消继续保留未知总消费；本要求不把中间累计值提升为精确账单，也不把已知账单提升为候选接纳许可。

证据入口：`crates/codegen/sampler/src/stream/chat_completions.rs::stream_chat_completions`、`crates/codegen/sampler/src/actor/request_task.rs::{run_one_attempt,drive_l2}`、同文件 `drive_l2_buffered_terminal_outranks_simultaneous_cancel_and_preserves_usage`；既有结算入口为 `crates/codegen/shell/src/session/actor/turn/sampling.rs::sampling_usage_sink`。Chat 尾部窗口新增场景待实施验证。
