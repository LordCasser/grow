# 最终覆盖审查：只读盘点阶段

**这不是最终通过意见。** 本阶段仅对原清单逐条核对处置与证据映射，不运行Cargo、不归档、不改backlog，也未重审最新全部实现或尚未构建的release。审查时tasks 1–8完成，task9仍含C2/C4/T1，task10–12尚未完成。后续独立终审需以最终源码、相关完整验证和release实物为准。

阶段结论：在下述范围内，**未发现除已明确保留的C2、C4、T1之外，另有未映射的非暂缓旧事项；未发现把“待专项验证/待故障注入/待定位”偷换成延期**。原清单已完成的历史债务有归档证据，不要求重新实施；原本仅分析的方案则映射到本轮实际归档，不能仅靠分析change的tasks全勾当成实现完成。

## 核对方法

- 逐段读原`backlog-before.md`全部125行，再对照当前`algorithm-inventory.md`、tasks及11月2/3归档证据。
- 原快照内90个相对证据链接全部按原`openspec/backlog.md`基准解析，目标均存在。涉及79个不同archive目录；其tasks不存在未勾项。这只证明文档引用和归档闭环线索，**不等同本阶段运行了测试或全部分析已实现**。
- 原“本轮闭合的缺陷与测试债务”表21项、原“历史完成记录”表22项逐项保留在下方映射；没有把它们因时间较早漏掉。
- 新假设、最新上游功能及未列入原清单的架构扩展不在本盘点范围。当前backlog尚有旧措辞待task10精简；本阶段不拿其未清理状态判定已归档工作没有完成。

## 原新增/待处理事项映射

| 原清单位置/事项 | 当前处置与证据 |
| --- | --- |
| L3 Dashboard UX | 已实施并独立审查：[UX验证](../2026-11-03-align-dashboard-session-ux/verification.md)、[review](../2026-11-03-align-dashboard-session-ux/review.md)；删除modal Resize/minimal退出两项已修正。 |
| L5 项目规则更新冷恢复 | [持久ProjectInstructions](../2026-11-03-restore-persisted-project-instructions/verification.md)，区别fresh/Seeded/Existing，模型与classifier同源。此前只做用户session本机修复，不能替代此实现。 |
| L12 恢复准入分析 | 已拆成恢复S1规则、恢复S3失败retry、出生MCP和执行依赖准入；见下面18组验收定位表。没有裸删hash后不检查实际依赖。 |
| L14 fork删除资格A1/A2 | [独立fork删除](../2026-11-02-allow-independent-fork-session-deletion/verification.md)、[删除不受维护预算](../2026-11-02-delete-session-without-maintenance-observation-budget/verification.md)。 |
| L15 删除页面收敛A3/A4 | [runtime/views reconciliation](../2026-11-02-reconcile-session-deletion-with-runtime-and-views/verification.md)，含Stay、原binding和迟到回执。 |
| L16 Dashboard停止A5 | [background stop](../2026-11-02-fix-dashboard-background-stop/verification.md)；本轮UX还分离Stop/Close/Delete。 |
| L17 fork失败A8及A6/A7 | [空占位页](../2026-11-02-retire-empty-failed-fork-placeholders/verification.md)、[来源先关闭](../2026-11-02-preserve-fork-failure-identity-after-source-close/verification.md)、[runtime close](../2026-11-02-close-dashboard-sessions-through-runtime/verification.md)、[退出scope](../2026-11-02-scope-exit-suspension-to-runtime-owners/verification.md)。 |
| L18 挂起历史显式清理G1/R1 | [显式discard](../2026-11-02-allow-explicit-discard-of-independent-suspended-history/verification.md)、同上退出scope。 |
| L20 resume_from继承展示 | [新child继承transcript](../2026-11-03-preserve-task-resume-inherited-transcript/verification.md)；与原identity自动续跑的[deferred history衔接](../2026-11-02-fix-subagent-resume-history/verification.md)区分。 |
| L22 workspace路径别名 | [执行依赖准入](../2026-11-03-admit-session-restores-by-execution-dependencies/verification.md)，含同实体alias与异实体拒绝。 |
| L23 出生MCP成员（原待专项验证） | [出生grant持久化](../2026-11-03-persist-child-birth-mcp-membership/verification.md)、后续执行准入真实cold fixture；未留在延期。 |
| L26 ACP Plan malformed响应 | [Plan验证](../2026-11-03-bind-restored-plan-approval-to-one-snapshot/verification.md)；同gateway actor显式第二次RestorePlanApproval恢复，不新增自动重试。记录明确不是完整外部client session/load RPC测试，不夸大Bundled Pager入口。 |
| L27 Plan artifact二读竞态（原待故障注入） | 同上Plan验证；真实barrier red/green、exact snapshot/ACK失败、stale bit发布顺序。未以正常UI未复现为由延期。 |
| L33 Sampler日志捕获（原放在暂缓段，但只是待定位） | [调查验证](../2026-11-03-diagnose-sampler-auth-log-test-observation/verification.md)；当前默认并行/32线程未复现，保留5项正向与wire/no-secret断言，仅加失败capture。**关闭调查候选不等于修复flake；历史根因仍未知。** |
| L42 Subagent委派四项 | [2026-10-31原修复](../2026-10-31-fix-subagent-delegation-authority/verification.md)保留；其中独立列出的路径别名/出生MCP另由本轮上述两项处理，没有混同live与cold证明。 |

