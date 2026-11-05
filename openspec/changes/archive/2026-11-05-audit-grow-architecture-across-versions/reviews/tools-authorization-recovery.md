# Tools / authorization 独立补审

状态：独立补审完成，待父 owner 审视验收。交付三项当前静态发现（MCP 初始化后撤销复核、SDK leader 路由身份、SDK 重名声明）和一项主规范冲突；未执行 Cargo/平台运行验证。此文件是本次执行者唯一报告。前一 reviewer 及 resume 的 ChildResult 持久化 ACK 未确认，仍为 finalization_pending；它们不构成本次报告、清理或验收证据。本次未读取或写入原拟报告。最终分区 accept、跨区合成、活动功能 review 与重型 Cargo 验证由父 owner 负责。

## 基线与范围

- Git HEAD：`d6128138cacdc22553aba9dafba9bc6cc934e4d1`。本区 Git 对象与 v2.6.0 `5475dff997f0` 的选定 scope 完全相同；这不表示整个 HEAD 等同于该 tag。
- 本次读取时工作树含预先修改：tools 的 registry/normalization/ToolInput/Output/task、workspace permission types，以及 shell preflight 的 `SetSessionModel` 投影和并行 resume/模型实现。它们只作为活动区边界记录，不修改或验收其最终行为；下文事实以明确的 HEAD / 读取工作树 / 历史版本区分。
- 责任区：tools、common/tool-protocol、tool-runtime、tool-types、workspace/src/permission、sandbox、hooks、mcp、workspace-types；shell 的 tool preflight/dispatch 与 child ceiling 为消费接口。历史额外覆盖 v1.0.0 的 grow-tools-api 与 xai-hooks-plugins-types。未 checkout、merge、commit，未运行重型 Cargo，未外部写入。
- 已读根 AGENTS、OpenSpec README、本审计 proposal/design/tasks/inventory，以及 tool-authorization、extension-runtime、sandbox-boundary 当前规范；范围内未找到嵌套 AGENTS。已加载 software-engineering 入口及 Rust、directory-structure、handoffs。Atlas skill 无法加载（catalog 不含），有界 `atlas.explore(SubagentCapabilityState)` 返回旧行号/旧结构，不能作为当前代码权威，结论以源码与 Git 对象为准。

## 已核实的全部发布标签覆盖

独立执行 `git tag --list --sort=version:refname`、`git ls-remote --tags --refs origin`：本地和远端均为 46 个，ref 名与未剥离 tag object ID 全部一致（包括 annotated tags）；commit 列另以 `^{commit}` 剥离，避免把 tag object 当源码 commit。

Scope 为当前及旧名的九个责任模块完整目录（workspace 只取 permission 和 Cargo.toml），加 shell `actor/tool`、`subagent_capability.rs` 和旧 `acp_session_impl/{tool_calls,tool_dispatch,types}.rs`。`Scope SHA-256` 是有序 `git ls-tree -r <ref> -- <scope>` 原始行加 LF 的 SHA-256；Manifest SHA-256 是 scope 内全部 `Cargo.toml` 的有序 `path<TAB>blob ID<LF>` 的 SHA-256，显示前 16 位。清单保存 Git 对象身份，不代表已逐行穷举旧代码。A/M/D 取相邻版本的 `git diff --no-renames --name-status`；因此 v1.1.0 目录迁移显示为删除/新增。Owner 记号：T tools、P protocol、R runtime、Y types、A permission、S sandbox、H hooks、M MCP、W workspace-types、C shell 消费接口。`= previous` 仅表示本 scope 完全相同，明确复用上一条结构证据。

