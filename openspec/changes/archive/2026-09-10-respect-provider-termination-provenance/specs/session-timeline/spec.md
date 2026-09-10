## ADDED Requirements

### Requirement: Turn terminals identify their actual authority

新 Turn terminal SHALL 明确区分 provider 响应驱动、宿主控制/错误、用户取消和中断恢复。provider 驱动终态 SHALL 关联相同 Turn 的已完成 request，其原始 provider terminal 由 request/attempt 证据保存。宿主 stop_reason/completion_kind SHALL 仅是本地分类。历史未记录来源的终态 SHALL 保持未知，不据旧字符串猜测或回写。

#### Scenario: Natural response closes the turn
- **WHEN** 完整正常 provider 响应使普通 Turn 收尾
- **THEN** Turn terminal 标识 Provider 来源并关联其 request，可追溯到对应 attempt 的原生终止。

#### Scenario: Host stops after a provider response
- **WHEN** provider 已结束，但宿主因预算、控制、持久化错误或用户取消收尾
- **THEN** 保留原始响应证据，同时追加宿主或用户来源的 Turn terminal，不覆盖原始原因。

#### Scenario: Process recovery closes an open turn
- **WHEN** 重启时发现未闭合生命周期
- **THEN** 追加 Recovery 来源的 interrupted 终态；不伪造 provider 事件或正常成功。

#### Scenario: Historical source is absent
- **WHEN** 读取没有记录来源的旧终态
- **THEN** 来源保持 Unknown，不把 end_turn 推断为 provider 返回。
