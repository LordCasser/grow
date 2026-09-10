# 待启动事项

> **定位**：本文件只记录长期计划，不是当前版本待办或实现授权。任何条目都不会自动进入实现；即使成熟度或验收条件已经满足，也必须由用户另行明确启动。

Grow 的配置保持本地化：全局配置位于 `$GROW_HOME/config.toml`，项目配置位于项目内的
`.grow/config.toml`。项目配置只影响当前项目，并按现有解析策略覆盖全局配置。

远程配置管理、deployment-config 服务、签名策略同步及其专用 CLI 不在规划范围内。

## 压缩后的 portable 工具历史

2026-09-10 核对 session `01a08910-219a-78f1-a45c-98b448764a05`：局部压缩保留了 tail identity，但 `finish_surface_replacement` 重置整个 native epoch 后，`project_portable_history` 把最新的成对工具调用/结果也转换成 Historical tool exchange 文本。已观察到压缩后模型只给出行动预告便合法 stop，因果强度仍受单样本限制。本次修复历史摘要范围与下一 Step 续接；结构化 portable tail 单独立项。

- ceiling：继续清除旧 native 签名/加密 reasoning，不按相同 model/backend 猜测它们仍合法，也不通过回复长度/冒号强制重采样。
- trigger：在继续评估此类语义停顿、跨模型迁移或 portable 请求协议时，由用户明确启动。
- upgrade：分别证明三个后端对不含旧 reasoning carrier 的完整工具往返的接受边界，设计同 route 与跨 route 的投影策略；覆盖 partial compaction、完整/悬空/孤立工具项、模型切换和 replay。验证实际 wire 与后续工具执行，避免重发已执行副作用或从历史恢复 native state。

## 无签名 Messages 回退切断工具往返

2026-09-10 审计 session `01a08906-7afd-70e2-af4e-5ff9ea4db84c`：原始请求 seq 49616、49659 只有最新 tool_result，没有对应 tool_use；Timeline 中调用及执行结果完整。当前 2.1.6 的 `push_response_durably` 在 unsigned thinking 导致 native 缺失时，把 portable prefix 固定在 assistant 之后、尚未追加的结果之前。`request_segments` 分别投影两侧，prefix 内的调用因无结果被删除，后缀结果却继续以原生协议发送。审计与本地复现见 `changes/archive/2026-09-10-audit-session-colon-stop/`。

- 范围：这是跨层请求配对缺口，区别于上述完整工具历史被文本化的问题；不能因服务端接受了请求就认定配对有效。
- 限制：三次行动预告后均收到合法 end_turn，其中首个请求没有孤立结果，不能把所有提前结束归因于本缺口。
- 后续验收：从 unsigned thinking + tool_use 经真实 ChatState 接纳、工具结果追加到下一 wire，保证工具往返完整表达或整体安全降级；覆盖多个工具、文本/推理混排、控制边界、模型切换和恢复。继续清除无效签名，不重放已执行工具，不通过冒号或短句猜测完成状态。修复需单独 change，不在本审计实施。

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

- **子 Agent 诊断面板目标路由**：`pager/src/app/root/dispatch/status.rs` 的 Usage/Context/SessionInfo 打开及异步结果处理固定定位根 Agent，而 `ctx.rs` 已有可定位当前子视图的 helper。若对子视图增加用量点击入口，会错误打开父账本；`improve-goal-status-and-usage` 仅在普通主会话提供新入口。后续单独统一打开与异步结果的目标身份，验证切换/关闭/重开子视图、父子同时读取和迟到结果不会串用数据，不改变账本 owner 或把子用量重复计入父会话。

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

## 持续审计待拆分事项

- **长会话在线追加的生命周期复制开销**：本次仅移除私有批量恢复中的累计 `LifecycleFold` 克隆；在线 `Timeline::prepare/accept` 仍通过克隆保障失败原子性。后续若优化在线追加，应先测量长历史写入延迟，再设计不削弱拒绝原子性的最小变更；不能直接复用遇错会丢弃整个 fold 的恢复路径。恢复中的 JSON 解析、重复重建和 artifact 校验也需分别测量，不能以跳过校验换取速度。见 [accelerate-long-timeline-replay](changes/archive/2026-09-10-accelerate-long-timeline-replay/verification.md) 的验证记录。

- **更新目录的多平台保留语义（已完成）**：已修正为保留当前版本和最高其他版本的全部已识别平台产物，包含回退与多平台真实目录回归。见 [fix-update-version-group-retention](changes/archive/2026-09-07-fix-update-version-group-retention/verification.md)。

- **LSP inspect 同名来源展示（已完成）**：允许来源与被禁用的同名项目定义现分别展示，并共享报告的信任结果。见 [fix-inspect-lsp-fallback-display](changes/archive/2026-09-07-fix-inspect-lsp-fallback-display/verification.md)。

- **LSP inspect 插件启用状态（已完成）**：允许来源使用 registry.active_plugins，禁用和未信任插件声明分别标记并单列，不能遮蔽活动来源。见 [fix-inspect-lsp-plugin-status](changes/archive/2026-09-07-fix-inspect-lsp-plugin-status/verification.md)。

- **web_fetch 缓存容量（已完成）**：零容量不保存条目，满容量更新已有 URL 不再误删其他页面；新增 URL 保留最早插入淘汰策略。见 [fix-web-fetch-cache-capacity](changes/archive/2026-09-07-fix-web-fetch-cache-capacity/verification.md)。

- **web_fetch 跳转授权传播（已完成）**：全部跳转返回目标，由新的工具调用经过现有授权，不再将初始 URL 许可扩大到同主机的其他路径或端口。见 [fix-web-fetch-redirect-authorization](changes/archive/2026-09-07-fix-web-fetch-redirect-authorization/verification.md)。

- **MCP 恢复配置探测的取消边界（已完成）**：stdio/HTTP 调度与循环的配置探测等待现监听取消，取消不推送 Disabled，已取得标记释放。见 [fix-mcp-config-probe-cancellation](changes/archive/2026-09-07-fix-mcp-config-probe-cancellation/verification.md)。

- **MCP dispatcher 强制终止的取消通知（已完成）**：dispatcher future 持有 CancellationToken drop guard，fatal/超时 abort 也通知恢复取消，正常 drain 保留。见 [fix-mcp-dispatcher-abort-cancellation](changes/archive/2026-09-07-fix-mcp-dispatcher-abort-cancellation/verification.md)。

- **MCP 迟到工具错误的恢复身份（已完成）**：可恢复错误绑定失败服务，旧错误不再重置新 Ready，见 [fix-mcp-stale-tool-error-recovery](changes/archive/2026-09-07-fix-mcp-stale-tool-error-recovery/verification.md)。
- **MCP 迟到工具超时的 reset 身份（已完成）**：超时 reset 在同一状态锁内核对失败服务，保留新 Ready/进行中握手且不重放工具。见 [fix-mcp-stale-tool-timeout-reset](changes/archive/2026-09-07-fix-mcp-stale-tool-timeout-reset/verification.md)。

- **MCP HTTP 恢复最后提交边界（已完成）**：成功/失败均在最后状态锁内核对客户端身份和 HTTP 配置，Superseded 停止旧循环，不重试替代连接。见 [fix-mcp-http-recovery-identity](changes/archive/2026-09-07-fix-mcp-http-recovery-identity/verification.md)。

- **MCP 初始化取消时锁竞争（已完成）**：ClientState 私有短同步锁确保取消清理在锁竞争后完成；Empty/Pending 跨线程回归与真实初始化取消通过。见 [fix-mcp-init-cancellation-contention](changes/archive/2026-09-07-fix-mcp-init-cancellation-contention/verification.md)。stdio 取消也已收敛至 Empty，见 [fix-mcp-stdio-init-cancellation](changes/archive/2026-09-07-fix-mcp-stdio-init-cancellation/verification.md)。

- **HTTP Hook 请求边界（已完成）**：已分别完成 [DNS 地址绑定](changes/archive/2026-09-07-fix-http-hook-dns-binding/verification.md)、[正文容量限制](changes/archive/2026-09-07-fix-http-hook-response-limit/verification.md) 和 [共享总超时](changes/archive/2026-09-07-fix-http-hook-total-timeout/verification.md)。各项验证限制见记录，不据此声明所有 Hook 行为已审计完毕。

- **Hook 成功状态下的结构化输出错误（已完成）**：共享对象解析和结构化错误分类，未知字段/类型错误/截断对象/数组不再混同日志；普通文本保留既有规则。见 [fix-hook-structured-output-errors](changes/archive/2026-09-07-fix-hook-structured-output-errors/verification.md)。

- **Hook 相对命令的去重身份（已完成）**：直接相对命令键包含 source_dir，shell/绝对路径保持原规则，执行与去重共享路由判定。见 [fix-hook-relative-command-dedup](changes/archive/2026-09-07-fix-hook-relative-command-dedup/verification.md)。

- **Hook 匹配器派生状态恢复边界（已完成）**：registry 的 serde/append/dedup 统一按配置重建，None 清缓存，无效 Never；workspace 不再手工修复并有真实匹配往返断言。见 [fix-hook-registry-matcher-boundary](changes/archive/2026-09-07-fix-hook-registry-matcher-boundary/verification.md)。

- **Discovery 重读收敛**：Pager 新目录事件的额外 AdvertiseCommands 已移除，见 fix-pager-duplicate-discovery-advertisement（静态验证，未执行 Pager 构建）；ReloadSkills 自行负责发布。已进一步核对扫描函数内部同步完成，尚未证实旧扫描晚提交覆盖；发现的同路径元数据协调缺陷已单独立项 fix-skill-baseline-metadata-reconciliation。证据见 audit-discovery-reload-fanout；与目录实体注册修复分开。

- **技能 $N 简写语法**：现有识别范围为参数数量（至少 1）加 20，范围外金额如 $100 保留；这与明确的 $ARGUMENTS[N] 语法并不统一。单次替换修复保留金额测试契约，后续若统一语法需明确金额/转义规则后独立变更，不能默默扩大替换范围。

- **技能正文读取边界**：frontmatter helper 已在 fix-skill-frontmatter-read-bound 限制底层消费，但bound-skill-description-preview 已限制描述回退的底层读取与 UTF-8 截断；显式正文加载仍可完整读取文件，需独立核对容量及错误行为，不把 metadata 限额直接套到正文导致静默丢内容。

- **技能 reset 的来源所有权**：grow/skills/reset 将整个 SkillsConfig 设为 default，包含注释称由 launcher 注入的 server_skill_dirs/bundled_skill_dirs。当前只找到配置反序列化入口，尚未发现仓内实际 launcher 写入者；不能仅凭注释认定重置行为错误。后续厘清用户配置与注入来源边界后再定义 reset 范围，不混入配置读取错误保护。

- **设置并发保存结果归属（普通设置已修复）**：实际 dispatcher 已复现旧失败覆盖新选择；fix-setting-persistence-order 按 key 单次在途、最新选择合并及回滚基线传递，完整 dispatcher 879 项通过。独立 PermissionModePersist 通知策略未改，退出时排空/强制 abort 不在本次保证内。

