# Bound after-snapshot capture and preserve unknown rewind state

## Why

`FileStateTracker::end_prompt` 在最终答案后重新读取本轮触及的全部文件以记录 after 快照：8 路并发没有总期限，一个永不返回的 read 即可阻止收尾（backlog「Workspace after-snapshot阻塞prompt返回」）。同时 `.unwrap_or(None)` 把读取错误与 UTF-8 失败折叠成 `content=None`，把"未知"伪装成"确认不存在"。消费端 shell rewind 预览用 `rev().find_map(after_snapshots.get(path))` 取 after：跳过缺失条目的最新 point、落到更早 point 的旧 snapshot，且 missing 与当前 `None` 误判 clean。本轮在该行为的 owner（文件状态采集）定义 after 不可得语义，并让消费端如实呈现 unknown。

## What Changes

- Workspace：`end_prompt` 以固定 5 秒总预算约束整轮 after 捕获（deadline 覆盖排队与读取等待），8 路并发不变；预算内读取成功项保留；读错误 / UTF-8 失败 / 到期不插入任何条目（该路径最终状态=未知，绝不插入假 `None`）；确认不存在才记 `content=None`。同一 prompt 再次捕获先清旧该轮 after，避免重试残留；到期后不做后台晚写，也不声称底层读取被终止。提供 `#[cfg(test)]` 私有可控 duration 入口，不加用户配置、不加公共实体。
- Shell：rewind 预览改为"最新触及该 path 的 point（`file_snapshots` membership）+ 其自身 after 条目"三态判定；缺 after ⇒ `conflict_type "snapshot_unavailable"`，无论当前文件存在与否都不进 clean；已确认删除（after `content=None`）与当前不存在继续判 clean；preview 提示明确 unknown；`force=true` 保持既有用户明确放弃冲突语义，不加新审批。`merge_rewind_points_from` 折叠后轮时，后轮触及 path 而无 after 必须删除前轮同 path 旧 after，不得继续呈现可用旧状态。
- Schema 不变：unknown 即 after 条目缺失；不加持久字段 / version / fallback；metadata 扫描与 lazy 读取不变。

## Impact

workspace `session/file_state.rs`（捕获与 merge）、shell `session/actor/rewind.rs`（预览判定与提示）及两处 rewind 相关测试（workspace 内嵌 tests、shell `actor/tests/rewind_*` 注册小改）。`session/commands/handle/run_loop`、`turn/mod`、settlement、subagent、其他 workspace client fs、Cargo 依赖不动。对应 backlog 非延期项"Workspace after-snapshot阻塞prompt返回"；Persistence FlushAndAck 的未知写入所有权不在本项范围。