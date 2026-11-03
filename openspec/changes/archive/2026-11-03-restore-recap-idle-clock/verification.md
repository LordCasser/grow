# 验证

`RUST_MIN_STACK=33554432 cargo test --locked -p shell --lib recap_clock --offline -- --test-threads=1`：1 passed。真实 primary HTTP 产生 canonical Request 事实，Existing bootstrap 恢复非零历史时间；后来 Sideband 活动不推进时钟，Fresh/Seeded/没有 Request 的历史仍为0。

重跑完整图片恢复模块：54 passed，0 failed，包含原有恢复与取消、四辅助消费者、三 backend、Memory off、preflush/compaction、cold restore，以及新增时钟场景。

`cargo build --locked -p cli --bin grow --no-default-features --features sandbox-enforce --offline` 通过（final-4，42.77s）。真实 ACP 5 条旅程在该 binary 上复跑通过（21.27s）。

Full/Minimal fresh-process cold load 使用真实图片和三轮主请求，再发送原生 FocusLost、持续 drain PTY 181 秒、FocusGained。最终3项PTY测试全部通过（195.29s），两模式均验证图片拒绝、vision描述、文字重试、屏幕marker，以及canonical Sideband Request来源、Result和Completed终态。详见同轮 `verify-auxiliary-image-process-journeys` 的记录。
