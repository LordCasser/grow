## ADDED Requirements

### Requirement: Accepted response history is rebuilt from Timeline authority

每个带 response admission identity 的 canonical assistant response SHALL 具有 versioned、可校验、可重建的 replay projection。`updates.jsonl` SHALL 只保存该 projection 的 cache record，不得把 provisional provider chunks 或 public SamplingAttempt lifecycle 当作第二份 response authority。所有 production replay 入口 SHALL 在建立 replay snapshot、cursor cutoff 或释放 buffered live events前，以 Timeline identity、event 与 digest 校验并补齐缺失 projection。

#### Scenario: Process stops after Timeline admission

- **WHEN** Timeline 已 durable 接纳 response，但进程在 projection record 提交或 public Accepted 前停止
- **THEN** cold load 从同一 Timeline response 合成并展示恰好一份 accepted history；不得调用 provider、继续 truncation/pause-turn 或执行工具。

#### Scenario: Projection append acknowledgment is lost

- **WHEN** projection record 可能已提交但 ACK 丢失
- **THEN** 系统按 response identity、Timeline event、digest 和 projection version exact reconcile；同内容不重复，不同内容 typed conflict 并 fail closed。

#### Scenario: Candidate output precedes durable projection

- **WHEN** text、reasoning 或 fallback candidate 已向支持撤回的 live client 发布，但 projection durable ACK 尚未返回
- **THEN** candidate 仍可撤回且不作为 accepted cache；ACK 前不发布 Accepted，不开始后续 provider request、工具或 Turn success。

#### Scenario: Quarantined response is replayed

- **WHEN** Timeline admission 的 deterministic quarantine result 非零
- **THEN** projection 记录已处理/Discarded disposition，不把 raw malformed tool preview 重建成 accepted 或 executable history；现有安全 repair/diagnostic 语义保持。

#### Scenario: Tool-bearing response has later tool history

- **WHEN** accepted response 包含工具调用且 cache 中存在后续 ToolCall/ToolCallUpdate 或 tool result 展示
- **THEN** response projection boundary 在这些记录之前恢复，reconciliation 不重新 dispatch 工具，既有工具结果只显示一次。

#### Scenario: Earlier attempt was discarded

- **WHEN** 同一 request 的较早 attempt 只有 transient candidate，而较后 attempt 具有 Timeline response admission
- **THEN** 只按 exact `{request_id, attempt}` 重建较后 response；较早 candidate 不进入 replay。

#### Scenario: Rewind or legacy response

- **WHEN** identity-bearing response 已被 rewind 切出当前 branch，或历史 response 没有 admission identity
- **THEN** reconciliation 不复活 rewound response，也不通过文本、位置或邻近 request 猜测 legacy projection。

#### Scenario: Direct replay without writer authority

- **WHEN** 子任务视图、导出或其他 read-only production reader 读取存在缺失 projection 的会话
- **THEN** 使用同一 Timeline-derived projector 在内存中返回完整去重历史，不获取 writer lease或修改持久数据。

#### Scenario: Cursor intersects one projection record

- **WHEN** reconnect cursor 位于一个可展开为多条 ACP update 的 response projection 内部，且无法证明其余 update 已应用
- **THEN** replay 回退为完整历史替换，不跳过半个 response，也不在 later event 之后补发缺失前件。