- **默认权限保存顺序（已修复）**：fix-default-permission-persistence-order 已复现并修正旧失败覆盖最新默认值，接入普通 PersistSetting 顺序协调；完整 dispatcher 881 项通过，当前会话隔离保持。旧独立协议更新到删除候选 R8，尚未删除。

- **技能管理路径别名一致性（部分已修复）**：fix-skill-path-alias-management 已统一现存路径的 tilde/canonicalize 比较，临时符号链接回归验证添加去重、ignore 清理、移除和计数，保留原始配置写法。fix-skill-config-env-path-comparison 进一步修复 `${VAR}` 原始配置比较，保持原文及请求字面语义。fix-missing-skill-path-anchoring 已修复目标不存在且 cwd 相对时未锚定绝对路径的问题。尚需定义 cwd 不可读取时的错误传播、缺失路径包含 .. 的身份比较和已删除符号链接的历史目标边界；不可通过随意折叠 .. 改变现有链接语义。

- **技能发现阻塞执行边界（扩展重载已修复）**：fix-skill-reload-execution-boundary 将扩展重载放入有界 blocking worker，超时后保留许可直到实际退出。其他 session/inspect/workflow 调用者、配置来源展示仍同步扫描；永久 OS 阻塞与 runtime shutdown 边界也需独立审计，不把请求超时解释为任务已中止。

- **技能自动来源与递归链接 scope**：fix-config-skill-scope-aliases 已让配置根按规范路径分类。fix-skill-discovery-cwd-alias-boundary 进一步规范自动发现 cwd/Git root，修复别名越过仓库边界，同时保留 .grow 链接入口的 Local scope。用户 home 别名边界与配置根内递归链接继承根 scope 的政策仍需单独核对，不将根级修复扩称为逐文件来源分类已完成。

- **技能无效元数据的回退策略**：reject-oversized-skill-frontmatter 已阻止超限 header 降级成普通技能。reject-malformed-skill-metadata 已拒绝未闭合 header 与 YAML 错误，停止生产重新引号化/标量抢救，合法无 frontmatter 回退保留。validate-skill-paths-field-types 已严格拒绝 paths 错误类型及混合列表；validate-skill-invocation-booleans 已严格校验两个调用开关；allowed-tools 等其他字段级宽松转换需后续独立核对。

- **技能 allowed-tools 执行语义**：audit-skill-allowed-tools-consumers 确认当前仅解析/序列化/详情展示，未发现权限判定消费者；用户指南已说明。若要升级为执行约束，需先定义技能调用起止、多技能合并与会话权限上限，不能与 CLI disallowed_tools 混同。解析错误类型丢字段属于元数据质量问题，另行处理；现阶段不据字段名称宣称权限漏洞。

- **技能展开诊断的成功时点（已处理）**：fix-plugin-skill-use-outcome 将 admission 的 PluginUsed.success 与逐引用正文加载结果对齐。SlashCommandUsed/SkillDispatched 仍表示尝试/派发，active_skill 与 skill.activated 保留 turn 归属含义；不将这些事件等同于加载成功。

- **剪贴板统计单位（已处理）**：fix-clipboard-character-stats 将共享 clipboard_stats_suffix 改为 Unicode 标量计数并处理 char/chars 单复数，保留 str::lines 行数规则；复制、TUI/CLI 导出共用此函数。
- **复制/导出剩余响应性**：background-transcript-file-writes 已将显式文件 I/O 移入应用内有限串行后台队列，并验证顺序和原会话反馈。正文渲染及剪贴板/默认备份仍同步，大文稿仍可能产生长帧；需单独测量，不能宣称整个命令已完全非阻塞。队列只限制请求数量，不限制单请求字节数。

- **复制文件原子提交（已处理）**：fix-atomic-copy-file 已改为同目录私有临时文件完整写入后提交，保留失败前的旧内容和权限，成功目标为 Unix 0600；显式路径及默认备份共用实现。显式文件已由后台队列处理，默认备份仍同步。

- **外部分页器剩余响应性**：fix-private-pager-transcripts 处理临时快照权限和所有权；普通 Markdown 渲染、minimal 完成后的快照写入仍同步。后续应测量慢文件系统影响，独立处理，不把本次私有文件修复描述为后台写入。崩溃清理需要额外生命周期策略，本次不新增常驻清理机制。

- **外部分页器失败反馈（已处理）**：fix-pager-process-feedback 保留启动错误和退出状态，恢复界面后显示失败原因；原有临时文件所有权和超时重试保留。fix-pager-quoted-arguments 进一步修复引号参数边界，保持直接启动；排队请求跨视图来源关联仍需独立审计。原始证据：verify-cli-transcript-files。

- **Minimal transcript 重连截短（已处理）**：fix-transcript-reload-restart 在生产重连入口使旧构建失效，reload 期间等待，完成后从最终正文重取 ID 并清空旧前缀。回归覆盖完整 replay、cursor 恢复、失败回滚及两帧间完成的 reload；保留无关 agent 和标签切换。原审计：audit-transcript-reload-boundary。

- **滚动记录器运行边界**：默认关闭；启用后首条记录 lazy-open，同步 BufWriter 写入并在 finalize flush。fix-scroll-log-default-collision 处理默认同秒路径覆盖。fix-scroll-log-failure-state 已修复 Disabled 后状态误报及重试需两次切换。后续需独立检查日志增长上限、慢文件系统对输入的影响及显式文件路径覆盖语义。不因诊断功能可选而直接删除。

- **Minimal FPS HUD 缺失（已处理）**：fix-minimal-fps-hud 对 minimal draw hook 计时并在 viewport 顶部预留两行读数，空间不足优先保留正文，禁用不新增采样或计时器。旧 FrameMetrics/dev overlay 说明已纠正；采样仍不等于后台 PTY 完成或屏幕绘制延迟。

- **分页器反馈来源（已处理）**：audit-transcript-feedback-origin 确认成功文件交接只保留 path/ANSI，等待和进程失败按当前视图发布。fix-pager-feedback-origin 已将文件、ANSI 和原 root/child 会话身份绑定到同一请求，重试保留来源，失效来源走独立可见通知，不写入新会话正文。Minimal 子 agent 的输入/绘制/导出目标一致性仍待单独验证，不混入通知修复。

- **词选择提示退场测试误报（已处理）**：fix-word-select-tip-retirement 的诊断显示 key=clipboard_image_tip、prompt=a、snapshot=None，词选择提示实际已正常退出。测试现在关闭 image_input 探测，避免读取主机剪贴板，并断言词选择提示身份退出而非要求任何提示都不存在；root 1270 项全通过，无产品行为修改。

- **剪贴板元数据原生调用边界**：fix-clipboard-tip-probe-contention 去除 UI 等待原生读图锁，但首次 AppKit OnceLock/dlopen 和持锁后的原生消息仍无总超时。fix-clipboard-metadata-snapshot 已在分类前后核对版本并拒绝类型不可用结果，纠正“单次调用即原子快照”的注释；这只检测采样期间观察到的版本变化，返回后的外部复制及原生调用总超时仍不在保证范围。

- **AppleScript 剪贴板回退剩余边界**：fix-private-clipboard-probe-files 已消除固定临时路径冲突并显式以 0700 创建目录。fix-clipboard-applescript-path-arguments 已将三条图片脚本的路径改为 argv，实测特殊字符参数往返与脚本编译；bound-clipboard-applescript 已将三条图片脚本限制为5秒及每流1MiB并回收拥有的进程组，清理失败显式报告；pbpaste、原生调用和原生/回退图片文件读取及解码仍需后续核对执行与字节上限。不在临时目录修复中混入进程生命周期与内容预算。

- **TUI doctor 同步采集（已处理）**：audit-doctor-execution-boundary 确认 report 和 fix 均在 dispatcher 同步收集报告，随后才后台规划；background-doctor-report 已将 report/list/fix 的采集移入单许可后台执行，保留会话绑定与修复确认；取消等待者不会提前释放仍运行的采集名额。bound-tmux-diagnostic-output 已限制 stdout/stderr 各64 KiB，超限终止进程树并返回错误；真实子进程提前结束与边界回归通过。

- **托管配置回滚与并发编辑（已处理可观察冲突）**：transaction::rollback 仅校验父目录，随后直接 rename 原内容或 remove 新目标。发布后、校验/同步失败前发生外部编辑时，恢复路径可能覆盖该编辑；现有 CorruptTemp 测试要求恢复任何字节变化，不能区分外部修改与本事务损坏。需独立建立所有权/冲突契约，保留恢复材料，并测试原有目标与新目标。不得把 optimistic revalidate 宣称为对非协作写入者的原子 CAS。

  fix-managed-config-rollback-conflicts 已对发布身份、字节、模式及符号链接核验，观察到变化则保留目标和原始备份，在 Recovery 报告实际备份路径。检查与最终文件操作之间的非协作写入竞态仍存在，不宣称原子 CAS。

- **托管条目批量规划的重复解析（已处理）**：audit-managed-format-roundtrip 核对 plan/item_state/render_update，n个请求执行2n+2次完整解析，预览还会再次解析。现有typed_inspection测试并未验证单次解析，已纠正名称。doctor现有规划通常只有1条，尚未测量运行时间；先以大量条目与接近读取上限的配置测量，再独立设计共享解析或批量渲染，必须保留未知条目、顺序、换行和最终格式校验。

  measure-managed-plan-batches 已测得release在1 MiB配置上1/16/64条中位2.758/21.779/84.515ms。已立项 batch-managed-config-render，基于同一原文解析批量渲染，保留最终校验和格式语义。

  batch-managed-config-render 将规划解析次数降为2次，同探针release的1/16/64条中位1.927/1.831/1.803ms；保留完整输出解析、未知条目顺序及换行行为。该数据为本机小样本，不是跨平台性能承诺。

- **tmux配置目标发现**：TerminalContext::tmux_config_path 的诊断提示固定普通~/.tmux.conf、Byobu~/.byobu/.tmux.conf；自动修复后者使用有效BYOBU_CONFIG_DIR，普通仍用HOME/.tmux.conf。需独立核对tmux构建的XDG搜索路径、-f、config_files与多层source场景，避免把静态默认路径称为实际加载路径。correct-tmux-reload-guidance只修复重连不能代替reload的文字，不改变目标选择。

  audit-tmux-config-authority 已核对上游config_files未转义逗号拼接与启动候选语义，不能作为唯一已加载目标。后续已立项 resolve-tmux-config-target，贯通显式路径、候选证据和统一显示/写入；已归档为2026-09-08-resolve-tmux-config-target，相关回归与规范校验通过，真实tmux服务器验证因本机缺少tmux未执行。


- **剪贴板编码图片预算（macOS读取已处理）**：bound-macos-clipboard-image-bytes 将原生Rust复制前长度及回退文件流读取限制为50,000,000字节；超限错误不再回退AppleScript。NSData系统内部获取/分配、脚本创建文件时的瞬时磁盘占用、非macOS读取、图像解码像素预算仍需分别审计，不能用后置传输限制宣称这些资源已受限。


- **占位图片文件实际读取预算**：load_canonical_placeholder_image先metadata检查max_bytes，再fs::read并事后检查。文件增长时仍可能在拒绝前完整分配；bound-placeholder-image-reads 已通过打开文件后limit+1读取形成实际预算，并保留allowlist、MIME验证及错误分类。路径规范化/metadata到open之间的身份变化仍需单独审计，不宣称这些检查构成原子快照。

