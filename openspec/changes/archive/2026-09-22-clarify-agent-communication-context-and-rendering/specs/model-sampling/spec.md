## ADDED Requirements

### Requirement: Runtime agent messages project to complete tool exchanges

有来源的 runtime agent-message context item SHALL 在模型请求中投影为完整、可配对的专用收件工具调用/结果，使用从 receipt 确定的稳定身份。该投影 SHALL NOT 冒充模型主动执行或触发工具 dispatch，不得追加孤立 ToolResult 或篡改其他真实工具的结果。canonical Surface 坐标 SHALL 不因 provider 一对多展开改变。

#### Scenario: Receiving a message during a tool batch
- **WHEN** 消息到达而现有真实工具批次尚未闭合
- **THEN** 消息在安全边界加入请求，完整收件调用/结果对不插断原工具配对。

#### Scenario: Provider switch and portable history
- **WHEN** 含已消费 agent 消息的请求切换现有 provider adapter 或供 Sideband 冻结
- **THEN** 完整正文、来源和稳定身份保留，portable projection 不把它作为孤立结果丢弃，也不执行历史调用。

#### Scenario: Compaction and source permissions
- **WHEN** 对含 runtime agent message 的 Surface 进行 compaction 或权限判定
- **THEN** input_ref 仍指向 canonical 事实，消息不获得 DirectUser/Interjection 权限证据；一对 provider item 不生成两份消费坐标。
