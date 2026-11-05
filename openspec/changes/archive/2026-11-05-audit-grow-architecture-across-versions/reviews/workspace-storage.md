# Workspace、存储与索引分区审计

本报告只记录事实与独立修复边界，不修改业务实现。基线为 `d6128138cacdc22553aba9dafba9bc6cc934e4d1`（本区归一化指纹与 v2.6.0 相同）及审计时工作树。当前已确认四个应优先补验的机制问题：索引事件合并丢失删除、client FS 摘要缓存错配、worktree 删除跟随未核对的 gitfile 递归删除目标、hunk reject 覆盖尚未同步到缓存的外部编辑。另有 memory 项目标识碰撞和 envrc 路径被 shell 求值。本文逐项区分静态结论、原函数抽取实验及尚未运行的真实公共 API 验证。

## 边界和证据等级

- 范围：`crates/codegen/workspace` 非 permission 部分，以及 `fast-worktree`、`grow-gix-status`、`hunk-tracker`、`codebase-graph`、`fsnotify`、`memory`、`sqlite-journal`、`paths`。读取 shell 实际消费者用于确认所有权与可达性。permission、Timeline、工具执行授权和 native sqlite-vec 的实现结论归相邻分区。
- 所有 46 个本地 release tag 均解析了真实包路径、Cargo.toml blob、crate tree 和相邻 diff。v1.0.0 的 grow-/xai- 前缀已按包名映射；没有把旧路径缺失当作功能不存在。后附逐标签精确 OID 表。
- 读过根 `AGENTS.md`、OpenSpec 索引、本 change proposal/design/tasks、memory-search/configuration-rules/client-surfaces 的相关契约，以及 software-engineering 的 Rust、目录和交付约束；全仓只发现根 AGENTS。报告是本分区唯一仓库写入。
- 未启动 Cargo。父侧统一安排 `codebase-graph`、`hunk-tracker`、`fast-worktree` 公共 API 验证，避免并行重编译。下列实验用 `git show HEAD:<path>` 抽取原函数，`rustc --edition=2024` 编译标准库夹具；外部依赖用最小等价支撑替代，只能证明被抽取机制，不等于完整产品测试。
- `/tmp/grow-workspace-audit-stdlib.rs`、`/tmp/grow-workspace-audit-boundaries.rs` 和 `/tmp/grow-workspace-audit-hunk.rs` 是可丢弃实验。持久事实、输入、输出和通过标准已写入本文，报告不依赖这些临时路径长期存在。
- Atlas skill 无法加载（catalog 未提供），已报告限制。Atlas MCP 的焦点调用图仅作导航，返回预算截断，未作为完整调用方证明。

## 当前职责、所有权与真实消费者

| 模块 | 已核对的职责与消费链 | 审查结论及覆盖边界 |
|---|---|---|
| workspace handle/session/ops | `WorkspaceHandle` 组装共享资源，session 冻结 cwd/env，更新锁序列化工具集替换；`WorkspaceOps::bind_local_session/prepare_local_session_bind` 接 shell 已组装工具集 | terminal backend 是 session 资源，替换工具集复用；workspace 自建 hunk actor 持 cancel token，外部 tracker 的取消权不归 workspace。读了替换、失败清理、drop/session teardown 及对应寿命测试；没有把 shell 的持久化责任归到 workspace |
| workspace discovery/trust/config | shell folder-trust 负责决策消费，workspace 提供身份、磁盘 trust store、项目 config 扫描和 envrc | trust schema2 绑定 canonical key、root/marker/commondir 实体，锁内读改写、临时文件同步发布；审计工作树改为无配置读取的 topology discovery。项目配置 malformed 行为按实际 fallback 记录，permission 合并另区 |
| workspace file_system | 服务端 `ClientFs*` 经 `resolve_service_path` 根约束；shell ext ops 是另一契约；Local/ACP adapter 供工具与 rewind；fuzzy/index manager 为可复用资源 | client 的 root-relative、安全边界、分页/read cap 与 shell 绝对路径语义分开核对；path wrapper 本身不提供安全约束。摘要 memo 见 WS-02。ACP 删除返回明确 unsupported，不能当本地文件系统等价 |
| workspace file_state/recovery/export/status | rewind 的 before/after 快照、pinned historical file、恢复扫描；Git/JJ 状态和导出是外围 I/O | 当前 rewind 读取有单记录/总字节/条数限制，blocking worker 持 pinned guard 跨取消；移除的 checkpoint mirror 不算现实现。外围命令/清理仅做边界审查，未运行 GitHub 导出或真实清理 |
| fsnotify | canonical root→Weak 共享 watcher，专用 runtime，Git lock state settle/cooldown，元数据路径分类和目录 watch budget | shell `session/fs_watch.rs:1077` 遇 broadcast lag 会清空 op 缓冲并请求 rebuild；不能据 workspace 旧桥接的日志分支认定 shell 无恢复。shell 明确持 watcher/refresh join owner；平台 backend 未做 Linux/Windows 实机运行 |
| codebase-graph | shell `mvp_agent/code_nav.rs` 按能力懒启动并以 resident resource 持强引用；workspace map 存 Weak，底层 canonical 全局 Weak 去重；线程串行处理事件/查询 | 读了 actor、增量应用、cache/lock、生命周期和 coalescer 测试，确认 WS-01。查询/符号语言准确性不是本轮穷尽证明；缓存恢复与原子发布的剩余覆盖见文末 |
| hunk-tracker | shell 创建 actor；工具写通知与 watcher 更新 actor-owned baseline/current cache；`grow/hunk-tracker/hunk-action`→WorkspaceOps→handle→actor | 读了 actor batching、mutation、Git baseline/status、内容分类、actions/queries、LOC attribution sink 与测试入口。reject 的缓存/磁盘及错误提交问题见 WS-06，已作局部实验，真实 handle 尚未运行 |
| fast-worktree | workspace worktree 编排创建/流式反馈/后台 ignored copy；fast crate 负责 linked/standalone/overlay/btrfs/copy、DB/GC、移除 | 读了计划/执行、partial cleanup、copy worker/CoW、Git 注册、discovery、sync、GC 与消费者。copy cancellation 只停 walker，已排队 worker 需结束后清理；WS-03 是删除指针问题。未实机验证 privileged overlay/btrfs |
| grow-gix-status | hunk、worktree 的 gix status 通过预算 wrapper 限制 produce workers | 当前默认上限、环境 override、平台线程预算和资源耗尽子进程测试已核对；RLIMIT_NPROC 是 UID 范围，当前进程线程计数只是保守估算，不能声称全局绝不 EAGAIN |
| memory | shell spawn 创建 scoped storage/watcher/backend params；tool/injection/recovery 共享 factory；Markdown 是事实源，SQLite FTS/vector 是派生索引；dream 由 shell 调模型、storage 提交 | 读了 storage/backend/index/watcher/chunker/dream/lock、shell startup/dream 消费与测试。WS-04 是项目隔离问题；dirty 失败与 dream 并发见剩余问题，不把全部可能性直接定级 |
| sqlite-journal | 各 SQLite owner 选择 WAL 或 network TRUNCATE，必要时 per-host DB，读连接 query-only | 读了路径选型、网络 FS 检测、旧 WAL 处理与测试；没有启动网络 FS 夹具。host 名截断/清洗不是跨进程全局互斥保证 |
| paths | UTF-8 absolute/relative 表示、lexical normalize、join/contains | 测试明确 lexical 语义；RelPath 允许 `..` 不自动成为根约束漏洞，真正边界在各 I/O owner。没有建议再加一套泛化路径框架 |

