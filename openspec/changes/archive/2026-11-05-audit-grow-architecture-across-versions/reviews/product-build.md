# Product、build、platform、quality 与 vendor 分区审计

本区确认七项当前机制问题：stdio bridge 关闭会话未从恢复缓存移除、并发新建请求的恢复身份串配、ptyctl 生命周期控制未接线、精确测试执行器对独立进程组后代错误确认清理，以及 crash capture 的多进程文件所有权、跨进程符号化和失败归档丢证据。除执行器有实际隔离实验外，结论均标为静态确认，未声称跑通完整产品复现。发布 commit 门禁、更新器取消处理和 native vendor 的多项旧问题已修复，分别列入历史，不能作为当前阻塞。

本报告不改业务实现、规范或 backlog；修复和跨区去重由父审计统一安排。

## 基线、范围与证据强度

- 基线 HEAD：`d6128138cacdc22553aba9dafba9bc6cc934e4d1`；最新 release tag `v2.6.0` 指向 `5475dff997f0…`，二者不是同一 commit。按本区 tracked path+blob OID 计算，scope SHA-256 均以 `1bd80eceacae` 开头，manifest 集合均以 `135ea2059b13` 开头，可以复用本区已提交源码；不能据此说全仓树相同。
- 初始工作树已有 `diagnostics/src/context.rs` 与 `docs/development.md` 等用户改动。前者将 `Repository::discover` 换成不打开仓库配置的 `discover_path`，本区按实际工作树核对且保留；模型/resume、workspace discovery 等活动修改未覆盖、未提交。表中的 Git 指纹不包含未提交内容。
- 逐个解析了 46 个本地 release tag 的树、真实旧路径、manifest 和相邻 `git diff --name-status -M`，再读重要实现补丁、当前入口、消费者与测试。树/hash 证明版本枚举和可复用范围，不证明每版每函数都动态测试通过。
- 读过根 AGENTS、OpenSpec 索引、本 change proposal/design/tasks/inventory、development-workflow、sandbox-boundary、client-surfaces 相关条款，以及 software-engineering 的 Rust、Python、目录和交付原则。报告目录未发现更深 AGENTS。
- 当前 contract 引用以 `openspec/specs` 为准；CLI bridge、ptyctl 和 crash capture 尚无覆盖下述全部场景的独立主规范，结论来自公开接口、实际消费链和必要的一致性不变量，没有虚构已有 SHALL。
- Atlas skill 无法加载；直接 Atlas 调用只确认焦点图中 `probe_target_version_uncached → probe_version_by_exec`。结果带 focus closure / budget_exhausted 限制，未把它当全仓调用图。Context7 未返回有效 backtrace 专题资料；跨进程符号化核对的是 lockfile 对应 `backtrace 0.3.76` 实际缓存源码。
- 临时证据放在 `/tmp/grow-product-build-audit/`，包括 inventory、原始 Git 提取、测试产物和执行器实验。长期可复核的触发、对象指纹、实际输出和通过标准均写在本文，不依赖临时目录长期保留。

## 当前模块、入口和所有者

以下路径均相对仓库根。表内“已核对”表示架构入口、依赖/状态 owner、重要失败分支及测试意图审查；未运行的部分另列限制。