- **Dashboard 混合路径与异步 file URLs 回退**：bound-drop-batch-retention 核对发现 dashboard/state.rs 的图片专用入口经 try_read_images_from_paste 过滤 NonImage，混合图片/普通文件批次会只插入图片；异步 file_urls 分类未命中时，两种界面只用 ctx.source.text_to_insert_on_miss 回退，原剪贴板无文本时不会保留探测得到的 URLs。需单独统一路径插入和失败文本所有权，验证 dispatch/peek/question、无原始文本、预算拒绝和已有附件上限。批次内存限制仅保证分类器不返回部分结果，不构成所有 UI 回退已保真的证明。

  preserve-dashboard-mixed-drop-paths 已处理 Dashboard dispatch/peek 三个插入入口的混合 NonImage 保留，并使明确但失效的 file URL 保留为路径文本；243项 Dashboard 回归通过。仍未分类的异步 URLs（如批次预算拒绝）在原始文本缺失时的回退问题仍待独立修复。

  preserve-unclassified-clipboard-file-urls 已补齐两种界面的分类未命中回退：成功无栅格探测、未分类 URLs 且无原始非空文本时插入 URL 原文；已有文本、已分类结果及拒绝/失败分支不重复插入。Agent粘贴78项、Dashboard245项通过。以上混合路径/异步URL回退事项已处理，像素预算和系统I/O边界仍是独立事项。

- **Kitty sips 转换临时文件与执行预算**：terminal/image.rs::convert_via_sips 在系统临时目录用 PID/时间戳构造文件名，File::create 与多个 ? 早退缺少统一所有权清理；Command::status 无时限，输出 fs::read 无实际字节预算。bound-overlay-conversion-pixels 只限制进入后端的源像素数，不宣称这些边界已修复。后续独立处理私有目录 RAII、失败路径回收、进程生命周期和转换结果读取上限，保留颜色配置处理。

  bound-overlay-conversion-pixels 已在 Kitty 非 PNG 转换进入 sips/Rust 后端前限制源像素为16,000,000；直传 PNG/iTerm 的终端解码、其他全图解码及转换进程/输出资源边界未由此覆盖。

  own-sips-conversion-temp-files 已将转换源/结果放入唯一私有目录（Unix0700），以TempDir所有权回收正常和错误出口；14项终端图片回归通过。进程时限、后代清理和转换结果实际读取预算仍待后续处理。

  bound-sips-process-lifetime 已限制sips子进程执行10秒，使用现有ProcessGroup在退出/错误/超时回收组并reap直接子进程，16项终端图片测试通过。输出文件实际读取预算仍未处理；系统不可中断等待、逃逸进程组或拒绝终止不能由执行期限保证回收。

  bound-sips-output-reads 已将sips结果限制为非空普通文件、100,000,000编码字节，实际读取limit+1；Unix拒绝symlink/FIFO，超限仍清理目录。最终20项终端图片回归通过。验证期间正常退出进程测试偶发EPERM，未定位，已独立立项 investigate-sips-process-eperm；不能用后续通过宣称该问题消失。写入磁盘前的产物大小和Rust回退编码器仍不在此读取预算内。

- **图片查看器真实入口同步加载**：audit-image-viewer-entrypoints 证实 prompt ImagePreview 调用同步open，后台链仅由测试构造loading状态触发。已立项 defer-live-image-viewer-loading 连接真实入口、内存/磁盘源、终端协议快照和逐次打开owner身份。open_from_path仅有测试调用，作为R20等待删除确认；不能将待接线后台链一并删除。同步/后台路径当前fs::read的文件字节预算需随后独立审计，异步化本身不提供该预算。

  defer-live-image-viewer-loading 已将实际Enter入口接入后台链，支持Arc内存/持久路径与输入侧协议捕获，每次打开独立owner；真实key handler集成4项、子Agent1项、共享图片162项通过。查看器文件读取预算仍需独立处理，关闭查看器不取消已运行的blocking转换，仅丢弃过期结果。

  bound-viewer-source-bytes 已对实际后台查看器加载增加50MB来源预算：内存复制前检查、普通文件同句柄检查与limit+1实际读取；165项共享图片回归通过。来源字节读取缺口已处理，聚合并发与转换缓冲预算仍独立，R20未删除。

- **Slash MRU 写入及加载边界**：isolate-slash-mru-temp-writes 已消除多个写入者共用 .json.tmp 的冲突，10项模块回归通过；完整快照仍是last-writer-wins，未实现跨进程合并。bound-slash-mru-loading 后续将首次加载限定为普通文件、1 MiB编码输入和最多limit+1实际读取，14项模块回归通过；同步慢文件系统仍无墙钟超时。coalesce-slash-mru-writes 随后将后台改为一个待写最新快照加一个在途快照，移除UI同步写入回退。它限制排队数量，不提供单快照字节预算、跨进程合并或退出flush。

- **输入历史审计 H1/H2/D1**：audit-prompt-history-and-draft-entrypoints确认真实history搜索sync_channel(256)+send可阻塞UI及Drop，需合并最新items/query且非阻塞通知；重开时复制旧shared snapshot，且结果无请求identity，需防过期结果被接受；local draft load未拒绝FIFO且缺实际读取超限判断，需同句柄regular/有界读取回归。三项分开修复，不借审计重构整套输入系统。selected空壳API已列R21，仅待确认删除。

  H1已由coalesce-history-search-requests修复：pending合并items/query，容量1非阻塞通知，Drop停止优先；history过滤测试48通过/1既有隔离测试忽略。H2旧快照请求身份、D1草稿源读取仍未修复，不以H1结果代替验证。

  H2已由reject-stale-history-search-results修复：提交编号经合并和worker原样回传，UI清除旧选择并仅接受当前请求；history过滤49通过/1既有隔离测试忽略。D1草稿读取仍未修复。

  D1已由bound-local-draft-source-reads修复：同句柄regular检查、Unix非阻塞打开、解析前实际256KiB预算，超限保留quarantine；11项草稿回归通过。隔离目录保留预算、隔离路径并发替换和同步慢盘超时仍不在此读取修复范围内，后续审计需分别建立证据。

- **草稿关闭/删除生命周期**：retire-closed-agent-draft-state已立项：关闭Agent不清loaded会使同session新Agent跳过恢复并可能用空记录删除磁盘草稿；keys/tracked也长期保留。另发现RPC transfer先disarm再remove，删除失败只warn无pending重试；capture失败清tracked再删除也没有单独删除deadline。后者的撤销/重试语义需独立修复，不能用关闭生命周期修改掩盖。当前均为源码状态证据，待回归复现。

  retire-closed-agent-draft-state现已修复关闭后loaded残留：最后owner离开检查点，成功释放干净状态，失败保留pending/backoff且重开优先内存；16项草稿回归通过。RPC/capture删除失败的重试仍未解决，与这项区分。

  删除失败问题已独立立项retry-local-draft-invalidations：需逐键删除意图、deadline、重开抑制和新内容取消，并覆盖cwd/session双键；内存重试不承诺进程死亡后的持久化删除保障。尚未实施。

  retry-local-draft-invalidations已完成：逐键内存删除deadline接入定时重试，绑定不迁移待删旧文件，关闭/重开抑制恢复，新payload取消该键旧删除；18项草稿回归通过。进程死亡后的意图持久化和跨进程冲突不在本项保证内。

- **导出路径补全扫描预算**：bound-export-completion-enumeration已立项。1000上限当前只统计过滤后可见items，隐藏项/错误可绕过，UI同步遍历量无界；改为迭代器结果计数。/debug与scroll-debug都真实启用，冗余别名列R22待确认，HUD不属于删除候选。

  bound-export-completion-enumeration已完成：take(1000)在错误/隐藏项过滤前执行，保持100项结果与排序；6项命令回归通过，计数测试有限1001项可明确发现回归。同步单次文件系统延迟仍无时间保证。

- **滚动日志特殊目标**：reject-special-scroll-log-targets已修复显式FIFO可能阻塞输入的问题，Unix非阻塞打开并在截断前同句柄检查普通文件；9项日志回归通过。记录大小、慢盘同步写入和多进程显式路径覆盖仍是独立边界。

  bound-scroll-log-recording-bytes已为每个记录器加入64MiB完整行预算，停止前flush并禁用，11项日志回归通过；无自动轮转/旧文件删除，不等于目录总配额。slow IO和显式路径多写入者仍单独审计。

- **输入诊断目标归属**：align-input-dump-target-ownership已立项。root两处record_input只消费父view delta，child输入inner无自记录；dump只取顶层Agent，dashboard popup会silent return。需联动记录/导出并验证父层截获，不能只换一个get_active_agent helper。已有200条/脱敏/独占私有文件保障保留；Other枚举项列R23待确认。

- **Pager unified log 生命周期**：audit-pager-log-forwarding-lifecycle 核对到 drain 后缺少当前 runtime 会丢批次、未初始化 flush 会清空缓存、重复 init 会增加 timer；已立项 preserve-pager-log-dispatch-ownership。独立债务：flush_blocking 不等待先前 detached batches，退出等待无期限；初始化前 Vec、单条字节数及 ACP/任务积压无总体预算。16条触发发送不等于背压。尚未声称真实 plain-thread 调用或内存耗尽复现。

  bound-pager-log-flush-wait已为当前批次ACP确认加入2秒等待上限，真实保留确认句柄和断线测试通过。此前detached批次的退出交付屏障、消息字节/积压预算仍未处理；该上限不代表整个退出过程或远端执行的总时限。

- **Recap接纳与能力变化**：reject-unqueued-recap-requests处理会话命令通道关闭却返回ok的问题。另有独立窗口：shell运行时关闭recap会返回{ok:true,disabled:true}，pager SendRecap忽略响应体；初始化时能力仍为true的手动请求可能保留等待提示。需明确disabled应答消费与能力更新，勿以异步生成成功替代接纳语义。当前配置解析默认ON，连接结构原有OFF注释过时。

  handle-recap-admission-responses已处理disabled响应消费：pager解析实际扩展封装，只对ok=true且未disabled的响应等待通知，其他走既有session错误清理；40项pager recap回归通过。没有永久关闭连接能力或引入重试。

- **自动recap的会话归属**：通知分支对任何实时root recap调用app级FocusTracker.mark_recap_shown，后台会话结果可能抑制当前会话的自动请求。isolate-recap-replay-feedback先隔离历史回放的当前状态副作用；实时多会话归属、重复focus-lost事件重置离开计时仍需分别核对，后者现有测试明确期待重置，不能直接当无争议bug修改。

  scope-away-recap-by-session已按SessionId隔离当前离开周期内的展示记录和90秒重试退避，两自动入口共享当前会话资格检查；44项recap回归通过。跨离开周期迟到通知的归属仍沿用现有行为，未引入周期/request标识。

- **公告隐藏状态存储**：audit-announcement-storage-boundaries确认read_to_string无普通文件/字节预算且在启动await；写入丢弃错误，pager恒报Ok；hide/show/prune多个snapshot独立spawn，可乱序覆盖。commit-announcement-hidden-state先处理完整快照提交和真实错误；读取预算和进程内最后状态顺序需独立处理。

