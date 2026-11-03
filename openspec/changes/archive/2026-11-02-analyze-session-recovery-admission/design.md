# Session 恢复准入：保留执行不变量，移除环境全等限制

推荐先修规则来源和失败重试，再拆掉聚合 hash 的一票否决。AGENTS.md 编辑不应使原任务无法恢复；颜色、目录路径的等价拼写也不应成为执行资格。恢复仍须证明：谁拥有 writer、继续哪个任务、哪些副作用已经发生、下一步能获得什么权限。

本文件是方案，不是已实现行为。当前代码与已归档规范仍会严格比较 source hash。

## 已有证据的边界

- 主 session `01a19a05-5281-7201-bae3-89f599512353` 的两次失败只由 projectRules 漂移引起。另一个 chat 已在明确授权下修复最新 checkpoint 来源绑定；用户随后自行恢复成功，持久记录了 RestorePrepared、Resumed、ExecutionReleased，并开始 inference。本轮不能再用旧备份覆盖历史，也不需要重复修数据。
- fork `01a19ba9-e3b1-7b71-a385-8f4cb89da44a` 的两次 incomplete-barrier 失败，早于稍后写入的 CommandsSettled、Checkpoint、root-only TreeSuspended。来源候选也与当前来源精确匹配，export 成功、没有已观察到的 writer lease 占用。这支持重新尝试，不等同于已经跑通该 fork 的执行型恢复。
- 因此不能把早先 incomplete 报错直接认定为错误校验或缓存 bug。真正需要保证的是：事实稍后补齐后，同进程重试也能重核对，用户无需靠重启、手改 hash 或理解内部 barrier 恢复可用性。

证据来自 [本机恢复记录](../2026-11-02-fix-suspended-project-rules-resume/verification.md) 和 [用户旅程审计](../2026-11-02-audit-fork-multi-session-journey/audit.md)；以上链接在归档后指向同级 change。本轮没有读取或操作真实 session 文件。

## 当前机制与问题

`SessionActor::runtime_source_hash` 把 projectRules、hook registry、client hooks、workspace 字符串、完整序列化 AgentDefinition、toolConfig、authoredCapabilityTools、selector/plugin、role、maxTurns、taskOutputBudget、model/endpoint/backend/effort 放进一个 JSON digest。`prepare_runtime_restore` 用一次字符串全等决定能否继续。

这个集合没有按用途分开：

1. AgentDefinition 的 `color` 明确只是展示元数据，也被整体序列化；description 是目录/提示文案，不是执行授权。
2. toolConfig 和 authoredCapabilityTools 被显式补入，但 `session_tools_allowlist/denylist` 等字段被 manifest 跳过序列化，hash 没有直接包含它们。实际 toolset 可能已体现一部分限制，不能据此推断存在绕过；同样不能用 hash 相等代替有效权限检查。
3. ProjectInstructions 已在 Timeline，但 hash 和 classifier 从新建 Agent 的磁盘规则缓存取值。恢复路径不替换已有历史规则，造成“模型读旧规则，classifier 读新规则”。
4. workspace 使用路径字符串；相同目录的 macOS `/tmp`、`/private/tmp` 别名也可能不相等。
5. modelImageInputKey 原本服务图片能力路由缓存，含实际 model/backend/endpoint/query 指纹。它可以辅助路由识别，却不是完整执行资格；API key 没有进入该指纹，凭据轮换本来就不是这个 hash 的拒绝原因。
6. maxTurns/输出预算用全等处理，不能表达“保留已消费计数、采用更紧上限”；新增宽松上限也不能因此重置原预算。

整体 hash 也不能告诉用户哪个依赖变了。把错误文字改详细只能改善诊断，解决不了正常规则编辑被拒绝。

## 校验裁决

