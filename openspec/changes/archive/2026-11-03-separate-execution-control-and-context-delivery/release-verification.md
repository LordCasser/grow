# 联合旅程收口与本地 release

最终状态：联合验证与修正复测、独立 review、规范归档及本地 release 均完成。首个增量、中间失败及覆盖边界按实际情况保留，不把历史失败改称全绿。

## 实现与独立审查

完成工具等待、双向真实咨询路由、Goal 暂停/预算与显式恢复、held FIFO、取消到 idle、挂起/replacement、Received/Consumed ACK 丢失，以及真实 JSONL 上的公共删除竞态联合测试。逐项断言与证据层级见 [joint-task-evidence.md](joint-task-evidence.md)；原始运行记录见 [joint-verification.md](joint-verification.md)。

联合测试驱动的生产修正限定为两处责任边界：

- 取消 intent 与成功 terminal 接纳在现有 owner 内选择赢家。直接 Ask 保留真实 host 回应；Peer 保持原交换、部分帧和 pinned target，不能把本地取消意图当作远端已经取消。
- Peer 成功候选在证明发布前通过真实 JSON encoder 检查现有 IPC 帧上限。不可交付的成功候选明确失败，保留已完成 Sideband/用量，不截断、不生成永久无法消费的义务。

成功咨询仍是回答方的被动上下文来源；不抢占 stream/tool、不单独唤醒 idle、不取得人类权限或 Goal 后继执行资格。其他上下文变更仍由其原机制管理。最终独立对抗性审查及后续夹具复审见 [final-independent-review.md](final-independent-review.md)，未留本范围内必须修复项。

## 最终源码与运行证据

原工作区在外部并行任务中持续变化，故使用完整独立源码快照 `/tmp/grow-passive-release-20261103`，私有构建缓存 `/tmp/grow-passive-release-target-20261103`。源 HEAD 为 `bbd2c4dfa19d2ba789d15e0d8f7e8e2608a236cb`，包含当时未提交工作区文件。这个 HEAD 不是本次改动的已提交证明；产物追溯使用完整编译输入指纹。原工作区后续其他修改没有被覆盖，也不自动算入该产物。

最终 2739 个工作区编译输入 SHA256 清单见 `release-source-manifest.json`，聚合指纹为 `00fc4c095d10a5b162d06233b518d40447417ef72890c05cf8207365051474f7`。该指纹涵盖 crates、third_party、Cargo.toml/Cargo.lock、.cargo，以及 include_str! 引用的四个顶层输入：`.grow/workflows/deep-research.rhai`、`docs/architecture/agent-core-timeline.md`、`docs/workflow-rhai.md`、`agent.md.example`。完成 observation 修正时独立复核的 2735 文件范围指纹为 `94070283c23b2e96f93749610eb53840e702e4cb6c451aff49c515d67f986377`；增加这四个追踪项没有修改源码内容。规范与本 change 审计文档不属于编译输入。

| 验证 | 实际结果 |
| --- | --- |
| 最后一轮完整 ChatState | 610 passed / 0 failed / 3 ignored，26.91 秒。 |
| 最后一轮完整 Shell | 4546 passed / 1 failed / 9 ignored，724.93 秒。唯一失败是下述只读历史测试夹具；本行不改称全绿。 |
| 修复后 observation 模块，同一父 GROW_HOME 连续两次 | 两次均 3 passed / 0 failed，0.27 / 0.30 秒；包含真实公开查询、scope 拒绝、损坏 canonical 引用、单次 Inspect 和超时。 |
| Tools backend 完整模块 | 27 passed / 0 failed，5.03 秒。 |
| 最后一轮完整 Shell 内的新增联合测试 | message_delivery 16 项、双向/Peer/API 路由 2 项、真实删除 1 项均通过；原 steer/Goal/Workflow/fork/compaction 共享回归同轮执行。 |

完整库日志为 `/tmp/grow-passive-snapshot-full-libs-green.log`，名称不表示其结果全绿。该轮编译输入指纹为 `8156943cb788e51014ba5a327a6ef41899d91763ee8dfb43f979db54d749649d`；运行前后相同。唯一失败发生在 `public_subagent_get_reads_only_the_authorized_canonical_cold_result`：固定 ID 的 root 已存在于上轮测试目录，`init_session_with_summary` 返回 `AlreadyExists`。该夹具依赖 EnvGuard 切换 GROW_HOME，而 `grow_home()` 已经由进程 OnceLock 缓存，故实际上没有切换目录。

