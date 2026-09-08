# Why
无效 matcher 在恢复时被编译成 Never，意图禁止执行。但 matcher_allows 在事件没有 match_value 时直接返回 true，绕过 Never，使配置错误的 Hook 仍能运行。

# What Changes
Never 优先于缺失值容忍规则，所有事件均返回不匹配；有效 matcher 和未配置 matcher 的无字段行为不变。

# Impact
只修复恢复后的无效匹配器执行边界，不改变正常事件匹配语义。
