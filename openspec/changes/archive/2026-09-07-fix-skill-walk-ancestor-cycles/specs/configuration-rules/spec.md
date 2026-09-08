## ADDED Requirements

### Requirement: Skill walks do not revisit ancestor directories
技能递归扫描 SHALL 在加入技能文件及递归之前拒绝指向当前祖先目录的链接，使用规范目录身份；不同非祖先别名入口 SHALL 保留既有词典序与发现行为。

#### Scenario: Child links back to scan root
- **WHEN** 子目录链接回已有 SKILL.md 的扫描根
- **THEN** 不经链接再次加入根技能或重复递归，正常子技能仍发现。

#### Scenario: Independent aliases share a target
- **WHEN** 两个非祖先链接指向同一技能目录
- **THEN** 两入口仍按词典序发现，后续身份去重负责消除重复文件。
