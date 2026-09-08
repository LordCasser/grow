## Results
cargo test --locked --offline -p pager --lib transcript --quiet：38 passed，0 failed。无增量、debug=0、2 jobs、RUST_MIN_STACK=16777216；只有既有 macOS compact unwind 警告。

## Scenario evidence
- blocked_file_io_does_not_block_async_runtime：后台写入闭包阻塞等待释放时，current-thread async runtime 仍能恢复测试并观察任务未完成；释放后返回成功。闭包有两秒保护，回归不会永久卡住。
- queue_is_bounded_serial_and_ignores_stale_completion：一个 active、八个 pending，第十个拒绝；错误/重复 id 不释放当前任务，按 FIFO 继续。
- file_writes_are_deferred_ordered_and_ignore_rebound_session_feedback：dispatcher 返回 Effect 时无文件；Export 后接同路径 Copy，只返回一个运行 Effect，依序执行后最终为第二个快照；切换 session 后两个旧结果均不追加通知，但队列继续。
- failed_file_write_advances_queue_after_origin_is_removed：首个非法目录目标失败且原 agent 已删除，下一已接纳请求仍启动并写入正确内容。
- full_file_write_queue_rejects_new_request_without_dropping_accepted_jobs：真实 dispatcher 满载显示提示；九个已接纳任务均输出，拒绝项不写入；成功通知只在提交结果应用后出现。
- worker_panic_returns_failure：spawn_blocking 的 panic 映射为 Err TaskResult，不丢失终态。
- 之前 copy/export 路径、权限、加载窗口与消息选择回归保留，改由实际 execute 函数运行 Effect 后派发 TaskComplete，未删除原文件/内容断言。

## Review and limits
两个 dispatcher 的显式文件分支只构造快照并入队，没有直接 write/sync。文件策略复用原 helper。首次编译仅补齐 AppView 测试夹具字段；新增测试 remove 的弃用警告已改成 shift_remove。

正文渲染、剪贴板及默认备份仍同步；队列只限制请求数量，单个大文稿没有字节上限。程序退出不持久化待处理任务，已启动 OS 写入无法强制取消。未模拟真实慢磁盘或终端 UI，响应性由阻塞闭包测试证明后台 I/O 与 async runtime 分离。未重新链接 CLI。
