## ADDED Requirements

### Requirement: Expanded skill reference index reflects loaded blocks
技能展开提示的引用索引 SHALL 只列出实际成功生成正文块的技能，且保持成功顺序。

#### Scenario: Mixed successful and failed expansions
- **WHEN** 多技能请求中有可加载技能、缺失文件或消失的目录条目
- **THEN** 正文与引用索引只包含加载成功项，不能将失败项列为已加载。

#### Scenario: All expansions fail
- **WHEN** 所有技能均未生成正文块
- **THEN** 继续返回 None，不生成空引用信封。
