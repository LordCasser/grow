# Verification

- 旧实现 stale_cleanup_does_not 回归：1 失败，旧清理取走新句柄。
- 修复后 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p mcp --lib --quiet`：156 通过、0 失败、0 忽略，5.02 秒。
- 新回归确定性构造与状态检查/替换交错相同的共享 slot 状态：旧 token 已取消，新 token 活跃；旧清理不得取走新句柄，新清理仍正常释放。未加入随机并发或延时竞争测试。
- 全组包含实际 poller、恢复与旧 transport 事件回归。生产 set_liveness_handle 在 slot 锁内替换并 drop 旧句柄，是取消复核成立的必要前提，已检查全部槽位修改入口。
- 原先调查的 SessionActor 握手配置交错没有在本次宣称完整修复；此次仅关闭 liveness 槽位误清理的具体问题。
