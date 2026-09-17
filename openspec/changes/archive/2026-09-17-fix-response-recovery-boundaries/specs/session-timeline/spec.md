## ADDED Requirements

### Requirement: Repeated rewind preserves retained response provenance

Timeline SHALL 在一次或多次 rewind 后继续把保留的历史响应关联到原 admission；compaction 或 Surface replacement SHALL NOT 将该归属重置为最近一次 rewind。切出当前 branch 的响应 SHALL 不再成为 replay authority。

#### Scenario: Rewind twice after replacing a later prompt

- **WHEN** 会话保留早期 response，rewind 较晚 prompt，生成替代分支后再次 rewind
- **THEN** 早期 response 的 admission 和重载回放保持一次，两个被切走的 response 均不复活。

#### Scenario: Repeated rewind crosses compacted history

- **WHEN** 保留前缀经过 compaction，随后发生重复 rewind 和冷 fold
- **THEN** 保留 response 的 provenance 与未压缩前缀一致，回放不遗漏也不复制历史响应。

### Requirement: Exact replay commits acknowledge durable barriers

Response projection 和独立 ACP event 的 exact commit SHALL 只有在所需文件及目录同步完成后返回成功。读取到相同 key/payload SHALL 仅证明身份一致，不得代替同步确认。

#### Scenario: Complete record remains readable after sync failure

- **WHEN** append 已写出完整记录，但 file sync 或 directory sync 失败
- **THEN** exact reconcile 在同步仍失败时返回错误，不放行 Accepted、后继 provider 或工具。

#### Scenario: Exact retry after storage recovers

- **WHEN** 同一记录在同步恢复后按原 identity/payload 重试
- **THEN** 同步已有记录后成功且不重复追加；payload conflict 仍拒绝。
