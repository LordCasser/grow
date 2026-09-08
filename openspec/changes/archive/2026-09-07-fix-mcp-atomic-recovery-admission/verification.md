# Verification

- 旧实现 concurrent_recoveries_admit 回归：1 失败，revision 从 1 变 3，预期只重置一次到 2。
- 修复后 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p mcp --lib --quiet`：157 通过、0 失败、0 忽略，5.02 秒。
- 新测试通过真实 in-process ACP 握手进入 Ready，显式持锁/poll 排队两个 recover；验证一次 reset revision、两者共享相同服务 Arc，initialize 总计两次（初始+恢复）。没有网络或进程依赖，不以随机压力测试代替确定性交错。
- 此修复针对同时竞争 Ready 的 recover；较晚抵达并看到已经完成恢复的 Ready，以及工具超时的强制 reset 仍需独立核对是否绑定原调用服务身份，未混入本次修改。