## 当前发现

### WS-01：Modified→Removed 事件合并抹去已索引文件的删除（高，静态确认）

- 位置：`codebase-graph/src/index_manager.rs:1545` 的 `CoalescedEvents::insert`，`:1554` 将 `(Created | Modified, Removed)` 从 map 删除；`:1133` 的 `apply_coalesced` 只对剩余项执行 `remove_file`；`:1246` 才会删除该文件的索引内容。
- 可达链：shell `session/fs_watch.rs:467` 的 `CodebaseIndex::on_change`→`send_event`；普通非 Git-op 的 FilesChanged 在 `:700` 直接 forward，actor 将当前 drain 批次合并。已有文件快速修改后删除可进入同一批次，不需要恶意输入。
- 不变量：增量索引应收敛到文件系统最终状态。对新建后删除可以取消，但已存在文件 Modified→Removed 仍必须清理其 definitions/references/file metadata。现实现丢失这一事实；下一次重建或新事件之前可持续返回不存在文件的导航结果。
- 实验：从包含 `/repo/already_indexed.rs` 的集合出发，以 HEAD 原 `CoalescedEvents` 加 Modified、Removed；输出 `coalesced_events=0`、`deleted_file_still_indexed=true`。应用集合只是后果模型，真实 ScopeGraph API 尚未运行。
- 测试现状：`test_coalesce_modify_then_remove_cancels` 固定了空 map 的预期，这个预期本身不能证明正确。父侧补验应从真实已索引 Rust 文件，经生产合并与应用入口后查询 symbol，断言 definition/reference/file metadata 都不再含该文件；同时保留 Created→Removed 和 rename 的对照。
- 所有者：codebase-graph 的事件归约/应用 owner。修复边界是保留删除事实和回归序列，不需要改 shell 协议或另造事件系统。
- 历史：全部 46 tag + HEAD 的 coalescer 函数区块 SHA-256 都为 `c527e6c127113b49c9d0ec3f363fa50b3e632e9dd56f7642acea5f9f0a047f18`。这证明机制从 v1.0.0 延续，不证明每版 shell 的入口/能力条件完全相同。

### WS-02：client FS 以 size+mtime_ms 命中旧摘要，内容与 hash 不一致（中，机制实验确认）

- 位置：`workspace/src/file_system/client_fs.rs:50` 的 `MemoEntry`、`:68` 的 lookup、`:117` 的毫秒截断；`:247` 的 stat 命中即返回旧 hash，read_file 同样以 memo 命中决定仅读取范围。workspace shared 持有 memo，不在每次请求重建。
- 触发：同一路径从 `old` 改成 `new`，长度不变，mtime 分别为 epoch 秒 `1700000000` 的 100000ns 和 200000ns；时间在同一毫秒中。替换 inode/保留 mtime 的复制也满足条件。路径约束能限制读取位置，不能验证 memo 所指内容代际。
- 实验：HEAD 原 lookup/store（仅 parking_lot 换标准 mutex）加真实文件，输出 `different_ns=true;same_size=true;same_mtime_ms=true;new_content=new;cached_hash=Some("sha256-of-old")`。普通连续写是否恰落同毫秒无需概率推断，显式两个不同纳秒时间已构造充分条件。
- 后果：RPC 声称 full-content SHA-256，但可能给新 bytes 配旧 digest，影响消费者内容寻址/变更判断。服务根约束与 list/read cap 不覆盖此一致性。
- 所有者：workspace client FS hash memo。通过标准：真实 `ClientFsStat/ClientFsReadFile` 先缓存，再同大小同毫秒不同纳秒改写、原子替换/同 mtime 改写，返回 digest 必须匹配实际完整文件，或明确更改契约。真实公共操作未运行。
- 历史：全部 tag + HEAD memo 核心区块 SHA-256 相同：`f6ef62b3b318a37a54999c007e4e452b786a89d3963aecf4ec83d6122eea4314`。

### WS-03：worktree 清理把未经身份核对的 gitdir 当作可递归删除注册目录（高，静态与局部机制实验确认）

- 位置：`fast-worktree/src/api.rs:838` 的 `read_worktree_gitdir` 从 `<checkout>/.git` 读取 `gitdir: ...`，接受绝对/相对目录并 canonicalize；`:635` 清理前保存该指针，`:658` 在 checkout 删除后对目标 `remove_dir_all`，错误被忽略，仍返回成功 RemoveReport。没有在此边界核对 common-dir、`.git/worktrees/<id>` 拓扑或 registration/gitdir backlink。
- 可达入口：`workspace/src/worktree/mod.rs:1114` 的 remove→`:1173` 的 `fast_worktree::remove_worktree_with_delegate`；`shell/src/session/worktree.rs:199` 的失败回收调用同一公共删除 API。后者先证明 pristine，不能据此证明 `.git` 指针只能指注册目录。管理删除接受哪些路径的最终限制需父侧真实 API 测试确认。
- `/tmp` 受控实验抽取 HEAD `remove_worktree_from_disk` 与 `read_worktree_gitdir`；运行于 macOS，与原代码的非 Linux 路径一致；anyhow 只替成 std::io 错误上下文。只删除本实验创建目录。

| 输入 | gitfile/调用 | 实际输出 |
|---|---|---|
| 合法 linked | git 初始化并 commit，`git worktree add --detach <tmp>/linked HEAD`，调用抽取 remove | `result_ok=true;checkout_after=false;registration_after=false`，正确对照 |
| separate Git dir | `git init --separate-git-dir <tmp>/separate-metadata <tmp>/separate-checkout`；gitfile 为 `gitdir: <tmp>/separate-metadata` | `result_ok=true;checkout_after=false;metadata_after=false`；metadata 并非 linked 注册 |
| 普通无关目录 | checkout/.git=`gitdir: <tmp>/unrelated-owned-fixture`，目标内有 `must-survive.txt`，同样调用 | `result_ok=true;checkout_after=false;unrelated_target_after=false` |