| 模块/入口 | 实际职责、消费者与边界 | 核对结果 |
| --- | --- | --- |
| `cli` | 产品组合根：先分发 Mermaid worker、version/doctor/du；安装 minimal renderer、allocator hooks、终端恢复、可选 crash capture；建立有界 worker runtime；再安装 ring、分发完整 TUI/headless/agent/stdio/serve/leader/update | `main.rs:1298`、`async_main:1366`、`run_agent_command:630`、headless options 与测试均沿调用方核对。更新配置持久化留在 shell/CLI，未引入反向 `update→shell` 依赖。恢复缓存见 PB-01/02 |
| `update` | CLI 与 leader 后台检查、GitHub semver 渠道选择、实际 PATH 安装目标探测、下载/校验/解包、managed 双入口交换或外部目标替换、补全及旧产物回收 | `auto_update.rs` 的目标解析/探测、install、swap/rollback、cleanup 与 I/O/network 测试已核对。正式 archive 路径限制压缩/原生文件各 512 MiB、SHA256SUMS 128 KiB；校验在解包/替换前。临时名带独立身份，更新器是有意允许竞争并收敛的设计，未据“无全局锁”单独报错 |
| `version` 与 `cli/build.rs` | 版本文本、channel 展示；显式完整 RELEASE_COMMIT 校验并取 7 位；本地 Git fallback；normal/linked worktree/ref/packed-refs 的 rebuild watch | 四项独立 Git identity 测试实跑通过。正式构建身份与本地 fallback 区分；只 watch `.git/HEAD` 的旧实现已修复 |
| `diagnostics` | task-local session/prompt 关联、结构事件、统一日志、debug firehose、可选 hooks/memory/sampling 日志、Chrome span timer、TLS provider 初始化 | 当前不保留 remote telemetry exporter。统一日志 64 条队列/64 KiB 单记录/5 MiB 文件、firehose 64 条/64 KiB/32 MiB 与 flush 上限及 inode 锁对应主规范。optional sampling/hooks/memory logger 不是同一条 writer，不能把统一日志全部预算推给它们；默认关闭/feature gate 也已区分 |
| `crash-handler` | Unix fatal signal 保存原始 IP/最小元数据并恢复终端；下次普通启动解析、符号化、写私有文本与最多 10 份历史；Windows capture 部分退化 | 不在 signal handler 分配/做普通 Rust 解析的设计有明确边界；0600 创建和已有文件收紧有测试。多进程与恢复提交边界见 PB-05/06/07；未发真实 fatal signal |
| `tracing-macros` | timed/timestamp 属性为调用方插桩，调用方提供 tracing；不负责日志文件/远端传输 | 全部三个源文件及宏使用边界已读，未发现需新抽象的现有问题；不因宏包小或重复语法建议合并 |
| `tty-utils` | 控制终端隔离、Windows console flags、稳定进程组/Job 句柄、ProcessScope 的 Weak 注册与 close latch、Linux parent-death opt-in、native stderr 与 runtime helpers | `detach`、kill-on-drop、process-group owner 是不同职责。`ProcessScope::kill_all` 明确不 wait 以免与正常 owner 竞争，不能把被说明的 wedged-owner zombie 残余误当完整回收承诺。credential helper 的 Windows suspended-enroll-resume 与普通 test Job 的 best effort 已区分 |
| `ptyctl` | portable-pty + alacritty Terminal、resize/wait/raw-tail、HTTP/WS 控制服务 | CLI/HTTP 实际 stop 终点未控制 child/server，PB-03。Pager harness 只复用 terminal/grid 等构件，另持真实 child/tree owner；不能把 PtySession 的缺陷直接等同于所有 Pager 测试无清理 |
| `ptyctl-cli` | `ptyctl run/send/screen/wait/stop`，loopback 绑定与名称 registry | `--timeout/--linger` 被传进未消费的 SessionConfig；run 先 spawn 再 bind，服务未接 graceful shutdown。registry 的 `--force` 明确只替换注册，不负责停旧 server；未把它误解成进程接管 |
| `pager-pty-harness` | 真实 PTY child、TestProcessTree、退出状态缓存、独立 sandbox/mock、screen/scroll/timing 场景及历史二进制比较 | 当前 `PtyHarness` 先观察退出再清后代/回收，Drop 有 owner；LeaderCluster 持专用 `TestProcess`，普通 Drop/unwind 测试已接 Core exact gate。bench 包括真实剪贴板等入口，本次只读，不运行主机交互 |
| `test-support` | TestSandbox env_clear/私有 root；Tokio TestProcess 和窄 TestProcessTree；ACP/headless/leader clients；mock inference/SSE/UDS fixtures | `process.rs:437` spawn、`:605` WNOWAIT-before-reap、`:788` Drop 与 leader 初始 daemon 句柄已核对。夹具不从 lockfile PID 获得 signal authority，replacement generation 观察不等于拥有。直接运行这些库回归需要工作区 build，本次未跑 |
| `common/test-utils` | 少量 Git、env、runfiles、image、tracing capture helpers | 是测试边界，当前 Bazel feature 无实际 runfiles 依赖。旧 `ensure_hermetic_git_on_path` 仍修改进程环境，和 TestSandbox 的 per-child env 不是同一保证；未据名称推断所有 helper 完全 hermetic |
| `scripts/run_exact_cargo_test.py` | 唯一 Cargo no-run JSON 产物选择，直接 enumerate/execute 同一文件、package cwd、exact name/ignored/one-pass 判据、phase deadline/reader tail | 现有 34 项脚本测试通过（1 跳过），但独立会话后代不在其 PGID 内，PB-04。不能用顺利的普通退出测试证明强杀执行器也能跑 Rust Drop |
| 其他 Python/JS scripts | prompt cache 证据只读比较；local coordination/agent messages 用本地模型和隔离 GROW_HOME；trajectory Playwright 页夹具 | analyze 的 digest/路径/正文比较和脱敏 15 项实跑通过；coordination 的独立 process/EOF/kill timeout 与 model finally 已读；真实产品二进制和浏览器脚本未运行。语法检查通过 |
| `.github/workflows/release.yml` | annotated tag/default branch/workspace version→完整 commit；10 平台 catalog；相同 commit 的 Core/native coordination/Windows storage/OpenSpec gate；全部成功且未 cancelled 才 publish | 发布前两次复核 tag 未移动、archive 内容、attestation、SHA256SUMS；已有 public release 不覆盖，draft 验证后才发布。审查了 YAML 和 mutation/static tests；没有真实 dispatch/publish，也没有验证外部服务当次运行状态 |
| `build-one.yml`、其余 reusable workflows | builder checkout/verify 同一 commit；native/cross/Alma/OHOS 的版本环境传递；strip/sign 后 smoke/archive；平台回归和 pinned OpenSpec CLI | riscv/musl 等跨目标有明确未实跑 smoke 的路径，不能宣称 10 平台都在 runner 上执行原生 grow；native 编译 gate 与 Core 库回归补其有限覆盖 |
| `scripts/build-ohos.sh` | 专用容器内 pinned Rust 1.98/core source closure/SDK，release-dist+sandbox-enforce；独立 smoke 路径 | runner image 允许 latest，不等于 core/toolchain 源未固定。容器外运行不符合前置条件，未执行；脚本会设置该容器 Cargo mirror，未在开发主机执行 |
| 根 `Cargo.toml/.cargo/config.toml/rust-toolchain.toml` | 57 workspace package、target flags、release panic=abort、dist debug=0/thin LTO、patch/feature closure、Rust 1.93.1 本地/主 CI | `sandbox-enforce` 显式进入发行构建；Windows 启用 feature 不表示有 Unix kernel sandbox，和 sandbox-boundary 保持一致。reqwest rustls-no-provider 与入口 ring 安装/测试 ctor 对应，OHOS 不引入 aws-lc |
| 根 README/config.example/ROADMAP/docs | 产品、构建、调试入口和当前 OpenSpec 导航 | BYOK/本地诊断、TUI/headless/ACP、下载校验、独立构建/主规范链接已核对；示例显式 `auto_update=false` 不等同未配置默认值。vendor 删除条件说明有局部过时，见文档债务 |
| `third_party/dagre_rust`、`mermaid-to-svg` | 两个 workspace member；consumer Mermaid→纯离线布局/SVG→受限 raster，实际 pager worker 在独立进程隔离 panic/超时 | 读取 maintained diff、版本头、消费说明和相关测试入口，没有逐行复审上游布局算法。dagre 原 static mut ID 改 AtomicUsize；Mermaid 保留 CJK/wrap/sequence/XY/flowchart 本地实现，group 展开先限 4096 |
| `third_party/nono` | sandbox 的 Unix exact 0.53.0 path patch；riscv syscall、musl ioctl 类型；裁掉未使用的 sigstore/crypto 链 | 头部说明和 sandbox manifest/实际 linux 代码核对；它不是 workspace member，默认不作为自有 crate 全目标 lint。当前 Rust pin 约束和 remove/upgrade 条件有依据；未重新实跑 Linux/macOS kernel enforcement |
| `third_party/nix-ohos` | 仅满足 pprof 的 nix ^0.26，workspace 当前 nix ^0.31 仍用 registry | OHOS-as-musl cfg、ABI lint、TimeVal i64 微秒计算维护补丁已核对；未误将它描述为全仓统一 nix。FreeBSD/OHOS ABI 未在该主机重编译 |
| `third_party/sqlite-vec` | memory 的 exact native extension；补齐 upstream 包遗漏的 C includes，并保留边界/析构/错误所有权修复 | 真实 SQLite C harness 普通与 ASan/UBSan 均通过。`memory::init_sqlite_vec` 的一次性 auto_extension/typed transmute 和 schema consumer 已核对；不是穷尽所有向量算法/SQL/架构 |
| `markdown/fuzz` | 独立 workspace、valid UTF-8 输入，pretty/nonpretty × full/streaming 四次调用，chunk 1/16/32，Syntect disabled | 当前 README 准确陈述 panic-only 路径。没有 finish、结果相等 oracle、ANSI 或语法高亮覆盖；这是可解释的覆盖上限，不从“缺少 fuzz”推出运行缺陷。未运行 fuzz campaign |
| 根 `bin/`、`cp/` | 当前没有 tracked 产品构建入口；旧 bin/protoc 已退出构建 | 审计时目录只读；可能的用户临时内容均未删除、覆盖或执行 |

## 当前发现

### PB-01：stdio bridge 实际入口漏掉会话关闭，重连会重新加载已关闭会话（中，静态确认）

