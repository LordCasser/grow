## MODIFIED Requirements

### Requirement: Scoped memory storage

默认 MemoryStorage SHALL 将全局记忆和工作区记忆置于 memory root，并由工作区标识隔离项目目录。git `origin` 可识别为网络远端时，工作区标识 SHALL 是 host-qualified canonical identity：小写 host、仅含与 scheme 默认不同的 port、规范 repo path（大小写保留、尾部斜杠不敏感、仅去除一个末尾 `.git`），不含 transport scheme 或凭据。同一 host 与 path 的常规 SCP、`ssh://` 与 `http(s)://` 表示 SHALL 归同一 identity；不同 host 或不同非默认 port SHALL 隔离。默认 port（http 80、https 443、ssh 22、git 9418）SHALL 与无 port 等价。origin 无法安全识别（无 scheme 且非 SCP 形式、非接受的 scheme、空 host、带 query/fragment、path 无 `org/repo` 形状）SHALL 不产生 repo identity 并沿用 cwd canonical path 标识；本地 `file://` origin SHALL NOT 并入网络 identity。host-qualified identity 对应的新 workspace 目录 SHALL NOT 迁移、并入或删除既有 hostless 目录。

#### Scenario: 初始化存储

- **WHEN** 通过 MemoryStorage::new 创建工作区记忆
- **THEN** global_dir 使用 override 或 grow_home/memory，workspace_dir 追加 compute_workspace_hash；构造时不创建目录。new_flat 使用已隔离的 root。

证据：`crates/codegen/memory/src/storage.rs` — `new_inner`。

#### Scenario: 同仓库跨 transport 表示归一

- **WHEN** 同一仓库的 origin 分别为 `git@host:org/repo.git`、`ssh://git@host/org/repo`（含显式默认 port `:22`）与 `https://host/org/repo.git`
- **THEN** 三者得到同一 identity， clones 与 linked worktree 选择同一 workspace 目录；repo path 除末尾单个 `.git` 外大小写保留。

证据：`crates/codegen/memory/src/storage.rs` — `normalize_remote_url`、`compute_workspace_hash`。

#### Scenario: 不同 host 或非默认 port 隔离

- **WHEN** 两个 origin 的 org/repo 相同但 host 不同，或同 host 而其一携带非默认 port（如 `:8443`、`:2222`）
- **THEN** 得到不同 identity 与不同 workspace 目录，不共享 MEMORY.md、sessions 或索引。

证据：`crates/codegen/memory/src/storage.rs` — `normalize_remote_url`。

#### Scenario: 不可识别 origin 降级且不采纳旧目录

- **WHEN** origin 为 `file://` 本地路径、携带 query 或 fragment，或 path 无 `org/repo` 形状
- **THEN** 不产生 repo identity，workspace 目录沿用 cwd canonical path fallback；memory root 下既有 hostless 目录的内容不被构造迁移、覆盖或清除。

证据：`crates/codegen/memory/src/storage.rs` — `normalize_remote_url`、`compute_workspace_hash`、`clear_workspace`。