| Tag | Commit | Scope SHA-256 | Files / manifests | Manifest SHA-256 | Adjacent A/M/D | Changed owners |
| --- | --- | --- | --- | --- | --- | --- |
| v1.0.0 | `da034e22d87b` | `0d50c62f1c158250` | 377 / 11 | `151429f99fc0fab4` | baseline | TPRYASHMWC |
| v1.1.0 | `7ea59a9db583` | `a4eb9eb4eb2dc374` | 353 / 9 | `0b614525231094c3` | 353/0/377 | TPRYASHMWC |
| v1.1.1 | `4f8e0232eae0` | `3942224508034b96` | 353 / 9 | `075189104d585882` | 0/1/0 | M |
| v1.1.2 | `abca23e0b295` | `7320a3f101eecf82` | 353 / 9 | `075189104d585882` | 0/3/0 | TC |
| v1.1.3 | `1b24b9f03668` | `7320a3f101eecf82` | 353 / 9 | `075189104d585882` | 0/0/0 | = previous |
| v1.1.4 | `28b798dc17b2` | `61f4386750ec2252` | 353 / 9 | `4e9ef432e15ca6b1` | 0/13/0 | TRSHMC |
| v1.1.5 | `05a3fbcc57c1` | `651ee91aed98fac1` | 353 / 9 | `521d8910aba7d6c8` | 2/32/2 | TYC |
| v1.1.6 | `ca3400250af9` | `58b892d41e3e5f4e` | 355 / 9 | `521d8910aba7d6c8` | 2/22/0 | TAMC |
| v1.1.7 | `62d9b47fd9d5` | `73bf4e91676c731f` | 355 / 9 | `521d8910aba7d6c8` | 0/9/0 | TYAC |
| v1.1.8 | `bde0050632c2` | `9cde01c3bfafc5ed` | 355 / 9 | `521d8910aba7d6c8` | 0/15/0 | TYAC |
| v1.1.9 | `465d9b12a42d` | `9cde01c3bfafc5ed` | 355 / 9 | `521d8910aba7d6c8` | 0/0/0 | = previous |
| v1.1.10 | `38b2d7fc7039` | `ee6b09b2524ad262` | 356 / 9 | `521d8910aba7d6c8` | 1/4/0 | THWC |
| v1.1.11 | `79495bcd40e5` | `e1fc9e91f0b9c330` | 356 / 9 | `521d8910aba7d6c8` | 0/21/0 | TYC |
| v2.0.0 | `2282940c1120` | `bf860004632809be` | 315 / 9 | `358b1f160f1f0864` | 7/155/48 | TPRYAHMWC |
| v2.0.1 | `ae8df2a3b4b9` | `767bcc24f4e371f7` | 315 / 9 | `358b1f160f1f0864` | 0/24/0 | TC |
| v2.0.2 | `7233f4e51e6d` | `7a588fbf412e880e` | 315 / 9 | `358b1f160f1f0864` | 0/4/0 | TC |
| v2.0.3 | `5aabf367b53a` | `7a588fbf412e880e` | 315 / 9 | `358b1f160f1f0864` | 0/0/0 | = previous |
| v2.0.4 | `1b52597ccdb5` | `b3c592b1fc245c6f` | 315 / 9 | `358b1f160f1f0864` | 0/12/0 | TAHM |
| v2.1.0 | `d43756888adf` | `5008a17bf7541df5` | 316 / 9 | `7de86d90daa6dfc1` | 1/42/0 | TASHMWC |
| v2.1.1 | `96565d6975d8` | `5008a17bf7541df5` | 316 / 9 | `7de86d90daa6dfc1` | 0/0/0 | = previous |
| v2.1.2 | `1ca5c6b120ea` | `69c6cb6f61fa7512` | 316 / 9 | `7de86d90daa6dfc1` | 0/13/0 | TAC |
| v2.1.3 | `09c68b808c41` | `2a3a090ded431e43` | 318 / 9 | `7de86d90daa6dfc1` | 2/5/0 | TC |
| v2.1.4 | `87d1df4c3b0a` | `c41fb2ebb9916c33` | 318 / 9 | `7de86d90daa6dfc1` | 0/1/0 | C |
| v2.1.5 | `a8dc93716f9a` | `a1f9e56c50ceb91c` | 319 / 9 | `7de86d90daa6dfc1` | 1/48/0 | TAHMWC |
| v2.1.6 | `2a8297e818b6` | `8c5ff6804e525d96` | 320 / 9 | `7de86d90daa6dfc1` | 1/17/0 | TAC |
| v2.1.7 | `9f721d8e927c` | `4b670b38d18f0b38` | 322 / 9 | `7de86d90daa6dfc1` | 2/9/0 | TYAC |
| v2.1.8 | `7ed6441a8fb8` | `4b670b38d18f0b38` | 322 / 9 | `7de86d90daa6dfc1` | 0/0/0 | = previous |
| v2.1.9 | `bd1f5085a9c9` | `4b670b38d18f0b38` | 322 / 9 | `7de86d90daa6dfc1` | 0/0/0 | = previous |
| v2.1.10 | `c17b085d07f8` | `4b670b38d18f0b38` | 322 / 9 | `7de86d90daa6dfc1` | 0/0/0 | = previous |
| v2.1.11 | `3024dad1ef75` | `4b670b38d18f0b38` | 322 / 9 | `7de86d90daa6dfc1` | 0/0/0 | = previous |
| v2.2.0 | `a84ce03b0d13` | `700c67834c989fa7` | 322 / 9 | `5d9b0c49a74b8b6e` | 0/35/0 | TPYASMC |
| v2.2.1 | `b38627016b41` | `19545f4afdebe5a6` | 323 / 9 | `5d9b0c49a74b8b6e` | 1/5/0 | A |
| v2.2.2 | `afbe3e48209d` | `bbf763b4c25f6406` | 323 / 9 | `5d9b0c49a74b8b6e` | 0/1/0 | C |
| v2.3.0 | `ef665ec41eac` | `bbf763b4c25f6406` | 323 / 9 | `5d9b0c49a74b8b6e` | 0/0/0 | = previous |
| v2.3.1 | `7b7d56ab98a6` | `bbf763b4c25f6406` | 323 / 9 | `5d9b0c49a74b8b6e` | 0/0/0 | = previous |
| v2.4.0 | `cd5d88ec7f5d` | `a09e3fc8e3fae374` | 330 / 9 | `78a9c22664d4de0b` | 7/51/0 | TYAMC |
| v2.4.1 | `5fcbd12b0c10` | `702baa4cf8ae30f6` | 330 / 9 | `78a9c22664d4de0b` | 0/14/0 | TC |
| v2.4.2 | `ecb0f9cc6b48` | `c8e90a3ba264098b` | 330 / 9 | `78a9c22664d4de0b` | 0/1/0 | T |
| v2.4.3 | `2069c43db5a3` | `c8e90a3ba264098b` | 330 / 9 | `78a9c22664d4de0b` | 0/0/0 | = previous |
| v2.5.0 | `8443a4d1648d` | `fd3efeab924be3d0` | 331 / 9 | `78a9c22664d4de0b` | 1/35/0 | TYASHMWC |
| v2.5.1 | `a58b5dd7e8c7` | `17ebee6add3ff15d` | 331 / 9 | `78a9c22664d4de0b` | 0/2/0 | T |
| v2.5.2 | `1c88fccb4adf` | `66a8e7a19d1ddbfb` | 331 / 9 | `78a9c22664d4de0b` | 0/2/0 | T |
| v2.5.3 | `fce2f3d590dd` | `66a8e7a19d1ddbfb` | 331 / 9 | `78a9c22664d4de0b` | 0/0/0 | = previous |
| v2.5.4 | `069f712894ca` | `66a8e7a19d1ddbfb` | 331 / 9 | `78a9c22664d4de0b` | 0/0/0 | = previous |
| v2.5.5 | `599873595c94` | `5476d0f9c2969089` | 331 / 9 | `78a9c22664d4de0b` | 0/6/0 | TYC |
| v2.6.0 | `5475dff997f0` | `7dedc43d67104492` | 338 / 9 | `78a9c22664d4de0b` | 7/43/0 | TRYAMC |
| HEAD | `d6128138cacd` | `7dedc43d67104492` | 338 / 9 | `78a9c22664d4de0b` | 0/0/0 | = previous |

以上表是全版本结构和 diff 覆盖，代码深查与结论另列，不能从相同 tree 推出外围调用方/配置也相同。精确复现可使用表中 commit 和 scope 重新生成完整 SHA；模块 Git tree 可用 `git rev-parse <ref>:<path>`，manifest 内容可用 `git show <ref>:<path>/Cargo.toml`。scope 内现有九份 manifest 对应 tools、hooks、mcp、sandbox、workspace-types、workspace、tool-protocol、tool-runtime、tool-types；v1.0.0 另有 tools-api、hooks-plugins-types。根 workspace 依赖解析变化另外按有意义的版本检查，不把 manifest digest 不变当作所有第三方版本不变。

## 当前已核实关键链与所有者

