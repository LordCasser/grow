# 未解决边界

截至 2026-10-20，此前 backlog 缺陷的完成记录见下表。退出挂起节点修复之后的扩散审计又确认三个待修边界，保留用户此前明确暂缓的 Remote Control，以及依赖真实路由与负载证据的 Provider/Recap 缓存优化。MCP 不在本轮修改范围。



- **SideQuestion 接收端生命周期（独立跟踪）**：`actor/run_loop.rs` 的 SideQuestion 使用独立 `spawn_local`，不监听 `respond_to.closed()`；请求接收端退出后任务仍可能继续。/btw 已改为只读复用主图片分支，等待主转换时响应 session Sideband cancellation；接收端关闭与请求任务生命周期仍需另项明确及验证，不混入图片分支复用修复。

- **Runtime native 分段与收件消息交错（待证实生产可达性）**：此前审计的真实结果保全、Responses AgentMessage 和 Chat Runtime reasoning 三项已由 [修复记录](changes/archive/2026-10-21-fix-runtime-projection-integrity/verification.md) 处理。剩余 native assistant 分段后插入 AgentMessage 的 adapter 排序反例见 [定向审计](changes/archive/2026-10-21-audit-runtime-thinking-projections/audit.md)；常规通知/用户插话受整批边界保护，生产可达性尚未证实，独立跟踪，不作为线上已复现。

- **Remote Control / Agent Host 跨设备使用（暂缓、未排期）**：用户认可从其他设备连接和操作已有 Grow 会话的方向，但当前不实施、不设日期，不纳入 MCP 多轮或保留计划提案。本次不展开远端控制架构，也不因此引入统一 Dock、Memory 或其他大型功能。未来明确启动时另建 change，重新界定身份、权限、执行 owner、断线和恢复边界；本条不构成实现授权。决策见 [2026-10-11 上游主题评审](changes/archive/2026-10-18-record-upstream-review-2bdd1d6/review.md)。

- **Provider 缓存策略与 native epoch 的关系（暂缓、待真实路由证据）**：[理论 review](changes/archive/2026-09-28-evaluate-provider-cache-affinity/results.md) 只能确认当前 key、breakpoint 与 retention 的本地机制及各官方契约，不能证明统一调整会改善多 provider 的成本与延迟。若要优化，先确认具体 provider/model/deployment、字段透传与负载，再按该 route 的真实证据另开最小行为 change；不预建通用 cache policy registry。
- **Subagent 完成但输出不可验证时的恢复终态（独立债务）**：当 backend 返回 `CompletedOutputUnavailable`，而 child session 存在但尚无合法 `SubagentResult` 时，恢复可能把该观察固化为成功结果和 parent terminal。应保留 spawn 未闭合，直到存在可验证的 canonical child result；已有合法 result 可照常恢复。补充覆盖该组合的测试后另建行为 change；不属于 unified Timeline storage/reconcile reorder 的本轮实现。
- **Recap 与主请求缓存的关系（暂缓、待真实共享前缀与收益证据）**：recap 使用独立 key 且没有主请求 tools，已修正把本地 wire 形状表述成远端复用的注释和测试名。真实共享前缀与收益仍未验证；保留 Sideband tool-free 契约，不能为缓存假设增加工具定义。


- **Release 测试选择与诊断 Chrome 计时（独立工具债务）**：`insert_block_before_rejects_an_already_committed_anchor` 的无条件 `should_panic` 依赖 release 已关闭的 `debug_assert!`，需单独明确测试的构建条件；本轮 release 明确排除且保留失败证据。`instrumentation::timer()` 在 Chrome 模式没有 span guard，现有 duration 埋点不输出该模式的 timing；本轮只使用 log 模式，不顺带改诊断基础设施。证据见 [P0 对抗审查](changes/archive/2026-10-22-profile-resume-presentation-attribution/adversarial-review.md)。
- **Pager 精确导航与缓存的独立余项**：`entry_top_scroll_offset` / center 导航直接计算 sticky prompt 余量，即使关闭 sticky display 也可能给目标保留该余量；须另项核对产品预期，不混入 P1。`StreamingMarkdownRenderer::clone` 会重建中间 view，克隆 pending 历史块可能提前并重复解析；普通 resume 未发现此热调用，先按实际 clone/search 调用归因再决定是否优化。P1 保留内容等价和有界视口校准，未新增克隆缓存框架。


- **子树展示恢复的跨实体校验与无身份重入（独立债务）**：`restore_descendant_state` 是 best-effort 展示路径，canonical parent spawn 授予遍历，但不等同执行/导出的严格 Seed/Result 交叉链接校验；是否加强该策略须独立明确，不能复用展示 payload 获得执行权威。其 forced replay 对缺少 `eventId` 的合法 lifecycle presentation 无 source-local numeric highwater，重复恢复可重复添加后代任务行；现代 producer 有稳定身份，本轮现代基准保留该身份和重入断言。失败证据见 [P2对抗审查](changes/archive/2026-10-22-reuse-validated-descendant-recovery/adversarial-review.md) 与 [原始失败](changes/archive/2026-10-22-reuse-validated-descendant-recovery/descendant-before-corrected.log)，不在重复读取优化中改身份/显示策略。

- **tmux 清理期限的确定性判别（测试债务）**：近期限成功回归保持 1.5s 主期限、1.2s 模拟 leader 和持 pipe 子孙，但名义 300ms 余量大于 TERM 等待的 100ms，不能严格区分共用期限与独立 post-exit 期限。2.5.0 准备仅改用显式 shell 消除临时脚本启动成本，不扩大超时或修改生产清理；未来独立以可控同步验证该差异，不引入通用时钟框架。

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

## 本轮已解决

- **Pager/CLI 构建身份失效路径（独立债务）**：build.rs 以 crate 相对 `.git/HEAD` 注册并不存在的路径，使重复 Cargo 调用重新编译；发布门禁已观察 pager 在 no-run 后的枚举阶段再次编译。exact runner 将直接运行已编译产物以隔离阶段预算；Git/worktree/branch ref 的精确失效追踪另项修正，不混入本次 runner 改动。


| 问题 | 归档与验证 |
| --- | --- |
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

- Pager build identity: `crates/codegen/pager/build.rs` watches crate-relative `.git/HEAD`, which is absent in this checkout; consecutive exact tests on clean source repeatedly rebuild Pager (~25s each). Confirm Cargo fingerprint diagnostics and reuse the existing release identity approach with correct worktree/ref tracking in a separate change. This release preparation only reuses verified test executables locally; no build-script change.

- PTY 测试的独立 Leader 清理所有者：本机发布验证发现五个带 `--no-exit-on-disconnect` 的专用测试 Leader 在 sandbox 删除后仍驻留。后续单独补齐测试 harness 的独立 daemon shutdown/owner，不改变产品的常驻语义；本轮已按 candidate 路径、隔离 GROW_HOME 和启动时间确认并终止，无用户实例被清理。

- 局部辅助进程与测试的后代清理边界：质量清理逐处核对发现，本地 OCR 的现有 kill_on_drop/timeout/wait 只保证直接子进程，部分同步并发 fixture 只在正常断言路径显式 wait/kill，未统一证明 panic 或任意后代回收。后续按真实失败证据独立验证现有 ProcessScope 能否复用；不在 lint 清理中新增进程管理层或改独立 helper 所有者。见 [资源与路径复核](changes/clean-resource-path-clippy-diagnostics/review.md)。