- 位置与触发：[cli/src/main.rs](../../../../../crates/codegen/cli/src/main.rs) `forward_stdio_line_to_leader:589` 仅在行包含精确 `"initialize"`、`"session/load"` 或 `"session/new"` 时调用缓存函数（`:600`）。正常 `_grow/session/close` / `grow/session/close` 行不满足该条件，却正常发给 leader。缓存 helper 的 close 分支在 `:352`，因此经过实际入口时没有执行。
- 后果：先 new/load `s1`，再关闭 `s1`，随后 leader 断连重选；`replay_acp_state_after_reconnect:524` 仍对缓存中的 s1 发送 session/load。关闭的 runtime/session 可被重新打开，`last_session_id` 也可继续指向它。已落盘 Timeline 本身并未被证明损坏，问题是 bridge 的在线恢复集合不等于真实生命周期集合。
- 不变量：恢复只能重建仍需保持的 client session；close 的成功业务动作必须进入恢复 owner 的状态。`tests::cache_session_close_stops_replaying_it:2369` 直接调用 helper，绕过真实 forward 条件，不能证明生产接线正确。
- 历史：`095ab550c` 已有相同入口筛选及 close helper；v1.0.0 旧 `grow-pager-bin` 代码已确认，迁移后至全部 46 tag/HEAD 未修复该机制。v2.0.0 移除 snake_case 兼容没有修这个问题。
- 所有者/修复边界：CLI stdio bridge 缓存接纳。通过标准是用真实 forward 入口顺序发送 initialize、new/load、close，再断线重放，leader 观察不到已关闭 ID 的 load；未关闭的其他 ID 仍能恢复。无需新增第二套 session 生命周期管理器。

### PB-02：新建会话只保留单个 pending，不按 JSON-RPC id 关联结果（中，静态确认）

- 位置：[cli/src/main.rs](../../../../../crates/codegen/cli/src/main.rs) `StdioReplayState::pending_new:285` 是一个 Option；`:341` 每个 session/new 覆盖它；`cache_incoming_session_id:365` 对任何含 result.sessionId 的响应直接 take，没有核对请求 id，也不处理 new 的 error 清理。
- 可达序列：同一 bridge 客户端先发 new(id=10,cwd=A,MCP=A)，在响应前发 new(id=11,cwd=B,MCP=B)。第一个成功响应返回 sA 时，sA 被绑定 B 的缓存参数；第二个响应 sB 时已无 pending，sB 不进入 sessions。即便 leader 顺序处理，客户端 stdin/stdout 是不同任务（`:817` / `:836`），允许两个请求先后进入，不能用 JSON-RPC 请求处理次序替代 request-id 关联。另一个 load 成功响应也可能错误消费 pending new。
- 后果/不变量：下一次 reconnect 可对 sA 使用错误 cwd/MCP，且漏恢复 sB；一次失败 new 的残留还能污染后续 sessionId 响应。这里未证明权限绕过，准确问题是请求参数和创建结果失去同一请求身份。
- 测试/历史：现有 `cache_session_new_is_pending_until_response_assigns_id:2341` 只覆盖一个 outstanding new；该机制 `095ab550c` 引入并存在于 v1.0.0 至 HEAD。
- 所有者：CLI replay state。通过标准：两个/多个 new 的顺序与逆序响应、new error 与 load response 交织，都保持 ID→cwd/MCP 对应；只有对应请求被结算，所有已确认且未关闭 session 恢复一次。实际产品多请求/leader crash 测试未运行。

### PB-03：ptyctl stop/timeout/linger 未连接 child 与 server 生命周期（中，静态确认）

- 位置：[ptyctl/src/session.rs](../../../../../crates/codegen/ptyctl/src/session.rs) `start:97` 创建 `_shutdown_rx` 后从未消费，返回即丢弃；`stop:320` 发送失败被忽略，始终 Ok。`config.timeout/linger` 仅定义/传入而未消费。[server.rs](../../../../../crates/codegen/ptyctl/src/server.rs) `handle_stop:262` 继续忽略结果并回复 `{ok:true}`。[ptyctl-cli run.rs](../../../../../crates/codegen/ptyctl-cli/src/commands/run.rs) `:69` spawn，`:77` bind，`:107` 无 shutdown future 的 axum::serve。
- 可达条件：`ptyctl run --timeout 1 -- <长命令>` 不会因该 timeout 停止；`ptyctl stop` 的 HTTP 成功也不终止 command/server。默认非 linger 的自然退出仍留下服务。若 bind/registry 失败发生在 spawn 后，没有一个已接线 supervisor 完成统一取消。子进程 kill API 存在于 PtyChild，但该路径不调用它。
- 相关一致性：reader 在 `session.rs:128` 将 alive=false；child waiter `:181` 看到这个位就退出，不再确认/记录 exit code。WS 仅在 broadcast Closed 时发送约定 closed message（`server.rs:333`），而 PtySession 保留 output_tx 强 sender，正常服务持有状态时 channel 不会因 child exit 关闭。这些均反映“输出结束”“子进程结算”“服务关闭”未有同一个 owner；未把所有平台 zombie 后果都当已复现事实。
- 现有覆盖为何不足：HTTP wait 回归验证了锁不阻塞 send/screen，收尾通过给 shell 发 `exit`，不验证 stop/timeout/linger。Pager harness 自己拥有 child/tree，不使用这套 PtySession 作为唯一 owner，因此本项只定位 ptyctl/session/CLI 服务。
- 历史：session.rs blob `8c144f7fe96e79e653eee6a6a45b5dad44d70a03`、CLI run.rs blob `a123cd12c436e6403f9487e6e573615956924ec6` 在全部 tag+HEAD 完全一致；最早可见引入为 `c68e39f60`。term/styled 的后续变更未修生命周期。
- 所有者与通过标准：ptyctl session/server 的生命周期 owner，加 CLI graceful shutdown 接线。真实 CLI 夹具分别覆盖 stop、timeout、自然退出、linger、bind/注册失败，证明 owned child 已结算、服务按语义关闭或保留、exit_code/WS terminal 一致；不从观察到的任意 PID 扩张清理权限。

### PB-04：精确测试执行器将同 PGID 清理误当完整后代清理（中，机制实跑确认）

