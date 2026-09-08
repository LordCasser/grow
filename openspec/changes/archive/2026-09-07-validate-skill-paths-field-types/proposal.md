## Why
paths 类型错误当前返回 None，混合列表丢弃非字符串元素。限制字段错误因此可能变成无条件技能，应拒绝整份损坏声明而非静默丢字段。

## What Changes
paths 仅接受字符串、纯字符串列表或可选空值；错误类型返回解析错误并跳过技能。

## Capabilities
### Modified Capabilities
- configuration-rules: paths 类型校验。

## Impact
tools 技能元数据解析；分隔、花括号和已有空/匹配全部语义不变。
