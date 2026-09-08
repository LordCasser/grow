## MODIFIED Requirements

### Requirement: Hook registries own matcher reconstruction
HookRegistry SHALL 在反序列化及规格追加/去重接纳时从 configured_matcher 重建 matcher，不信任传入缓存。Ignored 事件保留配置原文但清除 matcher，不编译或筛选；Tested 事件合法模式编译，非法模式 Never，缺失模式清除缓存。wire 调用者不必另行重编译。

#### Scenario: 序列化往返
- **WHEN** 带配置模式的 registry 完成 serde 往返
- **THEN** 立即保持相同匹配范围，无须额外调用修复。

#### Scenario: 程序化缓存过期
- **WHEN** 传入 matcher 与 configured_matcher 不一致或模式已被清除
- **THEN** 接纳后仅按当前配置匹配，不保留旧筛选或扩大无效模式。

#### Scenario: 忽略模式的事件经过 registry
- **WHEN** Ignored 事件带有合法或非法配置模式并经接纳、恢复或刷新
- **THEN** 模式只保留用于显示，matcher 为 None，不因模式跳过该事件。

### Requirement: Invalid hook matchers never execute without match values
Tested 事件恢复时因无效模式产生的 Never 匹配器 SHALL 在事件有无 match_value 时均不匹配；缺失字段的宽容规则 SHALL 不覆盖该状态。有效匹配器的原规则保持。

#### Scenario: 无效匹配器遇到无字段事件
- **WHEN** Tested 事件的 Hook 模式编译失败且事件未提供匹配值
- **THEN** 匹配判断返回 false，执行计划跳过该 Hook。
