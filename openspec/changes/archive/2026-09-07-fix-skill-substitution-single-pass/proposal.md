## Why
技能替换多轮改写正文，前一轮插入的用户参数或目录值会被下一轮重新解释，导致参数中的占位符文本失真。显式大索引还会落入 $ARGUMENTS 部分匹配。

## What Changes
仅扫描原始模板的占位符，替换值按字面插入；显式 $ARGUMENTS[N] 的缺失索引变为空。保留现有简写索引识别范围与无参数 token 时追加参数的行为。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能模板单次替换。

## Impact
apply_substitutions 与真实调用者共享的工具层实现，不涉及 shell 执行或引号解析。
