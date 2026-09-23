## Context

本 change 来自 `grow 2.1.11 (3024dad)` 的真实恢复失败。诊断基于 session `01a0c7c4-70eb-7890-8d17-ddfbde5061cc` 的持久化事实和 unified log；源码核对以同一 HEAD 上的当前工作副本为准，保留任务开始前已有的未提交改动。

相关子 Agent：

| subagent id | parent Spawned | parent Ended | child SubagentResult | outcome |
| --- | ---: | ---: | ---: | --- |
| `01a0c8ce-b7e4-79e2-a0a0-bfa350139872` | 23477 | 24482 | 1702 | `cancelled` |
| `01a0c8d2-d4d5-7381-a20c-252b06828f79` | 23880 | 24479 | 1129 | `cancelled` |

两条 child Timeline 均保留匹配 parent timeline/spawn sequence 的 `SubagentSeed`，父 terminal 的 `result_ref` 指向 child 中唯一的 `SubagentResult`，身份、outcome、duration、tool calls、turns、tokens 和 error 一致。当前 `grow export` 可完整校验父、子实体。resume 在取消约六分钟后发起，不是紧邻清理窗口的即时重试。

同一事故的取消日志记录了以下持久化失败：

```text
attempt persistence failed: attempt usage ledger: timeline event violates the causal fold:
child Timeline is closed by its subagent result fact
```

这证明至少一个已准入 attempt 的结算晚于 `SubagentResult`。它不能单独证明 resume resolver 当时失败的具体子检查；该信息已被现有 `Option` 接口抹去。

## Current Flow and Failure

### Cancellation and terminal publication

1. coordinator 的 active-child cancel 同时触发 cancellation token，并通过 `ShellChildRuntime::cancel` 向 child 发送 `Cancel` 和 `Shutdown`。
2. `await_subagent_turn_or_cancellation` 对 cancellation token 与 child prompt result 做 `select`。token 获胜时立即返回合成的 `Cancelled`，不再等待 child prompt terminal。
3. `handle_request` 读取当时可见的 usage，随后调用 `record_child_result`。
4. `Timeline::validate_header` 在 child 出现首个 `SubagentResult` 后拒绝所有后续事件；该事实是 child Timeline 的不可逆关闭边界。
5. 尚在收尾的 sampler/turn actor 如果随后提交 attempt evidence 或 usage settlement，会被 causal fold 拒绝。
6. parent `Ended` 再引用该 child result。表面上父子 link 可以完整，但关闭点之前的 attempt frontier 不一定完整。

问题不在 `SubagentOutcome::Cancelled` 的数据模型；Timeline 明确允许 `Completed`、`Failed` 和 `Cancelled`，并校验 failed/cancelled 具有 error。缺口是取消 token 被当成了“已取消请求的通知”和“child 已完成持久化收尾的确认”两个不同概念。

### Resume source resolution

`durable_resume_source_for` 当前按以下顺序读取并校验：

1. 打开并校验 parent session Timeline；
2. 查找匹配 subagent id 的 `Spawned` 和 `Ended`；
3. 验证 lifecycle root 与 security parent；
4. 打开 child session，验证 summary lineage/kind；
5. 校验 child Timeline；
6. 通过 `Timeline::validate_subagent_result_link` 验证 seed、result reference 和 terminal payload。

该函数没有 `outcome == Completed` 门槛，但所有步骤都经 `.ok()?` 或 `None` 返回。调用方只在 resolver 返回空后额外查询 live coordinator；若来源不再 active，所有其他原因都会变成 `no completed canonical lifecycle was found`。因此当前错误文案既丢失真正原因，也错误暗示 cancelled outcome 本身不合法。

此外，调用方只在 durable 解析失败后检查 live ownership。如果磁盘 terminal 已经提交、原 child runtime 却仍在 sampler/persistence 收尾，resolver 会先成功并允许新 resume epoch 与旧 runtime 重叠。事故中的晚到 attempt 正好证明“存在 terminal fact”目前不能等价于“live owner 已退出”。

### Resume failure matrix

下表覆盖有效 `resume_from` 从请求准入到新 child 首个 prompt admission 的失败面。处理方式分为三类：修复实现缺陷、保留合法拒绝但使其可诊断、或保持通用 spawn 失败语义。失败不得静默降级为 fresh spawn。