恢复分析原V01–V18是后续终审的证据定位，不因分散归档而消失：

| 原验收组 | 对应归档 |
| --- | --- |
| V01–V04 持久规则/无规则/typed分支/父子与fork来源 | restore-persisted-project-instructions；preserve-task-resume-inherited-transcript；persist-child-birth-mcp-membership；相关旧配置/权限规范。 |
| V05–V10 展示漂移/路径/当前权限/路线/额度/Hook和Workflow | admit-session-restores-by-execution-dependencies及其design/verification；出生MCP归档。其design明确runtime_overrides、已消费预算、sampling retry/deadline与当前maxTurns派发检查，不把aggregate hash相等当这些性质的证明。 |
| V11–V14 失败阶段、准备/释放、并发、writer与显式Retry | reload-unprepared-failed-session-restores；admit-session-restores-by-execution-dependencies；原runtime悬挂等待与真实并发cold-resume归档。 |
| V15 root关闭/两个leader退出owner | close-dashboard-sessions-through-runtime；scope-exit-suspension-to-runtime-owners。 |
| V16–V17 fork/长历史/责任保护/显式discard/删除失败与Stay/fork失败 | 11月2的A1–A8/G1相关归档及本轮UX真实PTY。 |
| V18 损坏因果/foreign owner/未知副作用 | 执行依赖准入原负例、原canonical/Runtime验证、出生事实缺失/损坏及Hook Unknown拒绝记录。 |

这张表不声称审查者在本阶段重跑了各V用例；最终实现审查还应核对对应源码/测试保留，尤其不能用简单hash格式测试代替恢复语义。

## 原算法候选与后续profile映射

| 原候选 | 处置证据/剩余状态 |
| --- | --- |
| R1 Timeline admission | [原归档](../2026-10-21-optimize-timeline-admission-validation/verification.md)。 |
| R2 response append plan | [原归档](../2026-10-21-reuse-response-projection-append-plan/verification.md)。 |
| R3 Value树/typed replay往返 | inventory已核对两项早已消除；必要version-first严格解析保留。未将未单独归因的version probe说成零成本。 |
| R5 writer首次prefix | [原归档](../2026-10-21-optimize-timeline-writer-prefix/verification.md)。 |
| R6 同次canonical复用 | [原归档](../2026-10-21-reuse-resume-canonical-projections/verification.md)。 |
| P0/P1/P2 presentation profile跟进 | [P0](../2026-10-22-profile-resume-presentation-attribution/verification.md)、[P1](../2026-10-22-defer-historical-markdown-presentation/verification.md)、[P2](../2026-10-22-reuse-validated-descendant-recovery/verification.md)。 |
| U1 = R4 布局 | [原归档](../2026-10-21-optimize-replay-layout-maintenance/verification.md)，不能重复计为两个待办。 |
| S1 exact commit | [原归档](../2026-10-21-index-exact-update-commits/verification.md)。 |
| S2 search | [本轮validated prefix](../2026-11-03-reuse-validated-search-ledger-prefix/verification.md)；保留完整prefix hash、统一scanner、失败隔离、cold/fallback及保留内存限制。 |
| S3 listing | [本轮typed key/延后派生](../2026-11-03-defer-session-listing-materialization/verification.md)。 |
| U2 picker | [成本门槛](cost-gates.md)，默认30与额外100/128/512/2048按入口区分，证据关闭而非延期。 |
| C1 images | [本轮复用选择offset](../2026-11-03-reuse-image-budget-choice-offsets/verification.md)。 |
| C2 malformed/dangling/duplicate repair | **尚未闭合**，实现/测试进行中；task9未勾。 |
| C3 rewind anchor | [本轮accepted派生](../2026-11-03-cache-last-rewind-anchor/verification.md)。 |
| C4 healthy integrity clone | **尚未闭合**，task9保留，不能因低频或待测改为延期。 |
| T1 watch有界选择 | **尚未闭合**，task9保留，不能仅因默认budget较大关闭。 |
| T2 多hunk行表 | [本轮复用](../2026-11-03-reuse-file-line-tables-for-hunk-patches/verification.md)。 |
| T3 metadata | [本轮复用](../2026-11-03-avoid-workspace-entry-metadata-dup/verification.md)。 |
| T4 manifest | [成本门槛](cost-gates.md)，真实preset与合成kernel分开，证据关闭。 |
| U3 top8 | 同上成本门槛；排序已不在当前production roster路径，按调用事实关闭失效热点。 |

