# Qualify memory repository identities with host and port

## Why

WS-04（`openspec/changes/archive/2026-11-05-audit-grow-architecture-across-versions/reviews/workspace-storage.md`）：`normalize_remote_url` 把 origin 归一为裸 `org/repo`，host 与 port 在进入 `compute_workspace_hash` 前被丢弃，`https://github.com/acme/widgets.git` 与 `https://gitlab.internal/acme/widgets.git` 必然共享同一 workspace memory 目录（MEMORY.md、sessions、index.sqlite），与 memory-search「Scoped memory storage」按工作区标识隔离项目目录的要求冲突。同一处 `trim_end_matches(".git")` 还会反复裁后缀，`repo.git.git` 被折叠成 `repo`。

## What Changes

- `crates/codegen/memory/src/storage.rs` 的 `normalize_remote_url` 改产 host-qualified canonical identity：小写 host（domain 与 IP 字面量统一）+ 仅与 scheme 默认不同的 port + 规范 repo path（大小写保留、尾部斜杠不敏感、仅去一个末尾 `.git`），不含 transport scheme 与凭据。同 host/path 的 `git@host:org/repo.git`、`ssh://git@host/org/repo`（含显式 `:22`）与 `https://host/org/repo.git` 归同 identity；不同 host 或非默认 port 隔离。默认 port（http 80/https 443/ssh 22/git 9418）与无 port 等价。
- 解析复用 workspace 既有 `url` crate：`Url::parse` 处理带 scheme 的 URL，SCP 形式仅做最小 `ssh://user@host/path` 改写后交给同一 parser，不手写 authority parser；ssh/git 是 non-special scheme（host 不折叠、显式默认 port 保留），由本函数统一折叠。接受 scheme 限 ssh/git/http/https；无 scheme 且非 SCP、空 host、带 query/fragment、path 不成 `org/repo` 形状一律返回 `None`，沿既有 cwd canonical path fallback，`file://` 等本地 origin 不并入网络 identity。
- 持久策略：identity 输入变化使 git-origin 工作区在下次构造时选择新 `{slug}-{hash8}` 目录；旧 hostless 目录不迁移、不并入、不删除，构造保持惰性（空旧目录仍由既有 gc 按 max_age_days 回收，含 durable 内容的目录原样保留）。不加 migration registry / version map；non-git fallback、`new_flat`、ephemeral 策略不变。
- 依赖：`memory/Cargo.toml` 增加 `url = { workspace = true }`（workspace 已声明、Cargo.lock 已有 2.5.8），Cargo.lock 仅在 memory 包依赖列表补一行，不整锁升级。
- 测试：`normalize_remote_url` 单元覆盖 transport 归一、host/port 隔离、host 折叠、path 大小写、单次 `.git`、IPv6、凭据排除、query/fragment 与本地 origin 降级；Tier 2 用真实 git2 临时 repo + origin 与 `MemoryStorage::new(root_override=temp)` 对照目录选择，含 worktree 同目录、旧 hostless 目录不被构造采纳/清除、`new_flat` 与无 origin 稳定。

## Impact

- 代码：`crates/codegen/memory/src/storage.rs`、`crates/codegen/memory/Cargo.toml`、`Cargo.lock`（memory 依赖列表一行）。
- 契约：`openspec/specs/memory-search/spec.md` 的 Scoped memory storage 由本 change delta 修改，明确 host-qualified identity、默认 port 等价与不可识别 origin 的 fallback。
- 行为影响：所有 git-origin 工作区的 workspace 目录 hash 输入新增 host，首次构造落到新目录；旧目录内容原样保留且不再被该工作区读写。同 org/repo 跨 host/port 不再共享记忆与索引。非 git 工作区目录名不变。
- 非目标：不建全面 Git URL 框架（不解析 SSH config、网络别名，不解码折叠 path）；不做旧目录迁移/合并/注册表；不改 Shell 调用方、backlog 与共享 docs。
