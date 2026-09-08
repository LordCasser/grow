## ADDED Requirements

### Requirement: Agent skill preloading respects disabled entries
Agent skills 声明解析 SHALL 在名称解析后尊重选定技能的 enabled 状态。禁用条目 SHALL 不被预加载或注入提示词，且不得因禁用回退到另一个同名条目。

#### Scenario: Disabled skill explicitly named
- **WHEN** agent 声明匹配禁用的 native 技能或 qualified plugin 技能
- **THEN** 该技能不进入预加载结果或提示词。

#### Scenario: Disabled entry precedes enabled name collision
- **WHEN** 已解析名称选中禁用技能，后续目录还有同名启用技能
- **THEN** 不加载后续同名技能；单独声明的启用技能仍正常加载。
