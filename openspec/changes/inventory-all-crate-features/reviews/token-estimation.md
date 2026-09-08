# token-estimation 逐包核查

包路径：`crates/codegen/token-estimation`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/codegen/token-estimation/Cargo.toml`
- `crates/codegen/token-estimation/src/lib.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Shared approximate token arithmetic](../specs/model-sampling/spec.md#requirement-shared-approximate-token-arithmetic)：Token 估算 SHALL 使用 UTF-8 字节数整除 4，而非 Unicode 字符数；反向预算采用饱和乘 4，每张图片采用 765 的近似 token 成本。
- [Context percentage display arithmetic](../specs/model-sampling/spec.md#requirement-context-percentage-display-arithmetic)：上下文占比 SHALL 在 total 为零时返回零、最高为 100%；分别提供浮点占比、四舍五入整数和截断整数，剩余 token 使用饱和减法。
- [Shared threshold boundaries](../specs/context-compaction/spec.md#requirement-shared-threshold-boundaries)：压缩阈值辅助函数 SHALL 使用饱和整数运算和大于等于比较；context_window 为零时返回 false，headroom 在缩放阈值中扣除并饱和到零。

## 边界

- estimate_tokens 返回 0，不把估算当 Provider 实际用量。
- 结果饱和到 u64 上限。
- rounded 返回 43，truncated 返回 42；used 超过 total 时 free_tokens 为 0。
- used=850 触发，849 不触发。
- 阈值饱和到零，used=0 也触发；零窗口仍不触发。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
