# 验证

`cargo check --locked -p shell --lib` 通过。`cargo test --locked -p shell --lib idle_memory_flush_tests -- --test-threads=1`：2 passed。

测试运行真实 run_session，使用 loopback HTTP provider。覆盖 foreground 与会话活动繁忙、活动完成后完整 idle interval、持续 GetCurrentModel 查询、一次 provider 失败后同历史重试、fork 初始化长度基线、真实 Rewind command 及分支 reset 后资格、零 timeout 不触发。compaction reset 消费由实际 run loop 验证；完整 enabled preflush + 真实 compaction + JSONL/Markdown 组合由同轮 `compact_flush_journey` 另外验证（1 passed）。

`openspec validate --all --strict --no-interactive`：25 passed，0 failed。限定改动的 `git diff --check` 通过。测试期间不终止其它会话的 Cargo，也不删除共享 target。
