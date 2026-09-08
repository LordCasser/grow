# 待启动事项

> **定位**：本文件只记录长期计划，不是当前版本待办或实现授权。任何条目都不会自动进入实现；即使成熟度或验收条件已经满足，也必须由用户另行明确启动。

Grow 的配置保持本地化：全局配置位于 `$GROW_HOME/config.toml`，项目配置位于项目内的
`.grow/config.toml`。项目配置只影响当前项目，并按现有解析策略覆盖全局配置。

远程配置管理、deployment-config 服务、签名策略同步及其专用 CLI 不在规划范围内。

## 长期：MCP Elicitation 与交互式 MCP

> **状态**：等待协议与生态稳定；满足条件也不会自动进入实现，必须由用户手动启动。

MCP Elicitation 以及配套 UI、交互暂不绑定当前 draft wire。目标交互模型需要先进入有日期、非 draft 的正式 MCP 规范；官方 Rust SDK 和至少另一个主流官方 SDK 需要完整支持；生态中至少需要出现两个可互操作客户端和五个独立服务端，并且已有真实的 Form/URL 使用场景。这里写的是严格的成熟度证据清单，不是自动门禁。

未来如果启动，Grow 的边界固定为：

```text
版本化 MCP adapter
  → Grow PendingInteraction/UI
  → Timeline interaction request/outcome
  → 恢复原 MCP call
```

Elicitation 不是新 turn，不进入用户 FIFO，不建立第二条 interjection 通道，不采用上游 single-slot 覆盖模型。影响 MCP 调用是否继续的请求和结果属于 Timeline 事实；窗口焦点、开关等纯 UI 状态仍可临时保存。Form 不采集秘密，URL 模式要求显示服务端与完整目标地址、显式同意且禁止预取，秘密和令牌不得进入 Timeline 或模型 Surface。

