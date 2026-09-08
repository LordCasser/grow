# test-utils 逐包核查

包路径：`crates/common/test-utils`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/common/test-utils/Cargo.toml`
- `crates/common/test-utils/src/env.rs`
- `crates/common/test-utils/src/git.rs`
- `crates/common/test-utils/src/image.rs`
- `crates/common/test-utils/src/lib.rs`
- `crates/common/test-utils/src/runfiles_util.rs`
- `crates/common/test-utils/src/tracing_capture.rs`

Cargo feature：`{"default-bazel": ["bazel"], "bazel": []}`。

## 功能与规范映射

- [Hermetic git test execution](../specs/developer-support/spec.md#requirement-hermetic-git-test-execution)：测试 Git 辅助 SHALL 可通过 GIT_BIN_PATH 一次性前置 binary 目录；run_git_with_env 固定作者身份、屏蔽 global/system 配置、禁止交互并断言命令成功。
- [Runfiles and test fixtures](../specs/developer-support/spec.md#requirement-runfiles-and-test-fixtures)：启用 bazel feature 时测试资源 SHALL 按 RUNFILES_DIR/TEST_SRCDIR 和 manifest 查找；crate_root 宏找不到时回退调用方 CARGO_MANIFEST_DIR。
- [Scoped tracing counters](../specs/developer-support/spec.md#requirement-scoped-tracing-counters)：测试日志计数器 SHALL 根据 message 前缀累计事件，clone 共享计数；提供 thread scoped 与 process global 安装。

## 边界

- envs 最后应用，可覆盖辅助函数默认值；返回 trim 后 stdout。
- 执行 init/config 或 add/commit；这些便捷入口只 unwrap 进程启动结果，不额外断言 exit status。
- 返回 None；default-bazel feature 启用 bazel。
- 分别提供解析失败默认值、单帧 ICO 包装、按目录分组的约量文件树、可供 rebase 的 feature/base 提交布局。
- panic，不把错误测试配置当成零计数。
- thread scoped 只观察当前线程；global 已存在 subscriber 时 panic，可选将过滤日志同时输出到 stderr。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
