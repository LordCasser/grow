# Why
HookSpec 的 compiled matcher 被 serde 跳过，HookRegistry 默认反序列化后可能将配置模式当成 match-all，只有特定 workspace adapter 手工补编译。append/dedup 也接受过期 matcher，重编译在配置变为 None 时又未清除旧缓存，配置事实与派生状态可不一致。

# What Changes
registry 的反序列化、append 和 dedup 入口统一按 configured_matcher 重建缓存，无效模式 Never，未配置清除。workspace adapter 不再承担额外修复步骤。

# Impact
序列化形状不变，匹配器编译责任内聚到持有其不变量的 registry；不改模式语法和来源优先级。
