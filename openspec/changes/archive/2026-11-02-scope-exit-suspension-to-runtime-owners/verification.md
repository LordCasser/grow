# 验证

- Pager `cargo test --locked -p pager --lib exit_ -- --test-threads=1`：44 通过，覆盖启动 pending、失败后的 r/f、退出 admission 屏障和原失败/强退 UI。`ctrl_x`：19 通过。`quit` 初次 37 通过、1 失败（旧空页面立即 Quit 预期），按新契约改为等待 runtime ACK；最终 38 通过（含当前前缀被 Quit 终止）。
- Shell `RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib exit_ -- --test-threads=1`：最终 14 通过。真实 ext_method 覆盖独立 scope、同 scope 重试复用、并发 close/writer drain、未确认 lifecycle、丢失 RPC waiter 后的新退出。测试日志曾被两个排队的命令共用，后一次输出的 14 项逐项完成与 exit code 已核对；未把此前 13 项当作最终结果。
- 同次 Shell binary `leader::server::tests`：138 通过，含真实双 socket 的 driver/viewer 分离、伪造 scope 覆盖、direct/wrapped ext wire、在途 new/load 注册屏障。`runtime_handoff::tests`：8 通过、1 ignored。
- `tree_freeze` 首次 1 通过、1 失败：原 60ms initializer fixture 在更早的 freeze 等待处耗尽 deadline；未放宽断言，隔离重跑 2 通过。原 tree checkpoint 主体仅提取到持有同一 lifecycle guard 的方法，没有重写其控制逻辑。
- `cargo build --locked -p cli --bin grow --no-default-features --features sandbox-enforce`：通过。沿用前轮已说明的本机 jemalloc 生成头环境问题，无 allocator 构建保留 sandbox。
- `PAGER_BINARY=/Users/lordcasser/workspace/projects/grow/target/debug/grow RUST_MIN_STACK=16777216 cargo test --locked -p pager --test pty_e2e_persistence ctrl_d_suspends_running_root_and_continue_resumes_without_replay -- --ignored --test-threads=1`：1 通过。真实键盘/PTY、运行中的命令、Ctrl+D 挂起退出、新进程继续，验证原 prompt 不重发。
- `git diff --check` 与归档前全量 strict：通过（17 项）。

没有操作用户真实 session；多客户端验证使用隔离 socket，PTY 使用既有 isolated ContentController/临时 GROW_HOME。跨客户端范围由真实 leader 转发覆盖，runtime drain/并发由公共 ext_method 覆盖；未将模拟 socket backend 描述为多客户端完整模型运行。

归档后全量 strict：16 通过；`openspec validate --archived --strict --no-interactive`：770 通过。