| 对象 | 应保留的约束 | 应删除或降级的限制 | 最小落点 |
| --- | --- | --- | --- |
| writer / actor incarnation | 独占 lease、lifecycle gate、旧 writer drain；同 operation/instance 恢复 | 页面存在、进程退出或超时不能代替 owner 结算 | MvpAgent lifecycle 与 load |
| Timeline / 引用 / checkpoint | checksum、序列因果、原 Turn/Step/call/input identity、精确结果/交付引用 | 不为正常恢复重建第二套事实；不重新提交已完成工作 | ChatState、history、runtime_handoff |
| 挂起树 | CommandsSettled、每个未结束节点的合法 checkpoint、最终 barrier；已释放资格不可重用 | 一次 incomplete 错误不能永久阻断后续有效事实；不要求所有事实必须在第一次 load 前就齐备 | complete_partial_tree_barrier、retry |
| AGENTS.md 正文 | 模型、classifier 和 source 的有效规则一致；外部权限上限继续生效 | 删除“当前磁盘正文必须等于挂起时正文”这个准入条件 | spawn、prompt_build、runtime_source |
| 目录/隔离 | 实际原 workspace、隔离边界、folder trust 与当前访问权 | 删除同一目录的路径拼写全等；Git HEAD/工作文件正常改变不是拒绝理由 | session identity / cwd 构造入口 |
| Agent selector/plugin | 真实限定身份；禁止同名跨 plugin 冒充 | 不以 color、description 等描述变化阻断续接 | AgentDefinition 的执行投影 |
| role / completion / Workflow source | 原任务有效语义可重建；未知副作用不能自动换实现重跑 | 已持久化的 role 不必和今天重新渲染的同名配置全等 | Control、Workflow 冻结来源 |
| tool / MCP / child capability | 原 child 出生上限 ∩ 当前父/全局约束；当前调用逐次授权、transport incarnation | 有效权限收紧不应一律阻断整树；新配置更宽不能给旧任务扩权 | AgentRebuildSpec、delegation authorization |
| Hook | 已触发 occurrence 的 handler plan、完成/中断/未知事实；不重复已运行 Hook | 不以整份 registry 或 Debug 文本全等约束无关未来 Hook | actor/hooks、HookEvent.handlers |
| 模型路线 | 保留已持久化实际路线，不静默换 endpoint/backend/model；旧 native lane 不重建 | 同实际路线的 catalog alias 不应阻断；effort 可在后续合法 Step 控制边界更新 | model route snapshot / control |
| 限额 | 原消费、deadline/attempt 事实与 child grant 保留 | 删除纯数值全等；更紧上限在下一次派发准入生效，不能补发余额 | sampler recovery budget、task grant |
| fork 来源 | fork 自己的独立执行身份；真正 Spawn/Seed/Result 仍受保护 | 删除 summary 来源 parent ⇒ subagent 结果依赖的推断 | JSONL delete dependency lookup |
| 删除规模 | writer 退休、真实跨 session/Plan/挂起引用不被破坏 | 删除 16 MiB 观察预算作为显式删除的硬上限；不为删历史验证无关 provider body 全文 | delete_opened_session 与依赖核对 |

不是所有“当前文件不同”都属于授权变化。也不能用 RWX 字符串相同证明 MCP 成员集合、operator clamp 和 Hook 执行源相同。各类判断须直接使用对应权威来源。

## S1：最小修复，本次规则漂移不再阻断原任务

### 选择：先沿用持久规则恢复

恢复原任务默认使用挂起 frontier 对应分支上已生效的 typed `ProjectInstructions`。磁盘新规则可以用于新 session，既有任务先按自己的历史继续。它不新授予权限：当前 sandbox、folder trust、全局禁用与逐次工具授权仍适用。

落点：

1. 在 `actor/prompt_build.rs` 增加一个小型纯函数，从已验证 Timeline/当前有效分支解析规则投影，复用 `is_project_instructions`。不按 `<system-reminder>` 外形猜来源，不在磁盘重读失败后猜规则；首选现有 projector 已给出的有效 typed 项。
2. `actor/spawn.rs` 在设置 classifier 前取得该投影；classifier 和 `runtime_source_hash` 都使用同一有效投影。模型继续消费相同的 Timeline 规则。可以把现有两个调用点改为共用解析函数，先不增加持久 snapshot、规则 registry 或异步更新服务。
3. 没有历史规则时保持“本轮没有项目规则”的语义；不能因为现在新建了 AGENTS.md 就在旧 Turn 恢复前偷偷加进来。若原配置禁止 AGENTS.md，也保持该选择。多个有效规则块按现有模型投影次序处理，不能随手取物理日志最后一项；rewind、compaction、fork 继承都需核对实际分支。
4. 保留原 checkpoint，不写新 hash、不伪造恢复事实。本次已证实把 projectRules 换回原持久来源即可匹配旧 hash，因此该修复可以直接解决同类历史，不需要先设计 hash 版本兼容层。
5. 规则选择完成后再确认新的 actor/load 可用；classifier 的共享句柄仅由原本拥有 permission manager 的 root 设置。child 不覆盖父级 classifier slot。

这一步不能只改 `runtime_source.rs`。否则 hash 虽然匹配，classifier 仍可能使用新规则。Agent 重建后的全部规则消费者也必须保持相同投影，不能在后续 Agent/Behavior 切换时悄悄变回磁盘规则。

### 为什么不默认“恢复时立即接受新 AGENTS.md”