| 边界 | 已核实事实与证据 | 状态所有者 / 依赖方向 |
| --- | --- | --- |
| 描述与动态派发 | `tool-protocol::ToolCapabilities` 声明 max_access（默认 All）与 subagent_review（默认 Inherit）；`tool-runtime::Tool` 的 Args/Output 约束与 type erasure 统一，LocalRegistry 仅负责进程内查找/派发，不声称授权（capabilities.rs:10、tool.rs:36、registry.rs:152）。 | protocol/types 不依赖 shell。tool-runtime 不持有 PermissionManager。tools 的 FinalizedToolset/ToolBridge 提供具体注册、参数适配和资源。 |
| Frozen preflight | 工作树 preparation.rs:652 先按 exact wire name `try_parse`，再投影具体 ToolInput/AccessKind、descriptor max 和 MCP trust-domain mask；child hard eligibility 在 gate 前拒绝不可审批身份（905、916）。PreToolUse 以最终目标为 cause；原 Hook 决策恢复仅接纳唯一匹配且已完成的 occurrence，不重跑未知外部效果（120）。 | shell SessionActor 冻结参数与当前 behavior，workspace permission 管策略和可记忆决定，Hook runner 管 handler 效果。活动中的 SetSessionModel 投影不作最终验收。 |
| 普通权限路径 | preparation.rs:1205 使用显式 Primary/Child 来源、execution cwd 与 child mode；PermissionManager 仍接收 in-fence 请求，GatePreflight 在 fast path 前应用 deny/Ask（gate_preflight.rs:23、manager.rs:1755）。 | root 与各 child 是独立 PermissionScopeState；state/persistence 属于 workspace permission，而不是 tool descriptor。 |
| 一次性 permit | authorization.rs:180 canonical JSON 排序后哈希；issue_tool_call_permit:294 绑定 call ID、wire/target、args hash、cwd、descriptor max、required access、actor source/epoch、MCP client ID/generation/mask。dispatch.rs:50 首先 CAS 消费，再逐项验证，通过才用 authority 中同一 bridge 执行（16）。 | permit 只属本次运行实例，不是持久化授权；workspace 上保存的 toolset 不是执行权威。 |
| child ceiling | subagent_capability.rs:34 的 DelegableCapabilityCeiling 用 initial mode 交集并精确绑定出生 MCP client ID；live SubagentCapabilityState 把 native identity eligibility、初始 RWX、review-required、MCP membership 分开。Required 不进入 RWX fast path；可见但 forbidden 的身份不打开审批（85、239、253、269）。 | live child 只可缩小继承的身份与能力；Ask/Auto 只发精确 permit。Agent harness replacement 改 epoch，不改变初始 RWX。 |
| sandbox | SandboxManager 在进程启动 apply/install；known Linux child launch 再加网络 seccomp；child_net.rs:260 先对 fd>=3 设置 CLOEXEC，只豁免 Grow 显式映射的 state pipes，然后安装 filter。is_active、requested profile 与 hook write-deny 是否必须成功为不同状态（lib.rs:48、108、121、175）。 | OS 隔离属 sandbox，不能由 LLM permission allow 声称已经建立；macOS/Linux/Windows 和 enforce feature 的承诺不同。 |
| Hooks | dispatcher.rs:49 生成 Execute/Skip plan，按 enabled、policy、matcher 决定；每个 handler 结果和 fail-open/on_failure=block 分离，按顺序遇拒绝停止后续执行（115、258）。 | hooks 管计划和 runner，shell 管 durable occurrence 与工具 gate；没有第二份 Hook 运行状态可代替 Timeline。 |
| MCP / wire | MCP manifest 固定 rmcp `=3.5.1`；servers/client/transport 管连接，依赖 tools 来注册工具；workspace-types 仅有 RPC/event DTO 与 delimiter，不持有权限或运行时（lib.rs:1）。 | 当前 MCP/SDK identity 与未合入旁支须分别核实，不能把分支的 3.6 验证写成 main 能力。 |

## 注册、权限与生命周期核对

`ToolRegistryBuilder::register_with_params`（tools/src/registry/types.rs:637）同时捕获 `T::Args` schema、typed parse、`Tool::capabilities`、`ToolMetadata` 与运行 handle；finalize 先检查 unknown tool、参数类型/语义、client name 重复和工具组合 requirements（849），再生成客户端 schema/描述与 reverse param map（1085）。`try_parse`（1446）和 `prepare_dispatch`（1707）均按最终客户端身份反向映射，派发后统一 finalize output。runtime 的 `LocalRegistry` 不二次推断权限。动态 MCP 以 `server__tool` 同时注册 descriptor、registry ID 和 parse target；动态 schema 来自 server，Args=Value，实际参数验证最终也依赖远端实现。`use_tool` 只接受已发现的 MCP qualified ID，移除 inner dispatch extension 后执行，避免递归落入自身。

已读 registry 的 duplicate-name、参数描述 remap、无 Terminal stream 测试及 `tool_kind_contract.rs` 全量 built-in allowlist。它们能约束 schema/注册漂移，但本次未执行。对受信任 ToolPack 的重复 registry ID、非双射参数 rename，以及 concatenated-JSON 修复选择等疑点，尚未建立用户入口上的失败闭环，未列为已确认缺陷。当前 builtin/标准 MCP 路径没有据此证实授权绕过；这也不是对任意第三方 ToolPack 的保证。

PermissionManager 的新增复杂度有实际约束支撑：`PermissionScopeState`（manager.rs:1412）拥有单域 grants/counters/cancellation；actor 用 `JoinSet` 独立运行请求（2829），控制命令仍由同一 actor 提交。模式变更只 invalidate primary，reset 取消全部旧域并重建 root，release 移除一个 child 并取消其 token（2840、2856、2871）。classifier 和 human prompt 等待同时观察 requester closed 与 scope cancellation，等待后再筛掉迟到结果（1930、2584、2691）；prompt mutation 仅在回应仍被接纳后合并到当前域，child 不写 root 文件（2600、2658）。持久化在取得串行 writer 后才取最新快照（1452），reset ACK 包括写入结果，shutdown 关闭入口、cancel 并 join 全部已接纳请求后 ACK（2913）。已读 `concurrency_tests.rs` 的并发 grants 合并、reset/release/mode 重入撤销、admission 上限及 ACK 写失败夹具；静态链与这些目标一致，未将“测试存在”写成执行通过。

