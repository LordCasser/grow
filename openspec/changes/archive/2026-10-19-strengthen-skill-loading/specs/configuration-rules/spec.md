## MODIFIED Requirements

### Requirement: Agent skill preloading respects disabled entries
Agent skills 声明解析 SHALL 在名称解析后尊重选定技能的 enabled 状态。禁用条目 SHALL 不被预加载或注入提示词，且不得因禁用回退到另一个同名条目。显式声明的技能缺失、歧义、禁用或正文读取失败 SHALL 作为明确配置错误使 Agent 构建失败，不得仅写日志后继续构建一个缺少声明依赖的 Agent。

#### Scenario: Disabled skill explicitly named
- **WHEN** agent 声明匹配禁用的 native 技能或 qualified plugin 技能
- **THEN** 该技能不进入预加载结果或提示词，构建返回明确的无法加载错误。

#### Scenario: Disabled entry precedes enabled name collision
- **WHEN** 已解析名称选中禁用技能，后续目录还有同名启用技能
- **THEN** 构建明确失败，不加载后续同名技能；独立构建中单独声明的启用技能仍正常加载。

#### Scenario: Missing or unreadable declared skill
- **WHEN** agent skills 声明的名称无法唯一解析或正文读取失败
- **THEN** 构建返回指明技能名称与原因的错误，不把未满足的依赖静默省略。
