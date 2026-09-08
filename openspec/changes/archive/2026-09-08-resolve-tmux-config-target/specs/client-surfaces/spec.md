## ADDED Requirements

### Requirement: Tmux fix targets have explicit provenance
tmux自动修复 SHALL 将选择后的配置目标固定用于诊断、预览、写入和重载提示；无法确定目标时 SHALL 提供显式路径选择方式，不静默改写猜测的默认文件。

#### Scenario: Explicit custom target
- **WHEN** 用户为tmux修复指定合法配置目标
- **THEN** 所有阶段使用该目标，保持已有确认与事务检查。

#### Scenario: Ambiguous server configuration evidence
- **WHEN** 配置来源查询不可用、为空或无法无歧义确定目标
- **THEN** 说明无法自动确定并要求显式目标，不把逗号列表任一项或HOME默认文件当成已证实来源。

#### Scenario: Consistent Byobu target
- **WHEN** Byobu修复选择有效自定义配置目录
- **THEN** 诊断与写入使用同一目标，不再显示另一个固定默认路径。
