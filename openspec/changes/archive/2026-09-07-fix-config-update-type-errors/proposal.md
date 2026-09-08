## Why
设置读改写使用容错 Config 解析：某字段类型错误会让整段配置变成默认值，随后保存可能覆盖或删除原配置。语法有效并不意味着可以安全编辑 typed 配置。

## What Changes
编辑路径严格解析实际会写回的 cli/models/ui/skills/session 与 toolset.ask_user_question；失败时返回段名和类型错误，不执行修改闭包。运行时容错加载不变。

## Capabilities
### Modified Capabilities
- configuration-rules: 设置编辑拒绝配置段类型错误。

## Impact
Shell 设置编辑解析与测试。未知字段仍由现有合并保留，不校验不参与写回的远端/权限等配置。