- 不变量/后果：授权删除一棵 checkout 不应自动授权删除其可编辑 gitfile 指向的任意目录。canonicalization 只解决路径拼写，不证明删除所有权。底层机制可误删无关数据；生产入口是否接纳 separate-git-dir 及对抗 gitfile 的完整验证仍未运行，不能把抽取实验描述为真实公共 API 成功复现。
- 所有者：fast-worktree 删除 owner 应利用现有 source/common-dir/backlink/托管记录核对 registration 身份；workspace 编排提供可信来源或明确范围。已有 `git/worktree.rs` 的 scoped stale registration 清理说明项目已有可复用所有者，不需要新增清理框架。通过标准是公共 remove API 保持合法 linked 成功，面对 separate/无关目录保留无关目标并返回可判定结果。
- 历史：全部46 tag + HEAD 的 `read_worktree_gitdir` 原函数区块 SHA-256=`595a5d9e6620c1d1d4c4645b6f505a877bbcabe8c18c94a1f258ff7c51810044`；它证明指针解析机制未修复。删除副作用还逐版本核对 api 相邻 diff；完整公共入口的准入条件不能只靠此函数 hash 反推。

### WS-04：memory 项目标识去掉 host，两个不同仓库共享持久目录（中，静态与函数实验确认）

- 位置：`memory/src/storage.rs:665` 以 normalized repo identity 选目录，`:722` 的 `normalize_remote_url` 主动去掉 host，`:699` 对剩余 `org/repo` 作 hash。shell `session/actor/spawn.rs:1470` 的默认 storage 直接采用该身份；这不是随机 hash8 碰撞。
- 输入：`https://github.com/acme/widgets.git` 和 `https://gitlab.internal/acme/widgets.git`；HEAD 原 normalize 输出都为 `acme/widgets`。相同 slug/hash 输入必然生成相同 workspace memory 目录，MEMORY.md、sessions、index.sqlite 共享。
- 与契约关系：memory-search 的 Scoped memory storage 要求用工作区标识隔离项目；允许同仓库克隆/worktree 共用不意味着不同 host 的同名仓库可视为同一项目。需 owner 明确仓库身份的语义，避免默默合并知识/索引。
- 所有者：MemoryStorage 身份函数。通过标准：同 host 同 repo 的 HTTPS/SSH 表示可归一，跨 host/端口的不同 repo 必须区分；clone/worktree 等价用真实 Git 配置的 storage 构造验证。还应覆盖 `.git.git` 后缀（当前 trim_end_matches 可重复裁去），但不据此新增单独严重发现。
- 历史：全部 tag + HEAD normalize 区块 SHA-256 均为 `0c6beb60d49bc398bd1a95541681912deab4348308a518310144066b6175c2b5`。

### WS-05：envrc fallback 将目录名插入 shell 源码（中，静态与路径实验确认）

- 位置：`workspace/src/envrc.rs:129` 的脚本把 dir/envrc 以双引号插值，再以 `/bin/bash -c` 执行，`:155` 同步 output；`shell/src/agent/mvp_agent/agent_ops.rs:1557` 和 `acp_agent.rs:1183` 在创建/恢复会话时调用 trusted wrapper。
- 实验：自建目录名 `$(touch injected-marker)repo`，envrc 仅 `export AUDIT_OK=1`，按原脚本 cd/source 结构调用。输出 `exit_ok=false;path_command_executed=true`；Bash 求值路径中的命令替换，标记存在且 envrc 加载失败。双引号不使 shell 源码中的 `$()` 变成字面路径。
- 边界：wrapper 在未 trusted 时不执行，因此这不是证明 folder trust bypass；trusted `.envrc` 本身已是执行授权。问题是普通合法路径字节被当代码、可能执行路径附带动作和破坏初始化。direnv 成功时不会进入 fallback。同步 subprocess 无 timeout/cancel/output cap 的寿命问题是另一项边界，未做挂起进程实验。
- 所有者：workspace envrc 加载器及 shell 启动 I/O owner。通过标准：参数化传递路径并以特殊字符目录做真实加载验证，命令替换标记不存在、变量正确；取消/超时另立明确 subprocess 生命周期测试。
- 历史：`load_envrc_via_bash` 到下一 `load_envrc_or_empty` 声明前区块，v1.0.0 SHA-256=`304ad76653609a7444abfd9d0cafbaa0828ffb4957454cec19c135ab5597006e`，v1.1.0–v1.1.11=`97dc215609b7528ba4ffb44633f0f82bea3054754b727138e38a9fd3abb46ac6`，v2.0.0–v2.6.0/HEAD=`2ac2471062a39976e1da125e767823128b9a72245c1cbc2a8fbbb495a4d0efd6`。已核对改名/兼容清理的相邻 diff，双引号路径插值仍保留；没有运行每版启动路径。

### WS-06：hunk reject 用落后缓存覆盖磁盘，失败时统计也提前提交（高，静态与局部机制实验确认）

- 位置：`hunk-tracker/src/actor/actions.rs:169` 的单 hunk reject，`:202` 从缓存 current patch，`:235` 直接覆盖磁盘；新建/删除分支分别删除磁盘文件或恢复缓存 baseline。`apply_hunk_action:84` 和 batch `:382` 在 fallible I/O 前先累加统计、移除 turn_index。
- 可达链：shell `extensions/hunk_tracker.rs:391` 的 `grow/hunk-tracker/hunk-action`→`WorkspaceOps::HunkSingleActionReq(:613)`→handle→actor `mod.rs:368`。普通外部写在 OS watcher 通知送达 actor 前，可与已缓存内容不一致；actor 只序列化它已接收的消息，不拥有磁盘外部 writer。
- 既定契约：`types.rs:240` 的 Accept 更新 affected lines 的 baseline；Reject 回退 hunk。实现也只用该 hunk 的 start/count/old_text patch 缓存，并非显式要求用户同时放弃尚未展示的其他行。应保留无关新编辑或报告冲突，不能把缓存当作当前磁盘事实。
- `/tmp/grow-workspace-audit-hunk.rs` 抽取 HEAD 原 `apply_hunk_action/update_session_stats/remove_from_turn_index/accept_hunk/reject_hunk/patch_lines`，Tokio FS 替成执行同一 std::fs write/remove 的 ready async shim。`send_event` 与 `recompute_hunks` 为空支撑；它们位于磁盘写入之后，不能改变本次已观测的写入后果，但该实验不是完整 actor/handle 测试。

| 受控序列 | 实际结果 |
|---|---|
| baseline=`base\nkeep\n`，cache=`agent\nkeep\n`，所选 hunk 仅第1行；缓存建立后磁盘变为 `agent\nkeep\nexternal-unique\n`，不注入 watcher 更新，调用原 apply_hunk_action Reject | `result_ok=true;disk="base\nkeep\n";external_edit_survived=false`，未涉及第1行回退的外部新增行被覆盖 |
| 相同 cache/hunk，真实目标是目录，触发 std/Tokio write 的 IsADirectory | 返回 `Err(WriteError { .. })`；`rejected_stats=1;turn_index_contains=false;hunk_still_present=true`，失败后计数/turn projection 与未移除 hunk 分歧 |