- **公共atomic writer临时文件归属**：config/src/fs_atomic.rs在create_new失败后仍remove_file计算路径；若pid+nonce路径预先存在，会尝试删除未拥有的文件。公告提交改动不复用该helper，后续应独立验证碰撞/失败清理并修复公共实现。

  commit-announcement-hidden-state已实现独占临时文件+sync+原子替换，并向pager传递IO错误；库11项、pager45项通过。首个effect测试因GROW_HOME OnceLock误写真实隐藏偏好，测试数据已移除但原值无备份；事故及修正后的独立进程隔离见该change。快照排序/读取预算仍未处理。

  preserve-unowned-atomic-write-temporaries已修复公共helper创建失败时误删未拥有temp：open失败提前返回，成功创建后才清理；3项显式路径回归通过。没有新增碰撞重试或目录对抗竞态处理。

  bound-announcement-state-bytes已限制普通文件读取及原子写入快照为1 MiB，同句柄metadata和实际limit+1读取双检查，Unix FIFO非阻塞拒绝；13项库回归通过。快照排序、普通慢文件系统期限及上游配置大小仍独立。

  serialize-announcement-state-writes已将hide/show/prune汇入AppView单写入调度，忙时合并最新状态，成功或失败完成后推进pending；47项pager公告回归通过。跨进程合并和退出drain仍无新保证。

- **权限与诊断缓存读取边界**：permission/state.rs 的 try_load_state 仍用无界 read_to_string，非 NotFound 读取失败与文件不存在均触发共享权限回退；diagnostics/id.rs 接受任意非空缓存且无普通文件/字节预算。当前仅源码证据，需独立明确失败语义与测试，不混入 isolate-permission-client-cache 的文件名映射修复。

  stop-permission-fallback-on-read-errors 已修复非 NotFound 读取错误继承共享授权：修复前真实 UTF-8 错误回归失败，修复后 27 项状态测试通过，失败源保留。普通文件/字节预算及诊断缓存边界仍待处理。

  bound-permission-state-io 已对权限缓存加入同句柄普通文件检查、Unix 非阻塞打开及 1 MiB metadata/实际读取双预算，写入同预算超限保留旧目标；30 项状态测试通过。诊断缓存、普通慢盘期限、内存授权及目录总量仍独立待审计。

- **诊断设备标识可达性**：audit-diagnostic-device-id-reachability 未找到 diagnostics::id::agent_id 的仓库调用方，列 R25 待确认删除。此前记录的读取/格式/权限及 mid 子进程边界属于闲置公共路径，不应声称已影响当前启动。若保留或启用，须独立建立读写和计算预算契约；本轮未修改代码或调用设备指纹计算。

- **Debug firehose 保留与资源生命周期**：安装 per-session logger 时才执行七天 mtime 清理，没有周期/字节配额；旧 mtime 不证明文件无打开的写入者，现有注释的绝不删除活动文件保证过强。SinkMap 每会话持有 worker 到退出，并发首次打开可停放重复 guard。需独立明确日志配额、活动文件保护与 worker 归属；preserve-unowned-debug-link-temporaries 只处理 latest 临时路径预删除。

  retire-redundant-debug-writers 已修复同sink并发首开冗余guard终生停放：仅选中writer移交全局registry，loser在routing锁外flush/retire；23项debug回归通过，独立进程验证session/fallback共2个guard及全部首写行。仍允许短暂重复打开，不同session累计资源与日志保留配额尚未解决。

  protect-open-debug-logs-from-pruning 已修复协作writer长期空闲被mtime清理的问题：writer持shared lock，普通日志删除需exclusive lock并重新检查句柄mtime。跨进程旧实现删除回归失败、修复后24项通过；非协作旧进程/对抗路径替换不在保证内，字节配额和周期清理仍待处理。

- **统一日志裁剪与写入预算**：bound-unified-log-trim-reads 已改为同句柄长度定位尾部、最多读取2.5MiB，20项回归通过。trim独占锁仅协调trimmer，write_lines未参与文件锁，追加与rewrite/truncate并发仍可能丢行；没有换行仍不裁剪，记录序列化和snapshot读取需独立审计，5MiB维护阈值不是硬磁盘配额。

- **统一日志快照可达性与测试隔离**：isolate-leader-integration-log-output确认snapshot_log仅有测试消费者，snapshot_session_log无仓库调用方列R26待确认。stdio integration缺失pre-main日志重定向已补齐，子进程临时home回归验证没有日志落入home；仅执行新增隔离测试，未声称其他测试副作用均隔离。snapshot字节预算仍未变化。

- **图片正规化并发预算**：serialize-image-normalization-workers核对并保留已完成的混合拖放/URL回退，发现跨session完整decode适配器无全局门控，现以单许可在blocking闭包内持有直到实际完成（取消也不提前释放）。缓存/取消13项及图片正规化41项通过。排队编码payload、arboard系统RGBA获取和PNG编码、直传终端解码内存仍为独立预算边界，不能把进程级单decode等同于总内存上限。

- **图片缓存启用入口**：audit-normalization-cache-activation确认全仓无apply_remote_settings_side_effects调用，global默认关闭；R27列可选缓存与断开hook待确认，未擅自开启。若保留需独立确定配置来源和startup/update连线；非原生转码在缓存前，不能宣称已被缓存合并。关闭不清存量仅是当前语义，没有单凭此认定内容错误。

- **图片资产失败生命周期**：cleanup-failed-image-asset-batches已在保存函数内部失败时回收已成功创建的本批次资产，保留原文件与primary error；19项描述/资产测试通过（含第二次IO失败注入）。当前write_atomic发布后sync失败已是warn，不是普通写入错误，勿重复修复。保存成功后后续admission失败及进程崩溃的资产回收仍需独立审计。

- **资产保存与输入commit边界**：audit-image-asset-admission-boundary确认资产先存、随后commit；AcknowledgementLost不证明未提交，禁止据此盲目删除。UUID保存导致重试副本；make-image-assets-retry-safe已立项，待核对durable input身份/合并/恢复并选择稳定namespace与回收归属，尚未实施。

  make-image-assets-retry-safe已实现有序内容批次目录、独占stage/no-replace发布与逐文件验证复用；重试不重新写文件，并发只发布一批，失败只清stage，21项图片描述/资产测试通过。ACK错误不删除已发布批次的原接纳逻辑保留（源码核对，未加完整ACK丢失注入）。旧UUID资产、不再引用的不同批次以及崩溃stage回收仍未处理，跨批次单图去重也不作保证。

- 同进程 close/load writer lease 退出边界已由 `join-session-persistence-owner` 修复；验证记录见归档 change。
- 跨重启 catalog endpoint/provider identity 变化与模型链连续性：当前只投影 model ID/effort，transport fingerprint 不能重建原 route。需明确 durable route 变化的记录策略，不能靠跳过 continuity 校验处理。
- SessionThread 最后 owner 的 Drop/reaper 停机路由已由 `stop-persistence-in-session-reaper` 补齐；析构只请求 eventual stop，完整 writer 释放确认仍由显式 drain 提供。

- **TTL 清理候选句柄预算**：bound-search-bootstrap-readers 已修复搜索枚举和 Timeline 读取的 O(session count) 句柄持有，真实 96 session / soft fd=64 bootstrap 回归通过。cleanup_stale_sessions_sync 仍通过 scan_opened_sessions(None, identity) 收集全部 OpenedSession，候选目录句柄随数量增长；这是待独立验证的清理资源边界，不能直接以搜索回归宣称修复。后续先确定重复身份检测、全扫描失败与删除副作用的顺序，再设计受限候选处理，禁止简单边枚举边删破坏现有重检。

  bound-cleanup-candidate-handles 已完成该 TTL 句柄预算修复：先 Summary 全扫描，再逐候选共享根 authority、独立维护缓存/lease；96过期session/fd64旧实现只删49且errors=0，修复后全删96。刷新后保留、维护锁释放、调用方活跃writer保留及重复ID全扫描失败均有回归。扫描器对目录/摘要读取错误直接continue的分类策略仍待独立处理；本项不声称任意IO失败均完整报告。

  propagate-session-scan-io-failures 已修复扫描IO错误分类：仅NotFound/InvalidData继续跳过，其他cwd/session打开、summary读取和physical identity标记读取错误上抛。真实Unix四权限场景验证扫描失败且清理零删除，20项相关回归通过。搜索接收扫描失败后不进入prune/完成标记为源码顺序验证，未全入口权限注入；已有无效条目的索引保留策略未变更。

- **回退历史解析与资源边界**：propagate-rewind-history-load-failures 处理了IO失败仍继续部分内存结果的问题；read_rewind_jsonl_from_file 仍逐行跳过JSON反序列化错误，且read_line和全量点集合没有明确字节预算，同步读取仍在async路径。需独立验证坏JSON行是否让回退提交部分投影，不能以本项invalidUTF8 IO回归代替；metadata picker降级策略和无生产消费者的workspace::rewind_files也应分开审计。

  reject-malformed-pinned-rewind-records 已将active pinned读取的serde失败改为InvalidData+物理行号，31项回退点测试通过，包括不合并prefix、保留live/source、修复后retry和metadata fallback。读预算/同步IO仍待处理；metadata只计数/跳过snapshot内容，不能宣称与完整FileSnapshot反序列化等价。cfg(test)旧path读取器仍未删除。

  metadata到UI链路进一步审计发现picker目标来自prompt索引，metadata只补充文件改动信息；因此metadata验证强度不决定目标是否展示。offer-file-rewind-for-later-checkpoints已修复模式资格只看target自身、漏掉later checkpoint的问题（45项UI/dispatcher测试通过）。metadata失败与“确认无文件改动”的展示区分、嵌套snapshot验证仍为独立事项。

- **回退异步结果归属**：points-loaded、preview-complete/failed handler仅按AgentId应用结果，未检查仍有对应请求，dismiss会清空state/points。迟到结果可能重建弹窗或覆盖新请求状态。需独立跟踪关闭、重开、会话切换和实际执行结果的不同语义，再设计请求归属及回归；本次preselected range修复不包含此项。

  ignore-obsolete-rewind-read-results 已为points/preview读加入UUID、session和phase归属，dismiss清除、新读替换、匹配结果消费；32项dispatcher测试通过，包括旧成功/失败不覆盖重开请求、session变化、phase变化、跨view有效结果和points失败恢复draft。execute仍按原有已提交结果同步，未增加请求取消或后端协议。

- **回退执行结果与会话切换**：RewindExecuteComplete/Failed目前只带AgentId，执行已经可能提交，不能套用可取消读取的弹窗生命周期过滤。需追踪fork/load与执行是否可并发，以及解绑/重绑后的实际结果应如何同步归属会话；通用bind/unbind未重置rewind overlay，也需明确草稿归属再处理。本轮仅以已有binding epoch关闭读取结果的同ID重绑缺口，未声称上述执行路径已证明有用户可达损坏。

  bind-rewind-reads-to-session-epoch 已复用现有u32 binding epoch替换保存的sessionID，关闭unbind后同ID重绑接受旧结果的缺口。33项dispatcher回归通过，包含无unbind的同ID重复bind仍有效。通用overlay reset与执行结果归属仍待独立核对。

