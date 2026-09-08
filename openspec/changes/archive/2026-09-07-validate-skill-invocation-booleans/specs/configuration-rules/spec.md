## ADDED Requirements

### Requirement: Skill invocation switches reject invalid boolean values
user-invocable 和 disable-model-invocation SHALL 接受 YAML 布尔及 true/false 字符串，其他显式值 SHALL 触发解析错误，不得默认为 false。

#### Scenario: Invalid invocation switch
- **WHEN** 任一调用开关为 yes、数字、null、列表或映射
- **THEN** 发现跳过该技能，不按默认调用能力加载。

#### Scenario: Valid switches and defaults
- **WHEN** 开关为合法布尔/字符串或未声明
- **THEN** 保留显式值，缺省 user-invocable 为 true、disable-model-invocation 为 false。
