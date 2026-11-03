# 验证

- 新的 `explicit_delete_abandons_independent_runtime_while_ttl_and_writer_guards_remain` 在旧实现因 unfinished runtime handoff 拒绝而失败；修复后通过。用 Suspending/Suspended/Restoring/Released 四种合法 canonical history，验证竞争 writer 拒绝且字节不变、释放后永久删除、重复删除幂等；自动 TTL 对未消费阶段保留。
- 最终同次 Shell binary、`RUST_MIN_STACK=16777216` 运行 `session::storage::jsonl::tests`：159 通过。原始 canonical 结果链单体拒绝、维护 checkpoint 保留、fork 不继承运行 ownership、import/rewind 相关存储边界均保持。
- 删除逻辑不追加任何 Runtime event；没有把尚未恢复的 checkpoint 改写成 ExecutionReleased。只调整显式、独立目标的资格，writer ownership、双向引用和 quarantine 代码沿用。
- 所有 storage fixture 使用临时 root；`git diff --check`、归档前 full strict 17 项通过。没有操作用户真实 session。

OpenSpec：归档前 17 通过，归档后 16 通过，archived strict 773 通过。