- **单个thinking block的多次signature delta语义**：audit-thinking-signature-delta-semantics核对官方Python/Java替换与Go追加的差异；现有真实proxy样本只有一次delta，不能据此认定需要拼接。保留当前赋值，若收到分片报告需获得同block多事件及续接接受证据，禁止仅凭delta名称或网络包分片推导签名拼接。此项不是已复现bug。

- **采样请求日志中的凭据前缀**：审计短效 helper token 时发现 `sampler/src/client.rs::post` 的 `client_post` info 日志输出 Authorization 前 20 字符和 x-api-key 前 12 字符。短凭据可能完整落日志；需要独立核对 sampling_log 落盘/转发消费者及现有 attribution fragment 用途，再删除或替换原始头前缀。本次短效 token 修复不混入日志契约修改。

  remove-credential-fragments-from-sampling-logs 已修复 client_post 原始头前缀，并一起移除 sampling_request 的 auth_prefix；45项客户端测试通过，包含内存 JSON 日志捕获与真实 request builder 认证头断言。已有日志不回写，401 attribution 回调及其消费者未变更，不能据此声称所有诊断路径均不含凭据片段。

- **CLI 覆盖分母与 trace 配置依赖**：map-cli-audit-coverage 将 19 个顶层 Command 与实际分发对应；这是入口枚举，不是总体审查百分比。Trace 分支加载模型配置并构造未传入导出函数的 AgentConfig，需隔离验证无效配置是否阻止故障资料导出。嵌套命令、隐藏 flag、默认 TUI/slash/tools 仍须独立枚举，不能由归档数量推算覆盖率。

  decouple-trace-from-model-config 已用隔离 HOME/GROW_HOME 子进程复现 Trace 因 TOML 错误提前失败；移除无用强制配置加载后，实际 async_main 返回会话缺失错误（1项回归通过）。共同启动的 best-effort 配置读取保留；未以此证明完整 CLI main 或有效会话归档的所有行为。
## 待核对：nono平台沙箱契约与运行时实现边界

`third_party/nono`同时提供能力清单schema/codegen、Landlock/Seatbelt/Windows等平台实现、命令与文件描述符策略、环境变量和资源限制。39条静态契约已登记，但未运行平台沙箱、build.rs生成或跨平台权限矩阵；后续需按平台后端、manifest编译和进程执行边界拆分动态验证，本轮不改第三方实现。

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

