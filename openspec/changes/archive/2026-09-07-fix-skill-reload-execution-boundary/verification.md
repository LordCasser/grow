## Implementation
reload_skills 持有 cwd 和 PluginRegistry 克隆，在 blocking worker 内用当前 runtime Handle 执行配置加载和发现。run_skill_reload 的 deadline 包含 acquire_owned 与 worker 等待；扩展共享一个 Semaphore，OwnedSemaphorePermit 移入 worker 保留至退出。

add/remove/reset 三个保存后入口将重载错误改为明确的“configuration was saved”错误。list/config/toggle 的重载错误直接传播。正常空列表不被当作错误。

## Validation
低磁盘配置运行 `cargo test --locked --offline -p shell --lib extensions::skills::tests --quiet -- --skip test_resolve_tilde_path`：20 passed。新增三个测试：
- oneshot 确认 worker 已开始，阻塞至外部释放；超时后 permit 为 0，后续请求超时且 scan 不执行，真实退出后 permit 恢复。
- 空 Vec 成功与 worker panic 错误区分，panic 后 permit 恢复。
- 排队请求取消后释放 permit，不执行已取消的扫描。
保留此前 17 项回归，跳过旧 HOME 修改测试；存在既有 macOS linker unwind 警告。

## Evidence limits
测试使用受控任务验证生产 worker 调度函数，未制造真实 OS 文件系统卡死或执行用户配置写盘。保存后错误措辞与操作顺序经源码核对。timeout 不杀死 blocking worker；永久挂起扫描会保留唯一许可，后续重载可超时失败。运行时关闭等待 blocking task 的边界未在本轮解决。配置保存、来源展示和其他发现调用仍不属于本轮 5 秒 deadline。

CLI 尚未重新链接。磁盘可用约 69 GiB，target 9.8 GiB。
