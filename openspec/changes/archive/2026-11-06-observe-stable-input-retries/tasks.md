# Tasks: observe-stable-input-retries

- [x] 1. Shell 接纳：canonical `input_state` 只读命中核对替换 blob 遍历扫描；
      FIFO Prompt/Followup 派生互认；冲突语义保留（payload/Steer/route）。
- [x] 2. `PromptCompletionKind::AdmissionReplay` 与 `queue_input` 重试短路
      （先应答后 persist_ack）；queue 编辑保存显式处理 replay。
- [x] 3. ACP `prompt` persist_ack 接线：biased 应答优先、Working 仅在 durable
      ACK 后、AdmissionReplay 短路应答与 `grow/inputAdmission` meta、无假
      Idle/TurnCompleted/usage。
- [x] 4. Tier 2/3 验证：统一actor相关96项、queue编辑replay反例及真实CLI ACP入口均通过，见 verification.md；独立review发现已处理。
- [x] 5. 只对自有文件格式化、定向编译与测试通过；父严格校验并归档，开发者说明与backlog同步。
