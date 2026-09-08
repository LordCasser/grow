## 旧实现失败
config_watch_late_directory_and_recreation 使用临时 home/project 和实际 OS watcher。旧实现 0 passed / 1 failed，在启动无 .grow 后创建目录并写入 config.toml，等待 5 秒仍无项目事件。

## 实现
ConfigFileWatcher 用一个共享锁串行化注册集合及底层 debouncer；callback 持 Weak 不形成所有权环。目录事件先核对 parent 仍注册，再 unwatch 旧目录并按存在性重新挂载，随后输出 config.toml 项目事件。start/watch_path/unwatch_path API 不变，start_config_reload 两类消费者无需分别维护。
依据本地 notify-debouncer-mini 0.6.0 src/lib.rs：原始 handler 只向 debouncer channel 发送，用户 callback 由独立线程执行；Debouncer Drop 发 Shutdown 而不 join 自己。未改变其他 watcher 类型的线程模型。

## 最终测试
- targeted 目录生命周期：1 通过，包含首次创建、再次写入、删除、重建后两次写入、unwatch 后重建无事件。
- config::watcher：7 通过，0 失败/忽略（包含上述用例）。
- config::reloader：4 通过，0 失败/忽略。
命令使用 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib 对应过滤器。rustfmt 与 git diff --check 通过。已有 macOS linker unwind warning 不影响运行。

## 限制
实际 OS 回归在 macOS 执行，未声称验证其他平台通知实现。注册失败仍记录并保留人工刷新退路；不保证目录权限/配额故障后无事件也自动恢复。范围为注册 cwd，未扩展被显式关闭的动态项目监听。没有读取或修改真实用户配置。