- 调用方结果：磁盘 I/O 错误转换为 WorkspaceError::HunkActionFailed；shell extension `:416` 返回 `ActionResponse { success:false,error:Some(..),affected_count:None }`。外部行丢失场景写入成功，现实现返回 success:true，没有冲突信号。
- 所有者：hunk actor 的动作提交边界。通过标准：真实 `HunkTrackerHandle::hunk_action` 在 cache→磁盘外部写→Reject 序列保留无关行或明确冲突；强制写/删失败时 hunk、turn index、统计保持未提交状态。还应验证 file/turn/all batch 的部分成功可解释性，不必改成全项目事务。
- 历史：全部46 tag + HEAD 的 reject_hunk 区块 SHA-256=`6e103e213cce87321b4f37f2631879bb6b8fbb4a865268e88fb082ee6add8c54`，apply_hunk_action 区块=`1de0d115ad3fe63917f5eb840653fc19d770caedf89e4f4fbbfc839cb9df21d2`。核心机制跨版本相同，真实公共 API 与历史各版 watcher 时序未运行。

## 历史上已修复或已移除的事项

| 版本 | 已核对差异与意义 | 当前判断 |
|---|---|---|
| v1.0.0→v1.1.0 | `grow-workspace`/`xai-*` crate 边界改名；读取旧 foreign_sessions 的 module 与 Codex DB scanner，它们调用 ApprovedRoot/只读 transaction，并限制候选、时间和投影文本大小；v1.1.0 删除 foreign vendor session scanner | 不把旧模块消失当未覆盖，也不建议恢复跨工具兼容功能 |
| v1.0.0 独立 xai-file-utils→v1.1.0 workspace 内部模块 | `xai-file-utils/src/lib.rs` 以92%相似度移至 `workspace/src/file_utils.rs`，streaming SHA-256 函数体相同；`src/workspace_classifier.rs` 以100%相似度移至 `workspace/src/file_utils/workspace_classifier.rs`；旧 Cargo.toml 删除。当前 pager project_picker/detection re-export workspace 分类函数 | 旧 crate tree=`1971c2bdc649f503eb3caef5e68798d4dbdafedc`，manifest blob=`9b754aecc3de4dde93cd9fd11a81c950e88738fa`，lib blob=`358d31c504a3b664fee3b4f9865428e3cefe1e02`，classifier blob=`5fecbee2d16feb683b62c9f03745b1166e7c307c`。这是并入 owner，非丢功能；该独立历史 crate 不计入下方九包 S，作为显式补充证据 |
| v1.1.2 | workspace content search 增加 rg 缺失的安装提示，避免只返回 spawn ENOENT | 错误可解释性改进，未改变数据 owner |
| v1.1.4 | 合并 TLS/HTTP/hashbrown/nix 等依赖，memory 测试安装 ring provider，分类测试共享 env lock | manifest 变化与行为变化区分；不把测试环境修复算新架构 |
| v1.1.5–v1.1.11 | capability 未分类工具收紧、目标工具种类拆分、fuzzy on_update 唤醒及相关 wrapper；后续能力 authority 已迁出 workspace | 能力策略细节由权限分区合成；fuzzy 从轮询到事件唤醒有明确消费者需求 |
| v2.0.0 | 移除 workspace capability、checkpoint/checkpoint_store 及双重 rewind 权限；WorkspaceShared 收敛为资源组装，shell Timeline 是 durable owner。读取旧 checkpoint store 的镜像、cap、serialize 与失败分支 | 旧镜像恢复未接线和双轨权威属于移除历史，不当作当前缺陷；本轮不重建 mirror |
| v2.0.1 | prepare/commit local session bind 持 owned update guard；post-turn capture buffer_unordered(8) 限制 remote/ACP fanout | 更新锁与调用侧 causal gate 的边界更清楚，读取实际实现及生命周期测试 |
| v2.0.2 | memory GC 只删无 durable 内容且过期的目录；非空 tmp、只有 MEMORY.md/SQLite 的目录保留，flat root 不按多工作区解释 | 先前可删除有效记忆，现版对应回归断言已改为保留，不登记成当前阻塞 |
| v2.0.4 | 去除 managed config 层的 folder trust 配置来源，discovery 说明同步 | 按当前配置权威审视，不延续旧优先级假设 |
| v2.1.0 | tree-sitter/git2/rusqlite 升级；embedding endpoint-bound capability；FTS backend factory；trust identity/topology 收紧与 ACP API 适配 | 当前读存储层和 shell factory；依赖升级不代表已覆盖平台运行 |
| v2.1.3 | index cache 复用有界 cwd encoding，新增长路径不同身份测试 | 旧 URL 编码目录可能超255字节的历史风险已有针对修复 |
| v2.1.4 | vector 空间 identity/dimensions 变化失效缓存并在无 dirty Markdown 时补 embedding | 保留 FTS，不让旧模型向量混入新空间 |
| v2.1.5 | embedding upsert 绑定 chunk hash 防止异步旧结果覆盖；claim token 加 nonce 并条件释放；watcher 过滤 sibling workspace；移除 disconnected rewind wrappers，严格历史读取失败不当空成功 | 都是已有一致性修复，应在总报告列已修复历史，不能重复报为当前 |
| v2.2.0 | fuzzy pending coalesce＋query_id 区分最新请求，Drop 非阻塞并通知取消；rewind 预算、严格字段、blocking worker 持源锁跨取消 | 解决旧代搜索结果与 UI 阻塞，持久 rewind 不再接受被截断/损坏的集合 |
| v2.3.0 | fuzzy 测试-only 调整，无新的生产语义 | 相邻 diff 已核对 |
| v2.4.0 | standalone/auto GC 没有 session 恢复 authority，因此默认不回收 subagent checkout/记录；增加 pristine 检查，ignored 内容也算用户数据 | 现版不把 TTL 当恢复责任的替代；force 仍明确是破坏管理语义 |
| v2.4.1–v2.5.0 | Windows/dunce canonical 路径与 Clippy/test 进程 owner 说明收敛 | 不以样式差异判架构债务 |
| v2.6.0 | fsnotify 用 bounded heap 保留最浅 watch 目录且维持 stable ordinal；hunk query 共享 line tables 并有旧输出 oracle；daily log 返回 Written/SkippedEphemeral；walk 保留 symlink metadata | 区分性能纯重构与 daily log 成功事实；没有提前把 ephemeral skip 当 persisted path |

## 后续验证与覆盖限制

