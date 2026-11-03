## Context

2026-11-02 18:05:14、18:05:33（Asia/Shanghai）的恢复均在 `prepare_runtime_restore` 的 source hash 校验失败。项目 AGENTS.md 于会话运行中修改；会话仍保留启动时规则。当前契约要求严格匹配来源，因此拒绝本身符合已归档规则，不能通过删除 checkpoint 或宽泛跳过检查修复。

## Evidence and Decision

使用真实 MvpAgent/config、JSONL writer load 与 actor 重建，在隔离 session 副本中计算当前 source。当前 hash 为 `90beb27cc51d71e41c983855ffafae2e2350741de032211fe483b3fa11dc95c5`；仅把 projectRules 换为 Timeline event 7 / 3794 中原始 ProjectInstructions，即精确得到 checkpoint 原 hash `b11a10aca2fea8610be0297d4adf0e3a1acec2019e1a1441538178abafb76c47`。其他来源轴保持完全一致。

用户明确优先授权修复 session 文件，因此实施一次离线来源重新绑定。仅修改最新 operation `01a19bb4-c29b-7462-9d3f-14350df5ea08` 的 domain event 7060（physical record 9254）的 runtime_source_hash 及同条 checksum。不改早先已消费 checkpoint，不改历史 ProjectInstructions，不伪造 RestorePrepared/Resumed/ExecutionReleased。

## Atomicity and Recovery

同时持有目标 parent writer lease 与 Timeline append lock，确认无现存 writer。原 Timeline 全量备份至用户 Grow recovery 目录，并记录修复前后 SHA-256。临时文件 fsync 后原子替换、再 fsync session 目录。新旧文件长度相同，除该条记录外逐字节相同。需要回滚时须先确认无 writer，再恢复备份；不能在用户已经继续产生新历史后覆盖整个文件。

## Limits

修复放行的是来源校验；没有实际续跑原任务。历史模型上下文保留原规则，重新构建的 runtime 使用当前 AGENTS.md 来源。此为显式用户授权的数据修复，不新增生产绕过机制。今后挂起期间再次修改规则仍可能按现有契约被阻止，产品层规则恢复策略独立登记。
