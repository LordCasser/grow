# 验证

真实 fork dispatch 分配 placeholder，再经 TaskComplete 注入普通/Worktree 初始失败：旧实现新增 3 测试全部失败，修复后 4 通过。覆盖空页返回来源、Dashboard attach 恢复、后台不抢焦点、已绑定替换不被改写、directive 与后续 composer 同时保留且没有自动执行。

隔离 GROW_HOME、RUST_MIN_STACK=16777216：fork 整组 53、本地无 ID Dashboard 退休 1、session modal 组 17 通过（以最终日志为准）。Dashboard 原测试 fixture 还保留 startup prompt-history loading，第一次回归拒绝退休；明确构造 settled 状态后通过，生产保护不放宽。没有删除任何实际副本/用户会话。

命令：cargo test --locked -p pager --lib failed_fork_ -- --test-threads=1；其余直接运行同次最终 lib test binary。日志 /tmp/grow-fork-placeholder-green-final.txt 与 /tmp/grow-fork-placeholder-regressions-final2.txt。

OpenSpec 全量 strict 归档前 17、归档后 16 通过；含归档 775 通过。
