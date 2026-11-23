## ADDED Requirements

### Requirement: Coordination questions select evidence within the prepared route window

Parent, child and peer inquiries SHALL preserve the complete wrapped question and frozen system messages and bound the remaining input by the answering route's configured window, using the Sideband input estimator. Selection SHALL retain a recent suffix of portable evidence, preserving whole projected tool batches and their attached model skill metadata, global call identity checks, and original Surface coordinates. Only selected media SHALL undergo recovery. Each admitted attempt SHALL identify the original revision and its selected context coordinates. Recovery SHALL keep that selection. A locally oversized mandatory or recovered input SHALL settle the opened Sideband as Failed with the estimated input and limit before dispatching that answer attempt.

#### Scenario: Short question arrives after a long history
- **WHEN** the frozen history exceeds the prepared route window but the system messages, complete question and recent tool evidence fit
- **THEN** the receiver retains the fitting recent evidence with every retained call paired to its results, sends the unchanged question, and delivers the answer through the usual receipt and proof.

#### Scenario: Required question input exceeds the window
- **WHEN** the system messages and wrapped question alone exceed the route window
- **THEN** the receiver records a failed inquiry and a failed Sideband with the estimate and limit, makes no answer provider request and produces no successful answer receipt.

### Requirement: Completed children hand off through their final response

Child guidance and rejection feedback for ineligible proactive subagent messages SHALL direct completed work to the ordinary final response and reserve parent inquiries for concrete questions or dependencies.

#### Scenario: A child attempts proactive delivery after finishing
- **WHEN** a completed child tries to send its result using an ineligible proactive message
- **THEN** the rejection identifies the final response as the result channel and keeps the existing eligibility and receipt-bound reply rules.

## MODIFIED Requirements

### Requirement: Inquiry snapshots retain completed tool evidence

协调询问 SHALL 在回答路由预算选中的冻结历史中保留已经提交且可正确配对的工具调用身份、参数、结果、附件和 Assistant 正文，包括连续工具交换的尾部。未返回的调用 SHALL NOT 形成悬空工具协议或被伪造成已完成；同批已完成的交换 SHALL 保留。询问 SHALL 继续无工具执行，生成期间不修改目标主 Surface；成功问答随后按被动通知消费进入目标上下文，不构成权限授予。

#### Scenario: Parent asks about a completed child inquiry
- **WHEN** 子 Agent 的冻结上下文包含已完成的 ask_parent，后面紧接其他完成的工具交换，parent 发起 ask_subagent
- **THEN** 回答请求仍包含 ask_parent 的问题、真实结果和后续已完成的工具证据，不因消息位于尾部而删除。

#### Scenario: Inquiry arrives during a partial tool batch
- **WHEN** 父子或 peer 询问冻结时，同一批工具仅部分返回或全部尚未返回
- **THEN** 回答请求保留已完成交换及 Assistant 正文，省略未完成的结构化调用，不产生虚构结果；冻结和生成不修改目标主 Surface，后续被动同步不拆开主任务的工具批次。