修复复用现有测试的独立子进程模式，在首次读取配置前设置新的临时 GROW_HOME，完整保留公开 API 与所有断言。`/tmp/grow-passive-snapshot-observation-fixed-first.log` 和 `...-second.log` 两轮都先运行会初始化父目录的 nonblocking 用例，再运行隔离的 canonical 用例，使用同一个已有父目录，不通过清空旧数据或忽略 AlreadyExists 过关。子进程必须实际精确执行一项且通过，零测试不算成功。

逐文件对比确认：该项隔离修正时，相对上述完整回归仅 `crates/codegen/shell/src/agent/mvp_agent/tests/subagent_observation_tests.rs` 的测试隔离包装改变；全部生产输入不变。独立审查接受只复跑该模块验证隔离修正，不重复未受影响的全部库测试。其他三个先前失败的 oracle 修正及定向 3/3 结果均已进入最后完整库回归，未留下未验证生产修改。

源码变更之后的命令均使用独立 GROW_HOME、私有 CARGO_TARGET_DIR、macOS 原生工具优先 PATH；测试线程栈 `RUST_MIN_STACK=16777216`。完整库命令：`cargo test --locked -p shell -p chat-state --lib -j4 -- --test-threads=1`；隔离模块命令：`cargo test --locked -p shell --lib subagent_observation_tests -j4 -- --test-threads=1`。Tools 先经 Cargo 重建，再运行刚生成二进制中的真实 backend::tests 模块；错误的文件名过滤曾选中 0 项，未计入通过数。

## 证据边界

有限状态 demo、canonical ACK-loss 夹具、真实 provider 路由与真实文件删除各证明自己的边界；不声称覆盖无限调度或所有 source×phase×fault 的笛卡尔积。通用进程 compaction/fork 回归不冒充包含新咨询来源的整条物理进程组合。默认忽略的既有测试未擅自算作通过。未新增通用 controller、第二消息账本、权限流程或重试墙。


## 静态检查收口

- 冻结快照的 `cargo fmt --all -- --check`、`git diff --check` 通过；默认生产依赖图 `cargo check --locked -p cli --bin grow -j4` 通过（1 分 57 秒，`/tmp/grow-passive-snapshot-cli-check.log`）。
- `cargo check --locked --workspace --all-targets -j4` 最终通过（21.25 秒，`/tmp/grow-passive-snapshot-all-targets-check-v4.log`），包含 Pager tests；没有另把未运行的 PTY 或 ignored 集成场景算作通过。
- 全 target 初次发现冻结的外部图片 PTY fixture 缺两个 import 和一个原版纯 helper，第二次编译再暴露三个 expectation guard 缺 `mut`；仅补入原作者已有对应定义和可变绑定。第三次发现另一份 ignored 集成测试的临时请求 Vec 借用及 pinned future 生命周期问题，分别绑定 Vec、用局部作用域在 mutable close 前释放同一 future。所有断言、请求数、顺序与 production 保留。三次失败日志按 `all-targets-check.log`、`...-v2.log`、`...-v3.log` 保留，不冒称此前已通过。
- 最终逐文件比较完整库运行时的 2735 文件集合：只变更 observation 隔离包装、`pager/tests/pty_e2e_auxiliary_image_commands.rs` 的缺失定义/可变绑定，以及 `shell/tests/test_session_protocol_recovery.rs` 的测试借用作用域，生产输入零差异。独立审查逐项确认，见最后审查附录；没有因测试编译失败更新运行时行为。
- 归档前核对 delta 操作：local-coordination 四项 MODIFIED 均有主规范原文，新增 Peer transport requirement 改正为 ADDED；连同 input-admission 两项、session-timeline 三项，共六项新增、四项修改。active+main 严格校验冻结快照 25 passed / 0 failed，原工作区 23 passed / 0 failed。归档后的结果另行记录。

工具链为 Homebrew rustc 1.99.1（58cf8f9c3，2026-10-13），cargo 1.99.1，host aarch64-apple-darwin。编译仍有既有 Atomic API 弃用、部分测试 import 及大 debug 二进制 unwind 警告，未为此扩张本次运行时修改。


## 归档

原工作区与冻结快照均执行 `openspec archive separate-execution-control-and-context-delivery --yes`，归档路径为 `openspec/changes/archive/2026-11-03-separate-execution-control-and-context-delivery/`。合入三个能力的六项新增、四项修改；十项 requirement 正文逐项与归档 delta 完全匹配。没有用主工作区较新的无关规范覆盖冻结代码的规范基线。

