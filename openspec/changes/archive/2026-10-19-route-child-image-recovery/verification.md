# 验证

## 静态检查

- `rustfmt --edition 2024`：已对本 change 修改的 Rust 文件执行。
- `git diff --check`：已通过。
- `openspec validate --all --strict --no-interactive`：已通过（23/23）；修改 fixture 后针对 child-image 与 compaction changes 的 strict validation 也通过。

## 回归测试

新增或扩展的回归包括：

- `child_image_recovery_full_notification_path_replays_one_terminal_row`
- `child_turn_completed_settles_only_its_matching_image_recovery_prompt`
- `finished_child_settles_unfinished_image_recovery`
- `replay_inherited_updates_preserves_child_image_lifecycle_order`
- `compaction_completion_preserves_root_and_child_todos_until_explicit_plan_update`

主 agent 首次运行时分别报告 Pager ACP 用例 351 passed / 1 failed / 1 ignored，以及 child 用例 45 passed / 1 failed。失败来自测试 fixture：child TODO 未初始化（child Plan notification 当前不写入 `AgentView.todo`），history fixture 未创建 `timeline.jsonl`，导致 reconciled reader 无法读取事件。已修复两个 fixture，并将 replay 改为单 reader 保序投影。修复后统一测试二进制实跑：Pager ACP 352 passed / 0 failed / 1 ignored；child 模块 46 passed / 0 failed。上述新增回归均包含在通过的两个模块中。

## OpenSpec 归档核对

2026-10-19 已通过 `openspec archive <change> --yes` 归档；全部 tasks 完成。归档前全量严格校验 23/23；归档后 `openspec validate --all --strict --no-interactive` 16/16，`openspec validate --archived --strict --no-interactive` 634/634。F2 先于 F3 归档，核对主规范同时保留 cursor merge 与 child lifecycle/实际历史顺序场景。开发说明和 backlog 已同步，无 MCP 实现修改。