P2原文“就绪与历史分页分离未做”已专门回溯：[proposal](../2026-10-22-reuse-validated-descendant-recovery/proposal.md)明确“Preserve ... existing complete-load ready barrier. No ... pagination or early readiness”，design/verification相同。它是保留正式ready边界的范围说明，不是另一个未映射缺陷；本轮不擅自添加早放行产品行为，也不把它计成第七项延期。原“真实多节点负载才能支持进一步优化”同样是已闭合优化的外推限制，不代表有新的未实施既定方案。

## 明确保留的六项

| 原位置 | 保留事项与原条件 |
| --- | --- |
| L32 | 跨路由reasoning更强语义：只有真实产品目标提出更强continuation/provenance语义才启动。 |
| L34 | Remote Control/Agent Host：明确暂缓、未排期、不实施不设日期。 |
| L35 | Provider cache/native epoch：先有具体route/model/deployment、透传与真实负载证据。 |
| L36 | Recap与主请求cache：先有真实共享prefix及收益证据；独立key/tool-free Sideband不变。 |
| L37 | 多合法选区连续摘要：当前契约一次有界后超窗终止，新的继续压缩契约未启动。 |
| L38 | MCP多轮elicitation：既有proposal的上游SDK接点门槛未通过，tasks 2.0a/后续未勾；不按最新SDK猜测现已可用。 |

Sampler不在这六项中。其历史根因未知的限制仍须保留，清单文字不得改成“日志flake已修复”。

## 原已闭合缺陷与测试债务逐项映射（L48–L68）

下表沿原记录列明归档，不把旧修复重新当成本轮实施。当前inventory的对应已闭合记录与这些链接一致。

