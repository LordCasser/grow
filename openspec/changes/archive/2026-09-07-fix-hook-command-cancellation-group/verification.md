# Verification

- 旧实现真实子进程回归失败：grandchild survived: scoped=false, cancelled=true。
- 修复后 Unix 回归覆盖 scope 有/无 × 任务 abort/执行超时四种组合。后台脚本写 ready 后延迟写存活标记，ready 确认后取消或等待超时，随后标记均不存在。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p hooks --quiet`：215 单元、13 集成、1 doctest 全通过，0 失败/忽略。
- 正常完成撤销异常清理守卫，超时/IO错误显式 drop，future 取消隐式 drop。已有会话关闭组回收与关闭 scope 立即取消测试通过。
- 仅本机 Unix 运行了进程回归，未执行 Windows 测试。Windows 沿用已有 Job Object 的 close-kills 语义；无 scope 路径现在也建立 Job Object。进程组创建失败仍降级，逃逸到独立进程组的后代不由此保证清理。
