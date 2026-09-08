## ADDED Requirements

### Requirement: Managed config rollback preserves observed conflicts
托管配置事务 SHALL 在回滚前核对已发布文件身份、字节和权限；观察到变化或符号链接替换时 SHALL 保留当前目标、保留已有原始备份并报告实际备份路径，不继续覆盖或删除目标。该检查 SHALL NOT 被描述为对非协作写入者的原子 CAS。

#### Scenario: Existing target changes after publication
- **WHEN** 发布失败后的恢复发现目标已被外部编辑、替换或删除
- **THEN** 返回恢复错误，保留现状及实际原始备份，不覆盖外部内容。

#### Scenario: New target changes after publication
- **WHEN** 本次创建的目标在回滚前发生可观察变化
- **THEN** 返回恢复错误并保留现状，不删除新内容。

#### Scenario: Published target remains unchanged
- **WHEN** 发布后发生错误且目标仍为本事务发布的身份、内容和权限
- **THEN** 按既有规则恢复原内容或移除本次新建文件。
