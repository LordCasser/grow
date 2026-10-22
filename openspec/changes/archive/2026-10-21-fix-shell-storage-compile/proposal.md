## Why
用户要求消除上轮 CLI 检查中的四个 Shell 编译错误，同时保留工作区其他未提交改动。
## What Changes
修正模块拆分后的内部可见性、多余字段初始化，以及用户消息文本提取的类型错误。HistorySnapshot 的 reseed_event_counter 已存在于当前工作区，不覆盖该实现。
本次只修复编译接线，不改变行为契约、持久格式或存储迁移设计；skip_specs=true，不虚构 delta。
## Impact
限定 persistence.rs、jsonl/ledger.rs 和 history/projector.rs 的必要行，独立于仍进行中的 unify-timeline-history-storage。