- 位置：[scripts/run_exact_cargo_test.py](../../../../../scripts/run_exact_cargo_test.py) `_terminate_process_tree:69` 在 Unix 只向 `process.pid` 对应组发 TERM/KILL 并 wait 直接子进程；`_run_phase:310` 以这个返回值、直接 child exit、输出 reader 结束共同判 cleanup confirmed。独立 session/group 且不持继承 pipe 的后代不在这几项观测中。
- 实际实验：直接 import 真实 runner，在 `/tmp` 调 `_run_phase([python,-c,launcher,pid_file], 'execute', 0.5)`。launcher 启动 `start_new_session=True`、stdin/stdout/stderr=DEVNULL 的 30 秒后代，然后等待。结果为 `Cargo execute timed out after 0.5s`，不含 cleanup-unconfirmed；同时 `ps` 观察到后代 `PPID=1`、PGID=自己的 PID、状态 `Ss`。实验 finally 精确 SIGKILL 自建进程组；随后确认该 PID 不存在。没有触碰用户或产品进程。
- 真实消费链：Core exact 运行 `leader::ownership_tests::leader_cluster_reaps_dedicated_daemon_on_drop_and_unwind`；LeaderCluster `leader.rs:55`→`TestProcess::spawn(process.rs:437)`→`detach_command(:460)`→Unix setsid。该 daemon 可进入独立组。正常 Drop/unwind 回归能成功不代表测试进程被 TERM/KILL 时 Drop 能运行。库 TestProcess 的正常清理逻辑本身不是本实验判错的对象。
- 契约：development-workflow 的 Exact Cargo regression runners expose bounded phases 要求清理被确认或明确报告 unconfirmed。当前实验不是“未尽力终止”争论，而是不能由已有证据确认完整 tree 却省略 unconfirmed；取消分支还会打印 `process tree termination confirmed`。
- 历史：v2.4.1 的初版 runner 还没有 phase deadline/tree cleanup；`02432cb64` 首次在 v2.4.2 加入当前清理判据，`ecb0f9cc6` 同版修 pinned executable，没有修独立组问题，延续至 HEAD。
- 所有者/通过标准：精确执行器及其拥有的 fixture 进程容器。用 ordinary descendant、setsid+持 pipe、setsid+DEVNULL 三组对照，超时和取消均不得在后代仍活跃时声称 tree confirmed；已证明拥有的后代须被回收，做不到则显式 unconfirmed。不要用全局进程名匹配或陈旧 lock PID 代替所有权。

### PB-05：crash capture 共享单个 blob，仍活跃进程可向已 unlink 的 inode 写证据（中，静态确认，显式启用时）

- 位置：[cli main.rs](../../../../../crates/codegen/cli/src/main.rs) `:1323` gate 成功后，各种 Grow 进程都用同一 GROW_HOME/crash；[handler.rs](../../../../../crates/codegen/crash-handler/src/handler.rs) `install:453` 对固定 last-crash.bin 以 O_CREAT|O_TRUNC 打开并长期保留 fd；[lib.rs](../../../../../crates/codegen/crash-handler/src/lib.rs) `check_previous_crash:122` 处理后 unlink `:138`。
- 可达序列：同一用户 A/B 两个开启 capture 的 Grow 已打开同一 inode；A 崩溃写 blob；C 启动读取 A 的 blob、unlink 并 install 新 inode；B 后续崩溃仍写旧 fd，内容已没有可供下次启动发现的路径。两个同时崩溃/一个启动 truncate 另一个正在写也可覆盖数据。多终端和 leader/client 是产品支持场景，不是人为假定多租户攻击。
- 受损不变量：某进程拥有的待恢复 crash evidence 不能被另一个 live 进程的启动回收操作撤销。每个进程内部先 check 再 install 只解决单进程序列；0600、MAX_FRAMES 和历史数量不解决跨进程 inode 所有权。
- 边界/历史：默认 gate 是 false（`shell/util/config/resolve/crash_handler.rs:10`），GROW_CRASH_HANDLER 或显式 `[diagnostics].crash_handler=true` 开启；config.example 显式打开。不把它报成默认所有启动都会丢 crash。v1.0.0 旧路径已存在此机制；v1.1.0 重命名后 crash-handler 整树至 HEAD 未变。
- 所有者：crash-handler 的文件身份与恢复 claim。通过标准：真实隔离双进程/三进程序列让每个异常退出都留下独立、可识别、只被一次恢复消费的记录；并发启动不 truncate live owner 的 pending 文件。无需扩大为诊断服务框架。

### PB-06：保存旧进程绝对 IP，却用新进程加载地址解析，ASLR 后不能可靠符号化（中，静态确认）

- 位置：[format.rs](../../../../../crates/codegen/crash-handler/src/format.rs) CrashBlob v1 只保存绝对 frames、app_version 等，没有 module identity/load bias；[symbolicate.rs](../../../../../crates/codegen/crash-handler/src/symbolicate.rs) `resolve_frames:25` 在下次启动对旧 IP 调 `backtrace::resolve:36`。已核对 backtrace 0.3.76 的 gimli Cache::new 使用 native_libraries，avma_to_svma 用当前加载区间和 bias 反解，而非上个进程的地址空间。
- 条件/后果：同版本 PIE 程序重启后 slide 改变，旧 IP 通常不在新进程对应映射内，得到 unknown；若恰好命中其他当前映射也不能把它当原 crash 的可信符号。版本升级还改变符号布局。保留 raw IP 有诊断价值，但不能声称只需保留当前 binary symbols 就能恢复历史函数/file/line。
- 未验证/历史：未发真实 fatal signal，也未在 Linux、Windows/stripped 全平台复现；非 PIE 或偶然相同 slide 的运行可能成功，这不使一般恢复语义正确。format blob `50f259784b95aa9b903b5b3d1274e81feb50af4d` 全部 tag 相同，symbolicate v1.0→v1.1 的差异为名称/平台文字等，未新增旧地址空间身份。
- 所有者与通过标准：crash format/symbolication owner。隔离可调试 PIE 子进程在已知函数触发 crash，在改变 load base 的新进程恢复，必须按记录的 executable/module identity 和相对地址得到该函数，或明确标记无法符号化且保留离线所需证据；不能拿已失效绝对地址套用当前进程符号。

### PB-07：文本/历史保存失败仍删唯一 raw blob，并报告成功路径（中，静态确认，启用 capture 时）

- 位置：[crash-handler/src/lib.rs](../../../../../crates/codegen/crash-handler/src/lib.rs) `check_previous_crash:132` 忽略 write_owner_only 错误，`:135` archive_report 本身也丢弃 mkdir/write 错误；`:138` 随后删除 raw，仍返回 Some(report_path)。CLI 因此输出一个未成功创建的报告路径。
- 触发与不变量：raw 可读、crash root 可删除文件，但 last-crash-report.txt 被目录占用且 history 也不能写（或写时磁盘满/权限变化）。两份文本都失败仍删除唯一原证据。成功读取/解析不是成功归档；0600 只限定泄露面，不保证发布完成。
- 历史：v1.0.0 起即有相同顺序，未见修复。现有测试只验证空目录/报告格式和 Unix mode，不验证“失败必须保留原证据”。未做磁盘故障注入，因此不宣称 full API 实跑。
- 所有者/通过标准：同一 crash 恢复发布边界；注入报告路径/历史路径不可写及部分写失败，失败返回可观察且 raw 保留可重试；至少一个承诺的 durable report 成功提交之后才能消费 raw，不能返回不存在路径作为成功。

## 历史已修复、已收敛的边界

