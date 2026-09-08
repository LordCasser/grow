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

## 持续审计待拆分事项

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
