## 1. 回归与实现

- [x] 1.1 在 sampler owner 边界增加阻塞 evidence ACK 与阻塞 usage ACK 的回归；先证明当前正常 shutdown 会提前完成或丢失 sink，再以目标行为断言 shutdown 在释放 ACK 前保持等待、释放后恰好一次完成。
- [x] 1.2 将 `SamplerActor` 正常关闭改为停止准入、取消 active token 并协作 join 全部 request task；运行 1.1 回归及现有 `bounded_shutdown_*` 测试，确认只有外层 deadline 可以触发 forced abort。
- [x] 1.3 运行 shell 的 `shutdown_sampler_joins_drainer_and_breaks_session_cycle`，确认 sampler event channel 仍在 request task 结束后关闭，drainer 顺序和 Session 引用释放不回归。

## 2. 文档与验证

- [x] 2.1 更新 `docs/development.md` 的采样恢复说明，明确 graceful shutdown 等待 attempt settlement、forced deadline 返回失败；校对与 `model-sampling` delta 一致。
- [x] 2.2 创建 `verification.md`，记录红绿回归、定向测试、未覆盖边界和实际命令；运行受影响 Rust 格式检查、`git diff --check` 及 `openspec validate --all --strict --no-interactive`。
- [x] 2.3 验证结束后检查文件系统与 `target/` 占用并执行 `cargo clean`，在 `verification.md` 记录清理前后大小和剩余空间。
