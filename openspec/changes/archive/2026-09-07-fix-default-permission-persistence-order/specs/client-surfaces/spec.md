## ADDED Requirements

### Requirement: Default permission writes use ordered settings persistence
默认权限修改 SHALL 使用普通设置的顺序保存和回滚基线协调，且 SHALL 不改变当前会话权限或发送会话权限通知。

#### Scenario: Earlier default permission write fails
- **WHEN** 用户连续选择不同默认权限，较早保存失败
- **THEN** 最新选择继续可见并保存，连续失败回到最后确认默认值。

#### Scenario: Active session permission remains separate
- **WHEN** 默认权限保存成功或失败
- **THEN** 当前会话权限不变，未来会话仍按最终保存的默认配置初始化。
