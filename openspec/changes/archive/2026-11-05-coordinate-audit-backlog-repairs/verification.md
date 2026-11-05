# Verification and accepted investigation

本change最终按用户停止新审查的指令收束为[开发里程碑](milestone.md)，并非全backlog闭合。下文保留历史调查及接受过程，最终以里程碑中逐项结果、最后收尾记录和独立change为准。未启动项全部保留backlog，不继续派发。

当前处于按统一架构逐项实施阶段，尚不能宣称backlog整体闭合。WS03、WS06b和WS06a已完成独立修复与里程碑归档；SAMPLER-1模块实现、定向运行和独立review已验收，跨产品入口里程碑仍待运行，其change和backlog保持开放。

父复核输入事实：Timeline::input_state已正确处理Dismissed(RouteSuperseded)，pending_allowed_inputs按Submitted顺序；Shell restore无条件Steer→FIFO且使用revision prompt id，existing_input_state平行fold漏supersedes，queue_input按当前foreground重新分类且replayed返回Removed。确认F1-F3当前仍成立；调查交付仅供设计，不是冷spawn/运行通过。拟议接口及reorder仍需精确定约后实施。

父复核插件事实：agent_ops本次session_registry与LSP process-snapshot分叉；apply_plugin_registry_snapshot先发布None但撤销plugin/Hook与mark generation置于Some内；Skills API请求cwd和shared registry，Pager丢session_id发送cwd:"."。同cwd双session反例成立。AC02/03a可沿现接口修，AC03b管理目录conditional语义和AC06采用ACK待定，尚不接受为实现。

父复核Sampler actor/state/commands和两个collect入口：重复RequestId替换token后old join无条件删当前登记，panic不删；两个CancelOnDrop按同RequestId发cancel。Tokio官方文档经context7确认JoinSet::join_next_with_id/JoinError::id、跟踪期间Id唯一，当前Cargo.lock Tokio1.53.1。计划同owner以task identity清理、每提交token绑定drop取消，public cancel(id)仍针对当前登记，无新通用身份框架。

三项首轮只读调查均在host以serialization missing created_at终态失败、无交付正文；父释放失败交付不作结论，通过原会话续跑取得有界静态结果，逐项亲查上述决定性源码。不把host故障归因当前仓库某版本或自动改正常存储。后续MCP共享接口建议仍待父核对实际祖先资格/SDK可信owner/source merge。

所有测试仅独立GROW_HOME和/tmp自建fixture，固定Rust/Cargo/rustdoc1.93.1。编译窗口串行：第7 WS06b已结束并释放，第8分配WS03在候选冻结后开始，其他coder未获窗口不启动Cargo。实现review与实际运行记录写各行为change，整体里程碑在本change汇总；未经验证不清空原backlog。

## 已验收的独立修复

WS-03 已归档为[工作树删除归属修复](../2026-11-05-bind-worktree-registration-cleanup/verification.md)。父先审普通删除候选及32项证据，独立review补出旧stale后置清理绕过，再把同一删除许可收回fast-worktree共同owner。修正后原32+新12共44个不同exact在最终hash全部通过，权限跳过0；独立旧helper副本的Path/Prefix/public rehydrate三项在预期误删/错误成功断言失败。父读回完整结果与关键原始日志、重算四文件hash并接受Luna复核。Workspace force/取消与Shell失败清理链静态核对成立，实际复合公共API和public rehydrate已运行；Shell pristine gate、Windows及真实Linux/btrfs未运行，数据动作整体里程碑仍有其他未闭合项。

WS-06b 已归档为[成功后提交hunk投影](../2026-11-05-commit-hunk-projections-after-success/verification.md)。父核对完整diff和最终SHA-256、读回原始日志17个不同exact tests通过，独立luna未发现生产blocker。all公共入口确定性覆盖先成功2文件再失败及重试；turn覆盖实际失败/重试与分组，部分成功提交共享同一batch owner。turn专属测试不保证每次至少一文件先成功，此限制明确保留，没有为测试引入无产品意义的排序。WS06a现可接同一actions/tests写范围，数据完整性里程碑尚待stale错误集成，不以一项成功关闭全部backlog。