工具资源持久化是独立状态域：`ResourcesPersistence` 单 writer、Save/SaveAndFlush 分离（tools/src/persistence.rs:208、434）；本地 store 绑定父目录 capability，读写拒绝非普通目标并限制 16 MiB。`FinalizedToolset::save_and_flush_persistence` 在资源锁内取快照并 enqueue，随后 await ACK（registry/types.rs:1973）；目录 fsync 失败发生在 rename 发布后会明确报告“已发布但持久性不确定”，不盲目回滚。这些机制不替代 shell Timeline 的工具/Hook 事实 owner。普通 `flush()` 只记录写失败，不能当作 durable success；要求持久提交的调用必须使用返回 Result 的保存接口。

MCP 初始化采用单 flight：短同步 state lock，握手在锁外，InitGuard 保持到结果提交且 disarm 与写入之间没有 await（servers.rs:3474、3571）。tools/list 分页收集拒绝 cursor 循环，注册与搜索发布在当前 bridge 的本地 publication 边界完成；等待网络时不占该边界。liveness watcher 持 Weak client，在同一 slot 锁下检查取消再清理。普通调用保留原 request ID/service，超时或 future drop best-effort 取消；未知结果不自动重放，旧结果只在原服务仍 current 时影响 reset（2017、2118、2158）。下面 F1 指出的是“尚未发送前等待初始化”的准入缺口，不把已经发出的操作误当成可撤销事务。

sandbox 的 requested profile、enforce 编译开关和 applied 状态分开。Linux Hook write-deny 校验 plan 的设备/inode/type，bwrap 内核对有效只读并安装 namespace lockdown；`Off`/`Devbox` 的承诺不同。child 网络 filter 同时处理 syscall architecture/x32、io_uring 和继承 fd；permission allow 不会自动获得这些 OS 保证。未在 Linux/Windows 或实际 sandbox 重执行环境运行，不能凭 macOS 静态阅读声称跨平台防护已通过。

## 全部相邻版本的变化解释

每一非空相邻区间均核对改动路径和 manifest 差异；对所有权/权限/生命周期切换继续读旧源码或具体 diff。小修只列与本区有关的变化，不将 release commit 的全仓标题当作本区实现结论。无变化区间以表中的相同 scope hash 显式复用；外围代码不在复用保证内。

| 相邻区间 | 本区变化与核对意义 |
| --- | --- |
| v1.0.0 → v1.1.0 | `d1a0279b4` 的 crate 边界迁移和 `e3602e7ff` 的工具 scope 重排；详见下段旧 API 去向。`578dac11a` 将无回应审批变为有界超时。 |
| v1.1.0 → v1.1.1 | MCP manifest 移除 rmcp 的 `reqwest` feature，避免重新引入错误 TLS provider；非调用语义改写。 |
| v1.1.1 → v1.1.2 | grep/ripgrep 缺失时返回安装提示，消费端 goal/image 分支调整。 |
| v1.1.2 → v1.1.3 | scope 完全相同，复用 v1.1.2。 |
| v1.1.3 → v1.1.4 | TLS/HTTP 依赖与测试 provider 初始化收敛，dirs/hashbrown/zip 等 manifest 统一；goal 工具消费链变化。 |
| v1.1.4 → v1.1.5 | document pipeline 转 anydoc/pdf/pdf-inspector，删除独立 PPTX 路径与全局 PDF cache；behavior/goal 共享类型和工具 IO 归并。 |
| v1.1.5 → v1.1.6 | 新建 subagent_capability 与旧 request_tool_access 路径；MCP live parent eligibility 加 client incarnation/祖先链，不能再仅按 server name 继承。 |
| v1.1.6 → v1.1.7 | workflow tagged action 的 object schema 修正；shell access/风险分类调整。 |
| v1.1.7 → v1.1.8 | `720cdedeb` 将 child escalation 改为 contextual/nonblocking，引入 pending mutation、owner shutdown/audit 边界；后续 v2.2.1 才进一步收敛 request-local actor。 |
| v1.1.8 → v1.1.9 | scope 完全相同，复用 v1.1.8。 |
| v1.1.9 → v1.1.10 | built-in ToolKind contract 表进入 registry；Hook StopCancelled/共享 DTO 与 tool 消费变化。 |
| v1.1.10 → v1.1.11 | Goal planner structured submission/resume 相应 task/update_goal/IO 同步；不将旧 planner 工具当作当前注册集合。 |
| v1.1.11 → v2.0.0 | Timeline owner 切换及 actor/tool 拆分；`5d9516b34` 将超出父初始能力的请求由拒绝改为交集，`bc7816f33` 使 child permission mode 独立。Hook 进程辅助依赖 tools → tty-utils，tools 增 cap-std/cap-fs-ext；旧 vendor/version 兼容工具与死 IO 被移除。 |
| v2.0.0 → v2.0.1 | 生命周期边界与 provider-qualified route 后续；本区 terminal、skill、task completion 和 dispatch 适配，不建立另一份会话 owner。 |
| v2.0.1 → v2.0.2 | 工具/任务 notice 控制反馈与 AccessKind 投影调整。 |
| v2.0.2 → v2.0.3 | scope 完全相同，复用 v2.0.2。 |
| v2.0.3 → v2.0.4 | permission 去除旧兼容/重复分支，Hook config、MCP 与输出截断调整；不是新授权 owner。 |
| v2.0.4 → v2.1.0 | Hook durable lifecycle/计划与失败策略收敛；rmcp 2.1 → 2.2、anydoc 0.2.4，workspace tree-sitter/git2 更新；加入 session coordination 工具消费。 |
| v2.1.0 → v2.1.1 | scope 完全相同，复用 v2.1.0。 |
| v2.1.1 → v2.1.2 | malformed tool response quarantine、compaction 后 recall gate、coordination recovery；schema/parse/IO 调整相互对应。 |
| v2.1.2 → v2.1.3 | agents_md discovery/reminder 与结果链更新；目录长路径与恢复变化由相邻 owner 负责。 |
| v2.1.3 → v2.1.4 | 仅 tool/result 两行替换与旧分支删除；模块树不变而消费代码变，不能只看 crate tree。 |
| v2.1.4 → v2.1.5 | `175b08368` 修复 Hook 有界捕获/进程组/DNS admission、permission 文件隔离/有界读取；其后删除 AvailableSkills 重复快照和未注册 Skill IO。resources capability fsync 后续由 `f47190ca2` 修正。 |
| v2.1.5 → v2.1.6 | child sideband inquiry/parent intervention 的 interaction 工具与 AccessKind/输出同步；协议/持久化仍归各既有 owner。 |
| v2.1.6 → v2.1.7 | ToolDefinition/model_image 共享类型下沉 common，task interaction 与前置 parse 适配。 |
| v2.1.7 → v2.1.8；v2.1.8 → v2.1.9；v2.1.9 → v2.1.10；v2.1.10 → v2.1.11 | 每个区间 scope 均无变化，逐项复用前一版本。 |
| v2.1.11 → v2.2.0 | `SubagentReviewPolicy::Required` 显式描述子审批，避免由 ToolKind 推权；guarded edits、bounded evidence 与 Linux fd admission；增加 same-file 与 sampling-types 依赖。 |
| v2.2.0 → v2.2.1 | `1d075b36b` 从阻塞权限 actor 转为 JoinSet request-local，reset/release ACK 和最新快照串行保存；新增 concurrency_tests。 |
| v2.2.1 → v2.2.2 | tool/result 中 sampling cache usage 投影调整。 |
| v2.2.2 → v2.3.0；v2.3.0 → v2.3.1 | 每个区间 scope 均无变化，逐项复用前一版本。 |
| v2.3.1 → v2.4.0 | planning/todo/explicit load_skill 与 suspend/exit 增加真实状态控制；MCP rmcp 2.2 → 精确 3.5.1，policy/server-name/SDK contract 测试加入。3.5.1 探针不等于 MRTR 产品交互闭环。 |
| v2.4.0 → v2.4.1 | shell snapshot payload 与 allexport 隔离、完整 terminator，skill loading/read/plan 后续；核对 `76f174a1c`、`fb1beab83`。 |
| v2.4.1 → v2.4.2 | shell_state 清理收尾；仅一个 tools 文件改变。 |
| v2.4.2 → v2.4.3 | scope 完全相同，复用 v2.4.2。 |
| v2.4.3 → v2.5.0 | `0091c8a3d` 复用完整分页收集，发现后发布到当前 Agent bridge；其余 resource scope、canonical path/Clippy 清理不等于新框架。 |
| v2.5.0 → v2.5.1 | task/update_goal 的最终报告提示与 completed parent goal 场景。 |
| v2.5.1 → v2.5.2 | 本区仅两个 LSP 测试文件修正。 |
| v2.5.2 → v2.5.3；v2.5.3 → v2.5.4 | 每个区间 scope 均无变化，逐项复用前一版本。 |
| v2.5.4 → v2.5.5 | child authorization/live catalogs 的 tool preflight 和 Task/planning 类型消费调整，保持 exact identity 与初始能力分离。 |
| v2.5.5 → v2.6.0 | subagent status/wait 与 observation boundary、task output deadline、MCP typed/structured 结果与取消/no replay、冻结 child 目录后续。`fccd91b10` 要求 cold child 初始 grant 等父 discovery/publication，防止快照出生即不完整。 |
| v2.6.0 → HEAD | 选定 scope 完全相同，复用 v2.6.0；活动工作树另列。 |