| Phase | Condition | Current behavior | Decision |
| --- | --- | --- | --- |
| Request policy | agent definition removed/unknown, type disabled, validation unavailable, depth/Goal ownership invalid | 在 source 读取前按普通 Task gate 拒绝 | 保留；这是当前 policy/ownership，不绕过以复活历史 agent |
| Live ownership | source 仍在 pending/active，但 durable link 已可见 | durable 成功时跳过 active 检查，可并行续接 | 修复：live active/settling 优先，要求稍后重试 |
| Parent source | parent storage missing/unreadable、Timeline invalid、spawn missing、terminal missing | 全部折叠为 generic missing completed lifecycle | 修复：typed cause；无 terminal 且 live owner 存在时报告 settling |
| Security | requester 不是 lifecycle root 或 recorded security parent | generic lifecycle failure | 保留拒绝；对外合并为不可用，内部保留 security category |
| Child source | child missing/unreadable、summary id/parent/kind mismatch、Timeline invalid | generic lifecycle failure | 保留 fail closed；改为 typed storage/identity/validation cause |
| Canonical link | seed missing/mismatch、result ref missing/out-of-range、terminal/result payload conflict | generic lifecycle failure | 保留 fail closed；改为 typed canonical-link cause |
| Outcome | exact linked result 为 `cancelled` | 数据模型允许，但事故中被 generic `completed` 文案误判 | 明确允许；outcome 不代替 canonical validation |
| Agent identity | requested subagent type 与 source type 不同 | 精确拒绝 | 保留；resume 不能换 agent identity |
| Non-worktree cwd | source cwd 已删除 | 前置 canonicalize 将其误报为 workspace 越界；后续安全回退不可达 | 修复：仅“路径不存在”回退 parent cwd；存在但越界/非目录/不可验证仍拒绝 |
| Isolation | 当前请求/definition 要求 isolation，但 source 没有 worktree | 精确拒绝 | 保留；不能把原非隔离 workspace 伪装成隔离续接 |
| Worktree | source worktree 消失且无 snapshot | 精确拒绝 | 保留；workspace state 不可恢复 |
| Worktree | snapshot 存在但 ref/仓库/目标 materialization 失败或路径不一致 | parent spawn 后以 failed terminal 结束 | 保留；原 source 与 snapshot 不改写，修复环境后可重试 |
| Model catalog | source model 已下架 | 精确拒绝 | 保留；不得静默换模型 |
| Reasoning effort | source effort 不再被该 model 支持 | 精确拒绝 | 保留；不得猜测替代 effort |
| Transport | backend/base URL/query transport key 与 source 不同 | 精确拒绝 | 保留；不得把另一 transport 冒充原 route |
| Source context | Timeline 无法 materialize、Surface 为空、缺 source stable System head | 精确拒绝；另有当前 System head 渲染失败的伪依赖 | 保留 source 校验；修复为 resume 不渲染当前 head，不让当前模板故障阻断已验证历史 |
| Context budget | source transcript 超过 source model 当前 window 的 80% safety bound | 精确拒绝 | 保留；本 change 不做隐式 compaction 或丢历史 |
| Prompt artifacts | inherited blob ref 非法、source directory 不可用、artifact 缺失/损坏 | bootstrap abort | 保留；错误纳入 resume context category |
| Completion output artifact | source 的最终输出 artifact 被删除 | 不参与 context materialization | 明确保持可恢复；resume authority 来自 Timeline Surface 和 exact result link，不依赖展示 artifact |
| Workflow route | Workflow Run 或冻结 route 已不存在，agent definition 无法从 snapshot 解析 | 精确拒绝 | 保留；workflow-owned resume 仍受原 Run authority 管理，不回退到当前全局 definition |
| Parent admission | 新 resume epoch 的 parent Spawned 无法持久化 | 失败且不创建 workspace/child | 保留；原 source 可重试 |
| Child admission | rehydrate、new child persistence、session spawn、catalog convergence 失败 | 新 epoch 以失败或未完成 canonical chain 收尾 | 保留；验证原 source 仍可重试且不复用半成品 child |
| Promotion | cancel 在 pending→active promotion 竞争中获胜 | 新 child cancelled，resumed worktree 被保留 | 保留；不得删除 source-owned workspace |
| First prompt | child control publication、Goal snapshot mailbox、prompt mailbox 或 durable user-message ACK 失败 | 当前部分只 warning/忽略，或退化成 `Child session dropped unexpectedly` | 修复：全部 fail closed，等待已准入 prompt 结算后关闭 derived epoch；原 source 仍可重试 |

