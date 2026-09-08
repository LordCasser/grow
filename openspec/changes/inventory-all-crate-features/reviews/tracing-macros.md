# tracing-macros 逐包核查

包路径：`crates/codegen/tracing-macros`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/codegen/tracing-macros/Cargo.toml`
- `crates/codegen/tracing-macros/src/lib.rs`
- `crates/codegen/tracing-macros/src/timed.rs`
- `crates/codegen/tracing-macros/src/timestamp.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Timing and timestamp macros](../specs/developer-support/spec.md#requirement-timing-and-timestamp-macros)：timed 宏 SHALL 保留代码块返回值，裸调用返回 value 与毫秒耗时；log 分支在指定或默认 debug 级别记录时间，sync/async try 分支保留 Result 并为 Err 附加错误字段。

## 边界

- 分别通过 tracing info/warn 输出 Unix 秒前缀；系统时间早于 epoch 时用 0，不直接占用 stdout。
- 宏展开依赖调用方可解析的 tracing 路径；crate 本身没有运行时 dependency。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