### v1.0.0 独有模块的实际去向

`grow-tools-api` 不是 grow-tools 的简单改名：v1.0.0 的 build.rs 生成 `grow.tools.v1` protobuf，lib.rs 导出 Execute/Finalize/State/ToolConfigEntry/streaming 等 gRPC DTO；`config_validation.rs` 为后端 save-time 与工具服务 finalize 共用 params_json/name_override/unknown-ID 验证；`slash_commands.rs` 和 `default_client_name`（lib.rs:112）承载前端/服务端一致的工具命名。v1.1.0 的 `d1a0279b4` 删除整个 crate、proto/build/wire_shape 测试，不是另起同名 API。需保留的 slash 文案迁至 `tools::slash_commands`；typed Rust `ToolConfig.params: Option<Map<...>>` 消除 JSON 字符串再解析，`ToolRegistryBuilder::validate_config`/`params_validation` 接手实际配置验证，默认名称用注册的未限定 `ToolEntry.id`，不再依赖按冒号预测远程名称。被删除的 gRPC 操作本身没有声称迁入 workspace-types，故不把类型名称相近当作保留旧 wire。

同期 `xai-hooks-plugins-types` 的 lib.rs 以 99% rename similarity 迁到 `extension-types/src/lib.rs`（`d1a0279b4`），由邻区持有当前 Hook/plugin/MCP 展示 DTO；它没有合并进 workspace-types。Hook domain 与 DTO 转换仍分属 hooks 和 shell extension。当前 workspace-types 保留轻量 RPC/event DTO，只依赖 chrono/serde/json；没有继续维护一条旧工具服务执行轨。两旧 manifest/源码均已纳入 v1.0.0 scope，迁出后的当前 owner 只做边界追踪，不扩扫邻区。

### 已修复的历史问题

| 历史缺口 | 进入修复的首个 tag / 证据 | 当前核对与限制 |
| --- | --- | --- |
| Hook 输出先无界收集、取消时仅直接 child 或依赖 session scope | v2.1.5，`175b08368`；旧 wait_with_output → capture_output，新增 HookProcessGuard | 每路保留 64 KiB+1 且继续排空，timeout 覆盖 stdin 与读取；非正常退出 guard kill 组。未在本次执行进程组夹具。 |
| Hook HTTP 校验后再次 DNS/proxy 解析、正文无界或超时不覆盖全程 | v2.1.5，同提交 | no_proxy + resolve_to_addrs 固定 admitted 地址、禁止 redirect，64 KiB body 和 DNS→body 总 timeout；未运行真实 TLS/DNS 网络测试。 |
| client identifier 字符替换产生 permission cache 同名；特殊/巨大缓存文件读取 | v2.1.5，同提交 | SHA-256 文件名、1 MiB 读上限、Unix O_NONBLOCK/普通文件检查；per-client 无法读取不回退共享 grants。源码有边界与 FIFO control 测试，本次未运行。 |
| Linux 子进程沿继承 socket 绕过只限制新建网络 syscall 的边界 | v2.2.0，`d30f10f35` | fd>=3 CLOEXEC 后只放行 Grow state pipes；测试以真实继承 TCP/UDP 成功对照排除空通过。Linux 运行未验证。 |
| 一个权限等待阻塞 actor 控制，迟到允许或旧快照可能覆盖新状态 | v2.2.1，`1d075b36b` | request-local JoinSet、scope token、control ACK、最新状态 merge/串行保存；已读真实并发夹具。 |
| MCP doctor/注册/descriptor 分页不一致，Agent 切换可能把目录发给旧 bridge | v2.5.0，`0091c8a3d` | list_all_tools 与 current publication boundary；已有 pagination、replacement 夹具。不能据此推断 F1 的调用等待已复核。 |