| 首个发布版本 | 核对的旧行为→新行为与 Git 证据 | 当前判断 |
| --- | --- | --- |
| v1.1.0 | crate 前缀/边界整理；`72f05caf7` crash-handler 的 Windows 版本输入改用明确 app_version；移除不需的 layout mirrors；sqlite-vec 补齐 upstream 同 tag 四个 C 文件，nono riscv/musl patch 纳入实际 graph | 不是当前 Windows 编译错误，也不能因当前旧路径缺失漏审 v1.0.0 |
| v1.1.1 | `f58ff2cc8`/OHOS 移植：nix0.26 cfg patch、nono 移除无消费者 sigstore 及 aws-lc 链；distro-pm feature 让 package manager 拥有升级 | 保留有目标依据的窄 patch，不建议无条件删除 vendor 或给 OHOS 套普通 Linux 假设 |
| v1.1.4 | `11bc581f7` 合并 reqwest 0.13/rustls-no-provider、ring provider 入口与测试 ctor，async-openai fork 固定 revision；`371e54935` 拆 reusable build-one，后续 heredoc JSON 修正 | 当前入口先装 provider；测试绕过 main 的路径也有安装，不沿用旧 TLS provider panic 结论 |
| v1.1.7 | `f58e82ecf` release-dist debug 从 1→0，避免产生随后剥离的 debug/PDB；签名、strip 在 archive 前 | 这是明确发行取舍；crash 解析能力不能假定发行包自带全部调试信息 |
| v1.1.8 | tty-utils 加 Windows credential-bearing helper 的 CREATE_SUSPENDED→Job attach→resume | 当前受限 helper 可在执行用户代码前取得 Job 所有权；不能把旧 post-spawn race 普遍套给此链 |
| v1.1.9 | `f21c82632` updater 不再盲装 GROW_HOME：PATH/PATHEXT 选实际目标，验证 grow version，managed/symlink/regular 分策略落地；同目标用于磁盘版本判断和 restart | 旧“更新到了旁边、用户执行仍旧版”已修；外部目标不维护 managed agent links |
| v2.0.0 | updater 增 SHA256SUMS 严格解析与边界、archive 流量限制、byte-range 长度检查、Windows same-version 发布 fallback；CLI 移除 legacy permission/capability 兼容，restart-for-update 执行新 binary | checksum 保障 transport/release asset 一致性，不把它夸为独立于 GitHub 的离线信任根；底层 download_silent 无 cap 的通用 seam 不等于正式 installer 无 cap |
| v2.0.4 | CLI/update 删除 managed deployment refresh/setup 旧服务接线，保留本地 BYOK/用户配置权威 | 当前没有必须依赖上游部署服务才能启动的结论 |
| v2.1.3 | test-support SSE fixture 区分 response item id/call_id，先 output_item.added 后 arguments delta；ACP capture Grow notifications | fixture 更贴近实际 streaming 协议，不能据旧 mock 误报生产事件序 |
| v2.1.5 | `175b08368` milestone：probe/smoke kill_on_drop；补全每 shell 10 秒、取消终止直接 child，失败/空输出保留原文件；清理保留完整当前/最高其他版本组，未来/未知 mtime 保留；trace 不先强制构造失效模型 config | 当前符合 client-surfaces 直接 child/清理契约，不要求它终止任意后代树；“显式版本 pin 成功后才关闭 auto_update”仍由调用方 future 提交 |
| v2.1.7 | update 从 shell 解耦至 config，CLI 注入 resolved auto_update、channel switch 意图及设置更新 future | 依赖方向更窄，有真实配置 owner；没有为“纯度”引入 provider/factory 框架 |
| v2.2.0 | diagnostic debug firehose 从旧 appender 收敛；OHOS core source closure/toolchain pinned；Mermaid flowchart 本地 port/4096 cap；fuzz 文档修正为实际四个 Syntect-disabled 路径 | 当前需要按各日志 writer 的真实 budget 验证，不能继承旧“所有日志无界同步”笼统结论 |
| v2.4.1 | `404aaa799` release Core/native/Windows/OpenSpec 都接 validated full commit、publish DAG 显式成功及 !cancelled；首次 exact test 名称+执行数判据 | latest-main 独立绿灯不能替代发布 commit 的历史问题已修；旧 runner 没有后续 phase/产物绑定能力 |
| v2.4.2 | `99556e83c` RELEASE_COMMIT 穿容器/构建身份且 malformed fail；nix TimeVal 微秒 i64；sqlite missing-IN/unsupported dispatch/primary-key length修复。`02432cb64` native metadata name destructor、phase deadline/tail；`ecb0f9cc6` enumerate/execute 固定同一个 Cargo JSON test executable | 这些修复及对应现有脚本/C harness 已核对。runner 的独立后代问题仍见 PB-04，不能把已修 part 当整个 tree 问题已完 |
| v2.5.2 | `1c88fccb4` local Git/worktree/ref rebuild watches；Chrome timer EnteredSpan 及时 drop；LeaderCluster 从隐式 daemon 观察改为专用 TestProcess owner | 当前正常 Drop/unwind 有可验证所有权，不从 pid/socket 观察扩张权限；强杀 test 不执行 Drop 另归 PB-04 |
| v2.5.3 | `fce2f3d59` unified log overflow accounting 同步测试/worker 屏障 | 不把之前 racing 测试统计误当当前 producer 无界或损失计数仍必错 |
| v2.5.5 | `599873595` sqlite float parser 先初始化错误所有权，再进入 allocation-failure；自动更新 None 默认开启、显式 false 关闭；新增 CLI 层真实 PTY notice 回归，allocator unprefixed feature 按平台限定 | C allocation fault regression 及 ASan/UBSan 实跑通过；UI PTY notice 只读未运行，未宣称已在本机验证显示 |
| v2.6.0 | ACP fixture 加 typed content/image prompt 与 timeout；Core 接更多生命周期/诊断/PTy 回归；root README 同步产品能力 | 本区 tracked tree 内容与 HEAD 相同，但当前用户活动区仍需父审计最终复核 |

## fork 导入与已移除模块

`upstream/main=bc7f02edd…` 是 2026-08-28 的 monorepo sync ref，不是 Grow release。它与 main 的 merge-base 为 `02d9359435d0e9c20a20945679389cdce441e431`；父 inventory 核对 main-only 678/ref-only 27。本区有界检查 root tree、manifests、merge-base→ref 的相邻 diff，以及 fork 当前消费边界，没有把上游 27 提交的每行当作本产品审计承诺。

