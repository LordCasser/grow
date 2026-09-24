## 验证记录

- `cargo test --offline -p pager --lib reopening_after_clear_starts_with_fresh_interaction_state -- --nocapture`：通过，1 passed。覆盖关闭后同一 query 重开、旧快照 generation 高于本地 floor、旧请求 ID 被拒及当前请求结果接纳。
- `git diff --check`：通过。
- `openspec validate close-pager-file-search-state-audit --strict --no-interactive`：通过。
- Cargo 链接器报告 `__eh_frame section too large` compact-unwind 警告；构建与测试成功。

代码证据：`FuzzyFileMatcherDaemon::set_query` 同步分配 query ID，worker 在结果快照中返回 query 文本和 ID；`FileSearchState::start_query` 清空旧结果并保存新 ID，`apply_results` 同时核对 query 与 ID。`clear_context` 后 reopen 的 `start_query` 也重置 selection、hover 和 scroll。
