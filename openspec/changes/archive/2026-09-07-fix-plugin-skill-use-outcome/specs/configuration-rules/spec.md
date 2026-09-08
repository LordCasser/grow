## ADDED Requirements

### Requirement: Plugin skill usage reports expansion outcome
Turn admission 的插件技能使用诊断 SHALL 按实际正文展开结果设置 PluginUsed.success，不得在加载前无条件宣称成功。

#### Scenario: Plugin skill expansion fails
- **WHEN** 请求的插件技能正文读取失败或目录条目已消失
- **THEN** 对应 PluginUsed.success 为 false。

#### Scenario: Mixed plugin expansion results
- **WHEN** 同一请求中部分技能成功生成正文块、部分失败
- **THEN** 逐引用分别记录成功或失败，不以整体存在正文代替单项结果。

#### Scenario: Skill interjection
- **WHEN** 技能通过 interjection 注入运行中的 turn
- **THEN** 保留派发诊断，不新增代表 turn 归属的 PluginUsed 事件。
