## ADDED Requirements

### Requirement: Inquiry snapshots retain completed tool evidence

协调询问 SHALL 在冻结上下文中保留已经提交且可正确配对的工具调用身份、参数、结果、附件和 Assistant 正文，包括连续工具交换的尾部。未返回的调用 SHALL NOT 形成悬空工具协议或被伪造成已完成；同批已完成的交换 SHALL 保留。询问 SHALL 继续无工具执行，不修改目标主 Surface，也不将询问回答视为权限授予。

#### Scenario: Parent asks about a completed child inquiry
- **WHEN** 子 Agent 的冻结上下文包含已完成的 ask_parent，后面紧接其他完成的工具交换，parent 发起 ask_subagent
- **THEN** 回答请求仍包含 ask_parent 的问题、真实结果和后续已完成的工具证据，不因消息位于尾部而删除。

#### Scenario: Inquiry arrives during a partial tool batch
- **WHEN** 父子或 peer 询问冻结时，同一批工具仅部分返回或全部尚未返回
- **THEN** 回答请求保留已完成交换及 Assistant 正文，省略未完成的结构化调用，不产生虚构结果，主 Surface 保持原样。

证据入口：`crates/codegen/shell/src/session/actor/coordination.rs` — `handle_coordination_inquiry` 与真实 provider 请求回归。
