# Backlog

截至 2026-10-29，本轮已清完登记的缺陷与测试债务。用户确认“清完缺陷与测试债务，暂缓项保留说明”。已解决项保留逐项证据；原始清单见 [清理前快照](changes/close-backlog-and-release-patch/backlog-before.md)。新的问题仍须独立建立 change，不以本次审计结论推断未来改动无风险。

## 保留的暂缓项

- **Remote Control / Agent Host 跨设备使用（暂缓、未排期）**：保留方向，不实施、不设日期；未来启动时另建 change，明确身份、权限、执行 owner、断线和恢复边界。不得据此引入 Dock、Memory 或远端控制框架。决策见 [上游主题评审](changes/archive/2026-10-18-record-upstream-review-2bdd1d6/review.md)。
- **Provider 缓存策略与 native epoch（待真实路由证据）**：[理论 review](changes/archive/2026-09-28-evaluate-provider-cache-affinity/results.md) 仅证明本地 key/breakpoint/retention 机制。先取得具体 provider/model/deployment、字段透传和真实负载证据，再做该 route 的最小优化，不预建 registry。
- **Recap 与主请求缓存（待真实共享前缀及收益证据）**：recap 保持独立 key、tool-free Sideband 契约；不为假设中的缓存复用添加主请求 tools。先测真实收益。
- **跨多个合法选区连续摘要（契约选择，未启动）**：当前一次有界摘要后仍超窗会保留已提交事实并终止回合；继续压缩更多选区需另立契约，不增加无界重试。
- **MCP 多轮 elicitation（现有独立提案）**：宿主 SDK 接点仍未通过既定门槛，保留 [提案与任务](changes/add-mcp-multi-round-elicitation/tasks.md)，不把未实现功能列为本轮缺陷。

## 本轮闭合的缺陷与测试债务

以下路径将随对应变更归档固定；正式发布和完整验证汇总见 [发布记录](changes/close-backlog-and-release-patch/verification.md)。

| 问题 | 修复与验证 |
| --- | --- |
| ACP 初始化被同步 watcher 注册阻塞；注册间隙 config/policy 漏同步 | [unblock-acp-during-watcher-registration](changes/archive/2026-10-29-unblock-acp-during-watcher-registration/verification.md) |
| LSP refresh mock 使用固定 sleep | [synchronize-lsp-refresh-test](changes/archive/2026-10-29-synchronize-lsp-refresh-test/verification.md) |
| SideQuestion 接收者关闭后仍运行 | [cancel-abandoned-side-questions](changes/archive/2026-10-29-cancel-abandoned-side-questions/verification.md) |
| 不可验证 child 完成被固化为成功 | [require-canonical-unavailable-child-result](changes/archive/2026-10-29-require-canonical-unavailable-child-result/verification.md) |
| release 运行 debug-only panic 测试 | [clarify-committed-anchor-release-test](changes/archive/2026-10-29-clarify-committed-anchor-release-test/verification.md) |
| Chrome duration 入口无计时 span | [emit-chrome-instrumentation-timers](changes/archive/2026-10-29-emit-chrome-instrumentation-timers/verification.md) |
| sticky 关闭/compact 时导航几何不一致 | [fix-pager-sticky-navigation-geometry](changes/archive/2026-10-29-fix-pager-sticky-navigation-geometry/verification.md) |
| 后代展示跨实体校验与无 ID 重入重复 | [fix-descendant-presentation-replay](changes/archive/2026-10-29-fix-descendant-presentation-replay/verification.md) |
| tmux post-exit 清理预算缺少确定性区分 | [test-tmux-post-exit-cleanup-budget](changes/archive/2026-10-29-test-tmux-post-exit-cleanup-budget/verification.md) |
| Git/worktree 构建身份监听无效路径（原两条合并） | [fix-build-git-identity-invalidation](changes/archive/2026-10-29-fix-build-git-identity-invalidation/verification.md) |
| PTY 独立 daemon fixture 无 owner | [own-pty-leader-test-daemons](changes/archive/2026-10-29-own-pty-leader-test-daemons/verification.md) |
| OCR 后代与并发 fixture 失败清理 | [own-local-ocr-process-tree](changes/archive/2026-10-29-own-local-ocr-process-tree/verification.md) |
| Plan live handoff 同 Turn 已消费后误报 ACK 丢失 | [fix-plan-handoff-consumption-recheck](changes/archive/2026-10-29-fix-plan-handoff-consumption-recheck/verification.md) |
| Goal Control 后首请求前 scope 未同步 | [fix-goal-control-context-scope](changes/archive/2026-10-29-fix-goal-control-context-scope/verification.md) |
| CI 发现日志溢出测试的诊断位置/中间计数时序假设 | [同步测试](changes/archive/2026-10-29-synchronize-unified-log-overflow-test/verification.md)，55项全量及200次精确执行通过 |
| 活动组真实PTY仍沿用旧分组边界与过早等待 | [契约与同步修复](changes/archive/2026-10-29-align-activity-group-pty-contract/verification.md)，完整活动组交互与剩余16项发布PTY通过 |
| Runtime 回归未进入正式 Core 门禁 | [gate-backlog-runtime-regressions](changes/gate-backlog-runtime-regressions/verification.md) |