这也可以成为产品行为，但需要在一个已确认的控制边界把新规则同时交给模型与 classifier。仅恢复失败后提示“继续”不能作为权限放宽或替换已接纳工具参数的授权。若用户明确更新规则，应复用现有 Timeline/Control 的变更边界，确认写入后更新两端；已经接纳的响应和工具不回滚、不重跑。

首个 change 无须增加专用 `/reload-rules`、确认弹窗或开关。先解决沿用原规则恢复；显式规则更新能力如有需求再单独定义。不可用“规则变化可能有风险”继续保留默认全树拒绝。

## S2：移除聚合全等，按执行依赖决定是否可继续

长期目标是取消 `prepare_runtime_restore` 的“重建当前整份配置 → digest 全等”的总门槛。可以保留 digest 用于诊断或校验真正被冻结的执行源，但不能继续把无关变更混入一票否决。

最小边界不是新建通用 RecoverySpec 系统，而是复用已有事实：System head / ProjectInstructions / Control role、模型 route baseline、Spawn execution recipe 与出生 RWX、Hook handler plan、Workflow 的冻结定义、已有权限 actor。

实施时有一个真实缺口必须承认：普通 SessionControlSnapshot 目前主要保存 `agent_name/behavior/goal`，不等于完整的有效工具授权来源；child recipe 也不是完整 AgentDefinition。不能声称这些数据已经足够重建全部出生工具/MCP 上限。Workflow 已有 `WorkflowAgentDefinitionSnapshot::capture/restore`，包含 manifest、resolved tool config、authored tools 和 operator clamps；需要冻结普通 Agent 的执行语义时，应先评估抽取/复用它的必要部分，附着在既有 Agent Control/Spawn 事实，不能再造独立磁盘账本或每个 checkpoint 重复保存全部配置。

Backlog 已有“挂起 child 出生 MCP 成员集合持久化”专项。这里没有以 source hash 的相等性证明该问题不存在，也不把“RWX 相同”当作完整 capability 不变。S2 必须把这项缺口纳入授权测试；它不阻碍只调整规则来源的 S1。

具体拆分：

- **可直接排除的元数据**：color、与执行身份无关的说明文字、配置文件位置的无关变化。selector 仍是正式身份；description 若进入未来 catalog，只改变后续展示/选择信息，不改原已接纳任务。不得顺手修改整个 AgentDefinition 的业务相等函数，只收窄恢复使用的投影。
- **可从 ledger 恢复的内容**：原规则、已持久 role、模型路线、原 Turn/Step。直接恢复原投影，无须拿它和当前文件再次全等。
- **允许收紧的策略**：当前禁用的工具、权限 ceiling、额度更紧时，恢复历史和身份后在下一次调用/采样处按有效约束拒绝或等待用户处理；不能先运行一部分工具再发现原出生上限缺失。若连原 ceiling 都无法证明，则必须在整树执行放行前停住该处。
- **不能直接放松的执行源**：尚未完成的 Workflow 程序、待执行 Hook handler、原 child 出生能力和插件正式身份。缺失可靠原来源时，返回具体缺项；不能只因为想提高可用性而用当前同名定义替代。对于没有在途依赖的 registry 更新，采用当前正常配置更新边界，无须比较整份 registry。
- **模型控制**：同真实 backend/model/endpoint/query 的 alias 调整不应产生变更；effort 更新沿 Step 控制边界。更换真实数据目的地或模型不是同任务无声恢复，需走正常显式模型控制路径。不要通过规范化 URL 文本猜两个 endpoint 等价。
- **workspace**：用现有文件系统能力证明同一个目录；统一创建/恢复的身份入口。不要添加 `/private` 字符串替换、模糊 path 归一化或“找个同名目录”回退。

S1 可以先独立发布。S2 中“去元数据”和“出生授权/执行源重建”可以分成小 change，不能为删一个颜色检查同时实现配置热更新框架。新的持久字段只为目前没有权威来源且确实需要恢复的执行事实服务，不落凭据正文，不增加双重版本/兼容分支。

## S3：失败 load 可在同进程重试，保留原 owner

当前 `load_session` 每次都会 `load_light`；不是完全不读磁盘。但若已有 open resident，它会重用 actor，`restore_session_tree` 又读取该 actor 的 ChatState runtime。第一次恢复失败发生在 actor 注册之后，错误返回没有统一回收恢复临时 actor；其 Agent 规则等重建来源也随 actor 保留。所以需要修的是失败事务的所有权和重建边界，不能简单“每次忽略缓存”。

按提交进度处理：

