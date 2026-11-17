# Verification

- 用户报告的会话含已结算的 7 条任务。隔离会话副本的原始对照见[直属子任务终态验证](../2026-11-17-restore-paged-subagent-terminals/verification.md)。
- `settled_child_is_available_to_exact_queries_after_cold_load` 通过真实 JSONL 发布与冷载，从索引读取只含 Finished 的页，核对同截点的准确归属、已确认终态，以及没有 child Result 的情形。
- `page_ownership_survives_omitted_load_metadata_and_resolves_nested_ancestors` 用超过 1 MiB 的启动 prompt 使 load metadata 省略整份可选归属，页选择仍能取得直接父关系、祖先和孙任务的前置失败终态。
- Pager 回归覆盖页外终态、已有空子视图的前置失败、直接父的本地历史缺失时的嵌套终态、两页恢复按相反顺序完成、错误归属与重连绑定。合成状态恢复不增加正文行，也不保留已结算任务的取消按钮。
- Rust 1.93.1 最终定向验证：Shell descendant recovery 11 项通过；Pager descendant recovery 14 项通过、1 项原手动基准忽略。分页乱序用例经 `HistoryPageLoaded` 实际采用 Latest 与 Before 页后，以相反顺序交付两个读取结果。
