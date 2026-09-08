# version 逐包核查

包路径：`crates/codegen/version`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/codegen/version/Cargo.toml`
- `crates/codegen/version/build.rs`
- `crates/codegen/version/src/lib.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Installed version and display](../specs/application-maintenance/spec.md#requirement-installed-version-and-display)：版本层 SHALL 以编译期 GROW_VERSION 优先于 package version，installed() 允许运行期 GROW_TEST_VERSION 覆盖并去除两端空白；展示函数分别追加渠道后缀。

## 边界

- build.rs 触发重新构建版本信息。
- installed_semver 返回解析错误；display_version 使用编译版本而非测试覆盖值。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
