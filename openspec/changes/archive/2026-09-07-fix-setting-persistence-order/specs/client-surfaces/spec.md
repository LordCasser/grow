## ADDED Requirements

### Requirement: Sequential setting choices retain persistence ownership
同一普通设置的连续保存 SHALL 有确定顺序，较早失败 SHALL 不覆盖用户较新的待保存选择。所有待保存修改失败后 SHALL 恢复最后确认状态，而非未保存的中间值。

#### Scenario: Earlier failure with newer choice pending
- **WHEN** 同一设置已有更新选择，较早保存失败
- **THEN** 最新选择继续可见并继续保存。

#### Scenario: Consecutive failures
- **WHEN** 同一设置的连续修改均保存失败
- **THEN** UI 恢复到这些修改之前的最后确认状态。

#### Scenario: Latest queued choice and recursive reset
- **WHEN** 一个设置保存进行中，用户反复修改该设置，或通过递归 reset 动作修改
- **THEN** 每个 key 最多一个请求在途并保留最新排队选择，递归动作不重复登记；其他 key 可独立保存。