- Pager 审计债务：Effects helpers mix wire parsing, persistence policy and UI projection：One effects module and helper file contain ACP JSON shapes, filesystem persistence, permission notification policy, session picker mapping, model/status formatting and task result construction, so changes can cross transport and UI boundaries without a type-level contract.
- Pager 审计债务：Test fixtures mutate process-global environment：setup_grow_home_in_tempdir changes GROW_HOME with unsafe process-global state; parallel tests or unrelated code can observe the temporary path unless the surrounding runner serializes access.
- Pager 审计债务：Best-effort persistence has policy encoded in result variants：WithRollback and BestEffort differ in notification, rollback and TaskResult semantics; callers must preserve the distinction or can report a false persisted state.
- Pager 审计债务：Session picker parsing and roster projection duplicate identity policy：UpdatedAt/title filtering, restore metadata and dormant roster mapping are spread across parsing helpers and dispatch consumers, leaving canonical session identity and display policy distributed.
- Pager 审计债务：Wire-shape compatibility relies on stringly typed JSON keys：screenMode, askUserQuestion, permissionMode, grow/listScope, codeRestore and ACP outcome kinds are validated by ad hoc JSON parsing and tests rather than a shared typed schema.
- Pager 审计债务：render_with_scratch still carries a TODO to make rendering pure after layout preparation; the immutable state signature is present, but the planned PureWidget-style abstraction is not implemented.
- Pager 审计债务：render_content has a large argument surface and performs delegation, selection post-paint and selection-box derivation in one method; splitting those responsibilities would be a separate refactor.
- Pager 审计债务：The pane relies on runtime expect calls for prepared caches and paint-range bounds; callers must preserve the prepare_layout invariant.
- Pager 审计债务：Sticky header rendering uses a reusable scratch buffer only for clipped headers while other content allocation and delegated work remain outside this file; frame-level allocation/performance is not established here.
- Pager 审计债务：The nested end-to-end module mutates internal CTA state directly to arrange phases, which keeps scenarios deterministic but leaves a gap between public user actions and some reducer preconditions.
- Pager 审计债务：Test setup is duplicated across many top-level cases; shared builders could reduce fixture drift, but that refactor is outside this audit.
- Pager 审计债务：isolate_grow_home uses a process-global OnceLock and unsafe environment mutation, so test isolation depends on the first caller and process-wide environment state.
- Pager 审计债务：MCP polling and dismissal behavior is verified through synthetic TaskResult dispatches rather than a controllable scheduler or clock, leaving timing contracts partially indirect.
- Pager 审计债务：The suite contains both focused dispatch checks and nested composed flows with overlapping success/error coverage; the ownership boundary between unit and end-to-end evidence could be made clearer.
- Pager 审计债务：The modes suite mixes tip, behavior, permission, modal snapshot and theme concerns in one dispatch test module, which makes ownership boundaries harder to discover.
- Pager 审计债务：Several security-critical tests build large PermissionViewState fixtures inline; a stable fixture builder could reduce duplication, but its semantics would need to remain explicit.
- Pager 审计债务：Tests mutate process-level appearance and theme caches, so isolation relies on manual restoration and helper environment scoping rather than an injected settings/cache dependency.
- Pager 审计债务：The always-approve contract is asserted through reducer effects and synthetic response channels; a clock/scheduler and shell contract harness would provide stronger integration evidence for queued transitions.
- Pager 审计债务：Behavior confirmation tests arrange a warning directly on the agent, leaving the code path that creates and expires that warning covered elsewhere.
- Pager 审计债务：SearchDaemon owns a detached JoinHandle and communicates through an unbounded channel; this avoids blocking input but leaves lifecycle and queue-growth policy implicit.
- Pager 审计债务：ScrollbackSearchIndex rebuilds every searchable entry whenever content_generation changes; per-entry incremental indexing is deferred until profiling justifies added state.
- Pager 审计债务：Stale-result rejection compares both generation and raw query, duplicating identity across editor, request and snapshot; a single typed request identity could make the contract easier to audit.
- Pager 审计债务：The search state performs synchronous matcher compilation for immediate UI feedback and repeats matcher compilation on the daemon, trading responsiveness for duplicate work.
- Pager 审计债务：Tests reach private daemon snapshots/channel messages directly, which provides precise regression coverage but couples the suite to implementation details of the coalescing protocol.
- Pager 审计债务：A single module owns filtering, map-index semantics, selection persistence, delete confirmation, worktree selection and row presentation, so changes to one coordinate space can affect several consumers.
- Pager 审计债务：PickerItem uses separate implicit index namespaces (original entry index versus content hit index plus a numeric offset); a typed backing-key model would reduce reliance on the constant offset.
- Pager 审计债务：The same grouping/order logic is shared by map and rendering, but the contract is maintained by convention rather than one returned grouped model.
- Pager 审计债务：build_content_entry_data_at repeatedly calls filtered_indices.contains, making content-row construction linear in hit count times filtered-entry count for the common path.
- Pager 审计债务：Relative-time strings are computed during row-data construction and depend on the sampled clock, which can make snapshot stability and cross-surface rendering harder without an injected time source.
- Pager 审计债务：helpers.rs 将图片物料化、prompt 日志、MCP/CTA、session restore、错误清理、timeline 读取、picker/roster、全部设置写入、permission 通知、task kill 解析和 active-session 注册集中在一个 1030 行模块，职责边界高度耦合。
- Pager 审计债务：persist_setting 通过字符串 SettingKey 的大型 match 维护键到 writer 的重复类型映射；新增设置需要同时更新 registry、setter、rollback 与此处分派，编译器不会保证穷尽同步。
- Pager 审计债务：permission persistence 和通用 setting rollback 没有 request generation/revision 判定，旧完成结果可能覆盖较新的乐观状态；这是既有 delta 明确记录的未版本化语义。
- Pager 审计债务：多处 JSON 解析采用 wrapped-result/top-level fallback 或缺省值，兼容性便利会把协议漂移和字段缺失压低为静默空数据。
- Pager 审计债务：生产 helpers 没有同文件测试，行为证据分散在 effects/tests.rs、dispatch tests 与调用方，重构时容易出现契约与局部实现脱节。
- Pager 审计债务：UserPromptBlock 同时承载数据模型、非可信 replay 元数据校验、Unicode wrapping、主题样式、选择坐标、折叠估算和 BlockContent 能力，职责跨度较大。
- Pager 审计债务：token_styled_line 依赖 lines() 返回 self.text 子切片并用指针差恢复 byte offset，虽有 debug_assert，仍把实现安全性绑定到该切片不变量。
- Pager 审计债务：截断路径需先按样式行重排再拼接 ellipsis，并手工维护 LinkSource、Selectable span 和 source_column，布局与选择元数据耦合度高。
- Pager 审计债务：is_foldable 的固定 MIN_CONTENT_WIDTH=60 与真实 ctx.width 分离，在宽终端上可能提前进入可折叠状态；这是源码主动接受的保守策略。
- Pager 审计债务：Skill token range 使用 UTF-8 byte range，而测试和渲染同时依赖 span 索引与显示列宽，后续扩展复杂 token 或 grapheme 交互时存在多坐标体系维护成本。
- Pager 审计债务：模块同时维护 Markdown 检测、缓存键协议、用户设置显示策略、按钮文案/几何和输出行插入，检测层与交互呈现层耦合。
- Pager 审计债务：缓存文件名依赖手工 RENDER_REVISION=3；渲染行为变化若遗漏递增会复用旧 PNG，编译器无法约束。
- Pager 审计债务：按钮宽度和三列 gap 由静态标签与 UnicodeWidthStr 计算，绘制/命中虽共享 AffordanceRow，但国际化或文案变更仍需同步协议与测试。
- Pager 审计债务：post-wrap 插入通过 source_for 索引、insert_at+k 和反向插入维护文档顺序，调用方必须保持 prewrap_ranges 与 output.lines 同一帧映射。
- Pager 审计债务：MermaidContent 明确不保存 per-diagram render state，所有异步状态由 AgentView worker 层维护，跨模块状态边界清晰但重构成本较高。
- Pager 审计债务：One AgentView module centralizes search, todo, task, catalog, modal, viewer, question, dropdown, and base-pane routing; adding another pane increases precedence coupling and makes ownership review harder.
- Pager 审计债务：handle_scroll clones workflow view and synthesizes MouseEvent values for prompt scrolling, while the same method directly mutates many modal-specific offsets; scroll ownership is split across heterogeneous APIs.
- Pager 审计债务：Task action eligibility is reconstructed from session maps in the router, so display identity and action identity can drift if TasksPane projections change without updating this module.
- Pager 审计债务：Search paste has a separate route from key handling and deliberately consumes browse-mode paste as Unchanged; this implicit ownership contract is only covered by one inline test.
- Pager 审计债务：This dispatcher combines startup gating, AgentView construction, picker state cleanup, deep-search generation, restoration projection, error recovery, and reconnect control replay; further session behavior increases cross-domain coupling in one file.
- Pager 审计债务：Picker invalidation is centralized but relies on separate welcome list and deep-search counters, with surface liveness inferred from ActiveView and modal presence; this makes stale-response reasoning dependent on callers preserving those invariants.
- Pager 审计债务：Session-load success emits many unrelated effects and performs substantial state projection inline, so ordering between queue draining, metadata fetches, extension refresh, registration, notification, and descendant restoration is difficult to audit in isolation.
- Pager 审计债务：Reconnection traversal returns early when a view has no session id and therefore does not recurse into its descendants; this may be intentional ownership gating but should remain an explicit invariant when child views can outlive parent identity.
- Pager 审计债务：Selection reanchoring accepts a generic Option map but encodes grouped-header semantics locally; the same anchor policy can drift from picker rendering or input navigation unless a shared state helper owns it.
- Pager 审计债务：One dispatcher mixes user-facing clipboard/export/file I/O, viewer construction, extensions modal lifecycle, asynchronous result reconciliation, and diagnostics dump persistence, creating broad ownership and review coupling.
- Pager 审计债务：The extension fetch set and result handlers encode tab ordering and marketplace row-index arithmetic in dispatch code; renderer/navigation changes can drift from these local index assumptions.
- Pager 审计债务：Copy and export each format delivery notices independently while sharing lower-level providers, so wording, fallback semantics, and toast timing can diverge across entry points.
- Pager 审计债务：The input-log dump writes a timestamped file using a best-effort directory creation and no retention or collision policy; diagnostic persistence concerns are embedded in UI dispatch.
- Pager 审计债务：The block viewer constructor match is a growing RenderBlock taxonomy switch; new block variants require editing this dispatcher to preserve fullscreen/viewer coverage.
- Pager 审计债务：The shared module is a broad fixture registry spanning nearly every dispatch domain, so changes to AppView, AgentSession, modal state, ACP DTOs, and global settings all converge here and increase test coupling.
- Pager 审计债务：test_app manually initializes the full AppView field set; although make_test_agent_session centralizes session construction, root fixture changes still require editing a large literal and can obscure which defaults each test relies on.
- Pager 审计债务：Several helpers duplicate production assumptions (dashboard focusables, picker modal field layout, permission option classification) rather than exposing reusable production-owned builders, allowing test mirrors to drift.
- Pager 审计债务：Global theme and mouse-capture state require manual lock/reset conventions. A panic inside the callback or a test bypassing the helper can leak process state into unrelated tests.
- Pager 审计债务：Helpers use panic-on-invalid-fixture semantics for convenience (`expect`, direct indexing, and explicit assertions), which makes setup failures clear but limits reuse for negative-path tests.
- Pager 审计债务：The file combines loader lifecycle, protocol serialization, cache policy, media-link indexing, hit testing, Mermaid actions, native OS opening, clipboard dispatch, and gboom input; each new media surface increases cross-domain coupling.
- Pager 审计债务：Inline media CPU cache and Kitty GPU-id lifetime intentionally diverge, requiring reset and draw cleanup coordination that is easy to break when session/replay boundaries change.
- Pager 审计债务：The cache eviction policy removes the first map key rather than an explicit LRU/age record, so insertion order is an implicit policy and memory pressure behavior is not visible in the type.
- Pager 审计债务：Mermaid affordance layout/hit registration and action execution are split between rendering-owned vectors and this dispatcher-owned source table; stale or malformed indices degrade to an empty source while still consuming the click.
- Pager 审计债务：Filesystem retry, image preparation, native opening, and clipboard copying use separate error/reporting paths, producing inconsistent user-visible diagnostics and limited structured correlation.
- Pager 审计债务：History indexing, background-thread lifecycle, nucleo query state, UI activation, pointer hover, keyboard navigation, and selection projection are combined in one module, increasing coupling between matching policy and input state.
- Pager 审计债务：The daemon owns an optional JoinHandle but Drop performs only a nonblocking Stop send and does not join the worker; shutdown completion and worker lifetime are therefore implicit.
- Pager 审计债务：SetItems/SetQuery coalescing is implemented as an ad hoc message rewrite state machine; adding another history mutation requires preserving its atomicity and latest-query semantics manually.
- Pager 审计债务：The selected() API still returns Option<&HistoryEntry> even though snapshots contain HistoryMatchResult and the implementation always returns None; selected_text/result_at are the real APIs, leaving a compatibility-shaped dead surface.
- Pager 审计债务：The source order assumption and reverse-at-display policy are encoded in publish_matches rather than represented in a named history-order type, so an upstream ordering change can silently invert the UI.
- Pager 审计债务：view.rs combines snapshot normalization, terminal capability interpretation, clipboard policy, warning-to-finding conversion, remediation prose, and probe-note projection; changes in any diagnostic domain require editing one high-coupling module.
- Pager 审计债务：ClipboardRecovery retains legacy fix strings inside the view while manual findings carry separate guidance, so the compatibility facts field and user-facing remediation can drift.
- Pager 审计债务：RuntimeEvidence::Unavailable and sentinel-like Available(None) are interpreted in several local branches, making evidence completeness policy implicit rather than represented by a dedicated report-state type.
- Pager 审计债务：Stable DiagnosticId mapping and long remediation strings are hard-coded beside report assembly, which couples schema identity and copy text to the Rust control flow.
- Pager 审计债务：The WezTerm newline suppression guard depends on wezterm_shape plus runtime evidence availability and duplicates terminal-specific policy with other startup/formatter layers; cross-layer changes can silently alter which fallback is shown.
- Pager 审计债务：The block combines content lifecycle, elapsed-time policy, rendering, style policy, fold/display semantics, selection preamble, and appearance affordances, increasing coupling between data and presentation concerns.
- Pager 审计债务：EXPAND_HINT duplicates the literal key chord used by an input interceptor instead of consulting the keybinding registry, so remapped controls can advertise the wrong shortcut.
- Pager 审计债务：Replay timing is split between this block and ScrollbackState::finish_running_with_time, with implicit precedence between local Instant and server elapsed values.
- Pager 审计债务：Body de-emphasis is implemented as a style patch after MarkdownContent output and uses global legacy-console detection, leaving palette blending, terminal SGR support, and block styling policy distributed across layers.
- Pager 审计债务：The implementation returns finished_display_mode Some(Collapsed) while running and finished transitions are interpreted by outer entry/state code; this mode ownership is easy to desynchronize when new display modes are introduced.
- Pager 审计债务：Exact human output strings are concentrated in one large test module, so copy changes require broad fixture edits and provide limited structured coverage of individual formatter fields.
- Pager 审计债务：Most fixtures construct full probe snapshots manually, coupling formatter tests to low-level probe DTO shape and making it harder to isolate presentation behavior from snapshot assembly.
- Pager 审计债务：Runtime merge tests call view and runtime collection before format_doctor, while the legacy clipboard test mutates facts after view; the suite therefore spans multiple ownership layers and can obscure which layer owns a regression.
- Pager 审计债务：Several tests assert remediation prose, config paths, and command strings together with semantic IDs; stable IDs and user-facing copy have different change lifecycles but are locked in the same snapshots.
- Pager 审计债务：The formatter contract relies on exact section ordering and literal labels while runtime findings are assembled from separate modules, leaving ordering policy distributed between collection and formatting paths.
- Pager 审计债务：FileSearchState combines context parsing orchestration, background matcher lifecycle, generation fencing, dropdown interaction, and text replacement policy, coupling filesystem search timing to prompt editing semantics.
- Pager 审计债务：The daemon is deliberately built on the first @ use on the UI thread, so thread-spawn cost and EAGAIN risk are shifted to an input edge rather than isolated behind a startup/runtime service.
- Pager 审计债务：min_generation is a local floor incremented for every query while the matcher maintains its own generation; correctness depends on undocumented alignment between these counters and can be fragile across daemon recreation.
- Pager 审计债务：clear_context does not reset selected, hovered, scroll_offset, or min_generation, leaving latent interaction state that is masked by visibility and later start_query resets.
- Pager 审计债务：try_replace encodes prompt terminator handling, directory commit/dismiss semantics, cursor byte arithmetic, and path normalization in one pure method; changes to prompt element/undo contracts must be coordinated manually.
- Pager 审计债务：The hard cap of 1000 is embedded in the view state and passed to the workspace matcher, so result-volume policy is not represented as configuration or a named UI contract.
- Pager 审计债务：The event combines authorization provenance, access payloads, classifier explanations, latency, replay-safe text, and terminal rendering in one block module, coupling audit data schema to presentation text.
- Pager 审计债务：SubagentPermissionBlock stores epoch metadata but its mutation API permits unrestricted push/extend, leaving primary-turn/epoch integrity entirely dependent on outer ScrollbackState orchestration.
- Pager 审计债务：Full access_detail and classifier_reason are rendered without bounds or redaction while compact/searchable paths separately omit some fields; callers must choose the correct projection to avoid exposing verbose/sensitive data.
- Pager 审计债务：Outcome aggregation duplicates verb/cardinality/styling decisions from other scrollback grouping components and identifies children by raw session strings rather than a shared child identity type.
- Pager 审计债务：The compact line and aggregated header both claim selection range Some(0), while member-level click/detail mapping is implemented elsewhere; this distributed geometry contract can drift when rows or prefixes change.
- Pager 审计债务：BgTaskBlock combines lifecycle state, compact user-facing wording, terminal styling, preamble wrapping, and central-store identity, coupling task execution semantics to scrollback presentation.
- Pager 审计债务：Task termination policy is encoded as a small string allowlist for killed signals; new signal spellings or platform-specific outcomes can silently render as failed with misleading detail.
- Pager 审计债务：Description normalization differs by surface: compact rows replace newlines with spaces while preambles preserve lines and collapse blanks, leaving cross-surface text consistency to duplicated local rules.
- Pager 审计债务：Preamble rendering reuses permission_view::render_bash_command_display_lines, creating a cross-block dependency for shell wrapping and making viewer layout behavior sensitive to permission-panel changes.
- Pager 审计债务：The `has_bullet` contract always returns true while bullet returns None for a finished Started block after running; the outer renderer must supply the intended default gray bullet without a single explicit state representation.
- Pager 审计债务：Mermaid detection and image-reference extraction are duplicated lifecycle caches that remain stale for streaming chunks until finish; callers must respect that finalization boundary.
- Pager 审计债务：output() and diagram_affordances() independently rebuild rendered_output for diagram messages, trading deterministic consistency for repeated work on each frame.
- Pager 审计债务：The block couples presentation to process-global appearance and terminal-image flags, making pure rendering isolation and test parallelism harder.
- Pager 审计债务：The Mermaid affordance row carries source text but no render state or path; action/render lifecycle remains split across block, renderer, worker, and input layers.
- Pager 审计债务：The collector API passes many independently computed facts through large snapshot structs and a high-arity collect_standalone_from constructor, increasing assembly drift risk.
- Pager 审计债务：Startup and doctor collection duplicate display-server/native-tool and host probing decisions, while standalone collection uses a separate path with different availability semantics.
- Pager 审计债务：Targeted fix probing is keyed by diagnostic IDs in this low-level collector, coupling probe selection to diagnostics policy constants rather than a typed probe plan.
- Pager 审计债务：RuntimeEvidence and TmuxProbeResult represent availability at different layers; downstream conversion must preserve distinctions manually, leaving room for inconsistent unavailable/error presentation.
- Pager 审计债务：Each paint reconstructs dense output for every candidate entry and then flattens lines, with no per-width or per-generation cache in this module.
- Pager 审计债务：The painter duplicates policy boundaries from full ScrollbackPane: no sticky headers, padding, gaps, or accent chrome are intentionally hard-coded here, so visual behavior can drift when shared layout rules change.
- Pager 审计债务：Current-turn selection is inferred by reverse scanning for the last user prompt rather than consuming an authoritative turn range, leaving prompt identity and turn projection coupled to block classification.
- Pager 审计债务：Global thinking visibility and process-global Theme::current are read during pure-looking projection, which complicates deterministic rendering and parallel tests.
- Pager 审计债务：The parser is a permissive ad-hoc text protocol coupled to exact Markdown marker strings, with malformed input silently converted to empty/zero fields.
- Pager 审计债务：Path shortening recomputes a display string from config.grow_home on every result and uses textual prefix matching instead of a path-aware component comparison.
- Pager 审计债务：Structured result parsing and UI presentation are coupled through MemoryResult fields while searchable_text and tool dispatch live elsewhere, spreading the memory-search contract across modules.
- Pager 审计债务：Timing state is duplicated across each concrete tool block and relies on caller-set started_at; this file provides completion timing but cannot enforce start/finish lifecycle ordering.
- Pager 审计债务：The fixture repeats manual transcript construction and fixed layout preparation across tests, so changes to turn/materialization setup can invalidate many cases at once.
- Pager 审计债务：Jump picker lifecycle is coordinated by root dispatch, AgentView input, rewind, inline edit, reload, and ScrollbackState restore paths; this test file exposes the cross-module coupling but no single lifecycle owner.
- Pager 审计债务：The tests use EntryId stability at the selection boundary while picker entries also retain turn previews and restore state, leaving two identity/position models that require careful synchronization.
- Pager 审计债务：Overlay refusal and hidden-picker cleanup are tested through a narrow cancel-turn stand-in; adding a new input owner requires updating multiple precedence and teardown paths.
- Pager 审计债务：The dispatch tests combine block-viewer media routing and plugin-modal delivery in one transcript module, so coverage ownership is split across unrelated UI surfaces.
- Pager 审计债务：Native image opening is fire-and-forget from the dispatcher and has no observable effect contract in these tests, leaving launch failures outside the in-memory verification boundary.
- Pager 审计债务：The plugin collapse invariant is represented by a modal boolean/set seed flag and is verified only through repeated delivery; a dedicated modal-level contract could reduce coupling to dispatcher fixtures.
- Pager 审计债务：Passive coordination state is represented by OtherToolCallBlock and inferred through a coordination field, coupling sideband identity and ordinary tool rendering instead of using a dedicated row type.
- Pager 审计债务：merge_coordination_rows_from_tail silently ignores coordination rows with no matching original identity, so a caller must separately define the policy for tail-only inquiry events.
- Pager 审计债务：The module relies on a production expect for coordination identity; malformed upstream notices therefore remain a panic boundary rather than an explicit error path.
- Pager 审计债务：Timing and replay semantics are encoded through the generic running/finish fields, which leaves passive-row lifecycle rules distributed across ScrollbackState and OtherToolCallBlock.
- Pager 审计债务：Hook rendering owns outcome counting, status glyphs, detail truncation, and output truncation in one helper module, coupling semantic presentation policy to ratatui styling.
- Pager 审计债务：The repeated 120-character and three-line limits are hard-coded in both Blocked/Failed detail paths and output handling rather than represented by a shared rendering budget.
- Pager 审计债务：The module has no local tests despite being a central presentation boundary; current OpenSpec evidence is indirect through entry/session-event composition requirements.
- Pager 审计债务：HookPhase is a data enum in this file but rendering receives separate vectors and does not use the phase value directly, leaving phase validation to callers.
- Pager 审计债务：Permission response delivery reports a closed requester only through a UI toast and has no typed result for callers or telemetry.
- Pager 审计债务：Selection dispatch combines queue mutation, sticky cursor persistence, MCP/bash metadata projection, and AlwaysApprove mode transition, making the per-request response path carry several policy concerns.
- Pager 审计债务：Root/child ownership is inferred from session-id strings in a shared queue; the queue does not expose a dedicated ownership abstraction to this module.
- Pager 审计债务：The no-prefix MCP server fallback and bash highlight slicing are defensive assumptions embedded in dispatch rather than validated request types.
- Pager 审计债务：This test module combines extension modal fetch admission, new-session questions, session close cleanup, project-picker creation, and marketplace request coalescing, making ownership boundaries broad for a single fixture module.
- Pager 审计债务：Extension fetch coalescing is represented by mutable modal flags and effect counting; there is no typed request generation or observable correlation in these tests.
- Pager 审计债务：Close cleanup mixes view switching, session unregister effect construction, map removal, fork-reference repair, and memory release in one dispatcher path.
- Pager 审计债务：The tests depend on global or shared test-support state for memory-release counting and filesystem/git fixture assumptions, which can make isolation and failure diagnosis harder.
- Pager 审计债务：ListLayoutCache::virtual_y and item_height silently return zero/one for out-of-range indices, which can mask stale visible-index callers instead of exposing an explicit invalid-state result.
- Pager 审计债务：The FixedHeight/Variable mode contract is enforced by a runtime panic in extend_heights rather than a type-level API that makes invalid incremental updates impossible.
- Pager 审计债务：Prefix sums use usize while source heights are u16 without checked accumulation, leaving extreme aggregate-height behavior implicit.
- Pager 审计债务：The cached width is metadata only in this module; correctness depends on ListPaneState to invalidate the cache whenever width or wrapping inputs change.
- Pager 审计债务：The dialog shell is painted cell by cell with manual border glyphs and repeated theme/style literals instead of a reusable popup chrome primitive.
- Pager 审计债务：dialog_width_for casts the aggregate Unicode display-width calculation to u16 before clamping, leaving extreme-width behavior implicit.
- Pager 审计债务：Rendering couples directly to NewWorktreeDialogState::viewport and its byte range/display-column representation, so the view contract depends on editor internals without a local invariant check.
- Pager 审计债务：The Unicode cursor test asserts only that some highlighted cell exists, allowing a misplaced cursor to pass while still meeting the test predicate.
- Pager 审计债务：Timing lifecycle is manually duplicated across set_error, finish, and elapsed_ms with public Option fields, allowing callers to mutate started_at/elapsed_ms without a type-level state transition.
- Pager 审计债务：Elapsed duration is cast from as_millis() to i64 without checked conversion, leaving extreme-duration behavior implicit.
- Pager 审计债务：Header width budgeting uses byte lengths for the ASCII prefix and suffix and delegates path shortening separately, so display-cell width invariants are not expressed at this boundary.
- Pager 审计债务：The production module has only two narrow header tests; core output modes, selection metadata, timing, style, and fold contracts can regress without local evidence.
- Pager 审计债务：The modal and welcome paths are manually constructed twice at the call sites; the PickerSurface abstraction reduces field plumbing but still requires duplicated current-repository and grouped-mode setup.
- Pager 审计债务：session_picker_list_seq is validated here but advanced elsewhere, leaving request invalidation ownership split across dispatch handlers and making the freshness contract non-local.
- Pager 审计债务：The relaxed notification latch is a PathBuf equality cache coupled to app.cwd while selection anchoring uses the agent session cwd for modal surfaces, so notification and picker repository scopes can diverge during cwd transitions.
- Pager 审计债务：Loaded and failed handlers always return empty effect vectors and mutate the view directly, so data reconciliation, notice production, and side effects are coupled in one dispatcher boundary.
- Pager 审计债务：No inline tests exist in this module despite multiple race and surface-routing branches; the contract depends on external dispatch tests and integration paths.
- Pager 审计债务：The test depends on long wall-clock waits and sentinel polling, making failure diagnosis and runtime stability dependent on PTY scheduling and model-fixture timing.
- Pager 审计债务：Visual distinction is asserted through one hard-coded Unicode rail and DIM/ITALIC flags, coupling the test to presentation details instead of a semantic style contract.
- Pager 审计债务：The fixture writes configuration files directly and selects NO_COLOR through process environment, so configuration precedence and color initialization are exercised implicitly rather than through an explicit test seam.
- Pager 审计债务：Both end-to-end tests are ignored, leaving the minimal thinking visual and collapse/reopen behavior outside routine automated coverage.
- Pager 审计债务：The collapse test uses raw byte 0x05 for Ctrl+E and substring-based full_text checks; neither expresses the key binding or fold target as a typed contract.
- Pager 审计债务：MAX_DROPDOWN_ROWS is exported but render_dropdown uses the supplied Rect height, while AgentView::draw independently applies MAX_DROPDOWN_ROWS and dropdown_height is not used by that caller; the row-cap and height contract is duplicated.
- Pager 审计债务：dropdown_height includes a separator row although render_dropdown explicitly renders only result rows, so its caller contract depends on undocumented panel arithmetic outside this module.
- Pager 审计债务：Fuzzy matching indices are consumed against the normalized display path without a local invariant tying them to that transformed string; a path normalization change could silently shift accent positions.
- Pager 审计债务：The manual cell-by-cell renderer mixes byte indexing, Unicode scalar widths, u16 coordinate arithmetic, and wide-character continuation handling without dedicated property tests in this file.
- Pager 审计债务：The public height helper and the row renderer have no local tests, leaving visibility, truncation, scrollbar reservation, and styling regressions dependent on higher-level coverage.
- Pager 审计债务：ListOverlay::height, visible_rows, scroll_offset, row_at, and render encode the shared geometry in one place, but the three-row reservation and the extra padding row remain implicit numeric constants rather than named layout roles.
- Pager 审计债务：scroll_offset assumes selected is a valid list index and does not clamp the computed offset to len - visible_rows; invalid caller state can produce an empty render window even though row_at remains safe.
- Pager 审计债务：The renderer uses manual Rect arithmetic and direct cell painting for the accent bar and row background while row content uses Line; this split has no render-level regression tests.
- Pager 审计债务：Theme::current is read inside the renderer rather than passed as an explicit dependency, making deterministic style testing and cross-frame theme ownership less direct.
- Pager 审计债务：The shared abstraction covers geometry but still leaves caller closures to independently truncate and style content, so visual consistency across jump and rewind remains partly distributed.
- Pager 审计债务：The regression is ignored, leaving the user-visible pager suspend/restore path outside routine automated coverage.
- Pager 审计债务：The test depends on a real external `less` binary and fixed wall-clock sleeps, making reproducibility and CI diagnostics sensitive to host installation and scheduler load.
- Pager 审计债务：The 40 ms frame delay is passed through an environment variable and the test infers the race from final text; there is no explicit writer-drain or frame-order observation seam.
- Pager 审计债务：Visual correctness is represented by hard-coded substrings, occurrence counts, and a column-zero bracket heuristic rather than a structured live-region or terminal-frame contract.
- Pager 审计债务：The scenario combines inference setup, minimal startup, paced editing, slash command dispatch, external pager lifecycle, repaint settling, and shutdown in one large end-to-end test, so failures have a broad diagnosis surface.
- Pager 审计债务：The model mixes raw evidence, compatibility projections, user findings, and remediation references in one report graph; consumers must understand which fields are authoritative for a given surface.
- Pager 审计债务：DiagnosticReport::issue_count contains a special clipboard fallback keyed to two concrete IDs, coupling aggregate counting to finding identity constants and making future delivery dispositions easy to double-count or omit.
- Pager 审计债务：Probe names are unrestricted static strings and live-TUI classification is a string match, so adding or renaming a probe has no compiler-checked relationship to probe collection.
- Pager 审计债务：ClipboardFacts retains the legacy optional fix string beside named findings, leaving two remediation representations that can diverge unless view consumers keep them synchronized.
- Pager 审计债务：The large aggregate structs have no local constructors or invariants; callers assemble public fields directly and can create contradictory combinations that remain Eq/PartialEq-valid.
- Pager 审计债务：The sole regression test is ignored and requires serialized explicit invocation, leaving multi-client leader behavior outside routine automated coverage.
- Pager 审计债务：Long 240-second and 120-second waits plus repeated wheel bursts make the test expensive and sensitive to host scheduling, PTY throughput, and suite contention.
- Pager 审计债务：Exactly-once and liveness contracts are encoded as hard-coded text counts and a `panicked` substring check instead of structured session/replay/frame evidence.
- Pager 审计债务：The test combines leader startup, replay, bidirectional streaming, viewport expansion, scrolling recovery, and leader-survival lifecycle in one scenario, so failures have a broad diagnosis surface.
- Pager 审计债务：The wheel/ESC fallback depends on terminal event routing and focus behavior; its local retry helper can mask whether scrolling, focus changes, or event cadence actually caused the recovery.
- Pager 审计债务：The only N-client regression test is ignored and requires serialized explicit invocation, leaving scaled leader fan-out outside routine automated coverage.
- Pager 审计债务：Long 240-second and 120-second waits and per-viewer settle/update pumps make runtime sensitive to host scheduling, PTY throughput, and suite contention.
- Pager 审计债务：Fan-out correctness is encoded as hard-coded sentinel occurrence counts and a `panicked` substring check rather than structured session/replay/frame evidence.
- Pager 审计债务：VIEWERS, Tokio worker-thread count, and post-driver pump duration are coupled by comments and manual tuning rather than a typed scaling budget or parameterized stress harness.
- Pager 审计债务：The test combines shared setup, replay, live broadcast, delayed-frame duplicate detection, process lifecycle, and cleanup in one scenario, broadening the failure diagnosis surface.
- Pager 审计债务：The regression test is ignored, leaving minimal parked-plan scrollback behavior outside routine automated coverage.
- Pager 审计债务：Plan correctness relies on hard-coded generated sentinel substrings and occurrence counts rather than a structured scrollback block identity or native-history assertion.
- Pager 审计债务：The test registers AgentTurnExpectation handles but drops them without awaiting satisfaction, so the UI path and scripted backend path are only loosely synchronized.
- Pager 审计债务：Fixed 100 ms update loops, a 60-second parking wait, a 40-second first-turn wait, and a 20x100 geometry make the scenario sensitive to scheduler and PTY timing.
- Pager 审计债务：One test combines filesystem seeding, initial session creation, two tool calls, revision input, approval, scrollback completeness, duplicate detection, and shutdown, giving failures a broad diagnosis surface.
- Pager 审计债务：The regression test is ignored, leaving minimal /new session transition behavior outside routine automated coverage.
- Pager 审计债务：The test identifies a new session with a broad `Grow` substring count rather than a typed welcome-card marker or session identity, so unrelated banner text could satisfy the condition.
- Pager 审计债务：History preservation and frontier reset are inferred from text reachability and a second banner; the test has no structured session snapshot or committed-frontier assertion.
- Pager 审计债务：Fixed tall-response generation, polling loops, and long waits make the scenario sensitive to terminal scheduling and content/render timing.
- Pager 审计债务：One end-to-end test combines tall markdown rendering, native scrollback entry, slash command pacing, session creation, history preservation, fresh streaming, panic detection, and shutdown, broadening the failure diagnosis surface.
- Pager 审计债务：The transcript pager regression is ignored, leaving the minimal /transcript integration path outside routine automated coverage.
- Pager 审计债务：Pager execution is inferred from a broad response-sentinel count instead of a structured child-process, temp-file, or transcript artifact signal.
- Pager 审计债务：PAGER=cat avoids the interactive pager path that motivated the related restore tests, so the primary full-screen/alternate-screen lifecycle remains unexercised here.
- Pager 审计债务：Fixed polling deadlines and 100 ms updates make the test timing-sensitive while providing no explicit frame-drain or child-exit synchronization.
- Pager 审计债务：The test combines mock setup, session startup, prompt submission, slash input pacing, transcript export, restore, panic scanning, and shutdown in one scenario, broadening failure diagnosis.
- Pager 审计债务：The resize regression is ignored, leaving minimal committed-history resize behavior outside routine automated coverage.
- Pager 审计债务：History preservation is inferred from one sentinel substring without a structured block identity, line-count, or duplicate assertion, so wiping and reprinting can be hard to distinguish.
- Pager 审计债务：The source comments specify forbidden production mechanisms, but the test has no instrumentation or static guard proving those paths are absent.
- Pager 审计债务：Fixed 40-second/30-second waits and an 800 ms settle interval make the scenario timing-sensitive while providing no explicit frame or resize completion barrier.
- Pager 审计债务：The test combines tall markdown rendering, native scrollback entry, a two-dimensional resize, screen health, post-resize streaming, and shutdown in one scenario, broadening failure diagnosis.
- Pager 审计债务：The queue regression is ignored, leaving minimal queue UX and promotion outside routine automated coverage.
- Pager 审计债务：Queue correctness is inferred from hard-coded status and snapshot substrings rather than structured queue state or turn identity assertions.
- Pager 审计债务：The test registers AgentTurnExpectation values but never awaits their satisfaction, coupling backend fixture setup to UI substring waits without a terminal barrier.
- Pager 审计债务：Fixed stream chunk delay and wall-clock waits make the race window and promotion timing sensitive to scheduler/PTY load.
- Pager 审计债务：One scenario combines slow inference, prompt queue admission, status rendering, slash inspection, snapshot commit, promotion, panic detection, and shutdown, broadening failure diagnosis.
- Pager 审计债务：The regression test is ignored, leaving minimal read-header and expand behavior outside routine suite coverage.
- Pager 审计债务：The contract is inferred from hard-coded text substrings and one control byte rather than a structured read-block identity and exact collapsed/expanded state assertion.
- Pager 审计债务：The test creates a must_use AgentTurnExpectation but does not await or assert it, allowing the UI text path to be the only synchronization barrier for tool-call correctness.
- Pager 审计债务：The broad scenario couples filesystem fixture creation, inference scripting, permission/trust bootstrap, read-header projection, keyboard expansion, panic detection, and shutdown, so failures have coarse diagnosis boundaries.
- Pager 审计债务：The regression is #[ignore], so minimal Escape cancellation is outside default automated coverage.
- Pager 审计债务：Timing and literal text assertions leave the cancellation contract indirectly specified and diagnosis coarse.
- Pager 审计债务：The regression is #[ignore], so minimal Escape cancellation is outside default automated coverage.
- Pager 审计债务：Timing and literal text assertions leave the cancellation contract indirectly specified and diagnosis coarse.
- Pager 审计债务：端到端设置 modal 流程仍依赖 UI 文案 sentinel，且打开、关闭、健康检查和清理集中在一个 PTY 场景中；可补充结构化 modal/focus probe 和更细粒度测试。
- Pager 审计债务：The regression is #[ignore], so minimal idle Ctrl+C quit confirmation is outside default automated coverage.
- Pager 审计债务：Literal text matching and acceptance of PendingStatus leave the command-state and clean-exit contracts indirect.
- Pager 审计债务：25个模块名手工维护，缺少目录与声明同步校验；集成拓扑依赖注释和 shared helper 约定。
- Markdown 审计债务：README 与 target 的 syntect 覆盖矩阵存在漂移；fuzz target 是 crash-oriented，缺少输出差分与结构化属性检查。
- **Bracketed paste 与后续按键归属**：preserve-paste-across-input-batches 核对发现 `coalesce_rapid_keys` 在同一 drain 同时出现 Event::Paste 和普通按键时调用 `merge_paste_fragments`，后者会合并 Enter/字符并丢弃非文本按键。现有测试把它视为 Windows 终端碎片恢复，但完整 bracketed paste 后的真实输入缺少可靠边界证据。需独立复核 Crossterm 的平台事件保证，覆盖粘贴后立即 Enter、方向键、Ctrl+C 和连续两次独立粘贴；不在本次未括号粘贴的 5,000 事件截断修复中混入平台协议重写。

