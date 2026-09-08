# Verification

- 旧实现运行 `cargo test --locked --offline -p mcp --lib watcher_does_not_retain --quiet`：1 失败，断言 watcher owns the removed client。
- 最终 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p mcp --lib --quiet`：160 通过、0 失败、0 忽略。
- 新回归在任务首次 poll 前丢弃最后外部强引用，通过 Weak::upgrade 验证客户端可销毁；推进时钟验证槽位清理且无事件。
- 既有非 Ready 静默退出测试保留外部 Arc，确保继续覆盖 Transient 分支，而非意外改测客户端已释放分支。
- 本次直接验证引用所有权，不声称执行了真实配置移除与子进程退出集成测试。运行中的检查仍可短暂持有 Arc；不承诺同步 join 或中断锁等待。
