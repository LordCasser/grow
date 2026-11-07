# Runtime tree provider acceptance

2026-11-07，ChatState provider已由coder `01a1b441-4494-7da3-a099-8182189c8d4e` 实施，父architect读实际command/handle/actor diff、完整新增fixture/三个用例与原始测试日志后接受。仅本provider已验收；Shell consumer、实际长历史超时复现、最终CLI双Ctrl+D/恢复和独立整体review仍未完成。

`runtime_tree_snapshot`在同一mailbox操作内从已验证Timeline先过滤Spawned/Ended再clone，与runtime生命周期一起返回；保原header/seq/payload/result_ref，未新增状态或cache。句柄send/reply失败明确Err。接口文档警示投影不是完整ledger，不可独立from_events。

实际命令：`PATH=/usr/bin:/bin:/usr/sbin:/sbin:/Users/lordcasser/.cargo/bin CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 /Users/lordcasser/.cargo/bin/cargo +1.93.1 test -p chat-state --lib runtime_tree_snapshot --locked --offline`。编译52.63秒，三个测试全部通过，0failed/0ignored/646filtered，测试体0.01秒，无warning。覆盖live/cold与完整ledger事实一致、非空runtime、晚到Ended、原快照不变、无关大正文排除、无child、mailbox关闭和reply丢失。该测试不是性能基线，不证明实际退出5秒成功。

最终四文件rustfmt check和局部diff check通过；coder执行OpenSpec all strict30/30。已有input_state/FIFO diff保留。无提交、归档或Shell改动。原始日志位于该子会话terminal `call_2b8b86a597cd41b0bec6bfde41dafa3f.log`，父已读其3/3输出。
