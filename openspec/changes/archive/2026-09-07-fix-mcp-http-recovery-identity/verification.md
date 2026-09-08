# Verification

- 新回归 superseded_http_recovery_does_not_retry_replacement 覆盖第 1 次与第 3 次恢复返回 Superseded；同名服务器仍保持 configured，验证旧循环停止而不是依靠下轮配置探测退出，且没有状态推送或工具注销。
- 既有 HTTP 真实失败后成功、耗尽、取消、去重及 dispatcher/e2e 回归用于验证保留行为。
- 生产路径核对：recover 的成功与错误均保存至最后身份检查后；禁用列表同步读取后取得一次状态锁，Arc 身份和 HTTP/SSE 配置检查至返回无 await。缺失或已改传输类型也返回 Superseded。
- 本次新回归针对恢复循环，未构造真实 HTTP actor 在握手完成与状态锁等待间替换配置的集成测试；最终锁边界由源码核对和编译验证。磁盘读取不是持久配置事务，未宣称封闭任意外部磁盘修改竞态。
- 首次编译发现 recover 返回服务句柄与 Result<(), RecoveryError> 不匹配，已显式 map 成 ()。

最终 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib mcp_ --quiet`：127 通过、0 失败、0 忽略，仅已有 __eh_frame 链接警告。
