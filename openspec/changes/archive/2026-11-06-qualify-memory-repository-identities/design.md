# Design: qualify-memory-repository-identities

## 现状与所有者

- `compute_workspace_hash`（`crates/codegen/memory/src/storage.rs`）以 `extract_repo_identity` 的返回值作 hash 输入，slug 取 identity 最后一段；identity 失败时回退 cwd canonical path。
- `extract_repo_identity` 用 `git2::Repository::discover` 读 `origin`；`normalize_remote_url` 目前把 URL 裁成裸 `org/repo`（丢 host/port，`trim_end_matches` 反复去 `.git`）。
- 身份归一是 MemoryStorage 目录选择的内部职责；无其他调用方依赖 identity 形状（全仓仅本文件与 memory-search 规范引用函数名）。

## 决策

### Canonical identity 形状

`{host}[:{port}]/{repo_path}`：

- host 小写：domain、IPv4、`[IPv6]` 字面量统一折叠。special scheme（http/https）由 rust-url 解析时已折叠；ssh/git 是 non-special scheme，host 按 opaque 保留原大小写，由本函数 `to_lowercase()` 统一（百分号转义的十六进制大小写随之统一，转义本身不解码）。
- port 仅在与 scheme 默认不同时保留：`Url::port()` 对 special scheme 已去掉显式默认 port（80/443），non-special 的 ssh/git 需按表（22/9418）显式过滤。identity 不含 scheme，因此同 host/path/有效 port 的 http 与 ssh 表示归同 repo。
- repo path 取 `Url::path()`：URL 标准的点段归一由 parser 完成；本函数只做尾部斜杠不敏感处理、仅去一个末尾 `.git`、去一个前导斜杠，其余大小写与编码原样保留。仅去一个前导斜杠使 `git@host:/org/repo`（服务器绝对路径，URL path 以空段开始）与 `git@host:org/repo` 保持隔离，不做无依据合并。
- 必须呈 `org/repo` 形状（去前导斜杠后仍含 `/`），否则 `None`。

### 解析策略

- 带 scheme 的 URL 直接 `Url::parse`；无 scheme 时仅识别 SCP 形式：首个 `:` 前含 `@` 且不含 `/`，最小改写为 `ssh://{authority}/{path}` 后交给同一 parser 验证。不手写 authority parser。
- 接受 scheme 限 `ssh`、`git`、`http`、`https`。`file`、`ftp`、`rsync` 及未知 scheme 返回 `None`，沿 cwd canonical path fallback，不猜 host、不合并本地 origin。
- 凭据（userinfo）不进入 identity：只取 host、port、path。
- query 或 fragment 存在即 `None`：这类 origin 不是可安全识别的网络远端，宁可降级也不静默丢组件。

### 已知边界（如实记录，不扩框架）

- SCP 形式无 port 语法：`git@host:2222/org/repo.git` 按 git 语义把 `2222/org/repo` 当 path，identity 为 `host/2222/org/repo`，确定且不误合并。
- SCP 形式的 IPv6（`git@[::1]:org/repo`）经改写后无法通过 `Url::parse`，返回 `None` 降级；scheme URL（`ssh://git@[::1]:2222/org/repo.git`）正常支持。
- 不解析 SSH config、网络别名、尾部点 host、IDNA 之外的域名等价；不做 path 百分号解码。

### 持久化策略

- identity 进入 `compute_workspace_hash` 的 hash 输入，slug 仍取 repo 名，目录名仍为 `{slug}-{hash8}`；git-origin 工作区因 hash 输入含 host 而选择新目录。
- 旧 hostless 目录不自动归属新 identity：构造保持惰性，不迁移、不拷贝、不重命名、不删除。空旧目录由既有 gc 在 `max_age_days` 后回收；含 durable 内容的旧目录按现行 gc 规则永久保留，由用户自行处置。不加 migration registry / version map。
- `new_flat`（root 即工作区权威）与 non-git cwd fallback、ephemeral 策略不变。

### 依赖

`url = { workspace = true }`：workspace 已声明 `url = "2"`，Cargo.lock 已锁定 2.5.8 且被既有包使用，不引入新 package 版本；Cargo.lock 只在 memory 包依赖列表补一行。

## 备选与否决

- 保留裸 `org/repo`、仅在冲突时补 host：需要猜测与回退逻辑，且不同 host 仍会先合并再拆分；按确定性身份直接执行否决。
- `url::Host::parse` 显式标准化 host：`Url::host_str()` 已覆盖 domain/IPv4/`[IPv6]`（IPv6 输出为规范小写带方括号），再叠加 `Host::parse` 属重复分支；仅 `to_lowercase()` 即可覆盖 opaque host 折叠。
- 手写全套 authority parser：违反“勿增实体”，rust-url 已是 workspace 依赖。
- 迁移/注册表：旧hostless目录不能证明归属任一具体远端，自动迁移会重新引入混合风险；本次选择保留原位并说明目录切换，不增加迁移registry。
