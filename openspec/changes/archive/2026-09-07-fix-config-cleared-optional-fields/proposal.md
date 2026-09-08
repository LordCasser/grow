## Why
设置函数明确用 None 表示恢复默认/清空，但 merge_section 只覆盖序列化后存在的键；skip_serializing_if 导致 None 被忽略，原磁盘值因此保留。取消子任务策略 ask、空 screen_mode、清空默认模型均受影响。

## What Changes
读改写比较修改前后的已知字段序列化，仅删除原来存在而明确清空的字段，然后使用现有合并保存。未知字段和未修改值保留，支持嵌套已知可选字段。

## Capabilities
### Modified Capabilities
- configuration-rules: 可选设置清空真正持久化。

## Impact
仅 update_config 的变更语义；显式整份 save_config 不推断清空意图。保持共享锁和原始读取，不新增字段名单框架。
