## ADDED Requirements

### Requirement: Validated UTF8 path wrappers
AbsPathBuf 和 RelPathBuf SHALL 分别校验 absolute/relative 形态及 UTF-8；RelPathBuf 的字符串 serde 反序列化使用同一校验。

#### Scenario: 错误类别
- **WHEN** 以相对路径构造 AbsPathBuf，或以绝对路径构造 RelPathBuf
- **THEN** 分别返回 NotAbsolute 或 NotRelative；非 UTF-8 返回对应 NotUtf8。

#### Scenario: 父目录片段
- **WHEN** relative 输入包含 ..
- **THEN** 仅校验 relative 形态，不据此声称 filesystem containment 或阻止 symlink escape。

证据：`crates/codegen/paths/src/lib.rs` — `RelPathBuf`；`crates/codegen/paths/src/lib.rs` — `AbsPathBuf`。

### Requirement: Root conversion semantics
路径转换 SHALL 为 relative 路径拼接 root，对已有 absolute 路径保留原值；宽松相对化遇到 root 外路径原样返回，严格 RelPathBuf::from_absolute 则返回错误。

#### Scenario: root 外路径
- **WHEN** abs_path 不在 root 下
- **THEN** to_relative_path 不修改输入；from_absolute 返回 NotRelative。

#### Scenario: 恰好是 root
- **WHEN** abs_path 等于 root
- **THEN** 剥离后得到空 relative 路径。

证据：`crates/codegen/paths/src/lib.rs` — `to_relative_path`；`crates/codegen/paths/src/lib.rs` — `from_absolute`。

### Requirement: Lexical normalization boundary
normalize_lexically SHALL 在不访问文件系统的条件下处理 . 与 ..；AbsPathBuf::contains_path 只规范化候选路径，再与保存的 root 比较。

#### Scenario: 未规范化 root
- **WHEN** root 为 /a/b/..，候选为 /a/c
- **THEN** contains_path 返回 false，不隐式规范化 root。

#### Scenario: lexical 路径计算
- **WHEN** 相对路径 src/.. 或绝对路径 /../../tmp
- **THEN** 分别得到 . 与 /tmp；结果不证明与含 symlink 的原路径指向同一实体。

证据：`crates/codegen/paths/src/lib.rs` — `normalize_lexically`；`crates/codegen/paths/src/lib.rs` — `contains_path_does_not_normalize_the_stored_root`。
### Requirement: Pager child cwd and worktree flag independent derivation

Child path derivation SHALL use SubagentInfo.child_cwd when present and otherwise clone the parent cwd. Independently, is_worktree is true exactly when info exists and worktree_path is present. worktree_path does not supply cwd, child_cwd does not imply worktree, and the helper does not canonicalize, verify existence or compare either path.

#### Scenario: Child cwd
- **WHEN** child_cwd is present
- **THEN** its raw path becomes the effective cwd.

#### Scenario: Parent fallback
- **WHEN** info or child_cwd is absent
- **THEN** the parent cwd is cloned.

#### Scenario: Worktree
- **WHEN** worktree_path is present
- **THEN** the worktree flag is true independently of child_cwd.

#### Scenario: Cwd only
- **WHEN** child_cwd exists but worktree_path does not
- **THEN** the child path is used with false worktree flag.

证据：`crates/codegen/pager/src/app/acp_handler/background.rs` — `derive_child_cwd`。