- **Native 分段与 AgentMessage 交错候选**：真实生产 drain 位于整批工具边界，未发现反例生产可达性；关闭待证实项，保留直接构造 adapter 历史/未来新调用方的限制。见 [通路审计](changes/archive/2026-10-29-audit-runtime-native-message-tool-batch-boundary/audit.md)。
- **Markdown clone 候选**：普通 resume 移交 entries，搜索 clone 为 Arc；重建 Markdown view 的调用未进入该生产热点。不引入缓存框架，见 [调用归因](changes/archive/2026-10-29-fix-pager-sticky-navigation-geometry/audit.md)。
- **压缩、计数与模型切换**：假零计数、replacement/Completed/Control 的 ACK 边界、取消后重新提升、异步通知、rewind/route 的 portable 重估和独立硬窗口准入已闭合，见 [状态修复](changes/archive/2026-10-29-fix-compaction-pressure-lifecycle/verification.md)、[组合故障注入](changes/archive/2026-10-29-verify-compaction-publication-boundaries/verification.md)。
- **真实持久化与中断恢复**：Summary 失败事务重复冷加载及跨事务 replacement 归属已修复；真实文件/SIGKILL 六组边界通过，见 [验证](changes/archive/2026-10-29-verify-compaction-process-recovery/verification.md)。不等同整机断电。
- **低值 provider 校准与输出预留**：低值回执、连续压缩再增长、冷恢复 Goal 首请求、usage 到 Pager 的恢复快照，以及输出预留触发预压缩均已闭合，见 [四项验证](changes/archive/2026-10-29-resolve-context-token-boundaries/verification.md)、[组合验证](changes/archive/2026-10-29-verify-composed-context-transitions/verification.md)。未配置输出上限时不猜后端默认值。

## 算法优化候选与进度（2026-10-21）

用户要求保存 resume 优化分析并扩展审计主要子系统。完整证据、复杂度、边界、覆盖范围和验证方案见 [算法审计](changes/archive/2026-10-21-audit-grow-algorithmic-hotspots/audit.md)。审计后用户明确授权优先实施 R1、R2、U1/R4、S1，四项已分别归档并保留前后基准；其余候选仍待独立启动，不构成批量实施授权。

