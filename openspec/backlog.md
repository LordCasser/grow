# Backlog

这里只保留已有明确暂缓决定或启动门槛的事项。2026-11-06用户重新授权“完成修复和处理，处理backlog的非延期项目”；本轮按[实施范围与验收](changes/archive/2026-11-07-complete-nondeferred-backlog/proposal.md)推进，下文2026-11-05停止点仅作历史。刚分析的child收尾及SessionEnd顺序在本轮范围，其他明确暂缓事项继续保留。八项非延期验证债务已完成修复、验证并随 [v2.6.0](https://github.com/LordCasser/grow/releases/tag/v2.6.0) 发布；逐项 owner、证据和剩余边界见[发布阻塞闭合矩阵](changes/archive/2026-11-04-close-v2-6-0-release-blockers/verification.md#八项闭合矩阵)，原始八项快照见[backlog-before.md](changes/archive/2026-11-04-close-v2-6-0-release-blockers/backlog-before.md)。既有历史整理记录见[本轮闭合记录](changes/archive/2026-11-03-close-nondeferred-backlog-and-build-release/algorithm-inventory.md)及其[清理前快照](changes/archive/2026-11-03-close-nondeferred-backlog-and-build-release/backlog-before.md)。

## 2026-11-07 发布前范围收拢

用户最新要求只完成 P0、必要或已经进行的修复，然后准备 2.6.2。当前实施范围以[收拢记录](changes/archive/2026-11-07-complete-nondeferred-backlog/proposal.md#2026-11-07-范围收拢)为准；长历史双 Ctrl+D 五秒退出超时为发布主线。表中尚未启动的卸载/普通更新保存错误、Markdown 性能、Workflow 测试和 Agent 公共实体整理继续保留，不在本轮追加实施。ACP 容量实验尚未写源码，已停在[准备材料](changes/measure-acp-gateway-backlog/design.md)，没有运行结果，也没有容量政策改动。

2026-11-08 后续完成其中的卸载/普通更新持久化错误传播，验收见下方 AC-08 闭合记录；上述 11-07 收拢文字保留为历史停止点。

## 本轮全仓审计发现，待独立修复

用户于2026-11-05要求停止新增审查并把当前成果作为发布前里程碑。已接受修复逐项归档；下列未闭合项保留为后续工作，本里程碑不再启动新的调查或实施。状态、验证限制和原停止点见[里程碑记录](changes/archive/2026-11-05-coordinate-audit-backlog-repairs/milestone.md)，发布与验收见[2.6.1发布记录](changes/archive/2026-11-06-prepare-release-v2-6-1/verification.md)。

以下事项来自 [跨版本架构审计](changes/archive/2026-11-05-audit-grow-architecture-across-versions/proposal.md)。用户随后已授权按全局职责与共享契约统筹修复，执行顺序、待核实边界与集成里程碑见[统一修复设计](changes/archive/2026-11-05-coordinate-audit-backlog-repairs/design.md)。各项仍按独立 owner 建 change，验收后移出本表，不并入 model/effort 特性；静态、机制实验和真实入口证据分别保留。WS-01 见[索引最终删除修复与57项回归](changes/archive/2026-11-05-preserve-code-index-removals/verification.md)。已闭合的 WS-06b 见[成功提交修复与验证](changes/archive/2026-11-05-commit-hunk-projections-after-success/verification.md)，WS-03 见[工作树删除归属修复与44项回归](changes/archive/2026-11-05-bind-worktree-registration-cleanup/verification.md)；WS-02 见[ClientFS 当前内容摘要修复与18项回归](changes/archive/2026-11-06-read-client-files-with-current-digests/verification.md)。已闭合的 WS-06a 外部编辑保护见[磁盘冲突与Shell传播验证](changes/archive/2026-11-05-reject-stale-hunk-targets/verification.md)。

本轮已闭合的 WS-04 跨主机记忆身份见[修复与321项回归](changes/archive/2026-11-06-qualify-memory-repository-identities/verification.md)，WS-05 envrc 路径求值见[特殊目录名与平台限制](changes/archive/2026-11-06-pass-envrc-paths-as-arguments/verification.md)；Session F4见[恢复前缀屏障与真实failed-completion对照](changes/archive/2026-11-06-confirm-workflow-journal-recovery-barriers/verification.md)，F5见[journal不可变完成与69项回归](changes/archive/2026-11-06-reject-repeated-journal-completions/verification.md)。Responses usage补全保留重复字段冲突见[raw字段保真与70项回归](changes/archive/2026-11-06-preserve-response-normalization-field-conflicts/verification.md)。readiness预算归属见[6项Coordinator与250项子任务回归](changes/archive/2026-11-06-separate-finalization-readiness-budget/verification-shell.md)，未就绪观察不消耗尝试，原绝对期限保持。child SessionEnd/Result顺序见[正常、取消、故障与普通退出验收](changes/archive/2026-11-06-finalize-child-hooks-before-result/verification.md)；该项本轮已关闭，不再列为延期。流错误资格与Responses控制帧顺序见[三reader旧红新绿、42项分类和79项reader回归](changes/archive/2026-11-06-qualify-stream-error-envelopes/verification.md)，HTTP非2xx与合法服务错误分类保持。AC-02 空插件快照撤销见[真实Hook执行与28项回归](changes/archive/2026-11-06-clear-hooks-on-empty-plugin-snapshots/verification.md)，config/Agent来源保留。PB-01见[真实转发与17项CLI回归](changes/archive/2026-11-06-observe-stdio-session-close-before-replay/verification.md)，PB-07见[22项恢复、真实崩溃子进程及CLI启动控制](changes/archive/2026-11-06-preserve-crash-evidence-until-report-commit/verification.md)，AC-05见[目录环/合法别名与58项发现回归](changes/archive/2026-11-06-stop-agent-discovery-directory-cycles/verification.md)。PB-02见[精确RPC身份配对与21项CLI回归](changes/archive/2026-11-06-correlate-stdio-session-creation-responses/verification.md)。MCP-F3见[重复SDK数组拒绝与七项入口/解析回归](changes/archive/2026-11-06-reject-duplicate-sdk-mcp-declarations/verification.md)。AC-04见[物理checkout祖先、linkedworktree与82项Agent回归](changes/archive/2026-11-06-bind-agent-rules-to-physical-checkout/verification.md)。F1见[双进程冷恢复与原Turn单次消费](changes/archive/2026-11-06-restore-steering-to-preserved-turns/verification.md)。F3的canonical查询提供方已独立[验收](changes/archive/2026-11-06-expose-canonical-input-lifecycle/verification.md)，消费方已通过[actor96项与真实ACP重试链](changes/archive/2026-11-06-observe-stable-input-retries/verification.md)；F2见[canonical槽位继承与真实JSONL编辑/冷恢复身份验收](changes/archive/2026-11-07-preserve-edited-fifo-recovery/verification.md)，原行/runtime与immutable编辑输入身份分别保持，手动reorder重启持久化不在该修复范围。

原生子采样控制的自然收尾夹具已按已归档的child退休顺序修正，三个竞态场景保留；完整21个native和10个owner场景及独立actor拒绝控制已通过，见[阶段修正与最终回归](changes/archive/2026-11-07-align-native-control-retirement-fixture/verification.md)。父工具同一行 Dispatching→Queued/Failed、child普通控制反馈及启动发布顺序已验收，见[模块与真实终端链](changes/archive/2026-11-07-show-subagent-sampling-controls/verification.md)：保Queued≠Applied，child先建立control epoch，closed-Step后一次notice和实际目标model/effort请求；freshbuild v10原exact与正常退出均0。模型池仍遵循独立设计批准门槛。

2.6.2候选中的 `rg --hostname-bin` 已按实际helper执行、Ask/Auto/grant和冻结RX投影完成修复，含`--glob --`反例，见[权限修复与21项最终回归](changes/archive/2026-11-06-guard-ripgrep-external-execution/verification.md)。MCP取消派发三秒Grow等待界见[真实受阻传输旧红新绿与16项回归](changes/archive/2026-11-06-bound-mcp-cancellation-dispatch/verification.md)。Goal空闲人类依赖解除唤醒见[真实mailbox判别、Goal/planning回归与原报告限制](changes/archive/2026-11-06-repair-goal-turn-continuation/verification.md)。四项候选的实际采纳和未证边界见[固定来源对照](changes/archive/2026-11-07-complete-nondeferred-backlog/upstream-2-6-2-review.md)。 MCP 目录名/provider 投影边界现已闭合：完整 identity 保留至 256，直接声明保持 64，真实 catalog 17/17 及 descriptor 完整源码旧红新绿通过；见[验证与存储边界](changes/archive/2026-11-09-separate-mcp-catalog-name-bound/verification.md)。

MCP-F1初始化后调用资格已验收，见[真实握手旧红、16项资格矩阵与197/13项回归](changes/archive/2026-11-07-revalidate-mcp-call-eligibility/verification.md)：model/direct共用当前client/disabled及live上游ceiling复核，保原absolute deadline和no-replay；Shell真实显式endpoint控制保持。父审查发现的direct预算退还及测试received计数已修订；网络中已发请求不承诺回滚，远端生产/Windows未实测。

PB-04 exact runner错误确认已闭合，见[真实setsid＋DEVNULL三类中断旧红新绿和18项回归](changes/archive/2026-11-07-report-detached-runner-cleanup/verification.md)。Unix仍尽力清理本次进程组并回收直接child，对脱组后代明确未确认，不承诺任意树已回收；PB-03 的 ptyctl 生命周期已按本轮独立验证记录闭合。

问答两模式已验收，见[模块、独立review与最终真实产品链](changes/archive/2026-11-07-offer-advisory-user-questions/verification.md)：Settings允许Agent逐次选择、关闭后live/cold强制blocking；建议迟答以稳定普通输入接纳，旧工具单一结算。新CLI同产物mouse1pass、keyboard/Settings2pass，正文无需额外导航即可显示；原阻塞问答卡住报告仍按下方现场证据门槛保留，不因本特性宣称已复现修复。

AC-01 见[共享设置锁、原始字段和 CTA 实际消费方验收](changes/archive/2026-11-06-serialize-plugin-config-mutations/verification.md)。AC-03a会话LSP来源隔离见[真实ACP旧红新绿与三阶段进程验收](changes/archive/2026-11-07-bind-session-lsp-plugin-snapshot/verification.md)：新backend只取本次session插件快照，同cwd不同pluginDirs逐会话归属、不同cwd同名file/inline、sharedlatest污染下None/disabled及合法user/project控制均通过；继承Arc保持、无LSP全量reload，Windows未实测。

Sampler任务归属与取消回合预算见[模块及产品链验收](changes/archive/2026-11-05-bind-sampler-request-task-lifetime/verification.md)和[取消终态修复](changes/archive/2026-11-05-retire-cancelled-turn-sampling-state/verification.md)：64项模块回归已独立验收，新增默认CLI上的原生取消/重试exact通过，五个请求/结算/证据身份一致。

以下表中每一行是独立修复边界；同一报告有多项发现时仍分别派发，不把整个分区打包实施。详细触发、版本/符号证据和可证伪通过标准在所链接报告，优先级以真实影响和证据为准。

AC-06 Skills 配置热更新已闭合：leader真实配置→目录/广告/模型资格及custom roots里程碑1/1；in-process九项及相邻MCP两项11/11；共享watcher22/22。独立review的已完成注册取消析构、None误去重均修正验证。macOS discovery以metadata polling避开已动态确认的FSEvents注册阻塞。见[两入口验证](changes/archive/2026-11-07-deliver-skills-config-reloads/verification-leader.md)与同目录in-process/backend记录。

长历史退出主线已完成：完整真实 owner 的旧复制/重放25,050ms转为窄查询3.916ms；复杂305.7MB实际ACP旧红5,002ms unconfirmed转123ms confirmed，最终2.6.2同产物连续两轮立即双Ctrl+D，实际wait0为236.87/240.73ms，历史prefix/冷恢复/持久引用完整。见[最终CLI验收](changes/archive/2026-11-07-avoid-exit-history-replay/verification-final-cli.md)。通用OS文件监听退役抖动继续以独立P2保留，未隐藏早期慢轮。

PB-05 多进程 crash capture 归属已闭合：live owner/并发同秒恢复/exec 锁不继承等真实 ownership 场景、crate 持久化回归与 CLI 多 capture 排空/首错停止两项均通过，独立数据完整性 review 接受。见[证据](changes/archive/2026-11-07-isolate-crash-capture-owners/verification.md)，保留 Windows 未原生运行限制。

MCP-F2 SDK 注册者路由已闭合：实际 ingress覆盖身份/畸形早拒绝、A注册/Bdriver/同serverId及断线无fallback、真实new/resident/cold与SharedMcpPool Arc继承实际调用均通过。见[生命周期证据](changes/archive/2026-11-07-route-sdk-mcp-to-registration-owner/verification-sdk-lifecycle.md)及同目录leader验证，继承覆盖限SDK Arc/transport，不扩大为任意child启动全流程。

AC-08 普通 Install 子项已闭合：共享 application/真实 ACP 两项有效保存失败旧红转绿，Pager public dispatch 的 save/parse/read/success 控制通过；不继续 post-install 或伪报 reload，已发布 artifact 保留并明确不完整状态。见[安装验证](changes/archive/2026-11-07-report-plugin-install-persistence-failures/verification.md)。卸载和普通更新现已分别完成：坏登记早失败、真实删除失败保记录、保存部分完成不伪报成功、实际 ACP/CLI 及混合 marketplace 批次通过，见[卸载验收](changes/archive/2026-11-09-report-plugin-uninstall-failures/verification.md)与[更新验收](changes/archive/2026-11-09-report-plugin-update-persistence-failures/verification.md)。源码级差分控制三组旧红/新绿，最终独立 grow CLI 11 次调用通过；完整远端 CI 仍由整合任务确认。

AC-03b 已完成 Shell 精确 session 插件快照与 Pager session/epoch 回包接线：真实 ACP 六方法入口回归与目录旧红/新绿、owner 3 项和既有 24 项通过，Pager 七项通过且独立 review 无发现。见[Shell](changes/archive/2026-11-07-bind-skill-catalogs-to-session-snapshots/shell-verification.md)与[Pager](changes/archive/2026-11-07-bind-skill-catalogs-to-session-snapshots/pager-verification.md)。全局 disabled 与目录热更新仍依各自契约，不复制额外目录 owner。

PB-06 已以最小 raw-only 策略闭合：旧恢复会误归因当前函数的反例转绿，真实不同 ASLR slide 对照保全部 IP、禁止自动归因；全 crate 31 unit、9 integration、3 ownership、1 doc 通过，见[地址归属验证](changes/archive/2026-11-07-avoid-cross-process-crash-symbols/verification.md)。未新增 module/load mapping，也未声称 Windows 原生运行验证。

PB-03 PTY 生命周期已完成 Session→HTTP/WS→CLI 接线：真实 CLI 10 项、库 30 项和 registry 2 项通过，Windows lib/tests cross-check 通过但未原生运行，见[整合证据](changes/archive/2026-11-07-settle-pty-session-lifecycle/verification-service.md)。直接 child 与 I/O 实际退役、慢连接有界错误、force 名称归属均按同一 owner 验收，未扩展任意进程树控制。

AC-07 ACP 更新采用已通过真实 bare remote 的旧红新绿；checkout 与 registry commit/plugin map 一致，第二次同 tip 不产生更新，见[验证记录](changes/archive/2026-11-07-adopt-acp-plugin-update-results/verification.md)。保存错误的 AC-08 仍按安装、卸载、更新分别验收。

Workflow 测试的已记录 Unix 用户发现污染已闭合：共享 sandbox 隔离 62 个既有测试入口，保留真实发现与原 manifest 上限。完整远端 Shell 有 4,794 项通过；额外固定产物在污染外层环境中复现非隔离的 524288 字节拒绝，隔离后 148 项 Workflow、3 项启动和 2 项恢复回归通过。来源哈希、精确测试裁剪边界与完整 CI 的独立性见[隔离验证](changes/archive/2026-11-09-isolate-workflow-test-discovery/verification.md)。Windows 的 Known Folder 用户 Agent/Skill 来源不遵循 USERPROFILE，本项未宣称该平台隔离完成；原历史资源字段字节占比仍未测。

| 项目 | 独立 owner / 修复边界 | 当前证据与启动门槛 |
| --- | --- | --- |
| macOS 文件监听退役抖动 · P2 · 现场待进一步复现 | 通用 fsnotify watcher 的 native Drop/join 与进程退出尾部 | 2.6.2 长历史 CLI 验证中，一轮 filesystem watcher 退役 4747ms，runtime 4845ms 已确认但进程 6019ms 仍活；其它同负载两轮 actual wait 225ms。慢段未采到动态栈，源码有同步 join，不能凭两轮成功宣称该 OS 抖动已修。保留 `/tmp/grow-complex-cli-acceptance-ready-8cf671e4/` 与 `/tmp/grow-complex-cli-slowwatch-8cf671e4/`，独立定位，不扩大本轮为通用 fsnotify backend 重写。 |
| ACP-1 · 容量债务 | ACP transport明确queue/handler预算及过载语义 | [客户端报告](changes/archive/2026-11-05-audit-grow-architecture-across-versions/reviews/client-ui.md)；静态风险，先慢handler/生产者负载实验或证明现有外部上限，不预设正常CLI失效阈值 |
| MD-1 · 已测量性能债务 | Markdown开放fence输出复用边界 | [优化构建测量](changes/archive/2026-11-09-measure-open-fence-copy-costs/verification.md)：502 项输出回归及显式测量通过；4096 行合成 YAML 的 warm clone 为 3.101ms，实际 Markdown 增长帧中位 7.772ms、最大 8.638ms。未超过默认 30FPS 动画间隔不等于全 UI 预算通过；累计复制仍为二次增长，未改生产表示或宣称债务已修复。 |
| Agent公共实体用途核对 · 待用途核对 | Agent owner确认ToolExecConfig/Retry与preset public seam消费者 | [配置报告](changes/archive/2026-11-05-audit-grow-architecture-across-versions/reviews/agent-config.md)，无仓内消费不足以证明公开API可删；先确认受支持外部产品需求，再做窄删除/依赖收缩，不预建框架；announcements直接依赖config已独立[验收](changes/archive/2026-11-07-depend-on-config-for-announcements/verification.md)，原转发路径和状态语义保持 |

| 交付释放/迟到结果 · 源码已修，现场待新构建恢复 | tool-types LocalPlan历史review绑定与Shell canonical结果提交 | [独立修复与实验](changes/archive/2026-11-05-audit-released-delivery-finalization/verification.md)：原公共API反例已在新生产库转为Ok，planning26/26、独立review和默认CLI check/build通过。只放宽历史Released(None)，其他review/canonical/终态门槛保持。当前旧宿主仍Receipt Unknown、link未terminal，需正常退出后用新项目构建恢复，再核对真实结果与replan；不能将源码验证当作现场闭合 |

## 采样解码后续项（2026-11-06 有界分析）

用户本轮要求分析是否仍有同类问题；以下分别是独立修复边界，本次只登记、不实施，不扩成全仓审计。完整输入、证据强度与候选限制见[分析](changes/archive/2026-11-06-analyze-sampling-decode-residuals/analysis.md)。

流错误资格已在独立change关闭，见本轮闭合记录；不以该项代替下文Anthropic标准错误分类或真实跨协议路由验证。

## 跨协议模型切换的官方契约对照（2026-11-06）

用户明确本题主要是同会话跨协议、跨模型切换；本次完成官方资料和本地表示验证，尚无新first-party推理请求。来源、旧六方向实测限制及22项当前回归见[官方对照](changes/archive/2026-11-06-compare-official-sampling-api-contracts/analysis.md)。以下两项分别是独立边界，不能以关联的单次流解码修复代替：

- **目标Chat对runtime合成工具历史的许可**：DeepSeek官方说明Chat不支持client在历史中段插入非模型生成的工具调用/结果，而Responses/Anthropic格式支持。Grow的AgentMessage和用户SkillLoad会投影为synthetictool往返，portable切换后仍保留；本地wire与该限制的冲突前提已定位，实际target是否拒绝、何种history/版本适用仍待受控目标验证。后续由目标Chat适配与中性runtime事实表达共同确认契约，不把来源升级为DirectUser，不删已接纳内容、不伪造调用执行。对照真实模型工具历史、runtime合成历史及三个目标backend的接纳结果；不扩大为所有Chat或所有foreign真实工具历史都不支持。
- **reasoning回放的完整性和可迁移保证**：DeepSeek thinking+tools要求所有先前回合完整reasoning_content，当前VisibleReasoningItem只存可见text，Responses/Claude来源可能只给summary或不给正文。route隔离、目标明确400后的ACK学习、空字段补齐已覆盖，但这些不能恢复未取得的full reasoning。后续先验证具体目标接受何种foreign/无reasoning历史，再确定迁移支持及最小provenance需要；不预建全局capability框架，不捏造签名/完整原生思考。既有真实六方向矩阵没有触发reasoning400，不能算该恢复分支的线上成功证据。

关联但独立的分类项：**Anthropic标准api_error/timeout_error**已完成共享owner修复，见[两处映射、真实Messages reader/loopback及53项回归](changes/archive/2026-11-07-classify-standard-messages-stream-errors/verification.md)。合法流内精确类型分别使用逻辑500/504，真实HTTP非2xx状态和显式veto保持，恢复仍受原结算/候选撤回/次数与期限约束；未知lookalike不放行。typed顶层provider request_id仍仅raw保留，本项未新增结构化提取，也不声明真实Anthropic部署现场已复现。

## 原阻塞问答迟答报告的证据门槛

原任务结束后补答导致持续卡住的报告尚未复现。[已归档调查与父裁决](changes/archive/2026-11-07-settle-late-question-replies/verification.md)包含长等待、receiver关闭后迟答和持久ACK扣留判别，三项问答判别与14项worker回归通过；现有InteractionResolved和Pager清理链成立。扣留ACK只证明通用前提，silent send失败也不证明持续busy，因此不新增猜测的取消协议或持久化期限。再次出现时需精确source/call、原Turn结束原因和同期transport/canonical状态，或真实duplex受控复现，再按根因启动修复；新增advisory迟答功能不能替代这个现场结论。

## 子任务正文显示后未收尾（2026-11-06 纯代码分析）

用户无法取得Windows现场，明确改为纯代码分析。主报告见[收尾链路分析](changes/archive/2026-11-06-investigate-windows-subagent-result-stall/investigation.md)。以下为源码边界，均不等于该机器根因已确认，不在本轮实施：

- **已闭合的观察与快照边界**：Shell Outstanding ACK和轮询共用原绝对期限，超时保留incomplete及child责任，见[用量冻结修复与18项回归](changes/archive/2026-11-06-bound-usage-freeze-observation/verification.md)。Workspace after读取采用5秒总观察预算、保留已成功项，未知与确认不存在分开；Shell最新触及点未知时给出snapshot_unavailable，见[after-snapshot与rewind验证](changes/archive/2026-11-06-bound-after-snapshot-capture/verification.md)。这些验证不证明Windows现场根因，也不提供强制中断同步I/O或锁等待的保证。
- **readiness 预算边界已闭合**：原“未就绪观察耗掉三次机会”的问题已由[独立修复](changes/archive/2026-11-06-separate-finalization-readiness-budget/verification-shell.md)关闭。当前 readiness 观察不消耗 reconciliation 尝试，原绝对期限、有限 mutation/ACK 次数及 writer lease 保持；child SessionEnd/Result 顺序也已[独立验收](changes/archive/2026-11-06-finalize-child-hooks-before-result/verification.md)。这些源码与回归结论不等同于 Windows 现场根因确认。

## 明确暂缓

- **Sampler 并发日志捕获测试（待独立复现）**：本次稀疏失败回归首轮并发执行时，既有 `client::tests::sampling_auth_logs_omit_credentials` 失败一次；该项单独重跑与完整串行326项均通过。尚未确认根因，不据此认定生产凭据泄漏或修改日志实现；后续应在并发日志订阅/捕获边界建立受控复现后独立修复。原始失败、exact和串行对照见[验证记录](changes/archive/2026-11-06-decode-sparse-responses-failure/verification.md)。

- **普通长 prompt 导出文件寿命（待独立链路复现）**：`run_turn_via_sampler` 局部的 PromptBlobExport 在采样返回时删除 TempDir，而模型的 read_file 发生在后续工具阶段；需独立复现普通 prompt 的采样→全文读取，再确定既有 artifact owner 的寿命边界。当前现场走 steer，未使用该导出路径，不并入插话保真修复。证据和范围见[长输入调查](changes/archive/2026-11-03-preserve-long-steering-input/design.md#separate-audit-finding)。

- **共享布局以外的字素边界（待对应复现）**：本次 [字素布局修复](changes/archive/2026-11-03-fix-tui-grapheme-layout/design.md) 限于普通文本共享内置换行、截断和搜索列映射。`ratatui-inline/src/segment.rs` 的 ANSI 历史行拆分仍按 scalar 计宽，`pager-render/src/render/terminal_output.rs::TermSink::put` 的 ANSI cursor model 仍按字符推进；跨样式 span 的字素归属也需先与 Ratatui 的逐 span 绘制语义统一。若出现对应 Minimal resize、带 cursor movement 的中文工具输出或跨 span 组合符号复现，再分别建立 change 验证各自所有者，不能将本次字体回退截图当成这些路径的复现。

- **跨路由 reasoning 后续边界**：[第一阶段审查](changes/archive/2026-10-29-improve-cross-route-reasoning-compatibility/audit.md) 发现通用 native-bearing HTTP 400 仍会触发一次 reset，以及可见 reasoning 仅保存 text、未区分摘要和完整正文。本阶段修复错误事实保真和目标请求投影；若真实目标要求更强语义，应独立处理精确 continuation 拒绝分类或最小 provenance，不预建全局 capability registry。

- **Remote Control / Agent Host 跨设备使用（暂缓、未排期）**：保留方向，不实施、不设日期；未来启动时另建 change，明确身份、权限、执行 owner、断线和恢复边界。不得据此引入 Dock、Memory 或远端控制框架。决策见 [上游主题评审](changes/archive/2026-10-18-record-upstream-review-2bdd1d6/review.md)。

- **Provider 缓存策略与 native epoch（待真实路由证据）**：[理论 review](changes/archive/2026-09-28-evaluate-provider-cache-affinity/results.md) 仅证明本地 key/breakpoint/retention 机制。先取得具体 provider/model/deployment、字段透传和真实负载证据，再做该 route 的最小优化，不预建 registry。

- **Recap 与主请求缓存（待真实共享前缀及收益证据）**：recap 保持独立 key、tool-free Sideband 契约；不为假设中的缓存复用添加主请求 tools。先测真实收益。

- **跨多个合法选区连续摘要（契约选择，未启动）**：当前一次有界摘要后仍超窗会保留已提交事实并终止回合；继续压缩更多选区需另立契约，不增加无界重试。

- **MCP 多轮 elicitation（现有独立提案）**：宿主 SDK 接点仍未通过既定门槛，保留 [提案与任务](changes/add-mcp-multi-round-elicitation/tasks.md)，不把未实现功能列为本轮缺陷。

- **ProcessScope supervisor 与 leader reap 的身份同步（待受控 PID/PGID 复用证据）**：terminal 已在最终组信号后、reap 前撤销自身强引用；全局 `kill_all` 若更早 upgrade Weak，仍有独立的跨线程收尾交错。先在可控制 PID 复用的隔离环境验证，不凭假设引入全局 owner/锁协议。范围和当前证明边界见[命令组审查](changes/archive/2026-11-04-finish-command-groups-before-leader-reap/review.md)。


## 2026-11-12 子任务观察与 Tasks 恢复审查

四项 P2 已修复。触发条件见[原审查](changes/archive/2026-11-12-wake-on-any-subagent-result/audit.md)，实现范围与测试见[修复验证](changes/archive/2026-11-12-close-observation-and-resume-gaps/verification.md)。
