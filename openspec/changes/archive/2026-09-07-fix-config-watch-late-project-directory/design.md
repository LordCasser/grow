## Evidence
- config/watcher.rs::watch_cwd_dirs 明确记录 .grow 不存在后无法自动补挂的已知限制。
- callback 只识别 config.toml，目录事件被忽略；watch_path 对 watched_cwds 直接返回。
- config/reloader.rs::start_config_reload 为 leader 与 in-process pager 共用入口，reloader 运行于 tokio::spawn，watcher 返回调用方持有。
- agent/app.rs 动态路径注册在 LocalSet 的 Rc<RefCell> 中，不能直接把该对象跨线程传给 reloader。

## Design constraints
监听注册责任留在 watcher/runtime 层；目录维护必须先于内容去重，不能依赖 ConfigUpdate::McpCatalogChanged（相同文件内容可能被去重）。维持非递归 cwd 与 .grow 范围，不递归整个仓库。不新增轮询扫描或把 UI 作为唯一修复入口。
需要明确线程安全的 watcher 维护句柄及 teardown 所有权；实现前验证 notify/debouncer 回调是否持锁，避免在回调内部反入 watch 导致死锁。当前尚未选择最终维护接口。

## Validation plan
真实临时目录 OS watcher：启动无 .grow、创建后写配置应通知，再次修改也应通知；删除/重建后继续通知。测试不用真实 GROW_HOME。还要验证 unwatch 后不重新挂载及重复注册不放大范围。

## Selected ownership
ConfigFileWatcher 持有 Arc<parking_lot::Mutex<ConfigWatchState>>，state 保存 debouncer 与注册 cwd；debouncer callback 仅持 Weak，避免循环引用。回调维护和 watch/unwatch 共用同一锁。notify-debouncer-mini 0.6.0 源码显示 callback 在独立 debounce 线程，底层通知只 channel.send；Debouncer Drop 只发 Shutdown，不 join callback。因此回调取得状态锁后修改底层注册不会等待自己。
.grow 目录事件先在锁内检查 parent 注册资格，再解除旧目录watch并按当前存在性重挂，输出该目录 config.toml 的重读事件；发生在内容去重之前。所有权 API 保持不变，leader/pager 共用。