## 审计债务：跨 resume 的会话用量账本

2026-09-09 核对 `/usage`：`extensions/usage.rs` 读取进程内 UsageLedger，当前窗口为启动或最近 resume 之后。历史 Request 完成记录保留部分用量，但失败/重试调用、Sideband、子会话和旧 wire model 的 provider 归属需要统一核对，不能简单叠加展示日志或猜测回填。后续单独立项完善持久化归属与恢复；本次仅实现 `/usage` 的 provider/model 分项与缓存命中率。

- **辅助采样的统计口径与恢复所有者**：本次 `unify-sampling-attempt-recovery` 统一了主/子 agent 的模型步骤账本与恢复预算。Sideband 仍使用其既有独立 attempt、证据、Goal 结算及预算，不混入 main-loop usage；若产品需要统一全部辅助消费展示，应独立核对 SidebandUsage、session totals、parent fold 的统计口径后立项，不能简单重复累计。
- **可撤销预览的资源上限**：leader 的候选暂存和持久化投影暂存只保留当前未接纳候选，不保留已接纳历史；单个超长候选的内存上限仍需与现有 stream/body/evidence 限制统一审计。应独立设计有界溢出停止或落盘策略，不能为了限流提前把未接纳内容外发/写入回放。

- **断线中的活跃候选前缀补传**：`unify-sampling-attempt-recovery` 隔离并清理未接纳预览，`updates.jsonl` 只包含已接纳内容。现有 root load 与子视图按需回放不能据此保证完整补传断线期间仍在生成的候选前缀。若需要不中断地恢复完整实时展示，应独立核对 leader 的 load cutoff、当前候选快照和子会话历史水位，在同一 session/attempt 归属下补传并去重；不得把临时预览重新写成接纳历史，也不能把该 UI 能力等同于重新执行 provider。

- **会话读取与 Summary 投影修复的边界**：`JsonlStorageAdapter::load_session` / `load_light_data` 在读取后仍调用 `reconcile_session_title_projection` 和 `reconcile_model_projection`；投影滞后时会尝试持久化修复及写者准入。因此，即使 Windows 观察句柄已支持与运行中写者共存，也不能据此宣称所有滞后 Summary 都能纯只读加载。本次发布修复已一致投影下的共享冲突；后续独立定义只读投影与显式修复边界，验证运行中写者、滞后 title/model Summary、无写权限观察者及写者接管，不改变现有错误场景来隐藏差异。
