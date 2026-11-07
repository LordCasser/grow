# Tasks: qualify-memory-repository-identities

验证方法（实现前约定）：Tier 2。目录选择必须经真实公共入口——`git2::Repository::init` + 真实 `origin` remote（SCP/ssh/https 对照、linked worktree）+ `MemoryStorage::new(cwd, root_override=temp)`——断言同 host 跨 transport 与 worktree 同目录、不同 host/非默认 port 不同目录、不可识别 origin 沿 cwd fallback、旧 hostless sentinel 目录内容不被构造迁移/覆盖/清除；`normalize_remote_url` 单元覆盖 host 折叠、默认 port 等价、单次 `.git`、IPv6、凭据、query/fragment、本地 origin。本窗口禁 cargo（父共享构建窗口）：测试只写入并自查，执行留给父在可用窗口运行 `cargo test -p memory --lib --locked --offline storage::`；自文件 `rustfmt --edition 2024 --check`；`openspec validate qualify-memory-repository-identities --strict --no-interactive`；最终 diff 自查并出 `/tmp` 显式 pathspec patch。

- [x] 1. 建 proposal、delta（WHEN/THEN）、design、tasks；`openspec validate qualify-memory-repository-identities --strict --no-interactive` 通过。
- [x] 2. 实现 host-qualified `normalize_remote_url`（`Url::parse` + 最小 SCP 改写、scheme 白名单、默认 port 表、单次 `.git`、query/fragment 降级）；`memory/Cargo.toml` 加 `url = { workspace = true }`，Cargo.lock 仅补 memory 依赖列表一行。
- [x] 3. 单元测试：transport 归一、host/port 隔离与默认 port 等价、host 大小写折叠、path 大小写、`.git.git` 单次裁剪、IPv6、凭据排除、query/fragment/本地 origin 返回 None、绝对 SCP path 不误合并。
- [x] 4. Tier 2 测试：真实 git2 repo + origin + `MemoryStorage::new(root_override=temp)` 的目录对照（clones/worktree 同目录、跨 host/port 隔离、默认 port 等价、不可识别 origin fallback、旧 hostless 目录不被采纳/清除、`new_flat` 与无 origin 稳定）。
- [x] 5. 自文件 rustfmt、最终 diff 自查、`/tmp` 专属 patch（显式 pathspec 含本 change docs，排除继承 dirty）。测试执行依赖第 6 项。
- [x] 6. 实际执行 `cargo +1.93.1 test --locked --offline -p memory --lib -- --test-threads=1`：321 passed / 0 failed，父亲核原始日志与最终源码；SCP分隔符和新增git2夹具借用修正后通过，未放宽断言。见 verification.md。
