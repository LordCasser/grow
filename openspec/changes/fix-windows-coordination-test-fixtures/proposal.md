## Why

新启用的 main push 协调 CI 在 Windows 暴露两个既有夹具问题：`/tmp` 不满足 Windows 绝对路径类型；模拟滞后 Summary 时绕过已持有的目录能力另开路径写者，触发共享冲突。本次新加的三个启动竞态测试均已通过，不能把这两个夹具失败归为该竞态未修复。

## What Changes

- SessionHandle 测试 helper 使用 `std::env::temp_dir()` 构造一致的 cwd、文件系统和工具上下文。
- 滞后 Summary 夹具通过原 writer 的目录能力原子写入，继续验证 observer 不修改事实及 replacement writer 只收束一次。
- 保留全部断言，在 Windows 原生 CI 验证；不放宽路径或文件共享的产品约束。
- 协调 CI 的 push/PR path filters 覆盖实际测试 helper、工具测试和 durable storage 依赖，确保此次夹具提交也自动验证。

## Capabilities

无契约变化；`skip_specs: true`，仅修测试夹具，不修改运行时代码、持久化格式或现行规范。

## Impact

修改 `agent/mvp_agent/tests.rs`、`session/storage/jsonl/tests.rs` 和协调 workflow 的测试依赖路径，验证记录保留失败日志与平台覆盖边界。