| ref/资产 | 树与 manifest 证据 | 去向/当前范围 |
| --- | --- | --- |
| upstream/main | root tree `1f9266ee49f4f1d82450b45f10f68a7dd58b23ef`，Cargo.toml blob `1df2e8666d1d860529adbea2f37916bf21e91de1`；97 members、99 manifests | 仍有 telemetry、workspace client/daemon、voice、bundle、secrets、diag-server、mixpanel、computer-hub、proto、xai-tracing。不能把这些上游能力按旧 package 名当当前 Grow 在线入口 |
| v1.0.0→v1.1.0 | 69→57 workspace members；当前仍 57；根 manifest blob 从 `a954bee7b85457596ddf08d84759ae8ff86bc2c1` 到 HEAD `c6832d1721626a29d2462ecf2174df8ca35bfb19` | 大多数 grow-/xai- 名字是边界迁移，manifest 并集不等于当前 package 数；旧 path 按 tree 追踪 |
| grow-pager-bin | v1.0.0 tree `d2284cfa1a14128ca685628f9fa179a2a380ca96` | v1.1.0→cli；组合根职责保留，build.rs 当次 R100，后续 build identity 单独演进 |
| xai-proto-build + bin/protoc | v1.0.0 tree `f0a853bd34a6b9ef249f51d21a81690928b8e437`；真实 consumer `grow-tools-api/build.rs` 调 XaiProtoBuilder，支持 PROTOC/bin wrapper/PATH 查找及 proto rerun dependencies | 随 tools-api 归并/去 protobuf，在 v1.1.0 前移除；当前不需要 protoc/dotslash。上游后续 proto debug-redact 扩展不重新进入 Grow build |
| xai-system-power | v1.0.0 tree `cbbd982a7a9ecd617a12dcbdcb97c72e1c8de9a4`；consumer 是旧 AuthManager sleep_gate，处理 OIDC refresh 在 sleep/dark-wake 的旋转 token 风险 | `0e9c46037` 随 BYOK-only 去旧认证栈而删除，首次不含为 v1.1.0。当前 pager::notifications::SleepInhibitor 防 idle sleep，是另一职责，不能说旧通知库被合并进去 |
| prod/mc/cli-chat-proxy-types | v1.0.0 tree `ccb5988574587038fa9edc8fe6dbd10b07b1f12c`，含 deployment policy、服务反馈/会话/storage DTO | `ab1c69909` 删除服务 DTO；deployment_config_types R094→config/signed_policy/types.rs，保留本地 signed-policy 类型权威。不是所有旧 prod 行为都保留 |
| graphlib_rust | v1.0.0 tree `3b3e2311798aa3c52b7f271a504fe4eaa4745229` | `618f4c7c1` 移除无语义 patch 的镜像，dagre/mermaid 使用精确 registry `=0.0.2`；不是删了图能力 |
| ordered_hashmap | v1.0.0 tree `659f3dd60dd325a48a0702cf1c8635e747ce6f6b` | 同提交改为精确 registry `=0.0.3`；本地 mirror 无维护必要。许可证/NOTICE 入口保留 |
| nono/nix/sqlite vendors | 不是 workspace member；通过 `[patch.crates-io]` 各满足具体依赖版本；dagre/mermaid 是 member | 自有包、维护 vendor patch、未变 upstream 算法分别处理，不据默认 workspace 测试未 lint vendor 推出所有 patch 无验证 |

## 全版本覆盖与可复用对象

下表 `S` 是 scoped tracked files 的 `path<TAB>blob-OID<LF>` 排序后 SHA-256；`M` 是其中 manifest 的相同算法。scope 含本区 crate 及历史 aliases、三个 patch vendors、两个布局 vendors、已移除模块、fuzz、scripts、.github/.cargo、根构建/产品入口及 docs。SHA 仅展示前 12 位；raw JSON 保留完整值，`git ls-tree -r <tag>` 可重算。相邻路径数是过滤后的 name-status 项数，rename 两端已纳入；不代表代码行数或缺陷数。