| 原已闭合事项 | 原归档证据 |
| --- | --- |
| 原生 float 向量分配失败后错误指针未初始化；RISC-V C 编译 warning | [共享错误输出初始化与故障注入回归](../2026-10-31-fix-native-vector-error-initialization/verification.md) |
| Plan 窄屏操作消失、Minimal 预览/批注与提示错配、普通批注去向不明 | [完整旅程修复与回归](../2026-10-31-audit-plan-user-journey/verification.md) |
| Runtime suspend 等待已 park 的 foreground，导致活动压缩排空后仍超时 | [压缩排空与前台收回](../2026-10-31-fix-suspension-compaction-drain/verification.md) |
| Agent 禁用错误归因、完整 ID 诊断及 toggle 运行时同步（审计 A1/A2/A3） | [fix-agent-authorization-and-toggle-reload](../2026-10-31-fix-agent-authorization-and-toggle-reload/verification.md)：typed 拒绝、主/子会话目录安全边界刷新、已有 spawn context 当前 gate 和切换候选失效回归；个人委派名单按用户明确要求更新 |
| ACP 初始化被同步 watcher 注册阻塞；注册间隙 config/policy 漏同步 | [unblock-acp-during-watcher-registration](../2026-10-29-unblock-acp-during-watcher-registration/verification.md) |
| LSP refresh mock 使用固定 sleep | [synchronize-lsp-refresh-test](../2026-10-29-synchronize-lsp-refresh-test/verification.md) |
| SideQuestion 接收者关闭后仍运行 | [cancel-abandoned-side-questions](../2026-10-29-cancel-abandoned-side-questions/verification.md) |
| 不可验证 child 完成被固化为成功 | [require-canonical-unavailable-child-result](../2026-10-29-require-canonical-unavailable-child-result/verification.md) |
| release 运行 debug-only panic 测试 | [clarify-committed-anchor-release-test](../2026-10-29-clarify-committed-anchor-release-test/verification.md) |
| Chrome duration 入口无计时 span | [emit-chrome-instrumentation-timers](../2026-10-29-emit-chrome-instrumentation-timers/verification.md) |
| sticky 关闭/compact 时导航几何不一致 | [fix-pager-sticky-navigation-geometry](../2026-10-29-fix-pager-sticky-navigation-geometry/verification.md) |
| 后代展示跨实体校验与无 ID 重入重复 | [fix-descendant-presentation-replay](../2026-10-29-fix-descendant-presentation-replay/verification.md) |
| tmux post-exit 清理预算缺少确定性区分 | [test-tmux-post-exit-cleanup-budget](../2026-10-29-test-tmux-post-exit-cleanup-budget/verification.md) |
| Git/worktree 构建身份监听无效路径（原两条合并） | [fix-build-git-identity-invalidation](../2026-10-29-fix-build-git-identity-invalidation/verification.md) |
| PTY 独立 daemon fixture 无 owner | [own-pty-leader-test-daemons](../2026-10-29-own-pty-leader-test-daemons/verification.md) |
| OCR 后代与并发 fixture 失败清理 | [own-local-ocr-process-tree](../2026-10-29-own-local-ocr-process-tree/verification.md) |
| Plan live handoff 同 Turn 已消费后误报 ACK 丢失 | [fix-plan-handoff-consumption-recheck](../2026-10-29-fix-plan-handoff-consumption-recheck/verification.md) |
| Goal Control 后首请求前 scope 未同步 | [fix-goal-control-context-scope](../2026-10-29-fix-goal-control-context-scope/verification.md) |
| CI 发现日志溢出测试的诊断位置/中间计数时序假设 | [同步测试](../2026-10-29-synchronize-unified-log-overflow-test/verification.md)，55项全量及200次精确执行通过 |
| 活动组真实PTY仍沿用旧分组边界与过早等待 | [契约与同步修复](../2026-10-29-align-activity-group-pty-contract/verification.md)，完整活动组交互与剩余16项发布PTY通过 |
| Runtime 回归未进入正式 Core 门禁 | [gate-backlog-runtime-regressions](../2026-10-29-gate-backlog-runtime-regressions/verification.md) |

原L70–L74另有五项证据关闭/组合验证：

- Native交错候选：[production边界证据关闭](../2026-10-29-audit-runtime-native-message-tool-batch-boundary/audit.md)。
- Markdown clone候选：[归因证据关闭](../2026-10-29-fix-pager-sticky-navigation-geometry/audit.md)。
- 压缩/计数/模型切换：[状态](../2026-10-29-fix-compaction-pressure-lifecycle/verification.md)、[组合故障](../2026-10-29-verify-compaction-publication-boundaries/verification.md)。
- 真实文件/进程中断恢复：[SIGKILL与重复冷载验证](../2026-10-29-verify-compaction-process-recovery/verification.md)，保留不等同断电的限制。
- 低值provider/输出预留：[四项](../2026-10-29-resolve-context-token-boundaries/verification.md)、[组合](../2026-10-29-verify-composed-context-transitions/verification.md)；未配置输出上限不猜后端默认值。

## 原历史完成记录逐项映射（L98–L119）

当前inventory用“历史遗留缺陷与测试债务”汇总引用10月29发布清理；为避免汇总掩盖旧条目，本阶段显式展开原22项。各归档文件均存在且tasks已完成；10月29[总验证](../2026-10-29-close-backlog-and-release-patch/verification.md)保留正式CI/发布证据。这里不重新验证历史tag、也不把旧跨平台CI当成本轮未构建release已通过。