1. 父侧统一补验 WS-01/02/03/06 的真实公共 API；各项已列生产入口、触发序列和可证伪断言。当前无 Cargo 或全产品测试通过声明。
2. memory watcher `take_dirty` 后 reindex/delete 失败不 requeue（backend.rs:247–275）；`reindex_file` 读失败还返回空 stats，search 可能继续旧 FTS，下一启动全量 reindex 可恢复。取消期间 reindex claim 只有显式 release，stale threshold 提供延迟恢复；这些是可定位的一致性/恢复风险，未做故障注入，不凭静态可能性扩出实现。
3. dream 锁注释明确 best-effort 而非互斥；shell 在模型调用之后才 `execute_dream` acquire。并发保存与 cleanup 的真实冲突尚未运行；内容写入直接 std::fs::write、没有代际对比。`build_dream_user_message` 的 32000 是追加整个 session 后才检查，40001 bytes 单文件的原函数实验产出40025 bytes；这不等于精确上界。留给 memory owner 独立确定约束，不与本轮数据完整性修复混写。
4. codebase-graph 的 cache 发布为 `ScopeGraphIndex::save`（graph.rs:1364）直接 File::create/flush，reader（:1463）根据磁盘长度字段分配；格式/版本和普通 I/O 错误可触发重建，但没有据此证明损坏长度、并发进程读取半写入缓存也能有界恢复。未做坏缓存/并发发布故障注入，不把恢复能力说满。
5. worktree async claim 在 `workspace/worktree/mod.rs:836–840`、`:1537–1542` 通过正常 await 后手动 mark complete，后台 copy 注册另有 Drop guard。creator future 中断是否会残留 in-progress 标记尚未做真实 owner 取消实验，不能把 copy 的 cleanup guard 推及所有创建状态。
6. 全部包已做架构入口/依赖/状态 owner 审查，重点数据路径逐函数核对；语言 grammar/query、各平台 OS watcher、privileged overlay/btrfs、network FS SQLite、CLI bench、每条外围错误分支未穷尽动态验证。后附 tree/hash 表证明版本枚举与复用，不是每个函数/版本都通过测试的证明。
7. 交付前已重读本区 `git diff`：`fsnotify/source.rs`、workspace `session/git.rs` 和 `trust.rs` 仍是用户已有无配置发现修复，已保留；shell 模型配置/effort 等预先修改未覆盖。本分区只新增本报告，未改其他报告、规范、tasks 或 backlog，未提交 Git。
8. 本分区文档检查已执行 `openspec validate --all --strict --no-interactive`：18 passed、0 failed。全 change 的任务勾选、综合核实和归档由父侧 owner 执行，本报告不把 strict 成功当公共 API 运行验证。

## 逐标签证据（46 个 release + HEAD）

`S` 是排除 workspace/permission 后、按归一化 crate 相对路径与 blob OID 排序的 SHA-256 指纹；它包含本区 manifest、实现及测试。具体输入为 `"\n".join(f"{module}/{relative_path} {blob_oid}" for ... in sorted_items)` 的 UTF-8 字节，末尾没有换行；路径来自每版 `git ls-tree -r`，各旧 crate 根替成上表现代模块名。47 行 S 及全部423组实际 tree/manifest 对象已重新计算核对。

`树/manifest组` 依次为 W/F/G/H/C/N/M/J/P，对应 workspace、fast-worktree、grow-gix-status、hunk-tracker、codebase-graph、fsnotify、memory、sqlite-journal、paths。组定义见后表，实际 Git crate tree 包括 permission，不能把 S 等同于原始 workspace tree。相同 S 可复用本区源码证据，仍逐行记录相邻差异与版本。

| 标签 | Commit | S（前12） | 树/manifest组 | 相邻差异（本区，permission 排除） |
|---|---|---|---|---|
| v1.0.0 | `da034e22d87b` | `b711f27086ec` | 1/1/1/1/1/1/1/1/1 | 初始历史基线 |
| v1.1.0 | `7ea59a9db583` | `eebff2ecca97` | 2/2/2/2/2/2/2/2/2 | 180 项，见演进说明 |
| v1.1.1 | `4f8e0232eae0` | `eebff2ecca97` | 2/2/2/2/2/2/2/2/2 | 与 v1.1.0 的 S 相同 |
| v1.1.2 | `abca23e0b295` | `e85b2267473e` | 3/2/2/2/2/2/2/2/2 | 1 项，见演进说明 |
| v1.1.3 | `1b24b9f03668` | `e85b2267473e` | 3/2/2/2/2/2/2/2/2 | 与 v1.1.2 的 S 相同 |
| v1.1.4 | `28b798dc17b2` | `7dee111a3de8` | 4/3/2/2/3/3/3/2/2 | 7 项，见演进说明 |
| v1.1.5 | `05a3fbcc57c1` | `65d60b7512ee` | 5/3/2/2/3/3/3/2/2 | 4 项，见演进说明 |
| v1.1.6 | `ca3400250af9` | `b4cea5f4ae57` | 6/3/2/2/3/3/3/2/2 | 1 项，见演进说明 |
| v1.1.7 | `62d9b47fd9d5` | `b4cea5f4ae57` | 7/3/2/2/3/3/3/2/2 | 与 v1.1.6 的 S 相同 |
| v1.1.8 | `bde0050632c2` | `98b8c142fd3c` | 8/3/2/2/3/3/3/2/2 | 1 项，见演进说明 |
| v1.1.9 | `465d9b12a42d` | `98b8c142fd3c` | 8/3/2/2/3/3/3/2/2 | 与 v1.1.8 的 S 相同 |
| v1.1.10 | `38b2d7fc7039` | `406bcc6d634c` | 9/3/2/2/3/3/3/2/2 | 2 项，见演进说明 |
| v1.1.11 | `79495bcd40e5` | `94018de419a1` | 10/3/2/2/3/3/3/2/2 | 1 项，见演进说明 |
| v2.0.0 | `2282940c1120` | `0bcbd9b23036` | 11/4/2/3/4/4/4/2/2 | 39 项，见演进说明 |
| v2.0.1 | `ae8df2a3b4b9` | `82656ca4edfa` | 12/4/2/3/4/4/4/2/2 | 5 项，见演进说明 |
| v2.0.2 | `7233f4e51e6d` | `d62b39802db6` | 13/4/2/3/4/4/5/2/2 | 2 项，见演进说明 |
| v2.0.3 | `5aabf367b53a` | `d62b39802db6` | 13/4/2/3/4/4/5/2/2 | 与 v2.0.2 的 S 相同 |
| v2.0.4 | `1b52597ccdb5` | `7240614f05bf` | 14/4/2/3/4/4/5/2/2 | 3 项，见演进说明 |
| v2.1.0 | `d43756888adf` | `8d45d1aff266` | 15/5/2/3/5/5/6/3/2 | 21 项，见演进说明 |
| v2.1.1 | `96565d6975d8` | `8d45d1aff266` | 15/5/2/3/5/5/6/3/2 | 与 v2.1.0 的 S 相同 |
| v2.1.2 | `1ca5c6b120ea` | `8d45d1aff266` | 16/5/2/3/5/5/6/3/2 | 与 v2.1.0 的 S 相同 |
| v2.1.3 | `09c68b808c41` | `590b3271ee7d` | 17/5/2/3/5/5/6/3/2 | 1 项，见演进说明 |
| v2.1.4 | `87d1df4c3b0a` | `785c349c4bea` | 17/5/2/3/5/5/7/3/2 | 6 项，见演进说明 |
| v2.1.5 | `a8dc93716f9a` | `39fd2dce0784` | 18/5/2/3/5/5/8/3/2 | 8 项，见演进说明 |
| v2.1.6 | `2a8297e818b6` | `39fd2dce0784` | 19/5/2/3/5/5/8/3/2 | 与 v2.1.5 的 S 相同 |
| v2.1.7 | `9f721d8e927c` | `39fd2dce0784` | 20/5/2/3/5/5/8/3/2 | 与 v2.1.5 的 S 相同 |
| v2.1.8 | `7ed6441a8fb8` | `39fd2dce0784` | 20/5/2/3/5/5/8/3/2 | 与 v2.1.5 的 S 相同 |
| v2.1.9 | `bd1f5085a9c9` | `39fd2dce0784` | 20/5/2/3/5/5/8/3/2 | 与 v2.1.5 的 S 相同 |
| v2.1.10 | `c17b085d07f8` | `39fd2dce0784` | 20/5/2/3/5/5/8/3/2 | 与 v2.1.5 的 S 相同 |
| v2.1.11 | `3024dad1ef75` | `39fd2dce0784` | 20/5/2/3/5/5/8/3/2 | 与 v2.1.5 的 S 相同 |
| v2.2.0 | `a84ce03b0d13` | `d9150ddc6451` | 21/5/2/3/5/5/9/3/2 | 7 项，见演进说明 |
| v2.2.1 | `b38627016b41` | `d9150ddc6451` | 22/5/2/3/5/5/9/3/2 | 与 v2.2.0 的 S 相同 |
| v2.2.2 | `afbe3e48209d` | `d9150ddc6451` | 22/5/2/3/5/5/9/3/2 | 与 v2.2.0 的 S 相同 |
| v2.3.0 | `ef665ec41eac` | `0a1976d0f5e2` | 23/5/2/3/5/5/9/3/2 | 1 项，见演进说明 |
| v2.3.1 | `7b7d56ab98a6` | `0a1976d0f5e2` | 23/5/2/3/5/5/9/3/2 | 与 v2.3.0 的 S 相同 |
| v2.4.0 | `cd5d88ec7f5d` | `9484cb40cdd3` | 24/6/2/3/5/5/9/3/2 | 4 项，见演进说明 |
| v2.4.1 | `5fcbd12b0c10` | `4f6d7b6870f0` | 24/7/2/3/5/5/9/3/2 | 1 项，见演进说明 |
| v2.4.2 | `ecb0f9cc6b48` | `4f6d7b6870f0` | 24/7/2/3/5/5/9/3/2 | 与 v2.4.1 的 S 相同 |
| v2.4.3 | `2069c43db5a3` | `4f6d7b6870f0` | 24/7/2/3/5/5/9/3/2 | 与 v2.4.1 的 S 相同 |
| v2.5.0 | `8443a4d1648d` | `d7bd50c74c4b` | 25/8/2/3/5/5/9/3/2 | 2 项，见演进说明 |
| v2.5.1 | `a58b5dd7e8c7` | `d7bd50c74c4b` | 25/8/2/3/5/5/9/3/2 | 与 v2.5.0 的 S 相同 |
| v2.5.2 | `1c88fccb4adf` | `d7bd50c74c4b` | 25/8/2/3/5/5/9/3/2 | 与 v2.5.0 的 S 相同 |
| v2.5.3 | `fce2f3d590dd` | `d7bd50c74c4b` | 25/8/2/3/5/5/9/3/2 | 与 v2.5.0 的 S 相同 |
| v2.5.4 | `069f712894ca` | `d7bd50c74c4b` | 25/8/2/3/5/5/9/3/2 | 与 v2.5.0 的 S 相同 |
| v2.5.5 | `599873595c94` | `d7bd50c74c4b` | 25/8/2/3/5/5/9/3/2 | 与 v2.5.0 的 S 相同 |
| v2.6.0 | `5475dff997f0` | `102854a16261` | 26/8/2/4/5/6/10/3/2 | 9 项，见演进说明 |
| HEAD | `d6128138cacd` | `102854a16261` | 26/8/2/4/5/6/10/3/2 | 与 v2.6.0 的 S 相同 |

