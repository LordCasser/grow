# paths 逐包核查

包路径：`crates/codegen/paths`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/codegen/paths/Cargo.toml`
- `crates/codegen/paths/src/lib.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Validated UTF8 path wrappers](../specs/workspace-paths/spec.md#requirement-validated-utf8-path-wrappers)：AbsPathBuf 和 RelPathBuf SHALL 分别校验 absolute/relative 形态及 UTF-8；RelPathBuf 的字符串 serde 反序列化使用同一校验。
- [Root conversion semantics](../specs/workspace-paths/spec.md#requirement-root-conversion-semantics)：路径转换 SHALL 为 relative 路径拼接 root，对已有 absolute 路径保留原值；宽松相对化遇到 root 外路径原样返回，严格 RelPathBuf::from_absolute 则返回错误。
- [Lexical normalization boundary](../specs/workspace-paths/spec.md#requirement-lexical-normalization-boundary)：normalize_lexically SHALL 在不访问文件系统的条件下处理 . 与 ..；AbsPathBuf::contains_path 只规范化候选路径，再与保存的 root 比较。

## 边界

- 分别返回 NotAbsolute 或 NotRelative；非 UTF-8 返回对应 NotUtf8。
- 仅校验 relative 形态，不据此声称 filesystem containment 或阻止 symlink escape。
- to_relative_path 不修改输入；from_absolute 返回 NotRelative。
- 剥离后得到空 relative 路径。
- contains_path 返回 false，不隐式规范化 root。
- 分别得到 . 与 /tmp；结果不证明与含 symlink 的原路径指向同一实体。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