Sentinel `resume_from`（空白、`null`、`none`、`undefined`）在 Task 工具输入边界被定义为“参数缺失”，不是有效 resume 请求；该既有兼容行为不扩展到任何真实 id。多个显式并发 resume 是否构成分支语义不是本事故的失败原因，本 change 不新增 source lease；隔离 worktree 的并发所有权另行审计，不混入本次修复。

Atlas 对 `durable_resume_source_for` 的局部 Focus 证据确认上述调用关系；incoming caller 结果受 Focus closure 限制，调用入口同时由源码中的 `handle_request` 直接核对。

## Goals / Non-Goals

**Goals:**

- 让 cancellation request 与 canonical terminal acknowledgment 成为两个明确阶段。
- 保证 `SubagentResult` 之前不存在仍可能向 child Timeline 提交的已准入 sampling attempt 结算。
- 让完整、严格校验的 cancelled lifecycle 与 completed lifecycle 一样可作为 resume source。
- 让每个 resume 拒绝保留内部 typed cause，并映射为准确、可操作且不越过安全边界的错误。
- 防止 live source 与 derived resume epoch 重叠，并让安全的 missing-cwd 情况可以继续。
- 保证 route/workspace/context 或 derived-epoch 启动失败不会损伤原 source 的再次恢复资格。
- 使用确定性交错测试覆盖事故路径，而不是依赖真实 provider 额度或时间竞争。

**Non-Goals:**

- 不改变 root Goal 因 billing、预算或 incomplete usage 停止的规则，也不自动重启 Goal。
- 不自动 resume 被取消的 child；resume 仍由明确的后续 Task 请求触发。
- 不把 fresh spawn 伪装成原 child 的 continuation，也不迁移或猜测缺失的历史事实。
- 不改变 completed/failed source 的既有结果语义、模型固定规则、worktree rehydrate、权限 ceiling 或安全 lineage。
- 不处理 subagent UI、通知文案以外的展示、通用 session resume 性能或其他 architecture backlog。
- 不自动替换已下架 model/effort/transport，不隐式 compact 超窗 transcript，不为缺失 worktree/snapshot 构造空 workspace。
- 不在本 change 定义同一 source 的多分支并发 resume 或隔离 worktree lease；没有事故证据支持把它和 terminal barrier 合并实现。

## Decisions

### 1. Child prompt terminal is the cancellation settlement gate

外层 cancellation token 只发起取消，不能直接授权 terminal publication。token 触发后，runner SHALL 继续等待现有 child prompt result/terminal acknowledgment；正常 child turn 路径负责在返回前完成其已准入 attempt 的 evidence 与适用 usage settlement。取消结果应优先使用实际 `PromptCompletionKind::Cancelled` 及其 category/context，不再在 token 分支立即合成 canonical result。

若 child actor 在返回 prompt terminal 前关闭，runner SHALL 进入现有 session lifecycle drain/错误路径，确认 sampler owner、event drainer 和 persistence frontier。只有该 frontier 得到确认，才能从已验证 child Timeline 构造 cancelled terminal；确认失败时不得写入会关闭 Timeline 的 `SubagentResult`，也不得把该 source 宣称为可恢复。

该设计复用 child turn、sampler shutdown 和 session persistence 的现有确认边界，不新增 persisted cancellation marker 或第二套 child 状态机。

**Alternative: token 到达后延迟固定时长再写 result。** 拒绝。时间不能证明 attempt 已结算，慢 provider 或 I/O 会重现相同竞态。

**Alternative: 允许 `SubagentResult` 后继续追加 attempt events。** 拒绝。它会破坏 child terminal 的不可变 frontier，使 result 的 usage/诊断随尾部事件失真，并削弱现有 causal fold。

### 2. `SubagentResult` remains the single irreversible child close

`SubagentResult` 仍是 child Timeline 的唯一关闭事实。写入前必须满足：

- child 已停止新的 provider admission；
- 已准入 provider work 已结束或取消；
- attempt evidence 与所有适用 usage settlement 已确认，或已按现有契约持久记录 unknown/incomplete；
- child prompt/turn 已有终态，不再存在可合法追加的 turn/tool/request 事实；
- result payload 中的 usage、turn/tool counts 和 error 来自该稳定 frontier。