| Tag | root tree | S | M / manifests | 相邻项数；本区差异判断 |
| --- | --- | --- | --- | --- |
| v1.0.0 | `64abe4ad2989` | `fb0214af9106` | `5880cf9d36ff` /21 | 初始；旧模块与三类当前缺陷起点已核对 |
| v1.1.0 | `a699a001031e` | `0c432a9c7537` | `6d12d6ea1ad2` /18 | 231；重命名、去旧认证/proto/prod、vendors |
| v1.1.1 | `872b1b2ad676` | `042643ee6866` | `bf690fdf32bf` /19 | 78；OHOS/nix、nono crypto、distro-pm |
| v1.1.2 | `27a383704ddc` | `7e15648cd834` | `44f629e1d2c3` /19 | 11；OHOS/release 平台映射及说明 |
| v1.1.3 | `47a93d82df3b` | `60d0d3484fa3` | `2efa366948ec` /19 | 5；根版本/lock/示例/发布工具，crate 实现复用 |
| v1.1.4 | `eaa643213bdc` | `f46976c0ab47` | `bb5a4382aac5` /19 | 31；统一TLS/deps、build-one、test provider |
| v1.1.5 | `02dd5458500b` | `b7c7e49bb42f` | `8cab7c98974e` /19 | 15；文档解析依赖/诊断事件/PTY 场景 |
| v1.1.6 | `c1cb4b01a566` | `14f83ecaaf87` | `6acbe5fde1fe` /19 | 6；发布构建/act工具，crate 实现复用 |
| v1.1.7 | `54195d811e95` | `18f314a035ea` | `5684137a5209` /19 | 11；dist debug=0、OHOS/签名/CI |
| v1.1.8 | `d5553899e823` | `185baa2862d4` | `0e687fac6886` /19 | 12；Windows suspended Job、诊断事件 |
| v1.1.9 | `2417327b11dc` | `1e57ea37fbbd` | `7fde871cc234` /19 | 9；updater实际目标与验证/landing |
| v1.1.10 | `ac5b66c7fd13` | `4a5619c0ee4b` | `cdaf7ebfac40` /19 | 6；du 提前分发、根版本/示例 |
| v1.1.11 | `106ead71704e` | `c92e0d331153` | `4d3417e9a65d` /19 | 3；根版本/lock，核心实现复用 |
| v2.0.0 | `4851b89291cc` | `69f9a0cc0ee5` | `ac3e06e2a4a9` /19 | 45；CLI authority收敛、checksum/资源边界/Windows落地 |
| v2.0.1 | `4cc3ba5e754a` | `d9c6aefb6cd4` | `e853a17caa87` /19 | 10；Windows storage gate、CLI options/发布说明 |
| v2.0.2 | `9ace7d994e02` | `65475039caa4` | `09671fe977ba` /19 | 6；release/validator/docs，crate 实现复用 |
| v2.0.3 | `aa8911ddac17` | `e02dd700f1d1` | `e7d2e70cee31` /19 | 5；release/config/版本，crate 实现复用 |
| v2.0.4 | `f6d4352cd226` | `263455c1dc09` | `85dbf5136ecb` /19 | 12；managed config旧服务移除、sandbox fixture |
| v2.1.0 | `c460a129da90` | `1e4a4ccc8b23` | `420cc69b4e51` /19 | 18；ACP2/渲染和git依赖、fixture协议 |
| v2.1.1 | `e161417d004b` | `91aade432354` | `618b50277b23` /19 | 3；根版本/lock，crate 实现复用 |
| v2.1.2 | `108ce26ba2ca` | `396b1b127373` | `477a67c96dc7` /19 | 11；native coordination workflow/script |
| v2.1.3 | `073647a57b7b` | `f633cb87ae9c` | `565b622d6f14` /19 | 5；SSE item身份及ACP通知捕获 |
| v2.1.4 | `a8ce09224c8e` | `528a67d2ed3f` | `0a5522ee0f98` /19 | 7；Core workflow入口、根版本/docs |
| v2.1.5 | `b544655949cd` | `d3f4223952f0` | `f8f8ba8691cb` /19 | 28；更新取消/补全/保留、trace、日志里程碑 |
| v2.1.6 | `8111229be28b` | `7229cff4e1b6` | `eda1a8389cb0` /19 | 15；append-only sampling能力、Windows cfg/workflows |
| v2.1.7 | `aadcea285ff9` | `cc20eb0218e1` | `46e6f6b2eb1d` /19 | 13；update配置解耦和注入 |
| v2.1.8 | `683f97e7e0ee` | `ec7c1801c055` | `d24aecd5c19f` /19 | 5；build workflow/根版本，crate 复用 |
| v2.1.9 | `80d1aeb23799` | `88a798adf2fc` | `72b9e64a5ab1` /19 | 3；根版本/lock，crate 复用 |
| v2.1.10 | `5ddc5a0ddbb5` | `ae9d0b928699` | `46aab184b71a` /19 | 7；本区根版本/lock/docs，crate 复用 |
| v2.1.11 | `95886fa3e242` | `bd2c39706d81` | `dcc2449afafe` /19 | 3；根版本/lock，crate 复用 |
| v2.2.0 | `15b28a2578ca` | `d76fe000b69c` | `b963e76dbcfc` /19 | 37；firehose/mermaid/fuzz/OHOS closure/测试脚本 |
| v2.2.1 | `e7003a9bbe3e` | `8b1a51ca3c6c` | `756567367270` /19 | 7；build workflow/根版本/docs，crate 复用 |
| v2.2.2 | `635596667f0e` | `834f149729f9` | `312caa5682c3` /19 | 5；只读prompt cache分析及测试 |
| v2.3.0 | `84a87832519f` | `9bfe0bb712b9` | `bff61753bb2b` /19 | 6；CLI Replay分发及根版本/docs |
| v2.3.1 | `f6699de8481d` | `24a2654e9d2d` | `e1c4a3c1dd0e` /19 | 3；根版本/lock，crate 复用 |
| v2.4.0 | `eaea6a806caa` | `13135e78825a` | `b4d4c1ae818f` /19 | 14；coordination与PTY计划场景/说明 |
| v2.4.1 | `daee523b72f8` | `60e1b03772a8` | `2310592e5e6f` /19 | 17；release commit门禁、exact runner、Windows/日志修复 |
| v2.4.2 | `aa0ce2f2be4c` | `24db957a9d90` | `f946350357c4` /19 | 19；build identity、native patch、runner deadlines/产物 |
| v2.4.3 | `95d4b572877e` | `80fdb0ce2917` | `2a541331e78a` /19 | 4；Core gate/根版本，crate 复用 |
| v2.5.0 | `1f747c57d259` | `e560298d6d17` | `2ec827162f7d` /19 | 16；日志预算、harness leader session updates、term/styled |
| v2.5.1 | `46fbfe0a2465` | `60464f818661` | `f105cd9dd8cd` /19 | 5；历史release resume比较测试及依赖 |
| v2.5.2 | `5395a3030249` | `bf4f363aa014` | `24d37f21b20e` /19 | 8；Git watches/Chrome timer/专有PTY daemon |
| v2.5.3 | `b1fa302b9e82` | `446f245dedd7` | `7ae8fc3a0157` /19 | 3；unified日志overflow测试同步 |
| v2.5.4 | `85fc3bfd37cd` | `e670703b6d02` | `63ec3d26eeba` /19 | 2；根版本/lock，crate 复用 |
| v2.5.5 | `698eb211e59b` | `8d95362755ea` | `0721fa614b71` /19 | 16；auto_update默认/提示、float错误所有权、allocator |
| v2.6.0 | `8c5647db2c14` | `1bd80eceacae` | `135ea2059b13` /19 | 11；ACP typed content与Core/native gates、README |
| HEAD | `91c8a6a05a58` | `1bd80eceacae` | `135ea2059b13` /19 | 0；本区 tracked 内容与 v2.6.0 完全相同 |

### 模块树复用组

同一行每项的 `版本范围:tree前12` 是整 crate tree 的精确复用组；未列区间不是随意抽样。v1.0.0 的旧路径见前文迁移表；其余均用当前路径。实际 tree 包含 manifest，所以版本变化也可能形成新组。全范围 `..HEAD` 均指依次列出的所有 release tag 加审计 HEAD。