### crate 树与 manifest 精确 OID

v1.0.0 的前缀名由包名映射确认，v1.1.0 起使用现路径；不是从当前路径反推历史。以下每组的路径、crate tree OID、Cargo.toml blob OID 均来自对应标签。

| workspace组 | 真实路径 | crate tree OID | Cargo.toml blob OID |
|---|---|---|---|
| 1 | `crates/codegen/grow-workspace` | `c2858ba72cdeb686ef6a8e5a01e1792ba3962622` | `713f8f8e36987f2faad4dcb01fedd90e8a8e180f` |
| 2 | `crates/codegen/workspace` | `0ffaf77670ff2c373f4f77268c2c7bfab53878d6` | `bd64f27d131e81da94bc7eacdae51291a91d677d` |
| 3 | `crates/codegen/workspace` | `e2670a04cbc2a8b1490f93aace547dae43ef7d61` | `bd64f27d131e81da94bc7eacdae51291a91d677d` |
| 4 | `crates/codegen/workspace` | `017646e1d738fa83e0f19958d33e642e4fbab4f8` | `0eba9b125af3becaf22e7600c1115098ee6ee178` |
| 5 | `crates/codegen/workspace` | `40b860322d9f1bec80317da206646d045e165fe4` | `0eba9b125af3becaf22e7600c1115098ee6ee178` |
| 6 | `crates/codegen/workspace` | `cb3fd799f81c0afac7dfbdc22055adcdca001515` | `0eba9b125af3becaf22e7600c1115098ee6ee178` |
| 7 | `crates/codegen/workspace` | `1a4ae9b3844de5ff4bc0ad7400d8a55c0da1a1d1` | `0eba9b125af3becaf22e7600c1115098ee6ee178` |
| 8 | `crates/codegen/workspace` | `2128ea57f6adf55d69d4b0795557c1c6a9eab1b3` | `0eba9b125af3becaf22e7600c1115098ee6ee178` |
| 9 | `crates/codegen/workspace` | `3869dfef304311d3fba56fe8ed514af24ebf1006` | `0eba9b125af3becaf22e7600c1115098ee6ee178` |
| 10 | `crates/codegen/workspace` | `ed77aba68339f93ad0f97c934f3ede0fe4a8d558` | `0eba9b125af3becaf22e7600c1115098ee6ee178` |
| 11 | `crates/codegen/workspace` | `ba2aff6c64f6fd2498d5bcc70a34b8309c437de5` | `0eba9b125af3becaf22e7600c1115098ee6ee178` |
| 12 | `crates/codegen/workspace` | `c2801053fd09b6f60b7aad9917a175a896608849` | `0eba9b125af3becaf22e7600c1115098ee6ee178` |
| 13 | `crates/codegen/workspace` | `b5b10030e39c06135f7b26b7b2b05733dfebe17d` | `0eba9b125af3becaf22e7600c1115098ee6ee178` |
| 14 | `crates/codegen/workspace` | `878c7875bb8ab90e04ccac95ae62a6d7b0886e61` | `0eba9b125af3becaf22e7600c1115098ee6ee178` |
| 15 | `crates/codegen/workspace` | `83dbaacc51a7b8e61dc01c291b2d72ddee84ecf9` | `31070dce32ce678324bbcc17a084b8ee8ef4b813` |
| 16 | `crates/codegen/workspace` | `4b32c01ee3b5872ec553c0d54f1fe39299ce6b1d` | `31070dce32ce678324bbcc17a084b8ee8ef4b813` |
| 17 | `crates/codegen/workspace` | `37c82d35caf0e3516b2cd359239bf3840ea098c1` | `31070dce32ce678324bbcc17a084b8ee8ef4b813` |
| 18 | `crates/codegen/workspace` | `2544b1bdf274f48761a404fca994be9a62967368` | `31070dce32ce678324bbcc17a084b8ee8ef4b813` |
| 19 | `crates/codegen/workspace` | `0cda617b55fc08e746b1ca371d5054a0eee7cf42` | `31070dce32ce678324bbcc17a084b8ee8ef4b813` |
| 20 | `crates/codegen/workspace` | `319fc341a5d7629f744e27645ece9fd940cf30ca` | `31070dce32ce678324bbcc17a084b8ee8ef4b813` |
| 21 | `crates/codegen/workspace` | `6ead424225249873a1e02481b1010d33449807e2` | `31070dce32ce678324bbcc17a084b8ee8ef4b813` |
| 22 | `crates/codegen/workspace` | `955559392d43c8bcf55dfaaee422ecd49c23d6b0` | `31070dce32ce678324bbcc17a084b8ee8ef4b813` |
| 23 | `crates/codegen/workspace` | `e49c69695646e6815dd16ef7e61a01ad3f279bb0` | `31070dce32ce678324bbcc17a084b8ee8ef4b813` |
| 24 | `crates/codegen/workspace` | `476dc6255e4481835e93af8d933e7138ea63d8a5` | `31070dce32ce678324bbcc17a084b8ee8ef4b813` |
| 25 | `crates/codegen/workspace` | `fbb17907e4072ebd05ad367a552ecbbf564f1919` | `31070dce32ce678324bbcc17a084b8ee8ef4b813` |
| 26 | `crates/codegen/workspace` | `ff93ba5e7d08316b767f68c6fb5ae0e6b8db8989` | `31070dce32ce678324bbcc17a084b8ee8ef4b813` |