| 原历史事项 | 原归档证据 |
| --- | --- |
| Plan resume PTY 把首段历史当输入就绪 | [真实 finalize 同步](../2026-10-22-synchronize-plan-resume-pty-readiness/verification.md)，最终 2.5.0 发布及同提交 push Core 均通过。 |
| Settings 全表键鼠、重置及保存失败重试/回滚覆盖不足 | [交互覆盖](../2026-10-20-complete-settings-interaction-regressions/verification.md) |
| show_tips / auto_update 保存回滚遗留旧默认值 | [默认值修复](../2026-10-20-align-preference-rollback-defaults/verification.md) |
| show_model_provider 的四项集成清单遗漏、完整 Settings integration 未进发布门禁 | [设置清单与门禁](../2026-10-20-complete-settings-preference-test-inventory/verification.md) |
| 恢复放行后、等待未重绑时再次挂起丢失逻辑等待 | [跨恢复周期等待](../2026-10-20-preserve-waits-across-runtime-release/verification.md) |
| 已处置的未派发任务仍阻塞 scope 切换 | [scope 义务校验](../2026-10-20-fix-undispatched-planning-scope/verification.md) |
| 树冻结失败重试换 owner、节点意图前 Suspended 等待丢失 | [树挂起准入边界](../2026-10-20-fix-tree-suspension-admission-boundaries/verification.md) |
| 自动 Goal 激活/重激活丢失 retained Plan 与提交计数 | [自动 Goal 快照](../2026-10-20-fix-automatic-goal-plan-snapshot/verification.md) |
| 退出挂起重复提交后台完成通知，以及节点挂起意图后的超时重试丢失人工等待 | [挂起重试证据修复](../2026-10-20-fix-suspension-retry-evidence/verification.md) |
| 持久 Bash 捕获前局部变量遮蔽同名导出值 | [捕获隔离修复](../2026-10-20-fix-bash-env-export-capture-shadow/verification.md) |
| session 移除/空闲卸载后动态配置监听未回收 | [resident 监听生命周期](../2026-10-20-reclaim-session-config-watches/verification.md) |
| 精确测试 runner 缺少阶段进度、子进程期限与有界收尾 | [runner 加固](../2026-10-20-harden-exact-cargo-runner/verification.md) |
| sqlite-vec metadata shadow 名称在公共析构遗漏释放 | [名称生命周期修复](../2026-10-20-release-vector-metadata-names/verification.md) |
| 第三方 C 边界/编译诊断、nix 微秒类型及 cross 安装旧 lock | [诊断修复（后续构建生效）](../2026-10-20-repair-third-party-build-diagnostics/verification.md) |
| OHOS 容器构建提交号 unknown 与 smoke 未校验身份 | [显式构建身份（后续构建生效）](../2026-10-20-bind-release-build-identity/verification.md) |
| Behavior 切换丢失 retained Plan 与提交计数 | [Plan 持久快照](../2026-10-19-preserve-plan-state-on-behavior-switch/verification.md) |
| 图片恢复 cursor 重连重复行及旧 spinner | [重连合并](../2026-10-19-merge-image-recovery-reconnect-rows/verification.md) |
| child 图片恢复通知、真实历史回放及异常结束漏处理 | [child 图片恢复](../2026-10-19-route-child-image-recovery/verification.md) |
| Windows storage CI 未覆盖构建配置路径 | [CI 触发范围](../2026-10-19-fix-windows-storage-build-input-triggers/verification.md) |
| 同步 compaction 清空当前 TODO | [保留计划投影](../2026-10-19-preserve-planning-through-compaction/verification.md) |
| 工具运行时失败继续采样、原始错误被后续 ACK/Step 错误遮蔽 | [工具失败停止边界](../2026-10-19-stop-on-tool-runtime-failure/verification.md) |
| child handoff 重复复制已有历史、重复校验已验证的冷 Timeline | [历史所有权优化](../2026-10-19-reduce-child-handoff-history-copy/verification.md) |

## 后续终审门槛

当前没有新增遗漏问题需要扩范围实施，但不能据此宣布umbrella完成：C2/C4/T1须分别以实现/收益证据或足够的证据关闭；随后核对最终相关回归、task10清单精简、strict/归档和本地release实物。最终审查还需检查本轮最新实现及其失败路径，尤其S2缓存/reader、C2/C4、T1等新增owner/算法边界；本次盘点不替代这些工作。
