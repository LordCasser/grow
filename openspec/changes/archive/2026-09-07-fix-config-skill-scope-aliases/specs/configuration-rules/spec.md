## ADDED Requirements

### Requirement: Config skill scope uses resolved source identity
配置技能 SHALL 根据配置根与仓库根的规范路径包含关系分类 Repo/User，不能因符号链接写法改变同一根的分类；扫描输入原文 SHALL 保持不变。

#### Scenario: External alias points into repository
- **WHEN** 配置路径在仓库外，但链接到仓库内现存技能根
- **THEN** 该配置来源分类为 Repo。

#### Scenario: Repository alias points outside
- **WHEN** 配置路径文本在仓库内，但链接到仓库外技能根
- **THEN** 该配置来源分类为 User。
