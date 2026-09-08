## 旧等待逻辑
提取同样 await generation 后 send 的交付逻辑并由两个生产分支调用。三项测试：正常交付通过，接收端预关闭仍 poll、执行中关闭仍 Pending 两项失败。

## 修复
biased select 优先响应 oneshot Sender.closed；结束时丢弃拥有的 generation future。外层任务 activity 随任务返回释放；Sideband 自身的持久化清理 activity 仍由现有终态修复负责，不声称同步完成所有清理。

## 限制
只取消本地 future/流和等待，不保证远端已开始的模型计算停止；fatal teardown 下 Sideband 按原有 fail-stop 规则保留未结束账本。本轮未修改模型选择及超时数值。

## 最终验证
Shell recap 60 项、Sideband Drop 取消终态 1 项、extensions::suggest 168 项，全通过。命令使用 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib 对应过滤器。rustfmt 与 git diff --check 通过。已有 macOS linker unwind 警告不影响测试。两个 run_loop 分支均调用共享 helper，未改其他请求。
