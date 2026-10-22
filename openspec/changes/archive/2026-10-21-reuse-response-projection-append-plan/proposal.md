# Reuse response projection append plan

## Why

冷恢复在 projection planner 已识别现存记录后，仍复制完整 updates、构造完整 reconciled 输出，并逐条扫描全量 existing 来判断是否需要 append。该路径对大量已有 projection 产生重复工作。

## What Changes

在恢复写入路径复用 planner 的 canonical append 决策，避免对完整 existing 重复查重，同时保留当前 exact conflict、anchor、candidate suppression、cursor 和 rewind 行为。该变更只优化内部执行成本，不改变已归档行为契约。

## Out of scope

不重构 replay、Timeline branch 分析或其他 projection 消费路径；不改变持久格式和外部行为。