| fast-worktree组 | 真实路径 | crate tree OID | Cargo.toml blob OID |
|---|---|---|---|
| 1 | `crates/codegen/xai-fast-worktree` | `bd66fe9dc14eaea861fa90cdf1529fe244634e08` | `c6057a9abee2014c8e3312ceda7d160e45acb7f3` |
| 2 | `crates/codegen/fast-worktree` | `af41e58a4ab0c0078bffbabe60a064b7315aee05` | `bacb67f178368128bc4d89c34753bb3843e9334c` |
| 3 | `crates/codegen/fast-worktree` | `58295249d037e88e2c09aa5c505c26f2adeb6d19` | `38c88f28d405400d8d0d12981bd1adbc749e7e8c` |
| 4 | `crates/codegen/fast-worktree` | `38c1196cd7bf704d7de6733d4e57087452135466` | `38c88f28d405400d8d0d12981bd1adbc749e7e8c` |
| 5 | `crates/codegen/fast-worktree` | `f462cded260f4535ee83674c8cf182214d84ca16` | `fcdaea654906f625ab904d12eaa0490edd5b239f` |
| 6 | `crates/codegen/fast-worktree` | `62d4836751020cde6fd29a6d76f7730d86ff0e96` | `fcdaea654906f625ab904d12eaa0490edd5b239f` |
| 7 | `crates/codegen/fast-worktree` | `20dacc6454285d31784e3348addd6f0f7c402598` | `fcdaea654906f625ab904d12eaa0490edd5b239f` |
| 8 | `crates/codegen/fast-worktree` | `73e1cbc03a09c56391c656b63067dc6037014393` | `fcdaea654906f625ab904d12eaa0490edd5b239f` |

| grow-gix-status组 | 真实路径 | crate tree OID | Cargo.toml blob OID |
|---|---|---|---|
| 1 | `crates/codegen/xai-gix-status` | `3c037e32bb410d2410de3cbaecb4911b12e6d597` | `65c06ba6a09dda5a228629bcb7d8550ee1e43545` |
| 2 | `crates/codegen/grow-gix-status` | `0428352b48c0ca4176d0965559bb0e6f336b07d1` | `7ff989b5d09859ac5ba354cf41ca2590771c17be` |

| hunk-tracker组 | 真实路径 | crate tree OID | Cargo.toml blob OID |
|---|---|---|---|
| 1 | `crates/codegen/xai-hunk-tracker` | `0946f88ff3a76e7ec894ae798de24ec21b7c78ff` | `d39e48e46a7084f37307b138cf06a4e508d0cf43` |
| 2 | `crates/codegen/hunk-tracker` | `b072611d57c9fc643ec3169e773746eb09bcf96e` | `3d1e6c95651d24e0f76deae9f5df23acd9c19eb7` |
| 3 | `crates/codegen/hunk-tracker` | `bc22ae4d4bb3cab4f3fe33ad7e01a8628ef89c9c` | `3d1e6c95651d24e0f76deae9f5df23acd9c19eb7` |
| 4 | `crates/codegen/hunk-tracker` | `b238498631917605bc3fc8ad7ec52a715d8dac10` | `3d1e6c95651d24e0f76deae9f5df23acd9c19eb7` |

| codebase-graph组 | 真实路径 | crate tree OID | Cargo.toml blob OID |
|---|---|---|---|
| 1 | `crates/codegen/xai-codebase-graph` | `274b33f8cc4c148e820339bcd1a528546252cb42` | `99e77d1bf5bfe5066fc5d99ba638b17552683e65` |
| 2 | `crates/codegen/codebase-graph` | `1f9adbe6cd606be872f26d327a77882f30c3229e` | `12163ce99dc90d4948710741c1621d13ab905444` |
| 3 | `crates/codegen/codebase-graph` | `04eaec6b17dc42ae9f1a6b17a27f450ee74b98ec` | `2b76bcfd92495f0b6666b0ce1487b02c60fec3f4` |
| 4 | `crates/codegen/codebase-graph` | `4c099757ad022f5ea7568c1d377a0902b5452365` | `2b76bcfd92495f0b6666b0ce1487b02c60fec3f4` |
| 5 | `crates/codegen/codebase-graph` | `89ee7447daaf323be62ea0d1ed525bd67e085d8f` | `e3bb91cc4d3dd4138e75187f37afdd23154be533` |

