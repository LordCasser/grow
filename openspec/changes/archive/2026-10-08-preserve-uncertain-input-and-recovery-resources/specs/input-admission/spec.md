## ADDED Requirements

### Requirement: User submission retries retain one durable input identity

同一 session 中同一用户提交请求的重试 SHALL 复用稳定 prompt identity、唯一关联的 input identity 和不可变请求快照，覆盖普通 prompt、queue、steer 与 followup。相同 identity/intent/payload 重放 SHALL 返回原接纳与消费状态，不重复 Submitted、Hook 或排队；不同请求内容复用同 identity SHALL 在 Hook 或执行前拒绝。新的显式发送使用新的身份，即使文字相同。

#### Scenario: Admission committed but RPC acknowledgment was lost
- **WHEN** 用户输入已持久接纳，但客户端丢失 ACK 并以原请求重试
- **THEN** 返回原 input identity 与当前状态，不新增接纳、不重复触发对应执行，不重新运行接纳 Hook。

#### Scenario: Same identity carries different input
- **WHEN** 同 session/prompt identity 被用于不同正文、图片、intent 或原始执行路由
- **THEN** 返回身份冲突，保留原输入，新的内容不会触发 Hook 或被排队。

#### Scenario: Concurrent clients repeat one request
- **WHEN** 多个请求并发提交同一稳定身份及内容
- **THEN** 它们共享一个接纳结果或在途 owner，不各自生成新的 input_id。

#### Scenario: User intentionally sends the same words again
- **WHEN** 用户明确新建一次发送意图，内容与先前输入相同但身份不同
- **THEN** 按新输入正常接纳，不依据文字相似性合并。

证据与实施入口：`crates/codegen/shell/src/session/input_inbox.rs::InputPayload`、`crates/codegen/shell/src/session/actor/input_admission.rs::run_human_input_admission_hook`、`crates/codegen/chat-state/src/commands.rs::SubmitInputDurably`、`crates/codegen/pager/src/app/root/dispatch/prompt.rs`。

### Requirement: Unconfirmed input admission requires authoritative reconciliation

输入状态 SHALL 区分请求已 Submitted、接纳未决、已允许、已拒绝、明确未接纳和状态未知。通信失败、运行 handle 缺失或只读快照暂未发现输入 SHALL NOT 单独证明未接纳。明确未接纳必须依据原操作明确拒绝，或完整旧实例退出后在合法 writer 边界验证无接纳事实及无在途操作。恢复 SHALL 保留既有接纳 Hook 的执行证据，不自动重跑结果不明的 Hook。

#### Scenario: Status read overtakes a queued submission
- **WHEN** 状态快照尚未包含一个仍可能进入接纳流程的原请求
- **THEN** 返回未确认，而非授权客户端以新身份自动重发。

#### Scenario: Process exits during admission hook
- **WHEN** Submitted 已持久化，但进程在接纳 Hook 决定确认前退出
- **THEN** 恢复保留原输入与 Hook 证据，并按接纳中断规则处理，不猜测 Hook 未执行并自动重跑。

#### Scenario: Replacement writer proves no admission occurred
- **WHEN** 原实例及在途操作已结束，新 writer 校验原身份没有提交事实
- **THEN** 可明确报告未接纳；后续原请求重试仍使用原身份及内容。

#### Scenario: Input evidence cannot be validated
- **WHEN** 请求关联重复、冲突或 payload/附件不可用
- **THEN** 报告证据不足或输入失效，保持其他可恢复输入，不按文本猜测接纳状态。

证据与实施入口：`crates/codegen/shell/src/session/actor/input_admission.rs`、`crates/codegen/shell/src/session/input_inbox.rs`、`crates/codegen/chat-state/src/timeline.rs::InputEvent/InputBlockReason`。