参考：[MCP Elicitation draft](https://modelcontextprotocol.io/specification/draft/client/elicitation)、[2026 Release Candidate](https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/)。

## 长期观察：Worktree 生命周期

Worktree 生命周期、复用与安全边界继续等待上游稳定。它不是当前关键路径，不进入 v2.1.0 临时待办；后续仍需用户明确指定后才能启动。

## 搁置：底部 Status line

上游 `[ui.status_line]` 已确认是底部附加行：全屏模式位于 shortcuts bar 上方，minimal 模式位于输入框 info row 下方，不是 Grow 顶部的 `AgentStatusBar`。

这一方向搁置，不修改顶部状态栏、输入框下方区域或配置，不设计 builtin/command status line，也不进入 v2.1.0 临时待办。除非用户重新开启，否则不再推进。

## 长期：v2.1.0 架构审计后续

以下项目不是 v2.1.0 的实现内容。它们只记录已经确认的边界、影响与未来验收条件，不能作为当前代码的第二事实源。

- **LLM 非聚合失败的因果取证**：工具协议污染修复已保留聚合响应和 `IntegrityRepair` replacement，但这不等于覆盖解析失败、半截 SSE、空响应重试及所有 Sideband 失败中的原始证据。后续应沿现有 request/attempt/result 生命周期核对记录边界；凡参与重试、停止、计费或降级的部分输出和决定，都必须在所属 Timeline 中留有可验证的证据或不可变 artifact 引用。验收要求在流中断、解析错误、重试和崩溃窗口注入故障后仍能重建因果链，不能只保留成功 attempt 或另建调试日志充当事实源。此项与已实现的 Surface 工具身份/配对修复分开处理。

- **Workflow 进度 checkpoint**：Spawn seed 与 lifecycle 可以在 manifest 缺失时恢复 Run identity、冻结契约和终态，但不能完整重建 `current_phase`、Agent 行、累计预算等中间进度。未来应新增有界 Timeline checkpoint 或 journal fold，并证明 sidecar 全失时的投影与正常恢复一致。
- **Workflow Forgotten/tombstone**：当前 clear tombstone 仍位于 sidecar，历史 Timeline 不表达“该 Run 已被遗忘”。未来应设计 typed `Forgotten` 事实，并证明清理后重启不会复活旧 Run。
- **Workflow restore cap 顺序**：当前恢复数量上限先于全部有效性与 tombstone 判定。未来应先解析权威身份和清理事实，再对有效候选施加稳定上限，避免坏记录挤占额度。
- **Workflow sidecar repair**：seed fallback 目前只恢复内存投影，不把重建结果当作普通 CAS 写回。未来应提供独立 repair transaction，验证崩溃幂等性，且不能放宽正常 revision CAS。
- **Behavior projection freshness**：projection 与服务端 admission 已共用纯判定，但 foreground 变化后的 UI 广播仍可能短暂显示上一次快照。未来应在 admission facts 变化时按 revision 推送，并证明旧投影不能覆盖新状态。
- **Pager 图片草稿所有权**：v2.1.0 明确拒绝持久化临时图片路径。未来若需要图片恢复，必须设计独立 blob 生命周期、配额、加密/清理和引用完整性；验收要求崩溃后不泄漏路径、不悬挂 blob、不自动发送。
- **子 Agent follow-up admission**：当前只有创建 child 时的初始 `QueuePrompt`，没有父 Agent 向存活 child 追加消息的协议或工具。本版本不新增通道。未来若启动，必须把 ownership 与 liveness 分开判定，使用稳定 `message_id`，并且只在消息原子进入 child 自己的 Timeline/admission 后返回 `Accepted`；`Rejected` 与 `Unconfirmed` 必须分离，saturation、deadline、payload limit、channel closed 使用 typed outcome。实现不得增加全局通道、复用父 Agent FIFO，或把父 Timeline 当成 child 消息事实源。
- **Sandbox child FD inheritance**：v2.1.0 的 child network filter 封闭新建 socket、`sendmmsg` 与 `io_uring` 绕过，但 syscall filter 无法阻止 child 通过继承且已连接的描述符使用 `write`、`writev`、`sendfile` 或 `splice` 发送数据。未来应在进程创建边界建立显式 FD allowlist/close-range 契约，并以预连接 TCP/UDP 描述符做威胁回归；验收要求未知描述符在 exec 前关闭，确需继承的通道具有 typed ownership 与最小权限。
- **Folder Trust handle-relative 使用边界**：当前 schema、持久化 CAS 和进程内 cache 已绑定 root/`.git`/common-dir 的 filesystem identity，并在决策与 store read 后重验，但 loader 最终仍按 pathname 打开项目配置；目录可在最后一次 identity check 与实际读取/执行之间被替换。未来应让 trust admission 产出持有 OS directory handle 的 workspace capability，配置发现、读取与进程 cwd 全部相对该 handle 完成，或对每个读取结果做前后 identity 验证且把执行绑定到同一打开实体；验收要求在 check→open、open→parse、parse→spawn 三个窗口注入 rename/replacement 时均不得执行替换实体内容。


## OpenSpec 迁移发现

这些条目只登记，不在本次文档迁移中修改代码。

- **Memory 注释漂移**：`crates/codegen/memory/src/lib.rs` 示例仍写纯 hash 目录，而 `storage.rs::new_inner` 使用 `compute_workspace_hash` 的实际结果。后续以实现及路径测试统一注释；本次 spec 只约束 root/工作区隔离，不复制过时格式。
- **Goal 架构文章版本漂移**：`docs/architecture/goal-continuation.md` 标题仍为 v9，`goal_tracker.rs` 声明版本 10 且实现连续阻塞审计。后续更新机制解释时核对整个生命周期，当前要求见 behavior-goal。
- **规范细粒度扩展**：当前规范覆盖核心边界，未穷举编辑工具算法、所有 slash 参数、渲染布局和每个平台的系统调用。触及这些行为时补充对应 delta 和实际测试，不据此发起全仓重构。
- **迁移前进行中的审计**：`docs/architecture/continuous-feature-audit.md`、`project-review-2026-09-07.md`、`trajectory-review-2026-09-07.md` 与根目录临时候选文件属于用户已有未提交工作。此次保留原文，不标记完成；后续继续时先将相关记录纳入独立 change，避免覆盖原工作。

- **工具流 UTF-8 tick 边界**：`crates/common/tool-runtime/src/streaming.rs::stream_chunk` 在待发字节只有不完整字符时，cut=0 的兜底会消费为替换字符。真实函数复现输入先 C3、后 C3 A9，输出两次 U+FFFD 而非 é，且 gap/truncated 为 false。详见 [逐包复现](changes/inventory-all-crate-features/reviews/tool-runtime.md#已复现的实现边界)。未来单独明确无新字节、尾部不完整字符、非法字节及 cap=0/1 的处理，验证任意合法 UTF-8 tick 划分都可无损拼接；当前不改算法。
- **工具公共接口注释漂移**：`tool-protocol/src/capabilities.rs` 注释仍称默认单并发，实际 max_concurrency=None；`tool-runtime/src/render.rs` 默认 model_output 为空，由运行时提取，sender 也未强制 assistant；`tool-runtime/tests/tool_dyn.rs` 部分注释把 All 写作 Write。规范按实际字段和断言提取。后续单独统一说明，不凭注释修改实际契约。

- **进程身份与 PID 输入边界**：shell-base 的 is_grow_process_strict 在 Linux/Windows 仍是 grow 子串匹配，macOS/BSD 是 basename 子串匹配；kill_process_with_signal 在 Unix 将 u32 直接转 i32，未过滤 0/负值语义。后续沿全部 caller 核查来源与校验，独立设计持有进程身份及终止范围的契约；不得把名称探测当作精确身份保证。当前仅登记源码事实，未尝试终止任意外部进程。
- **安全文件写入时序**：shell-base/util/secure_file.rs 对现有文件先 truncate/write/flush 后收紧权限，基于 pathname 操作且跟随 symlink；Unix 低九位已 0600 时不会清除其他特殊位。未来单独按凭据存储 caller 确认威胁模型、打开方式和原子写入需求，测试写前权限、路径替换和失败窗口；本迁移不更改行为。

- **ProcessScope enrollment 失败回收**：tty-utils/src/process_scope.rs::spawn 在 child spawn 成功后调用 enroll，ProcessGroup::new/attach 失败直接上抛，未显式 terminate/wait。后续需沿 Windows job 分配和各 caller 审计失败时的 child 所有权，加入故障注入确认不会遗留进程；当前仅记录可见缺少统一回滚，未宣称已动态复现泄漏。Weak 所有权与操作系统进程存活不等价，相关注释中的 reap 应与 wait 区分。

- **Mermaid subprocess 预算和误用回收**：`mermaid/src/subprocess.rs::run_with_timeout` 在 spawn 后才断言 payload 需要 piped stdin，此 debug 误用路径没有统一 kill/wait guard；scoped writer 必須 join，组外/Windows 后代持有 stdin 时可能超出等待预算。后续沿 pager 与 mmdc caller 单独审计 ownership、Job Object 与故障注入，区分进程组信号和真正 wait；当前记录源码边界，未动态复现后代泄漏。注释仍称 tty-utils ProcessGroup 只支持 tokio，与现有 attach_std 不符。

- **ptyctl-cli 独立构建与选项差异**：默认 `cargo build --locked -p ptyctl-cli` 在 reqwest 0.13.4 的 query feature 缺失处失败；显式 `--features reqwest/query` 构建后，mock HTTP 已复现 screen --json/--ansi 被忽略，status/cursor 对 HTTP 500 仍成功退出。后续单独补正确依赖 feature、确定 CLI 输出契约并回归，不借全工作区 feature 合并掩盖独立构建失败。
- **ptyctl 登记身份与生命周期**：CLI name 直接拼接路径，固定临时文件和探活后替换没有 CAS；server_alive 只检查 200 JSON 存在 size，不能证明会话身份。run 在启动 child 后 bind，serve 未绑定 session 退出信号，清理只比较 port。后续结合库层核查路径输入、并发 takeover、端口复用与失败回收；当前未声称已复现这些竞态。

- **ptyctl 生命周期未接通**：session.start 未消费 timeout/linger，shutdown receiver 丢弃使 stop 成为无效发送；reader 先置 alive=false 时 waiter 跳过 wait，exit_code 可丢失；session 持有 output_tx 而 WebSocket task 持有 session，child 退出不足以触发 Closed。已用短命自有 PTY 复现 stop/timeout 无效、无 linger 时服务仍存活、退出码 null。未来独立建立退出/回收/服务结束契约并覆盖成功、失败、超时、取消及 WS close，不能让 ok:true 冒充已停止。
- **ptyctl 查询与样式投影**：server.parse_range 单值 n 生成 n..n+1，经 terminal.resolve_range 返回两行/列（rows=1 已动态复现），usize::MAX 加一可溢出；非法 range 回退全屏。styled 尾空白裁剪仅检查 fg/bg/bold，忽略其他样式；HTML 忽略 inverse 与 cursor 参数。后续明确选区和格式契约、补边界回归；本次仅记录，不修改实现。

- **Crash evidence 持久化与符号身份**：check_previous_crash 忽略报告/历史写错误后仍删 blob；blob 仅保存绝对 IP，启动时在当前进程解析，缺少模块 build identity、load base 与重定位。未来独立设计可确认持久化的消费与准确符号解析，注入写失败、ASLR/升级、同 timestamp 崩溃；不把 Some(CrashReport) 当作报告已保存。
- **Crash handler 安装与平台说明**：sigaltstack/sigaction 失败未检查，altstack 全局 latch 在成功前置位但栈仅安装线程可用，重复 full install 不关闭旧 fd/handle；raw frame walk 仍能次生 fault。README non-Unix no-op 和默认 escape 与实现不符，Windows 非 x86_64 实际不捕获 PC。后续按平台单独验证安装/恢复/并发边界与更新说明；本次不扩大为代码整改。

- **配置容错的嵌套边界**：config-types 的 tolerant scalar visitor 不处理 seq/map；已直接调用真实类型复现 GC enabled=false 被兄弟数组字段导致的整个对象降级吞掉，display_refresh 数组字段导致 RemoteSettings 整体失败。后续按保留显式禁用值的契约统一容错并补矩阵测试；本次不更改配置行为。PermissionRule 的 action 字段实际必需，RuleAction::Default=Deny 不代表缺字段会自动拒绝，相关注释需独立澄清。

## config 逐包核查：写入与探测边界（待独立处理）

- 基础原子写入碰撞清理：`crates/codegen/config/src/fs_atomic.rs::write_atomically` 在create_new失败后仍删除候选tmp，可能删除非本次创建文件。后续独立变更应验证碰撞时原文件与碰撞对象均保留，仅清理成功创建的artifact。本次仅源码确认，不声称已动态复现。
- 托管路径身份：`managed_text/source.rs`的metadata/read/open分离，4MiB限制仅检查读取前metadata；`transaction.rs::open_lock`没有NOFOLLOW/type检查。后续独立审计应覆盖读取期间增长、锁链接、父路径替换及发布后外部写入，明确句柄身份和恢复所有权，不把当前阶段注入测试当作无竞态证明。
- 探测进程期限：`shell.rs`的--version及Windows where无deadline；`managed_text/validator.rs`预算始于spawn/attach之后，Unix正常退出不清理留存后代。独立变更应覆盖探测挂起、validator退出留子进程及attach失败，验证deadline和归属清理。
- Hook路径检查：`global_hook_sources.rs`在metadata错误停止并报告未见symlink，六个系统路径的豁免未按平台和真实链接目标核验；Linux mountinfo未解码转义路径。后续应明确fail-closed范围并用权限失败、伪装路径和含空白mountpoint验收。
- Windows Cmd连接符：`shell.rs`当前为Cmd返回分号；需结合调用方验证真实命令连接行为，再单独调整，不在本次文档迁移修改。

## ACP transport 逐包核查：资源与完成边界（待独立处理）

- stdin读行未限长：`acp-transport/src/stdin_reader.rs::forward_lines`的64项通道只限制行数，单行read_until仍可增长且receiver drop不打断阻塞read。后续独立变更应验证超长无换行输入、关闭消费者和线程退出；Windows还需覆盖NUL打开/SetStdHandle失败，不能将私有副本成功视为隔离成功。
- 行上限测试缺口：`line_reader.rs::read_line_capped_rejects_oversized`实际上只测普通行；独立验收应覆盖恰好64MiB、超过阈值、EOF部分行和分片跨阈值。此处记录覆盖缺口，未改测试或实现。
- Gateway并发与所有权：`gateway.rs::run`为每条消息独立spawn，现有顺序测试handler不挂起；`connection.rs`退出也没有集中join/cancel已spawn任务。需结合调用方确认所需排序和断线收敛契约，再用首条暂停/后条完成、连接关闭且handler pending的场景验收，不直接推导必然业务错误。

## workspace-types 逐包核查：现有失败和契约边界（待独立处理）

- Hook fixture失败：`workspace-types/src/rpc/hooks.rs::hook_registry_wire_round_trips_server_json`缺失`HookSpecWire.on_failure`必需字段。独立工作树全feature测试复现50通过/1失败。后续应从真实HookRegistry序列化确认fixture字段并修正，不应通过给必需字段随意加默认值掩盖测试漂移。
- Search默认值差异：`rpc/search.rs::ContentSearchRequest`派生Default给respect_gitignore=false，serde缺省给true；需结合Rust构造调用方核查意图，再独立统一或显式保留差异。
- Hunk空响应说明：`rpc/hunks.rs::HunkActionResponse`为空对象，而注释称operation返回null。待核查workspace dispatcher的转换后决定修正说明还是实现，不在此迁移推断线上结果。

## sqlite-vec 文本解析边界（待独立评估）

当前fvec_from_value/int8_vec_from_value并非严格JSON解析器，本地C动态扩展探针确认缺逗号、重复逗号、缺闭合括号、附加尾文本被接受。后续结合memory调用方输入来源与上游设计决定是否需校验，不在文档迁移中替换vendor解析器。验收应明确接受语法，覆盖上述样例并区分Cargo与动态扩展构建。

- **sqlite-vec bit展开顺序不一致**：主C的vec_eachColumn按每字节高位先行，而vec_to_json和binary quantization采用低位先行。动态扩展同一0x01输入复现不同顺序，证据见当前inventory change的sqlite-vec-each-probe.json。后续结合使用方决定修复或明确独立语义，本迁移不修改vendor实现。

## sqlite-vec 12字节文本元数据范围过滤失败

状态：待单独处理。触发：vec0 TEXT metadata恰好12字节，KNN MATCH同时使用>=等范围过滤。当前动态扩展探针中name='abcdefghijkl'等值成功，name>='abcdefghijkl'报Could not filter metadata fields，普通非KNN查询正常。

证据：third_party/sqlite-vec/sqlite-vec.c的vec0_metadata_filter_text中EQ/NE/IN使用<=VEC0_METADATA_TEXT_VIEW_DATA_LENGTH，而GT/GE/LE/LT使用<，12字节值会转入长文本查找。复现结果见changes/inventory-all-crate-features/reviews/sqlite-vec-knn-planner-probe.json。构建为本地dynamic extension，未声称Cargo路径测试完成。

未来验收：核对写入布局及全部比较操作，以11/12/13字节、公共前缀、嵌入NUL和SQL比较语义建立有意义回归，保证有效槽的KNN过滤与约定一致；修复单独建change，不混入文档盘点。

## sqlite-vec DiskANN丢弃已下推的过滤条件

状态：待单独处理。vec0BestIndex将KNN distance范围及多值rowid IN标记omit；vec0Filter_knn_diskann只解析MATCH/k，未应用这些条件。动态扩展实测distance>100仍返回约2.83/5.66，多值IN(2,3)仍返回rowid1。flat对应多值IN正常。证据：changes/inventory-all-crate-features/reviews/sqlite-vec-index-filter-probe.json与sqlite-vec.c上述函数。

未来验收：单独change明确索引的过滤能力，拒绝不支持的组合或在top-k之前正确应用，不能静默返回不满足WHERE的数据；覆盖多值IN、TEXT主键、距离上下界、buffer与graph路径。同步统一或明确k负数/上限差异；当前DiskANN负k空、4097接受，flat拒绝。不要把近似召回与SQL过滤正确性混为一谈。本迁移不改运行时代码。

## sqlite-vec 写入错误后的残留与类型约束不一致

状态：待单独处理。动态扩展同连接默认事务探针中，插入FLOAT metadata的INTEGER值报错，但随后的SELECT仍看到该rowid及vector。INSERT顺序在metadata验证前已写_rowids、validity、vector和auxiliary。另TEXT auxiliary的UPDATE接受INTEGER，BOOLEAN=4294967296经sqlite3_value_int接受并存为0。证据：changes/inventory-all-crate-features/reviews/sqlite-vec-write-probe.json，sqlite-vec.c的vec0Update_Insert、vec0_write_metadata_value和vec0Update_UpdateAuxColumn。

未来验收：独立change明确并验证语句失败的原子性，覆盖autocommit、显式事务、savepoint、OR REPLACE、metadata/auxiliary类型失败及多索引写入；核对失败后旧行保留和新行不可见。统一插入/更新类型约束，用int64验证BOOLEAN确为0/1。审查blob错误清理及返回码覆盖；本次只记录，不改运行时代码。

## sqlite-vec rescore忽略距离范围条件

状态：待单独处理。主planner把distance范围标为omit，但rescore_knn量化选候选、完整距离重算及输出均未执行范围条件。dynamic extension探针distance>100仍返回约2.83/5.66，rowid IN则正常。证据：sqlite-vec-rescore.c的rescore_knn及changes/inventory-all-crate-features/reviews/sqlite-vec-rescore-probe.json。

未来验收：单独定义rescore过滤发生阶段及候选不足的语义，保证返回值满足WHERE，覆盖上下界、oversample、无匹配及候选截断，不能靠SQLite复核已omit条件。顺带观察的零值量化>=0与公开函数>0差异、命令参数范围与schema差异需明确契约，不在迁移里修改。

## sqlite-vec 实验IVF槽位覆盖及过滤缺失

状态：待单独处理；仅显式SQLITE_VEC_EXPERIMENTAL_IVF_ENABLE=1启用，默认关闭。sqlite-vec-ivf.c删除将n_vectors减1但不压缩槽位，插入直接以n_vectors作为slot。实验动态扩展实测插1/2/3、删1、插4后rowid3读出4的向量且KNN丢失3；distance及多值rowid IN也被忽略。证据：changes/inventory-all-crate-features/reviews/sqlite-vec-ivf-probe.json。

未来验收：单独定义空槽分配或压缩策略，验证删除任意位置后插入保持全部存活rowid/vector一一对应；过滤在索引路径有效执行或明确拒绝。另审查find_or_create多余%d格式实参缺失、量化point输出、manual center/assign/clear量化与stride、DML/事务返回码。默认关闭不等于功能正确，不在迁移里修复或启用。

## sqlite-vec DiskANN完整距离未覆盖较小近似值

状态：待单独处理。diskann_search计算完整距离后复用diskann_candidate_list_insert；后者只在distance更小时更新重复候选，因此较小的量化距离保留，同时标confirmed。既有index-filter探针[3]*8到零向量返回8，flat返回8.48528099。证据：sqlite-vec-diskann.c上述函数与changes/inventory-all-crate-features/reviews/sqlite-vec-index-filter-probe.json。

未来验收：完整距离重算必须按定义覆盖并重新排序，测试近似值大于/小于真实值、多个metric及buffer合并。另独立核对int8 query fallback当float读取、int8量化缺夹取、rowid0 visited哨兵及错误吞掉路径；本迁移不修改算法。

## CLI stdio重放缓存遗漏关闭与并发请求关联

状态：待单独处理。main.rs的forward_stdio_line_to_leader仅在原文含initialize/session/load/session/new时调用cache_outgoing_acp_state；后者虽然支持grow/session/close和_grow/session/close，正常close消息被前置筛选挡住。cache_session_close_stops_replaying_it单测绕过forward直接调用cache，未覆盖生产入口。

另StdioReplayState只有一个pending_new，cache_incoming_session_id不核对response id便消费它；并发session/new可覆盖前一请求的cwd/mcp，其他含sessionId响应也可能关联错误。源码证据，尚未运行真实IDE/leader重连复现。

未来验收：单独change通过实际forward入口验证关闭后不重放；按JSON-RPC request id保留pending，覆盖多请求交错成功/失败响应和initialize/load并发。保持多会话顺序恢复及通知转发，不混入当前文档迁移。

## workflow journal API 与恢复边界待独立核查

逐包审阅发现：`fold_physical_entry`保留operation ID并接纳匹配的非pending替换，没有检查现有结果是否仍pending；普通重复log测试不覆盖重复operation完成。`record`/`begin_operation`自身不限制10000条，而restore/project限制条目数量；正常engine由MAX_HOST_CALLS限制，但公开Journal API调用方须继续核对。`agent(prompt)`没有map重载的trim非空校验。证据为crates/codegen/workflow/src/journal.rs与engine.rs，详见inventory-all-crate-features/reviews/workflow.md。后续单独变更应先核对生产调用方、补充对应边界回归并明确契约，本迁移不修改运行时代码。

## client-support 图片与剪贴板边界待独立处理

证据见inventory-all-crate-features/reviews/client-support.md：placeholder同轮新增路径未加入dedup集合；canonicalize/metadata/read未绑定同一文件句柄，读后cap不限制峰值读取；URI优先percent decode可能与literal percent文件名冲突。macOS fallback使用固定临时路径，AppleScript字符串转义与跨调用隔离待核对；Linux capture在child退出后join reader可能等待持管道的后代，非零回读变空会与空text匹配；Unix stderr fallback未检查dup失败。这些是源码边界，需各自独立复现、核对调用方与回归后处理，不混入文档迁移。

## marketplace 缓存与安装边界待独立处理

逐包证据见inventory-all-crate-features/reviews/plugin-marketplace.md：cache key不含branch，fresh TTL可复用同URL其他branch；本地递归copy跟随symlink，remote staged subdir缺canonical containment；AlreadyInstalled重装先删除旧安装再重试；changed只按HashMap首version；cache Git先wait后读stderr，安装update Git没有同等deadline。应分别核对调用方、构造回归并独立修正，当前文档迁移不改运行时。

## fast-worktree 独立审计项

来源：[逐文件审阅](changes/inventory-all-crate-features/reviews/fast-worktree.md) 与 [当前功能契约](changes/inventory-all-crate-features/specs/worktree-lifecycle/spec.md)。以下为代码确认的边界，尚未运行动态复现；按独立 change 处理，不混入迁移。

- 创建与复制：核对 Clean+Ignored Copy 重复制dirty项、Standalone早期返回未join、worker异常与保留receiver阻塞、取消后部分成功。分别通过真实入口验证内容、线程结束和取消结果，禁止以统计0证明快照成功。
- 删除与恢复：overlay以basename共用存储，缺统一ownership校验；metadata失败/删除失败仍清引用；默认DB注销与传入GC自定义DB不一致。分别覆盖同名dest隔离、拒绝路径时引用保留、失败可恢复及同一DB注销。
- 挂载与Git解析：根挂载前缀、UTF8 byte解码、受限/proc扫描、relative gitdir、unmerged三stage和SHA256索引。先核对支持范围及调用者输入，再以对应平台/真实Git仓库验证，不能靠synthetic happy path扩大保证。
- GC与DB：节流非跨进程锁、sweep读取后按旧id更新、rebuild同轮touch跨秒及已有dead记录不复活。明确并发和生命周期语义后单独实现，不将未来保证写成当前事实。
- 测试与诊断：Linux测试有固定全局路径/全局orphan清理、过时overlay布局、环境skip及错误只打印；bench修改source配置、A/B串行、手工JSON与UTF8字节切片。先建立隔离fixture并明确执行报告，再测实际快照/挂载；修正文案与输出也保留最小change。

## hunk-tracker 独立债务

依据inventory-all-crate-features的hunk-attribution规范：未知非文本首通知误作删除需定向复现；动作I/O失败统计/索引不回滚与缓存覆盖并发文件；拆分合并及直接clear的事件遗漏影响LOC；短读/增长与Git blob分配边界；patch换行/坐标；Git错误缓存、same-sync跳过、coalescing顺序与restore无事件；LOC持久化失败和无重放一致性。以上应分别立change处理，本迁移不修改运行时。


## Markdown 分隔符代码边界与暂存上限

逐包审阅用真实 `crates/codegen/markdown/src/latex_delimiters.rs` 独立探针确认：CRLF fenced close 不被识别，后续数学保持原文；多行 backtick span 内数学发生转换；转义 display dollar 仍参与合并。`scan_fence_close` 只接受 space/tab/LF，inline code 在 LF 重置，`classify_backslash` 未识别转义只消费一个字符。现有 36 项 delimiter 测试通过，但没有覆盖这些边界。

另 20000 backtick、关闭围栏后长空格、display opener 后 LF 加 20000 空格均能让 push 暂存约 20000 字节，超过注释声称的有界歧义尾部；尚未复现生产资源耗尽。后续独立 change 应先核对 parser/streaming 消费者，定义代码识别和转义契约，补 CRLF、跨行 code、长 run/whitespace 的实际入口回归及内存/输出推进边界。详细输入输出见 inventory-all-crate-features/reviews/markdown.md；当前迁移仅记录，不混入运行时修复。

## Markdown 渲染与验证边界待独立处理

来源：inventory-all-crate-features 的 markdown review、terminal-markdown 与 diagram-rendering delta。以下按不同责任拆分，当前仅记录，不改运行时。

- 映射与生命周期：streaming tail-relative line map、变换字节长度推进、SourceMap 包围非连续段、ParsedMarkdown 重复消费 metadata，应核对 pager/copy 消费者并分别建立真实入口回归。
- 样式与缓存：ANSI 普通 pretty 变换缺失、underline_color 丢失、polarity 绕过 cap/NO_COLOR、缓存 key 不含 Syntect 身份和单条超 256 KiB 仍插入，需分别确定输出一致性与资源契约。现有 502 测试通过不消除这些源码边界。
- 图形与表格：不可拆词导致表格超预算、Mermaid Diamond 画圆角框、多 self-loop 覆盖、fallback 窄窗溢出与总体内存无界、ER 空格 alias 测试弱断言。另有表格重复文本样式投影和 transform 跨 chunk 的源码疑点，尚未定向复现，不能直接认定生产错误。
- 开发工具：playground 错误路径未 RAII 恢复终端、u16 尺寸转换、旧控制说明；fuzz README 八组合与实际四路径不符且无 finish/equivalence oracle。后续最小 change 更新相应实现和验证，不把所有事项合并成一个重构。

## MCP 生命周期与输出边界待独立处理

依据 inventory-all-crate-features/reviews/mcp.md 及 mcp-integration delta。现有159项测试通过，以下源码疑点并未因此消失；按独立change处理，不混入文档迁移。

- 握手/恢复：guard disarm至结果发布取消窗口、stdio无restore、锁冲突仅notify、recover两次锁之间reset竞态、旧结果发布未校验revision；先用可控transport定向复现，不能从事件admission正确推导状态写入正确。
- liveness/bridge：旧watcher清新slot身份、task持client的生命周期、reader单向关闭、逐行输入与invoke数无独立cap，以及ACP per-tool大override可能被reverse默认budget先截止。逐项核对host所有者和真实停止路径。
- 工具/描述文件：分页无重复cursor/独立timeout，sanitize碰撞和旧文件保留、模型可见性与raw调试入口门禁、未处理audio/resourceLink/structuredContent；先确认产品契约与调用方，不擅自扩大输出或权限。
- 诊断/启动：ToolError提前返回丢失尾部诊断与输出标记、decode事件仅tracing与注释不符、日志无大小上限且同名截断、非UTF8 command lossy、header占位顺序替换。异常资源回收与scope/kill行为另立最小回归；本轮测试不覆盖所有OS和取消情形。

## memory 全包审阅发现的独立债务

来源：inventory-all-crate-features/reviews/memory.md；本次只记录，不修改运行时。

- 单行append规范化为纯标题，但搜索结构过滤剔除纯标题；应独立验证并统一持久笔记可检索契约。
- 嵌入响应未核对条数/index/数值维度，回填zip配对；cache身份检查不保护等待期间的chunk文本版本。另需处理claim取消恢复、owner释放及dirty失败重试。
- 工作区身份去掉远端主机，读取边界覆盖整个memory根，watcher全根事件可能进入当前索引；需明确跨工作区共享的实际权限和身份要求。
- Dream输入阈值在整份session追加后检查；输出覆盖、未处理旧session的后续mtime门控、无有效输出保留新锁时间、清理与并发写入协调应作为独立生命周期问题处理。
- storage文件名组件及session ID字节切片、flat列表重复、默认分块与自定义配置接线、GC跨进程协调和读路径竞态分别评估；不能在文档迁移中混入架构改造。


## pager-minimal 全包审阅发现的独立债务

来源：inventory-all-crate-features/reviews/pager-minimal.md；本次只记录，不修改运行时。

- cap footer 使用 set_style 而非清空 glyph，短提示后是否遗留原行文字需定向复现；部分终端写入失败重试并非事务输出，模型 finalize/mode 也不回滚。
- startup 路径按字符而非终端 cell 布局、面板最小5行与渲染2行 guard、极大数量 as u16 等窄屏/大输入边界应分别验证。
- transcript 单条渲染和最终整文件写不受8ms硬预算约束，thinking临时开关无panic恢复；临时文件权限及清理由宿主继续核对。cap buffer 不代表完整布局和语义链接计算资源有硬上限。
- 源码中 dropdown above 与实现 below、近全屏modal 与18行目标的注释差异另行整理；字符串guard不能替代依赖/调用约束。已提交块的宿主后续更新必须结合 pager 生命周期继续审阅。


## ratatui-inline 全包审阅发现的独立债务

来源：inventory-all-crate-features/reviews/ratatui-inline.md；文档迁移不修改运行时。

- OurFrame/Frame unsafe transmute仅检查size，需独立确认依赖布局契约；部分doc样例测试上游Terminal，不能作为fork覆盖。
- 零屏高、超高viewport、viewport宽与last_known_area宽不一致，以及u16普通加减/len转换缺少边界防护；分别定向复现，不混入统一重构。
- regions满屏恢复top line未重发链接；重叠链接在frame和insert分别后项/首项优先；cleared helper的Rect高度及上游diff使用需单独验证。
- IO失败在输出、viewport/Inline高度、buffer swap等不同阶段留下状态；同步包装和OSC8 close尝试不保证事务性恢复。明确真实writer故障模型后添加最小回归。
- legacy resize注释声称RIS但实现清屏序列，Fixed clear可能超rect；示例窄宽/极小屏高及错误退出raw恢复、递归poll事件积累属于独立维护范围。


## ratatui-textarea 逐包审阅发现的独立债务

来源：`crates/codegen/ratatui-textarea/src/textarea.rs`及`examples/textarea_demo.rs`，本次仅记录，不修改运行时代码。

- 公共选区与restore_elements缺少统一边界/身份校验，非法UTF-8位置可导致slice panic；另开输入不变量change处理。
- Stateless render未统一垂直裁剪；极窄/超大量行的u16几何与满行虚拟cursor需单独验证，当前随机测试不覆盖这些边界。
- undo group只比较元素数量而非metadata内容，快照也不覆盖完整UI状态；需要先明确undo契约再修改。
- scrollbar thumb默认位置与cursor-follow可能不同；内部鼠标映射缺右下范围限制，宿主路由与公开方法应明确责任。
- 示例中键粘贴不实际定位cursor、初始化失败缺终端恢复guard、host metadata不随撤销同步；均为示例独立问题，不混入规范迁移。

## dagre_rust 独立债务

源码审阅发现greedy去环未实现、未知ranker无fallback、acyclic undo未删除临时反向边；feasible_tree独立处理不连通图可能不终止，部分重心零权重会NaN，几何/rank存在截断及资源边界。来源third_party/dagre_rust/src/layout，详细证据见reviews/dagre_rust.md；需按调用方实际输入前提拆分验证与修复，不混入本次规范迁移。现有唯一单测不足以覆盖布局pipeline。

## mermaid-to-svg 独立债务

来源：inventory-all-crate-features/reviews/mermaid-to-svg.md；本次只登记现状，不修改运行时。

- SVG 样式字符串插值与固定 marker ID：分别验证属性转义及多图嵌入冲突，调用方栅格化限制不能代替库契约。
- 嵌套子图 ID 重复、递归环、同端点多边覆盖和几何非有限值：按实际可达输入建立定向复现，再分别修复；现有 smoke 测试不足以证明边界安全。
- 类图反向箭头、ER/requirement 平行边、sequence 生命周期与局部语法缺口：先明确支持范围，以图表类型拆分，不混入规范迁移。
- 文本二次分隔符拆分可能破坏字素、部分布局修正未覆盖所有方向；补充具体 Unicode 与方向回归后处理。实验 port 当前不可达，问题不得误写为公开路径行为。

## test-support 部分审阅发现的独立债务

`crates/codegen/test-support/src/headless.rs::stderr_tail`以字节偏移切UTF-8，错误报告本身可能panic；单独补充多字节边界复现后修复。headless全量read_to_end与counting_server无大小/超时上限也需按测试输入约束独立评估，本次不改实现。

### sampler 凭据诊断日志最小化（逐包审计发现，独立处理）

来源：`crates/codegen/sampler/src/client.rs` 的 `SamplingClient::new` / `post`。非法静态 api_key 的 debug 记录完整值；client_post 的 info 记录 Authorization 前20字符和 x-api-key 前12字符。短 key 可能完整落入前缀；base_url 日志也没有统一移除 query。需要独立变更梳理日志字段与现有归因需求，用不包含秘密的证据替代，并验证失败/重试路径。当前只记录实现事实，不在 OpenSpec 迁移中修改运行时代码。

### sampler 错误传输保留重试否决信息（独立核查）

`crates/codegen/sampler/src/events.rs` 的 `SamplingErrorInfo::from` 不携带 API should_retry，只保存未应用 veto 的 is_retryable；`sampling_error_from_info` 用后者重建 should_retry。API500 且 should_retry=false 的往返会丢失该否决。另有 Http 降 EventStreamError、Idle 秒数由文案提取。需在独立变更中追踪 actor/L2 的真实往返路径和现有失败用量传递，验证可达影响并保持结构化语义；当前不修改运行时。


## sampler 重复请求 ID 的回收隔离

证据：`crates/codegen/sampler/src/actor/mod.rs` 的 Submit 会按同一 ID 替换 token 并取消旧任务，但 run 收到旧任务正常返回仍按 ID 无条件 remove；旧任务退出可误删新任务登记，导致查询/显式取消无法定位新任务。JoinError 分支也没有按任务身份清理。后续独立 change 应核对 request_task 退出与 handle CancelOnDrop 的 ID 复用时序，明确重复 ID 契约并补并发回归；本次仅记录，不修改运行时代码。


## AgentBuilder 的预加载子代理目录与 Primary 组合

`crates/codegen/agent/src/builder.rs` 在with_preloaded_subagent_names设置可用名称后跳过live discovery，但Primary保留Task时仍expect完整live_subagents，可能panic。后续独立change应核对公开入口及host调用，明确预加载仅允许child或提供完整Primary描述；本次记录事实，不扩展文档迁移范围。


### 插件信任持久化与刷新并发边界

证据：`crates/codegen/agent/src/plugins/trust.rs` 的 revoke_trust 先删内存再截断重写，失败不回滚；无跨实例写锁。`plugins/local_refresh.rs` 的临时目录只以PID区分，sweep_stale只检查一小时年龄、不检查活跃进程，且目录替换由两次rename构成。后续单独change定义失败一致性与并发刷新契约并做故障注入；本次事实盘点不修改实现。


### 插件代理限定名的列表与加载一致性

`crates/codegen/agent/src/discovery.rs`：all_subagents_with_plugins_and_home保留原生冒号名后跳过同名插件项；by_name_in_cwd_with_plugins_and_home却将冒号namespace保留给插件。需独立change验证同qualified名称的展示描述与实际加载一致，并覆盖插件toggle追加路径；现有qualified测试只覆盖lookup。本次只记录，不修改实现。


## chat-state 首用户文本查询的注释与实际选择不一致

证据：crates/codegen/chat-state/src/actor/queries.rs::get_first_user_text 使用find_map，第一条User无首Text时继续查后面的User；命令及函数注释宣称第一条User非Text时返回None。审计仅记录当前实现，不修运行时代码。后续独立change应核对handle及shell调用方对会话标题/初始查询的期待，明确选择语义，并覆盖首User图片开头、同条后续Text、后续User首Text和空content场景。本项不是本轮规范迁移的附带修复。


## chat-state 控制上下文的 trajectory 投影不一致

证据：timeline.rs 的 active_control_contexts 及主surface释放逻辑在StepEnded激活AgentRole、GoalDefinition、PlanPhase，trajectory.rs 的TrajectoryProjector只释放前两层；其Control投影未对应处理retired_context_layers。后续独立change应统一展示投影与主fold的激活/退休语义，并覆盖PlanPhase step切换、退休pending与active层。此项为静态确认差异，尚未证明具体UI症状，本轮不修改运行时代码。

## chat-state Sideband 的 SurfaceId 来源覆盖缺口

证据：sideband.rs 的surface_id_exists仅识别Messages、ImageProjection、Control；timeline.rs 的appended_message_items及surface追加还包括Input Consumed和携input的Notification Consumed。后续独立change应核对真实sideband构造调用方是否传入这些ID，并补齐来源验证及正反例；不能绕过parent校验解决。本轮仅记录实现覆盖差异，不宣称已复现所有用户路径。


## pager-render 独立债务：配置颜色与字符串边界

证据：`crates/codegen/pager-render/src/appearance/config.rs` 的 OptionalColor 将 BLACK/WHITE/GRAY 解析为命名 ANSI 色，序列化却写 unknown，无法按同一解析器往返；Indexed 还会转换为 RGB 丢失表示身份。parse_hex_color 使用字节长度及固定索引切片，非 ASCII 输入的 UTF8 边界需定向验证。`src/util.rs::parse_schedule_interval_secs` 同样混合 Unicode whitespace 与字节切片，并用未检查乘法计算秒数。后续分别建立最小 change，核对配置入口及定时调用方后确定错误语义，补充 Unicode、溢出和往返场景。本次仅记录；现有全包测试通过不证明这些缺口已覆盖。

## pager-render 独立债务：渲染坐标与字素边界

证据：`crates/codegen/pager-render/src/render/safe_buf.rs` 只检查 y 和 x 右界，没有检查 x 左界；`highlight.rs` 使用直接 buffer 索引及 u16 转换，single_row 分支不执行 wrapped 的纵向裁剪；`line_utils.rs::truncate_str` 逐 char 处理，移除尾零宽字符后再加省略号可能超预算。后续按调用方约束分别验证非零原点、resize、超长行、组合字素，不把多个问题混入文档迁移或简单 UI 改动。

## pager-render 独立债务：GBOOM 随机值与世界边界

证据：`crates/codegen/pager-render/src/gboom/assets.rs::XorShift64::next_f32` 当前实际范围为 [0,0.5)，与 [0,1) 注释不符；现有范围测试过宽。`game.rs` 的近战完成只复查距离、不复查 LOS，移动碰撞只判终点；`frame_png` 没有尺寸上限，依赖外部尺寸辅助。后续先确认该功能保留范围，再分别确定算法/资源契约与验证，不能借 main 的删除候选消息提前省略当前代码事实。本次不改变游戏行为。

## pager-render 独立债务：同步探测与输出资源界限

证据：`terminal/tmux_probe.rs` 的后台 pipe drain 使用无字节上限 read_to_end，清理 child.wait 无独立期限；`theme/system_appearance.rs` 在异步轮询任务中同步detect，Drop abort不能立即中断该调用；`render/draw.rs` 通道及writer缓冲没有背压上限。后续按各自生命周期分别建立 change，设计明确的超时/容量/退出契约及故障注入。本次不把局部deadline或flush接受等同整体完成保证。

- **Shell bundle 发布与资源边界**：`shell/src/bundle.rs::extract_bundle_archive`逐项直写后验证version并发布manifest，无事务回滚；词法路径校验未验证磁盘父路径/目标链接，50MiB仅约束Regular声明大小。后续独立审计调用方可信来源、磁盘身份和失败恢复，再决定加固；当前仅源码边界，未动态复现链接逃逸或解压资源耗尽。

- **Bundle 损坏缓存恢复与下载体积**：`extensions/bundle.rs::bundle_cache_is_fresh`拒绝坏JSON，但后续`bundle.rs::extract_bundle_archive`仍先读取坏manifest而失败，不能视为自动修复。`remote/client.rs::fetch_bundle`成功响应整段收集且未设应用层大小上限。后续单独定义损坏缓存恢复和网络体积预算；当前未运行故障注入。

- **Bundle 后台同步可达性及gate生命周期**：当前工作树maybe_sync_bundle_in_background仅定义/注释引用，未发现生产调用，不能认定自动同步已启用；helper的AtomicBool仅正常await返回后复位，不覆盖取消/panic或显式sync并发。后续单独确认是否保留入口，再验证生命周期，当前不接线、不删除。

- **设置清除哨兵未删除旧键**：settings_writes.rs的screen_mode空串与cancel_subagents_on_turn_cancel=ask转None；client-support/ui_config.rs省略None，persist.rs深合并保留未序列化字段，已有值因此保留。后续单独定义显式删除语义并做真实文件回归，当前静态核对，不混入修复。

- **updates追加错误的提交状态不确定性**：shell session/storage/jsonl/mod.rs::append_update_with_bookkeeping把append_update_to_file全部错误包装为NotCommitted，但底层write_all/flush/文件同步/目录同步失败可能发生于部分或完整行已写之后。后续独立审计调用方是否基于该分类重试，注入短写、flush与两级sync失败，确认是否重复显示记录或误计Summary；本轮仅源码确认分类与磁盘事实不等价，未动态复现重复，不混入实现修复。


## Rewind恢复历史读取失败缺少显式失败信号

证据：workspace/src/session/file_state.rs的ensure_historical_loaded在读失败时只告警并保留lazy_source，get_rewind_points仍返回内存Vec；shell/src/session/actor/rewind.rs的recover_pending_rewind继续按该Vec构建并持久化next_points，replace_rewind_points会清空lazy_source，之后清除intent。应独立验证坏ledger或读取错误时是否导致历史投影被部分集合覆盖，并决定如何向恢复调用方传播完整性失败。当前是源码确认的错误信号缺口与风险链，尚未故障注入复现数据丢失；禁止将修复混入文档审计。


## Session清理TTL上界缺少验证

来源：shell/src/session/persistence.rs::resolve_cleanup_ttl_days只检查正整数后as u32，4294967296会变0；adapter以转换后天数计算cutoff。后续独立明确配置合法范围并验证越界拒绝或回退，检查启动清理调用链影响。当前仅静态转换证据，未运行真实删除或声称已发生误删；不混入运行时修复。

TTL调用链补充：acp_agent.rs initialize后台无skip清理，load_session后台带当前目录清理，两者共享Once且不await；首次执行的参数固定，后续skip不能覆盖。应独立验证过期会话恢复与初始化清理的时序，结合writer lease取得时机判定保护范围；当前没有动态复现误删。


## 普通ACP pending切换失败与flush确认语义

persistence.rs::run中maybe_merge_notification先替换pending，写出旧通知NotCommitted仅告警不恢复；FlushAndAck无Result且flush失败仍send(())。Grow可先于旧ACP pending落盘，Sideband亦不因flush失败停止。后续独立核对消费者是否依赖可靠顺序/确认，使用mock storage故障注入确认丢失与重排影响，再决定契约及实现调整。本次仅静态事实，未运行丢失复现，不混入修复。


## 标题fallback长度与提醒回退

helpers/session_title.rs的fallback仅限制10词，单长词可超过canonical标题160字符；title_source_text在提醒剥离为空后回退原文。后续独立定义fallback长度和仅系统提醒输入的语义，验证采纳失败与生成输入。当前为源码边界，未运行provider或动态采纳复现，不混入修复。


## Context异步结果与实时状态身份

pager dispatch/status.rs的handle_context_info_complete不检查session/revision，更新实时context先于nonce；SessionInfo有效身份结果也先更新实时状态再检查nonce；两类失败nonce0无session检查。task_result直接分发，已有旧epoch测试只证明弹窗不被填充。后续独立change核对请求生命周期与Agent复用，故障注入关闭/重开、切session和乱序响应，确定实时计数是否回退及失败是否跨session显示；当前为静态边界，未动态复现，不混入修复。


## Usage复制动作的Agent归属

CopyUsageModalValue仅携带行索引，dispatch_copy_usage_modal_value在所有agents里find_map，copy_value也不检查active_tab。后续独立核对多Agent保留Usage弹窗的可达性，使用不同会话同行值复现并确定action是否应携带owner或仅定位active agent。当前源码边界已写inventory契约，未实际操作剪贴板，不混入修复。


## 审计后续：Shell补全失败后的pending Tab释放

证据：pager app/root/effects/mod.rs的FetchShellSuggestions将传输/JSON/parser失败映射为CancelComplete；root/dispatch/task_result.rs该分支只trace，无Agent/generation身份或pending清理。agent_view/shell_completion.rs在run_tab_on_load且pending仍current时去重。因此一次失败后，在没有编辑/失效操作的情况下重复Tab可持续不发请求。后续独立change应设计带请求身份的失败收尾，验证失败后可重试、旧失败不解除新请求pending以及禁用自动建议时的路径；本次仅记录，不修运行时代码。


## du测试的Windows符号链接helper

crates/codegen/pager/src/du_cmd/tests.rs中symlinks_are_billed_as_entries_and_never_followed无平台cfg却调用仅cfg(unix)存在的symlink；Windows分支定义symlink_dir/symlink_file而未用于该测试。后续独立核对Windows测试目标编译并按平台接入正确helper。本次仅源码发现，未执行Windows编译，不混入实现修复。


## 待核对：设置持久化旧失败覆盖后续修改

本迁移worktree的PersistSetting逐请求spawn，SettingPersistFailed仅携带key/rollback_value/error；task_result处理无revision或当前值检查即回滚。需独立验证连续修改及乱序完成时的内存/磁盘一致性，并核对底层锁和模型companion effects；本次仅记录源码风险，不宣称已复现，也不混入文档迁移修复。证据：pager/src/app/root/effects/mod.rs与dispatch/task_result.rs。


## 待核对：CLI设置默认值与回滚helper漂移

迁移worktree的show_tips在registry读取None为false，setter却按true生成旧值；auto_update读取/setter为true，pr13_effective_default却为false。需要独立统一resolver、显式覆盖和失败恢复的Option语义，并补None/Some全矩阵验证。本次只记录源码差异，不混入实现修复。证据：pager settings/registry.rs及app/root/dispatch/settings/setters.rs、ui.rs。


## 待核对：设置reset测试的非默认前置未成立

迁移worktree的move_setting_away_from_default将show_tips设false、thinking设true（等于目录默认）；permission改active session而非默认；default_model只改agent模板。调用方未断言偏离默认，rollback守卫仅遍历实际effects。因此测试可未触发某些reset/rollback而通过。独立修复应验证每项真实前置与恢复后值，不能只加分支存在断言；auto_light的rosepine-dawn有效性另核对parser。此次只审计，不混入运行时或测试修复。证据：pager/src/app/root/dispatch/tests/settings.rs。


## 待核对：设置主题预览的全局关闭路径

Settings input的F2/Ctrl+,/Super+,直接返回Close，apply_settings_outcome直接清modal；Esc picker路径则返回原主题Preview。需独立复现先预览再快捷关闭是否遗留live主题，并决定统一退出语义。本次仅记录源码分支差异，不混入修复。证据：pager/views/settings_modal/input.rs、app/agent_view/mod.rs。


## 待核对：设置picker过高choice与group小窗口焦点

render.rs的picker_scroll_offset在单choice高于available_h时继续越过当前项，后续防御visible_end不一定恢复焦点；block_rect保留完整layout高度而非viewport交集。Group从首child绘制且无scroll_offset，键盘可选不可见child。需独立构造小窗口/长描述buffer测试，核对焦点可见性与命中边界，不能依注释声称已安全裁剪。本次仅记录源码风险，不混入修复。


## 待核对：设置breadcrumb hover在重绘前被清除

`handle_settings_mouse`在Moved进入breadcrumb时设置`breadcrumb_hovered=true`并返回Changed；下一次`render_settings_modal`进入Enum、Group或Editor分支后先调用`reset_hit_rects()`，该函数同时将`breadcrumb_hovered=false`，随后标题样式读取的已是false。现有测试只验证事件后的state翻转，并明确移除了颜色断言。需独立增加事件后重绘的Buffer测试并调整geometry清理与hover生命周期；此次只记录审计事实，不混入修复。证据：pager/views/settings_modal/input.rs、state.rs、render.rs及tests.rs。


## 待核对：跨batch focus CSI泄漏为按键

`CsiFragmentFilter`为了让用户键入的单独`[`立即显示，不跨batch保留Bracket；bare Esc同样在当前batch直接输出。因此终端focus report若恰在Esc之后切分到下一次drain，后一批`[I`或`[O`会作为普通按键进入输入链，既不触发FocusGained/Lost，也可能修改composer。现有测试明确锁定该known limitation。后续独立change需结合输入batch来源设计可区分的短时holding或更低层字节重组，并验证裸Esc、typed bracket延迟以及SSH focus事件；本次不改运行时代码。证据：pager/src/app/csi_filter.rs。


## 待补：leader cluster双向驱动场景

`app/leader_cluster/scenarios.rs::two_clients_share_session_and_stream_both_ways`的名称和注释声称driver/viewer角色会在下一轮翻转，但第二轮代码仍调用原client `a.act(Action::SendPrompt(...))`，viewer `b`从未驱动turn。当前只证明同一driver向viewer的replay与live fan-out，不证明已attach viewer提交后原driver会收到且双方各一次。后续独立调整测试时应让viewer实际发送第二轮，并保留session身份、inference次数和双端exact-once断言；迁移只登记测试证据缺口，不修改测试。


## 待核对：fork directive尾随空白契约

`slash/commands/fork.rs::ForkArgs.directive`注释称whitespace-trimmed，但`parse_fork_args`只在初始输入及每个已识别flag后调用`trim_start`，最终`rest.to_string()`保留尾随空白；与Plan、Workflow等命令的完整trim不同。后续独立确认首prompt是否应保持用户尾随空白，并补flag-only、未知flag、Unicode空白及尾随空白测试后统一注释或实现。本迁移按当前代码记录，不修改解析器。


## 待核对：slash工具集unknown注释与fail-closed实现相反

`slash/registry.rs::set_available_tools`的API note把`available_tools=None`描述为“tool list unknown, show everything”，但同文件字段文档、`tools_satisfied`实现及`tool_gated_command_hidden_when_toolset_unknown`测试都明确对声明required_tools的命令隐藏并禁止dispatch。后续独立改动应统一注释，并核对旧Shell丢失`meta.tools`时保留旧Some集合是否仍是期望策略；本迁移按实现记录unknown为fail-closed，不修改运行时代码。


## 待核对：slash参数完整性示例与Model picker行为漂移

`slash/command.rs::SlashCommand::args_required`的二元表仍以`/model <id>`示例takes_args=true、args_required=true并声称空参数阻止提交；实际ModelCommand未覆写args_required，`slash/mod.rs::selector_command_is_complete_without_args`也锁定`/model`与`/model `完整，run会打开picker。后续独立修正文档示例并确认是否还有调用方依赖旧表；迁移按实际optional selector记录，不修改代码。


## 待核对：inline slash前导Unicode空白边界

`slash/mod.rs::scan_inline_slash_tokens`注释称slash可由whitespace前导，但实现通过前一byte的`is_ascii_whitespace`判定，name终止与args phase却使用Unicode `char::is_whitespace`。因此不间断空格等非ASCII空白后的`/command`不会识别或高亮。后续独立决定token边界应统一为ASCII还是Unicode，并增加多字节空白、路径slash及cursor byte range测试；迁移按当前ASCII前导事实记录。


## 待核对：slash MRU跨进程写入覆盖

`slash/mru.rs::persist_async`的OnceLock writer只在单进程内串行，`MruSnapshot::write`固定使用共享`slash-mru.json.tmp`且把各进程启动时读取的完整map直接rename覆盖。多个共享GROW_HOME的pager进程可同时写同一temp/目标，既可能互相删除temp或rename失败，也可能以last-writer-wins丢失另一进程的新command timestamp。后续独立设计跨进程锁、read-merge-write或每写唯一temp，并验证两个进程交错touch；本迁移不改MRU格式或IO。


## 待核对：ACP skill包装器注释仍声称pager展开SKILL.md

`slash/acp_command.rs`文件头与AcpSlashCommand文档仍写pager读取SKILL.md、substitution并client-side处理，测试名也保留`run_skill_substitutes_skill_dir`；实际run从不访问skill_path，只生成raw `/skill args`单Text block交Shell展开，missing file同样通过。后续独立统一模块/struct/test命名与当前所有权，避免维护者在pager重复实现加载；迁移权威契约按Shell-owned expansion记录。


## 待核对：scroll log秒级默认文件名可截断同秒捕获

`input/scroll_log.rs::default_log_path`只使用UTC到秒的时间戳，`open_writer`以`File::create`打开。同一GROW_HOME内在一秒内关闭后重开debug log，或多个pager进程同时首次滚动，可选择同一文件并截断/争用，破坏flight recorder诊断证据。后续独立改为更高精度、PID/nonce或create_new重试，并增加同秒多recorder测试；迁移不改变日志格式与路径。


## 待核对：BackTab compact标签重复Shift

`input/key.rs::fmt::Display`先按modifier输出`Shift+`，随后对KeyCode::BackTab固定再输出`Shift+Tab`；因此canonical编码BackTab+SHIFT会显示`Shift+Shift+Tab`，而display_pretty已有去重逻辑。现有测试覆盖三种Shift-Tab匹配但没有覆盖compact标签。后续独立统一BackTab格式并为BackTab无modifier、BackTab+SHIFT、Tab+SHIFT锁定compact/pretty输出；迁移不改快捷键展示。

## 待核对：scripted scenario普通解析清单不覆盖目录

`tests/scripted_scenarios.rs`有40个ignored运行包装器，但三个普通parse测试只具名解析29份YAML；`tests/scenarios`当前共有45份YAML。新增或未列入的场景可能直到显式运行ignored suite才暴露语法或空steps错误，且Rust包装器注释与YAML步骤也没有一致性检查。后续独立从目录发现全部YAML并解析，另验证每个运行包装器引用存在且无孤立/重复映射；迁移只记录当前清单事实，不修改测试发现机制。

## 待修复：Doctor tmux超时夹具没有建立sleep进程

`tests/doctor_early_dispatch.rs`的三项tmux超时/后代回收测试把child PATH覆盖为只含fake-bin，fake `tmux`脚本却调用裸`sleep 30`；该目录没有sleep。本轮隔离探针确认当前`/bin/sh`在这种PATH下`type sleep`与`sleep 1`均command-not-found并退出127。因此快速返回、管道关闭和PID消失可能只是fixture命令未启动，不能证明Doctor timeout或kill。后续独立将脚本改用经确认的绝对sleep路径或在fake-bin提供受控helper，先断言descendant真实存活/阻塞，再触发Doctor并验证进程组回收及无配置写入；本迁移不修改测试。

## 待修复：PTY屏幕文本定位没有使用终端显示宽度

`tests/pty_e2e/common.rs::locate_screen_text`先以`str::find`取得字节位置，再对前缀执行`chars().count()`作为SGR mouse列。宽字符、组合字素或零宽字符位于目标前时，这个Unicode标量数量不等于终端display cell，后续mouse helper可能点击错误列。后续独立使用与终端renderer一致的宽度计算，并覆盖CJK、emoji、组合字符和水平裁剪；迁移按当前标量计数事实记录。

## 待修复：scrollback PTY helper忽略footer等待失败

`tests/pty_e2e/common.rs::drive_to_scrollback_with_turn`确认响应、prompt和turn sentinel后注入Tab，但对footer sentinel的`wait_for_text`结果使用`let _`丢弃；`scroll.rs::spawn_bottom_pinned_marker_scrollback_with_env`、`basename_path_demo_pty.rs`以及`drag_over_gap_rows_does_not_freeze_head_pty.rs`、`drag_from_chrome_stays_block_pty.rs`、`drag_enters_content_from_gap_pty.rs`与`drag_from_above_prompt_strip_pty.rs`也直接丢弃相同等待结果。调用路径可以在未证明scrollback焦点或footer出现时继续，后续断言若只依赖已有屏幕文本可能产生假阳性。后续让helper与直接用例传播等待错误，并增加Tab未生效的反例；迁移不修改测试运行时。

## 待修复：selection PTY将多个OSC52写入拼接后验证单次选择

`drag_select_wheel_scroll_extends_pty.rs`、`nested_quote_drag_copy_excludes_bars_pty.rs`、`drag_over_gap_rows_does_not_freeze_head_pty.rs`、`drag_from_chrome_stays_block_pty.rs`、`recap_header_not_in_selection_pty.rs`、`read_tool_header_selection_copies_path_only_pty.rs`、`quote_block_raw_mode_copy_keeps_source_pty.rs`、`verb_group_header_drag_copy_pty.rs`及`quote_block_drag_copy_excludes_bars_pty.rs`均收集多个OSC52 payload后以换行`join`再验证anchor、跨行首尾、包含或排除项。verb-group用例会按历史payload数量隔离每次gesture，但同一次gesture新增的多个payload仍会拼接。若一次手势错误地产生两次或更多clipboard写，各payload分别含部分目标，拼接结果仍可能满足“跨gap”“whole block”、extension、path-only、raw-source、header/member separation或quote prefix断言，无法证明单次最终copy的原子范围。后续明确选取手势release对应的最后一次非空payload，断言写入次数/顺序，并在必要时区分selection preview与commit；迁移按现有joined证据记录。

## 待修复：Minimal会话目录fixture可能选择错误会话

`tests/pty_e2e/common.rs::session_dir`最多等待十秒后直接返回`sessions`下第一次`read_dir`得到的目录，不核对cwd编码、session id、创建时间或期望Summary。若隔离HOME中同时出现多个session目录，测试可能读取或恢复错误实体，且目录遍历顺序没有稳定保证。后续让spawn路径返回明确session identity或按已知cwd/Summary校验唯一目录，并增加多目录夹具；迁移按当前首目录选择事实记录。

## 待修复：trackpad flood以未观测的arrival compression跳过under-travel

`tests/pty_e2e/trackpad_flood_does_not_under_travel.rs`在最终travel低于200且captured frames不超过12时打印`SKIP(compressed burst)`并正常返回。测试只看到最终marker travel和frame count，没有记录40个报告在pager端的arrival gaps、分类或每次flush，因此不能由低frame数排除实现回归同样产生低travel；标题所称does not under-travel在该分支没有被断言。后续由scroll flight recorder或测试专用观测记录实际arrival/classification/flush原因，再只对确认的host stall作显式测试跳过；回归路径必须仍失败。本轮迁移按当前条件成功事实记录，不修改测试。

## 待修复：Bash Tab前异步补全负断言没有等待窗口

`tests/pty_e2e/bash_mode_tab_completion_dropdown.rs`在注入`!cat SUGGEST`后立即读取一次screen并断言两个文件名缺失，随后马上发送Tab。异步as-you-type结果若稍后到达，该快照仍会通过，无法单独证明`GROW_SUGGESTIONS=0`时Tab前始终没有候选。后续以超过provider debounce/fetch预算的负等待断言确认dropdown和候选持续缺失，再发送Tab并核对请求触发因果；当前迁移只记录即时snapshot事实。

## 待修复：queued Bash编辑测试未排除原命令重复执行

`tests/pty_e2e/verify_bashq_claim3_edit_keeps_bash.rs`成功路径要求`CLAIMTHREE_EDITED_OK`出现且model user blobs不含`CLAIMTHREE`，但最终没有断言`CLAIMTHREE_ORIG_OK`不存在，也不记录Run block或shell执行次数。若旧row与编辑后row都保留Bash kind并各执行一次，现有断言仍可通过。后续核对queue identity替换，要求原输出和第二个Run block不存在、编辑后输出精确一次，并检查持久队列只保留一个相同identity版本；本轮不修改运行时或测试。

## 待修复：verb group refold只抽查部分成员消失

`verb_group_fold_expand_collapse_pty.rs`展开时确认a1/a2/a3三项，二次Left折回后只断言a1和a2消失；`verb_group_settings_toggle_pty.rs`关闭时确认t1/t2/t3三项，重新开启后只断言t2消失；`verb_group_header_drag_copy_pty.rs`的header复制只排除dragme1而不排除dragme2。若最后一项成员残留在折叠视图或复制范围中，现有测试仍可通过。后续对每个已展开成员逐项断言折回后不可见，并让header selection排除全部成员或精确等于聚合label；当前迁移只记录已抽查的成员集合。

## 待修复：Edit HL PTY未锁定目标语法样式

`edit_hl_inplace_refresh_pty.rs`以包含两个文本marker的整行`StyledRun` JSON前后不等作为upgrade证据，任何颜色、modifier、run切分或重复匹配行集合变化都可满足；最终HTML只要求全页存在任意`style=`或`<span`，没有定位目标token。测试还固定写`/tmp/edit_hl_video`且不清目录，多数artifact写错误被忽略，并发或旧文件可能污染演示产物。后续对目标token的预期foreground/background/modifier或syntax scope做结构化断言，artifact目录使用本次唯一临时路径并显式报告写入结果；当前迁移不把任意style变化称为正确highlight。

## 待修复：same-file Edit merge UI测试未验证全部修改事实

`edit_merge_parallel_pty.rs`只看一个`+2/-2` header且不读文件；`edit_merge_sequential_pty.rs`虽构造三个hunk，展开后只断言第一、第三marker与至少一个gap，未检查第二marker或最终文件，第四次edit也只看header。若工具失败、某个中间hunk丢失或diffstat与文件事实漂移，当前可见断言可能仍通过。后续逐项等待tool expectation、读回最终文件的全部replacement，展开时检查每个唯一marker及准确gap顺序，并区分UI聚合正确与runtime edit成功两个验收层；本轮只记录可见渲染证据。

## 待修复：queued message wire唯一性按blob数量而非出现次数

`queued_message_renders_once_not_twice.rs`用`users.iter().filter(|u| u.contains(QUEUED_TEXT)).count()`断言等于1；`cancel_before_task_completion_defers_auto_wake_until_user_prompt`同样统计包含POST_CANCEL的request body数；`auto_wake_cancel_preserves_queued_user_prompt`更只要求任意body和resume全文包含CLARIFY。若同一个blob/body内重复两次，或CLARIFY存在多份，结果仍可通过。屏幕侧只统计当前可见行，也不覆盖off-screen或durable重复。后续对每个结构化user item计算substring occurrences并要求总和为1，同时检查请求身份和持久Timeline投影；迁移按“匹配blob/body存在”记录，不声称wire或恢复文本精确一次。

## 待修复：auto-compact PTY以任意首个非空行代理status位置

`auto_compact_top_row.rs::first_content_row`只找screen中第一条trim后非空的行；两个用例据此把row 0解释为status bar和top padding消失，但不检查该行内容或样式，也不读配置确认用户compact值未被写回。若其它banner、提示或残留内容占据row 0，短屏断言可假阳性；若状态位置错误但仍有任意内容，也无法区分。后续定位稳定status标识或styled region并断言其row，读回配置确认resize前后不变，同时覆盖16/17/20/21边界；当前迁移只记录first-nonblank事实。

## 待修复：background reap PTY只跟踪shell PID而非sleep后代

`background_task_reaped_on_quit.rs`执行`echo $$ > pid; /bin/sleep 600; echo rc=...`，pidfile记录解释该命令的shell PID；由于sleep后还有命令，shell通常等待子sleep而非以其替换自身。测试在pager SIGINT后只轮询`kill(shell_pid, 0)`失败，没有取得或检查sleep PID。若退出路径杀死shell但遗漏其setsid/process-group内后代，sleep被重新托管后仍可能存活而测试通过。后续让脚本显式后台启动sleep并记录`$!`，同时记录shell/session/process-group身份，退出后分别等待所有PID ESRCH并检查无同组后代；当前迁移只保证记录的shell PID不再可见。

## 待修复：basename路径demo不能区分Block Viewer与fallback展开

`basename_path_demo_pty.rs`先尝试Ctrl-F，但15秒未见路径时会Esc后Enter展开；最终仅要求`screen_shows_full_path`为真。该helper除完整路径外，还接受去换行后NEST+文件名，甚至分散出现的`very/deep`、`nested/project`和文件名，因此不同UI区域的片段也可拼成成功。测试还使用固定`/tmp/basename_path_video`且多数写错误被忽略，旧/并发artifact可能混淆演示结果。后续把Block Viewer标题、surface身份和完整规范化路径作为同一modal内断言，fallback展开另立场景；artifact使用唯一临时目录并返回完整manifest。当前只记录“某种展开或modal画面满足模糊路径片段”。

## 待修复：rename PTY以UI确认代理持久化且未验证标题位置

`rename_title_shows_in_prompt_border.rs`把`Session renamed to`视为`summary.json`锁写往返的确认，但两个测试都不读取session event或summary文件；恢复项只由`--continue`后的UI title推断持久化来源。style检查只要求非bold/inverse、背景等于border且前景不同，没有测注释声明的右对齐、corner间距或窄宽度裁剪。后续直接核对持久化记录的title事件和summary投影，并在多宽度下按display cell验证title相对`╮`的位置；UI ack与存储事实分层验收。

## 待修复：word-select tip测试没有验证选词结果

`word_select_tip_on_double_click_pty.rs`在Ctrl+Y接受后及预置word_select的对照路径都只断言tip不出现；没有检查selection styled cells、选中token边界或OSC52/PRIMARY内容。提示错误地永久消失但双击仍走fold/nav时，现有测试仍可能通过。配置落盘也仅用字符串包含判断，重复key或错误table中的文本可满足。后续解析TOML确认唯一`ui.keep_text_selection`，并对ASCII、CJK与标点token核对选中cell及一次clipboard payload；tip展示与selection行为分别断言。

## 待修复：scroll debug HUD负断言和清除断言只看标题substring

`scroll_debug_hud_absent_without_env`只在初始当前帧检查一次`scroll debug`，不触发滚动或等待稳定窗口；`debug_scroll_command_toggles_hud_live`关闭时也只等500ms后检查标题缺失。若overlay残留其他字段、后续repaint又出现或内部debug采集仍开启，两项都可通过。后续为off状态触发多次滚动/repaint并在稳定窗口检查完整HUD region清空，同时从flight-recorder观测采集开关；env-on flood另记录每个report的classification/flush而非只看`last:trackpad`和最终位移。

## 待核对：viewer turn anchor混用wall clock差值与monotonic时钟

`src/app/acp_handler/prompt_origin.rs::viewer_turn_anchor`用当前UTC毫秒减shell提供的wall-clock timestamp，再从当前`Instant`回减。shell与pager取样之间若系统时钟跳变，elapsed可被夸大、变成非正数或超过`Instant`可表示范围；后两种都静默回当前时刻，使已运行turn看起来刚开始。后续让协议直接携带可验证的elapsed/started duration，或同时保存接收时wall-clock与单调锚点并明确clock-jump策略；覆盖future、极旧timestamp和前后跳时钟的可控测试。迁移只记录现有best-effort显示语义。

## 待修复：ACP未分配session兜底缺少创建关联

`src/app/acp_handler/routing.rs::find_session_match`在没有root/child精确匹配时，只要当前active agent的`session_id`仍为None，就把任意通知session id作为该agent的Root。注释假设这种窗口内“唯一可能”来源是用户刚创建的agent，但函数未核对pending SessionCreated请求、transport/channel、创建nonce或预期session id。旧session迟到通知、另一连接的陌生通知或并发创建事件可能污染新agent状态。后续为agent creation保存pending correlation并只接纳对应transport/request的首次session id，绑定后关闭兜底；增加陌生、迟到、双创建交错和root/child冲突测试。迁移保留现有race-window行为，不在本批修改路由。

## 待修复：权限Edit语义依赖英文选项展示名

`src/app/acp_handler/permissions.rs::is_edit_permission`不使用tool kind、typed meta或稳定option id，而是要求某个AllowAlways option的`name.to_lowercase()`包含英文`edit`。协议文案改写、本地化、没有AllowAlways或其它工具选项偶然含edit时，会漏判或误判，进而改变标题、file_path展示和protected-edit说明。后续由shell权限请求携带稳定的permission subject/kind，pager只按typed枚举分支；保留旧标题作为纯展示，不再作为授权语义。覆盖本地化名称、缺AllowAlways、非Edit名称含edit及kind缺失场景；本迁移只记录当前heuristic。

## 待修复：远端verb grouping翻转未递归失效孙级subagent transcript

`src/app/acp_handler/settings.rs::handle_settings_update`在`group_tool_verbs`解析值翻转时，只对每个top-level agent及其`subagent_views.values_mut()`直接child调用`clear_group_expansion`和`invalidate_heights`；同文件的model catalog与deferred control逻辑却显式递归所有child层级，说明嵌套AgentView是被支持的数据形态。若child再持有descendant view，孙级仍保留旧group shape的expansion id和height cache。后续抽取递归transcript invalidator，与model recursion使用一致的view tree遍历，并覆盖root→child→grandchild在on/off两次翻转后的分组、展开状态与高度。迁移不在当前审计批次改运行时。

## 待修复：background stdout字节数组容错会静默改写数据

`src/app/acp_handler/background.rs::route_bg_task_stdout`对raw `output`数组使用`filter_map(as_u64)`，再以`n as u8`转换。负数、字符串和浮点元素被静默跳过，大于255的整数按低八位截断；随后lossy UTF-8又可替换非法序列。因为mapped update无论是否成功提取都返回consumed，正常tracker也不会再看到原数据。后续复用shell canonical BashOutput typed DTO，严格要求每项0..=255并在非法payload时记录诊断/保留上一buffer；覆盖混合类型、256、负数、非法UTF-8和output_for_prompt优先级。本迁移只记录现有兼容行为。

## 待修复：git head通知只搜索第一层subagent

`src/app/acp_handler/background.rs::handle_git_head_changed`先搜索top-level root，再对每个root的`subagent_views.values_mut()`搜索一次，不递归child的child；而同目录models/deferred-control代码明确支持递归AgentView树。嵌套subagent收到branch/worktree变化时共享per-cwd cache和自身status字段不会更新。后续复用递归session-id resolver或让`find_session_match`返回可借用的完整view path，保证任意深度精确命中且root优先；覆盖root→child→grandchild、重复id和inactive view。本迁移不改git执行或缓存结构。

## 待修复：ACP live event高水位会丢跨通道合法乱序低序号

`src/app/acp_handler/mod.rs::handle`对root live SessionNotification以单一`last_applied_event_seq`执行`seq <= last`即丢弃。源码注释承认actor `event_tx` FIFO之外，bash stdout bridge和turn-start user echo可在较高id生成后先送达，使随后较低id的合法事件被当成stale；当前将一个actor drain hop窗口作为“accepted”。若被丢项是tool lifecycle、text delta或control update，transcript与状态可能永久缺失。后续统一所有ACP event进入同一有序队列，或使用有界reorder buffer/按event class独立序列并在缺口超时后显式诊断；覆盖high先到、low后到、重复、断号、reconnect与replay交叠。本迁移保留当前live-only highwater事实。

## 待修复：compaction failure保留旧held prompt

`src/app/acp_handler/session_notification.rs::apply_session_event`在AutoCompactStarted时保存`compact_held_prompt`并清`in_flight_prompt`；Completed与Cancelled都清held prompt，但AutoCompactFailed只清activity/live feedback并追加失败事件。失败后的旧prompt可残留到下一次compaction，且下一次Started因`compact_held_prompt.is_none()`为false不会捕获新的in-flight prompt。后续为失败定义明确恢复策略：恢复或丢弃原prompt后清held slot，并覆盖失败→新prompt→再次compact、取消及manual/async完成。本迁移不改运行时。

## 待核对：PluginsChanged在skills已Loading时缺少新代际refetch

`session_notification.rs`只在`skills_data`不是Loading时把它置Loading并设置`plugins_changed_needs_skills_refetch`。若旧skills fetch已在途时插件目录改变，新通知不会排第二次fetch；旧请求可能返回变更前快照并把modal置Loaded，之后没有触发源修正。后续为extensions fetch携带catalog generation或dirty-after-flight位，完成旧代请求时自动重取；覆盖Loading期间一次/多次PluginsChanged及agent/session切换。本迁移只记录现有coalescing行为。

## 待修复：ModelChanged无效reasoning effort静默当作缺省

`session_notification.rs::apply_model_changed`用`parse().ok()`把未知reasoning_effort变成None，然后继续用该None解析pending control并调用`set_current`。协议版本偏差或损坏字符串可能被当成“未指定”，既无法诊断，也可能重置/选择模型缺省effort。后续让通知DTO携带typed effort或显式区分Absent/Valid/Invalid，Invalid拒绝整项并保留当前选择；覆盖未知值、大小写、pending intent与catalog lag。本迁移保留当前容错事实。

## 待核对：Scrollback渲染scratch复用和release缓存等长前置条件

`crates/codegen/pager/src/scrollback/render.rs::ScratchBuffer::prepare`仍通过ratatui `Buffer::resize/reset`准备临时画布，源码TODO计划在自有Buffer实现中复用容量并批量fill；这会影响大scrollback帧的分配与清屏成本。相邻渲染路径对`entry_layouts_cache`与`entries`等长只使用`debug_assert`，release构建依赖调用方始终维持该不变量。后续先用帧级分配/耗时证据判断是否实现scratch优化，再把缓存等长变成构造层类型不变量或release可诊断检查，并覆盖失配输入；本轮只记录事实，不混入渲染改动。

## 待核对：Scrollback state注释、release前置条件与snapshot恢复存在漂移

`crates/codegen/pager/src/scrollback/state/mod.rs`中`finish_running`的文档描述短运行延迟，但本文件实现立即完成；`append_entries_from`的sibling/ID空间要求和`insert_block_before`的未提交anchor要求只由`debug_assert`保护；`ViewportSnapshot`捕获`total_height`而恢复函数不写回，仅依赖后续layout收敛。另需沿调用链确认`mark_structurally_dirty`在pending-input改变折叠成员资格时总会推进link-map generation。后续先核对外层调度和全部调用点，再决定修正文档、收紧构造不变量或增加release诊断；本轮不修改状态机。

## 待核对：Scrollback layout隐藏范围与数值不变量缺少release约束

`crates/codegen/pager/src/scrollback/state/layout.rs`承认leading hidden thinking可能让dense range起点偏离truncation header；`compute_paint_window`只用`debug_assert`维护平行数组长度并直接索引；exact oracle把结果截到`u16::MAX`，不能证明更高总高度的完全精确性；`patch_virtual_y_for_dirty`把负delta转`usize`时依赖总偏移不下溢。后续将范围、数组与数值边界变成可检查不变量，并为超u16历史、负delta和hidden-leading组补充针对性验证；本轮保留当前算法事实。

## 待修复：Picker窄宽度和超大列表缺少统一尺寸预算

`crates/codegen/pager/src/views/picker.rs`的filter indicator没有与search bar一致的窄宽省略策略，超长badge也缺少与right label同级的行宽预算；content renderer把`usize`视觉总高和滚动位置收窄为`u16`传给scrollbar，极大列表可能截断。后续统一按display-cell预算search/filter/badge/right-label，并在scrollbar边界保留宽整数或显式饱和，覆盖极窄Unicode行与超过65535视觉行；本轮不修改picker。

## 待修复：MCP setup快捷键缺少紧凑footer显示映射

`crates/codegen/pager/src/views/extensions_modal.rs`把`s`登记为MCP setup动作，解析器、完整cheatsheet与diagnostics也识别该键，但`action_key_display`没有`s`映射，导致紧凑footer省略可用的setup动作。后续为`s`增加稳定显示映射，并同时验证紧凑footer、cheatsheet、diagnostics与解析器的一致性；本轮仅记录当前行为，不混入UI修复。

## 待核对：Shortcuts help说明、Vim状态来源与expand API发生分叉

`crates/codegen/pager/src/views/shortcuts_help.rs`的模块级类别顺序说明已与`CATEGORY_ORDER`不一致；`handle_input`与modal footer读取进程级可变Vim外观缓存，而不是复用已传入的Vim模式投影，测试因此必须通过全局guard串行化；旧`hint_expand_action_id`兼容辅助仍与信息更完整的`ExpandKey`并存。后续以单一显式view state驱动输入和footer，更新说明并收敛expand identity；本轮保留当前行为。

## 待核对：Selection整块拖拽owner与复制几何存在隐式fallback

`crates/codegen/pager/src/app/agent_view/selection.rs::finish_block_drag`固定遍历父scrollback并使用父session cwd，而文本复制会显式切换`active_subagent`；同文件的`u8`多击计数直接加一，极端窗口可能在debug panic或release回绕；缺可见block width时整块复制静默按80列换行。后续先核对active subagent进入block drag的调用路径，再显式携带owner/width并用饱和计数，覆盖父子视图和非80列重排；本轮不改选择逻辑。

## 待核对：Question view并行状态与尺寸换算缺少类型约束

`crates/codegen/pager/src/views/question_view.rs`公开questions、selections、cursor、scroll与freeform等并行Vec，构造外部可破坏等长不变量并触发直接索引；scrollbar用整个Buffer宽度估算content而panel更窄；视觉行大量收窄到`u16`；`send_ext_response`忽略receiver dropped。后续封装每题状态、统一panel宽度测量、明确饱和上限并传播发送失败，覆盖畸形并行状态和超大CJK内容；本轮只登记债务。

## 待修复：Debug panel与status bar混用字符数和字节数作为终端列宽

`crates/codegen/pager/src/views/debug_style.rs::render_panel`按Unicode scalar数量截断/补空格，却把结果交给按display cell绘制的Buffer；`crates/codegen/pager/src/views/status_bar.rs`更直接用UTF-8字节`len()`计算center/right宽度和左侧避让。CJK、组合字素或宽字符会导致背景未覆盖、截断、错位或标签重叠，status right也没有与left/center的显式冲突门控。后续统一使用终端display width与按cell安全截断，建立left/center/right优先级，并覆盖CJK、组合字符、极窄宽度和冲突标签；本轮只记录当前行为。

## 待核对：Dashboard state身份编码、异步替换与持久写入缺少稳定不变量

`dashboard/state.rs`以`sub:<parent>:<child>`和首冒号切分持久subagent id，opaque parent若含冒号无法无歧义往返；atomic write临时名只含PID，同进程并发写可能争用；多个公开焦点字段可形成多焦点；recent异步结果按旧索引恢复选择；子目录在排序前截断1000项；每表面单个deferred send槽可覆盖前一stash。后续以结构化identity、唯一临时文件、封装focus枚举、稳定候选key、排序后限额和FIFO请求槽收敛，分别补并发/重排/大目录测试；本轮不修改状态。

## 待拆分：Root AppView聚合过多职责并含未受测或静默分支

`app/root/mod.rs`同时承载约9600行状态、路由、绘制、时钟和218个测试，`AppView`与`test_app`巨型字段字面量需多点同步；mouse路径保留空`if is_mouse_action {}`；ExitSession测试直接清popup而未覆盖真实`handle_input` outcome；可注入`arrived_at`与`PendingAction::expired(Instant::now)`时钟不一致；minimal draw缺hook时静默返回。后续按状态/输入/渲染/时钟边界拆分并先为这些具体分支建立测试，不把重构混入当前审计。

## 待拆分：Pager event loop调度、输入背压与外部命令解析耦合

`app/root/event_loop.rs::run`约1690行并拥有启动、线程、select、重连与刷新；terminal reader使用无界channel而消费者每轮最多drain 256；`$PAGER`用`split_whitespace`无法表达带空格或引用参数；`active_restored`返回值生产路径不再消费，`post_render_effects`恒空；TTY/child/delete多处错误被丢弃。后续先拆出可测的调度和生命周期组件，为输入设有界背压，使用平台命令解析/明确argv配置，并把恢复与I/O失败变成结构化结果；本轮只登记债务。

## 待核对：Session close用条目数代理唯一存活会话

`dispatch_sessions_confirm_close`的注释要求拒绝关闭“唯一alive agent”，实现却只检查`app.agents.len() == 1`，不读取会话运行/终态。若map含一个存活项和一个不可用终态项，关闭存活项会选择终态fallback并留下没有可继续会话的Pager。后续明确close约束使用“条目”还是“可恢复/可交互会话”，按语义过滤fallback并覆盖alive+dead、parent dead及全dead组合；本轮保留当前map长度事实。

## 待修复：Turn公开区间允许end小于prompt导致长度下溢

`scrollback/state/types.rs::Turn`公开`prompt_index`与`end_index`，`len()`直接执行`end_index - prompt_index`。外部可构造逆序区间，debug构建会panic，release可能回绕成极大长度；`range()`同样暴露无效Range。后续用校验构造器或内部字段维持`prompt <= end`，并为畸形输入提供明确拒绝或饱和策略；本轮不改变公开类型。

## 待拆分：AgentView draw混合渲染、状态推进与终端协议

`app/agent_view/render.rs::draw`约3588行，既计算布局又清pending kill、同步pane/hover/cache、触发media load并组装OSC输出；重绘频率可影响状态推进。`current_shortcut_hints`声称单一来源但draw重复permission/plan/question等分支；HOME缩写只做字符串前缀；多处宽度用chars/len；`should_show_tip`恒false；图片清理和最终写入错误未结构化返回。后续先把纯frame projection、状态effect和PostFlush协议拆开，再统一shortcut和路径/display-cell工具并传播I/O结果；本轮不重构。

## 待拆分：Prompt widget测试固化内部形状和进程全局状态

`views/prompt_widget/tests.rs`在4582行内混合241项输入、附件、slash、搜索、suggestion和绘制测试，多数直接改写widget内部字段，绕过公开事件路径；Kitty owner、graphics与embedded模式依赖进程全局guard/reset，但只有embedded测试显式串行。CR、paste阈值和image lifecycle还有重复回归组，fake search不覆盖异步排序/ignore。后续按行为边界拆分fixture，优先通过公开Action输入，并为全部全局状态统一串行隔离与恢复断言；本轮保留现有测试事实。

## 待拆分：AgentSession公开状态混合数据、展示与控制编排

`app/session/mod.rs`的`AgentSession`拥有百余公开或crate字段，文件虽称pure data types，却包含宽度文案、300ms阈值、queue merge和ACP tracker委派；Workflow状态使用裸字符串；background stdout的set保留头部而append保留尾部；screen-mode handoff归属只用`debug_assert`；queue id普通加法；`restore_degree`与`finalized_pr_meta`保留但没有生产消费。后续按会话事实、UI投影、队列和控制域封装，typed化Workflow状态、统一截断、在release校验handoff并明确id溢出/保留字段消费者；本轮不改变session结构。

## 待拆分：PromptWidget公开可变状态跨越编辑、附件、搜索与渲染协议

`prompt_widget/mod.rs`在3631行和约96个可见方法中聚合textarea、clipboard、file search、slash、建议、图片文件、诊断与overlay escape；公开textarea/images可绕过同步不变量。重复文本transform用`rfind`启发式映射chip；draw推进状态并依赖全局theme/overlay；Nothing鼠标仍返回Edited；cap/duplicate图片被丢弃后的临时文件清理不在本层；多处切片依赖元素有序、不重叠和UTF-8边界。后续封装document/attachment模型、显式编辑映射与纯render effect，并收紧事件结果和资源所有权；本轮不重构。

## 待拆分：Modal routing重复派生状态且Changed无法表达真实变化

`agent_view/modal_routing.rs`集中全部modal输入与绘制，新增ActiveModal需多处分支同步；session effective query/group/row在输入和绘制各自重建；许多无效分支也返回Changed；picker依赖进程Vim缓存；hit-area依赖上一帧数据未重排；DocPicker维护未使用entries；EditConfirm有路由却无绘制，仅靠外部不再arming。后续建立每modal自包含controller/render model和带变化语义的结果，frame identity绑定hit geometry，并删除不可达变体；本轮只登记债务。

## 待拆分：Root effect执行器集中99类副作用并折叠协议错误

`app/root/effects/mod.rs::execute`约3810行承载99种effect；多处把动态JSON解析失败降为默认空值或CancelComplete，通知发送失败也可能回同一完成结果。SetWorkingDir改变进程cwd而skills使用`.`；worktree用字符串前缀；session info硬编码BYOK；CheckMarketplaceUpdates实际执行升级；deep-search sleep越过deadline；64个payload expect会panic；active注册前后两份状态可分叉。后续按协议/进程/存储/扩展/会话拆执行器，typed解析和结构化失败优先，消除全局cwd与字符串路径推导，并重新命名/限界更新与搜索；本轮不改effect。

## 待拆分：Text selection双重线性模型与全局配置增加一致性风险

`scrollback/text_selection.rs`同时维护visible-model和full-output两套endpoint/range/joiner重建，命中与boundary大量线性扫描；word separator使用OnceLock，运行期配置变化不可见；table side-car只核对key不核对内容新鲜度；URL仅正则加括号计数；一个overlay测试未断言颜色/块，另一个修改全局theme不恢复。后续统一线性文本索引与generation、新鲜度绑定，显式注入separator/theme，并把URL解析和overlay断言分层；本轮保留现有选择事实。

## 待核对：Tasks pane控制身份、可停止状态与运行语义分叉

`views/tasks_pane.rs`的workflow稳定id使用`run_id`，overlay/hover/kill查找却使用可重复的name；实体保存`stoppable=can_stop()`，renderer只按`is_active()`显示kill，paused/budget-limited可能可停却无控件；scheduled恒为running并参与自动打开/阻止关闭；shell highlight cache只在theme切换清空；scheduled prompt按字符而非display cell截60。后续统一run_id控制身份和stoppable来源，拆configured/firing状态，给高亮有界淘汰并按cell预算预览；本轮不改任务面板。

## 待拆分：Permission view职责说明和布局/语法前置条件已漂移

`views/permission_view.rs`头注释称pure data且无render/input，实际约2700行负责渲染和shell文本算法；`bash_selection_count`直接切highlighted words；collapsed height在screen<10时可高于屏幕且依赖外层裁剪；quote breaker只覆盖简单单双引号而非完整shell grammar；多个scroll/cache字段不在本文件消费；historic fixture被ignore和环境变量双门控。后续拆DTO/layout/render/shell presentation，clamp索引和屏幕尺寸，明确文本换行不具授权语义，并补provenance/fallback/geometry测试；本轮不改权限行为。

## 待核对：Scrollback selection helper与group展开状态缺少统一语义

`scrollback/state/selection.rs`的`set_selected`与`on_activate`没有统一过滤隐藏项；`collapse_all`绕过block自身`collapse_mode`；expand-all只识别Collapsed而不识别Truncated；verb group与N-more共用没有provenance的`expanded_groups`；无选择时raw toggle仍触发全量layout重建。后续建立单一可选性/折叠状态转换和带group kind的展开identity，仅在实际变化时失效布局，并覆盖隐藏、Truncated和两类group碰撞；本轮保留当前行为。

## 待拆分：Agent modal控制器重复配置投影并依赖隐式请求身份

`agent_view/modals.rs`在2902行聚合extensions/settings、诊断和动作；mouse路径直接unwrap modal依赖调用方；keyboard/mouse/render重复生成config；ToggleExpand与通用fold两套逻辑；多条平行render cache及无变化Changed；pending没有request identity；uninstall同时保留本地与服务端确认。Agents/mouse/forms多数action及Skills slash-only/global Vim还缺直接测试。后续按modal controller拆分，共享单一view model/fold transition，以request id关联pending并收敛确认所有权；本轮不改行为。
## 待拆分：Pager diagnostics模块同时承担检测、报告、渲染与slash修复

`pager/src/diagnostics/mod.rs`把terminal/clipboard/notification探测、warning聚合、TUI报告和doctor slash参数/修复路由集中在一个模块；部分结果依赖全局Kitty/终端状态，probe suppression与可见报告共享隐式顺序。后续按probe snapshot、warning normalization、report rendering和fix command拆分，显式传递运行时上下文并补跨终端矩阵；本轮只登记源码事实。
## 待拆分：pager-render跨越外观、协议、剪贴板和游戏渲染边界

`pager-render` crate 同时承载appearance配置、clipboard/image、终端协议探测、主题、渲染、编辑器和gboom；多个模块依赖进程级OnceLock、线程局部缓存和环境快照，FFI、资源解码、外部opener和终端探测的错误语义也各自实现。后续按能力边界拆分并统一错误模型，补跨平台生命周期与全局状态隔离测试；本轮只登记已有契约和完整文件证据。
## 待拆分：shell crate跨越启动、持久化、终端、MCP与认证边界

shell crate同时承载agent bootstrap、session actor、JSONL durable storage、extension commands、leader/local IPC、terminal runtime、configuration、MCP和authentication。301条新增契约已把源码事实落到OpenSpec，但实现边界仍交叉，部分运行时/平台/并发语义尚未动态验证。后续按生命周期、持久化、协议、终端和认证能力拆分演进；本轮不混入架构重构。
## 待核对：pager-pty-harness静态契约与真实PTY设施之间的边界

pager-pty-harness的79条契约已覆盖全部40个文件，但脚本、PTY、scroll matrix和bench仍未运行；终端时序、操作系统进程、clipboard和外部服务行为尚无动态证据。后续用受控磁盘/PTY预算分层验证，避免把静态fixture来源误报为端到端通过；本轮不启动构建。

## 待拆分：tools crate混合构建资产、工具注册与协议投影

`crates/codegen/tools`同时承载外部搜索二进制打包、桥接与Skill baseline、registry/schema、MCP/JSON协议投影、文件系统与网络辅助、SQLite向量工具和测试支撑。198个文件的静态事实已登记，仍需后续按构建供应链、运行时工具注册、协议边界和存储/搜索能力拆分；本轮不执行build.rs下载、不运行外部二进制，也不混入重构。


## 待核对：nono平台沙箱契约与运行时实现边界

`third_party/nono`同时提供能力清单schema/codegen、Landlock/Seatbelt/Windows等平台实现、命令与文件描述符策略、环境变量和资源限制。39条静态契约已登记，但未运行平台沙箱、build.rs生成或跨平台权限矩阵；后续需按平台后端、manifest编译和进程执行边界拆分动态验证，本轮不改第三方实现。

## 待拆分：workspace crate的RPC、文件服务与配置投影耦合

`crates/codegen/workspace`同时承载workspace发现、typed RPC、文件传输、GitHub导出、配置解析、tracker活动状态和placeholder校验。35条既有能力需求已补齐56个文件证据，仍需后续按RPC协议、文件服务、配置与状态投影拆分动态验证；本轮不改实现。

## 待拆分：Pager task-result reducer集中多域完成与失败投影

`app/root/dispatch/tests/task_result.rs`反映的dispatch边界同时处理transport completion、权威session通知、UI notice、doctor身份、picker mutation、settings rollback、extension modal和sampling control reconciliation。69条测试事实已登记，后续需按完成协议、身份/控制token、持久化通知和界面投影拆分；本轮不改reducer。

## 待核对：nix-ohos平台抽象与OHOS适配边界

`third_party/nix-ohos`以vendored nix接口承载Unix/OHOS系统调用、feature门控、错误映射和类型封装。62条静态契约已登记，尚未运行OHOS工具链、真实系统调用或跨feature矩阵；后续需按平台后端和安全敏感调用分层动态验证，本轮不修改第三方代码。

## 待拆分：Pager edit block同时处理差异、布局和交互投影

`scrollback/blocks/tool/edit.rs`同时负责patch差异解析、语法高亮升级、布局换行、标题/选择/链接/折叠和复制patch，15条契约已登记。后续按diff模型、终端布局和交互状态拆分并补真实渲染验证；本轮不改编辑器行为。

## 待拆分：Pager agent input聚合多层输入所有权

`app/agent_view/input.rs`同时处理overlay优先级、Esc/Left、leader快捷键、行为确认、model/Agent picker、`/btw`焦点、Vim/Pane、子代理、粘贴和inline edit。70条静态契约已登记，后续需按输入所有权与视图控制器拆分并补真实交互矩阵；本轮不改输入路由。

## 待拆分：Pager links模块混合链接、CTA和遮挡路由

`app/agent_view/links.rs`同时处理链接高亮/点击、CTA与控件遮挡、prompt dropdown、subagent/modal优先级和拖拽状态。5条静态契约已登记，后续需按链接模型、命中测试和overlay优先级拆分并补真实终端交互；本轮不改路由。

## 待拆分：Pager agent interactions耦合提交、权限和问答状态

`app/agent_view/interactions.rs`同时处理transport/session清理、permission、cancel/Goal、question三态、鼠标命中、滚动、提交路由、dashboard answer、no-freeform和paste-chip。15条静态契约已登记，后续需按输入状态机、授权反馈和提交协议拆分并补真实交互矩阵；本轮不改实现。

## 待拆分：Pager agent interactions耦合提交、权限和问答状态

`app/agent_view/interactions.rs`同时处理transport/session清理、permission、cancel/Goal、question三态、鼠标命中、滚动、提交路由、dashboard answer、no-freeform和paste-chip。15条静态契约已登记，后续需按输入状态机、授权反馈和提交协议拆分并补真实交互矩阵；本轮不改实现。

## 待拆分：Pager paste模块混合剪贴板、媒体和弹窗路由

`app/agent_view/paste.rs`同时处理文本/路径/图片粘贴、异步剪贴板探测、弹窗路由、undo、Mermaid affordance和Kitty媒体生命周期。15条静态契约已登记，后续需按输入解析、媒体资源和UI提示拆分并补跨平台剪贴板验证；本轮不改粘贴行为。

## 待拆分：Pager dashboard dispatch集中服务响应和视图投影

`app/root/dispatch/dashboard.rs`集中dashboard请求/响应、trajectory、task pane、question、worktree和agent view状态投影。5条静态契约已登记，后续需按服务协议与界面投影拆分并补真实服务验证；本轮不改dispatch。

## 待拆分：Pager dashboard peek混合运行态摘要和交互控件

`views/dashboard/peek.rs`同时处理peek刷新、root/subagent投影、配置徽章、面板渲染、问题/权限选项、reply/paste、live tail、摘要和数字键路由。12条静态契约已登记，后续按数据投影与交互控件拆分并补实时服务验证；本轮不改peek行为。

## 待拆分：Pager list pane state集中选择、布局缓存和搜索导航

`views/list_pane/state/mod.rs`同时维护稳定ID选择、布局缓存淘汰、滚动/跟随、过滤搜索、键盘导航、能力开关、复制、视觉选择和粘贴。13条静态契约已登记，后续按选择模型、布局缓存和输入导航拆分并补真实交互验证；本轮不改列表行为。

## 待拆分：Pager router测试覆盖多域完成和控制token

`app/root/dispatch/tests/router.rs`集中路由、控制token、会话/子代理、模型/Agent、权限、dashboard和任务结果分支。15条静态契约已登记，后续按协议路由、身份围栏和视图副作用拆分并补动态dispatch验证；本轮不改router。

## 待拆分：Pager nav混合滚动、可见性和wrapped-line映射

`scrollback/state/nav.rs`同时处理turn/response导航、分页滚动、follow/page-flip、sticky header、可见性、搜索reveal、折叠展开和wrapped-line映射。11条静态契约已登记，后续按导航状态、布局映射和滚动策略拆分并补真实终端验证；本轮不改导航行为。

## 待拆分：Pager Mermaid worker混合线程、缓存和渲染回退

`app/agent_view/mermaid_worker.rs`同时处理Mermaid后台线程、主题/尺寸、状态轮询、缓存、渲染消息和失败回退。14条静态契约已登记，后续需按后台任务生命周期、缓存一致性和渲染适配拆分并补真实渲染验证；本轮不改worker行为。

## 待拆分：Pager dashboard row混合状态投影和渲染字段

`views/dashboard/row.rs`同时构造dashboard行模型、状态/可见性、身份和渲染字段投影。6条静态契约已登记，后续需按状态模型、身份索引和渲染适配拆分并补真实dashboard验证；本轮不改行模型。

## 待拆分：Pager list pane methods耦合状态变更与布局失效

`views/list_pane/state/methods.rs`集中列表状态方法、选择/过滤、布局缓存、滚动和输入操作。静态事实已登记，后续需按状态变更、布局失效和输入动作拆分并补真实列表交互验证；本轮不改实现。

## 待拆分：Pager agent view混合列表、会话状态和工作区摘要

`views/agent.rs`同时承载Agent列表/面板、工作区摘要、会话/子Agent状态和渲染投影。静态事实已登记，后续需按实体列表、会话状态和展示投影拆分并补真实Agent视图验证；本轮不改视图行为。

## 待拆分：Pager app模块聚合生命周期、输入和渲染协调

`app/mod.rs`集中App状态、生命周期、输入/渲染协调和测试辅助边界。11条静态契约已登记，后续需按生命周期、输入路由和渲染协调拆分并补真实终端验证；本轮不改App行为。

## 待拆分：Pager workflows视图耦合运行状态与列表交互

`views/workflows.rs`同时处理workflow列表/状态、选择、过滤、渲染和输入投影。静态事实已登记，后续需按运行状态模型和列表交互拆分并补真实Workflow视图验证；本轮不改视图行为。

## 待拆分：Pager scrollback block混合数据模型、折叠和渲染入口

`scrollback/block.rs`同时承载滚动块模型、折叠/展开、渲染入口、选择和条目元数据。10条静态契约已登记，后续需按数据模型、布局状态和渲染适配拆分并补真实终端验证；本轮不改block行为。

## 待拆分：Pager modal window混合布局、滚动和输入命中

`views/modal_window.rs`同时处理modal尺寸/布局、标题/边框、滚动、焦点和鼠标/键盘输入投影。12条静态契约已登记，后续需按布局模型、焦点状态和命中测试拆分并补真实终端验证；本轮不改modal行为。

## 待拆分：Pager entry renderer混合块包装和工具状态投影

`scrollback/wrappers/entry_renderer.rs`同时承载entry渲染包装、块/工具状态投影、文本布局和测试辅助边界。静态事实已登记，后续需按渲染包装、状态投影和布局拆分并补真实终端验证；本轮不改renderer行为。

## 待拆分：Pager announcements混合内容加载、布局和CTA交互

`views/announcements.rs`同时处理announcement加载/关闭、CTA、布局、缓存、权限和提示状态。8条静态契约已登记，后续需按内容状态、布局命中和CTA路由拆分并补真实终端验证；本轮不改announcement行为。

## 待拆分：Pager line viewer混合文件搜索、命中高亮和滚动定位

`views/file_search/line_viewer.rs`同时处理文件搜索行查看、匹配高亮、滚动定位和命中投影。3条静态契约已登记，后续需按搜索结果模型、布局高亮和滚动定位拆分并补真实文件搜索验证；本轮不改viewer行为。

## 待拆分：Pager block viewer混合内容查看、布局和选择状态

`views/block_viewer.rs`同时处理block查看器、滚动/布局、文本选择和输入状态投影。9条静态契约已登记，后续需按内容查看、布局和选择状态拆分并补真实终端验证；本轮不改viewer行为。

## 待拆分：Pager list pane render混合窗口布局和选择视觉

`views/list_pane/render.rs`同时处理列表行渲染、滚动窗口、选择/过滤、视觉状态和剪贴板提示。7条静态契约已登记，后续需按布局窗口、选择视觉和提示拆分并补真实终端验证；本轮不改render行为。

## 待拆分：Pager turn dispatch测试混合终态、控制和压缩

`app/root/dispatch/tests/turn.rs`集中turn完成/失败、prompt队列、压缩、行为/模型控制和终态通知。14条静态契约已登记，后续需按turn终态、控制确认和压缩协议拆分并补动态验证；本轮不改turn处理。

## 待拆分：Pager prompt模块混合编辑、发送和队列状态

`app/agent_view/prompt.rs`同时处理prompt编辑/发送、队列状态、快捷动作和输入投影。静态事实已登记，后续需按编辑模型、提交协议和队列状态拆分并补真实输入验证；本轮不改prompt行为。

## 待拆分：Pager agent viewer混合查看状态、滚动和内容投影

`app/agent_view/viewer.rs`同时处理viewer状态、滚动/布局和内容投影。9条静态契约已登记，后续需按查看模型、布局状态和内容投影拆分并补真实终端验证；本轮不改viewer行为。

## 待拆分：Pager agent session混合绑定、子Agent身份和工作区投影

`app/agent_view/session.rs`同时处理会话绑定/解绑定、子Agent身份、cwd/工作区投影和状态清理。静态事实已登记，后续需按会话身份、工作区投影和清理生命周期拆分并补真实会话验证；本轮不改session行为。

## 待拆分：Pager session lifecycle测试混合恢复、绑定和持久化终态

`app/root/dispatch/tests/session/lifecycle.rs`集中session生命周期、绑定/解绑、恢复、删除、重命名、终态通知和失败回滚。7条静态契约已登记，后续需按生命周期事务、身份绑定和持久化终态拆分并补动态存储验证；本轮不改session协议。

## 待拆分：Pager prompt dispatch混合队列、输入和权限路由

`app/root/dispatch/prompt.rs`同时处理prompt dispatch、队列/输入控制、权限和会话路由边界。14条静态契约已登记，后续需按队列协议、输入准入和权限路由拆分并补动态验证；本轮不改prompt dispatch。

## 待拆分：Pager session lifecycle跨越创建、trust、deferred control和picker

第二轮细化表明`session/lifecycle.rs`还同时覆盖session创建、worktree、deferred control、workspace trust、project picker、删除/退出和dashboard stop。新增12条静态契约已登记，后续需按创建事务、信任准入、延迟控制和picker状态拆分；本轮不改实现。

## 待拆分：Pager tasks pane混合任务身份、状态控制和列表渲染

`views/tasks_pane.rs`同时处理任务/Workflow列表、运行状态、选择、停止/取消、过滤、排序和提示投影。12条静态契约已登记，后续需按任务身份、运行控制和列表渲染拆分并补真实任务运行验证；本轮不改tasks pane。

## 待拆分：Pager scrollback state聚合导航、折叠和过滤状态

第二轮复核确认`scrollback/state/mod.rs`聚合导航、折叠、过滤、选择和布局状态。既有契约来源已补齐，后续需按状态域拆分并补真实终端矩阵；本轮不改scrollback state。

## 待拆分：Pager scrollback state还包含终端marker和stop hook折叠

第二轮核对补出`terminal marker stop hook stash identity folding`契约，说明scrollback state还承担终端marker、stop hook和stash identity折叠。后续需将该生命周期与普通滚动状态分离验证；本轮不改实现。

## 待拆分：Pager router混合Action路由和Effect分派

`app/root/dispatch/router.rs`集中Action路由、Effect分派和边界错误。12条静态契约已登记，后续需按路由表、effect执行和错误策略拆分并补动态验证；本轮不改router。

## 待拆分：Pager task-result dispatch混合结果投影和副作用

`app/root/dispatch/task_result.rs`集中TaskResult分支、完成/失败投影和副作用边界。9条静态契约已登记，后续需按结果协议、状态投影和副作用执行拆分并补动态验证；本轮不改task result。

## 待拆分：Pager modal模块混合通用布局、焦点和命中

`views/modal.rs`同时处理通用modal尺寸/布局、焦点、滚动、鼠标命中和边框/标题投影。6条静态契约已登记，后续需按布局、焦点状态和命中路由拆分并补真实终端验证；本轮不改modal行为。

## 待拆分：Pager agent mouse混合拖拽、滚轮、链接和overlay命中

`app/agent_view/mouse.rs`同时处理鼠标事件路由、拖拽、滚轮、链接和overlay命中。6条静态契约已登记，后续需按输入路由、命中几何和拖拽状态拆分并补真实终端验证；本轮不改mouse行为。

## 待拆分：Pager session load混合恢复、损坏处理和状态投影

`app/root/dispatch/tests/session/load.rs`同时处理session加载、恢复、损坏/缺失数据、绑定和状态投影。10条静态契约已登记，后续需按恢复事务、错误分类和状态投影拆分并补动态存储验证；本轮不改load行为。