写入 child result 成功后，parent `Ended` 才可引用其 exact `result_ref`。任何 barrier、child result 或 parent terminal 持久化失败都保持 fail closed；成功通知和 waiter 结果不能越过该链。

### 3. Resume eligibility is based on canonical linkage, not success outcome

本 change 不增加 outcome 白名单。对本次缺陷，`Cancelled` source 满足以下条件时 SHALL 可恢复：

- parent 包含唯一匹配的 `Spawned` 和 `Ended(cancelled)`；
- child summary 指向同一 lifecycle root，并属于 subagent session kind；
- child `SubagentSeed` 与 parent spawn 精确匹配；
- terminal `result_ref` 指向 child 的 exact `SubagentResult(cancelled)`；
- terminal/result 的 identity、outcome、duration、counts、usage 和 error 全部一致；
- requester 通过现有 security-parent 和 workspace confinement 检查。

resume 继续固定 source 的 agent/model/transport/reasoning effort、上下文和 worktree 语义；调用方不能借 resume 覆盖 source model。该规则只确认 continuation source 的真实性，不把 cancelled 工作标记为成功。

### 4. Resume resolution returns a typed failure

将 `durable_resume_source_for -> Option<DurableResumeSource>` 改为结构化 `Result`。内部至少保留以下类别：

- parent storage missing/load/Timeline validation；
- lifecycle missing 或只有 spawn、尚无 terminal；
- security parent mismatch；
- child storage missing/load；
- child summary identity/kind mismatch；
- child Timeline validation；
- missing/invalid seed 或 result link。

live coordinator 的 active 判断仍是瞬态事实，不能覆盖 durable validation。调用入口应把 durable `lifecycle incomplete` 与 live active 组合成 `still running`；其余错误保留原类别。安全不匹配对未授权调用方只返回统一的不可用结果，日志可记录无敏感 payload 的内部类别，不能泄露其他 session 是否存在。

用户可见错误不得再使用 `completed canonical lifecycle` 概括所有失败。建议类别为：仍在运行、canonical terminal 尚未完成、source session 无法读取/校验、canonical result link 无效、source 不可用于当前安全父级。

### 5. Live ownership wins over durable eligibility

resume admission SHALL 在启动任何 source materialization、worktree 或新 child side effect 前查询同一 coordinator 的 source ownership。只要 source id 仍在 pending/active 且属于请求者的即时安全父级，就返回 `still running or settling`。即使磁盘上已经存在 exact result link，也不能绕过该检查。

检查后 source 可能刚好完成；拒绝是安全的瞬态结果，调用方可重试。反方向不会发生：已退出 source 不会以同一 subagent id 重新进入 active map。durable facts 仍是最终恢复 authority，completed cache 或 UI status 不参与资格判断。

### 6. Missing non-worktree cwd has a narrow safe fallback

对没有 worktree 的 source，cwd 按以下顺序处理：

- 路径存在、是目录、canonicalize 后位于当前 parent workspace 内：继承该 cwd；
- 路径明确不存在：记录 warning，并使用当前 parent workspace；
- 路径存在但不是目录、canonicalize 失败或位于 parent 外：拒绝。

这一规则使 `resume_inherited_cwd` 的既有 fallback 真正可达，同时不把权限错误、symlink escape 或其他无法证明的路径当成“不存在”。worktree source 不走该 fallback；其目录与 snapshot 决定 reuse/rehydrate/reject。

### 7. Post-resolution failures leave the source retryable

model/effort/transport、worktree/snapshot、context/artifact 和新 child admission 失败只终止本次 derived epoch。它们 SHALL NOT 修改 source parent terminal、source child result、snapshot ref 或 source transcript。若本次请求已提交新的 parent Spawned，则沿现有规则提交对应失败 terminal 或保留可恢复的 open spawn；不得把 derived child 的失败写回原 source。

错误文本必须标明阶段和 source id。可修复环境问题（model catalog、cwd、snapshot/storage availability）处理后，使用同一 source 重试应重新执行全部 canonical validation，而不是缓存先前失败或绕过安全检查。

### 8. Resume uses historical context authority only