| 失败位置 | 下一次显式 Retry 的行为 |
| --- | --- |
| 旧 writer / 挂起 worker 尚未退休，事实在途 | 继续观察同 owner 和 operation；报告可重试状态。不启动第二个 writer，不另发一次挂起，不固定轮询重试墙。 |
| root/child 已完整 checkpoint，仍为 Suspended，尚无 RestorePrepared | 在 lifecycle guard 下只退休这次失败 load 创建、尚未发布可执行性的 actor，再 drain/reload 权威历史和当前可用依赖。已有 `RetireSuspendedRuntime` 可复用，但必须满足它的 phase/operation/idle 条件。 |
| 已写 RestorePrepared，尚无节点 ExecutionReleased | 沿同 operation/instance 核对既有准备，不能生成新 instance 重新开始。现有 retirement 仅接受 Suspended，不能直接拿它清 Restoring；该分支先复用已准备 owner。 |
| 某些节点已 ExecutionReleased | 复用已接纳执行，按既有 partial-release reconciliation 处理；绝不能把新运行内容当旧 checkpoint 重放。 |
| 只读 attach / 已正常执行的其他 viewer | 保留原 live actor，失败不能把别人的 owner 一并删除。 |

`complete_partial_tree_barrier` 已能在完整节点证据和 lease 下补 TreeSuspended，继续复用。不存在合法 checkpoint 时仍拒绝执行，但用户可以读历史、看到“等待保存完成/来源缺失”的具体原因；保存后来完成时，下一次 Retry 重新核对。

Pager 首次 LoadSessionFailed 已经会 unbind 并移除 load placeholder、回到 origin。这条正确路径保留；对失败 fork placeholder 另按 A8 收尾。不要把问题概括成“失败后全部 AgentView 都没清理”。重试只需现有 load/Retry 入口及可操作错误，错误的内部原因可放 ACP error.data，用户界面不展示 hash。

## 与多 session 旅程的独立修复顺序

| 顺序 | change 边界 / 最小落点 | 验收目的 |
| --- | --- | --- |
| 1 | S1：规则投影同源；`prompt_build/spawn/runtime_source` | 正常编辑 AGENTS.md 后可继续原任务，不再手改 session。 |
| 2 | S3：失败 load 重试事务；`acp_agent/runtime_handoff/session_lifecycle` | barrier 补齐或依赖恢复后，同进程 Retry 可用；不丢 owner、不重跑。 |
| 3 | A6：close/quit 所有权；`session/modal/router` 与既有连接/runtime owner | 页面消失不丢失执行责任；所有本进程 owned root 在退出前确认挂起。 |
| 4 | A3/A4：delete 结果收敛；`session_admin/task_result` | 预知拒绝先检查；writer 必须在删目录前退休；后续失败或 Stay 成功也清理/标明失效本地绑定。 |
| 5 | A1：分开 fork 谱系与结果引用；`jsonl` dependency lookup | 普通 fork 可单独删除，真正 subagent 结果链保护保留。 |
| 6 | A2：显式删除依赖核对；`delete_opened_session` | 长历史不再受观察预算限制。按 canonical 依赖事实流式核对，不为此校验无关 provider body 全文；仍核对 writer 与跨 session/Plan/挂起引用。不要只调大常量。 |
| 7 | A5/A7/A8：停止的实际来源与页面收尾 | child/Goal/Workflow-only Working 可处理；最后一页落 Dashboard/Welcome；fork 失败正确回原页面。 |
| 8 | S2：逐轴替代聚合 hash，分小 change | 取消展示/环境全等；依赖重建和有效权限有直接证明。 |

G1 的挂起历史删除还需要一个明确处置语义：对于没有跨 session 结果依赖的挂起 root，用户确认永久删除可以同时明确放弃其恢复责任。应在原 lifecycle/Timeline 上记录最小必要终止事实，再回收，不能伪造 ExecutionReleased 或完成任务。若涉及关联树，需要以真实引用范围核对完整处置，不能从单条删除递归 rm 其他会话；这属于独立行为设计，不放进 S1。

R1 的 leader 多客户端退出需专项验证，退出 RPC 应由现有 client/runtime owner 解析自己的 root 集合，不能把 MvpAgent 所有 resident 都当当前退出者拥有。当前只有代码风险证据，不把未执行的双客户端结果写成复现。

## 开发前必须写入 delta 的目标行为

- 规则文件正常改变后，原挂起任务以持久有效规则恢复；模型和 classifier 同源。
- 展示元数据改变不影响恢复；权限收紧在有效执行准入生效，不增加原授权。
- 后到的完整挂起事实使同进程显式 Retry 可重新核对；错误不永久冻结旧失败快照。
- 失败 load、close、delete、Quit 的物理 actor 与持久逻辑工作分别正确结算。

这些是后续 change 的 WHEN/THEN 起点，不提前合入主规范，也不把本轮分析写成已完成实现。
