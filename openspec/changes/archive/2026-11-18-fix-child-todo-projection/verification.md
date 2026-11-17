# Verification

- Rust 1.93.1 的 Pager `compaction_completion_preserves_root_and_child_todos_until_explicit_plan_update` 通过。实际 ACP 子路由更新该 child 的 TODO，root TODO 保持原值。
- Pager `replay_inherited_updates_preserves_child_image_lifecycle_order` 的历史夹具包含 child Plan。首次打开恢复已保存的 TODO，更新的 live 空 Plan 保持清空，随后到达的旧历史不重新填充 TODO。同一夹具核对了子历史的正文顺序。
- 同一 session identity 重连会清掉旧连接的 live 优先标记。`reconnect_refreshes_plan_of_already_opened_child` 验证正常后代读取工作将断线期间已提交的 Plan 应用于已打开子会话，并保持重连窗口收到的 live 空 Plan。Shell 的 `indexed_child_replay_keeps_exact_nested_ownership_and_lifecycle_ordered` 验证已校验的子 Timeline 提供最新 Planning 快照。