| 模块 | 整树相同的版本区间与 OID |
| --- | --- |
| cli | 1.0.0:`d2284cfa1a14`; 1.1.0:`23495ecbaea9`; 1.1.1–1.1.3:`924ad4901fff`; 1.1.4–1.1.9:`ed0ec561beaa`; 1.1.10–1.1.11:`8c9908141ca7`; 2.0.0:`faf38699681e`; 2.0.1–2.0.3:`90dc1f93c816`; 2.0.4–2.1.4:`de2e10659391`; 2.1.5:`d6c2c461a584`; 2.1.6:`33a017f2b92a`; 2.1.7–2.2.2:`66a8dc68912b`; 2.3.0–2.4.1:`5397b715fc45`; 2.4.2–2.5.1:`4c0a29a5b1a6`; 2.5.2–2.5.4:`503b09af3888`; 2.5.5–HEAD:`e42494f734c7` |
| update | 1.0.0:`457c4f09503b`; 1.1.0:`51c39a1e52f3`; 1.1.1:`d64b9da62f49`; 1.1.2–1.1.3:`3ff2dd656480`; 1.1.4–1.1.8:`83cb5ae7a64f`; 1.1.9–1.1.11:`7b80f25dded2`; 2.0.0–2.0.3:`8574e1d003fa`; 2.0.4–2.1.4:`d2bc2a90dc49`; 2.1.5–2.1.6:`10b5b520f419`; 2.1.7–2.4.3:`8566d82eaea0`; 2.5.0–2.5.4:`626b0e028ec2`; 2.5.5–HEAD:`a968b25d41a7` |
| version | 1.0.0:`2994c32fc925`; 1.1.0–2.5.1:`0c9621e428f6`; 2.5.2–HEAD:`d73e70d1ab96` |
| diagnostics | 1.0.0:`bffd7e49e839`; 1.1.0–1.1.3:`9ebfb77e00b5`; 1.1.4:`0bbb71b380c0`; 1.1.5–1.1.7:`9840992d742e`; 1.1.8–1.1.11:`a2cb68a5b2a7`; 2.0.0–2.0.3:`6b159b5fa3b8`; 2.0.4:`48c286779910`; 2.1.0–2.1.4:`70447b9f3c19`; 2.1.5–2.1.11:`2c5f7df9348d`; 2.2.0–2.4.0:`51b7eb18a9b6`; 2.4.1–2.4.3:`00b2455d6f32`; 2.5.0–2.5.1:`0657e211316a`; 2.5.2:`2fb456b0728a`; 2.5.3–HEAD:`6c37e95e2fb7` |
| crash-handler | 1.0.0:`8855a84a5f2b`; 1.1.0–HEAD:`015dbf20e589` |
| tracing-macros | 1.0.0:`b6ceebc9a363`; 1.1.0–HEAD:`a4c1d7c7e738` |
| tty-utils | 1.0.0:`e0601c619396`; 1.1.0–1.1.7:`d4737d241d0d`; 1.1.8–2.4.0:`3bc110cae25b`; 2.4.1–HEAD:`8bd8fc888274` |
| ptyctl | 1.0.0:`069bfbdd9dfb`; 1.1.0–2.1.11:`f15b0e73c7a8`; 2.2.0–2.4.3:`c3cbd75c5613`; 2.5.0–HEAD:`c8838db109c0` |
| ptyctl-cli | 1.0.0–1.1.3:`05e5f25e8eb5`; 1.1.4–HEAD:`cd6e3a43f6cb` |
| pager-pty-harness | 1.0.0:`0ebe2e28f963`; 1.1.0–1.1.3:`825ade4d44c8`; 1.1.4:`46c2e5a62db0`; 1.1.5–1.1.11:`b38681b0b091`; 2.0.0–2.0.4:`1b530524d5cb`; 2.1.0–2.1.4:`2227d8494108`; 2.1.5:`9e2f2e1b8741`; 2.1.6–2.1.11:`54ebb51698e1`; 2.2.0–2.3.1:`d5f8aed9c6a3`; 2.4.0:`56fe3040d5ba`; 2.4.1–2.4.3:`4cdf752c1885`; 2.5.0:`f9646e078696`; 2.5.1:`cc2ea21cd05c`; 2.5.2–2.5.4:`bfa38808c973`; 2.5.5–HEAD:`bcf64aa4ce3e` |
| test-support | 1.0.0:`641164836f09`; 1.1.0–1.1.3:`9bea110f96b5`; 1.1.4–1.1.11:`b757d038fa9c`; 2.0.0–2.0.3:`cf23d857526c`; 2.0.4:`798f1c8b13dd`; 2.1.0–2.1.2:`48275e9f6d56`; 2.1.3–2.4.0:`4a91d1f28809`; 2.4.1–2.4.3:`b498dd219ffd`; 2.5.0–2.5.5:`f23d18cf6581`; 2.6.0–HEAD:`45946e756fb5` |
| common/test-utils | 1.0.0:`c926d4e9863a`; 1.1.0–1.1.11:`f75a3cb906fb`; 2.0.0–HEAD:`be61208d4f5e` |
| dagre_rust | 1.0.0:`834e1be64801`; 1.1.0–HEAD:`1c0daa94a749` |
| mermaid-to-svg | 1.0.0:`960d2045feb7`; 1.1.0–2.1.11:`ef3850de67a6`; 2.2.0–HEAD:`2369ac0fa6dc` |
| nono | 1.0.0 不存在；1.1.0:`ae5950057786`; 1.1.1–1.1.11:`4d72efb5ae86`; 2.0.0–HEAD:`069be973e581` |
| nix-ohos | 1.0.0–1.1.0 不存在；1.1.1–1.1.3:`4e715fe52f39`; 1.1.4–2.4.1:`9ddcc93e9f74`; 2.4.2–HEAD:`5452e49ca589` |
| sqlite-vec | 1.0.0 不存在；1.1.0–1.1.3:`74d7d4cc685a`; 1.1.4–2.4.1:`533268c99c22`; 2.4.2–2.5.4:`0ab6b1ffb735`; 2.5.5–HEAD:`4b654468a2c8` |
| markdown/fuzz | 1.0.0:`41f7704f48b6`; 1.1.0–2.1.11:`13290d84ad27`; 2.2.0–HEAD:`2ce806a8d5b7` |
| proto/system-power/prod/registry mirrors | 仅 v1.0.0 存在于本产品 tag；精确 tree 和移除边界见前表，v1.1.0–HEAD 无这些旧 path |

## 文档与验证剩余边界

`third_party/README.md:70` 仍把 sqlite-vec 维护描述为仅补字节相同的四个文件，`:76` 说上游自含包出现即可删除；但 Cargo header 与 v2.4.2/v2.5.5 原生修复明确存在本地语义 patch。`docs/architecture/dependency-workarounds.md` 的同类删除提示也需同步。owner 是 vendor/dependency 文档，独立小修应把“重新评估 maintained fixes 并跑 native boundaries”纳入移除条件。当前 header 是入口自称的维护权威，因此这属于会误导升级的文档债务，不证明当前 binary 已丢补丁。README vendor 总表还未列 nix-ohos，应与实际 patch graph 核对。

| 实际执行 | 结果与含义 |
| --- | --- |
| `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s scripts/tests` | 34 tests，OK，1 skipped；跳过 OHOS 容器专用条件。含小型临时无第三方 Cargo fixture 与真实执行器流式/timeout/cancellation/选择测试；没有编译 Grow 工作区 |
| `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s scripts -p test_analyze_prompt_cache.py` | 15 tests OK；只用隔离 snapshot，不读用户真实 session |
| `cc -O2 -Wall -Wextra -Werror -DSQLITE_CORE third_party/sqlite-vec/tests/native_boundaries.c -lsqlite3 -lm …` | 真实 SQLite 边界和受支持 SQL 路径通过；输出 native boundaries and supported SQL passed |
| 同 C harness `-O1 -g -fsanitize=address,undefined -fno-omit-frame-pointer` | 通过，无本次 sanitizer 报告。macOS host 的结果不能等同 Linux leak checker 或全部 CPU/ABI |
| `rustc --edition=2024 --test crates/codegen/version/git_identity.rs …` | 4 tests OK：normal、detached、linked worktree、packed nested branch |
| 真实 runner 的 setsid/DEVNULL 后代超时实验 | PB-04 机制失败确认，精确自建 PID 已清理；不是新增仓库测试或实现修复 |
| `bash -n scripts/build-ohos.sh scripts/validate-release.sh`、`node --check scripts/test_trajectory_ui.cjs`、顶层 Python compile | 语法通过；不代表真实发布/主机交互/浏览器行为通过 |

未执行重型 Cargo、真实 release/install、所有目标交叉编译、OHOS 容器/设备、Windows Jobs/长路径运行、Linux/macOS sandbox kernel enforcement、真实 crash/fatal-signal、完整 CLI bridge 重连多请求、ptyctl stop/timeout、浏览器/剪贴板 bench、fuzz campaign。当前 finding 的验收分别列了这些入口所需定向补验；本区没有用新增保护、清理用户状态或真实发布来扩大授权。

OpenSpec 全量 strict/archive、全仓最终活动特性复核和 backlog 归并由父审计统一执行。本分区没有提前勾选根 change tasks，也没有把本报告的局部验证当成整体审计完成。