## 当前发现

### F1 · 高优先级：初始化等待后的 MCP 调用仍可使用已退休连接

**版本/位置。** HEAD `d6128138c`（相关代码等同 v2.6.0）；`mcp/src/servers.rs::McpErasedTool::run` 1785、`try_call_tool` 2017/2030/2062，`McpClient::call_tool` 4091；direct 的生产入口 `shell/src/extensions/mcp.rs::call_mcp_tool` 623/630/664。结论为静态可达，未运行 Rust PoC。

**触发。** 一次已接纳调用捕获 client A，等待 A 的首次或恢复初始化；在握手返回之前，live catalog 将该 server 移除或用同名 client B 替换；随后 A 的握手成功。model 路径可从已注册但重连中的 HTTP/SDK client 进入，direct 路径无需先有工具注册即可触发首次初始化等待。

**机制与后果。** run/direct 只在等待之前查 disabled/current client。ensure_initialized 只管理 A 自身 ClientState，不知道它是否仍归 session 持有；调用任务的 Arc 使 A 在 registry remove 后仍存活。等待完成后代码直接调用原 mcp_service，检查只剩 deadline，没有 session generation/client/上游资格复核。`is_current_client` 2098 只保护失败后的 reset，挡不住首次实际发送。因此 catalog 已撤销的服务仍可收到副作用调用，违反 extension-runtime 的 “Removed server ... calls cannot use its retired connection”（396）与 stale configuration 约束。disabled 更新和 parent revoke 也处于同一缺检查窗口，但 admitted deadline 的保持是正确行为，不能把缩短 timeout 当作撤销机制。

**owner / 最小修复边界。** MCP call admission，shell direct caller 提供 current McpState；在所有异步准备之后、发起 tools/call 之前复核捕获的同一 client 与当前工具资格，不按 server name 悄悄切换到 B。无需重写 PermissionManager 或持有 session 锁跨网络。

**可证伪通过标准。** 有可控握手 barrier，分别执行 remove/replace/disabled/ancestor revoke，再释放 barrier；原 client 的 tools/call 计数为 0，replacement 也为 0，旧调用明确失败。model 和 direct 均覆盖首次/恢复等待；未变更的 control 只发送一次并保留原 deadline。已发送后的调用取消仍只通知原 peer，不宣称回滚效果。旁支 `5b375b07c` 增加 validate_tool_client 与 `waiting_model_call_revalidates_disable_remove_and_same_name_replacement`，可作为机制/夹具参考，但不是 main 上执行通过的证据。

### F2 · 中优先级：leader 模式 SDK reverse request 缺少可路由的所属身份

**版本/位置。** HEAD/v2.6.0；`shell/src/session/acp_mcp.rs::SdkCallParams` 74、`GatewayAcpInvoker::invoke` 82/89；spawn.rs:1587 将全局 gateway 传入；`mcp/src/servers.rs::AcpServerEntry` 306 仅有 name/server_id。消费接口为 leader/server.rs:198、532、2159、2299、2440。

**触发与静态闭环。** 通过本地 leader 创建 SDK session，metadata 注册 grow/mcp/servers 后需要 initialize/tools/list/tools/call reverse RPC。producer 发出的 params 只有 serverId 和 message，无 sessionId，也无 `grow/leaderClientId`。AcpGatewaySender 原样 forward，并不补身份。leader 的 response-ID 路由只恢复客户端发起的 namespaced request；这里是新 reverse request，既不命中 target-client，也不命中 session route，带 id 的包最终在 2440 “Dropping non-routable response”。因此 SDK handshake/call 在 leader 传输上等到超时。直接单连接 ACP 不经该路由，不能用那种 mock 通过证明 leader 路径可用。本结论没有声称当前 main 已把 SDK 请求发送给其他客户端。

**owner / 最小修复边界。** shell SDK registration→reverse invoker→leader routing。保留实际注册连接身份，reverse params 携带 session 与受 leader 注入的 owner；即使同 session 由另一 client 驱动，也只能到注册者。不得从 serverId/name 或 last-active 猜目标；注册者断开后不可回退给 driver。

**可证伪通过标准。** 真实 GatewayAcpInvoker producer 接入 leader 夹具，A/B 用相同 serverId，A 注册、B 成为/保持 driver；A 的 SDK initialize/list/call 只到 A，generic reverse 仍遵守 driver 规则，A 断开不投递到 B。冷 load 重新绑定当前连接，不能延用旧 gateway。`5b375b07c` 包含 `sdk_reverse_requests_route_by_owner_not_shared_server_id` 和 `sdk_target_follows_registration_client_without_stealing_driver`，本次只审其源码断言，未运行。

### F3 · 中优先级契约偏差：SDK 同名声明被静默截成首项

**位置/触发。** HEAD/v2.6.0；`parse_acp_mcp_servers` acp_mcp.rs:29/45 接受 `grow/mcp/servers=[{name:"slot",serverId:"a"},{name:"slot",serverId:"b"}]`，仅警告并返回首项；`duplicate_names_keep_the_first`（127）明确锁定此行为。`agent_ops.rs:1920` 在真实 session spawn 前调用，返回 Vec 而非 Result，调用方不会获得 validation error。标准 mcp_servers 则由 acp_agent.rs:266/638 明确验证。

**受损契约/后果。** extension-runtime “MCP server name is the session identity”（493）要求单 client server array 重名时整组拒绝。当前 SDK array 形成静默部分成功，配置顺序决定真正执行目标，客户端无法据请求成功确认其声明已完整接纳；未据此推断跨用户越权。邻区接口审查需要识别 SDK 与标准 sources 是两条 admission 路，而非以标准 array 验证覆盖 SDK metadata。

**owner / 通过标准。** shell client-MCP ingress，联合 mcp 的 canonical server identity。实际接纳 SDK 声明的 new/cold-load 入口在任何状态副作用前拒绝重复组；resident/update 若接纳该声明字段也遵守同一规则，已有 resident catalog 不改变，有效控制输入保留。此项不要求扩充原本不支持 SDK 声明的入口。旁支把 parse 变为 Result 并统一 client source 的做法尚未合入，不能将该分支的 validation tests 记为 main 通过。

## 契约债务、旁支与复杂度结论

