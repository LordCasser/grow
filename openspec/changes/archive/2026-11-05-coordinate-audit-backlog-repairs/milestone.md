# 2026-11-05 开发里程碑

用户停止点：“停止新审查，当前作为一个里程碑，记录。然后准备收尾，以发布新版本。”从此不再派发新审查、未启动修复或扩展调查。本次归档记录本里程碑的已接受结果，原全backlog路线保留为后续输入，不宣称全部实现。发布准备见[2.6.1 change](../../prepare-release-v2-6-1/proposal.md)。

| 成果 | 当前接受事实与证据 |
| --- | --- |
| 普通resume的model/effort覆盖 | 来源最后实际选择、单字段覆盖、派生Spawn/child baseline及源事实不变已验收，[记录](../2026-11-05-allow-session-model-effort-control/verification.md)。 |
| 运行中的直属child模型控制 | 原生set_subagent_model以绑定parent和精确child runtime转发，现Step边界应用、queued仅为接纳，能力上限不变，[记录](../2026-11-05-control-running-subagent-model-effort/verification.md)。 |
| TODO绑定与父子控制 | mandatory Link ACK、todo_read、request-only上下文、typed edit/stop与正常收尾/结果/review已验收；真实defaultCLI两条exact均通过，[记录](../2026-11-05-bind-subagent-lifecycle-to-parent-todo/verification.md)。 |
| 压缩和Windows历史读取 | 未消费Skill事实保留、Windows独立目录读取共享边界均按各自证据验收；Windows现场限制保留，[压缩](../2026-11-05-preserve-unconsumed-skills-during-compaction/verification.md)、[Windows](../2026-11-05-fix-windows-child-history-sharing/verification.md)。 |
| 工作树删除归属 | compound公共API及stale registration同owner修复，44个unique定向通过、独立review接受，[记录](../2026-11-05-bind-worktree-registration-cleanup/verification.md)。 |
| Hunk磁盘与投影一致性 | 成功后提交投影、过期磁盘冲突拒绝、Shell→WorkspaceOps→actor真实链已接受；不承诺跨syscall CAS，[记录](../2026-11-05-reject-stale-hunk-targets/verification.md)。 |
| 索引最终删除 | 原公共Handle有意义red、同test green及最终57个unique lib全部通过，最终源426ad5aa，原在途Luna窄review无可行动问题；接受公共Handle→actor→索引链，[记录](../2026-11-05-preserve-code-index-removals/verification.md)。 |
| Sampler任务归属 | tracked task ID与每提交token修复，64个unique定向测试及独立review接受；结合Shell修复后的同session取消/重试产品链已通过，[记录](../2026-11-05-bind-sampler-request-task-lifetime/verification.md)。 |
| 取消Turn的采样预算退役 | 用户停止时已在途；最终局部4、恢复预算8、挂起exact1及新默认CLI原生exact1全部通过，原[1,2,1]/五attempt身份断言保持，父验收未新增独立review，[记录](../2026-11-05-retire-cancelled-turn-sampling-state/verification.md)。 |
| 迟到交付和load后恢复 | 原代码修复和隔离验证已归档；旧live host finalization未真实终态，原unknown-session首因未完整证实，不能将新源码替代现场证据，[交付](../2026-11-05-audit-released-delivery-finalization/verification.md)、[恢复](../2026-11-05-diagnose-lost-primary-session-owner/verification.md)。 |

原始全仓版本审计是覆盖及发现记录，不是所有问题已修或每个平台已运行的证明。剩余WS02/04/05、输入F1–F5、配置/插件/Skills/MCP、CLI replay/PTY/exact runner/crash、child SessionEnd Hook及容量/性能/用途待核实项全部保留[backlog](../../../backlog.md)。本次有界只读准备（例如ClientFS同遍stream、输入canonical查询和插件snapshot候选）不计完成，也不继续实施。

仍有效的现场/发布边界：测试全部隔离，正常Grow存储、安装、重启、真实provider/MCP/hooks未操作；旧host child01a1a77a仍未有可信finalization receipt。源码/默认debugCLI的验证不等于release-dist签名包、跨平台CI或正式GitHub Release。当前所有预存dirty保留，未提交Git、创建tag、push或发布。用户随后明确指定“按2.6.1准备候选和发布”，该版本的必要提交/tag/push及官方发布已获授权，仍需按既有门禁逐步执行。全部在途修复与验收已闭合；取消终态最终4+8+1项局部检查及新默认CLI exact通过，Sampler/取消两项归档后全量strict18/18、archive862/862。旧live host未知责任继续保留。本架构里程碑归档后strict17/17、archive863/863均通过；相关404条相对路径0缺失。发布提交与十平台门禁结果归独立发布change，不抹去失败。
