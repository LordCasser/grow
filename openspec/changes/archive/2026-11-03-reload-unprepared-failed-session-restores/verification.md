## 实施
失败load只退休本次创建且未取得RestorePrepared的Suspended root候选。ACK后立刻移出resident，但保留thread/writer和退休原因直到回收；取消/ACK未知后显式Retry先处理该owner。不存在自动load重试或新checkpoint。原viewer、Restoring和已运行owner不退休，逻辑child不Teardown。

## 运行证据
全部Cargo使用隔离临时GROW_HOME和RUST_MIN_STACK=16777216，不访问用户真实历史。

- 最终 `cargo test --locked -p shell --lib restore_retry_tests -- --test-threads=1`：3 passed。真实JSONL record/checksum、公共load连续失败两次，确认临时actor/thread退出后修复隔离fixture，原MvpAgent第三次load成功，原operation保留、prepare/release各一次，live reconnect同channel。另核对Running/Restoring不发退休命令。取消用例覆盖ACK前/后及延迟thread clean/panic四组，确认thread一直有owner、无coordinator Teardown消息、原checkpoint仍Suspended。
- `suspended_child_cold_restore_retains_birth_authority_end_to_end`：1 passed（四进程阶段）；missing/invalid birth仍阻断、无provider采样。负向断言改为核对root已退休和durable Suspended/no Ended，避免旧conditional resident断言失效。
- `suspension_admission_tests`：6 passed；`sweep`：1 passed；最终 `supervisor`：2 passed；`disconnect`：23 passed。
- 初次 `restore_retry_tests` 2项绿后，独立审查补出取消/drain问题；不把最初2项绿当作覆盖这些边界。修正后的3项为上述最终结果。
- 静态独立审查及复审见review.md。

归档前 strict 全量 18/18、归档后全量 17/17、archive 782/782 通过；`git diff --check` 通过。