**D1：当前主规范内部还留有旧 MCP replay 条款。** extension-runtime:61 “Late tool errors do not reset replacement services” 仍要求用 current service 做既有一次重试；同文件:529 “Uncertain MCP tool calls are never automatically replayed” 与 :314 已禁止恢复自动生成新 tools/call。实际 main 代码 2062 只发送一次，error 后 reset 为下一次独立调用准备，`late_tool_error_does_not_reset_already_recovered_service`、HTTP error/no-replay 测试也遵循新边界。分类为文档契约债务，不把较安全的当前 no-replay 判为需要恢复重试的实现缺陷。owner 是 extension-runtime 规范/该能力的变更归档；通过标准是一个 current contract 对“副作用发生但回复丢失”只允许一次调用，且不再有相反 WHEN/THEN。此报告不改主 spec 或 backlog，由父合成去重登记。

旁支核对以 Git 对象完成：`main...codex/mcp-policy-sdk-identity` 为 main-only 80、branch-only 2，merge-base `dc7c02d89a6486db900bab516409ba7f6f942070`；独有提交精确为 `5b375b07c`、`6de47919c`，均未成为 main 祖先。前者把 Standard/SDK 定义纳入单一 source/diff owner，增加注册连接 identity、初始化后 call gate 与联合源验证；后者固定 rmcp `=3.6.0` 并扩展弃置/continuation 精确取消探针。main 在共同祖先后仍有独有的完整分页/current bridge 与 v2.6 typed result/no-replay 等改动，不能直接按“旁支更新”整支替换。分支保留 Grow 原服务-owning cancellation guard，是因 3.6 自动取消在最后 Arc 释放时的 service 生命周期仍有边界；不是把双重 guard 叠到 typed helper。3.5.1/3.6 的 `sdk_limitation_*` 探针成功只表示限制被复现，不等于 Form/URL/MRTR 产品验收；`add-mcp-multi-round-elicitation` 仍是未实现提案。

目前抽象大多有必要的跨边界消费者：typed Tool/erasure 服务统一 schema 和 runtime；ToolBridge 服务本会话资源、注册与提醒；PermissionManager 服务请求隔离/撤销；MCP service/generation 服务连接替换；Hook plan 与 durable occurrence 分开服务副作用可追溯。没有证据支持另起统一工具/授权框架。最明显的剩余 owner 分裂是 SDK declarations 在 acp_mcp 而 Standard definitions 在 configs，导致 F2/F3 与热更新边界需要额外同步；修复应沿单一 MCP effective catalog 合并。大文件、HashMap 数量、trait 层数本身均未列为架构缺陷。

## 验证记录与交付边界

本次已执行：46 local/remote tag refs 一致性校验；47 个 tag/HEAD scope 和 manifest fingerprint、每个相邻 A/M/D 与变更 owner 复核；各模块 tree 获取；关键历史源码/diff 与分支独有提交核对；当前入口/消费/测试静态交叉核实。最后检查 F1/F2 所引 mcp、SDK producer、leader，以及 permission manager/hooks/sandbox 相关 tracked 源码未偏离 HEAD。已发现的三项以静态可达结论交付，不声称完成端到端运行复现。

遵守分工，未运行 Cargo build/test、真实 MCP 服务、网络 PoC 或 OS sandbox 实验；未修改业务代码、共享规范、backlog 或其他 reviewer 报告。set_session_model/resume 仍是并行活动区，不给最终验收结论。父 owner 需要独立接受本报告、决定 F1–F3/D1 的去重与后续 change、统一执行 Rust/平台验证及 OpenSpec strict/archive 校验；这些未执行检查不被文档完成状态替代。

## 模块 tree 与 manifest 对象附录

下面逐 tag 的 tree 显示前 12 位；permission 取 permission 子树，manifest 的 workspace Cargo 已在前表 digest 中单独包含。`=` 表示该列精确复用上一行 tree。相同 tree 允许复用该模块结构/源码身份，不意味着其他模块或消费接口相同。v1.0.0 独有旧树另外为 grow-tools-api `40ebb38c8e83`、xai-hooks-plugins-types `65c85308ce41`，后续均已移除原目录。

