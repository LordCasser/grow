## Why
Pager 会话保存 restore_degree，但注释明确称尚无渲染消费者。需核对它是否参与实际恢复或结果反馈，避免把内部闲置投影误当成整个恢复功能无用。

## What Changes
记录生产数据流，将仅用于未来显示的会话缓存列为 R12 待确认删除。无代码行为变化，skip_specs=true。
