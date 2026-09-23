## ADDED Requirements

### Requirement: Agent opinion consumption preserves source and exact context

正式 agent 消息 SHALL 复用 durable Received/Consumed 生命周期。消费与准确的 runtime agent-message context item SHALL 在同一持久事实中提交，保留 receipt、双方身份、reply 关联和正文；不得拆成先确认消费后另写模型输入。发送方既有调用正文 SHALL NOT 再作为自己的接收消息重复注入。

#### Scenario: Consume an opinion
- **WHEN** 目标在安全步骤消费已接收意见
- **THEN** 该 receipt 与有来源的准确正文一起进入 Surface，回复内容不带人类权限证据。

#### Scenario: Restore after consumption
- **WHEN** 消费后崩溃并重建 Session
- **THEN** 同一消息只恢复一次，保留 reply 关系，不重新投递、重新唤醒或伪造新输入。

#### Scenario: Send and receive on the same agent
- **WHEN** agent 发出意见后收到另一方回复
- **THEN** 自己的意见保留在原发送调用/回执中，对方意见作为接收 item；不会为自己的发送增加重复入站正文。