| Tag | T | P | R | Y | A | S | H | M | W |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| v1.0.0 | `0caf2fcf816c` | `22daa3ac3ce6` | `b09d7e8ebe1e` | `339e8f525aab` | `50a1a8f1d1f1` | `fe576f3125ad` | `789c9b74da2f` | `5321f677643b` | `b2b64c17c5fa` |
| v1.1.0 | `fefea421ee0e` | `c9731e2a384c` | `2b19876e0e70` | `4fa5c70761b9` | `383b270ed639` | `f18810c2954e` | `21b0d3b89ab3` | `8e40be2ef328` | `833c78d61e15` |
| v1.1.1 | = | = | = | = | = | = | = | `2a133938d754` | = |
| v1.1.2 | `23bc4b7c8041` | = | = | = | = | = | = | = | = |
| v1.1.3 | = | = | = | = | = | = | = | = | = |
| v1.1.4 | `5766873c791c` | = | `25fd1473e10f` | = | = | `2b5ec9aa9487` | `0d5b3e95c589` | `adeb8e470adb` | = |
| v1.1.5 | `0a18992a5816` | = | = | `ffc8501e1aa3` | = | = | = | = | = |
| v1.1.6 | `ce7c548d31dc` | = | = | = | `87d1e21b92e2` | = | = | `d1d4a6706483` | = |
| v1.1.7 | `1dfe5bb26962` | = | = | `c6838cb518e8` | `92d5d05b1fed` | = | = | = | = |
| v1.1.8 | `a794c6dc0f85` | = | = | `85533f6ce562` | `11088b25e82d` | = | = | = | = |
| v1.1.9 | = | = | = | = | = | = | = | = | = |
| v1.1.10 | `e9956e33df75` | = | = | = | = | = | `6a34fefd923c` | = | `65fb4e21c666` |
| v1.1.11 | `bfca227732b1` | = | = | `ebb5f2056fb6` | = | = | = | = | = |
| v2.0.0 | `c9be05a35f41` | `99c8ddf9747b` | `a192b557384d` | `8b876fa5d4e4` | `6c4ae81ae4e9` | = | `73d0d38fdd86` | `cb91ab9e4539` | `6e49d36c17d0` |
| v2.0.1 | `f5ce0fe9f867` | = | = | = | = | = | = | = | = |
| v2.0.2 | `7e5e71237f30` | = | = | = | = | = | = | = | = |
| v2.0.3 | = | = | = | = | = | = | = | = | = |
| v2.0.4 | `7603186b6725` | = | = | = | `ff475efcc8da` | = | `504c5ab74af2` | `45bd876d2c3c` | = |
| v2.1.0 | `3a78509d8a7e` | = | = | = | `43a55421b4c6` | `4c9e963ad758` | `2f7a27d4ba20` | `9c2f77c28960` | `c89cc55431b8` |
| v2.1.1 | = | = | = | = | = | = | = | = | = |
| v2.1.2 | `fdca3fdf2c09` | = | = | = | `a03f27ee417e` | = | = | = | = |
| v2.1.3 | `efe6df92ad33` | = | = | = | = | = | = | = | = |
| v2.1.4 | = | = | = | = | = | = | = | = | = |
| v2.1.5 | `c3b4f108e3fb` | = | = | = | `466a701d8ff2` | = | `1b10975619e3` | `4c00d3a2fbd5` | `448216b18798` |
| v2.1.6 | `5d2b34a6d98a` | = | = | = | `6c30c40f9104` | = | = | = | = |
| v2.1.7 | `1bd4ca674caa` | = | = | `9d63949f5bd0` | `2d1b55dea21a` | = | = | = | = |
| v2.1.8 | = | = | = | = | = | = | = | = | = |
| v2.1.9 | = | = | = | = | = | = | = | = | = |
| v2.1.10 | = | = | = | = | = | = | = | = | = |
| v2.1.11 | = | = | = | = | = | = | = | = | = |
| v2.2.0 | `fea630350320` | `c062201a60f7` | = | `0dbecf6b5e3c` | `6f93f5bacd84` | `6ed4c12b7f32` | = | `ff860a36084a` | = |
| v2.2.1 | = | = | = | = | `90e731a2a610` | = | = | = | = |
| v2.2.2 | = | = | = | = | = | = | = | = | = |
| v2.3.0 | = | = | = | = | = | = | = | = | = |
| v2.3.1 | = | = | = | = | = | = | = | = | = |
| v2.4.0 | `e8ac1053c3c2` | = | = | `431c14c0e533` | `06eaa3659f75` | = | = | `d4fc1e4779bd` | = |
| v2.4.1 | `06c1110f8a81` | = | = | = | = | = | = | = | = |
| v2.4.2 | `f33d57f1f30f` | = | = | = | = | = | = | = | = |
| v2.4.3 | = | = | = | = | = | = | = | = | = |
| v2.5.0 | `0a4bb4d9efe3` | = | = | `698b70bcdff7` | `d46a6ab95bbf` | `a06c97f7b077` | `0080ec37726f` | `0e3360cde0cc` | `a0559b8d7117` |
| v2.5.1 | `dbe07ef2a051` | = | = | = | = | = | = | = | = |
| v2.5.2 | `d72574ee1247` | = | = | = | = | = | = | = | = |
| v2.5.3 | = | = | = | = | = | = | = | = | = |
| v2.5.4 | = | = | = | = | = | = | = | = | = |
| v2.5.5 | `6a0064e4a87e` | = | = | `9a4d630b8dc1` | = | = | = | = | = |
| v2.6.0 | `cf90277365d3` | = | `7e0da870e2f3` | `0bc2061bc856` | `324d1d76a5c8` | = | = | `344beeff3ebc` | = |
| HEAD | = | = | = | = | = | = | = | = | = |

Manifest 的 blob 向量按首次改变 tag 列出；未列 tag 精确沿用上一向量，与前表逐 tag 的 aggregate digest 对应。T/P/R/Y/S/H/M/W 与上表同义；A-manifest 为 workspace/Cargo.toml，不是 permission 子树。v1.0.0 路径使用 grow-/xai- 旧名，额外两个 manifest 也列在该行。

| 首次 tag | 变更的 manifest blob ID（前 12 位，其余沿用） |
| --- | --- |
| v1.0.0 | T `595a6cb7bdc2`，P `b18753b7b861`，R `3c65d2024a92`，Y `edb701854731`，A `713f8f8e3698`，S `e15c40604dda`，H `bbc376d0009f`，M `b7d7b807629f`，W `537f90629a17`；tools-api `b723ed22d719`，hooks-plugins-types `8e552f90ad2f`。 |
| v1.1.0 | 旧路径删除，当前路径 T `1f15298df705`，P `f7b2738a7c00`，R `c57a17a16bd9`，Y `d5b9050a0485`，A `bd64f27d131e`，S `2e982a825877`，H `82bfcbbe5a22`，M `ad0edc78e7f0`，W `d9495ed6cae4`；tools-api manifest 无替代，hooks-plugins-types 已移出本区。 |
| v1.1.1 | M `58a7030aaf06`。 |
| v1.1.4 | T `657f64712aa8`，A `0eba9b125af3`，S `2cc80fa58dbe`，H `f163447882f9`，M `6e236265f082`。 |
| v1.1.5 | T `afa46fe5190a`。 |
| v2.0.0 | T `6fc4c4b556ca`，H `50c5a01c9611`，M `dfe7b72a18bf`，W `e0597b805a0d`。 |
| v2.1.0 | T `deb5ca5cf090`，A `31070dce32ce`，M `64e0b463b9a7`。 |
| v2.2.0 | T `6ad255cbaca8`。 |
| v2.4.0 | M `bababd9c7a84`。 |

另外逐 tag 读取根 Cargo.lock 核对了本区三个关键依赖的实际解析版本，避免把叶 manifest 不变误判为依赖未变：

| 连续范围（该范围内所有 tag） | rmcp | reqwest | tokio |
| --- | --- | --- | --- |
| v1.0.0～v1.1.3 | 2.1.0 | 0.12.24、0.13.4 双版本 | 1.52.3 |
| v1.1.4～v2.0.4 | 2.1.0 | 0.12.28、0.13.4 双版本 | 1.53.1 |
| v2.1.0～v2.3.1 | 2.2.0 | 0.13.4 | 1.53.1 |
| v2.4.0～v2.6.0、HEAD | 3.5.1 | 0.13.4 | 1.53.1 |

HEAD 根 Cargo.toml blob 为 `c6832d172162`，Cargo.lock blob 为 `34b4db056122`。历史双 HTTP 栈的隔离理由不能直接作为当前 main 继续双栈的债务；锁文件中已收敛。本核对不等于逐版本依赖编译或第三方实现审计。
