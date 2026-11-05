# Backlog

这里只保留已有明确暂缓决定或启动门槛的事项。八项非延期验证债务已完成修复、验证并随 [v2.6.0](https://github.com/LordCasser/grow/releases/tag/v2.6.0) 发布；逐项 owner、证据和剩余边界见[发布阻塞闭合矩阵](changes/archive/2026-11-04-close-v2-6-0-release-blockers/verification.md#八项闭合矩阵)，原始八项快照见[backlog-before.md](changes/archive/2026-11-04-close-v2-6-0-release-blockers/backlog-before.md)。既有历史整理记录见[本轮闭合记录](changes/archive/2026-11-03-close-nondeferred-backlog-and-build-release/algorithm-inventory.md)及其[清理前快照](changes/archive/2026-11-03-close-nondeferred-backlog-and-build-release/backlog-before.md)。

## 本轮全仓审计发现，待独立修复

用户于2026-11-05要求停止新增审查并把当前成果作为发布前里程碑。已接受修复逐项归档；下列未闭合项保留为后续工作，本里程碑不再启动新的调查或实施。状态、验证限制和原停止点见[里程碑记录](changes/archive/2026-11-05-coordinate-audit-backlog-repairs/milestone.md)，发布准备见[2.6.1候选](changes/prepare-release-v2-6-1/proposal.md)。

以下事项来自 [跨版本架构审计](changes/archive/2026-11-05-audit-grow-architecture-across-versions/proposal.md)。用户随后已授权按全局职责与共享契约统筹修复，执行顺序、待核实边界与集成里程碑见[统一修复设计](changes/archive/2026-11-05-coordinate-audit-backlog-repairs/design.md)。各项仍按独立 owner 建 change，验收后移出本表，不并入 model/effort 特性；静态、机制实验和真实入口证据分别保留。WS-01 见[索引最终删除修复与57项回归](changes/archive/2026-11-05-preserve-code-index-removals/verification.md)。已闭合的 WS-06b 见[成功提交修复与验证](changes/archive/2026-11-05-commit-hunk-projections-after-success/verification.md)，WS-03 见[工作树删除归属修复与44项回归](changes/archive/2026-11-05-bind-worktree-registration-cleanup/verification.md)；WS-06a 外部编辑保护见[磁盘冲突与Shell传播验证](changes/archive/2026-11-05-reject-stale-hunk-targets/verification.md)。

Sampler任务归属与取消回合预算见[模块及产品链验收](changes/archive/2026-11-05-bind-sampler-request-task-lifetime/verification.md)和[取消终态修复](changes/archive/2026-11-05-retire-cancelled-turn-sampling-state/verification.md)：64项模块回归已独立验收，新增默认CLI上的原生取消/重试exact通过，五个请求/结算/证据身份一致。

以下表中每一行是独立修复边界；同一报告有多项发现时仍分别派发，不把整个分区打包实施。详细触发、版本/符号证据和可证伪通过标准在所链接报告，优先级以真实影响和证据为准。

| 项目 | 独立 owner / 修复边界 | 当前证据与启动门槛 |
| --- | --- | --- |
| WS-02 · P2 · 内容和摘要错配 | workspace ClientFS memo契约，不以size+mtime_ms证明内容代际 | [存储报告](changes/archive/2026-11-05-audit-grow-architecture-across-versions/reviews/workspace-storage.md)，静态/函数实验确认；真实stat/read回归需同大小同毫秒不同纳秒与atomic replacement |
| WS-04 · P2 · 跨host记忆合并 | MemoryStorage仓库身份规范化保留host/port，仍合并同仓库SSH/HTTPS | 同报告，固定同org/repo输入已确认确定性碰撞；真实storage构造验证clone/worktree隔离 |
| WS-05 · P2 · envrc路径求值 | workspace envrc把路径作为参数传递，独立评估进程取消/期限 | 同报告，受控路径命令替换已执行；trusted wrapper存在，不能称folder-trust绕过 |
| Session F1 · P1 · 挂起steer冷恢复失败 | shell input restore遵循保留Turn的Timeline route，validator不放宽 | [会话报告](changes/archive/2026-11-05-audit-grow-architecture-across-versions/reviews/session-runtime.md)，静态完整链；建立带未消费Steer(T)的冷恢复fixture，与无checkpoint中断对照 |
| Session F2 · P2 · 编辑FIFO恢复位置/身份变化 | durable queue replacement与重建owner保留原位置/行及运行身份 | 同报告，A/B与A′物理顺序和edit身份已核实；多次编辑后各ACK边界冷恢复验证 |
| Session F3 · P2 · 重试受运行态影响 | admission先查stable canonical lifecycle，移除不完整平行fold | 同报告，idle/running改变同请求intent、supersedes状态漏折叠；覆盖ACK丢失和冷writer重试不重复Hook/执行 |
| Session F4 · P2 · journal恢复未确认屏障 | workflow JournalStorage及shell Run恢复确认可读前缀后才继续 | 同报告，完整append后注入sync失败时live Pending/cold Completed；需真实storage的file/dir sync故障测试，不声称已证重复child |
| Session F5 · P3 · 冲突completion覆盖 | workflow journal同一operation的不可变结果，live/loader/projector一致 | 同报告，原源码实验接受pending→completed→异结果completed；正常producer可达性未证，先补完整性验证 |
| AC-01 · P1 · 插件配置旁路写 | shell config已有raw mutation/SAVE_LOCK/atomic发布覆盖全部plugin/CTA writer | [配置报告](changes/archive/2026-11-05-audit-grow-architecture-across-versions/reviews/agent-config.md)，硬读错置空与直写已核实；保留未知字段/环境引用，故障与并发保存测试 |
| AC-02 · P1 · 空插件快照遗留Hook | session plugin adoption在Some→None时撤销旧插件hook贡献 | 同报告，Some-only清除及None生产者已核实；可观察Hook执行对照，不删Agent/config hooks |
| AC-03a · P1 · LSP跨会话快照 | session bootstrap使用本次per-cwd/session plugin registry | 同报告，构造已独立但LSP另读shared；A/B不同cwd/同名LSP及session pluginDirs进程fixture |
| AC-03b · P2 · Skills目录作用域混合 | Skills API明确session/cwd身份，再与模型可调用owner对齐 | 同报告；同cwd不同pluginDirs反例先定共享契约，不凭cwd猜唯一会话 |
| AC-04 · P2 · 规则alias越过checkout | agent rules从一致物理身份构造真实祖先链 | 同报告，路径实验漏repo rule并加入外部祖先；canonical equality不足以补回alias指向内部子目录的根规则 |
| AC-05 · P2 · Agent扫描链接环 | Agent递归walker祖先身份/预算，保留合法namespace | 同报告，32 lexical/1 physical实验；不依赖OS链接层数限制，非环符号链接策略明确 |
| AC-06 · P2 · Skills配置事件丢弃 | shell/pager config routing交付既有reload owner并更新custom roots watcher | 同报告，leader仅日志/in-process丢弃已核实；直接写config对照，UI常规toggle另有refresh不能混报 |
| AC-07 · P2 · ACP插件更新存旧记录 | plugin application采用已有UpdateResult→registry逻辑 | 同报告，CLI正确对照已核实；本地bare remote推进commit/plugin，第二次同tip应no-op |
| AC-08 · P2 · 插件I/O失败仍成功 | plugin application按安装/卸载分别传播durable失败与部分完成 | 同报告，关键save/removal被warn吞掉；分别建有界修复任务，不把installer重写成框架 |
| ACP-1 · 容量债务 | ACP transport明确queue/handler预算及过载语义 | [客户端报告](changes/archive/2026-11-05-audit-grow-architecture-across-versions/reviews/client-ui.md)；静态风险，先慢handler/生产者负载实验或证明现有外部上限，不预设正常CLI失效阈值 |
| MD-1 · 性能债务 | Markdown开放fence输出复用边界 | 同报告，逐帧clone已知residual；先分别测parse和copy成本与帧预算，再决定最小表示变化 |
| PB-01 · P2 · 已关闭session被重放 | CLI stdio forward→replay cache覆盖close事件 | [构建与产品报告](changes/archive/2026-11-05-audit-grow-architecture-across-versions/reviews/product-build.md)，真实入口过滤漏掉helper的close分支；验收走forward→断线→replay，不能只测helper |
| PB-02 · P2 · 并发new响应身份串配 | CLI replay state按RPC id管理pending创建 | 同报告，单Option覆盖与任意sessionId take静态确认；顺/逆序响应、error/load交织与重连对照 |
| PB-03 · P2 · PTY生命周期未接线 | ptyctl child/session/server统一stop/timeout/linger结算 | 同报告，shutdown receiver丢弃/timeout未消费；真实CLI隔离对照再修，Pager harness独立owner不归本项 |
| PB-04 · P2 · exact runner错误确认清理 | regression runner已拥有后代树的回收/未确认语义 | 同报告，真实_run_phase超时后setsid+DEVNULL自建后代仍活跃；普通后代/持pipe/不持pipe三组，不能按全局名字杀进程 |
| PB-05 · P2 · crash共享inode丢证据 | crash capture每进程文件身份与恢复claim | 同报告，显式开启capture才适用；双/三进程隔离序列验证，不能随另一进程启动撤销live owner证据 |
| PB-06 · P2 · 旧IP跨启动符号化 | crash格式/解析保存module identity和相对地址或明确限制 | 同报告，ASLR静态证据；隔离PIE已知函数跨slide恢复，保留raw离线证据，未声称当前全平台实测 |
| PB-07 · P2 · crash归档失败删raw | crash恢复提交owner在durable report成功前保留raw | 同报告，写错被忽略后unlink；故障注入报告/history路径，返回可观察错误且原证据可重试 |
| MCP-F1 · P1 · 初始化后发送到退休服务 | MCP call owner在异步初始化后复核同client/disabled/上游资格 | [工具权限报告](changes/archive/2026-11-05-audit-grow-architecture-across-versions/reviews/tools-authorization-recovery.md)，静态完整发送链；旁支有未合入修正，不能整支替换当前no-replay；可控握手barrier下旧/新tools/call均0 |
| MCP-F2 · P2 · SDK reverse缺owner路由 | SDK registration→invoker→leader保留注册连接身份 | 同报告，producer仅serverId/message，leader丢不可路由包；A注册/B驱动及同名serverId实际producer夹具，owner断线不回退driver |
| MCP-F3 · P2 · SDK重名静默首项 | client-MCP ingress联合校验canonical server name | 同报告，违反整组拒绝契约；new/load/update副作用前拒绝、旧catalog保持，标准array与SDK metadata均验证 |
| Vendor文档 · 升级债务 | dependency文档同步sqlite-vec maintained语义patch与nix-ohos实际graph | [产品报告](changes/archive/2026-11-05-audit-grow-architecture-across-versions/reviews/product-build.md)，不能只因上游补齐include就删本地边界修复；独立文档小修，移除前重跑native boundaries |
| Agent实体/announcements依赖 · 待用途核对 | Agent owner确认ToolExecConfig/Retry与preset public seam消费者；announcements直接使用config根 | [配置报告](changes/archive/2026-11-05-audit-grow-architecture-across-versions/reviews/agent-config.md)，无仓内消费不足以证明公开API可删；先确认受支持外部产品需求，再做窄删除/依赖收缩，不预建框架 |

| 交付释放/迟到结果 · 源码已修，现场待新构建恢复 | tool-types LocalPlan历史review绑定与Shell canonical结果提交 | [独立修复与实验](changes/archive/2026-11-05-audit-released-delivery-finalization/verification.md)：原公共API反例已在新生产库转为Ok，planning26/26、独立review和默认CLI check/build通过。只放宽历史Released(None)，其他review/canonical/终态门槛保持。当前旧宿主仍Receipt Unknown、link未terminal，需正常退出后用新项目构建恢复，再核对真实结果与replan；不能将源码验证当作现场闭合 |

## 明确暂缓

- **child 结束后的 SessionEnd Hook 因果写入（待独立修复）**：运行时模型控制的隔离真实 child 测试在 canonical Result 关闭 Timeline 后，日志出现 `session-end hook lifecycle was not durable: child Timeline is closed by its subagent result fact`。当前 model/provider/权限/竞态回归通过，未据此放宽封闭 Timeline 或顺带改收尾链路；后续应独立核对 SessionEnd Hook owner 与 Result 的持久顺序，覆盖有无Hook、正常完成/取消与冷恢复。触发和验证限制见[本次验证记录](changes/archive/2026-11-05-control-running-subagent-model-effort/verification.md#shell-verified-candidate)。

- **普通长 prompt 导出文件寿命（待独立链路复现）**：`run_turn_via_sampler` 局部的 PromptBlobExport 在采样返回时删除 TempDir，而模型的 read_file 发生在后续工具阶段；需独立复现普通 prompt 的采样→全文读取，再确定既有 artifact owner 的寿命边界。当前现场走 steer，未使用该导出路径，不并入插话保真修复。证据和范围见[长输入调查](changes/archive/2026-11-03-preserve-long-steering-input/design.md#separate-audit-finding)。

- **共享布局以外的字素边界（待对应复现）**：本次 [字素布局修复](changes/archive/2026-11-03-fix-tui-grapheme-layout/design.md) 限于普通文本共享内置换行、截断和搜索列映射。`ratatui-inline/src/segment.rs` 的 ANSI 历史行拆分仍按 scalar 计宽，`pager-render/src/render/terminal_output.rs::TermSink::put` 的 ANSI cursor model 仍按字符推进；跨样式 span 的字素归属也需先与 Ratatui 的逐 span 绘制语义统一。若出现对应 Minimal resize、带 cursor movement 的中文工具输出或跨 span 组合符号复现，再分别建立 change 验证各自所有者，不能将本次字体回退截图当成这些路径的复现。

- **跨路由 reasoning 后续边界**：[第一阶段审查](changes/archive/2026-10-29-improve-cross-route-reasoning-compatibility/audit.md) 发现通用 native-bearing HTTP 400 仍会触发一次 reset，以及可见 reasoning 仅保存 text、未区分摘要和完整正文。本阶段修复错误事实保真和目标请求投影；若真实目标要求更强语义，应独立处理精确 continuation 拒绝分类或最小 provenance，不预建全局 capability registry。

- **Remote Control / Agent Host 跨设备使用（暂缓、未排期）**：保留方向，不实施、不设日期；未来启动时另建 change，明确身份、权限、执行 owner、断线和恢复边界。不得据此引入 Dock、Memory 或远端控制框架。决策见 [上游主题评审](changes/archive/2026-10-18-record-upstream-review-2bdd1d6/review.md)。

- **Provider 缓存策略与 native epoch（待真实路由证据）**：[理论 review](changes/archive/2026-09-28-evaluate-provider-cache-affinity/results.md) 仅证明本地 key/breakpoint/retention 机制。先取得具体 provider/model/deployment、字段透传和真实负载证据，再做该 route 的最小优化，不预建 registry。

- **Recap 与主请求缓存（待真实共享前缀及收益证据）**：recap 保持独立 key、tool-free Sideband 契约；不为假设中的缓存复用添加主请求 tools。先测真实收益。

- **跨多个合法选区连续摘要（契约选择，未启动）**：当前一次有界摘要后仍超窗会保留已提交事实并终止回合；继续压缩更多选区需另立契约，不增加无界重试。

- **MCP 多轮 elicitation（现有独立提案）**：宿主 SDK 接点仍未通过既定门槛，保留 [提案与任务](changes/add-mcp-multi-round-elicitation/tasks.md)，不把未实现功能列为本轮缺陷。

- **ProcessScope supervisor 与 leader reap 的身份同步（待受控 PID/PGID 复用证据）**：terminal 已在最终组信号后、reap 前撤销自身强引用；全局 `kill_all` 若更早 upgrade Weak，仍有独立的跨线程收尾交错。先在可控制 PID 复用的隔离环境验证，不凭假设引入全局 owner/锁协议。范围和当前证明边界见[命令组审查](changes/archive/2026-11-04-finish-command-groups-before-leader-reap/review.md)。
