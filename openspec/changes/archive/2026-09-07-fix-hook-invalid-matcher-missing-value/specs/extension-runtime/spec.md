## ADDED Requirements

### Requirement: Invalid hook matchers never execute without match values
恢复时因无效模式产生的 Never 匹配器 SHALL 在事件有无 match_value 时均不匹配；缺失字段的宽容规则 SHALL 不覆盖该状态。有效匹配器的原规则保持。

#### Scenario: 无效匹配器遇到无字段事件
- **WHEN** Hook 模式编译失败且事件未提供匹配值
- **THEN** 匹配判断返回 false，执行计划跳过该 Hook。