| 模块 / 候选 | 后续切口与顺序 |
| --- | --- |
| ChatState Timeline / R1 | 已完成：[admission 增量校验](changes/archive/2026-10-21-optimize-timeline-admission-validation/verification.md)，包含 active Surface 摘要及 compaction/rewind 边界。 |
| Session 恢复 / R2、R3 | R2 已完成：[复用 missing append plan](changes/archive/2026-10-21-reuse-response-projection-append-plan/verification.md)；R3 的 JSON 中间树和 typed replay 往返仍是后续独立优化。 |
| Writer 首次 Timeline append / R5 | 已实施：[前缀bulk恢复](changes/archive/2026-10-21-optimize-timeline-writer-prefix/verification.md)，消除逐事件累计lifecycle复制，保留hash/seq/锁与durable barrier。 |
| 同次恢复 canonical / R6 | 已实施：[canonical复用与exact字节借用](changes/archive/2026-10-21-reuse-resume-canonical-projections/verification.md)，限同次validated snapshot；模型路由补写后重建，保留conflict、rewind/cursor、合成来源及resident frontier。 |
| Pager presentation / P0、P1 | [P0归因已完成](changes/archive/2026-10-22-profile-resume-presentation-attribution/verification.md)；[P1已完成](changes/archive/2026-10-22-defer-historical-markdown-presentation/verification.md)：默认关闭诊断的现代512/2048轮末屏中位数420.0→173.1ms、1384.2→560.9ms，短128轮持平；源/媒体观察/复制/布局门槛保留。[P2已完成](changes/archive/2026-10-22-reuse-validated-descendant-recovery/verification.md)：复用同一load结果的已校验root ownership，后代一次完整读取/校验；现代2048轮默认关闭诊断就绪800.1→577.1ms，star/chain展示恢复约快46%，末屏和RPC基本持平。缺失/超限metadata仍完整回退，不按UI无child跳过校验；就绪与历史分页分离未做。 |
| Pager resume / U1=R4 | 已完成：[布局维护与可见恢复](changes/archive/2026-10-21-optimize-replay-layout-maintenance/verification.md)。含真实历史推进、loading 输入忽略、Ctrl+D 退出再恢复；完整 replay frame 收益很小，未声称解决所有真实负载停顿。 |
| Session storage / S1 | 已完成：[exact commit 物理位置索引](changes/archive/2026-10-21-index-exact-update-commits/verification.md)，保留完整冲突核对与 file/directory sync barrier。 |
| 搜索与列表 / S2、S3、U2 | 搜索可重建增量投影、summary typed-key/延后转换；picker 按实际列表规模评估可见项构造。搜索状态与缓存失效另项设计。 |
| 请求/上下文 / C1–C4 | 图片替换 SurfaceId 索引、rewind anchor 优先；损坏历史单遍 repair 和健康请求 clone 复用先按实际工作负载测量。 |
| 工具与文件系统 / T1–T4 | 多 hunk 共用行表、普通条目 metadata 复用、初始 watch 有界选择分别小步实施；tool manifest 共享先测拷贝成本。 |
| Dashboard / U3 | 历史 child 足够多时评估稳定 top-8，保留运行状态优先、tie 顺序及总数；不因只显示八项就预先截断输入。 |

## 历史完成记录