MCP-D1 已归档为[迟到错误与no-replay一致性修正](../2026-11-05-align-late-mcp-error-with-no-replay/verification.md)：父检查发送函数及现有late-error回归断言，主规范消除自动重试矛盾，docs链接同一权威条款。无业务代码变化、未运行Cargo，不把静态审阅当作本轮MCP运行通过；MCP-F1/F2/F3仍待修。

## 后续模块与里程碑记录

SAMPLER-1已归档为[任务寿命修复记录](../2026-11-05-bind-sampler-request-task-lifetime/verification.md)。以tracked task ID精确清理、每次提交token绑定collect Drop，保留原公开RequestId取消与旧任务结算。父审源码与14新公共Handle用例、5份日志65次执行/64唯一全部通过、最终指纹；独立Luna未发现运行时问题，唯一过时注释已独立修复并核纯文档diff。CLI取消/重试先发现独立Shell终态遗漏，保持原失败断言拆项修复后，新增默认CLI的同session原生exact已通过，五个HTTP/settlement/evidence身份一致；模块、产品链和backlog均已闭合。

WS-06a已归档为[外部编辑保护](../2026-11-05-reject-stale-hunk-targets/verification.md)。single/filebatch共用实际字节/存在性检查，其他scope自然复用；137项actor回归通过，20组非root权限故障保持精确Read/Write/Delete/restore语义和失败投影，唯一ignored是原性能复现。父读全日志、重算三文件hash，独立Luna无可行动问题。后续三个集成测试走实际Shell extension→WorkspaceOps→真实actor：四scope冲突均success:false且不报affectedCount、全投影保持，匹配正常反转/countonce，creation/deletion重建冲突保护成立；首次运行全通过。父已读完整test/diff/日志并对齐最终指纹，06a/06b合成里程碑验收，WS06a移出backlog。未覆盖ACP传输层、Windows权限或任意writer跨syscall原子CAS，未把这些限制隐去。

## Final milestone closure

WS-01已归档为[索引最终删除](../2026-11-05-preserve-code-index-removals/verification.md)：原公共Handle Modified→Removed有意义red，完全相同测试green，最终57项lib全部通过；原在途Luna review无可行动问题，父亲读全部diff/日志和五个保护hash接受。按路径最终Removed交给现remove_file清三类事实，COW及无关文件保持；未宣称OS watcher交付已实测。

Shell取消终态已归档为[预算退役修复](../2026-11-05-retire-cancelled-turn-sampling-state/verification.md)：真正terminal ACK后、可选Hook前释放actor continuation，terminal失败及非Turn stop保持。4项owner/8项恢复预算/1项挂起exact通过，原native CLI exact在新默认产物通过，Completed[1,2,1]及五次HTTP/settlement/evidence相合。父独立解析日志/hash接受，未新增review，遵用户停止点；夹具故障和零测试选错均保留且不计通过。默认CLI SHA256 aecdaab85a17e957c0348f4644ed1c86a86f5ae120cbb400f589f53109d827e6仅为本地milestone产物，后续2.6.1版本维护将生成新身份。

索引和Sampler沿原计划独立review，取消终态只有父验收，二者明确区分。当前已在途实现和验收全部结束，未启动项、旧host child01a1a77a的未知finalization/replan、Windows/Linux/PTY现场和进程树清理限制保留。用户明确授权2.6.1候选及发布，后续步骤只在[发布change](../../prepare-release-v2-6-1/proposal.md)执行，不扩大本架构修复范围。归档收尾实际结果：Sampler/取消两项归档后strict18/18、archive862/862；本架构里程碑归档后strict17/17、archive863/863，全为exit0。本轮相关文档及开发入口404条相对路径核对0缺失（不代表fragment全正确），证据留存`/tmp/grow-261-milestone-hhv4x1ka/results.json`。版本候选及正式发布继续独立执行。
