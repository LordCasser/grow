# Verification

## 场景与实现核对

- 复用已有 Timeline 折叠；Checkpoint 只提供等待保留资格，不创建新 attempt。ExecutionReleased 仅保留匹配 operation、调用和等待种类的证据。
- 三类交互 × 四种状态（未重绑、新 attempt 等待、已真实解决、已有真实 ToolResult）。每项均走首次 drain 超时、同 operation 重试、冷恢复、prepare/commit/release、新挂起与第二 checkpoint 冷读取。启动信号屏障模拟恢复 worker 尚未调度；不是操作系统进程 E2E。
- 未解决等待保留依赖身份，无伪造结果；新 attempt 被接管；已解决等待不复活；真实工具结果保持单份且排除旧等待。另从真实事件副本移除原 checkpoint 的等待资格，验证历史 Suspended 不能独自跨 release。
- 已有六种真实 resolution、不同 operation、通知回执、Question/Plan 重绑、树 freeze 身份与运行恢复测试纳入定向回归。现有 ToolResult 在分类前排除调用，Turn 已结束则不保留 cursor。

## 执行

使用 Rust/Cargo 1.93.1 和 `RUST_MIN_STACK=16777216`。联合验证命令与结果在完成记录中填写。未安装二进制、未触碰原故障 session；未运行全平台发布或真实 provider 矩阵。

## 完成记录

```sh
RUST_MIN_STACK=16777216 PATH="$HOME/.cargo/bin:$PATH" cargo test --locked -p shell --lib -- session::actor::suspension:: session::actor::tests::planning_dispatch_tests:: session::actor::tests::reverse_request_session_id_tests:: session::actor::tests::runtime_ agent::mvp_agent::tests::suspension_admission_tests:: --test-threads=1
```

**53 passed、0 failed、0 ignored**，日志 `/tmp/grow-resuspend-scope-regressions.log`。新增跨 release 矩阵为一个测试、内部 12 个组合；scope 为 5 个测试，不将组合数重复累计。归档前 strict OpenSpec：18 passed、0 failed。`git diff --check` 通过。

归档后当前规范 strict：16 passed、0 failed；archive strict：657 passed、0 failed。

联合收尾执行 `cargo clean -p shell -p tools`：移除 547 个文件、3.4 GiB 编译残留。