| 问题 | 归档与验证 |
| --- | --- |
| Plan resume PTY 把首段历史当输入就绪 | [真实 finalize 同步](changes/archive/2026-10-22-synchronize-plan-resume-pty-readiness/verification.md)，最终 2.5.0 发布及同提交 push Core 均通过。 |
| Settings 全表键鼠、重置及保存失败重试/回滚覆盖不足 | [交互覆盖](changes/archive/2026-10-20-complete-settings-interaction-regressions/verification.md) |
| show_tips / auto_update 保存回滚遗留旧默认值 | [默认值修复](changes/archive/2026-10-20-align-preference-rollback-defaults/verification.md) |
| show_model_provider 的四项集成清单遗漏、完整 Settings integration 未进发布门禁 | [设置清单与门禁](changes/archive/2026-10-20-complete-settings-preference-test-inventory/verification.md) |
| 恢复放行后、等待未重绑时再次挂起丢失逻辑等待 | [跨恢复周期等待](changes/archive/2026-10-20-preserve-waits-across-runtime-release/verification.md) |
| 已处置的未派发任务仍阻塞 scope 切换 | [scope 义务校验](changes/archive/2026-10-20-fix-undispatched-planning-scope/verification.md) |
| 树冻结失败重试换 owner、节点意图前 Suspended 等待丢失 | [树挂起准入边界](changes/archive/2026-10-20-fix-tree-suspension-admission-boundaries/verification.md) |
| 自动 Goal 激活/重激活丢失 retained Plan 与提交计数 | [自动 Goal 快照](changes/archive/2026-10-20-fix-automatic-goal-plan-snapshot/verification.md) |
| 退出挂起重复提交后台完成通知，以及节点挂起意图后的超时重试丢失人工等待 | [挂起重试证据修复](changes/archive/2026-10-20-fix-suspension-retry-evidence/verification.md) |
| 持久 Bash 捕获前局部变量遮蔽同名导出值 | [捕获隔离修复](changes/archive/2026-10-20-fix-bash-env-export-capture-shadow/verification.md) |
| session 移除/空闲卸载后动态配置监听未回收 | [resident 监听生命周期](changes/archive/2026-10-20-reclaim-session-config-watches/verification.md) |
| 精确测试 runner 缺少阶段进度、子进程期限与有界收尾 | [runner 加固](changes/archive/2026-10-20-harden-exact-cargo-runner/verification.md) |
| sqlite-vec metadata shadow 名称在公共析构遗漏释放 | [名称生命周期修复](changes/archive/2026-10-20-release-vector-metadata-names/verification.md) |
| 第三方 C 边界/编译诊断、nix 微秒类型及 cross 安装旧 lock | [诊断修复（后续构建生效）](changes/archive/2026-10-20-repair-third-party-build-diagnostics/verification.md) |
| OHOS 容器构建提交号 unknown 与 smoke 未校验身份 | [显式构建身份（后续构建生效）](changes/archive/2026-10-20-bind-release-build-identity/verification.md) |
| Behavior 切换丢失 retained Plan 与提交计数 | [Plan 持久快照](changes/archive/2026-10-19-preserve-plan-state-on-behavior-switch/verification.md) |
| 图片恢复 cursor 重连重复行及旧 spinner | [重连合并](changes/archive/2026-10-19-merge-image-recovery-reconnect-rows/verification.md) |
| child 图片恢复通知、真实历史回放及异常结束漏处理 | [child 图片恢复](changes/archive/2026-10-19-route-child-image-recovery/verification.md) |
| Windows storage CI 未覆盖构建配置路径 | [CI 触发范围](changes/archive/2026-10-19-fix-windows-storage-build-input-triggers/verification.md) |
| 同步 compaction 清空当前 TODO | [保留计划投影](changes/archive/2026-10-19-preserve-planning-through-compaction/verification.md) |
| 工具运行时失败继续采样、原始错误被后续 ACK/Step 错误遮蔽 | [工具失败停止边界](changes/archive/2026-10-19-stop-on-tool-runtime-failure/verification.md) |
| child handoff 重复复制已有历史、重复校验已验证的冷 Timeline | [历史所有权优化](changes/archive/2026-10-19-reduce-child-handoff-history-copy/verification.md) |

挂起优化仅消除已经确认的重复工作。单次本机合成历史测量不代表端到端挂起耗时，也不能解释所有真实长会话等待；仍保留 live 历史验证、最新事实重读、writer/child 关系检查和既有期限。未来只有真实多节点负载证据才能支持进一步优化，不预建缓存或并行退役框架。

原审计证据见 [v2.4.0 后续审计](changes/archive/2026-10-19-audit-post-v2-4-0-boundaries/audit.md)。

本轮后续维护不修改已经发布的 v2.4.1 tag 或产物；历史 release 验证记录保持原样。
