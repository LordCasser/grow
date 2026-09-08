# 验证记录

本次只新增 `failed_goal_settlement_retains_attempt_and_retries_exactly_once` 测试；GoalSupport 中此前 C005 的运行时代码修改不属于本 change。基线迁移已完成后，重新读取了主规范 behavior-goal 和 development-workflow，未发现本次测试需要改变契约。

已完成的 Rust 验证：

```sh
CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline \
  -p shell --lib --quiet session::actor::goal_support::tests -- --test-threads=2
```

运行句柄 `96240` 已以 exit 0 完成：**17 passed / 0 failed**。该次构建重新编译了依赖，因此等待时间较长；全程接续原句柄，没有重复启动。新增测试覆盖已知与未知用量在提交失败时回滚且保留 attempt，恢复后正确提交，重复结算不追加 Timeline 事实。没有发现本次覆盖路径需要新增运行时修复。

`git diff --check` 已通过。OpenSpec 全量严格校验 15 项通过（本 change 与 14 项主规范）。该校验只证明文档结构，不证明 Rust 行为。

与文档任务协调：用户明确本任务留在 main，OpenSpec 基线治理任务在独立分支处理；本 change 及新增 GoalSupport 测试属于功能审计，已向该任务明确归属。

归档完成：`openspec archive audit-goal-settlement-retry --skip-specs --yes` 成功。归档后全量严格校验 15 项通过（14 项主规范与文档任务的 verify-openspec-baseline），archive 校验 2 项通过。本任务留在 main；文档任务已确认使用 `/Users/lordcasser/workspace/projects/grow-openspec-sdd` 的 `codex/openspec-sdd`，仅复制自己负责的文件，并保留共同目录副本。

范围限制：注入的故障是 ChatState 提交不可用，不是部分磁盘写入、fsync 失败或真实崩溃。测试预期检查：已知/未知用量提交失败回滚，attempt 保留，恢复提交后的 Timeline 值正确，重复结算不新增事实。
