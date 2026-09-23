## ADDED Requirements

### Requirement: Sampling recovery stop evidence survives session storage

会话存储 SHALL 接受采样器持久记录的 `sampling_evidence/recovery_stop`，在恢复及导入导出中保留原始恢复决定，并继续校验记录名称、类型及 artifact 引用完整性；停止恢复记录 SHALL NOT 被当作已接纳的模型响应或重发请求的指令。

#### Scenario: Load a session after recovery stops
- **WHEN** 会话 Timeline 包含格式有效的 recovery_stop 证据
- **THEN** 观察加载和写者恢复均能读取该会话，证据保持原值，不因该记录启动 provider 请求或增加模型上下文。

#### Scenario: Transfer mixed sampling evidence
- **WHEN** 导入导出的 Timeline 同时包含 recovery_stop 和带 body 的 response 证据
- **THEN** 保留停止决定并完整校验二进制 artifact；缺失或篡改 body 仍被拒绝。

#### Scenario: Invalid evidence remains invalid
- **WHEN** 证据类型未知、名称与类型不一致或分块引用不合法
- **THEN** 读取失败并保留原始记录，不以支持 recovery_stop 为由跳过校验。

证据入口：`crates/codegen/shell/src/session/sampling_evidence.rs::decode_record`、`crates/codegen/shell/src/session/storage/jsonl/tests.rs`、`crates/codegen/shell/src/extensions/session_state.rs::sampling_blob_tests`。
