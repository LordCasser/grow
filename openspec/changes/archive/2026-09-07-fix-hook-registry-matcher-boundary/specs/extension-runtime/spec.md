## ADDED Requirements

### Requirement: Hook registries own matcher reconstruction
HookRegistry SHALL 在反序列化及规格追加/去重接纳时从 configured_matcher 重建 matcher，不信任传入缓存。合法模式编译，非法模式 Never，缺失模式清除缓存。wire 调用者不必另行重编译。

#### Scenario: 序列化往返
- **WHEN** 带配置模式的 registry 完成 serde 往返
- **THEN** 立即保持相同匹配范围，无须额外调用修复。

#### Scenario: 程序化缓存过期
- **WHEN** 传入 matcher 与 configured_matcher 不一致或模式已被清除
- **THEN** 接纳后仅按当前配置匹配，不保留旧筛选或扩大无效模式。
