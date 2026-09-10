## ADDED Requirements

### Requirement: Compaction retains structured completed tool history

局部压缩保留 tail 中完整的本地工具往返时，后续请求 SHALL 保留其结构化调用/结果及顺序，同时继续撤销旧 native reasoning 和签名。摘要范围说明及已获准下一 Step 的 AutoContinue 规则保持；压缩 SHALL NOT 重新执行历史工具。

#### Scenario: Async publication retains a recent tool result
- **WHEN** 工具执行后异步摘要在 Step 边界发布，完整工具往返留在 tail
- **THEN** 下一请求仍有配对工具协议、摘要范围说明及应有的 AutoContinue；本次工具只执行一次。

#### Scenario: Surface replacement and replay
- **WHEN** 压缩范围替换完成或随后恢复会话
- **THEN** 保留的工具事实从 Timeline 以结构化中性历史投影，不从旧 artifact 或相同模型名恢复 native。
