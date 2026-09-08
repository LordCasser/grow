## Why
技能添加清理 ignore 和来源计数使用字符串前缀，会将 foo 与 foobar 当作包含关系，意外恢复相邻目录的技能并夸大新增数量。应与已有发现过滤器的路径组件语义保持一致。

## What Changes
技能添加仅清理同路径或祖先、后代的 ignore；来源与添加计数按路径组件判断。

## Capabilities
### Modified Capabilities
- configuration-rules: 技能路径边界。

## Impact
shell 技能扩展及相关测试；不改变 reset 或路径别名解析。
