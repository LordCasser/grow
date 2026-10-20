## ADDED Requirements

### Requirement: Preference rollback uses the displayed effective default
Settings 保存与失败回滚 SHALL 对 show_tips 使用 false、对 auto_update 使用 true 作为未配置时的有效默认值，与菜单读取保持一致。已经成功确认的值 SHALL 在之后保存失败时恢复并正确显示，同值重试 SHALL 仍可保存。

#### Scenario: Show tips reset fails after successful opt in
- **WHEN** show_tips 已成功保存 true，随后重置 false 的保存失败
- **THEN** Settings 恢复显示 true，重新重置仍能发起保存。

#### Scenario: Auto update reset fails after successful opt out
- **WHEN** auto_update 已成功保存 false，随后重置 true 的保存失败
- **THEN** Settings 恢复显示 false，重新重置仍能发起保存。

#### Scenario: First preference write fails
- **WHEN** show_tips 或 auto_update 未配置，首次修改保存失败
- **THEN** 分别恢复显示 false 或 true，再次选择失败的目标值能重新保存。