修复 36 条指向代码的相对链接，更新 demo README 的归档后运行命令；全部 77 条相对链接解析通过，未发现外部文档指向旧 active 路径。模型结果 JSON 随 demo 一并保留。

归档命令当时如实保留 4.3/4.4 两个未勾选的物流收尾项，因此首次 archived strict 的唯一失败是本 change 的 2 incomplete tasks（23/25），不是规范场景失败；active/main strict 当时通过。release 构建/验证与归档检查完成后才勾选这两项，并重新执行全量与 archived strict，不提前将待做操作写成完成。


## Release 产物与最终结果

归档后执行 `cargo build --release --locked -p cli --bin grow -j4`，使用默认 jemalloc + sandbox-enforce 特性、原生工具 PATH、独立 GROW_HOME 与私有 target；显式构建身份为 GROW_VERSION=2.5.5、RELEASE_COMMIT=bbd2c4dfa19d2ba789d15e0d8f7e8e2608a236cb。构建成功，用时 **4 分 14 秒**，日志 `/tmp/grow-passive-snapshot-release-build.log`。构建前后 2739 项输入清单逐文件相同，聚合指纹保持 `00fc4c095d10a5b162d06233b518d40447417ef72890c05cf8207365051474f7`。

- 可执行文件：`/Users/lordcasser/workspace/projects/grow/target/release/grow-2.5.5-00fc4c095d10`。这是本次冻结快照的独立文件名，避免被其他工作区构建覆盖；未安装到全局路径、未修改版本号或创建发布 tag。
- 版本输出：`grow 2.5.5 (bbd2c4d)`；`--help` 退出成功。文件为 Mach-O 64-bit executable arm64，大小 **150647264 bytes**。
- SHA256：`5897260de2452c8e64f75cbd36b7ee35c2c656e34180bb550e76673e99ffa2f5`。保存前后对比字节哈希一致。
- `codesign --verify --strict --verbose=2` 通过，valid on disk / satisfies Designated Requirement。`otool -L` 仅列系统库及系统 Framework，不依赖临时构建缓存中的动态库。
- 对**本次实际 release**执行 `python3 scripts/test_local_coordination.py --binary /tmp/grow-passive-release-target-20261103/release/grow --keep`，退出 0，**10 项真实独立进程检查全部 PASS**。覆盖默认 TMPDIR 发现、busy 前台/FIFO/去重/无工具 Sideband、typed failure 与来源隔离、取消/跨目录审批、真实 list+ask 工具、重启持久查询、崩溃租约清理、同 ID 不同来源、resident replay，以及 active+queued 崩溃后唯一 interrupted 终态。日志 `/tmp/grow-passive-release-coordination.log`；隔离 fixture `/tmp/grow-coordination-e2e-8s3qd_t7`。这不是为每个新咨询消费组合单独增加的进程测试。

测试与所有开发检查结束后，只对私有缓存执行 `CARGO_TARGET_DIR=/tmp/grow-passive-release-target-20261103 cargo clean --profile dev`，Cargo 报告移除 577106 files、166.9 GiB 逻辑产物；随后执行 release 构建。没有清理原工作区 target 或中止其他任务。冻结源码工作树和本次 release 保留供追溯。没有 commit、push、安装或远程发布。

最终 tasks 的验证/归档/构建项均按实际完成勾选；本范围内没有未解决的运行时缺陷或默默延后的 backlog。最终严格校验结果如下。


- 原工作区最终 `openspec validate --all --strict --no-interactive`：**18 passed / 0 failed**；`--archived --strict --no-interactive`：**821 passed / 0 failed**。
- 冻结快照最终同样两项：**24 passed / 0 failed**、**813 passed / 0 failed**。两目录包含的其他阶段 change 不同，因此项目数不同，均包含本次归档规范。
- 本 change **25/25 tasks 完成**；最终 78 条相对链接通过。交付副本再次通过 `--version` 和严格签名校验，字节 SHA256 与测试二进制相同。
- 冻结快照最终源码清单与构建前后清单一致，2739 项，`00fc4c095d10a5b162d06233b518d40447417ef72890c05cf8207365051474f7`。原工作区本 change 的文档/规范范围 diff 检查通过；其他并行编辑不属于这份已验证产物。