| fsnotify组 | 真实路径 | crate tree OID | Cargo.toml blob OID |
|---|---|---|---|
| 1 | `crates/codegen/xai-fsnotify` | `5b4241da91c6ac303aa78e4ce2d4118b777b62d4` | `23403f80c5c6a67571e1b7c980d57af015690a2c` |
| 2 | `crates/codegen/fsnotify` | `ad05afe671b7be1e345ffc52e37b41f4144e08be` | `e38f730e585fcb439c862420f4155900b085f04f` |
| 3 | `crates/codegen/fsnotify` | `bb0434c78cbd90b3af44b0b4d818913516e49cab` | `2fb85ce93acb408df1c7333414c3e4fa8cbd467c` |
| 4 | `crates/codegen/fsnotify` | `39640cd6ce1c6571b5c5b75c2ae2861c06fb7179` | `2fb85ce93acb408df1c7333414c3e4fa8cbd467c` |
| 5 | `crates/codegen/fsnotify` | `41a3a43fb36339eb6347dfe8b211d4936b451e3a` | `67f8247a01bb4b323214abdecc7d835d950a8bfa` |
| 6 | `crates/codegen/fsnotify` | `4372a19c9c56f7547569371a0333ae4607312612` | `67f8247a01bb4b323214abdecc7d835d950a8bfa` |

| memory组 | 真实路径 | crate tree OID | Cargo.toml blob OID |
|---|---|---|---|
| 1 | `crates/codegen/grow-memory` | `d84022be0e6e781209990b94af163f90cd17103e` | `d057679899ea453b036a252ca2881719cc711e48` |
| 2 | `crates/codegen/memory` | `1e1b06cb7b77abdd89a4745e7586581f1345c373` | `dce4a7f6d23072e8cea8b8eb601ed596d0ec7231` |
| 3 | `crates/codegen/memory` | `c4169ecc64cd9aeb28ed004b2cbafd498bb45644` | `1714e94474075d3550da7f4664d1666617fe5199` |
| 4 | `crates/codegen/memory` | `c79e02868d00f6d5a8cc8b575ccef5561981d28d` | `1714e94474075d3550da7f4664d1666617fe5199` |
| 5 | `crates/codegen/memory` | `1531e0a69ce4929c817ce6c3107919db58631f13` | `1714e94474075d3550da7f4664d1666617fe5199` |
| 6 | `crates/codegen/memory` | `3de56b6968d904e224d42d9fe9d5850ed0117a8e` | `c14f220cd42815e405a182df0a4e2cc04f845c66` |
| 7 | `crates/codegen/memory` | `f89d1c12da9c8eb7692fb597401955d6dcc40697` | `c14f220cd42815e405a182df0a4e2cc04f845c66` |
| 8 | `crates/codegen/memory` | `6b1785274d3e45edfdd4ee44b40d7093d45b9ee0` | `c14f220cd42815e405a182df0a4e2cc04f845c66` |
| 9 | `crates/codegen/memory` | `f28b634df7cae95538c65cdc78f95a75a0b34b19` | `c14f220cd42815e405a182df0a4e2cc04f845c66` |
| 10 | `crates/codegen/memory` | `6f0dca0e907f7347a0e7722d5a40a7090e6b325c` | `c14f220cd42815e405a182df0a4e2cc04f845c66` |

| sqlite-journal组 | 真实路径 | crate tree OID | Cargo.toml blob OID |
|---|---|---|---|
| 1 | `crates/codegen/xai-sqlite-journal` | `54b34cc6fd0fe2a56f7cad054a4957a72d4a3566` | `024cc8b6404172d3f56a365422dd5dfc492cee4f` |
| 2 | `crates/codegen/sqlite-journal` | `cca733c589c16da5f90999a992f5179c56d9ad04` | `970806bc3271f908404b7c8d41167a5ba89d2d3a` |
| 3 | `crates/codegen/sqlite-journal` | `90c8d50799705a976e80310735fa4dd491d46155` | `9163d3fd89db38598958496b483c068b180d3e0e` |

| paths组 | 真实路径 | crate tree OID | Cargo.toml blob OID |
|---|---|---|---|
| 1 | `crates/codegen/grow-paths` | `fc27b158090b6deca75ce447049b011c147bae3c` | `ab2136ad1ca0e96df55c79f138da003bb3349b9e` |
| 2 | `crates/codegen/paths` | `6095da73bb4c57368eb818c7cb3b854964173c96` | `7b00dca198d492ce5b4725c2e4e61db09c3a5523` |

### S 完整指纹

| 首次版本 | SHA-256 |
|---|---|
| v1.0.0 | `b711f27086ec38fa532e51e70cbe95e159845ebd27af666fe7e2e92f5399c2dd` |
| v1.1.0 | `eebff2ecca9794f72aa1f768fb0bc314eba594e5daef184b6906d0d2b77ff1d0` |
| v1.1.2 | `e85b2267473e25c7d54db390f71de6158c42074239610e84e5cb12480246aecb` |
| v1.1.4 | `7dee111a3de87a65b0cbe35f074ab82ed1fab8f7b6df23042a73417409653479` |
| v1.1.5 | `65d60b7512eee4be30a690feb14a322346ec2d484a0a0a9242b6daa498257652` |
| v1.1.6 | `b4cea5f4ae572034446bdbd73fc22102b71b4a124471ce211d9c45326bbedf1d` |
| v1.1.8 | `98b8c142fd3c2e09928487835683c6bf3db08ffd52966f84705643ad73f22d66` |
| v1.1.10 | `406bcc6d634c13739e657ed56937005ee4c5c861dbf879c5fd2ff2667aaa69d1` |
| v1.1.11 | `94018de419a1327dd954b880d307925a9cc159f6cf25e7eb6235faa8d8eafb41` |
| v2.0.0 | `0bcbd9b2303661d33ce3375bfeabfc40b115102c10033e90b310295c882188a8` |
| v2.0.1 | `82656ca4edfac286d09731a6b12600e0e198bfb1616c5c65a3cee320b4e6d712` |
| v2.0.2 | `d62b39802db6705fcbc19bbe922d4f66ba4a0ffa0b63a18346b9e8b1dfab7928` |
| v2.0.4 | `7240614f05bf8decf5be260f388fba3c1b055487d19fab4848091d5895cea998` |
| v2.1.0 | `8d45d1aff2664f771fee6e0086cce291febb9e9ed0f60e840726f4c465713a84` |
| v2.1.3 | `590b3271ee7d7926659cec8933f8bcbc8c3daa4bb7afdfcdbc39e5426d9cbea5` |
| v2.1.4 | `785c349c4beabf88acfe5955ebdb8fc79323e3338c3312bca27351a60384232f` |
| v2.1.5 | `39fd2dce07840fcaba5afc6c468f1821746fd25255ec3d26521f060cd381b07c` |
| v2.2.0 | `d9150ddc6451acab994136d22187861b3c922ab4f1c7f75cf2b40db562cce772` |
| v2.3.0 | `0a1976d0f5e28f04de34a238c9d69f371805f7c05173815e25120f106b93030a` |
| v2.4.0 | `9484cb40cdd327c407ae08f4ea3992adc6c91eafe9e85dd8176156ad76af921c` |
| v2.4.1 | `4f6d7b6870f000eb076b4f737f2a520420e968a2574034d475fe2f34ca05b77a` |
| v2.5.0 | `d7bd50c74c4bf2c8418d30aaf629be32cbddf128b097260fc4abf9df4dd564e9` |
| v2.6.0 | `102854a16261bcb33430cc8327ab49ea07f6e4208dc0be13cdbbcbb967b33173` |
