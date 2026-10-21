## MODIFIED Requirements

### Requirement: Integrity repair preserves evidence
工具响应完整性修复 SHALL 保留原始事实，并将修复后的 Surface 用于后续模型请求。

#### Scenario: 响应配对损坏
- **WHEN** 接收包含无效工具协议的聚合响应
- **THEN** 保留原始响应与修复事实，下一次请求使用可接受的投影。

证据：`crates/codegen/chat-state/src/actor/tests.rs` — `response_repair_retains_raw_fact_and_allows_the_next_request`。

#### Scenario: Completed results surround model skill metadata
- **WHEN** 同批工具结果被其匹配的 Model SkillLoad 元数据隔开
- **THEN** 当前 repair 仅稳定归整元数据与结果，不删除真实成功正文或将它替换为取消/未执行；归整本身持久化且重放一致，再次修复幂等。

#### Scenario: Missing results and real message boundaries remain distinct
- **WHEN** 结果确实缺失、重复，或被真实用户消息/不匹配元数据隔开
- **THEN** 仍执行原配对规则，不跨真实边界伪造完整交换；已有已提交 IntegrityRepair 精确证明保持可验证，不重写旧事件。
