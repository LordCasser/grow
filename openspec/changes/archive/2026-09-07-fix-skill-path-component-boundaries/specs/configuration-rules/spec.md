## ADDED Requirements

### Requirement: Skill configuration path component boundaries
技能添加清理 ignore 与技能路径计数 SHALL 使用路径组件包含关系，不能将仅共享字符串前缀的相邻路径视为祖先或后代。

#### Scenario: Add beside ignored directory
- **WHEN** 添加 foo，ignore 包含 foobar 和 foo 的祖先、后代或自身
- **THEN** 保留 foobar，仅移除与 foo 真正重叠的 ignore，重复添加不重复记录路径。

#### Scenario: Count source skills
- **WHEN** 统计 foo 下的技能，同时存在 foobar 下的技能
- **THEN** 只统计 foo 本身或其后代，添加响应复用相同语义。