`Resumed` 与 verbatim mirror-fork 已经从 source Surface 继承并校验稳定 System head，因此不得再调用当前版本的 child-audience head renderer。当前 renderer 只服务 `New` 与 normalized `Forked` context；它的模板、插件或配置错误不能否定既有 source 的历史真实性。

恢复上下文依赖 Timeline materialized Surface 及其直接引用的 immutable prompt blobs。completion output artifact 只是终态展示/诊断载体，不参与新 child 的 prompt materialization；只要 terminal/result link 与 source Surface 完整，缺失旧 output artifact 不阻断 resume。反之，Surface 实际引用的 blob 缺失仍 fail closed。

Workflow-owned resume 继续要求同一 live Workflow Run 的冻结 runtime route；Run/route 不存在时拒绝，不回退到当前全局 agent definition。此 gate 是 ownership/route authority，不是 source canonicality。

### 9. First prompt has an explicit admission boundary

新 child promotion 后，启动顺序为：发布 control snapshot 并收到 mailbox ACK；若为 Goal-owned child，则通过同一 FIFO mailbox 安装 immutable Goal snapshot；提交 `QueuePrompt` 并等待其 `persist_ack`，确认 user-message Timeline fact 已 durable commit；之后才把 turn 视为已准入。

control ACK、Goal snapshot send、QueuePrompt send 或 `persist_ack` 任一步失败，均设置明确的 derived launch error。若 prompt 已可能准入但 ACK 丢失，先发送 cancellation 并等待真实 prompt terminal，随后才写 child result；不得用 launch error 越过 attempt settlement barrier。命令 channel 的 FIFO 保证 Goal snapshot 在 QueuePrompt 前应用，因此不新增第二个 snapshot acknowledgment 或持久化事件。

### 10. Regression uses controlled acknowledgments

新增测试用可控 sampler/persistence acknowledgment 建立以下顺序：

1. root Goal 持有一个使用不同模型配置的 active child；
2. child 最后一个 attempt 已准入，测试阻塞其 evidence 或 usage settlement ACK；
3. root Goal 的停止路径取消 child；
4. 断言 child `SubagentResult` 与 parent `Ended` 均未提交，resume 不会错误开启新 child epoch；
5. 释放 ACK，断言 attempt settlement 先提交，随后 child result 和 parent terminal 形成 exact link；
6. 对同一 cancelled child 发起 resume，断言 source 被接受并固定原 child route；
7. 分别破坏 summary、seed、result ref 和 child Timeline，断言返回不同 typed cause 且不启动 child。
8. 构造 durable link 已存在但 coordinator 仍 active，断言不会启动重叠 epoch；移除 live owner 后同一 source 可恢复。
9. 删除合法的非 worktree source cwd，断言回退 parent workspace；existing file、越界目录和不可 canonicalize 路径仍拒绝。
10. 对 model/effort/transport、worktree/snapshot、oversize context、artifact 和 derived child admission 失败逐类断言错误阶段，随后确认原 source facts 未变化且可重新解析。
11. 让当前 child System head renderer 失败，断言 validated resume 仍使用 inherited head；new/normalized child 仍拒绝无 head。
12. 分别关闭 control、Goal snapshot、QueuePrompt 和 `persist_ack` 边界，断言 provider 不被错误继续启动、derived epoch 正确收尾、原 source 可重试。

测试不调用真实 provider，不依赖 sleep 竞争，不需要伪造 402 文本；Goal 停止只用于覆盖事故中的实际 cancellation owner 路径。

## Risks / Trade-offs

- **取消完成会等待真实 settlement，而不是立即返回。** 这是保证 usage/evidence 不丢失的必要延迟；沿用现有有界 session shutdown deadline，超时作为失败边界，不伪装成功。
- **等待 prompt terminal 可能暴露此前被即时 token 分支遮蔽的 actor 错误。** typed error 应保留该事实；不能回退为合成 cancelled result。
- **更具体的 resume 错误可能泄露 session 存在性。** 对未通过 security-parent 检查的请求继续合并为不可用；只对已授权 lineage 暴露 storage/link 类别。
- **已有不完整 lifecycle 仍不可恢复。** 不做启发式修复；完整且已验证的历史无需迁移。

## Migration Plan

无需存储迁移或 schema 版本变化。修复后的 resolver 直接读取现有 parent/child canonical facts。实现完成后更新相关开发说明，运行定向回归和严格 OpenSpec 校验，再归档 change；回滚到旧 binary 不受支持。
